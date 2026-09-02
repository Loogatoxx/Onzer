//! Retrouver une identité quand l'empreinte a échoué.
//!
//! # Ce que ce module répare
//!
//! L'identification acoustique laisse deux sortes de restes :
//!
//! | Reste | Ce que l'utilisateur voit |
//! |---|---|
//! | 256 morceaux `not_found` | Le nom du fichier, tel quel |
//! | 93 morceaux sans album | Un **tiret** à la place de l'album |
//!
//! Le tiret est le symptôme le plus visible : il ne dit pas « album inconnu »,
//! il dit « cette ligne est incomplète ». Or l'information existe — elle est
//! juste dans un catalogue que l'empreinte n'interroge pas.
//!
//! # Deux portes, pas une
//!
//! ```text
//!   morceau sans album
//!        │
//!        ├─► automatique : Deezer, titre + artiste concordants
//!        │                 → album, année et pochette écrits sans rien demander
//!        │
//!        └─► manuelle : Deezer + iTunes + MusicBrainz, côte à côte
//!                       → l'utilisateur reconnaît son morceau et choisit
//! ```
//!
//! La porte automatique n'accepte qu'une concordance forte (`catalog::auto_pick`).
//! La porte manuelle montre **tout**, y compris les réponses médiocres : c'est
//! l'utilisateur qui juge, et une liste vide n'apprend rien.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::{OnzerError, Result};
use crate::identify::catalog::{self, Candidate, Query};
use crate::identify::deezer::DeezerClient;
use crate::identify::itunes::ItunesClient;
use crate::identify::musicbrainz::MusicBrainzClient;
use crate::library::importer;
use crate::AppState;

/// Ce que la base sait d'un morceau avant d'aller chercher ailleurs.
struct Subject {
    title: String,
    artist: Option<String>,
    duration_ms: i64,
}

/// Propose des fiches venues de plusieurs catalogues.
///
/// Les trois services sont interrogés **en parallèle** : ils s'ignorent, et
/// les attendre l'un après l'autre ferait patienter l'utilisateur pour rien.
/// Un service muet ou en panne n'empêche pas les autres de répondre — c'est
/// tout l'intérêt d'en avoir plusieurs.
#[tauri::command]
pub async fn metadata_candidates(
    state: State<'_, AppState>,
    track_id: i64,
) -> Result<Vec<Candidate>> {
    let subject = subject(&state, track_id).await?;

    let query = Query {
        artist: subject.artist.as_deref(),
        title: &subject.title,
        duration_ms: subject.duration_ms,
    };

    let (deezer, itunes, musicbrainz) = tokio::join!(
        deezer_candidates(query),
        itunes_candidates(query),
        musicbrainz_candidates(query),
    );

    let mut all = Vec::new();
    all.extend(deezer);
    all.extend(itunes);
    all.extend(musicbrainz);

    Ok(catalog::rank(all, &query))
}

/// Applique la fiche retenue par l'utilisateur.
///
/// # Pourquoi l'écriture n'est pas partielle
///
/// Choisir une fiche, c'est dire « ce morceau est celui-là ». Écrire l'album
/// sans le titre laisserait la ligne dans un état mixte, moitié fichier moitié
/// catalogue, impossible à expliquer plus tard. Tout ce que la fiche apporte
/// est donc écrit d'un bloc : titre, artiste, album, année, pochette.
///
/// Les paroles suivent la même règle que la correction manuelle : effacées si
/// le titre change, puisqu'elles appartenaient à l'ancien.
#[tauri::command]
pub async fn apply_candidate(
    state: State<'_, AppState>,
    track_id: i64,
    candidate: Candidate,
) -> Result<()> {
    let title = candidate.title.trim().to_string();
    if title.is_empty() {
        return Err(OnzerError::Invalid(
            "cette fiche n'a pas de titre".to_string(),
        ));
    }

    let previous = subject(&state, track_id).await?;
    let artist = clean(candidate.artist.clone());
    let album = clean(candidate.album.clone());

    write_identity(&state, track_id, &title, artist.as_deref(), album.as_deref()).await?;

    crate::db::repository::restore_identity(
        &state.pool,
        track_id,
        &title,
        artist.as_deref(),
        album.as_deref(),
    )
    .await?;

    // La note d'identification doit dire la vérité : ces métadonnées ne sont
    // pas « les tags d'origine rétablis », elles viennent d'un catalogue et
    // d'un choix humain.
    sqlx::query("UPDATE tracks SET identification_note = ? WHERE id = ?")
        .bind(format!("fiche {} retenue à la main", candidate.source))
        .bind(track_id)
        .execute(&state.pool)
        .await?;

    if catalog::similarity(&previous.title, &title) < 0.95 {
        sqlx::query("UPDATE tracks SET lyrics = NULL WHERE id = ?")
            .bind(track_id)
            .execute(&state.pool)
            .await?;
    }

    attach(&state, track_id, &candidate, album.as_deref(), artist.as_deref()).await
}

