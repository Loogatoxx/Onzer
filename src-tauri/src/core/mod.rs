//! Fondations transverses du cœur métier : erreurs et résolution de chemins.
//!
//! Ce module ne dépend d'aucun autre module d'Onzer, et surtout pas de `tauri`
//! (ADR-002) : c'est ce qui rend le cœur testable sans lancer l'application.

pub mod error;
pub mod paths;

pub use error::{OnzerError, Result};
pub use paths::PathResolver;
