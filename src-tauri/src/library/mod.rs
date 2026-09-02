//! Gestion de la bibliothèque : lecture des métadonnées, rangement des
//! fichiers, indexation.
//!
//! Ce module ne connaît ni Tauri ni l'interface : il reçoit des chemins et une
//! connexion à la base, et retourne des résultats (ADR-002).

pub mod align;
pub mod artwork;
pub mod audio_hash;
pub mod hash;
pub mod importer;
pub mod lyrics;
pub mod metadata;
pub mod naming;
pub mod rebuild;
pub mod repair;
pub mod scanner;
pub mod transcribe;

pub use importer::{FileHandling, ImportOutcome};
pub use metadata::{is_supported_audio, TrackMetadata};
pub use scanner::{ScanProgress, ScanSummary};

#[cfg(test)]
mod tests_integration;
