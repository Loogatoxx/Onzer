//! Commandes de collection : playlists, favoris, paroles.

use tauri::State;

use crate::core::{OnzerError, Result};
use crate::db::playlists::{self, PlaylistSummary};
use crate::db::repository::TrackSummary;
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
#[tauri::command]
pub async fn track_lyrics(state: State<'_, AppState>, track_id: i64) -> Result<Lyrics> {
    let raw: Option<String> = sqlx::query_scalar("SELECT lyrics FROM tracks WHERE id = ?")
        .bind(track_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();

    Ok(raw.as_deref().map(lyrics::parse).unwrap_or_default())
}

/// Enregistre des paroles saisies ou collées à la main.
#[tauri::command]
pub async fn set_track_lyrics(
    state: State<'_, AppState>,
    track_id: i64,
    raw: String,
) -> Result<Lyrics> {
    let trimmed = raw.trim();

    sqlx::query("UPDATE tracks SET lyrics = ? WHERE id = ?")
        .bind((!trimmed.is_empty()).then_some(trimmed))
        .bind(track_id)
        .execute(&state.pool)
        .await
        .map_err(OnzerError::Database)?;

    Ok(lyrics::parse(trimmed))
}
