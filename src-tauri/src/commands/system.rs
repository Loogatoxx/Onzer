//! Commandes d'état général de l'application.
//!
//! Rappel de l'ADR-004 : une commande valide ses entrées, appelle un module
//! métier, et retourne. Aucune logique ne vit ici.

use serde::Serialize;
use tauri::State;

use crate::core::Result;
use crate::{db, AppState};

/// État renvoyé au frontend au démarrage.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    /// Version du schéma effectivement appliquée en base.
    pub schema_version: i64,
    pub database_path: String,
    /// Racine de bibliothèque configurée, ou `null` au premier lancement.
    pub library_root: Option<String>,
    /// La racine est-elle réellement accessible ? Distingue « pas encore
    /// configurée » de « SSD débranché » (ADR-006).
    pub library_online: bool,
    pub track_count: i64,
    pub event_count: i64,
}

#[tauri::command]
pub async fn app_status(state: State<'_, AppState>) -> Result<AppStatus> {
    let paths = state.paths.read().await;

    let track_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE deleted_at IS NULL")
            .fetch_one(&state.pool)
            .await?;

    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_events")
        .fetch_one(&state.pool)
        .await?;

    Ok(AppStatus {
        schema_version: db::schema_version(&state.pool).await?,
        database_path: paths.database_file().display().to_string(),
        library_root: paths
            .library_root()
            .map(|root| root.display().to_string()),
        library_online: paths.is_library_online(),
        track_count,
        event_count,
    })
}
