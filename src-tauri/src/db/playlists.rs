//! Playlists et favoris.
//!
//! # Pourquoi la position est la clé primaire
//!
//! `playlist_tracks` a pour clé `(playlist_id, position)` et non
//! `(playlist_id, track_id)` : un même morceau peut légitimement apparaître
//! deux fois dans une playlist, et c'est l'ordre qui fait la playlist.
//!
//! Conséquence : retirer un morceau impose de **renuméroter** ce qui suit.
//! C'est le prix d'un ordre garanti par le schéma plutôt que par la discipline
//! du code appelant.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::core::{now_ms, OnzerError, Result};

use super::repository::TrackSummary;

/// Une playlist, telle que la barre latérale l'affiche.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub track_count: i64,
    /// Pochette du premier morceau, en guise de vignette.
    pub cover_hash: Option<String>,
    pub updated_at: i64,
}

/// Crée une playlist vide.
pub async fn create(pool: &SqlitePool, name: &str) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err(OnzerError::Invalid("le nom ne peut pas être vide".into()));
    }

    let now = now_ms();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO playlists (name, kind, created_at, updated_at)
         VALUES (?, 'manual', ?, ?) RETURNING id",
    )
    .bind(name)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Liste les playlists, la plus récemment modifiée d'abord.
pub async fn list(pool: &SqlitePool) -> Result<Vec<PlaylistSummary>> {
    let rows = sqlx::query_as::<_, PlaylistSummary>(
        "SELECT p.id, p.name, p.kind, p.updated_at,
                (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id)
                    AS track_count,
                (SELECT al.artwork_hash
                   FROM playlist_tracks pt
                   JOIN tracks t   ON t.id = pt.track_id
                   LEFT JOIN albums al ON al.id = t.album_id
                  WHERE pt.playlist_id = p.id AND al.artwork_hash IS NOT NULL
                  ORDER BY pt.position LIMIT 1) AS cover_hash
           FROM playlists p
          ORDER BY p.is_pinned DESC, p.updated_at DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn rename(pool: &SqlitePool, playlist_id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(OnzerError::Invalid("le nom ne peut pas être vide".into()));
    }

    sqlx::query("UPDATE playlists SET name = ?, updated_at = ? WHERE id = ?")
        .bind(name)
        .bind(now_ms())
        .bind(playlist_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn delete(pool: &SqlitePool, playlist_id: i64) -> Result<()> {
    // `playlist_tracks` est en cascade : seule la playlist est à supprimer.
    // Les morceaux, eux, restent évidemment dans la bibliothèque.
    sqlx::query("DELETE FROM playlists WHERE id = ?")
        .bind(playlist_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Ajoute des morceaux à la fin.
pub async fn add_tracks(pool: &SqlitePool, playlist_id: i64, track_ids: &[i64]) -> Result<i64> {
    if track_ids.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let now = now_ms();

    let mut position: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?")
            .bind(playlist_id)
            .fetch_one(&mut *tx)
            .await?;

    let mut added = 0_i64;
    for track_id in track_ids {
        sqlx::query(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position, added_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(playlist_id)
        .bind(track_id)
        .bind(position)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        position += 1;
        added += 1;
    }

    sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(playlist_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(added)
}

/// Retire un morceau et renumérote ce qui suit.
///
/// La renumérotation est indispensable : la clé primaire porte sur la position,
/// et un trou ferait échouer le prochain ajout à cet emplacement.
pub async fn remove_at(pool: &SqlitePool, playlist_id: i64, position: i64) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ? AND position = ?")
        .bind(playlist_id)
        .bind(position)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "UPDATE playlist_tracks SET position = position - 1
          WHERE playlist_id = ? AND position > ?",
    )
    .bind(playlist_id)
    .bind(position)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
        .bind(now_ms())
        .bind(playlist_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Morceaux d'une playlist, dans l'ordre.
pub async fn tracks(pool: &SqlitePool, playlist_id: i64) -> Result<Vec<TrackSummary>> {
    let rows = sqlx::query_as::<_, TrackSummary>(
        "SELECT t.id, t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist,
                al.title AS album, t.year, t.track_no, t.duration_ms, t.format,
                t.relative_path, t.is_available, al.artwork_hash, t.is_loved, t.added_at,
                (t.lyrics IS NOT NULL AND t.lyrics <> '') AS has_lyrics
           FROM playlist_tracks pt
           JOIN tracks t ON t.id = pt.track_id
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE pt.playlist_id = ? AND t.deleted_at IS NULL
          ORDER BY pt.position",
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// ── Favoris ─────────────────────────────────────────────────────────────────

/// Bascule le statut de favori et retourne le nouvel état.
pub async fn toggle_loved(pool: &SqlitePool, track_id: i64) -> Result<bool> {
    let loved: bool = sqlx::query_scalar(
        "UPDATE tracks SET is_loved = NOT is_loved WHERE id = ? RETURNING is_loved",
    )
    .bind(track_id)
    .fetch_one(pool)
    .await?;

    Ok(loved)
}

/// Les favoris, du plus récemment ajouté au plus ancien.
pub async fn loved(pool: &SqlitePool) -> Result<Vec<TrackSummary>> {
    let rows = sqlx::query_as::<_, TrackSummary>(
        "SELECT t.id, t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist,
                al.title AS album, t.year, t.track_no, t.duration_ms, t.format,
                t.relative_path, t.is_available, al.artwork_hash, t.is_loved, t.added_at,
                (t.lyrics IS NOT NULL AND t.lyrics <> '') AS has_lyrics
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.is_loved = 1 AND t.deleted_at IS NULL
          ORDER BY t.added_at DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
