//! Commandes de lecture.
//!
//! Chaque commande retourne l'instantané de lecture mis à jour : l'interface
//! réagit immédiatement, sans attendre le prochain battement de la boucle de
//! surveillance.

use tauri::{Manager, State};

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

/// Ajoute des morceaux à la fin de la file, sans interrompre l'écoute.
#[tauri::command]
pub async fn enqueue_tracks(
    state: State<'_, AppState>,
    track_ids: Vec<i64>,
) -> Result<PlaybackSnapshot> {
    let tracks = repository::tracks_by_ids(&state.pool, &track_ids).await?;
    let items: Vec<QueueItem> = tracks.into_iter().map(QueueItem::from).collect();

    let paths = state.paths.read().await.clone();
    state.player()?.enqueue(&state.pool, &paths, items).await?;

    Ok(state.player()?.snapshot().await)
}

/// Insère juste après le morceau en cours.
///
/// # Pourquoi l'ordre des identifiants est préservé
///
/// `tracks_by_ids` rend les morceaux dans l'ordre de la base, pas dans celui
/// qu'on a demandé. Enfiler un album « à lire ensuite » le remettrait donc
/// dans un ordre arbitraire — celui de son insertion dans la bibliothèque.
#[tauri::command]
pub async fn play_next(
    state: State<'_, AppState>,
    track_ids: Vec<i64>,
) -> Result<PlaybackSnapshot> {
    let tracks = repository::tracks_by_ids(&state.pool, &track_ids).await?;

    let par_id: std::collections::HashMap<i64, QueueItem> = tracks
        .into_iter()
        .map(|track| (track.id, QueueItem::from(track)))
        .collect();

    let items: Vec<QueueItem> = track_ids
        .iter()
        .filter_map(|id| par_id.get(id).cloned())
        .collect();

    let paths = state.paths.read().await.clone();
    state.player()?.play_next(&state.pool, &paths, items).await?;

    Ok(state.player()?.snapshot().await)
}

/// Retire un morceau de la file, par sa place dans l'ordre de lecture.
#[tauri::command]
pub async fn remove_from_queue(
    state: State<'_, AppState>,
    position: usize,
) -> Result<PlaybackSnapshot> {
    let paths = state.paths.read().await.clone();
    state
        .player()?
        .remove_from_queue(&state.pool, &paths, position)
        .await?;

    Ok(state.player()?.snapshot().await)
}

