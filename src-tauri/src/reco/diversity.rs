//! Règles de diversité d'une playlist.
//!
//! # Pourquoi un moteur « parfait » produit des playlists détestables
//!
//! Un moteur qui se contente de classer les morceaux par score produit
//! invariablement la même chose : les huit meilleurs titres du même artiste, à
//! la suite. Chaque choix est individuellement optimal, et l'ensemble est
//! insupportable.
//!
//! La qualité d'une playlist ne se réduit pas à la qualité de ses morceaux :
//! elle tient aussi à leur **enchaînement**. Ce module impose donc des règles
//! qui *dégradent volontairement* le score de chaque emplacement pour améliorer
//! l'ensemble.
//!
//! # Deux natures de règles
//!
//! | Nature | Effet | Exemple |
//! |---|---|---|
//! | **Dure** | Le candidat est écarté, quel que soit son score | Jamais deux fois le même artiste d'affilée |
//! | **Souple** | Le score est pénalisé, le candidat reste possible | Quatre morceaux au même tempo à la suite |
//!
//! Les règles dures garantissent l'absence d'absurdité. Les règles souples
//! laissent un excellent morceau passer malgré une légère monotonie — ce qui
//! est souvent le bon arbitrage.

/// Politique de tempo, selon la nature de la playlist.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TempoPolicy {
    /// Alterner les tempos. Convient à une écoute générale : quinze morceaux à
    /// 128 BPM d'affilée finissent par lasser.
    #[default]
    Varied,
    /// Rester proche d'un tempo cible. Indispensable pour une radio de sport ou
    /// une playlist de concentration, où la rupture de rythme casse tout.
    Cohesive { target_bpm: f32 },
}

/// Paramètres des règles.
#[derive(Debug, Clone, Copy)]
pub struct DiversityRules {
    /// Nombre d'emplacements pendant lesquels un artiste est interdit après
    /// être passé. À 2, deux morceaux du même artiste sont toujours séparés par
    /// au moins deux autres.
    pub artist_cooldown: usize,
    /// Nombre maximal de morceaux d'un même artiste dans toute la playlist.
    pub max_per_artist: usize,
    pub tempo_policy: TempoPolicy,
    /// Nombre de morceaux récents examinés pour juger de la monotonie de tempo.
    pub tempo_window: usize,
}

impl Default for DiversityRules {
    fn default() -> Self {
        Self {
            // Deux emplacements de battement : le même artiste ne peut pas
            // revenir avant le troisième morceau suivant.
            artist_cooldown: 2,
            // Sur une playlist de 25 titres, trois du même artiste restent
            // agréables ; au-delà, c'est un album déguisé.
            max_per_artist: 3,
            tempo_policy: TempoPolicy::Varied,
            tempo_window: 3,
        }
    }
}

/// Écart de tempo en deçà duquel deux morceaux sont jugés « au même rythme ».
const SIMILAR_TEMPO_BPM: f32 = 4.0;

/// Pénalité appliquée à un candidat qui prolonge une série de tempos voisins.
///
/// Volontairement modérée : un morceau nettement meilleur doit pouvoir passer
/// malgré la monotonie.
const MONOTONY_PENALTY: f64 = 0.18;

/// Pénalité maximale pour un écart de tempo, en mode cohésif.
const TEMPO_DRIFT_PENALTY: f64 = 0.35;

/// Écart de tempo au-delà duquel la pénalité cohésive est maximale.
const TEMPO_DRIFT_RANGE: f32 = 30.0;

/// Ce que la diversité a besoin de connaître d'un candidat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub track_id: i64,
    /// `None` si l'artiste est inconnu : la règle d'artiste ne s'applique alors
    /// pas, faute de pouvoir la vérifier.
    pub artist_id: Option<i64>,
    /// `None` si aucune pulsation n'a été décelée.
    pub tempo: Option<f32>,
}

