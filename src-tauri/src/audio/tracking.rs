//! Journal d'écoute : la machine à états qui nourrit la recommandation.
//!
//! **C'est le module le plus important du projet pour l'IA**, et il ne contient
//! volontairement aucune ligne d'audio. Il reçoit des évènements horodatés
//! (démarrage, pause, reprise, saut, fin) et produit une ligne de `play_events`.
//!
//! Le temps est toujours **injecté**, jamais lu depuis l'horloge : c'est ce qui
//! permet de tester une écoute de trois minutes en une microseconde, et de
//! vérifier des comportements impossibles à reproduire à la main.
//!
//! ## La subtilité centrale : trois durées à ne pas confondre
//!
//! | Durée | Définition | Piège |
//! |---|---|---|
//! | Temps écoulé | `fin - début` | Inclurait une pause de trois heures |
//! | **Temps écouté** | Somme des périodes de lecture réelle | C'est le seul qui mesure l'intérêt |
//! | **Position** | Où l'on se trouve dans le morceau | Modifiée par les sauts, pas par les pauses |
//!
//! Un utilisateur qui saute en arrière pour réécouter un passage a un *temps
//! écouté* supérieur à sa *position finale*. C'est un signal d'appréciation
//! fort, et il serait perdu en ne mesurant que la position.

use chrono::{Datelike, Local, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

/// Sous les 15 secondes, un saut traduit un rejet franc plutôt qu'un simple
/// changement d'humeur. Seuil repris dans `track_stats.early_skip_count`.
pub const EARLY_SKIP_THRESHOLD_MS: i64 = 15_000;

/// Marge de fin de morceau. Passer au suivant dans les deux dernières secondes
/// n'est pas un rejet : le morceau a été écouté en entier.
const COMPLETION_GRACE_MS: i64 = 2_000;

/// D'où vient l'écoute. C'est **la seule boucle de qualité du moteur** :
/// comparer le taux de complétion des morceaux proposés par l'IA à celui des
/// morceaux choisis à la main est la seule façon de savoir si elle est bonne.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaySource {
    Library,
    Playlist,
    Radio,
    Reco,
    Search,
    Queue,
    Shuffle,
}

impl PlaySource {
    /// Doit correspondre exactement à la contrainte `CHECK` de `play_events`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Playlist => "playlist",
            Self::Radio => "radio",
            Self::Reco => "reco",
            Self::Search => "search",
            Self::Queue => "queue",
            Self::Shuffle => "shuffle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Le morceau est allé jusqu'au bout.
    Completed,
    /// L'utilisateur est passé au suivant.
    Skipped,
    /// Lecture arrêtée sans enchaîner.
    Stopped,
    /// Un autre morceau a été lancé à la place.
    Replaced,
    /// Échec de décodage ou fichier disparu.
    Error,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Stopped => "stopped",
            Self::Replaced => "replaced",
            Self::Error => "error",
        }
    }
}

/// Contexte d'une écoute, figé à son démarrage.
#[derive(Debug, Clone)]
pub struct PlayContext {
    pub track_id: i64,
    /// Instantané de la durée : le fichier peut être remplacé plus tard.
    pub duration_ms: i64,
    pub source: PlaySource,
    pub source_id: Option<i64>,
    /// Alimente la matrice de transitions : ce qui suit bien quoi.
    pub previous_track_id: Option<i64>,
    /// L'utilisateur a-t-il choisi ce morceau, ou s'est-il enchaîné seul ?
    /// Chercher activement un titre est un signal bien plus fort.
    pub was_manual_selection: bool,
    /// Casque, enceintes, AirPods… deux intentions d'écoute différentes.
    pub output_device: Option<String>,
    pub volume: f32,
}