/// Déplace un morceau dans l'ordre de lecture.
#[tauri::command]
pub async fn move_in_queue(
    state: State<'_, AppState>,
    from: usize,
    to: usize,
) -> Result<PlaybackSnapshot> {
    state.player()?.move_in_queue(from, to).await?;
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

/// Reprend une écoute venue de l'autre appareil.
///
/// # Pourquoi les chemins et non des identifiants
///
/// La synchronisation raisonne en chemins relatifs : ce sont eux qui traversent
/// et qui se traduisent d'une bibliothèque à l'autre. Les identifiants, eux,
/// n'ont de sens que sur l'appareil qui les a attribués.
#[tauri::command]
pub async fn resume_playback(
    state: State<'_, AppState>,
    paths: Vec<String>,
    position: usize,
    position_ms: i64,
) -> Result<PlaybackSnapshot> {
    let placeholders = vec!["?"; paths.len()].join(",");
    let sql = format!(
        "SELECT id, relative_path FROM tracks
          WHERE relative_path IN ({placeholders}) AND deleted_at IS NULL"
    );

    let mut requete = sqlx::query_as::<_, (i64, String)>(&sql);
    for chemin in &paths {
        requete = requete.bind(chemin);
    }

    let par_chemin: std::collections::HashMap<String, i64> = requete
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(|(id, chemin)| (chemin, id))
        .collect();

    // L'ordre de la file vient de l'autre appareil ; la base rend ses lignes
    // dans le sien. C'est le premier qui compte.
    let ids: Vec<i64> = paths
        .iter()
        .filter_map(|chemin| par_chemin.get(chemin).copied())
        .collect();

    if ids.is_empty() {
        return Err(crate::core::OnzerError::Invalid(
            "aucun de ces morceaux n'est ici".to_string(),
        ));
    }

    // Les morceaux que nous n'avons pas ont été écartés : la place du morceau
    // en cours vaut pour la file d'origine, il faut la ramener à celle-ci.
    let position = position.min(ids.len().saturating_sub(1));

    let tracks = repository::tracks_by_ids(&state.pool, &ids).await?;
    let par_id: std::collections::HashMap<i64, QueueItem> = tracks
        .into_iter()
        .map(|track| (track.id, QueueItem::from(track)))
        .collect();

    let items: Vec<QueueItem> = ids.iter().filter_map(|id| par_id.get(id).cloned()).collect();

    let paths_resolver = state.paths.read().await.clone();

    state
        .player()?
        .play_queue(
            &state.pool,
            &paths_resolver,
            items,
            position,
            PlaySource::Queue,
            None,
        )
        .await?;

    state.player()?.seek(position_ms).await?;

    Ok(state.player()?.snapshot().await)
}

#[tauri::command]
pub async fn playback_state(state: State<'_, AppState>) -> Result<PlaybackSnapshot> {
    Ok(state.player()?.snapshot().await)
}

// ════════════════════════════════════════════════════════════════════════════
//  Minuteur de sommeil
// ════════════════════════════════════════════════════════════════════════════

/// Échéance en cours, et le numéro de la demande qui l'a posée.
///
/// # Pourquoi un numéro plutôt qu'une annulation
///
/// La tâche qui attend ne peut pas être interrompue proprement une fois
/// lancée : on peut lui demander de s'arrêter, mais elle dort. Le numéro
/// renverse la question — à son réveil, elle vérifie qu'elle est toujours la
/// dernière. Une demande plus récente l'a-t-elle remplacée ? Elle ne fait rien.
/// C'est la seule façon de garantir qu'un minuteur annulé puis reposé ne coupe
/// pas la musique à l'heure de l'ancien.
static SOMMEIL: std::sync::Mutex<Option<(std::time::Instant, u64)>> =
    std::sync::Mutex::new(None);

/// Numéro de la dernière demande. Croît, ne redescend jamais.
static DEMANDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Ce qu'il reste à attendre, en millisecondes.
fn restant() -> Option<i64> {
    let garde = SOMMEIL.lock().ok()?;
    let (echeance, _) = (*garde)?;

    let reste = echeance.saturating_duration_since(std::time::Instant::now());
    if reste.is_zero() {
        None
    } else {
        Some(reste.as_millis() as i64)
    }
}

/// Arme, réarme ou annule le minuteur.
///
/// `delay_ms` à `None` annule. Le délai est en millisecondes et non en minutes :
/// « à la fin du morceau » ne tombe jamais sur une minute ronde.
#[tauri::command]
pub async fn set_sleep_timer(app: tauri::AppHandle, delay_ms: Option<i64>) -> Result<Option<i64>> {
    use std::sync::atomic::Ordering;

    let numero = DEMANDE.fetch_add(1, Ordering::SeqCst) + 1;

    let Some(delai) = delay_ms.filter(|valeur| *valeur > 0) else {
        if let Ok(mut garde) = SOMMEIL.lock() {
            *garde = None;
        }
        return Ok(None);
    };

    let duree = std::time::Duration::from_millis(delai as u64);
    let echeance = std::time::Instant::now() + duree;

    if let Ok(mut garde) = SOMMEIL.lock() {
        *garde = Some((echeance, numero));
    }

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(duree).await;

        // Une demande plus récente est passée : celle-ci n'a plus rien à dire.
        let toujours_valide = SOMMEIL
            .lock()
            .ok()
            .and_then(|garde| *garde)
            .is_some_and(|(_, pose)| pose == numero);

        if !toujours_valide {
            return;
        }

        if let Ok(mut garde) = SOMMEIL.lock() {
            *garde = None;
        }

        let etat = app.state::<AppState>();
        if let Ok(player) = etat.player() {
            let _ = player.pause().await;
        }
    });

    Ok(Some(delai))
}

/// Ce qu'il reste au minuteur, ou `None` s'il n'y en a pas.
#[tauri::command]
pub async fn sleep_timer() -> Result<Option<i64>> {
    Ok(restant())
}
