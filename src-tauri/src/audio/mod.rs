//! Moteur de lecture.
//!
//! Trois pièces, volontairement séparées :
//!
//! | Pièce | Rôle | Testable sans son |
//! |---|---|---|
//! | [`device`] | Possède le flux `cpal`, sur son thread | Non |
//! | [`queue`] | File de lecture, aléatoire, répétition | **Oui** |
//! | [`tracking`] | Journal d'écoute qui nourrit la reco | **Oui** |
//!
//! [`PlayerService`] est le chef d'orchestre : il traduit les intentions de
//! l'utilisateur en commandes pour le périphérique, fait avancer la file, et
//! veille à ce que **chaque écoute soit journalisée** — y compris celles qui
//! s'interrompent sur une erreur.

pub mod device;
pub mod queue;
pub mod tracking;

use std::path::PathBuf;

use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::core::{OnzerError, PathResolver, Result};
use crate::db::events;
use crate::library::importer;

use device::AudioDevice;
use queue::{Queue, QueueItem, RepeatMode};
use tracking::{EndReason, PlayContext, PlaySource, TrackedPlay};

/// Instantané envoyé à l'interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub current: Option<QueueItem>,
    pub queue: Vec<QueueItem>,
    pub queue_index: Option<usize>,
    pub is_playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub volume: f32,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

struct PlayerState {
    session_id: String,
    /// Écoute en cours de mesure. `None` quand rien ne joue.
    current: Option<TrackedPlay>,
    queue: Queue,
    volume: f32,
    /// Dernier morceau joué, pour alimenter `previous_track_id`.
    last_track_id: Option<i64>,
    /// Comment le morceau courant a été lancé.
    source: PlaySource,
    source_id: Option<i64>,
}

pub struct PlayerService {
    device: AudioDevice,
    state: Mutex<PlayerState>,
    /// Nom du périphérique de sortie, lu une fois au démarrage.
    /// Casque ou enceintes : deux intentions d'écoute différentes.
    output_device: Option<String>,
}

impl PlayerService {
    pub async fn start(pool: &SqlitePool) -> Result<Self> {
        let device = AudioDevice::start()?;
        let session_id = events::resume_or_open_session(pool).await?;

        Ok(Self {
            device,
            output_device: current_output_device(),
            state: Mutex::new(PlayerState {
                session_id,
                current: None,
                queue: Queue::default(),
                volume: 1.0,
                last_track_id: None,
                source: PlaySource::Library,
                source_id: None,
            }),
        })
    }

    // ── Commandes ───────────────────────────────────────────────────────

    /// Remplace la file et démarre la lecture.
    pub async fn play_queue(
        &self,
        pool: &SqlitePool,
        paths: &PathResolver,
        items: Vec<QueueItem>,
        start_at: usize,
        source: PlaySource,
        source_id: Option<i64>,
    ) -> Result<()> {
        if items.is_empty() {
            return Err(OnzerError::Invalid("file de lecture vide".to_string()));
        }

        let mut state = self.state.lock().await;

        // L'écoute en cours est remplacée : elle doit être journalisée avant
        // d'être oubliée, sinon la donnée est perdue pour toujours.
        self.close_current(pool, &mut state, EndReason::Replaced).await;

        state.source = source;
        state.source_id = source_id;
        state.queue.replace(items, start_at);

        // Choix explicite de l'utilisateur : signal d'affinité fort.
        self.start_current(pool, paths, &mut state, true).await
    }

    /// Ajoute des morceaux à la fin de la file, sans rien interrompre.
    ///
    /// # Pourquoi cela ne relance pas la lecture
    ///
    /// « Ajouter à la file » et « écouter maintenant » sont deux gestes
    /// différents, et les confondre est une des façons les plus sûres d'agacer :
    /// on empile trois titres pour plus tard, et le troisième coupe celui qu'on
    /// écoutait.
    ///
    /// Quand rien ne joue, en revanche, la file vide n'a pas de « plus tard » :
    /// on démarre.
    pub async fn enqueue(
        &self,
        pool: &SqlitePool,
        paths: &PathResolver,
        items: Vec<QueueItem>,
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        let mut state = self.state.lock().await;
        let was_empty = state.queue.is_empty();

        for item in items {
            state.queue.enqueue(item);
        }

        if was_empty {
            state.queue.jump_to(0);
            state.source = PlaySource::Queue;
            return self.start_current(pool, paths, &mut state, true).await;
        }

        Ok(())
    }

