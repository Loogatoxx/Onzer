//! Commandes de collection : playlists, favoris, paroles.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::State;

use crate::core::{OnzerError, Result};
use crate::db::playlists::{self, PlaylistSummary};
use crate::db::repository::TrackSummary;
use crate::identify::lrclib::LrcLibClient;
use crate::library::importer;
use crate::library::lyrics::{self, Lyrics};
use crate::AppState;

// ── Playlists ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_playlist(state: State<'_, AppState>, name: String) -> Result<i64> {
    playlists::create(&state.pool, &name).await
}

#[tauri::command]
pub async fn list_playlists(state: State<'_, AppState>) -> Result<Vec<PlaylistSummary>> {
    playlists::list(&state.pool).await
}

#[tauri::command]
pub async fn rename_playlist(
    state: State<'_, AppState>,
    playlist_id: i64,
    name: String,
) -> Result<()> {
    playlists::rename(&state.pool, playlist_id, &name).await
}

#[tauri::command]
pub async fn delete_playlist(state: State<'_, AppState>, playlist_id: i64) -> Result<()> {
    playlists::delete(&state.pool, playlist_id).await
}

#[tauri::command]
pub async fn add_to_playlist(
    state: State<'_, AppState>,
    playlist_id: i64,
    track_ids: Vec<i64>,
) -> Result<i64> {
    playlists::add_tracks(&state.pool, playlist_id, &track_ids).await
}

#[tauri::command]
pub async fn remove_from_playlist(
    state: State<'_, AppState>,
    playlist_id: i64,
    position: i64,
) -> Result<()> {
    playlists::remove_at(&state.pool, playlist_id, position).await
}

#[tauri::command]
pub async fn playlist_tracks(
    state: State<'_, AppState>,
    playlist_id: i64,
) -> Result<Vec<TrackSummary>> {
    playlists::tracks(&state.pool, playlist_id).await
}

// ── Favoris ─────────────────────────────────────────────────────────────────

/// Bascule le statut de favori et retourne le nouvel état.
#[tauri::command]
pub async fn toggle_loved(state: State<'_, AppState>, track_id: i64) -> Result<bool> {
    playlists::toggle_loved(&state.pool, track_id).await
}

#[tauri::command]
pub async fn loved_tracks(state: State<'_, AppState>) -> Result<Vec<TrackSummary>> {
    playlists::loved(&state.pool).await
}

// ── Paroles ─────────────────────────────────────────────────────────────────

/// Paroles d'un morceau, analysées.
///
/// L'analyse est faite ici et non côté interface : reconnaître le format LRC
/// est de la logique métier, et elle est déjà éprouvée par ses tests.
///
/// # Pourquoi le fichier est relu quand la base est muette
///
/// La colonne `lyrics` n'a été ajoutée qu'après coup : tous les morceaux
/// importés avant elle ont une case vide, définitivement. S'en tenir à la base
/// revenait à répondre « pas de paroles » pour toute une bibliothèque, y
/// compris pour des fichiers qui en portaient. Le fichier fait foi ; la base
/// n'est qu'un cache, qu'on renseigne au passage.
#[tauri::command]
pub async fn track_lyrics(state: State<'_, AppState>, track_id: i64) -> Result<Lyrics> {
    let row: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT lyrics, relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&state.pool)
            .await?;

    let Some((cached, relative_path)) = row else {
        return Ok(Lyrics::default());
    };

    if let Some(raw) = cached.filter(|text| !text.trim().is_empty()) {
        return Ok(lyrics::parse(&raw));
    }

    // Rien en base : le fichier a peut-être quelque chose à dire.
    let paths = state.paths.read().await.clone();
    let Ok(path) = importer::absolute_path(&paths, &relative_path) else {
        return Ok(Lyrics::default());
    };
    if !path.is_file() {
        return Ok(Lyrics::default());
    }

    let found = tokio::task::spawn_blocking(move || lyrics::read_from_file(&path))
        .await
        .map_err(|error| OnzerError::Invalid(error.to_string()))?
        .unwrap_or(None);

    let Some(raw) = found else {
        return Ok(Lyrics::default());
    };

    // On garnit le cache pour ne pas relire le fichier à chaque affichage.
    let _ = sqlx::query("UPDATE tracks SET lyrics = ? WHERE id = ?")
        .bind(&raw)
        .bind(track_id)
        .execute(&state.pool)
        .await;

    Ok(lyrics::parse(&raw))
}

