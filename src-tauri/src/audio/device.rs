//! Thread audio dédié.
//!
//! # Pourquoi un thread, et pas simplement un champ dans l'état partagé
//!
//! Le flux `cpal` n'est **pas `Send` sur macOS** : il ne peut ni traverser une
//! frontière de thread ni vivre dans un état partagé entre tâches async. Il est
//! donc confiné à un thread système unique qui le possède du début à la fin.
//!
//! La communication se fait dans les deux sens, sans jamais bloquer :
//!
//! ```text
//!   commandes (canal)  ─────────────►  ┌──────────────┐
//!                                      │ thread audio │  possède le flux cpal
//!   état (atomiques)   ◄─────────────  └──────────────┘
//! ```
//!
//! L'état remonte par des entiers atomiques plutôt que par un mutex : le reste
//! de l'application lit la position de lecture soixante fois par seconde, et ne
//! doit jamais pouvoir faire attendre l'audio.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::core::{OnzerError, Result};

/// Cadence de rafraîchissement de l'état publié. 50 ms suffisent à une barre
/// de progression fluide sans occuper le processeur.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
enum Command {
    Play(PathBuf),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetVolume(f32),
}

/// État publié par le thread audio, lisible sans verrou.
#[derive(Debug, Default)]
struct SharedState {
    position_ms: AtomicI64,
    is_playing: AtomicBool,
    /// Passe à vrai quand le morceau s'achève **de lui-même**. Le chef
    /// d'orchestre le consomme pour enchaîner et clore l'écoute.
    reached_end: AtomicBool,
    /// Le décodage a échoué : fichier corrompu ou format non géré.
    failed: AtomicBool,
}

/// Poignée vers le thread audio.
#[derive(Clone)]
pub struct AudioDevice {
    commands: Sender<Command>,
    shared: Arc<SharedState>,
}

impl AudioDevice {
    /// Démarre le thread audio et ouvre le périphérique de sortie par défaut.
    pub fn start() -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::new(SharedState::default());

        // Le résultat d'ouverture revient par un canal : on veut échouer tout
        // de suite si aucune carte son n'est disponible, plutôt que découvrir
        // le problème au premier morceau.
        let (ready_tx, ready_rx) = mpsc::channel();

        let thread_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("onzer-audio".to_string())
            .spawn(move || audio_thread(receiver, thread_shared, ready_tx))
            .map_err(|error| OnzerError::Invalid(format!("thread audio : {error}")))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                commands: sender,
                shared,
            }),
            Ok(Err(message)) => Err(OnzerError::Invalid(message)),
            Err(_) => Err(OnzerError::Invalid(
                "le thread audio n'a pas démarré".to_string(),
            )),
        }
    }

    /// Charge et lance un fichier. Remplace ce qui était en cours.
    pub fn play(&self, path: PathBuf) {
        self.shared.reached_end.store(false, Ordering::Release);
        self.shared.failed.store(false, Ordering::Release);
        self.send(Command::Play(path));
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    pub fn stop(&self) {
        self.shared.reached_end.store(false, Ordering::Release);
        self.send(Command::Stop);
    }

    pub fn seek(&self, position: Duration) {
        // La position cible est publiée **avant** d'envoyer la commande.
        //
        // Le thread audio traite les commandes de façon asynchrone : sans cette
        // ligne, l'instantané renvoyé à l'interface juste après un déplacement
        // contiendrait encore l'ancienne position, et le curseur reviendrait
        // visiblement en arrière avant de sauter au bon endroit.
        self.shared
            .position_ms
            .store(position.as_millis() as i64, Ordering::Release);

        self.send(Command::Seek(position));
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(Command::SetVolume(volume.clamp(0.0, 1.0)));
    }

    pub fn position_ms(&self) -> i64 {
        self.shared.position_ms.load(Ordering::Acquire)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.is_playing.load(Ordering::Acquire)
    }

    /// Consomme le signal de fin naturelle : vrai une seule fois par morceau.
    pub fn take_reached_end(&self) -> bool {
        self.shared.reached_end.swap(false, Ordering::AcqRel)
    }

    /// Consomme le signal d'échec de décodage.
    pub fn take_failed(&self) -> bool {
        self.shared.failed.swap(false, Ordering::AcqRel)
    }

    /// Une commande perdue signifie que le thread audio est mort : l'ignorer
    /// est préférable à faire paniquer l'interface.
    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("thread audio injoignable");
        }
    }
}