    /// Met en pause, sans jamais relancer.
    ///
    /// # Pourquoi ce n'est pas `toggle`
    ///
    /// Le minuteur de sommeil arrive quand il arrive : si l'écoute s'est déjà
    /// arrêtée entre-temps, une bascule la **relancerait** — le réveil parfait
    /// pour qui s'endormait.
    pub async fn pause(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        let Some(play) = state.current.as_mut() else {
            return Ok(());
        };

        if !play.is_paused() {
            play.pause(crate::core::now_ms());
            self.device.pause();
        }

        Ok(())
    }

    /// Bascule lecture / pause.
    pub async fn toggle(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        let Some(play) = state.current.as_mut() else {
            return Ok(());
        };

        let now = crate::core::now_ms();
        if play.is_paused() {
            play.resume(now);
            self.device.resume();
        } else {
            play.pause(now);
            self.device.pause();
        }

        Ok(())
    }

    /// Passe au morceau suivant. `automatic` distingue une fin naturelle d'un
    /// appui volontaire — ce qui change à la fois la file (répétition d'un
    /// morceau) et le journal (fin normale contre saut).
    pub async fn next(
        &self,
        pool: &SqlitePool,
        paths: &PathResolver,
        automatic: bool,
    ) -> Result<()> {
        let mut state = self.state.lock().await;

        let reason = if automatic {
            EndReason::Completed
        } else {
            EndReason::Skipped
        };
        self.close_current(pool, &mut state, reason).await;

        if state.queue.advance(automatic).is_none() {
            self.device.stop();
            return Ok(());
        }

        // Enchaînement : l'utilisateur n'a pas choisi ce morceau.
        self.start_current(pool, paths, &mut state, false).await
    }

    pub async fn previous(&self, pool: &SqlitePool, paths: &PathResolver) -> Result<()> {
        let mut state = self.state.lock().await;

        self.close_current(pool, &mut state, EndReason::Skipped).await;

        if state.queue.previous().is_none() {
            return Ok(());
        }

        self.start_current(pool, paths, &mut state, true).await
    }

    /// Lance un morceau précis de la file.
    pub async fn jump_to(
        &self,
        pool: &SqlitePool,
        paths: &PathResolver,
        index: usize,
    ) -> Result<()> {
        let mut state = self.state.lock().await;

        self.close_current(pool, &mut state, EndReason::Replaced).await;

        if state.queue.jump_to(index).is_none() {
            return Err(OnzerError::Invalid("position hors de la file".to_string()));
        }

        self.start_current(pool, paths, &mut state, true).await
    }

    pub async fn seek(&self, position_ms: i64) -> Result<()> {
        let mut state = self.state.lock().await;
        let Some(play) = state.current.as_mut() else {
            return Ok(());
        };

        let position = position_ms.max(0);
        play.seek(position, crate::core::now_ms());
        self.device
            .seek(std::time::Duration::from_millis(position as u64));

        Ok(())
    }

    pub async fn set_volume(&self, volume: f32) -> Result<()> {
        let volume = volume.clamp(0.0, 1.0);
        self.state.lock().await.volume = volume;
        self.device.set_volume(volume);
        Ok(())
    }

    pub async fn set_repeat(&self, mode: RepeatMode) -> Result<()> {
        self.state.lock().await.queue.set_repeat(mode);
        Ok(())
    }

    pub async fn set_shuffle(&self, shuffle: bool) -> Result<()> {
        self.state.lock().await.queue.set_shuffle(shuffle);
        Ok(())
    }

    /// Arrête la lecture et clôt l'écoute en cours.
    pub async fn stop(&self, pool: &SqlitePool) -> Result<()> {
        let mut state = self.state.lock().await;
        self.close_current(pool, &mut state, EndReason::Stopped).await;
        self.device.stop();
        Ok(())
    }

    // ── Boucle de surveillance ──────────────────────────────────────────

