//! Onzer — lecteur musical 100 % local.
//!
//! Ce crate porte toute la logique ; `main.rs` ne fait que l'appeler.
//! Découpage détaillé dans `docs/ARCHITECTURE.md`.

pub mod analysis;
pub mod audio;
#[cfg(target_os = "android")]
mod android;
pub mod commands;
pub mod core;
pub mod db;
pub mod identify;
pub mod ingest;
pub mod library;
pub mod reco;
pub mod stats;
pub mod sync;

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

/// Pourquoi le moteur audio n'a pas démarré.
///
/// # Pourquoi la raison est conservée
///
/// « Aucun périphérique audio disponible » ne dit pas **pourquoi**, et sur un
/// téléphone dont le constructeur chiffre les journaux système, c'est tout ce
/// qu'on peut voir. La cause exacte — celle que rodio a rendue — est donc
/// gardée pour être affichée. Même leçon qu'avec les touches multimédia
/// (ADR-030) : un défaut qui ne quitte pas les journaux est un défaut qu'on ne
/// peut pas corriger.
static RAISON_AUDIO: std::sync::OnceLock<String> = std::sync::OnceLock::new();

impl AppState {
    pub fn player(&self) -> core::Result<&PlayerService> {
        self.player.as_ref().ok_or_else(|| {
            let raison = RAISON_AUDIO
                .get()
                .map(|raison| format!(" ({raison})"))
                .unwrap_or_default();

            OnzerError::Invalid(format!("aucun périphérique audio disponible{raison}"))
        })
    }
}

/// Point d'entrée de l'application.
///
/// # Pourquoi cette annotation
///
/// Sur un bureau, `main` appelle cette fonction. Sur Android il n'y a pas de
/// `main` : c'est la machine virtuelle Java qui charge la bibliothèque native
/// et cherche un symbole convenu. `mobile_entry_point` le produit — sans lui,
/// l'édition de liens réussit et l'application ne démarre jamais.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    // La chaîne est coupée ici pour laisser entrer un greffon qui n'existe pas
    // partout : les raccourcis globaux n'ont pas d'équivalent sur mobile, où
    // c'est le système qui possède les commandes de lecture.
    let builder = tauri::Builder::default()
        // Sélecteur de dossier natif, pour choisir la racine de bibliothèque.
        .plugin(tauri_plugin_dialog::init());

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    builder
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
                    let _ = RAISON_AUDIO.set(error.to_string());
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

            #[cfg(target_os = "android")]
            android::retenir_la_poignee(app.handle().clone());

            spawn_playback_loop(app.handle().clone());
            // Les touches multimédia n'existent que sur un bureau : sur
            // mobile, ce sont les commandes du système qui pilotent la
            // lecture, par un tout autre mécanisme.
            #[cfg(desktop)]
            crate::register_media_keys(app.handle());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_status,
            commands::system::media_keys_status,
            commands::system::retry_media_keys,
            commands::system::suggested_roots,
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
            commands::playback::play_next,
            commands::playback::remove_from_queue,
            commands::playback::move_in_queue,
            commands::playback::resume_playback,
            commands::playback::set_sleep_timer,
            commands::playback::sleep_timer,
            commands::appairage::open_pairing,
            commands::appairage::link_command,
            commands::appairage::link_open,
            commands::appairage::stop_link,
            commands::appairage::close_pairing,
            commands::appairage::pairing_open,
            commands::appairage::sync_with_device,
            commands::appairage::fetch_missing_files,
            commands::appairage::read_pairing_link,
            commands::appairage::sync_journal,
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
            commands::artists::artist_albums,
            commands::playback::enqueue_tracks,
            commands::library::remove_track,
            commands::library::offline_tracks,
            commands::library::tracks_by_ids,
            commands::library::album_tracks,
            commands::library::list_albums,
            commands::library::listening_history,
            commands::library::unavailable_tracks,
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
            commands::preferences::set_display_name,
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
        // La révision interroge MusicBrainz et **réécrit des albums** : c'est
        // de l'identification automatique, quel que soit le nom qu'on lui
        // donne. L'utilisateur qui a éteint l'interrupteur voyait pourtant ses
        // albums changer au démarrage, sans rien avoir demandé.
        if !commands::preferences::auto_identification(&pool)
            .await
            .unwrap_or(true)
        {
            return;
        }

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
#[cfg(desktop)]
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
#[cfg(desktop)]
#[derive(Debug, Clone, Copy)]
enum MediaAction {
    Toggle,
    Next,
    Previous,
}