/// Ligne prête à être insérée dans `play_events`.
#[derive(Debug, Clone)]
pub struct PlayEventRecord {
    pub track_id: i64,
    pub started_at: i64,
    pub ended_at: i64,
    pub listened_ms: i64,
    pub duration_ms: i64,
    pub completion: f64,
    pub end_reason: EndReason,
    /// Renseigné **uniquement** en cas de saut. Position exacte au moment du
    /// rejet : trois secondes et deux minutes ne veulent pas dire la même chose.
    pub skip_at_ms: Option<i64>,
    pub seek_count: i64,
    pub pause_count: i64,
    pub source: PlaySource,
    pub source_id: Option<i64>,
    pub previous_track_id: Option<i64>,
    pub was_manual_selection: bool,
    pub output_device: Option<String>,
    pub volume: f32,
    pub hour_local: i64,
    pub weekday: i64,
    pub is_weekend: bool,
}

impl PlayEventRecord {
    /// Un saut avant le seuil précoce vaut rejet franc.
    pub fn is_early_skip(&self) -> bool {
        self.end_reason == EndReason::Skipped
            && self.skip_at_ms.is_some_and(|at| at < EARLY_SKIP_THRESHOLD_MS)
    }
}

/// Écoute en cours.
#[derive(Debug)]
pub struct TrackedPlay {
    context: PlayContext,
    started_at: i64,
    /// Temps de lecture réel accumulé, pauses exclues.
    listened_ms: i64,
    /// Position dans le morceau au dernier point de mesure.
    position_ms: i64,
    /// Instant de la dernière reprise. `None` si en pause.
    playing_since: Option<i64>,
    seek_count: i64,
    pause_count: i64,
}

impl TrackedPlay {
    pub fn start(context: PlayContext, now: i64) -> Self {
        Self {
            context,
            started_at: now,
            listened_ms: 0,
            position_ms: 0,
            playing_since: Some(now),
            seek_count: 0,
            pause_count: 0,
        }
    }

    pub fn track_id(&self) -> i64 {
        self.context.track_id
    }

    pub fn is_paused(&self) -> bool {
        self.playing_since.is_none()
    }

    /// Reporte le temps écoulé depuis le dernier point de mesure sur le temps
    /// écouté **et** sur la position. Appelé avant tout changement d'état.
    fn settle(&mut self, now: i64) {
        if let Some(since) = self.playing_since {
            let elapsed = (now - since).max(0);
            self.listened_ms += elapsed;
            self.position_ms += elapsed;
            self.playing_since = Some(now);
        }
    }

    pub fn pause(&mut self, now: i64) {
        if self.playing_since.is_none() {
            return; // déjà en pause : ne pas gonfler le compteur
        }
        self.settle(now);
        self.playing_since = None;
        self.pause_count += 1;
    }

    pub fn resume(&mut self, now: i64) {
        if self.playing_since.is_none() {
            self.playing_since = Some(now);
        }
    }

    /// Déplacement dans le morceau. Le temps déjà écouté est conservé : sauter
    /// en arrière pour réécouter un passage **augmente** le temps écouté, ce
    /// qui est précisément le signal d'appréciation que l'on veut capter.
    pub fn seek(&mut self, to_ms: i64, now: i64) {
        self.settle(now);
        self.position_ms = to_ms.clamp(0, self.context.duration_ms);
        self.seek_count += 1;
    }

    /// Position actuelle dans le morceau.
    pub fn position_ms(&self, now: i64) -> i64 {
        let extra = match self.playing_since {
            Some(since) => (now - since).max(0),
            None => 0,
        };
        (self.position_ms + extra).min(self.context.duration_ms)
    }