    /// Appelé périodiquement. Détecte les fins de morceau et les échecs de
    /// décodage, puis enchaîne. Retourne `true` si l'état a changé, ce qui
    /// évite d'inonder l'interface d'événements identiques.
    pub async fn tick(&self, pool: &SqlitePool, paths: &PathResolver) -> Result<bool> {
        if self.device.take_failed() {
            let mut state = self.state.lock().await;
            self.close_current(pool, &mut state, EndReason::Error).await;

            // Un fichier illisible ne doit pas arrêter l'écoute : on passe au
            // suivant, exactement comme s'il s'était terminé.
            if state.queue.advance(true).is_some() {
                self.start_current(pool, paths, &mut state, false).await?;
            } else {
                self.device.stop();
            }
            return Ok(true);
        }

        if self.device.take_reached_end() {
            self.next(pool, paths, true).await?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Périphérique de sortie courant. Élément du contexte d'écoute :
    /// un casque et des enceintes traduisent deux intentions différentes.
    pub fn output_device(&self) -> Option<&str> {
        self.output_device.as_deref()
    }

    /// Position de lecture, lue sans verrou. Appelée quatre fois par seconde
    /// par la boucle de surveillance : elle ne doit jamais attendre.
    pub fn position_ms(&self) -> i64 {
        self.device.position_ms()
    }

    /// Le morceau en cours, sans construire tout l'instantané.
    ///
    /// # Pourquoi cet accès existe
    ///
    /// La boucle de surveillance a besoin de savoir, quatre fois par seconde,
    /// si le morceau a changé. `snapshot` répondrait — en clonant la file
    /// entière, soit deux mille éléments pour comparer un entier.
    pub async fn current_track_id(&self) -> Option<i64> {
        self.state.lock().await.current.as_ref().map(|play| play.track_id())
    }

    pub fn is_playing(&self) -> bool {
        self.device.is_playing()
    }

    pub async fn snapshot(&self) -> PlaybackSnapshot {
        let state = self.state.lock().await;
        let current = state.queue.current().cloned();

        PlaybackSnapshot {
            duration_ms: current.as_ref().map_or(0, |item| item.duration_ms),
            current,
            queue: state.queue.items().to_vec(),
            queue_index: state.queue.current_index(),
            is_playing: self.device.is_playing(),
            position_ms: self.device.position_ms(),
            volume: state.volume,
            repeat: state.queue.repeat(),
            shuffle: state.queue.shuffle(),
        }
    }

    // ── Interne ─────────────────────────────────────────────────────────

    /// Journalise l'écoute en cours, quelle qu'en soit la raison de fin.
    ///
    /// Un échec d'écriture est enregistré dans les journaux mais **n'interrompt
    /// pas la lecture** : perdre un événement est fâcheux, couper la musique de
    /// l'utilisateur l'est davantage.
    async fn close_current(&self, pool: &SqlitePool, state: &mut PlayerState, reason: EndReason) {
        let Some(play) = state.current.take() else {
            return;
        };

        let track_id = play.track_id();
        let event = play.finish(crate::core::now_ms(), reason);

        if let Err(error) = events::record(pool, &state.session_id, &event).await {
            tracing::error!(%error, track_id, "écoute non journalisée");
        }

        state.last_track_id = Some(track_id);
    }

    /// Démarre le morceau courant de la file.
    async fn start_current(
        &self,
        pool: &SqlitePool,
        paths: &PathResolver,
        state: &mut PlayerState,
        manual: bool,
    ) -> Result<()> {
        let Some(item) = state.queue.current().cloned() else {
            return Ok(());
        };

        let path: PathBuf = importer::absolute_path(paths, &item.relative_path)?;

        if !path.is_file() {
            // Le fichier a disparu : on le signale en base plutôt que de
            // laisser l'utilisateur face à un silence inexpliqué.
            let _ = sqlx::query("UPDATE tracks SET is_available = 0 WHERE id = ?")
                .bind(item.track_id)
                .execute(pool)
                .await;

            return Err(OnzerError::Invalid(format!(
                "fichier introuvable : {}",
                item.relative_path
            )));
        }

        self.device.set_volume(state.volume);
        self.device.play(path);

        state.current = Some(TrackedPlay::start(
            PlayContext {
                track_id: item.track_id,
                duration_ms: item.duration_ms,
                source: state.source,
                source_id: state.source_id,
                previous_track_id: state.last_track_id,
                was_manual_selection: manual,
                output_device: self.output_device.clone(),
                volume: state.volume,
            },
            crate::core::now_ms(),
        ));

        Ok(())
    }
}

/// Nom du périphérique de sortie par défaut, si le système le donne.
///
/// Ce n'est pas une coquetterie : « AirPods Pro » et « Haut-parleurs du
/// MacBook » traduisent deux intentions d'écoute très différentes, et cette
/// colonne servira à conditionner la recommandation.
fn current_output_device() -> Option<String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let device = rodio::cpal::default_host().default_output_device()?;
    let description = device.description().ok()?;

    Some(description.name().to_string())
}

#[cfg(test)]
mod tests_playback;
