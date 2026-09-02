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
    /// Morceaux dont les paroles portent des horodatages.
    ///
    /// Distinguer les deux n'est pas un détail d'affichage : une bibliothèque
    /// téléchargée par deemix porte **toutes** ses paroles et **aucune**
    /// synchronisation. Ne compter que « avec paroles » faisait déclarer le
    /// travail terminé sur 1378 morceaux qui n'avaient jamais été regardés.
    pub with_synced: i64,
    pub total: i64,
    /// Vrai tant qu'une récupération en lot tourne.
    pub running: bool,
}

/// Vrai tant qu'un lot est en cours. Empêche d'en lancer deux.
static BATCH_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn lyrics_progress(state: State<'_, AppState>) -> Result<LyricsProgress> {
    let (with_lyrics, with_synced, total): (i64, i64, i64) = sqlx::query_as(
        "SELECT SUM(CASE WHEN lyrics IS NOT NULL AND lyrics <> '' THEN 1 ELSE 0 END),
                SUM(CASE WHEN lyrics LIKE ? THEN 1 ELSE 0 END),
                COUNT(*)
           FROM tracks WHERE deleted_at IS NULL",
    )
    .bind(lyrics::SYNCED_LIKE)
    .fetch_one(&state.pool)
    .await?;

    Ok(LyricsProgress {
        with_lyrics,
        with_synced,
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
    crate::commands::preferences::ensure_online_completion(&state.pool).await?;

    if BATCH_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(OnzerError::Invalid(
            "une récupération est déjà en cours".to_string(),
        ));
    }

    // Sont concernés les morceaux sans paroles **et** ceux dont les paroles
    // n'ont pas d'horodatage : les seconds sont invisibles pour qui ne
    // regarde que la présence du texte, et c'est précisément le cas de toute
    // une bibliothèque téléchargée par deemix.
    let pending: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tracks
          WHERE deleted_at IS NULL
            AND (lyrics IS NULL OR lyrics = '' OR lyrics NOT LIKE ?)
          ORDER BY id",
    )
    .bind(lyrics::SYNCED_LIKE)
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
                    // Un texte brut ne remplace pas un texte brut : le morceau
                    // en a déjà un, l'échanger contre un autre ne lui apporte
                    // rien et réécrirait son fichier pour rien.
                    if !found.synced && has_lyrics(&pool, track_id).await {
                        continue;
                    }

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
/// Le morceau porte-t-il déjà des paroles ?
async fn has_lyrics(pool: &sqlx::SqlitePool, track_id: i64) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM tracks
          WHERE id = ? AND lyrics IS NOT NULL AND lyrics <> ''",
    )
    .bind(track_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
        > 0
}

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

// ── Pochettes manquantes ────────────────────────────────────────────────────

/// Vrai tant qu'une récupération de pochettes tourne.
static ARTWORK_RUNNING: AtomicBool = AtomicBool::new(false);

/// Avancement de la récupération des pochettes.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkProgress {
    pub with_artwork: i64,
    pub total: i64,
    pub running: bool,
}

#[tauri::command]
pub async fn artwork_progress(state: State<'_, AppState>) -> Result<ArtworkProgress> {
    let (with_artwork, total): (i64, i64) = sqlx::query_as(
        "SELECT SUM(CASE WHEN al.artwork_hash IS NOT NULL AND al.artwork_hash <> ''
                         THEN 1 ELSE 0 END),
                COUNT(*)
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL",
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(ArtworkProgress {
        with_artwork,
        total,
        running: ARTWORK_RUNNING.load(Ordering::Relaxed),
    })
}

