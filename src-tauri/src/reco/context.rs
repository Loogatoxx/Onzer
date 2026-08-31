//! Score contextuel : ce que tu écoutes **à cette heure-ci, sur cet appareil**.
//!
//! # Le problème du contexte sur peu de données
//!
//! Découper l'historique par contexte le fragmente aussitôt. Avec 24 heures ×
//! 7 jours × 3 appareils, on obtient 504 cases pour quelques centaines
//! d'écoutes : la plupart sont vides, et les rares remplies ne contiennent
//! qu'une ou deux observations. Un score calculé là-dessus serait du bruit pur.
//!
//! Deux parades, appliquées ensemble :
//!
//! **1. Peu de cases.** Cinq périodes de la journée plutôt que 24 heures. Une
//! écoute à 19 h et une à 21 h relèvent du même moment de vie ; les séparer
//! n'apporte rien et divise les données par deux.
//!
//! **2. Lissage hiérarchique.** Le score contextuel n'est pas calculé dans le
//! vide : il est **tiré vers l'affinité globale** du morceau tant que les
//! preuves manquent.
//!
//! ```text
//!                      Σ(poids × récompense dans ce contexte) + k × affinité_globale
//!   score_contextuel = ───────────────────────────────────────────────────────────────
//!                                   Σ(poids dans ce contexte) + k
//! ```
//!
//! Sans aucune écoute dans ce contexte, le score **vaut exactement l'affinité
//! globale** : le module ne dégrade jamais rien, il ne fait qu'affiner quand il
//! a de quoi.

use serde::{Deserialize, Serialize};

/// Force du lissage vers l'affinité globale.
///
/// À 2, il faut environ deux écoutes dans un contexte donné pour que celui-ci
/// commence à peser autant que l'historique général du morceau.
const CONTEXT_PRIOR_STRENGTH: f64 = 2.0;

/// Moments de la journée.
///
/// Cinq tranches calées sur des moments de vie, non sur des heures rondes :
/// c'est le découpage qui maximise les données par case tout en gardant du sens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimePeriod {
    /// 22 h – 6 h
    Night,
    /// 6 h – 10 h
    Morning,
    /// 10 h – 14 h
    Midday,
    /// 14 h – 18 h
    Afternoon,
    /// 18 h – 22 h
    Evening,
}

impl TimePeriod {
    pub fn from_hour(hour: i64) -> Self {
        match hour.rem_euclid(24) {
            6..=9 => Self::Morning,
            10..=13 => Self::Midday,
            14..=17 => Self::Afternoon,
            18..=21 => Self::Evening,
            _ => Self::Night,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Night => "la nuit",
            Self::Morning => "le matin",
            Self::Midday => "en milieu de journée",
            Self::Afternoon => "l'après-midi",
            Self::Evening => "le soir",
        }
    }
}

/// Situation d'écoute au moment de la recommandation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningContext {
    pub period: TimePeriod,
    pub is_weekend: bool,
    /// Nom du périphérique de sortie. Un casque et des enceintes traduisent
    /// deux intentions d'écoute très différentes.
    pub output_device: Option<String>,
}

impl ListeningContext {
    pub fn now(hour: i64, weekday: i64, output_device: Option<String>) -> Self {
        Self {
            period: TimePeriod::from_hour(hour),
            is_weekend: weekday >= 5,
            output_device,
        }
    }

    /// Phrase affichable, pour expliquer une playlist à l'utilisateur.
    pub fn label(&self) -> String {
        let moment = self.period.label();
        if self.is_weekend {
            format!("{moment}, le week-end")
        } else {
            format!("{moment}, en semaine")
        }
    }

    /// Une écoute passée relève-t-elle de ce contexte ?
    ///
    /// Le périphérique n'est comparé que si les deux sont connus : exiger une
    /// correspondance stricte viderait la plupart des cases.
    pub fn matches(&self, period: TimePeriod, is_weekend: bool, device: Option<&str>) -> bool {
        if period != self.period || is_weekend != self.is_weekend {
            return false;
        }

        match (self.output_device.as_deref(), device) {
            (Some(current), Some(past)) => current == past,
            _ => true,
        }
    }
}