/// Vérificateur de règles, à mesure que la playlist se construit.
#[derive(Debug)]
pub struct DiversityGuard {
    rules: DiversityRules,
    chosen: Vec<Candidate>,
}

impl DiversityGuard {
    pub fn new(rules: DiversityRules) -> Self {
        Self {
            rules,
            chosen: Vec::new(),
        }
    }

    pub fn chosen(&self) -> &[Candidate] {
        &self.chosen
    }

    pub fn len(&self) -> usize {
        self.chosen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chosen.is_empty()
    }

    /// Le candidat est-il **autorisé** ? Règles dures uniquement.
    pub fn accepts(&self, candidate: &Candidate) -> bool {
        // Jamais deux fois le même morceau.
        if self
            .chosen
            .iter()
            .any(|already| already.track_id == candidate.track_id)
        {
            return false;
        }

        let Some(artist_id) = candidate.artist_id else {
            // Artiste inconnu : impossible d'appliquer la règle, on laisse
            // passer plutôt que d'écarter injustement.
            return true;
        };

        // Délai de carence : l'artiste vient-il de passer ?
        let recent = self
            .chosen
            .iter()
            .rev()
            .take(self.rules.artist_cooldown)
            .any(|already| already.artist_id == Some(artist_id));

        if recent {
            return false;
        }

        // Quota global sur toute la playlist.
        let total = self
            .chosen
            .iter()
            .filter(|already| already.artist_id == Some(artist_id))
            .count();

        total < self.rules.max_per_artist
    }

    /// Pénalité à retrancher au score. Règles souples.
    pub fn penalty(&self, candidate: &Candidate) -> f64 {
        let Some(tempo) = candidate.tempo else {
            return 0.0;
        };

        match self.rules.tempo_policy {
            TempoPolicy::Varied => {
                // Le candidat prolonge-t-il une série de tempos voisins ?
                let similar = self
                    .chosen
                    .iter()
                    .rev()
                    .take(self.rules.tempo_window)
                    .filter(|already| {
                        already
                            .tempo
                            .is_some_and(|other| (other - tempo).abs() < SIMILAR_TEMPO_BPM)
                    })
                    .count();

                // Une seule répétition est naturelle ; c'est l'accumulation qui
                // lasse. La pénalité ne démarre donc qu'au deuxième voisin.
                MONOTONY_PENALTY * similar.saturating_sub(1) as f64
            }

            TempoPolicy::Cohesive { target_bpm } => {
                let drift = (tempo - target_bpm).abs() / TEMPO_DRIFT_RANGE;
                TEMPO_DRIFT_PENALTY * f64::from(drift.clamp(0.0, 1.0))
            }
        }
    }

    /// Retient définitivement un candidat.
    pub fn push(&mut self, candidate: Candidate) {
        self.chosen.push(candidate);
    }

    /// Choisit le meilleur candidat autorisé, pénalités comprises.
    ///
    /// `scored` n'a pas besoin d'être trié : les pénalités peuvent renverser
    /// l'ordre initial.
    pub fn best(&self, scored: &[(Candidate, f64)]) -> Option<(Candidate, f64)> {
        scored
            .iter()
            .filter(|(candidate, _)| self.accepts(candidate))
            .map(|(candidate, score)| (*candidate, score - self.penalty(candidate)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(track_id: i64, artist_id: i64, tempo: f32) -> Candidate {
        Candidate {
            track_id,
            artist_id: Some(artist_id),
            tempo: Some(tempo),
        }
    }

    // ── Règles dures ────────────────────────────────────────────────────

    #[test]
    fn un_morceau_ne_peut_pas_apparaitre_deux_fois() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        let morceau = candidate(1, 10, 120.0);

        assert!(guard.accepts(&morceau));
        guard.push(morceau);
        assert!(!guard.accepts(&morceau));
    }

    #[test]
    fn le_meme_artiste_ne_passe_jamais_deux_fois_de_suite() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));

