//! Commandes du moteur de recommandation.
//!
//! Une playlist générée est **immédiatement jouée** : demander une radio puis
//! devoir cliquer sur « lecture » serait une étape de trop. La playlist est
//! néanmoins retournée à l'interface, pour qu'elle puisse afficher la raison de
//! présence de chaque morceau.

use chrono::{Datelike, Local, Timelike};
use tauri::State;

use crate::analysis::worker::{self, AnalysisProgress};
use crate::audio::queue::QueueItem;
use crate::audio::tracking::PlaySource;
use crate::core::{OnzerError, Result};
use crate::db::repository;
use crate::reco::bandit::Rng;
use crate::reco::engine::{self, GeneratedPlaylist, PlaylistKind};
use crate::reco::ListeningContext;
use crate::AppState;

/// Longueur par défaut d'une playlist générée.
///
/// Vingt-cinq morceaux ≈ 1 h 30 : de quoi tenir une session sans que le moteur
/// ait à se répéter, et sans figer un choix pour la soirée entière.
const DEFAULT_LENGTH: usize = 25;

/// Construit le contexte d'écoute courant.
fn current_context(output_device: Option<String>) -> ListeningContext {
    let now = Local::now();

    ListeningContext::now(
        i64::from(now.hour()),
        i64::from(now.weekday().num_days_from_monday()),
        output_device,
    )
}

/// Génère une playlist, la joue, et la retourne.
async fn generate_and_play(
    state: &AppState,
    kind: PlaylistKind,
    length: usize,
) -> Result<GeneratedPlaylist> {
    let player = state.player()?;
    let context = current_context(player.output_device().map(str::to_string));

    let data = engine::load(&state.pool, &context).await?;

    if data.total_tracks == 0 {
        return Err(OnzerError::Invalid(
            "aucun morceau disponible — la bibliothèque est vide ou hors ligne".to_string(),
        ));
    }

    // Graine tirée de l'horloge : deux radios lancées coup sur coup ne doivent
    // pas être identiques.
    let mut rng = Rng::from_entropy();
    let tracks = engine::generate(&data, &kind, length, &mut rng);

    if tracks.is_empty() {
        return Err(OnzerError::Invalid(
            "le moteur n'a rien trouvé à proposer".to_string(),
        ));
    }

    let session_id = engine::save_session(&state.pool, &kind, &context, &tracks).await?;

    // ── Lecture immédiate ───────────────────────────────────────────────
    let track_ids: Vec<i64> = tracks.iter().map(|track| track.track_id).collect();
    let summaries = repository::tracks_by_ids(&state.pool, &track_ids).await?;
    let items: Vec<QueueItem> = summaries.into_iter().map(QueueItem::from).collect();

    let paths = state.paths.read().await.clone();
    player
        .play_queue(
            &state.pool,
            &paths,
            items,
            0,
            // `source` et `source_id` sont ce qui permettra, écoute après
            // écoute, d'attribuer succès et échecs à la bonne stratégie.
            PlaySource::Reco,
            Some(session_id),
        )
        .await?;

    let progress = worker::progress(&state.pool).await?;

    Ok(GeneratedPlaylist {
        session_id,
        kind: kind.as_str().to_string(),
        title: title_for(&kind, &state.pool).await,
        subtitle: subtitle_for(&kind, &context, &progress),
        pool_size: data.total_tracks,
        analyzed_ratio: progress.ratio(),
        tracks,
    })
}

async fn title_for(kind: &PlaylistKind, pool: &sqlx::SqlitePool) -> String {
    match kind {
        PlaylistKind::Radio { seed_track_id } => {
            let title: Option<String> =
                sqlx::query_scalar("SELECT title FROM tracks WHERE id = ?")
                    .bind(seed_track_id)
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten();

            match title {
                Some(title) => format!("Radio · {title}"),
                None => "Radio".to_string(),
            }
        }
        PlaylistKind::ForNow => "Pour toi maintenant".to_string(),
        PlaylistKind::Forgotten => "Tu les avais oubliés".to_string(),
    }
}

fn subtitle_for(
    kind: &PlaylistKind,
    context: &ListeningContext,
    progress: &AnalysisProgress,
) -> String {
    // Tant que l'analyse est incomplète, le dire franchement : une playlist
    // moyenne s'excuse mieux qu'elle ne se cache.
    if !progress.is_complete() && progress.total > 0 {
        return format!(
            "{} · {} morceaux encore à analyser",
            base_subtitle(kind, context),
            progress.pending
        );
    }

    base_subtitle(kind, context)
}