#[cfg(desktop)]
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

/// Écart au-delà duquel une position n'est plus le simple écoulement du temps.
///
/// Une seconde et demie : trois battements de la boucle. En dessous, on
/// republierait pour du bruit — le décodeur rend une position qui suit le
/// tampon de sortie, et non l'horloge.
#[cfg(target_os = "android")]
const SAUT_MINIMAL_MS: i64 = 1_500;

fn spawn_playback_loop(handle: tauri::AppHandle) {
    use commands::playback::{PlaybackTick, STATE_EVENT, TICK_EVENT};

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(PLAYBACK_TICK);
        let mut last_tick = PlaybackTick {
            position_ms: -1,
            is_playing: false,
        };

        // Le morceau que le système d'exploitation croit entendre.
        //
        // # Le défaut que ça corrige
        //
        // L'état n'était poussé vers Android qu'à deux moments : une fin de
        // morceau atteinte toute seule, et un changement lecture/pause.
        // **Passer au suivant volontairement** n'est ni l'un ni l'autre :
        // cela vient d'une commande, qui change le morceau sans que la boucle
        // n'ait rien à en dire.
        //
        // La notification restait donc sur le morceau précédent, avec sa
        // durée à lui, jusqu'à ce qu'un appui la réveille. Comparer
        // l'identifiant couvre tous les chemins d'un coup — commande, fin
        // naturelle, saut dans la file — au lieu d'en énumérer trois et d'en
        // oublier un quatrième.
        #[cfg(target_os = "android")]
        let mut dernier_publie: Option<i64> = None;

        // La position telle qu'Android la croit. Elle sert à repérer un
        // **déplacement** : la session multimédia porte une vitesse, et le
        // système fait avancer son compteur tout seul à partir de la dernière
        // position publiée. Tant qu'on lui laisse celle du début, il continue
        // de compter à partir de là — les secondes de l'écran verrouillé
        // ignoraient donc tout saut dans le morceau.
        #[cfg(target_os = "android")]
        let mut position_publiee: i64 = -1;

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

            #[cfg(target_os = "android")]
            {
                let courant = player.current_track_id().await;
                let position = player.position_ms();

                // Ce que la position devrait valoir si rien d'autre que le
                // temps ne s'était écoulé depuis la dernière publication.
                let attendue = position_publiee + PLAYBACK_TICK.as_millis() as i64;
                let deplacement = position_publiee >= 0
                    && (position - attendue).abs() > SAUT_MINIMAL_MS;

                if courant != dernier_publie || deplacement {
                    dernier_publie = courant;
                    publier_vers_android(&handle, &player).await;
                }

                position_publiee = position;
            }

            let tick = PlaybackTick {
                position_ms: player.position_ms(),
                is_playing: player.is_playing(),
            };

            if tick.position_ms != last_tick.position_ms || tick.is_playing != last_tick.is_playing
            {
                // Le système d'exploitation a besoin du morceau, pas de la
                // position : republier la pochette quatre fois par seconde
                // ferait travailler la machine virtuelle pour rien. On ne
                // pousse qu'au changement de morceau ou d'état.
                #[cfg(target_os = "android")]
                if tick.is_playing != last_tick.is_playing {
                    publier_vers_android(&handle, &player).await;
                }

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

/// Donne au système d'exploitation de quoi peupler l'écran verrouillé.
///
/// # Pourquoi la pochette est réduite ici
///
/// L'image rangée par Onzer fait jusqu'à mille pixels de côté. L'écran
/// verrouillé n'en montre qu'une vignette, et la faire franchir la frontière
/// JNI à taille réelle coûterait un mégaoctet à chaque morceau. On envoie la
/// miniature, déjà produite pour l'interface.
#[cfg(target_os = "android")]
async fn publier_vers_android(handle: &tauri::AppHandle, player: &audio::PlayerService) {
    let snapshot = player.snapshot().await;
    let Some(current) = snapshot.current.as_ref() else {
        return;
    };

    let state = handle.state::<AppState>();
    let artwork_dir = state.paths.read().await.artwork_dir();

    let pochette = current
        .artwork_hash
        .as_deref()
        .map(|hash| library::artwork::thumbnail_path(&artwork_dir, hash))
        .filter(|path| path.is_file())
        .and_then(|path| std::fs::read(path).ok())
        .map(|octets| library::artwork::encode_base64(&octets))
        .unwrap_or_default();

    android::pousser_letat(
        &current.title,
        current.artist.as_deref().unwrap_or(""),
        snapshot.is_playing,
        snapshot.position_ms,
        snapshot.duration_ms,
        &pochette,
    );
}

#[cfg(test)]
mod garde_jni {
    //! Le pont JNI ne peut pas se vérifier à la compilation.
    //!
    //! Rust cherche une classe par son nom et une méthode par sa signature, au
    //! moment de l'appel. Ni le compilateur Rust, ni le compilateur Kotlin, ni
    //! R8 ne voient ce lien : chacun a raison de son côté, et l'application
    //! meurt à l'exécution. Ce test lit les deux bouts et vérifie qu'ils se
    //! tiennent.

    /// Les classes que le cœur touche par JNI, lues dans le pont lui-même.
    fn classes_appelees(source: &str) -> Vec<String> {
        let mut noms = Vec::new();

        // `find_class("com/loogatoxx/onzer/X")`
        for morceau in source.split("find_class(\"").skip(1) {
            if let Some(chemin) = morceau.split('"').next() {
                if let Some(nom) = chemin.strip_prefix("com/loogatoxx/onzer/") {
                    noms.push(nom.to_string());
                }
            }
        }

        // `Java_com_loogatoxx_onzer_X_maMethode` — l'autre sens du pont.
        for morceau in source.split("Java_com_loogatoxx_onzer_").skip(1) {
            if let Some(nom) = morceau.split('_').next() {
                noms.push(nom.to_string());
            }
        }

        noms.sort();
        noms.dedup();
        noms
    }

    #[test]
    fn toute_classe_appelee_depuis_rust_est_preservee_de_r8() {
        let racine = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        let pont = std::fs::read_to_string(racine.join("src/android.rs"))
            .expect("le pont JNI doit être lisible");

        let mut regles = String::new();
        let dossier = racine.join("gen/android/app");
        for entree in std::fs::read_dir(&dossier).expect("le projet Android doit exister") {
            let chemin = entree.expect("entrée lisible").path();

            // exFAT sème des jumeaux `._nom` à côté de chaque fichier : ce sont
            // des blocs binaires, pas des règles, et les lire fait échouer le
            // test pour une raison qui n'a rien à voir avec ce qu'il vérifie.
            let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if nom.starts_with("._") {
                continue;
            }

            if chemin.extension().is_some_and(|e| e == "pro") {
                regles.push_str(&std::fs::read_to_string(&chemin).expect("règle lisible"));
            }
        }

        let classes = classes_appelees(&pont);
        assert!(
            !classes.is_empty(),
            "aucune classe trouvée : l'analyse du pont ne repère plus rien"
        );

        for classe in classes {
            // La règle doit préserver **tous** les membres : garder la classe
            // sans ses méthodes laisse exactement le même défaut, en plus
            // discret.
            let attendue = format!("-keep class com.loogatoxx.onzer.{classe} {{ *; }}");
            assert!(
                regles.contains(&attendue),
                "{classe} est appelée depuis Rust mais R8 peut la renommer.\n\
                 Ajouter dans gen/android/app/proguard-onzer.pro :\n  {attendue}"
            );
        }
    }
}
