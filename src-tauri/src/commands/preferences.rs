//! Réglages de l'application, et remise à zéro de la bibliothèque.
//!
//! # Pourquoi un interrupteur pour la complétion en ligne
//!
//! Onzer sait combler ce qui manque : identifier un morceau à l'oreille,
//! chercher ses paroles, sa pochette, son album. Ces outils ont été écrits pour
//! une bibliothèque bâtie au fil de téléchargements approximatifs.
//!
//! Quand les fichiers arrivent déjà tagués et pochettés, ces mêmes outils
//! deviennent du bruit : quatre bandeaux qui proposent de réparer ce qui n'est
//! pas cassé. Les supprimer serait pourtant une erreur — la personne à qui
//! l'on prête l'application n'a pas forcément cette chance-là.
//!
//! D'où un **réglage** plutôt qu'un choix gravé dans le code : par défaut la
//! complétion est active, comme au premier jour ; qui n'en a plus besoin
//! l'éteint, et l'interface se tait.
//!
//! # Pourquoi le cœur refuse aussi, et pas seulement l'interface
//!
//! Masquer un bouton n'empêche personne d'appeler la commande qu'il déclenche.
//! Un réglage qui ne tient que par ce que l'on affiche n'est pas un réglage,
//! c'est une décoration : les commandes de complétion vérifient donc l'état
//! avant d'ouvrir la moindre connexion.
//!
//! # Ce que le réglage éteint, et ce qu'il n'éteint pas
//!
//! Il gouverne ce qu'Onzer entreprend **de lui-même** : l'ouvrier
//! d'identification, et les passes qui traitent toute la bibliothèque d'un
//! coup — paroles, pochettes, albums.
//!
//! Il ne touche pas aux gestes que l'utilisateur pose **explicitement** sur un
//! morceau : « Chercher en ligne » pour des paroles, « Chercher ailleurs »
//! pour une identité. Refuser ces gestes-là ne protégerait de rien : personne
//! n'est sollicité, c'est l'utilisateur qui demande. Le réglage fait taire une
//! proposition, il ne confisque pas un outil.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::{OnzerError, Result};
use crate::db::settings;
use crate::library::rebuild::{self, RebuildReport};
use crate::AppState;

/// Clé du réglage de complétion en ligne.
pub const ONLINE_COMPLETION: &str = "online_completion";

/// Clé du prénom affiché dans l'accueil.
pub const DISPLAY_NAME: &str = "display_name";

/// Clé du réglage d'identification automatique.
///
/// # Pourquoi il ne peut pas être le même
///
/// Les deux ont l'air d'une seule question — « Onzer a-t-il le droit d'aller
/// en ligne ? » — mais ils répondent à des besoins opposés. Vouloir des
/// paroles pour une bibliothèque déjà bien taguée est une demande courante ;
/// vouloir qu'un ouvrier réécrive les titres de ces mêmes fichiers ne l'est
/// pas. Les avoir liés faisait repartir l'identification acoustique au moment
/// où l'on demandait des paroles — un effet de bord que rien n'annonçait.
pub const AUTO_IDENTIFICATION: &str = "auto_identification";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    /// Comment l'accueil doit nommer l'utilisateur. Vide : il ne le nomme pas.
    pub display_name: String,
    /// Onzer peut-il proposer de compléter les métadonnées en ligne ?
    pub online_completion: bool,
    /// L'ouvrier d'identification acoustique tourne-t-il ?
    pub auto_identification: bool,
}

/// L'état du réglage, actif par défaut.
///
/// Le défaut compte : une application découverte pour la première fois doit
/// savoir réparer une bibliothèque en désordre, puisque c'est le cas le plus
/// probable.
pub async fn online_completion(pool: &sqlx::SqlitePool) -> Result<bool> {
    Ok(settings::get::<bool>(pool, ONLINE_COMPLETION)
        .await?
        .unwrap_or(true))
}

/// Refuse une opération de complétion quand le réglage est éteint.
pub async fn ensure_online_completion(pool: &sqlx::SqlitePool) -> Result<()> {
    if online_completion(pool).await? {
        return Ok(());
    }

    Err(OnzerError::Invalid(
        "la complétion en ligne est désactivée dans les réglages".to_string(),
    ))
}

/// L'ouvrier d'identification a-t-il le droit de tourner ?
pub async fn auto_identification(pool: &sqlx::SqlitePool) -> Result<bool> {
    Ok(settings::get::<bool>(pool, AUTO_IDENTIFICATION)
        .await?
        .unwrap_or(true))
}

/// Refuse une identification quand le réglage est éteint.
pub async fn ensure_auto_identification(pool: &sqlx::SqlitePool) -> Result<()> {
    if auto_identification(pool).await? {
        return Ok(());
    }

    Err(OnzerError::Invalid(
        "l'identification automatique est désactivée dans les réglages".to_string(),
    ))
}

/// Le prénom enregistré, ou une chaîne vide.
pub async fn display_name(pool: &sqlx::SqlitePool) -> Result<String> {
    Ok(settings::get::<String>(pool, DISPLAY_NAME)
        .await?
        .unwrap_or_default())
}

#[tauri::command]
pub async fn set_display_name(state: State<'_, AppState>, name: String) -> Result<()> {
    // Un nom vide n'est pas une erreur : c'est le choix de ne pas être nommé.
    settings::set(&state.pool, DISPLAY_NAME, &name.trim().to_string()).await
}

#[tauri::command]
pub async fn preferences(state: State<'_, AppState>) -> Result<Preferences> {
    Ok(Preferences {
        display_name: display_name(&state.pool).await?,
        online_completion: online_completion(&state.pool).await?,
        auto_identification: auto_identification(&state.pool).await?,
    })
}

#[tauri::command]
pub async fn set_auto_identification(state: State<'_, AppState>, enabled: bool) -> Result<()> {
    settings::set(&state.pool, AUTO_IDENTIFICATION, &enabled).await
}

#[tauri::command]
pub async fn set_online_completion(state: State<'_, AppState>, enabled: bool) -> Result<()> {
    settings::set(&state.pool, ONLINE_COMPLETION, &enabled).await
}

/// Vide la bibliothèque pour la reconstruire depuis le dépôt.
///
/// # Pourquoi la lecture s'arrête d'abord
///
/// Le moteur audio tient un morceau ouvert et une file d'attente qui pointent
/// vers des lignes sur le point de disparaître. Le laisser jouer par-dessus une
/// bibliothèque vidée donnerait une barre de lecture qui parle d'un morceau
/// qu'aucune page ne peut plus montrer.
#[tauri::command]
pub async fn rebuild_library(state: State<'_, AppState>) -> Result<RebuildReport> {
    // Le moteur audio peut être absent (aucun périphérique) : dans ce cas il
    // n'y a rien à arrêter, et la reconstruction n'a pas à s'en trouver
    // empêchée.
    if let Ok(player) = state.player() {
        player.stop(&state.pool).await?;
    }

    let paths = state.paths.read().await.clone();
    rebuild::run(&state.pool, &paths).await
}