fn base_subtitle(kind: &PlaylistKind, context: &ListeningContext) -> String {
    match kind {
        PlaylistKind::Radio { .. } => "Dans le même univers sonore".to_string(),
        PlaylistKind::ForNow => format!("Ce que tu écoutes {}", context.label()),
        PlaylistKind::Forgotten => "Aimés autrefois, plus écoutés depuis longtemps".to_string(),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Commandes
// ════════════════════════════════════════════════════════════════════════════

/// Radio construite autour d'un morceau.
#[tauri::command]
pub async fn start_radio(
    state: State<'_, AppState>,
    seed_track_id: i64,
    length: Option<usize>,
) -> Result<GeneratedPlaylist> {
    generate_and_play(
        &state,
        PlaylistKind::Radio { seed_track_id },
        length.unwrap_or(DEFAULT_LENGTH).clamp(5, 100),
    )
    .await
}

/// Ce qui convient à ce moment précis de la journée.
#[tauri::command]
pub async fn start_for_now(
    state: State<'_, AppState>,
    length: Option<usize>,
) -> Result<GeneratedPlaylist> {
    generate_and_play(
        &state,
        PlaylistKind::ForNow,
        length.unwrap_or(DEFAULT_LENGTH).clamp(5, 100),
    )
    .await
}

/// Morceaux aimés autrefois, plus écoutés depuis longtemps.
#[tauri::command]
pub async fn start_forgotten(
    state: State<'_, AppState>,
    length: Option<usize>,
) -> Result<GeneratedPlaylist> {
    generate_and_play(
        &state,
        PlaylistKind::Forgotten,
        length.unwrap_or(DEFAULT_LENGTH).clamp(5, 100),
    )
    .await
}

#[tauri::command]
pub async fn analysis_progress(state: State<'_, AppState>) -> Result<AnalysisProgress> {
    worker::progress(&state.pool).await
}

/// Remet toute la bibliothèque en file d'analyse.
#[tauri::command]
pub async fn reanalyze_library(state: State<'_, AppState>) -> Result<u64> {
    worker::requeue_all(&state.pool).await
}

/// Qualité mesurée du moteur, par origine d'écoute et par stratégie.
///
/// C'est le tableau de bord qui évite l'astrologie : si les morceaux proposés
/// par l'IA sont plus souvent abandonnés que ceux tirés au hasard, il faut
/// pouvoir le lire.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityRow {
    pub label: String,
    pub plays: i64,
    pub avg_completion: Option<f64>,
    pub early_skip_rate: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyRow {
    pub name: String,
    pub proposals: i64,
    pub estimated_success_rate: f64,
    pub judged: i64,
    pub observed_success_rate: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoDiagnostics {
    pub by_source: Vec<QualityRow>,
    pub by_strategy: Vec<StrategyRow>,
}

/// Lignes des vues de qualité, nommées pour rester lisibles.
type QualitySql = (String, i64, Option<f64>, Option<f64>, Option<f64>);
type StrategySql = (String, i64, Option<f64>, i64, Option<f64>);

#[tauri::command]
pub async fn reco_diagnostics(state: State<'_, AppState>) -> Result<RecoDiagnostics> {
    let sources: Vec<QualitySql> = sqlx::query_as(
        "SELECT source, plays, avg_completion, completion_rate, early_skip_rate
         FROM reco_quality ORDER BY plays DESC",
    )
    .fetch_all(&state.pool)
    .await?;

    let strategies: Vec<StrategySql> = sqlx::query_as(
        "SELECT name, proposals, estimated_success_rate, judged, observed_success_rate
         FROM reco_strategy_quality ORDER BY proposals DESC",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(RecoDiagnostics {
        by_source: sources
            .into_iter()
            .map(|(label, plays, avg, _rate, early)| QualityRow {
                label,
                plays,
                avg_completion: avg,
                early_skip_rate: early,
            })
            .collect(),
        by_strategy: strategies
            .into_iter()
            .map(
                |(name, proposals, estimated, judged, observed)| StrategyRow {
                    name,
                    proposals,
                    estimated_success_rate: estimated.unwrap_or(0.5),
                    judged,
                    observed_success_rate: observed,
                },
            )
            .collect(),
    })
}
