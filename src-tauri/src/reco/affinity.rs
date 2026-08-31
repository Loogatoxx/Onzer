//! Score d'affinité : combien tu aimes un morceau, aujourd'hui.
//!
//! # Les deux erreurs que ce module évite
//!
//! **Erreur n° 1 — traiter toutes les écoutes également.**
//! Un morceau adoré il y a deux ans ne dit plus grand-chose de tes goûts
//! actuels. Chaque écoute est donc pondérée par une décroissance exponentielle,
//! de demi-vie 30 jours : une écoute d'il y a un mois pèse moitié moins qu'une
//! écoute d'hier.
//!
//! **Erreur n° 2 — croire une écoute unique.**
//! C'est l'erreur la plus destructrice sur un petit historique. Un morceau
//! écouté une seule fois jusqu'au bout obtiendrait un score parfait de 1,0 et
//! écraserait un morceau écouté cinquante fois avec 95 % de complétion. Le
//! classement serait dominé par le bruit.
//!
//! La parade est le **rétrécissement bayésien** : on ajoute au calcul quelques
//! écoutes fictives neutres. Un morceau doit accumuler des preuves pour
//! s'écarter de la moyenne.
//!
//! ```text
//!                 Σ(poids × récompense) + force_du_prior × valeur_du_prior
//!   affinité  =  ─────────────────────────────────────────────────────────
//!                          Σ(poids) + force_du_prior
//! ```

use crate::audio::tracking::{EndReason, EARLY_SKIP_THRESHOLD_MS};

/// Demi-vie de la pondération temporelle.
///
/// 30 jours : assez long pour qu'un morceau aimé survive à quelques semaines
/// sans écoute, assez court pour que les goûts puissent évoluer.
pub const HALF_LIFE_DAYS: f64 = 30.0;

/// Nombre d'écoutes neutres fictives ajoutées à chaque morceau.
///
/// C'est le curseur de prudence. À 3, un morceau écouté une fois en entier
/// obtient 0,25 au lieu de 1,0 — il est prometteur, pas encore prouvé.
pub const PRIOR_STRENGTH: f64 = 3.0;

/// Valeur de ces écoutes fictives : neutre.
const PRIOR_VALUE: f64 = 0.0;

/// Une écoute choisie explicitement vaut davantage qu'une écoute subie.
///
/// Aller chercher un morceau est une déclaration d'intention ; le laisser
/// passer dans une file n'en est pas une.
const MANUAL_SELECTION_WEIGHT: f64 = 1.6;

/// Un arrêt de lecture juge moins le morceau que la fin d'une session.
const STOPPED_WEIGHT: f64 = 0.4;

/// Bonus appliqué à un morceau explicitement marqué comme aimé.
const LOVED_BONUS: f64 = 0.35;

/// Une écoute, réduite à ce qui sert au calcul.
#[derive(Debug, Clone, Copy)]
pub struct PlaySignal {
    /// Fraction du morceau réellement écoutée, entre 0 et 1.
    pub completion: f64,
    pub end_reason: EndReason,
    pub skip_at_ms: Option<i64>,
    pub was_manual_selection: bool,
    /// Ancienneté de l'écoute, en jours.
    pub age_days: f64,
}

/// Récompense d'une écoute, entre −1 et +1.
///
/// La complétion est convertie linéairement : écouter la moitié d'un morceau
/// est un signal neutre, en écouter 90 % est positif, l'abandonner au bout de
/// trois secondes est franchement négatif.
pub fn reward(signal: &PlaySignal) -> f64 {
    match signal.end_reason {
        // Un échec de décodage ne dit rien des goûts de l'utilisateur.
        EndReason::Error => 0.0,

        EndReason::Completed => 1.0,

        EndReason::Skipped => {
            let base = (2.0 * signal.completion - 1.0).clamp(-1.0, 1.0);

            // Un rejet dans les premières secondes est plus catégorique que ne
            // le suggère la seule complétion : sur un morceau de six minutes,
            // trois secondes représentent 0,8 %, mais l'intention est nette.
            match signal.skip_at_ms {
                Some(position) if position < EARLY_SKIP_THRESHOLD_MS => base.min(-0.85),
                _ => base,
            }
        }

        EndReason::Stopped | EndReason::Replaced => {
            (2.0 * signal.completion - 1.0).clamp(-1.0, 1.0)
        }
    }
}

/// Poids d'une écoute : sa fraîcheur, modulée par la force du signal.
pub fn weight(signal: &PlaySignal) -> f64 {
    let freshness = 0.5_f64.powf(signal.age_days.max(0.0) / HALF_LIFE_DAYS);

    let intent = match signal.end_reason {
        EndReason::Error => 0.0,
        EndReason::Stopped => STOPPED_WEIGHT,
        _ if signal.was_manual_selection => MANUAL_SELECTION_WEIGHT,
        _ => 1.0,
    };

    freshness * intent
}