        assert!(
            !guard.accepts(&candidate(2, 10, 125.0)),
            "c'est LE défaut le plus visible d'un moteur naïf"
        );
        assert!(guard.accepts(&candidate(3, 20, 125.0)));
    }

    #[test]
    fn un_artiste_revient_apres_le_delai_de_carence() {
        let mut guard = DiversityGuard::new(DiversityRules::default());

        guard.push(candidate(1, 10, 120.0));
        guard.push(candidate(2, 20, 120.0));
        assert!(!guard.accepts(&candidate(3, 10, 120.0)), "encore trop tôt");

        guard.push(candidate(3, 30, 120.0));
        assert!(guard.accepts(&candidate(4, 10, 120.0)), "deux morceaux ont passé");
    }

    #[test]
    fn le_quota_par_artiste_est_respecte() {
        let mut guard = DiversityGuard::new(DiversityRules::default());

        // Trois morceaux de l'artiste 10, correctement espacés.
        for tour in 0..3 {
            guard.push(candidate(tour * 3, 10, 120.0));
            guard.push(candidate(tour * 3 + 1, 20 + tour, 120.0));
            guard.push(candidate(tour * 3 + 2, 30 + tour, 120.0));
        }

        assert!(
            !guard.accepts(&candidate(99, 10, 120.0)),
            "le quota de 3 est atteint"
        );
        assert!(guard.accepts(&candidate(99, 40, 120.0)));
    }

    #[test]
    fn un_artiste_inconnu_nest_pas_penalise() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        let anonyme = Candidate {
            track_id: 1,
            artist_id: None,
            tempo: None,
        };

        guard.push(anonyme);

        // Faute de pouvoir vérifier la règle, on laisse passer plutôt que
        // d'écarter injustement tout un pan de la bibliothèque.
        assert!(guard.accepts(&Candidate {
            track_id: 2,
            artist_id: None,
            tempo: None,
        }));
    }

    // ── Règles souples : le tempo ───────────────────────────────────────

    #[test]
    fn un_tempo_isole_nest_pas_penalise() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));

        assert_eq!(
            guard.penalty(&candidate(2, 20, 121.0)),
            0.0,
            "une seule répétition est naturelle"
        );
    }

    #[test]
    fn une_serie_de_tempos_identiques_est_penalisee() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));
        guard.push(candidate(2, 20, 121.0));
        guard.push(candidate(3, 30, 119.0));

        let monotone = guard.penalty(&candidate(4, 40, 120.5));
        let contraste = guard.penalty(&candidate(5, 50, 165.0));

        assert!(monotone > 0.0, "pénalité obtenue {monotone}");
        assert_eq!(contraste, 0.0, "un tempo différent ne coûte rien");
    }

    #[test]
    fn le_mode_cohesif_privilegie_le_tempo_cible() {
        let guard = DiversityGuard::new(DiversityRules {
            tempo_policy: TempoPolicy::Cohesive { target_bpm: 128.0 },
            ..Default::default()
        });

        let proche = guard.penalty(&candidate(1, 10, 130.0));
        let lointain = guard.penalty(&candidate(2, 20, 80.0));

        assert!(
            lointain > proche,
            "proche {proche}, lointain {lointain} — une radio de sport ne doit pas ralentir"
        );
    }

    #[test]
    fn le_mode_cohesif_ne_penalise_pas_une_serie_homogene() {
        // À l'inverse du mode varié : c'est exactement ce qu'on veut ici.
        let mut guard = DiversityGuard::new(DiversityRules {
            tempo_policy: TempoPolicy::Cohesive { target_bpm: 128.0 },
            ..Default::default()
        });

        for track in 1..=5 {
            guard.push(candidate(track, track * 10, 128.0));
        }

        assert!(guard.penalty(&candidate(99, 990, 128.0)) < 0.01);
    }

    #[test]
    fn un_morceau_sans_tempo_nest_jamais_penalise() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));
        guard.push(candidate(2, 20, 120.0));

        let sans_tempo = Candidate {
            track_id: 3,
            artist_id: Some(30),
            tempo: None,
        };

        assert_eq!(guard.penalty(&sans_tempo), 0.0);
    }

    // ── Sélection ───────────────────────────────────────────────────────

    #[test]
    fn le_meilleur_candidat_autorise_est_retenu() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));

        // Le mieux noté appartient à l'artiste qui vient de passer.
        let scored = vec![
            (candidate(2, 10, 120.0), 0.95),
            (candidate(3, 20, 160.0), 0.60),
            (candidate(4, 30, 160.0), 0.40),
        ];

        let (retenu, _) = guard.best(&scored).unwrap();
        assert_eq!(retenu.track_id, 3, "la règle dure doit primer sur le score");
    }

    #[test]
    fn une_penalite_peut_renverser_le_classement() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));
        guard.push(candidate(2, 20, 120.0));
        guard.push(candidate(3, 30, 120.0));

        // Le premier est légèrement mieux noté, mais prolonge la monotonie.
        let scored = vec![
            (candidate(4, 40, 120.0), 0.50),
            (candidate(5, 50, 170.0), 0.45),
        ];

        let (retenu, _) = guard.best(&scored).unwrap();
        assert_eq!(retenu.track_id, 5, "la variété doit l'emporter de peu");
    }

    #[test]
    fn un_morceau_nettement_meilleur_passe_malgre_la_monotonie() {
        // La contrepartie : les règles souples ne doivent pas écarter une
        // évidence.
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));
        guard.push(candidate(2, 20, 120.0));
        guard.push(candidate(3, 30, 120.0));

        let scored = vec![
            (candidate(4, 40, 120.0), 0.95),
            (candidate(5, 50, 170.0), 0.45),
        ];

        let (retenu, _) = guard.best(&scored).unwrap();
        assert_eq!(retenu.track_id, 4);
    }

    #[test]
    fn aucun_candidat_autorise_ne_produit_aucun_choix() {
        let mut guard = DiversityGuard::new(DiversityRules::default());
        guard.push(candidate(1, 10, 120.0));

        let scored = vec![(candidate(2, 10, 120.0), 0.9)];
        assert!(guard.best(&scored).is_none());
    }

    #[test]
    fn une_liste_vide_ne_produit_aucun_choix() {
        let guard = DiversityGuard::new(DiversityRules::default());
        assert!(guard.best(&[]).is_none());
    }

    // ── Propriété d'ensemble ────────────────────────────────────────────

    #[test]
    fn une_playlist_construite_respecte_toutes_les_regles() {
        let mut guard = DiversityGuard::new(DiversityRules::default());

        // Bibliothèque déséquilibrée : un artiste possède la moitié des
        // morceaux, et les mieux notés.
        let mut pool: Vec<(Candidate, f64)> = Vec::new();
        for track in 0..40_i64 {
            let artist = if track % 2 == 0 { 1 } else { track };
            let score = if artist == 1 { 0.9 } else { 0.5 };
            pool.push((candidate(track, artist, 120.0), score));
        }

        while guard.len() < 20 {
            let Some((retenu, _)) = guard.best(&pool) else {
                break;
            };
            guard.push(retenu);
        }

        let choisis = guard.chosen();
        assert_eq!(choisis.len(), 20);

        // Jamais deux fois de suite le même artiste.
        for paire in choisis.windows(2) {
            assert_ne!(
                paire[0].artist_id, paire[1].artist_id,
                "artiste répété d'affilée"
            );
        }

        // Quota respecté, malgré des scores bien supérieurs.
        let artiste_dominant = choisis
            .iter()
            .filter(|c| c.artist_id == Some(1))
            .count();
        assert!(
            artiste_dominant <= 3,
            "l'artiste dominant apparaît {artiste_dominant} fois"
        );

        // Aucun doublon.
        let uniques: std::collections::HashSet<i64> =
            choisis.iter().map(|c| c.track_id).collect();
        assert_eq!(uniques.len(), 20);
    }
}
