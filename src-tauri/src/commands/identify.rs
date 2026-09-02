//! Commandes d'identification par empreinte acoustique.

use tauri::State;

use crate::core::Result;
use crate::db::settings;
use crate::identify::worker::{self, IdentificationProgress, API_KEY_SETTING, LAST_ERROR_SETTING};
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
    /// Aperçu masqué de la clé enregistrée, du genre `dp7d••••••`.
    ///
    /// Assez pour vérifier d'un coup d'œil qu'il s'agit de la bonne clé — et
    /// qu'elle n'a pas été tronquée au collage — sans jamais la révéler.
    pub key_hint: Option<String>,
    /// Dernière erreur de service, s'il y en a eu une.
    pub last_error: Option<String>,
    pub progress: IdentificationProgress,
}

/// Masque une clé en n'en laissant voir que le début.
fn mask(key: &str) -> String {
    let visible: String = key.chars().take(4).collect();
    format!("{visible}{}", "•".repeat(key.chars().count().saturating_sub(4).min(12)))
}

#[tauri::command]
pub async fn identification_status(state: State<'_, AppState>) -> Result<IdentificationStatus> {
    let key: Option<String> = settings::get(&state.pool, API_KEY_SETTING)
        .await?
        .filter(|value: &String| !value.trim().is_empty());

    Ok(IdentificationStatus {
        configured: key.is_some(),
        key_hint: key.as_deref().map(mask),
        last_error: settings::get(&state.pool, LAST_ERROR_SETTING).await?,
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
        tracing::info!(longueur = trimmed.len(), "clé AcoustID enregistrée");
    }

    // Une clé corrigée doit repartir sur une ardoise propre : garder l'ancienne
    // erreur laisserait croire que le problème persiste.
    settings::set(&state.pool, LAST_ERROR_SETTING, &Option::<String>::None).await?;

    Ok(())
}

/// Relance les morceaux dont l'identification avait échoué.
#[tauri::command]
pub async fn retry_identifications(state: State<'_, AppState>) -> Result<u64> {
    crate::commands::preferences::ensure_online_completion(&state.pool).await?;
    worker::retry_failed(&state.pool).await
}

/// Remet toute la bibliothèque en file d'identification.
#[tauri::command]
pub async fn reidentify_library(state: State<'_, AppState>) -> Result<u64> {
    crate::commands::preferences::ensure_online_completion(&state.pool).await?;
    worker::requeue_all(&state.pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_masque_laisse_verifier_la_cle_sans_la_reveler() {
        let masque = mask("dp7dSVeJVi");

        assert!(masque.starts_with("dp7d"), "obtenu {masque}");
        assert!(!masque.contains("SVeJVi"), "la clé ne doit pas fuiter");
    }

    #[test]
    fn le_masque_gere_une_cle_tres_courte() {
        // Cas d'un collage tronqué : il ne doit pas provoquer de panique.
        assert_eq!(mask("ab"), "ab");
        assert_eq!(mask(""), "");
    }

    #[test]
    fn le_masque_ne_revele_pas_la_longueur_dune_cle_tres_longue() {
        let masque = mask(&"x".repeat(200));
        assert!(masque.chars().count() <= 16, "obtenu {} caractères", masque.chars().count());
    }
}
