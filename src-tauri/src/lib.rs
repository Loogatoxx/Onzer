//! Onzer — lecteur musical 100 % local.
//!
//! Ce crate porte toute la logique ; `main.rs` ne fait que l'appeler.
//! Découpage détaillé dans `docs/ARCHITECTURE.md`.

pub mod analysis;
pub mod audio;
pub mod commands;
pub mod core;
pub mod db;
pub mod ingest;
pub mod library;
pub mod reco;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;

use crate::audio::PlayerService;
use crate::core::{OnzerError, PathResolver};

/// Cadence de la boucle de surveillance de la lecture.
///
/// 250 ms suffisent à enchaîner sans blanc perceptible et à animer la barre de
/// progression, tout en restant imperceptible côté processeur.
const PLAYBACK_TICK: Duration = Duration::from_millis(250);

/// État partagé, injecté dans chaque commande par Tauri.
pub struct AppState {
    pub pool: SqlitePool,
    /// Sous verrou partagé : la racine de bibliothèque change quand
    /// l'utilisateur la configure ou quand le SSD est rebranché, et les tâches
    /// d'import automatique doivent voir ce changement sans redémarrage.
    pub paths: Arc<RwLock<PathResolver>>,
    /// `None` si aucun périphérique audio n'a pu être ouvert. L'application
    /// reste alors pleinement utilisable pour gérer la bibliothèque — seule la
    /// lecture est indisponible.
    player: Option<PlayerService>,
}

impl AppState {
    pub fn player(&self) -> core::Result<&PlayerService> {
        self.player.as_ref().ok_or_else(|| {
            OnzerError::Invalid("aucun périphérique audio disponible".to_string())
        })
    }
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
            let opened = tauri::async_runtime::block_on(async {
                let pool = db::connect(&database_file).await?;
                db::migrate(&pool).await?;

                // Restaure la racine de bibliothèque choisie lors d'une
                // session précédente.
                let saved_root: Option<String> =
                    db::settings::get(&pool, db::settings::LIBRARY_ROOT).await?;

                Ok::<_, core::OnzerError>((pool, saved_root))
            });

            let (pool, saved_root) = opened?;

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

            // L'absence de carte son ne doit pas empêcher l'application de
            // démarrer : on dégrade au lieu d'échouer.
            let player = match tauri::async_runtime::block_on(PlayerService::start(&pool)) {
                Ok(service) => {
                    tracing::info!("moteur audio prêt");
                    Some(service)
                }
                Err(error) => {
                    tracing::error!(%error, "moteur audio indisponible");
                    None
                }
            };

            let data_dir = paths.data_dir().to_path_buf();
            let shared_paths = Arc::new(RwLock::new(paths));

            // Import automatique : dossier surveillé et API locale. Un échec
            // ici ne compromet pas le démarrage (voir `ingest::start`).
            ingest::start(
                pool.clone(),
                Arc::clone(&shared_paths),
                &data_dir,
                ingest::server::DEFAULT_PORT,
            );

            app.manage(AppState {
                pool,
                paths: shared_paths,
                player,
            });

            spawn_playback_loop(app.handle().clone());

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
            commands::playback::play_tracks,
            commands::playback::toggle_playback,
            commands::playback::next_track,
            commands::playback::previous_track,
            commands::playback::jump_in_queue,
            commands::playback::seek_to,
            commands::playback::set_volume,
            commands::playback::set_repeat,
            commands::playback::set_shuffle,
            commands::playback::stop_playback,
            commands::playback::playback_state,
        ])
        .run(tauri::generate_context!())
        .expect("échec au lancement d'Onzer");
}

/// Boucle de surveillance de la lecture.
///
/// Deux rôles bien distincts :
///
/// * détecter les fins de morceau et les échecs de décodage pour **enchaîner**
///   — c'est aussi le moment où l'écoute est journalisée ;
/// * publier la position pour animer l'interface.
///
/// Le battement de position n'est émis que lorsque quelque chose bouge : une
/// application au repos ne doit rien coûter.
fn spawn_playback_loop(handle: tauri::AppHandle) {
    use commands::playback::{PlaybackTick, STATE_EVENT, TICK_EVENT};

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(PLAYBACK_TICK);
        let mut last_tick = PlaybackTick {
            position_ms: -1,
            is_playing: false,
        };

        loop {
            interval.tick().await;

            let state = handle.state::<AppState>();
            let Ok(player) = state.player() else {
                continue; // pas de carte son : rien à surveiller
            };

            let paths = state.paths.read().await.clone();

            match player.tick(&state.pool, &paths).await {
                Ok(true) => {
                    let _ = handle.emit(STATE_EVENT, player.snapshot().await);
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "enchaînement interrompu"),
            }

            let tick = PlaybackTick {
                position_ms: player.position_ms(),
                is_playing: player.is_playing(),
            };

            if tick.position_ms != last_tick.position_ms || tick.is_playing != last_tick.is_playing
            {
                last_tick = tick;
                let _ = handle.emit(TICK_EVENT, tick);
            }
        }
    });
}

/// Journalisation. Le niveau est pilotable par la variable d'environnement
/// `RUST_LOG` (ex. `RUST_LOG=onzer_lib=trace`).
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("onzer_lib=debug,warn"));

    fmt().with_env_filter(filter).with_target(true).init();
}