/// Enregistre des paroles saisies ou collées à la main.
///
/// **Écrites dans le fichier**, pas seulement en base : elles suivront le
/// morceau si l'utilisateur l'ouvre ailleurs, et survivront à une
/// réinstallation. L'écriture disque échoue si le SSD est débranché — dans ce
/// cas on ne met pas non plus la base à jour, pour ne pas laisser croire que
/// les paroles sont enregistrées alors qu'elles ne le sont qu'à moitié.
#[tauri::command]
pub async fn set_track_lyrics(
    state: State<'_, AppState>,
    track_id: i64,
    raw: String,
) -> Result<Lyrics> {
    let trimmed = raw.trim().to_string();

    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&state.pool)
            .await?;

    if let Some(relative_path) = relative_path {
        let paths = state.paths.read().await.clone();
        let path = importer::absolute_path(&paths, &relative_path)?;

        if path.is_file() {
            let text = trimmed.clone();
            tokio::task::spawn_blocking(move || lyrics::write_to_file(&path, &text))
                .await
                .map_err(|error| OnzerError::Invalid(error.to_string()))??;
        }
    }

    sqlx::query("UPDATE tracks SET lyrics = ? WHERE id = ?")
        .bind((!trimmed.is_empty()).then_some(trimmed.as_str()))
        .bind(track_id)
        .execute(&state.pool)
        .await
        .map_err(OnzerError::Database)?;

    Ok(lyrics::parse(&trimmed))
}

// ── Paroles en ligne ────────────────────────────────────────────────────────

/// Client LRCLIB partagé.
///
/// Un seul pour toute l'application : le limiteur de cadence vit à l'intérieur,
/// et en créer un par appel reviendrait à n'en avoir aucun.
static LRCLIB: OnceLock<Option<LrcLibClient>> = OnceLock::new();

fn lrclib() -> Result<&'static LrcLibClient> {
    LRCLIB
        .get_or_init(|| LrcLibClient::new().ok())
        .as_ref()
        .ok_or_else(|| OnzerError::Invalid("client de paroles indisponible".to_string()))
}

/// Avancement de la récupération des paroles.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsProgress {
    pub with_lyrics: i64,
    pub total: i64,
    /// Vrai tant qu'une récupération en lot tourne.
    pub running: bool,
}

