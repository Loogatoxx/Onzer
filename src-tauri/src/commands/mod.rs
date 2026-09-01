//! Frontière IPC entre le frontend et le cœur métier.
//!
//! **C'est le seul module autorisé à importer `tauri`** (ADR-004).
//! Les commandes ne contiennent aucune logique : elles délèguent aux modules
//! métier, ce qui garde le cœur testable par `cargo test` sans lancer
//! l'application.

pub mod artists;
pub mod categories;
pub mod collection;
pub mod home;
pub mod identify;
pub mod library;
pub mod playback;
pub mod reco;
pub mod stats;
pub mod sync;
pub mod system;
