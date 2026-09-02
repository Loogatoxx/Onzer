//! Onzer — lecteur musical 100 % local.
//!
//! Ce crate porte toute la logique ; `main.rs` ne fait que l'appeler.
//! Découpage détaillé dans `docs/ARCHITECTURE.md`.

pub mod analysis;
pub mod audio;
pub mod commands;
pub mod core;
pub mod db;
pub mod identify;
pub mod ingest;
pub mod library;
pub mod reco;
pub mod stats;

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
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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

            // Remise en état, **avant** de rouvrir la porte aux imports.
            //
            // Le dédoublonnage ne survivait pas à une réécriture de tags : des
            // exemplaires multiples du même morceau ont pu entrer en base. La
            // passe calcule les empreintes audio manquantes puis fusionne ce
            // qui doit l'être. Sans elle, le nouveau filet resterait aveugle
            // sur toute la bibliothèque historique — et le dossier de dépôt
            // recommencerait à produire des doublons dès la seconde qui suit.
            let ingest_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
            {
                let pool = pool.clone();
                let paths = Arc::clone(&shared_paths);
                let ready = Arc::clone(&ingest_ready);

                tauri::async_runtime::spawn(async move {
                    // Le drapeau est levé quoi qu'il arrive : une réparation
                    // impossible ne doit pas condamner l'import automatique.
                    let _leve = LeveAuRetour(ready);

                    let resolver = paths.read().await.clone();
                    if !resolver.is_library_online() {
                        return;
                    }

                    match library::repair::run(&pool, &resolver).await {
                        Ok(rapport) if !rapport.is_empty() => tracing::info!(
                            empreintes = rapport.hashed,
                            fusionnes = rapport.merged,
                            fichiers_ecartes = rapport.files_set_aside,
                            tags_origine = rapport.originals_recovered,
                            ranges = rapport.refiled,
                            reindexes = rapport.reindexed,
                            "bibliothèque remise en état"
                        ),
                        Ok(_) => tracing::debug!("bibliothèque saine, rien à réparer"),
                        Err(error) => tracing::warn!(%error, "remise en état impossible"),
                    }
                });
            }

            // Import automatique : dossier surveillé et API locale. Un échec
            // ici ne compromet pas le démarrage (voir `ingest::start`).
            ingest::start(
                pool.clone(),
                Arc::clone(&shared_paths),
                &data_dir,
                ingest::server::DEFAULT_PORT,
                ingest_ready,
            );

            // Ouvrier d'analyse audio. Il travaille en fond, un morceau à la
            // fois, sur un fil bloquant dédié : l'interface et la lecture ne
            // doivent jamais le sentir.
            tauri::async_runtime::block_on(async {
                if let Err(error) = analysis::worker::requeue_outdated(&pool).await {
                    tracing::warn!(%error, "détection des analyses périmées impossible");
                }
            });
            analysis::worker::spawn(pool.clone(), Arc::clone(&shared_paths));

            // Ouvrier d'identification. Séparé du précédent à dessein :
            // l'analyse est purement locale et fonctionne hors ligne, celle-ci
            // dépend du réseau et d'une clé d'API. Les mélanger empêcherait la
            // recommandation de fonctionner sans connexion.
            identify::worker::spawn(pool.clone(), Arc::clone(&shared_paths));

            // Révision des albums écrits par une identification antérieure.
            //
            // `pick_release` retenait la parution la plus ancienne, ce qui
            // attribuait une compilation — et sa pochette — aux morceaux que
            // MusicBrainz ne connaît que par ce biais. La règle est corrigée,
            // mais elle ne défait pas ce qu'elle a déjà écrit. Cette passe s'en
            // charge, une seule fois, et en tâche de fond : elle interroge
            // MusicBrainz une fois par album.
            spawn_album_revision(pool.clone(), Arc::clone(&shared_paths));

            app.manage(AppState {
                pool,
                paths: shared_paths,
                player,
            });

            spawn_playback_loop(app.handle().clone());
            register_media_keys(app.handle());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_status,
            commands::system::media_keys_status,
            commands::system::retry_media_keys,
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
            commands::reco::start_radio,
            commands::reco::start_for_now,
            commands::reco::start_forgotten,
            commands::reco::start_loved,
            commands::reco::start_never_played,
            commands::reco::start_artist_mix,
            commands::home::home,
            commands::categories::categories,
            commands::categories::category_tracks,
            commands::categories::discover_artists,
            commands::categories::discover_tracks,
            commands::artists::list_artists,
            commands::artists::artist_tracks,
            commands::playback::enqueue_tracks,
            commands::library::remove_track,
            commands::library::offline_tracks,
            commands::library::rescue_set_aside,
            commands::library::near_duplicates,
            commands::library::ignore_duplicate_group,
            commands::sync::compare_playlist_file,
            commands::sync::compare_playlist_text,
            commands::sync::export_queries,
            commands::sync::playlist_save_command,
            commands::sync::spotify_status,
            commands::sync::set_spotify_credentials,
            commands::reco::analysis_progress,
            commands::reco::reanalyze_library,
            commands::reco::reco_diagnostics,
            commands::identify::identification_status,
            commands::identify::set_acoustid_key,
            commands::identify::retry_identifications,
            commands::identify::reidentify_library,
            commands::stats::wrapped,
            commands::collection::create_playlist,
            commands::collection::list_playlists,
            commands::collection::rename_playlist,
            commands::collection::delete_playlist,
            commands::collection::add_to_playlist,
            commands::collection::remove_from_playlist,
            commands::collection::playlist_tracks,
            commands::collection::toggle_loved,
            commands::collection::loved_tracks,
            commands::collection::track_lyrics,
            commands::collection::set_track_lyrics,
            commands::collection::fetch_lyrics,
            commands::collection::sync_lyrics,
            commands::collection::adopt_sidecars,
            commands::collection::lyrics_progress,
            commands::collection::artwork_progress,
            commands::collection::fetch_missing_artwork,
            commands::collection::suspect_tracks,
            commands::collection::restore_original_tags,
            commands::collection::correct_track,
            commands::collection::set_playlist_cover,
            commands::collection::clear_playlist_cover,
            commands::collection::set_playlist_description,
            commands::collection::track_note,
            commands::collection::set_track_note,
            commands::metadata::metadata_candidates,
            commands::metadata::apply_candidate,
            commands::metadata::missing_albums,
            commands::metadata::fill_missing_albums,
            commands::metadata::candidate_preview,
            commands::preferences::preferences,
            commands::preferences::set_online_completion,
            commands::preferences::set_auto_identification,
            commands::preferences::rebuild_library,
            commands::whisper::whisper_status,
            commands::whisper::listen_and_sync,
            commands::whisper::stop_listening,
            commands::whisper::sync_track,
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
/// Lève un drapeau à la sortie de portée, quel que soit le chemin emprunté.
///
/// Un `return` anticipé ou une erreur ne doit pas laisser l'import automatique
/// fermé pour toute la session.
struct LeveAuRetour(Arc<std::sync::atomic::AtomicBool>);