/// Va chercher les pochettes des morceaux qui n'en ont pas.
///
/// # Pourquoi cela ne passe pas par l'ouvrier d'identification
///
/// Celui-ci part de l'**empreinte acoustique**, et son verdict est définitif :
/// un morceau marqué `not_found` ou `rejected` ne sera jamais repris. Or une
/// pochette manquante n'a souvent rien à voir avec une empreinte introuvable —
/// le morceau peut être parfaitement identifié et son album simplement dépourvu
/// d'image dans la Cover Art Archive, ou n'avoir jamais eu d'album du tout.
///
/// Cette passe part donc du **texte** : artiste et titre, cherchés dans
/// MusicBrainz, puis la pochette de la parution retenue. Elle n'écrit que
/// l'image — jamais le titre ni l'artiste, qui ne sont pas ce qu'on répare ici.
#[tauri::command]
pub async fn fetch_missing_artwork(state: State<'_, AppState>) -> Result<i64> {
    crate::commands::preferences::ensure_online_completion(&state.pool).await?;

    if ARTWORK_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(OnzerError::Invalid(
            "une récupération est déjà en cours".to_string(),
        ));
    }

    let pending: Vec<(i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT t.id, t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1),
                t.duration_ms
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL
            AND (al.artwork_hash IS NULL OR al.artwork_hash = '')
          ORDER BY t.id",
    )
    .fetch_all(&state.pool)
    .await?;

    let pool = state.pool.clone();
    let paths = std::sync::Arc::clone(&state.paths);
    let total = pending.len() as i64;

    tauri::async_runtime::spawn(async move {
        let clients = (
            crate::identify::musicbrainz::MusicBrainzClient::new(),
            crate::identify::coverart::CoverArtClient::new(),
            crate::identify::deezer::DeezerClient::new(),
        );

        let (Ok(musicbrainz), Ok(cover_art), Ok(deezer)) = clients else {
            ARTWORK_RUNNING.store(false, Ordering::SeqCst);
            return;
        };

        for (track_id, title, artist, duration_ms) in pending {
            // ── D'abord MusicBrainz et la Cover Art Archive ──────────────
            //
            // Elles apportent aussi l'album et l'année, pas seulement l'image :
            // quand elles répondent, on en tire plus.
            let mut attached = false;

            if let Ok(hits) = musicbrainz.search(artist.as_deref(), &title).await {
                if let Some(hit) = hits.into_iter().next() {
                    if let Ok(Some(metadata)) = musicbrainz.lookup(&hit.recording_mbid).await {
                        let cover = cover_art
                            .fetch_front(
                                metadata.release_mbid.as_deref(),
                                metadata.release_group_mbid.as_deref(),
                            )
                            .await
                            .unwrap_or(None);

                        if let Some(bytes) = cover {
                            let resolver = paths.read().await.clone();
                            match attach_artwork(&pool, &resolver, track_id, &metadata, &bytes)
                                .await
                            {
                                Ok(()) => attached = true,
                                Err(error) => {
                                    tracing::warn!(track_id, %error, "pochette non attachée");
                                }
                            }
                        }
                    }
                }
            }

            if attached {
                continue;
            }

            // ── Puis Deezer ─────────────────────────────────────────────
            //
            // La Cover Art Archive est alimentée par des bénévoles : excellente
            // sur les catalogues anciens, lacunaire ailleurs. Le catalogue d'un
            // service commercial est complet par construction — mesuré sur huit
            // morceaux sans pochette, Deezer en a trouvé huit.
            let Ok(Some(bytes)) = deezer.cover(artist.as_deref(), &title, duration_ms).await
            else {
                continue;
            };

            let resolver = paths.read().await.clone();
            if let Err(error) = attach_cover_only(&pool, &resolver, track_id, &bytes).await {
                tracing::warn!(track_id, %error, "pochette Deezer non attachée");
            }
        }

        ARTWORK_RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(total)
}

/// Attache une pochette seule, sans métadonnées d'album.
///
/// Deezer ne nous apprend rien sur l'album qu'on voudrait écrire — et ce n'est
/// pas ce qu'on répare ici. Quand le morceau n'a pas d'album, l'image reste
/// donc sans support : mieux vaut cela qu'inventer un album pour la porter.
async fn attach_cover_only(
    pool: &sqlx::SqlitePool,
    paths: &crate::core::PathResolver,
    track_id: i64,
    bytes: &[u8],
) -> Result<()> {
    let hash = crate::library::artwork::store(&paths.artwork_dir(), bytes)?;

    crate::db::repository::attach_artwork(pool, track_id, &hash, None, None, None).await
}

/// Range une pochette et la rattache au morceau.
///
/// Quand le morceau n'a pas d'album, on en crée un depuis les métadonnées
/// trouvées : une pochette doit se rattacher à quelque chose, et l'album est ce
/// que l'interface affiche.
async fn attach_artwork(
    pool: &sqlx::SqlitePool,
    paths: &crate::core::PathResolver,
    track_id: i64,
    metadata: &crate::identify::musicbrainz::RecordingMetadata,
    bytes: &[u8],
) -> Result<()> {
    let hash = crate::library::artwork::store(&paths.artwork_dir(), bytes)?;

    crate::db::repository::attach_artwork(
        pool,
        track_id,
        &hash,
        metadata.album.as_deref(),
        metadata.filing_artist(),
        metadata.year,
    )
    .await
}

// ── Correction manuelle ─────────────────────────────────────────────────────

/// Corrige à la main le titre, l'artiste et l'album d'un morceau.
///
/// # Pourquoi cette porte est nécessaire
///
/// L'identification acoustique se trompe, et pas seulement sur des cas
/// tordus : un morceau nommé « Medecine » alors qu'il s'agit de « Ma go ». La
/// conséquence se propage — les paroles récupérées sont celles du mauvais
/// titre, la pochette aussi. Sans moyen de corriger, il faudrait sortir le
/// fichier de la bibliothèque et le réimporter.
///
/// # Ce que la correction emporte avec elle
///
/// Les **paroles sont effacées**. Elles avaient été trouvées pour l'ancien
/// titre : les garder laisserait le morceau afficher les paroles d'un autre,
/// ce qui est précisément le symptôme qu'on répare. Elles seront reprises au
/// prochain « Chercher en ligne », cette fois avec le bon nom.
///
/// L'état d'identification passe à `rejected` : sans cela, l'ouvrier
/// reprendrait le morceau et réappliquerait la correspondance qu'on vient de
/// corriger.
#[tauri::command]
pub async fn correct_track(
    state: State<'_, AppState>,
    track_id: i64,
    title: String,
    artist: Option<String>,
    album: Option<String>,
) -> Result<()> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(OnzerError::Invalid("le titre ne peut pas être vide".to_string()));
    }

    let artist = artist.map(|value| value.trim().to_string()).filter(|v| !v.is_empty());
    let album = album.map(|value| value.trim().to_string()).filter(|v| !v.is_empty());

    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&state.pool)
            .await?;

    // Le fichier d'abord : si le disque refuse, la base ne doit pas prétendre
    // le contraire.
    if let Some(relative_path) = relative_path {
        let paths = state.paths.read().await.clone();
        if let Ok(path) = importer::absolute_path(&paths, &relative_path) {
            if path.is_file() {
                let (titre, artiste, disque) = (title.clone(), artist.clone(), album.clone());
                tokio::task::spawn_blocking(move || {
                    crate::library::metadata::rewrite_identity(
                        &path,
                        &titre,
                        artiste.as_deref(),
                        disque.as_deref(),
                    )
                })
                .await
                .map_err(|error| OnzerError::Invalid(error.to_string()))??;
            }
        }
    }

    crate::db::repository::restore_identity(
        &state.pool,
        track_id,
        &title,
        artist.as_deref(),
        album.as_deref(),
    )
    .await?;

    // Les paroles appartenaient à l'ancien titre.
    sqlx::query("UPDATE tracks SET lyrics = NULL WHERE id = ?")
        .bind(track_id)
        .execute(&state.pool)
        .await?;

    Ok(())
}

