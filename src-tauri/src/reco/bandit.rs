//! Échantillonnage de Thompson : le moteur apprend quelles stratégies
//! fonctionnent **chez toi**.
//!
//! # Le problème que ça résout
//!
//! Un moteur de recommandation combine plusieurs signaux : ressemblance
//! sonore, affinité passée, contexte horaire, enchaînements… Reste à savoir
//! quel poids donner à chacun.
//!
//! La solution paresseuse consiste à choisir des poids à la main — 40 % de
//! similarité, 30 % d'affinité, et ainsi de suite. Ces nombres sont
//! **arbitraires**, ne valent que pour l'auteur du code, et ne s'améliorent
//! jamais.
//!
//! # Le pari du bandit
//!
//! On met plutôt les stratégies en concurrence. Chacune porte une loi
//! Beta(α, β) représentant sa probabilité de succès. À chaque emplacement de
//! playlist, on **tire au sort** dans chaque loi et on retient la stratégie qui
//! sort la plus haute.
//!
//! Ce tirage produit l'exploration gratuitement : une stratégie peu testée a
//! une loi très étalée, donc de bonnes chances de tirer haut de temps en temps.
//! À mesure que les preuves s'accumulent, sa loi se resserre et le moteur cesse
//! de la tester inutilement. Aucun paramètre d'exploration à régler.
//!
//! # Pourquoi un bandit sur les *stratégies* et non sur les morceaux
//!
//! Un bandit sur 5 000 morceaux aurait 5 000 bras pour quelques centaines
//! d'écoutes : il n'apprendrait jamais rien. Six stratégies convergent en
//! quelques dizaines d'écoutes.

use serde::{Deserialize, Serialize};

/// Les stratégies mises en concurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    /// Ressemble sonorement à ce qui est écouté en ce moment.
    Similarity,
    /// Morceaux les mieux notés par l'historique.
    Affinity,
    /// Ce qui est habituellement écouté à cette heure, sur cet appareil.
    Context,
    /// S'enchaîne bien après le morceau précédent.
    Transition,
    /// Presque jamais écouté : le fond de bibliothèque.
    Discovery,
    /// Aimé autrefois, plus écouté depuis longtemps.
    Forgotten,
}

impl Strategy {
    pub const ALL: [Strategy; 6] = [
        Strategy::Similarity,
        Strategy::Affinity,
        Strategy::Context,
        Strategy::Transition,
        Strategy::Discovery,
        Strategy::Forgotten,
    ];

    /// Doit correspondre aux clés de la table `reco_strategies`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Similarity => "similarity",
            Self::Affinity => "affinity",
            Self::Context => "context",
            Self::Transition => "transition",
            Self::Discovery => "discovery",
            Self::Forgotten => "forgotten",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }

    /// Libellé affichable, pour expliquer une recommandation à l'utilisateur.
    pub fn label(self) -> &'static str {
        match self {
            Self::Similarity => "Ressemble à ce que tu écoutes",
            Self::Affinity => "Tu aimes ce morceau",
            Self::Context => "Tu écoutes ça à cette heure-ci",
            Self::Transition => "S'enchaîne bien après",
            Self::Discovery => "Tu ne l'as presque jamais écouté",
            Self::Forgotten => "Tu l'aimais, tu l'as oublié",
        }
    }
}

/// État d'apprentissage d'une stratégie.
#[derive(Debug, Clone, Copy)]
pub struct Arm {
    pub strategy: Strategy,
    /// Succès + 1.
    pub alpha: f64,
    /// Échecs + 1.
    pub beta: f64,
}

impl Arm {
    /// Bras neuf : loi uniforme, soit « je ne sais rien ».
    pub fn new(strategy: Strategy) -> Self {
        Self {
            strategy,
            alpha: 1.0,
            beta: 1.0,
        }
    }

    /// Taux de succès estimé.
    pub fn success_rate(&self) -> f64 {
        self.alpha / (self.alpha + self.beta)
    }

    /// Nombre d'observations accumulées.
    pub fn observations(&self) -> f64 {
        self.alpha + self.beta - 2.0
    }
}

/// Générateur pseudo-aléatoire xorshift.
///
/// Écrit à la main plutôt que d'ajouter une dépendance : le tirage n'a aucun
/// besoin de qualité cryptographique, et une graine fixe rend l'ensemble du
/// moteur **reproductible**, donc testable.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed },
        }
    }

    pub fn from_entropy() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);

        Self::new(nanos | 1)
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniforme dans ]0, 1[. Les bornes sont exclues : un logarithme de zéro
    /// ferait diverger l'échantillonnage gamma.
    pub fn uniform(&mut self) -> f64 {
        let value = (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64;
        value.clamp(f64::EPSILON, 1.0 - f64::EPSILON)
    }

    /// Loi normale centrée réduite, par transformation de Box-Muller.
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform();
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// Loi gamma d'échelle 1, par la méthode de Marsaglia-Tsang.
    pub fn gamma(&mut self, shape: f64) -> f64 {
        // La méthode exige une forme ≥ 1. En dessous, on utilise la relation
        // Γ(a) = Γ(a+1) · U^(1/a).
        if shape < 1.0 {
            let boosted = self.gamma(shape + 1.0);
            return boosted * self.uniform().powf(1.0 / shape);
        }

        let d = shape - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();

        loop {
            let x = self.normal();
            let v = (1.0 + c * x).powi(3);

            if v <= 0.0 {
                continue;
            }

            let u = self.uniform();
            if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
                return d * v;
            }
        }
    }

    /// Loi Beta(α, β), obtenue par le rapport de deux lois gamma.
    pub fn beta(&mut self, alpha: f64, beta: f64) -> f64 {
        let x = self.gamma(alpha.max(1e-6));
        let y = self.gamma(beta.max(1e-6));

        if x + y <= 0.0 {
            return 0.5;
        }

        x / (x + y)
    }
}

