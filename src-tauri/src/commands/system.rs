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

// ── Touches multimédia ──────────────────────────────────────────────────────

/// Pourquoi les touches multimédia ne répondent pas, le cas échéant.
///
/// # Pourquoi remonter cela à l'écran
///
/// Sur macOS, capter F7/F8/F9 exige une autorisation d'accessibilité que le
/// système n'accorde pas de lui-même. Sans elle, l'enregistrement échoue en
/// silence : l'utilisateur appuie sur la touche, rien ne se passe, et rien ne
/// lui dit pourquoi. Un avertissement qui ne quitte pas les journaux est un
/// défaut qui attend — c'est ce qui avait retardé la découverte du décodeur
/// incapable de se déplacer.
static MEDIA_KEYS_ERROR: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

pub fn set_media_keys_error(error: String) {
    if let Ok(mut slot) = MEDIA_KEYS_ERROR.write() {
        *slot = (!error.is_empty()).then_some(error);
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaKeysStatus {
    pub working: bool,
    /// Message du système, quand l'enregistrement a échoué.
    pub error: Option<String>,
}

#[tauri::command]
pub fn media_keys_status() -> MediaKeysStatus {
    let error = MEDIA_KEYS_ERROR.read().ok().and_then(|slot| slot.clone());

    MediaKeysStatus {
        working: error.is_none(),
        error,
    }
}

/// Réessaie d'enregistrer les touches, après une autorisation accordée.
///
/// Sans cela, il faudrait redémarrer l'application pour que le réglage prenne
/// effet — le meilleur moyen de faire croire qu'il n'a pas marché.
#[tauri::command]
pub fn retry_media_keys(app: tauri::AppHandle) -> MediaKeysStatus {
    // Sur mobile, il n'y a rien à réenregistrer : les commandes de lecture
    // appartiennent au système. La commande existe quand même, pour que
    // l'interface reste la même partout — elle répond simplement que tout va
    // bien, ce qui est vrai.
    #[cfg(desktop)]
    crate::register_media_keys(&app);

    #[cfg(not(desktop))]
    let _ = app;

    media_keys_status()
}
