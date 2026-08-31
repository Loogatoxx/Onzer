//! Commandes d'identification par empreinte acoustique.

use tauri::State;

use crate::core::Result;
use crate::db::settings;
use crate::identify::worker::{self, IdentificationProgress, API_KEY_SETTING};
use crate::AppState;

/// État de configuration, sans jamais renvoyer la clé elle-même.
///
/// Une clé d'API n'a aucune raison de repasser par l'interface une fois
/// enregistrée : la renvoyer l'exposerait aux outils de développement du
/// navigateur embarqué pour un bénéfice nul.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentificationStatus {
    pub configured: bool,
    pub progress: IdentificationProgress,
}

#[tauri::command]
pub async fn identification_status(state: State<'_, AppState>) -> Result<IdentificationStatus> {
    let key: Option<String> = settings::get(&state.pool, API_KEY_SETTING).await?;

    Ok(IdentificationStatus {
        configured: key.is_some_and(|value| !value.trim().is_empty()),
        progress: worker::progress(&state.pool).await?,
    })
}

/// Enregistre la clé AcoustID. Une chaîne vide la supprime.
#[tauri::command]
pub async fn set_acoustid_key(state: State<'_, AppState>, key: String) -> Result<()> {
    let trimmed = key.trim().to_string();

    if trimmed.is_empty() {
        settings::set(&state.pool, API_KEY_SETTING, &Option::<String>::None).await?;
        tracing::info!("clé AcoustID retirée");
    } else {
        settings::set(&state.pool, API_KEY_SETTING, &trimmed).await?;
        tracing::info!("clé AcoustID enregistrée");
    }

    Ok(())
}

/// Relance les morceaux dont l'identification avait échoué.
#[tauri::command]
pub async fn retry_identifications(state: State<'_, AppState>) -> Result<u64> {
    worker::retry_failed(&state.pool).await
}

/// Remet toute la bibliothèque en file d'identification.
#[tauri::command]
pub async fn reidentify_library(state: State<'_, AppState>) -> Result<u64> {
    worker::requeue_all(&state.pool).await
}
