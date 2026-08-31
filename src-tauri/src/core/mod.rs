//! Fondations transverses du cœur métier : erreurs et résolution de chemins.
//!
//! Ce module ne dépend d'aucun autre module d'Onzer, et surtout pas de `tauri`
//! (ADR-002) : c'est ce qui rend le cœur testable sans lancer l'application.

pub mod error;
pub mod paths;

pub use error::{OnzerError, Result};
pub use paths::PathResolver;

/// Horodatage courant en millisecondes Unix UTC.
///
/// Toute la base utilise cette unité (voir `docs/DB_SCHEMA.md`) : centraliser
/// la fonction évite qu'un module écrive des secondes par inadvertance.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}