/// Affinité d'un morceau, entre −1 et +1.
///
/// `is_loved` applique un bonus explicite : un morceau mis en favori doit
/// remonter même s'il a été peu écouté récemment.
pub fn affinity(signals: &[PlaySignal], is_loved: bool) -> f64 {
    let mut weighted_sum = PRIOR_STRENGTH * PRIOR_VALUE;
    let mut total_weight = PRIOR_STRENGTH;

    for signal in signals {
        let w = weight(signal);
        weighted_sum += w * reward(signal);
        total_weight += w;
    }

    let score = weighted_sum / total_weight;
    let bonus = if is_loved { LOVED_BONUS } else { 0.0 };

    (score + bonus).clamp(-1.0, 1.0)
}

/// Quantité de preuves accumulées sur un morceau, entre 0 et 1.
///
/// Sert à distinguer « je sais qu'il ne l'aime pas » de « je ne sais rien ».
/// Un morceau jamais écouté a une confiance nulle, ce qui doit orienter vers
/// l'exploration plutôt que vers l'exclusion.
pub fn confidence(signals: &[PlaySignal]) -> f64 {
    let evidence: f64 = signals.iter().map(weight).sum();
    evidence / (evidence + PRIOR_STRENGTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ecoute(completion: f64, reason: EndReason, age_days: f64) -> PlaySignal {
        PlaySignal {
            completion,
            end_reason: reason,
            skip_at_ms: None,
            was_manual_selection: false,
            age_days,
        }
    }

    fn rejet_immediat(age_days: f64) -> PlaySignal {
        PlaySignal {
            completion: 0.02,
            end_reason: EndReason::Skipped,
            skip_at_ms: Some(3_000),
            was_manual_selection: false,
            age_days,
        }
    }

    // ── Récompense ──────────────────────────────────────────────────────

    #[test]
    fn une_ecoute_complete_vaut_la_recompense_maximale() {
        assert_eq!(reward(&ecoute(1.0, EndReason::Completed, 0.0)), 1.0);
    }

    #[test]
    fn un_rejet_immediat_vaut_la_recompense_minimale() {
        assert!(reward(&rejet_immediat(0.0)) <= -0.85);
    }

    #[test]
    fn un_saut_a_la_moitie_est_neutre() {
        let signal = ecoute(0.5, EndReason::Skipped, 0.0);
        assert!(reward(&signal).abs() < 1e-9);
    }

    #[test]
    fn un_saut_tardif_reste_positif() {
        // Écouter 90 % d'un morceau avant de passer au suivant n'est pas un
        // rejet.
        let mut signal = ecoute(0.9, EndReason::Skipped, 0.0);
        signal.skip_at_ms = Some(180_000);

        assert!(reward(&signal) > 0.5);
    }

    #[test]
    fn un_rejet_precoce_est_juge_severement_meme_sur_un_long_morceau() {
        // 3 s sur 6 min : la complétion vaut 0,008, donc la formule linéaire
        // donnerait déjà −0,98. Le plancher garantit que ce soit toujours le
        // cas, quelle que soit la durée.
        let mut signal = ecoute(0.008, EndReason::Skipped, 0.0);
        signal.skip_at_ms = Some(3_000);

        assert!(reward(&signal) <= -0.85);
    }

    #[test]
    fn une_erreur_de_lecture_ne_juge_pas_le_morceau() {
        assert_eq!(reward(&ecoute(0.0, EndReason::Error, 0.0)), 0.0);
        assert_eq!(weight(&ecoute(0.0, EndReason::Error, 0.0)), 0.0);
    }

    // ── Décroissance temporelle ─────────────────────────────────────────

    #[test]
    fn une_ecoute_dun_mois_pese_moitie_moins() {
        let hier = weight(&ecoute(1.0, EndReason::Completed, 0.0));
        let mois = weight(&ecoute(1.0, EndReason::Completed, HALF_LIFE_DAYS));

        assert!((mois / hier - 0.5).abs() < 1e-6);
    }

    #[test]
    fn une_ecoute_tres_ancienne_ne_pese_presque_plus_rien() {
        let poids = weight(&ecoute(1.0, EndReason::Completed, 365.0));
        assert!(poids < 0.001, "poids obtenu {poids}");
    }

    #[test]
    fn un_gout_recent_supplante_un_gout_ancien() {
        let ancien = affinity(&vec![ecoute(1.0, EndReason::Completed, 400.0); 20], false);
        let recent = affinity(&[ecoute(1.0, EndReason::Completed, 1.0)], false);

        assert!(
            recent > ancien,
            "récent {recent}, ancien {ancien} — les goûts doivent pouvoir évoluer"
        );
    }

    // ── Rétrécissement bayésien : le cœur du module ─────────────────────

    #[test]
    fn une_seule_ecoute_ne_donne_pas_un_score_parfait() {
        let score = affinity(&[ecoute(1.0, EndReason::Completed, 0.0)], false);

        assert!(
            score < 0.4,
            "une écoute unique ne prouve rien : score obtenu {score}"
        );
    }

    #[test]
    fn la_repetition_finit_par_convaincre() {
        let une = affinity(&[ecoute(1.0, EndReason::Completed, 0.0)], false);
        let vingt = affinity(&vec![ecoute(1.0, EndReason::Completed, 0.0); 20], false);

        assert!(vingt > 0.8, "vingt écoutes complètes : {vingt}");
        assert!(vingt > une * 2.0);
    }

    #[test]
    fn un_morceau_eprouve_bat_un_morceau_chanceux() {
        // LE cas qui compte : sans rétrécissement, le morceau écouté une fois
        // gagnerait, ce qui rendrait tout classement absurde.
        let chanceux = affinity(&[ecoute(1.0, EndReason::Completed, 0.0)], false);

        let mut eprouve = vec![ecoute(1.0, EndReason::Completed, 0.0); 40];
        eprouve.push(ecoute(0.4, EndReason::Skipped, 0.0)); // pas parfait

        assert!(
            affinity(&eprouve, false) > chanceux,
            "quarante écoutes doivent l'emporter sur une seule"
        );
    }

    #[test]
    fn un_morceau_jamais_ecoute_est_neutre() {
        assert_eq!(affinity(&[], false), 0.0);
    }

    #[test]
    fn un_morceau_systematiquement_rejete_devient_negatif() {
        let score = affinity(&vec![rejet_immediat(0.0); 10], false);
        assert!(score < -0.5, "score obtenu {score}");
    }

    // ── Intention et favoris ────────────────────────────────────────────

    #[test]
    fn un_choix_delibere_pese_plus_quune_ecoute_subie() {
        let subie = ecoute(1.0, EndReason::Completed, 0.0);
        let mut choisie = subie;
        choisie.was_manual_selection = true;

        assert!(weight(&choisie) > weight(&subie));
        assert!(affinity(&[choisie], false) > affinity(&[subie], false));
    }

    #[test]
    fn un_arret_de_session_juge_moins_severement_quun_saut() {
        let arret = ecoute(0.3, EndReason::Stopped, 0.0);
        let saut = ecoute(0.3, EndReason::Skipped, 0.0);

        // Même récompense négative, mais un poids plus faible : fermer
        // l'application n'est pas un avis sur le morceau.
        assert!(weight(&arret) < weight(&saut));
    }

    #[test]
    fn un_favori_remonte_meme_peu_ecoute() {
        let signals = [ecoute(1.0, EndReason::Completed, 0.0)];

        assert!(affinity(&signals, true) > affinity(&signals, false));
    }

    #[test]
    fn le_score_reste_toujours_borne() {
        let cas: Vec<Vec<PlaySignal>> = vec![
            vec![],
            vec![ecoute(1.0, EndReason::Completed, 0.0); 500],
            vec![rejet_immediat(0.0); 500],
        ];

        for signals in cas {
            for favori in [true, false] {
                let score = affinity(&signals, favori);
                assert!((-1.0..=1.0).contains(&score), "score obtenu {score}");
            }
        }
    }

    // ── Confiance ───────────────────────────────────────────────────────

    #[test]
    fn la_confiance_croit_avec_les_ecoutes() {
        let aucune = confidence(&[]);
        let une = confidence(&[ecoute(1.0, EndReason::Completed, 0.0)]);
        let vingt = confidence(&vec![ecoute(1.0, EndReason::Completed, 0.0); 20]);

        assert_eq!(aucune, 0.0);
        assert!(une > aucune);
        assert!(vingt > une);
        assert!(vingt < 1.0, "la certitude absolue n'existe pas");
    }

    #[test]
    fn la_confiance_sefface_avec_le_temps() {
        let recente = confidence(&vec![ecoute(1.0, EndReason::Completed, 0.0); 10]);
        let ancienne = confidence(&vec![ecoute(1.0, EndReason::Completed, 300.0); 10]);

        assert!(
            recente > ancienne * 5.0,
            "ce qu'on savait il y a un an n'est plus fiable"
        );
    }
}