impl Drop for LeveAuRetour {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// Lance la révision des albums, si elle n'a pas déjà été appliquée.
fn spawn_album_revision(pool: SqlitePool, paths: Arc<RwLock<PathResolver>>) {
    tauri::async_runtime::spawn(async move {
        let applied: Option<i64> = db::settings::get(&pool, db::settings::ALBUMS_REVISION)
            .await
            .ok()
            .flatten();

        if applied.unwrap_or(0) >= identify::revise::VERSION {
            return;
        }

        let resolver = paths.read().await.clone();
        if !resolver.is_library_online() {
            return; // on réessaiera au prochain démarrage
        }

        let Ok(musicbrainz) = identify::musicbrainz::MusicBrainzClient::new() else {
            return;
        };

        match identify::revise::run(&pool, &resolver, &musicbrainz).await {
            Ok(report) => {
                tracing::info!(
                    examines = report.examined,
                    effaces = report.cleared,
                    morceaux = report.tracks_touched,
                    "albums révisés"
                );

                // Marquée seulement en cas de succès : une révision
                // interrompue par une coupure réseau doit être rejouée.
                let _ = db::settings::set(
                    &pool,
                    db::settings::ALBUMS_REVISION,
                    &identify::revise::VERSION,
                )
                .await;
            }
            Err(error) => tracing::warn!(%error, "révision des albums impossible"),
        }
    });
}

/// Branche les touches multimédia du clavier.
///
/// # Pourquoi un raccourci global et non un événement de la page
///
/// Sur un clavier Apple, F7, F8 et F9 émettent des événements **système** de
/// contrôle de lecture, pas des frappes ordinaires. La page web ne les voit
/// jamais. L'API `MediaSession` du navigateur ne les capterait pas davantage :
/// elle suppose que le son sort de la page, alors qu'il sort du moteur Rust.
///
/// Le raccourci global est donc le seul chemin. Sa contrepartie est assumée :
/// il fonctionne même quand Onzer n'est pas au premier plan — ce qui est très
/// exactement ce qu'on attend d'une touche de lecture.
pub fn register_media_keys(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

    let shortcuts = [
        (Code::MediaPlayPause, MediaAction::Toggle),
        (Code::MediaTrackNext, MediaAction::Next),
        (Code::MediaTrackPrevious, MediaAction::Previous),
    ];

    for (code, action) in shortcuts {
        let shortcut = Shortcut::new(Some(Modifiers::empty()), code);
        let handle = app.clone();

        let outcome = app.global_shortcut().on_shortcut(shortcut, move |_, _, event| {
            // Une pression émet deux événements ; n'agir qu'au relâchement
            // évite de sauter deux morceaux d'un seul appui.
            if event.state() != ShortcutState::Released {
                return;
            }

            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = action.run(&handle).await {
                    tracing::warn!(%error, "touche multimédia sans effet");
                }
            });
        });

        if let Err(error) = outcome {
            // Une touche refusée n'est pas une raison d'empêcher le démarrage.
            // Mais elle ne doit pas non plus rester enterrée dans un journal :
            // l'utilisateur appuierait sur F8 sans rien comprendre. La raison
            // est conservée pour être affichée.
            tracing::warn!(%error, ?code, "touche multimédia non enregistrée");
            commands::system::set_media_keys_error(error.to_string());
            return;
        }
    }

    commands::system::set_media_keys_error(String::new());
}

/// Ce qu'une touche multimédia déclenche.
#[derive(Debug, Clone, Copy)]
enum MediaAction {
    Toggle,
    Next,
    Previous,
}

impl MediaAction {
    async fn run(self, app: &tauri::AppHandle) -> core::Result<()> {
        let state = app.state::<AppState>();
        let player = state.player()?;

        match self {
            Self::Toggle => player.toggle().await?,
            Self::Next | Self::Previous => {
                let paths = state.paths.read().await.clone();
                if matches!(self, Self::Next) {
                    player.next(&state.pool, &paths, false).await?;
                } else {
                    player.previous(&state.pool, &paths).await?;
                }
            }
        }

        // L'interface doit suivre : sans cet événement, la barre de lecture
        // continuerait d'afficher le morceau précédent.
        let _ = app.emit(commands::playback::STATE_EVENT, player.snapshot().await);
        Ok(())
    }
}

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