// ── Personnalisation ────────────────────────────────────────────────────────

/// Donne une image à une playlist.
///
/// # Pourquoi copier le fichier
///
/// Pointer vers l'image d'origine ferait dépendre la playlist d'un fichier que
/// l'utilisateur peut déplacer, renommer ou jeter — et la pochette
/// disparaîtrait sans explication. L'image est donc rangée dans le cache
/// d'Onzer, à côté des autres pochettes, sous son empreinte.
#[tauri::command]
pub async fn set_playlist_cover(
    state: State<'_, AppState>,
    playlist_id: i64,
    source_path: String,
) -> Result<()> {
    let bytes = tokio::fs::read(&source_path)
        .await
        .map_err(|error| OnzerError::Invalid(format!("image illisible : {error}")))?;

    let paths = state.paths.read().await.clone();
    let hash = crate::library::artwork::store(&paths.artwork_dir(), &bytes)?;

    sqlx::query("UPDATE playlists SET cover_path = ?, updated_at = ? WHERE id = ?")
        .bind(&hash)
        .bind(crate::core::now_ms())
        .bind(playlist_id)
        .execute(&state.pool)
        .await?;

    Ok(())
}

/// Retire l'image choisie : la playlist revient à la mosaïque de ses morceaux.
#[tauri::command]
pub async fn clear_playlist_cover(state: State<'_, AppState>, playlist_id: i64) -> Result<()> {
    sqlx::query("UPDATE playlists SET cover_path = NULL, updated_at = ? WHERE id = ?")
        .bind(crate::core::now_ms())
        .bind(playlist_id)
        .execute(&state.pool)
        .await?;

    Ok(())
}

/// Décrit une playlist en une phrase.
#[tauri::command]
pub async fn set_playlist_description(
    state: State<'_, AppState>,
    playlist_id: i64,
    description: String,
) -> Result<()> {
    let description = description.trim();

    sqlx::query("UPDATE playlists SET description = ?, updated_at = ? WHERE id = ?")
        .bind((!description.is_empty()).then_some(description))
        .bind(crate::core::now_ms())
        .bind(playlist_id)
        .execute(&state.pool)
        .await?;

    Ok(())
}

/// Note personnelle attachée à un morceau.
#[tauri::command]
pub async fn track_note(state: State<'_, AppState>, track_id: i64) -> Result<Option<String>> {
    let note: Option<String> = sqlx::query_scalar("SELECT note FROM tracks WHERE id = ?")
        .bind(track_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();

    Ok(note.filter(|value| !value.trim().is_empty()))
}

/// Écrit une note personnelle.
///
/// # Pourquoi elle ne va pas dans le fichier
///
/// Contrairement aux paroles, une note ne décrit pas le morceau : elle décrit
/// **ce qu'il représente pour toi**. L'écrire dans les tags la ferait voyager
/// avec le fichier, jusque chez quelqu'un d'autre, ce qui n'est pas ce qu'on
/// veut d'un souvenir. Elle reste dans la base.
#[tauri::command]
pub async fn set_track_note(
    state: State<'_, AppState>,
    track_id: i64,
    note: String,
) -> Result<()> {
    let note = note.trim();

    sqlx::query("UPDATE tracks SET note = ? WHERE id = ?")
        .bind((!note.is_empty()).then_some(note))
        .bind(track_id)
        .execute(&state.pool)
        .await?;

    Ok(())
}