/// Vrai tant qu'un lot est en cours. Empêche d'en lancer deux.
static BATCH_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn lyrics_progress(state: State<'_, AppState>) -> Result<LyricsProgress> {
    let (with_lyrics, total): (i64, i64) = sqlx::query_as(
        "SELECT SUM(CASE WHEN lyrics IS NOT NULL AND lyrics <> '' THEN 1 ELSE 0 END),
                COUNT(*)
           FROM tracks WHERE deleted_at IS NULL",
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(LyricsProgress {
        with_lyrics,
        total,
        running: BATCH_RUNNING.load(Ordering::Relaxed),
    })
}

/// Cherche les paroles d'un morceau sur LRCLIB.
///
/// **Sur action explicite uniquement.** Onzer reste un lecteur hors ligne : rien
/// ne part sur le réseau sans que l'utilisateur l'ait demandé. Ce qui est envoyé
/// se limite à l'artiste, au titre, à l'album et à la durée.
#[tauri::command]
pub async fn fetch_lyrics(state: State<'_, AppState>, track_id: i64) -> Result<Lyrics> {
    let query = lyrics_query(&state.pool, track_id).await?;
    let Some(query) = query else {
        return Ok(Lyrics::default());
    };

    let Some(found) = lrclib()?.fetch(&query).await? else {
        return Ok(Lyrics::default());
    };

    // Écrites dans le fichier : une fois trouvées, elles ne dépendent plus du
    // réseau, et suivent le morceau si on l'ouvre ailleurs.
    persist_lyrics(&state, track_id, &found.raw).await?;

    Ok(lyrics::parse(&found.raw))
}

/// Récupère en tâche de fond les paroles de tous les morceaux qui n'en ont pas.
///
/// Retourne aussitôt : la cadence courtoise d'une requête par seconde rendrait
/// l'attente insupportable sur une bibliothèque entière. L'avancement se lit
/// avec `lyrics_progress`.
#[tauri::command]
pub async fn fetch_missing_lyrics(state: State<'_, AppState>) -> Result<i64> {
    if BATCH_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(OnzerError::Invalid(
            "une récupération est déjà en cours".to_string(),
        ));
    }

    let pending: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tracks
          WHERE (lyrics IS NULL OR lyrics = '') AND deleted_at IS NULL
          ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;

    let pool = state.pool.clone();
    let paths = std::sync::Arc::clone(&state.paths);
    let total = pending.len() as i64;

    tauri::async_runtime::spawn(async move {
        for track_id in pending {
            let Ok(Some(query)) = lyrics_query(&pool, track_id).await else {
                continue;
            };

            let found = match lrclib() {
                Ok(client) => client.fetch(&query).await,
                Err(_) => break,
            };

            match found {
                Ok(Some(found)) => {
                    let resolver = paths.read().await.clone();
                    if let Err(error) = write_lyrics(&pool, &resolver, track_id, &found.raw).await {
                        tracing::warn!(track_id, %error, "paroles non enregistrées");
                    }
                }
                // Absentes de la base : c'est fréquent et normal.
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(track_id, %error, "LRCLIB indisponible, lot interrompu");
                    break;
                }
            }
        }

        BATCH_RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(total)
}

/// Assemble la requête à partir de ce que la base sait du morceau.
///
/// `None` quand le titre manque : sans lui, la recherche n'a aucun sens.
async fn lyrics_query(
    pool: &sqlx::SqlitePool,
    track_id: i64,
) -> Result<Option<crate::identify::lrclib::LyricsQuery>> {
    let row: Option<(String, Option<String>, Option<String>, i64)> = sqlx::query_as(
        "SELECT t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1),
                (SELECT al.title FROM albums al WHERE al.id = t.album_id),
                t.duration_ms
           FROM tracks t WHERE t.id = ?",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await?;

    let Some((title, artist, album, duration_ms)) = row else {
        return Ok(None);
    };

    // LRCLIB apparie sur le couple artiste + titre : sans artiste, la réponse
    // serait au mieux un homonyme.
    let Some(artist) = artist.filter(|name| !name.trim().is_empty()) else {
        return Ok(None);
    };
    if title.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(crate::identify::lrclib::LyricsQuery {
        title,
        artist,
        album,
        duration_ms,
    }))
}

/// Écrit des paroles dans le fichier puis en base, depuis une commande.
async fn persist_lyrics(state: &State<'_, AppState>, track_id: i64, raw: &str) -> Result<()> {
    let paths = state.paths.read().await.clone();
    write_lyrics(&state.pool, &paths, track_id, raw).await
}

/// Écrit des paroles dans le fichier puis en base.
///
/// L'ordre compte : si le disque refuse, la base ne doit pas prétendre le
/// contraire.
async fn write_lyrics(
    pool: &sqlx::SqlitePool,
    paths: &crate::core::PathResolver,
    track_id: i64,
    raw: &str,
) -> Result<()> {
    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(pool)
            .await?;

    if let Some(relative_path) = relative_path {
        if let Ok(path) = importer::absolute_path(paths, &relative_path) {
            if path.is_file() {
                let text = raw.to_string();
                tokio::task::spawn_blocking(move || lyrics::write_to_file(&path, &text))
                    .await
                    .map_err(|error| OnzerError::Invalid(error.to_string()))??;
            }
        }
    }

    sqlx::query("UPDATE tracks SET lyrics = ? WHERE id = ?")
        .bind(raw)
        .bind(track_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ── Identifications douteuses ───────────────────────────────────────────────

/// Un morceau dont l'identification contredit ce que le fichier annonçait.
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SuspectTrack {
    pub id: i64,
    /// Ce qu'Onzer a écrit.
    pub title: String,
    pub artist: Option<String>,
    /// Ce que le fichier annonçait avant identification.
    pub original_title: String,
    pub original_artist: Option<String>,
}

/// Morceaux dont l'identification est à vérifier.
///
/// # Pourquoi c'est proposé et non corrigé d'office
///
/// Une identification qui contredit les tags du fichier a souvent **raison** :
/// c'est même sa raison d'être, corriger un fichier mal nommé au
/// téléchargement. Mais elle a parfois tort, et un fichier de Damso s'est
/// retrouvé étiqueté « carmen » de Stromae. Onzer ne peut pas trancher à la
/// place de l'utilisateur ; il peut lui montrer les deux versions et le laisser
/// choisir.
#[tauri::command]
pub async fn suspect_tracks(state: State<'_, AppState>) -> Result<Vec<SuspectTrack>> {
    let rows = sqlx::query_as::<_, SuspectTrack>(
        "SELECT t.id, t.title, t.original_title, t.original_artist,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist
           FROM tracks t
          WHERE t.original_title IS NOT NULL
            AND t.identification_state = 'done'
            AND t.deleted_at IS NULL
          ORDER BY t.id",
    )
    .fetch_all(&state.pool)
    .await?;

    // Le filtrage se fait en Rust : la comparaison normalisée est déjà écrite,
    // éprouvée par ses tests, et la refaire en SQL la dédoublerait.
    Ok(rows.into_iter().filter(disagrees).collect())
}

/// Les deux versions désignent-elles des morceaux différents ?
fn disagrees(row: &SuspectTrack) -> bool {
    use crate::identify::verdict::{assess, CandidateEvidence, FileEvidence};

    // On réutilise le juge d'identification plutôt qu'une comparaison ad hoc :
    // deux règles de concordance qui divergeraient finiraient par se
    // contredire à l'écran.
    !assess(
        &FileEvidence {
            title: Some(row.original_title.clone()),
            artist: row.original_artist.clone(),
            duration_ms: 0,
        },
        &CandidateEvidence {
            title: row.title.clone(),
            artist: row.artist.clone(),
            length_ms: None,
            release_count: 1,
            // Volontairement sous le seuil de contradiction : on demande « ces
            // deux descriptions concordent-elles ? », pas « faut-il croire
            // l'empreinte ? ».
            score: 0.0,
        },
    )
    .is_accepted()
}

/// Rétablit les tags que le fichier portait avant identification.
///
/// Écrit dans le fichier **et** en base, et repasse le morceau en `rejected` :
/// sans cela, l'ouvrier d'identification le reprendrait et réappliquerait la
/// correspondance que l'utilisateur vient justement de refuser.
#[tauri::command]
pub async fn restore_original_tags(state: State<'_, AppState>, track_id: i64) -> Result<()> {
    /// Titre, artiste et album d'origine, plus le chemin du fichier.
    type OriginalTags = (Option<String>, Option<String>, Option<String>, String);

    let row: Option<OriginalTags> = sqlx::query_as(
        "SELECT original_title, original_artist, original_album, relative_path
           FROM tracks WHERE id = ?",
    )
    .bind(track_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some((title, artist, album, relative_path)) = row else {
        return Err(OnzerError::Invalid("morceau introuvable".to_string()));
    };

    let Some(title) = title.filter(|value| !value.trim().is_empty()) else {
        return Err(OnzerError::Invalid(
            "aucun tag d'origine mémorisé pour ce morceau".to_string(),
        ));
    };

    let paths = state.paths.read().await.clone();
    let path = importer::absolute_path(&paths, &relative_path)?;

    if path.is_file() {
        let (titre, artiste, album) = (title.clone(), artist.clone(), album.clone());
        tokio::task::spawn_blocking(move || {
            crate::library::metadata::rewrite_identity(&path, &titre, artiste.as_deref(), album.as_deref())
        })
        .await
        .map_err(|error| OnzerError::Invalid(error.to_string()))??;
    }

    crate::db::repository::restore_identity(
        &state.pool,
        track_id,
        &title,
        artist.as_deref(),
        album.as_deref(),
    )
    .await
}