    /// Clôt l'écoute et produit la ligne de journal.
    pub fn finish(mut self, now: i64, reason: EndReason) -> PlayEventRecord {
        self.settle(now);

        let position = self.position_ms.min(self.context.duration_ms);

        // Passer au suivant dans les dernières secondes n'est pas un rejet.
        let reason = match reason {
            EndReason::Skipped
                if position >= self.context.duration_ms - COMPLETION_GRACE_MS
                    && self.context.duration_ms > COMPLETION_GRACE_MS =>
            {
                EndReason::Completed
            }
            other => other,
        };

        let completion = if self.context.duration_ms > 0 {
            (self.listened_ms as f64 / self.context.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let (hour_local, weekday, is_weekend) = local_context(self.started_at);

        PlayEventRecord {
            track_id: self.context.track_id,
            started_at: self.started_at,
            ended_at: now,
            listened_ms: self.listened_ms,
            duration_ms: self.context.duration_ms,
            completion,
            end_reason: reason,
            skip_at_ms: (reason == EndReason::Skipped).then_some(position),
            seek_count: self.seek_count,
            pause_count: self.pause_count,
            source: self.context.source,
            source_id: self.context.source_id,
            previous_track_id: self.context.previous_track_id,
            was_manual_selection: self.context.was_manual_selection,
            output_device: self.context.output_device,
            volume: self.context.volume,
            hour_local,
            weekday,
            is_weekend,
        }
    }
}

/// Décompose un horodatage UTC en contexte **local** : heure, jour, week-end.
///
/// Ces trois valeurs sont dénormalisées en base à dessein : recalculer un
/// fuseau horaire sur des centaines de milliers de lignes à chaque requête de
/// statistiques serait absurde. Et c'est bien l'heure locale qui compte —
/// « j'écoute ça le soir » n'a aucun sens en UTC.
fn local_context(timestamp_ms: i64) -> (i64, i64, bool) {
    let Some(moment) = Local.timestamp_millis_opt(timestamp_ms).single() else {
        return (0, 0, false);
    };

    let hour = i64::from(moment.hour());
    // `num_days_from_monday` donne 0 pour lundi, ce qui correspond à la
    // convention retenue dans le schéma.
    let weekday = i64::from(moment.weekday().num_days_from_monday());

    (hour, weekday, weekday >= 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUREE: i64 = 200_000; // 3 min 20

    fn contexte() -> PlayContext {
        PlayContext {
            track_id: 1,
            duration_ms: DUREE,
            source: PlaySource::Library,
            source_id: None,
            previous_track_id: None,
            was_manual_selection: true,
            output_device: Some("Casque".to_string()),
            volume: 0.8,
        }
    }

    fn lecture() -> TrackedPlay {
        TrackedPlay::start(contexte(), 0)
    }

    // ── Temps écouté ────────────────────────────────────────────────────

    #[test]
    fn une_ecoute_complete_donne_une_completion_de_1() {
        let record = lecture().finish(DUREE, EndReason::Completed);

        assert_eq!(record.listened_ms, DUREE);
        assert!((record.completion - 1.0).abs() < 1e-9);
        assert_eq!(record.skip_at_ms, None);
    }

    #[test]
    fn une_pause_ne_compte_pas_comme_du_temps_ecoute() {
        let mut play = lecture();

        play.pause(30_000); // 30 s écoutées
        play.resume(3_630_000); // reprise une heure plus tard
        let record = play.finish(3_660_000, EndReason::Stopped); // +30 s

        assert_eq!(
            record.listened_ms, 60_000,
            "l'heure de pause ne doit pas être comptée"
        );
        assert_eq!(record.ended_at - record.started_at, 3_660_000);
    }

    #[test]
    fn une_pause_repetee_ne_gonfle_pas_le_compteur() {
        let mut play = lecture();
        play.pause(10_000);
        play.pause(11_000);
        play.pause(12_000);

        assert_eq!(play.finish(13_000, EndReason::Stopped).pause_count, 1);
    }

    #[test]
    fn reecouter_un_passage_augmente_le_temps_ecoute() {
        let mut play = lecture();

        play.seek(0, 60_000); // retour au début après 1 min
        let record = play.finish(120_000, EndReason::Stopped); // 1 min de plus

        assert_eq!(
            record.listened_ms, 120_000,
            "deux minutes ont bien été écoutées"
        );
        assert_eq!(record.seek_count, 1);
    }

    // ── Position et sauts ───────────────────────────────────────────────

    #[test]
    fn la_position_avance_avec_la_lecture() {
        let play = lecture();
        assert_eq!(play.position_ms(45_000), 45_000);
    }

    #[test]
    fn la_position_se_fige_en_pause() {
        let mut play = lecture();
        play.pause(20_000);

        assert_eq!(
            play.position_ms(999_000),
            20_000,
            "en pause, la position ne bouge plus"
        );
    }

    #[test]
    fn un_saut_deplace_la_position_sans_effacer_le_temps_ecoute() {
        let mut play = lecture();
        play.seek(150_000, 10_000);

        assert_eq!(play.position_ms(10_000), 150_000);
        let record = play.finish(10_000, EndReason::Stopped);
        assert_eq!(record.listened_ms, 10_000);
    }

    #[test]
    fn la_position_ne_depasse_jamais_la_duree() {
        let play = lecture();
        assert_eq!(play.position_ms(DUREE * 10), DUREE);
    }

    // ── Sauts : le signal clé de la recommandation ──────────────────────

    #[test]
    fn un_saut_enregistre_sa_position_exacte() {
        let record = lecture().finish(3_000, EndReason::Skipped);

        assert_eq!(record.end_reason, EndReason::Skipped);
        assert_eq!(record.skip_at_ms, Some(3_000));
    }

    #[test]
    fn distingue_un_rejet_franc_dun_changement_dhumeur() {
        let rejet = lecture().finish(3_000, EndReason::Skipped);
        let humeur = lecture().finish(120_000, EndReason::Skipped);

        assert!(rejet.is_early_skip(), "3 s = « je déteste ce son »");
        assert!(!humeur.is_early_skip(), "2 min = « pas maintenant »");
    }

    #[test]
    fn passer_au_suivant_en_toute_fin_nest_pas_un_rejet() {
        // L'utilisateur anticipe la fin d'une seconde : compter cela comme un
        // rejet fausserait durablement le score du morceau.
        let record = lecture().finish(DUREE - 1_000, EndReason::Skipped);

        assert_eq!(record.end_reason, EndReason::Completed);
        assert_eq!(record.skip_at_ms, None);
    }

    #[test]
    fn une_fin_normale_ne_porte_pas_de_position_de_saut() {
        for reason in [EndReason::Completed, EndReason::Stopped, EndReason::Error] {
            assert_eq!(lecture().finish(50_000, reason).skip_at_ms, None);
        }
    }

    // ── Robustesse ──────────────────────────────────────────────────────

    #[test]
    fn la_completion_reste_bornee_malgre_les_reecoutes() {
        let mut play = lecture();
        for tour in 1..=3 {
            play.seek(0, DUREE * tour);
        }
        let record = play.finish(DUREE * 4, EndReason::Completed);

        assert!(record.listened_ms > DUREE, "le morceau a bien été bouclé");
        assert!(
            record.completion <= 1.0,
            "la complétion ne doit jamais dépasser 1, obtenu {}",
            record.completion
        );
    }

    #[test]
    fn une_duree_nulle_ne_provoque_pas_de_division_par_zero() {
        let mut context = contexte();
        context.duration_ms = 0;

        let record = TrackedPlay::start(context, 0).finish(1_000, EndReason::Error);
        assert_eq!(record.completion, 0.0);
    }

    #[test]
    fn le_contexte_est_conserve_jusquau_journal() {
        let mut context = contexte();
        context.previous_track_id = Some(42);
        context.source = PlaySource::Reco;
        context.source_id = Some(7);
        context.was_manual_selection = false;

        let record = TrackedPlay::start(context, 0).finish(1_000, EndReason::Stopped);

        assert_eq!(record.previous_track_id, Some(42));
        assert_eq!(record.source, PlaySource::Reco);
        assert_eq!(record.source_id, Some(7));
        assert!(!record.was_manual_selection);
        assert_eq!(record.output_device.as_deref(), Some("Casque"));
    }

    #[test]
    fn le_contexte_horaire_est_calcule_en_heure_locale() {
        let record = lecture().finish(1_000, EndReason::Stopped);

        assert!((0..24).contains(&record.hour_local));
        assert!((0..7).contains(&record.weekday));
        assert_eq!(record.is_weekend, record.weekday >= 5);
    }

    #[test]
    fn les_libelles_correspondent_aux_contraintes_du_schema() {
        // Toute divergence ferait échouer une contrainte CHECK à l'insertion.
        assert_eq!(PlaySource::Reco.as_str(), "reco");
        assert_eq!(PlaySource::Shuffle.as_str(), "shuffle");
        assert_eq!(EndReason::Completed.as_str(), "completed");
        assert_eq!(EndReason::Replaced.as_str(), "replaced");
    }
}
