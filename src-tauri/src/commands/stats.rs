//! Commandes de statistiques.

use tauri::State;

use crate::core::Result;
use crate::stats::{self, Period, Wrapped};
use crate::AppState;

/// Rassemble toute la page en **un seul aller-retour**.
///
/// Sept requêtes côté base, mais un unique appel IPC : découper en autant de
/// commandes ferait clignoter la page au chargement, chaque section arrivant
/// à son rythme.
#[tauri::command]
pub async fn wrapped(
    state: State<'_, AppState>,
    period: Option<Period>,
    top: Option<i64>,
) -> Result<Wrapped> {
    stats::wrapped(
        &state.pool,
        period.unwrap_or(Period::AllTime),
        top.unwrap_or(10),
    )
    .await
}
