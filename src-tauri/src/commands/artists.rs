//! Navigation par artiste.
//!
//! # Pourquoi une entrée à part
//!
//! Une bibliothèque de 325 morceaux compte une soixantaine d'artistes. C'est
//! une échelle où l'on se souvient de **qui** on veut écouter bien avant de se
//! souvenir de **quoi** — « mets du Népal » vient plus naturellement que le
//! titre exact d'un morceau. La liste des artistes est donc une porte d'entrée
//! au moins aussi utile que celle des titres.
//!
//! # La pochette d'un artiste
//!
//! Onzer ne télécharge pas de portraits : cela supposerait d'aller les chercher
//! chez un tiers pour un ornement, et d'en gérer le cache. La vignette est donc
//! la pochette de l'album dont on possède le plus de titres — celle que
//! l'utilisateur associe déjà à cet artiste.

use serde::Serialize;
use tauri::State;

use crate::core::Result;
use crate::db::repository::{TrackSummary, TRACK_COLUMNS};
use crate::AppState;

/// Un artiste, tel que la liste l'affiche.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ArtistSummary {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    /// Pochette de l'album le mieux représenté.
    pub cover_hash: Option<String>,
}

/// Tous les artistes principaux, du mieux fourni au moins fourni.
///
/// Les invités n'apparaissent pas : figurer sur un featuring ne fait pas de
/// quelqu'un un artiste de la bibliothèque, et les faire remonter noierait la
/// liste sous des noms dont on ne possède rien.
#[tauri::command]
pub async fn list_artists(state: State<'_, AppState>) -> Result<Vec<ArtistSummary>> {
    let artists = sqlx::query_as::<_, ArtistSummary>(
        "SELECT a.id, a.name,
                COUNT(*) AS track_count,
                (SELECT al.artwork_hash
                   FROM track_artists ta2
                   JOIN tracks t2 ON t2.id = ta2.track_id AND t2.deleted_at IS NULL
                   JOIN albums al ON al.id = t2.album_id
                  WHERE ta2.artist_id = a.id AND ta2.role = 'main'
                    AND al.artwork_hash IS NOT NULL
                  GROUP BY al.id
                  ORDER BY COUNT(*) DESC
                  LIMIT 1) AS cover_hash
           FROM artists a
           JOIN track_artists ta ON ta.artist_id = a.id AND ta.role = 'main'
           JOIN tracks t ON t.id = ta.track_id AND t.deleted_at IS NULL
          GROUP BY a.id
          ORDER BY track_count DESC, a.name COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(artists)
}

/// Tous les morceaux d'un artiste, invités compris.
///
/// Ici, à l'inverse de la liste, les featurings **comptent** : on ouvre la page
/// d'un artiste pour tout ce qu'il a fait, pas seulement pour ce qu'il a signé
/// seul.
///
/// L'ordre suit celui d'un disque — album, puis numéro de piste — parce qu'on
/// vient y chercher une discographie, pas un classement.
#[tauri::command]
pub async fn artist_tracks(
    state: State<'_, AppState>,
    artist_id: i64,
) -> Result<Vec<TrackSummary>> {
    let tracks = sqlx::query_as::<_, TrackSummary>(&format!(
        "SELECT {TRACK_COLUMNS}
           FROM track_artists ta
           JOIN tracks t ON t.id = ta.track_id AND t.deleted_at IS NULL
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE ta.artist_id = ?
          GROUP BY t.id
          ORDER BY al.year DESC, al.title, t.disc_no, t.track_no, t.title",
    ))
    .bind(artist_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(tracks)
}