/// Boucle du thread audio. Possède le flux `cpal` pour toute sa durée de vie.
fn audio_thread(
    commands: Receiver<Command>,
    shared: Arc<SharedState>,
    ready: Sender<std::result::Result<(), String>>,
) {
    // # Pourquoi une panique est rattrapée ici
    //
    // Ouvrir un périphérique passe par du code natif — CoreAudio, AAudio.
    // Quand son environnement n'est pas celui qu'il attend, il ne rend pas une
    // erreur : il **panique**. Le fil meurt alors sans rien dire, et
    // l'application ne voit qu'un canal fermé : « le thread audio n'a pas
    // démarré », sans la moindre cause. Sur un téléphone dont le constructeur
    // chiffre les journaux, c'est un mur.
    let ouverture = std::panic::catch_unwind(rodio::DeviceSinkBuilder::open_default_sink);

    let sink = match ouverture {
        Ok(Ok(sink)) => sink,
        Ok(Err(error)) => {
            let _ = ready.send(Err(format!("périphérique audio indisponible : {error}")));
            return;
        }
        Err(panique) => {
            let cause = panique
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panique.downcast_ref::<&str>().map(|texte| (*texte).to_string()))
                .unwrap_or_else(|| "cause inconnue".to_string());

            let _ = ready.send(Err(format!("le pilote audio a paniqué : {cause}")));
            return;
        }
    };

    let player = rodio::Player::connect_new(sink.mixer());
    let _ = ready.send(Ok(()));

    // Vrai entre le lancement d'un morceau et sa fin : évite de signaler une
    // fin de lecture alors que rien n'a jamais été lancé.
    let mut playing_something = false;

    loop {
        // Force une publication de la position même en pause, lorsqu'une
        // commande vient de la déplacer délibérément (saut, nouveau morceau).
        let mut position_moved = false;

        // L'attente bornée sert de cadence de rafraîchissement : le thread
        // réagit immédiatement à une commande, et publie son état sinon.
        match commands.recv_timeout(POLL_INTERVAL) {
            Ok(Command::Play(path)) => {
                player.clear();
                position_moved = true;

                match open_decoder(&path) {
                    Ok(source) => {
                        player.append(source);
                        player.play();
                        playing_something = true;
                    }
                    Err(error) => {
                        tracing::warn!(fichier = %path.display(), %error, "décodage impossible");
                        shared.failed.store(true, Ordering::Release);
                        playing_something = false;
                    }
                }
            }
            Ok(Command::Pause) => player.pause(),
            Ok(Command::Resume) => player.play(),
            Ok(Command::Stop) => {
                player.clear();
                playing_something = false;
                shared.position_ms.store(0, Ordering::Release);
            }
            Ok(Command::Seek(position)) => {
                position_moved = true;
                if let Err(error) = player.try_seek(position) {
                    // Certains formats ne savent pas se déplacer. On le signale
                    // sans interrompre la lecture.
                    tracing::warn!(%error, "saut impossible sur ce format");
                }
            }
            Ok(Command::SetVolume(volume)) => player.set_volume(volume),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // L'émetteur a été détruit : l'application se ferme.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let paused = player.is_paused();

        // En pause, la position ne peut plus bouger. La republier laisserait
        // pourtant l'horloge de l'interface tressauter de quelques
        // millisecondes, car `get_pos` suit l'écoulement du tampon de sortie et
        // non l'intention de l'utilisateur.
        if !paused || position_moved {
            shared
                .position_ms
                .store(player.get_pos().as_millis() as i64, Ordering::Release);
        }

        shared
            .is_playing
            .store(!paused && !player.empty(), Ordering::Release);

        // File vide alors qu'un morceau tournait : il s'est terminé seul.
        if playing_something && player.empty() {
            playing_something = false;
            shared.reached_end.store(true, Ordering::Release);
        }
    }
}

/// Ouvre un décodeur **capable de se déplacer**.
///
/// # Le défaut que ce choix corrige
///
/// `Decoder::new` construit un décodeur sans déclarer la source seekable ni sa
/// taille. Le déplacement échouait alors sur chaque saut, avec un laconique
/// « Symphonia decoder returned an error » dans les journaux : cliquer en
/// arrière dans la barre de progression ne faisait rien.
///
/// `TryFrom<File>` renseigne les deux — c'est d'ailleurs ce que la
/// documentation de rodio recommande explicitement pour un fichier. Le décodage
/// reste en flux depuis le disque : un FLAC de 40 Mo n'est pas chargé en
/// mémoire pour être lu.
fn open_decoder(path: &PathBuf) -> Result<rodio::Decoder<std::io::BufReader<std::fs::File>>> {
    let file = std::fs::File::open(path)?;

    rodio::Decoder::try_from(file)
        .map_err(|error| OnzerError::Invalid(format!("format non géré : {error}")))
}
