//! Onzer — lecteur musical 100 % local.
//!
//! Ce crate porte toute la logique ; `main.rs` ne fait que l'appeler.
//! Découpage détaillé dans `docs/ARCHITECTURE.md`.

pub mod commands;
pub mod core;
pub mod db;
pub mod library;

use std::path::PathBuf;

use sqlx::SqlitePool;
use tauri::Manager;
use tokio::sync::RwLock;

use crate::core::PathResolver;

/// État partagé, injecté dans chaque commande par Tauri.
pub struct AppState {
    pub pool: SqlitePool,
    /// Sous verrou : la racine de bibliothèque change quand l'utilisateur la
    /// configure ou quand le SSD est rebranché.
    pub paths: RwLock<PathResolver>,
}

pub fn run() {
    init_tracing();

    tauri::Builder::default()
        // Sélecteur de dossier natif, pour choisir la racine de bibliothèque.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // `app_data_dir` pointe vers ~/Library/Application Support/Onzer
            // sur macOS. On ne code jamais ce chemin en dur.
            let data_dir: PathBuf = app.path().app_data_dir()?;

            let mut paths = PathResolver::new(data_dir);
            paths.ensure_dirs()?;

            let database_file = paths.database_file();
            tracing::info!(base = %database_file.display(), "ouverture de la base");

            // Le `setup` de Tauri est synchrone : on bloque le temps d'ouvrir
            // la base et d'appliquer les migrations. C'est volontaire —
            // l'application ne doit pas démarrer sur un schéma incertain.
            let pool = tauri::async_runtime::block_on(async {
                let pool = db::connect(&database_file).await?;
                db::migrate(&pool).await?;

                // Restaure la racine de bibliothèque choisie lors d'une
                // session précédente.
                let saved_root: Option<String> =
                    db::settings::get(&pool, db::settings::LIBRARY_ROOT).await?;

                Ok::<_, core::OnzerError>((pool, saved_root))
            });

            let (pool, saved_root) = pool?;

            if let Some(root) = saved_root {
                paths.set_library_root(Some(PathBuf::from(root)));

                if paths.is_library_online() {
                    tracing::info!("bibliothèque en ligne");
                } else {
                    // Cas nominal, pas une erreur : le SSD est simplement
                    // débranché. L'application reste pleinement utilisable.
                    tracing::warn!("bibliothèque hors ligne — volume non monté");
                }
            }

            app.manage(AppState {
                pool,
                paths: RwLock::new(paths),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_status,
            commands::library::set_library_root,
            commands::library::import_folder,
            commands::library::list_tracks,
            commands::library::search_tracks,
            commands::library::library_counts,
            commands::library::refresh_availability,
            commands::library::artwork_data_uri,
        ])
        .run(tauri::generate_context!())
        .expect("échec au lancement d'Onzer");
}

/// Journalisation. Le niveau est pilotable par la variable d'environnement
/// `RUST_LOG` (ex. `RUST_LOG=onzer_lib=trace`).
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("onzer_lib=debug,warn"));

    fmt().with_env_filter(filter).with_target(true).init();
}