/// Rapatrie la vignette d'une fiche, en `data:` URI.
///
/// # Pourquoi passer par le cœur Rust pour une image
///
/// La politique de sécurité du contenu interdit au webview de charger quoi que
/// ce soit hors de l'application (ADR-005). Un `<img src="https://…">` serait
/// bloqué net — et il vaut mieux qu'il le reste : c'est ce qui garantit que
/// l'interface d'un lecteur hors ligne ne parle à personne. Les octets passent
/// donc par ici.
///
/// Seules les adresses des catalogues connus sont acceptées : l'interface ne
/// doit pas pouvoir se servir de cette commande pour aller chercher n'importe
/// quoi n'importe où.
#[tauri::command]
pub async fn candidate_preview(url: String) -> Result<Option<String>> {
    if !is_known_host(&url) {
        return Err(OnzerError::Invalid(
            "cette adresse ne vient pas d'un catalogue connu".to_string(),
        ));
    }

    let Some(bytes) = crate::identify::images::fetch(&url).await? else {
        return Ok(None);
    };

    Ok(Some(format!(
        "data:image/jpeg;base64,{}",
        crate::library::artwork::encode_base64(&bytes)
    )))
}

/// L'adresse appartient-elle à un serveur d'images de catalogue ?
fn is_known_host(url: &str) -> bool {
    const HOSTS: [&str; 4] = [
        "https://cdn-images.dzcdn.net/",
        "https://e-cdns-images.dzcdn.net/",
        "https://api.deezer.com/",
        "https://is1-ssl.mzstatic.com/",
    ];

    HOSTS.iter().any(|host| url.starts_with(host))
        // Apple répartit ses images sur is1…is5, avec ou sans « -ssl ».
        || (url.starts_with("https://is") && url.contains(".mzstatic.com/"))
}

