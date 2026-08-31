//! Commandes de lecture.
//!
//! Chaque commande retourne l'instantané de lecture mis à jour : l'interface
//! réagit immédiatement, sans attendre le prochain battement de la boucle de
//! surveillance.

use tauri::State;

use crate::audio::queue::{QueueItem, RepeatMode};
use crate::audio::tracking::PlaySource;
use crate::audio::PlaybackSnapshot;
use crate::core::Result;
use crate::db::repository;
use crate::AppState;

/// Instantané complet, émis quand la file ou le morceau changent.
pub const STATE_EVENT: &str = "playback://state";

/// Battement léger — position et état seulement. Émis quatre fois par seconde
/// pour animer la barre de progression, sans réexpédier toute la file.
pub const TICK_EVENT: &str = "playback://tick";

/// Charge utile du battement. Volontairement minuscule : réexpédier une file de
/// 200 morceaux quatre fois par seconde serait absurde.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTick {
    pub position_ms: i64,
    pub is_playing: bool,
}

/// Lance une liste de morceaux à partir d'une position donnée.
#[tauri::command]
pub async fn play_tracks(
    state: State<'_, AppState>,
    track_ids: Vec<i64>,
    start_at: Option<usize>,
    source: Option<PlaySource>,
) -> Result<PlaybackSnapshot> {
    let tracks = repository::tracks_by_ids(&state.pool, &track_ids).await?;
    let items: Vec<QueueItem> = tracks.into_iter().map(QueueItem::from).collect();

    let paths = state.paths.read().await.clone();

    state
        .player()?
        .play_queue(
            &state.pool,
            &paths,
            items,
            start_at.unwrap_or(0),
            source.unwrap_or(PlaySource::Library),
            None,
        )
        .await?;

    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn toggle_playback(state: State<'_, AppState>) -> Result<PlaybackSnapshot> {
    state.player()?.toggle().await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn next_track(state: State<'_, AppState>) -> Result<PlaybackSnapshot> {
    let paths = state.paths.read().await.clone();
    // `automatic = false` : c'est un appui volontaire, donc un saut au sens du
    // journal d'écoute.
    state.player()?.next(&state.pool, &paths, false).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn previous_track(state: State<'_, AppState>) -> Result<PlaybackSnapshot> {
    let paths = state.paths.read().await.clone();
    state.player()?.previous(&state.pool, &paths).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn jump_in_queue(state: State<'_, AppState>, index: usize) -> Result<PlaybackSnapshot> {
    let paths = state.paths.read().await.clone();
    state.player()?.jump_to(&state.pool, &paths, index).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn seek_to(state: State<'_, AppState>, position_ms: i64) -> Result<PlaybackSnapshot> {
    state.player()?.seek(position_ms).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn set_volume(state: State<'_, AppState>, volume: f32) -> Result<PlaybackSnapshot> {
    state.player()?.set_volume(volume).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn set_repeat(state: State<'_, AppState>, mode: RepeatMode) -> Result<PlaybackSnapshot> {
    state.player()?.set_repeat(mode).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn set_shuffle(state: State<'_, AppState>, shuffle: bool) -> Result<PlaybackSnapshot> {
    state.player()?.set_shuffle(shuffle).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn stop_playback(state: State<'_, AppState>) -> Result<PlaybackSnapshot> {
    state.player()?.stop(&state.pool).await?;
    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn playback_state(state: State<'_, AppState>) -> Result<PlaybackSnapshot> {
    Ok(state.player()?.snapshot().await)
}