/// Tire une stratégie selon l'échantillonnage de Thompson.
pub fn select(arms: &[Arm], rng: &mut Rng) -> Option<Strategy> {
    arms.iter()
        .map(|arm| (arm.strategy, rng.beta(arm.alpha, arm.beta)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(strategy, _)| strategy)
}

/// Tire une séquence de stratégies, une par emplacement de playlist.
pub fn select_sequence(arms: &[Arm], length: usize, rng: &mut Rng) -> Vec<Strategy> {
    (0..length).filter_map(|_| select(arms, rng)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn arm(strategy: Strategy, alpha: f64, beta: f64) -> Arm {
        Arm {
            strategy,
            alpha,
            beta,
        }
    }

    /// Compte combien de fois chaque stratégie est retenue.
    fn distribution(arms: &[Arm], draws: usize) -> HashMap<Strategy, usize> {
        let mut rng = Rng::new(42);
        let mut counts = HashMap::new();

        for _ in 0..draws {
            if let Some(strategy) = select(arms, &mut rng) {
                *counts.entry(strategy).or_insert(0) += 1;
            }
        }

        counts
    }

    // ── Le générateur ───────────────────────────────────────────────────

    #[test]
    fn luniforme_reste_dans_lintervalle_ouvert() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let value = rng.uniform();
            assert!(value > 0.0 && value < 1.0, "obtenu {value}");
        }
    }

    #[test]
    fn la_loi_normale_a_les_bons_moments() {
        let mut rng = Rng::new(7);
        let echantillons: Vec<f64> = (0..50_000).map(|_| rng.normal()).collect();

        let moyenne = echantillons.iter().sum::<f64>() / echantillons.len() as f64;
        let variance = echantillons.iter().map(|v| (v - moyenne).powi(2)).sum::<f64>()
            / echantillons.len() as f64;

        assert!(moyenne.abs() < 0.03, "moyenne {moyenne}");
        assert!((variance - 1.0).abs() < 0.05, "variance {variance}");
    }

    #[test]
    fn la_loi_gamma_a_la_bonne_moyenne() {
        // L'espérance d'une loi gamma d'échelle 1 vaut sa forme.
        let mut rng = Rng::new(11);
        for forme in [0.5, 1.0, 3.0, 10.0_f64] {
            let echantillons: Vec<f64> = (0..30_000).map(|_| rng.gamma(forme)).collect();
            let moyenne = echantillons.iter().sum::<f64>() / echantillons.len() as f64;

            assert!(
                (moyenne - forme).abs() < forme * 0.05,
                "forme {forme} : moyenne obtenue {moyenne}"
            );
        }
    }

    #[test]
    fn la_loi_beta_a_la_bonne_moyenne_et_reste_bornee() {
        let mut rng = Rng::new(13);

        for (alpha, beta) in [(1.0, 1.0), (8.0, 2.0), (2.0, 8.0), (50.0, 50.0_f64)] {
            let echantillons: Vec<f64> = (0..30_000).map(|_| rng.beta(alpha, beta)).collect();
            let moyenne = echantillons.iter().sum::<f64>() / echantillons.len() as f64;
            let attendu = alpha / (alpha + beta);

            assert!(
                (moyenne - attendu).abs() < 0.02,
                "Beta({alpha}, {beta}) : attendu {attendu}, obtenu {moyenne}"
            );
            assert!(echantillons.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn une_graine_fixe_rend_le_tirage_reproductible() {
        let suite = |graine| {
            let mut rng = Rng::new(graine);
            (0..20).map(|_| rng.uniform()).collect::<Vec<_>>()
        };

        assert_eq!(suite(99), suite(99));
        assert_ne!(suite(99), suite(100));
    }

    // ── Le comportement du bandit ───────────────────────────────────────

    #[test]
    fn une_stratégie_nettement_meilleure_est_privilegiee() {
        let arms = [
            arm(Strategy::Similarity, 90.0, 10.0), // 90 % de succès
            arm(Strategy::Discovery, 10.0, 90.0),  // 10 % de succès
        ];

        let counts = distribution(&arms, 2_000);
        let bonne = counts.get(&Strategy::Similarity).copied().unwrap_or(0);
        let mauvaise = counts.get(&Strategy::Discovery).copied().unwrap_or(0);

        assert!(
            bonne > mauvaise * 20,
            "la bonne stratégie doit dominer : {bonne} contre {mauvaise}"
        );
    }

    /// La propriété centrale de Thompson, formulée en **comparaison** plutôt
    /// qu'en seuil chiffré : ce qui déclenche l'exploration n'est pas un
    /// mauvais score, c'est **l'incertitude**.
    ///
    /// Trois bras identiques en apparence pour deux d'entre eux — même
    /// estimation médiocre — mais l'un est à peine testé et l'autre largement
    /// prouvé. Le premier doit continuer d'être exploré, le second non.
    #[test]
    fn lexploration_suit_lincertitude_et_non_le_score() {
        let arms = [
            arm(Strategy::Similarity, 40.0, 10.0), // 80 %, bien établie
            arm(Strategy::Discovery, 1.0, 3.0),    // 25 %, presque pas testée
            arm(Strategy::Forgotten, 50.0, 150.0), // 25 %, largement prouvée
        ];

        let counts = distribution(&arms, 8_000);
        let etablie = counts.get(&Strategy::Similarity).copied().unwrap_or(0);
        let incertaine = counts.get(&Strategy::Discovery).copied().unwrap_or(0);
        let prouvee_mediocre = counts.get(&Strategy::Forgotten).copied().unwrap_or(0);

        assert!(
            incertaine > 0,
            "une stratégie incertaine ne doit jamais être définitivement écartée"
        );
        assert!(
            incertaine > prouvee_mediocre * 3,
            "à estimation égale, l'incertitude doit primer : incertaine {incertaine}, \
             prouvée médiocre {prouvee_mediocre}"
        );
        assert!(
            etablie > incertaine * 5,
            "la stratégie qui marche doit rester largement majoritaire : \
             établie {etablie}, incertaine {incertaine}"
        );
    }

    #[test]
    fn au_demarrage_toutes_les_strategies_sont_equitablement_testees() {
        // Toutes les lois sont uniformes : aucune ne doit dominer.
        let arms: Vec<Arm> = Strategy::ALL.into_iter().map(Arm::new).collect();
        let counts = distribution(&arms, 6_000);

        assert_eq!(counts.len(), Strategy::ALL.len(), "aucune ne doit être ignorée");

        for (strategy, count) in &counts {
            assert!(
                (600..1_400).contains(count),
                "{strategy:?} tirée {count} fois sur 6000 — répartition déséquilibrée"
            );
        }
    }

    #[test]
    fn plus_de_preuves_resserre_la_loi() {
        // Deux stratégies de même taux de succès, mais l'une bien mieux
        // documentée : ses tirages doivent être moins dispersés.
        let mut rng = Rng::new(5);

        let dispersion = |alpha: f64, beta: f64, rng: &mut Rng| {
            let echantillons: Vec<f64> = (0..20_000).map(|_| rng.beta(alpha, beta)).collect();
            let moyenne = echantillons.iter().sum::<f64>() / echantillons.len() as f64;
            (echantillons.iter().map(|v| (v - moyenne).powi(2)).sum::<f64>()
                / echantillons.len() as f64)
                .sqrt()
        };

        let peu_sur = dispersion(3.0, 3.0, &mut rng);
        let bien_sur = dispersion(300.0, 300.0, &mut rng);

        assert!(
            bien_sur < peu_sur / 5.0,
            "peu sûr {peu_sur}, bien sûr {bien_sur}"
        );
    }

    #[test]
    fn une_sequence_a_la_longueur_demandee_et_reste_variee() {
        let arms: Vec<Arm> = Strategy::ALL.into_iter().map(Arm::new).collect();
        let mut rng = Rng::new(3);

        let sequence = select_sequence(&arms, 30, &mut rng);

        assert_eq!(sequence.len(), 30);

        let distinctes: std::collections::HashSet<_> = sequence.iter().collect();
        assert!(
            distinctes.len() >= 3,
            "une playlist ne doit pas venir d'une seule stratégie : {distinctes:?}"
        );
    }

    #[test]
    fn une_liste_de_bras_vide_ne_provoque_pas_de_panique() {
        let mut rng = Rng::new(1);
        assert_eq!(select(&[], &mut rng), None);
    }

    // ── Correspondance avec la base ─────────────────────────────────────

    #[test]
    fn les_identifiants_font_laller_retour() {
        for strategy in Strategy::ALL {
            assert_eq!(Strategy::from_str(strategy.as_str()), Some(strategy));
        }
        assert_eq!(Strategy::from_str("inconnue"), None);
    }

    #[test]
    fn le_taux_de_succes_dun_bras_neuf_est_de_moitie() {
        let neuf = Arm::new(Strategy::Affinity);
        assert_eq!(neuf.success_rate(), 0.5);
        assert_eq!(neuf.observations(), 0.0);
    }
}