/// Combien de morceaux affichent encore un tiret à la place de leur album.
#[tauri::command]
pub async fn missing_albums(state: State<'_, AppState>) -> Result<AlbumProgress> {
    let missing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE deleted_at IS NULL AND album_id IS NULL",
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(AlbumProgress {
        missing,
        running: FILLING.load(Ordering::SeqCst),
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumProgress {
    pub missing: i64,
    pub running: bool,
}

/// Un travail à la fois : deux passes écriraient le même morceau.
static FILLING: AtomicBool = AtomicBool::new(false);

/// Complète les albums manquants, sans rien demander.
///
/// N'écrit **que** l'album, l'année et la pochette : ni le titre ni l'artiste,
/// qui ne sont pas ce qu'on répare ici et que l'utilisateur a peut-être déjà
/// corrigés à la main.
#[tauri::command]
pub async fn fill_missing_albums(state: State<'_, AppState>) -> Result<i64> {
    crate::commands::preferences::ensure_online_completion(&state.pool).await?;

    if FILLING.swap(true, Ordering::SeqCst) {
        return Err(OnzerError::Invalid(
            "une complétion est déjà en cours".to_string(),
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
          WHERE t.deleted_at IS NULL AND t.album_id IS NULL
          ORDER BY t.id",
    )
    .fetch_all(&state.pool)
    .await?;

    let pool = state.pool.clone();
    let paths = std::sync::Arc::clone(&state.paths);
    let total = pending.len() as i64;

    tauri::async_runtime::spawn(async move {
        let Ok(deezer) = DeezerClient::new() else {
            FILLING.store(false, Ordering::SeqCst);
            return;
        };

        for (track_id, title, artist, duration_ms) in pending {
            let query = Query {
                artist: artist.as_deref(),
                title: &title,
                duration_ms,
            };

            let Ok(hits) = deezer.search(artist.as_deref(), &title).await else {
                continue;
            };

            let ranked = catalog::rank(hits, &query);
            let Some(found) = catalog::auto_pick(&ranked, &query) else {
                continue;
            };
            let Some(album) = found.album.clone() else {
                continue;
            };

            let year = match &found.album_ref {
                Some(reference) => deezer.album_year(reference).await.unwrap_or(None),
                None => None,
            };

            let bytes = match &found.cover_url {
                Some(url) => crate::identify::images::fetch(url).await.unwrap_or(None),
                None => None,
            };

            let resolver = paths.read().await.clone();
            let hash = match bytes {
                Some(bytes) => crate::library::artwork::store(&resolver.artwork_dir(), &bytes).ok(),
                None => None,
            };

            // Sans image, l'album mérite quand même d'être écrit : un nom
            // d'album vaut mieux qu'un tiret, même sans pochette.
            let outcome = match hash {
                Some(hash) => {
                    crate::db::repository::attach_artwork(
                        &pool,
                        track_id,
                        &hash,
                        Some(&album),
                        found.artist.as_deref(),
                        year,
                    )
                    .await
                }
                None => {
                    crate::db::repository::attach_album_only(
                        &pool,
                        track_id,
                        &album,
                        found.artist.as_deref(),
                        year,
                    )
                    .await
                }
            };

            if let Err(error) = outcome {
                tracing::warn!(track_id, %error, "album non complété");
            }
        }

        FILLING.store(false, Ordering::SeqCst);
    });

    Ok(total)
}

// ── Rouages ─────────────────────────────────────────────────────────────────

async fn subject(state: &State<'_, AppState>, track_id: i64) -> Result<Subject> {
    let row: Option<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1),
                t.duration_ms
           FROM tracks t WHERE t.id = ?",
    )
    .bind(track_id)
    .fetch_optional(&state.pool)
    .await?;

    let (title, artist, duration_ms) =
        row.ok_or_else(|| OnzerError::Invalid("morceau introuvable".to_string()))?;

    Ok(Subject {
        title,
        artist,
        duration_ms,
    })
}

async fn deezer_candidates(query: Query<'_>) -> Vec<Candidate> {
    let Ok(client) = DeezerClient::new() else {
        return Vec::new();
    };

    client
        .search(query.artist, query.title)
        .await
        .unwrap_or_default()
}

async fn itunes_candidates(query: Query<'_>) -> Vec<Candidate> {
    let Ok(client) = ItunesClient::new() else {
        return Vec::new();
    };

    client
        .search(query.artist, query.title)
        .await
        .unwrap_or_default()
}

/// MusicBrainz ne rend ni album ni pochette dans sa recherche : sa fiche est
/// plus pauvre, mais son catalogue connaît des morceaux que les services
/// commerciaux ne vendent plus.
async fn musicbrainz_candidates(query: Query<'_>) -> Vec<Candidate> {
    let Ok(client) = MusicBrainzClient::new() else {
        return Vec::new();
    };

    let Ok(hits) = client.search(query.artist, query.title).await else {
        return Vec::new();
    };

    hits.into_iter()
        .map(|hit| Candidate {
            artist: hit.artist,
            duration_ms: hit.length_ms.unwrap_or(0),
            ..Candidate::new("MusicBrainz", hit.title)
        })
        .collect()
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Réécrit les tags du fichier avant de toucher à la base.
///
/// Si le disque refuse, la base ne doit pas prétendre le contraire.
async fn write_identity(
    state: &State<'_, AppState>,
    track_id: i64,
    title: &str,
    artist: Option<&str>,
    album: Option<&str>,
) -> Result<()> {
    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&state.pool)
            .await?;

    let Some(relative_path) = relative_path else {
        return Ok(());
    };

    let paths = state.paths.read().await.clone();
    let Ok(path) = importer::absolute_path(&paths, &relative_path) else {
        return Ok(());
    };

    if !path.is_file() {
        return Ok(());
    }

    let (titre, artiste, disque) = (
        title.to_string(),
        artist.map(str::to_string),
        album.map(str::to_string),
    );

    tokio::task::spawn_blocking(move || {
        crate::library::metadata::rewrite_identity(
            &path,
            &titre,
            artiste.as_deref(),
            disque.as_deref(),
        )
    })
    .await
    .map_err(|error| OnzerError::Invalid(error.to_string()))?
}

/// Récupère et attache la pochette de la fiche retenue.
async fn attach(
    state: &State<'_, AppState>,
    track_id: i64,
    candidate: &Candidate,
    album: Option<&str>,
    artist: Option<&str>,
) -> Result<()> {
    let Some(url) = candidate.cover_url.as_deref() else {
        return Ok(());
    };

    // Les images ont leur propre file, commune à tous les services : la
    // cadence d'une API protège l'API, pas le serveur d'images qui est
    // derrière.
    let Some(bytes) = crate::identify::images::fetch(url).await? else {
        return Ok(());
    };

    let year = match (candidate.year, &candidate.album_ref, candidate.source.as_str()) {
        (Some(year), _, _) => Some(year),
        (None, Some(reference), "Deezer") => {
            DeezerClient::new()?.album_year(reference).await.unwrap_or(None)
        }
        _ => None,
    };

    let paths = state.paths.read().await.clone();
    let hash = crate::library::artwork::store(&paths.artwork_dir(), &bytes)?;

    crate::db::repository::attach_artwork(&state.pool, track_id, &hash, album, artist, year).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seules_les_adresses_de_catalogue_sont_acceptees() {
        // Cette commande télécharge ce qu'on lui donne : sans cette barrière,
        // l'interface disposerait d'un passe-partout vers l'extérieur.
        assert!(is_known_host(
            "https://cdn-images.dzcdn.net/images/cover/abc/1000x1000-000000-80-0-0.jpg"
        ));
        assert!(is_known_host(
            "https://is1-ssl.mzstatic.com/image/thumb/Music/a.jpg/1000x1000bb.jpg"
        ));
        assert!(is_known_host("https://is5.mzstatic.com/image/thumb/b.jpg"));

        assert!(!is_known_host("https://exemple.invalide/pochette.jpg"));
        assert!(!is_known_host("file:///etc/passwd"));
        assert!(!is_known_host("http://cdn-images.dzcdn.net/a.jpg"));
    }
}
