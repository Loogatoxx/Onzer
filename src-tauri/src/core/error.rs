//! Type d'erreur unique du cœur métier.
//!
//! Une seule énumération pour tout le backend : chaque module retourne
//! `Result<T>` et laisse `?` faire la conversion. Le frontend, lui, ne reçoit
//! qu'un message lisible — il n'a pas à connaître la structure interne.

use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OnzerError {
    #[error("erreur de base de données : {0}")]
    Database(#[from] sqlx::Error),

    #[error("erreur de migration : {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("erreur d'entrée/sortie : {0}")]
    Io(#[from] std::io::Error),

    /// Aucune racine de bibliothèque n'a encore été choisie par l'utilisateur.
    #[error("la racine de bibliothèque n'est pas configurée")]
    LibraryNotConfigured,

    /// La racine est connue mais inaccessible — typiquement le SSD débranché.
    /// Ce n'est pas une erreur fatale : l'application reste utilisable.
    #[error("bibliothèque hors ligne : « {0} » est introuvable")]
    LibraryOffline(String),

    /// Protection contre une base corrompue ou un import malveillant qui
    /// tenterait de faire pointer un morceau hors de la bibliothèque.
    #[error("chemin hors de la bibliothèque : {0}")]
    PathOutsideLibrary(String),

    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, OnzerError>;

/// Sérialisation pour la frontière IPC.
///
/// Tauri exige que le type d'erreur d'une commande soit `Serialize`. On expose
/// simplement le message formaté : c'est suffisant pour l'interface, et ça
/// évite de fuiter des détails d'implémentation vers le frontend.
impl Serialize for OnzerError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