/// Preuves accumulées dans un contexte donné, pour un morceau.
#[derive(Debug, Clone, Copy, Default)]
pub struct Evidence {
    /// Somme des récompenses pondérées par leur fraîcheur.
    pub weighted_reward: f64,
    /// Somme des poids.
    pub weight: f64,
}

impl Evidence {
    pub fn add(&mut self, reward: f64, weight: f64) {
        self.weighted_reward += reward * weight;
        self.weight += weight;
    }

    /// Récompense moyenne observée, sans lissage. `None` si aucune preuve.
    pub fn mean(&self) -> Option<f64> {
        (self.weight > 1e-9).then(|| self.weighted_reward / self.weight)
    }
}

/// Score d'un morceau dans un contexte, entre −1 et +1.
///
/// Tiré vers `global_affinity` tant que les preuves contextuelles manquent.
pub fn contextual_score(evidence: Evidence, global_affinity: f64) -> f64 {
    let numerator = evidence.weighted_reward + CONTEXT_PRIOR_STRENGTH * global_affinity;
    let denominator = evidence.weight + CONTEXT_PRIOR_STRENGTH;

    (numerator / denominator).clamp(-1.0, 1.0)
}

/// Quantité de preuves contextuelles, entre 0 et 1.
///
/// Sert à décider si le contexte mérite d'être invoqué : proposer « tu écoutes
/// ça le soir » sur la foi d'une seule écoute serait ridicule.
pub fn contextual_confidence(evidence: Evidence) -> f64 {
    evidence.weight / (evidence.weight + CONTEXT_PRIOR_STRENGTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(observations: &[(f64, f64)]) -> Evidence {
        let mut evidence = Evidence::default();
        for (reward, weight) in observations {
            evidence.add(*reward, *weight);
        }
        evidence
    }

    // ── Découpage temporel ──────────────────────────────────────────────

    #[test]
    fn les_heures_tombent_dans_la_bonne_periode() {
        assert_eq!(TimePeriod::from_hour(3), TimePeriod::Night);
        assert_eq!(TimePeriod::from_hour(7), TimePeriod::Morning);
        assert_eq!(TimePeriod::from_hour(11), TimePeriod::Midday);
        assert_eq!(TimePeriod::from_hour(15), TimePeriod::Afternoon);
        assert_eq!(TimePeriod::from_hour(20), TimePeriod::Evening);
        assert_eq!(TimePeriod::from_hour(23), TimePeriod::Night);
    }

    #[test]
    fn dix_neuf_heures_et_vingt_et_une_heures_sont_le_meme_moment_de_vie() {
        // C'est tout l'intérêt du regroupement : ne pas diviser les données
        // pour une distinction sans signification.
        assert_eq!(TimePeriod::from_hour(19), TimePeriod::from_hour(21));
    }

    #[test]
    fn les_heures_aberrantes_ne_provoquent_pas_de_panique() {
        assert_eq!(TimePeriod::from_hour(-1), TimePeriod::Night);
        assert_eq!(TimePeriod::from_hour(99), TimePeriod::Night);
    }

    #[test]
    fn le_week_end_est_deduit_du_jour() {
        assert!(!ListeningContext::now(20, 2, None).is_weekend); // mercredi
        assert!(ListeningContext::now(20, 5, None).is_weekend); // samedi
        assert!(ListeningContext::now(20, 6, None).is_weekend); // dimanche
    }

    // ── Correspondance de contexte ──────────────────────────────────────

    #[test]
    fn une_ecoute_du_meme_moment_correspond() {
        let context = ListeningContext::now(20, 1, Some("Casque".into()));

        assert!(context.matches(TimePeriod::Evening, false, Some("Casque")));
        assert!(!context.matches(TimePeriod::Morning, false, Some("Casque")));
        assert!(!context.matches(TimePeriod::Evening, true, Some("Casque")));
    }

    #[test]
    fn le_peripherique_ne_discrimine_que_sil_est_connu_des_deux_cotes() {
        let avec = ListeningContext::now(20, 1, Some("Casque".into()));
        let sans = ListeningContext::now(20, 1, None);

        assert!(!avec.matches(TimePeriod::Evening, false, Some("Enceintes")));
        assert!(
            avec.matches(TimePeriod::Evening, false, None),
            "une écoute ancienne sans appareil connu ne doit pas être écartée"
        );
        assert!(sans.matches(TimePeriod::Evening, false, Some("Enceintes")));
    }

    // ── Lissage hiérarchique : le cœur du module ────────────────────────

    #[test]
    fn sans_preuve_contextuelle_le_score_vaut_laffinite_globale() {
        // La propriété la plus importante : le contexte ne dégrade jamais rien.
        for affinite in [-0.8, -0.2, 0.0, 0.3, 0.9_f64] {
            let score = contextual_score(Evidence::default(), affinite);
            assert!(
                (score - affinite).abs() < 1e-9,
                "affinité {affinite} → score {score}"
            );
        }
    }

    #[test]
    fn une_seule_ecoute_contextuelle_ne_renverse_pas_lhistorique() {
        // Un morceau globalement mal aimé, écouté une fois en entier le soir.
        // Il doit remonter un peu, pas devenir un favori.
        let score = contextual_score(evidence(&[(1.0, 1.0)]), -0.5);

        assert!(score > -0.5, "il doit remonter : {score}");
        assert!(score < 0.2, "mais pas exploser : {score}");
    }

    #[test]
    fn des_preuves_contextuelles_repetees_finissent_par_primer() {
        let faible = contextual_score(evidence(&[(1.0, 1.0)]), -0.5);
        let fort = contextual_score(evidence(&vec![(1.0, 1.0); 20]), -0.5);

        assert!(fort > 0.7, "vingt écoutes le soir : {fort}");
        assert!(fort > faible);
    }

    #[test]
    fn un_morceau_systematiquement_rejete_dans_ce_contexte_chute() {
        // Aimé en général, mais jamais supporté le matin.
        let score = contextual_score(evidence(&[(-1.0, 1.0); 10]), 0.7);

        assert!(score < -0.5, "score obtenu {score}");
    }

    #[test]
    fn le_score_reste_toujours_borne() {
        let cas = [
            (Evidence::default(), 5.0),
            (evidence(&vec![(1.0, 100.0); 50]), 1.0),
            (evidence(&vec![(-1.0, 100.0); 50]), -1.0),
        ];

        for (preuves, affinite) in cas {
            let score = contextual_score(preuves, affinite);
            assert!((-1.0..=1.0).contains(&score), "obtenu {score}");
        }
    }

    // ── Confiance ───────────────────────────────────────────────────────

    #[test]
    fn la_confiance_contextuelle_croit_avec_les_preuves() {
        assert_eq!(contextual_confidence(Evidence::default()), 0.0);

        let une = contextual_confidence(evidence(&[(1.0, 1.0)]));
        let dix = contextual_confidence(evidence(&[(1.0, 1.0); 10]));

        assert!(dix > une);
        assert!(dix < 1.0, "la certitude absolue n'existe pas");
    }

    #[test]
    fn la_moyenne_observee_nexiste_pas_sans_observation() {
        assert_eq!(Evidence::default().mean(), None);
        assert_eq!(evidence(&[(0.8, 2.0)]).mean(), Some(0.8));
    }

    #[test]
    fn le_libelle_est_lisible() {
        assert_eq!(
            ListeningContext::now(20, 5, None).label(),
            "le soir, le week-end"
        );
        assert_eq!(
            ListeningContext::now(8, 1, None).label(),
            "le matin, en semaine"
        );
    }
}
