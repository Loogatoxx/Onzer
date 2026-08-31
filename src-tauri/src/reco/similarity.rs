//! Similarité sonore entre morceaux.
//!
//! # Pourquoi normaliser avant de comparer
//!
//! Le vecteur brut mélange des grandeurs d'échelles incomparables : un MFCC
//! vaut quelques unités, un centroïde spectral quelques milliers de hertz, un
//! tempo entre 50 et 200. Une distance calculée directement serait entièrement
//! dominée par le centroïde et le tempo — les MFCC, qui portent pourtant
//! l'essentiel du timbre, ne compteraient pour rien.
//!
//! Chaque dimension est donc **centrée-réduite sur la bibliothèque entière**.
//! Après quoi, « être à un écart-type au-dessus de la moyenne » signifie la
//! même chose pour toutes les dimensions.
//!
//! # Pourquoi normaliser sur la bibliothèque et non sur un corpus général
//!
//! C'est l'avantage d'un moteur local : si toute ta bibliothèque est du rap
//! français, la brillance moyenne de *ta* bibliothèque devient la référence, et
//! les écarts qui te distinguent réellement un morceau d'un autre ressortent.
//! Un modèle entraîné sur un corpus mondial noierait ces différences.

use std::collections::HashMap;

/// Écart-type minimal retenu, pour éviter qu'une dimension constante ne
/// produise une division par zéro puis des valeurs infinies.
const MIN_DEVIATION: f32 = 1e-6;

/// Matrice de similarité de toute la bibliothèque, en mémoire.
///
/// Cinq mille morceaux × 54 dimensions ≈ 1 Mo : garder l'ensemble en mémoire
/// est trivial, et rend chaque requête instantanée sans index approché. C'est
/// un luxe qu'une bibliothèque de plusieurs millions de titres n'aurait pas.
#[derive(Debug, Default)]
pub struct FeatureSpace {
    track_ids: Vec<i64>,
    /// Vecteurs centrés-réduits **puis** normalisés en norme 1 : le produit
    /// scalaire de deux lignes vaut alors directement leur cosinus.
    vectors: Vec<Vec<f32>>,
    index: HashMap<i64, usize>,
}

impl FeatureSpace {
    /// Construit l'espace à partir des vecteurs bruts de la base.
    pub fn build(rows: Vec<(i64, Vec<f32>)>) -> Self {
        let Some(dimensions) = rows.first().map(|(_, vector)| vector.len()) else {
            return Self::default();
        };

        // Seuls les vecteurs de dimension cohérente sont retenus : un vecteur
        // produit par une version antérieure de l'analyseur n'est pas
        // comparable.
        let rows: Vec<(i64, Vec<f32>)> = rows
            .into_iter()
            .filter(|(_, vector)| vector.len() == dimensions)
            .collect();

        if rows.is_empty() {
            return Self::default();
        }

        let (means, deviations) = column_statistics(&rows, dimensions);

        let mut track_ids = Vec::with_capacity(rows.len());
        let mut vectors = Vec::with_capacity(rows.len());
        let mut index = HashMap::with_capacity(rows.len());

        for (position, (track_id, vector)) in rows.into_iter().enumerate() {
            let standardized: Vec<f32> = vector
                .iter()
                .enumerate()
                .map(|(dimension, value)| (value - means[dimension]) / deviations[dimension])
                .collect();

            index.insert(track_id, position);
            track_ids.push(track_id);
            vectors.push(l2_normalize(standardized));
        }

        Self {
            track_ids,
            vectors,
            index,
        }
    }

    pub fn len(&self) -> usize {
        self.track_ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.track_ids.is_empty()
    }

    pub fn contains(&self, track_id: i64) -> bool {
        self.index.contains_key(&track_id)
    }

    pub fn vector(&self, track_id: i64) -> Option<&[f32]> {
        self.index
            .get(&track_id)
            .map(|position| self.vectors[*position].as_slice())
    }

    /// Similarité cosinus entre deux morceaux, dans [−1, 1].
    pub fn similarity(&self, left: i64, right: i64) -> Option<f32> {
        let a = self.vector(left)?;
        let b = self.vector(right)?;
        Some(dot(a, b))
    }

    /// Les `limit` morceaux les plus proches d'un vecteur donné.
    ///
    /// `exclude` retire les morceaux déjà retenus, pour éviter les répétitions.
    pub fn nearest_to_vector(
        &self,
        query: &[f32],
        limit: usize,
        exclude: &dyn Fn(i64) -> bool,
    ) -> Vec<(i64, f32)> {
        let mut scored: Vec<(i64, f32)> = self
            .track_ids
            .iter()
            .zip(&self.vectors)
            .filter(|(track_id, _)| !exclude(**track_id))
            .map(|(track_id, vector)| (*track_id, dot(query, vector)))
            .collect();

        // Tri décroissant. `total_cmp` évite tout comportement indéfini si une
        // valeur non numérique s'était glissée dans les données.
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(limit);
        scored
    }

    /// Les morceaux les plus proches d'un morceau donné, lui-même exclu.
    pub fn nearest(&self, track_id: i64, limit: usize) -> Vec<(i64, f32)> {
        let Some(query) = self.vector(track_id) else {
            return Vec::new();
        };

        self.nearest_to_vector(query, limit, &|candidate| candidate == track_id)
    }

    /// Barycentre de plusieurs morceaux, renormalisé.
    ///
    /// Représente « le son moyen » d'un ensemble — utile pour construire une
    /// radio à partir de tout ce qui a été aimé récemment plutôt que d'un seul
    /// morceau de départ.
    pub fn centroid(&self, track_ids: &[i64]) -> Option<Vec<f32>> {
        let vectors: Vec<&[f32]> = track_ids
            .iter()
            .filter_map(|track_id| self.vector(*track_id))
            .collect();

        if vectors.is_empty() {
            return None;
        }

        let dimensions = vectors[0].len();
        let mut centroid = vec![0.0_f32; dimensions];

        for vector in &vectors {
            for (dimension, value) in vector.iter().enumerate() {
                centroid[dimension] += value;
            }
        }

        for value in centroid.iter_mut() {
            *value /= vectors.len() as f32;
        }

        Some(l2_normalize(centroid))
    }
}

/// Moyenne et écart-type de chaque dimension, sur toute la bibliothèque.
fn column_statistics(rows: &[(i64, Vec<f32>)], dimensions: usize) -> (Vec<f32>, Vec<f32>) {
    let count = rows.len() as f32;
    let mut means = vec![0.0_f32; dimensions];

    for (_, vector) in rows {
        for (dimension, value) in vector.iter().enumerate() {
            means[dimension] += value;
        }
    }
    for mean in means.iter_mut() {
        *mean /= count;
    }

    let mut deviations = vec![0.0_f32; dimensions];
    for (_, vector) in rows {
        for (dimension, value) in vector.iter().enumerate() {
            deviations[dimension] += (value - means[dimension]).powi(2);
        }
    }
    for deviation in deviations.iter_mut() {
        *deviation = (*deviation / count).sqrt().max(MIN_DEVIATION);
    }

    (means, deviations)
}

fn l2_normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();

    if norm > 1e-9 {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }

    vector
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bibliothèque de test : deux « familles » sonores bien séparées, plus un
    /// intrus.
    fn bibliotheque() -> FeatureSpace {
        FeatureSpace::build(vec![
            // Famille A : valeurs basses sur les deux premières dimensions.
            (1, vec![1.0, 1.0, 0.0, 5000.0]),
            (2, vec![1.1, 0.9, 0.1, 5100.0]),
            (3, vec![0.9, 1.1, -0.1, 4900.0]),
            // Famille B : valeurs hautes.
            (4, vec![8.0, 8.0, 3.0, 200.0]),
            (5, vec![8.2, 7.8, 3.1, 210.0]),
            // Intrus, entre les deux.
            (6, vec![4.5, 4.5, 1.5, 2600.0]),
        ])
    }

    #[test]
    fn un_morceau_est_parfaitement_similaire_a_lui_meme() {
        let space = bibliotheque();
        let similarite = space.similarity(1, 1).unwrap();

        assert!((similarite - 1.0).abs() < 1e-5, "obtenu {similarite}");
    }

    #[test]
    fn les_morceaux_dune_meme_famille_se_reconnaissent() {
        let space = bibliotheque();

        let dans_famille = space.similarity(1, 2).unwrap();
        let hors_famille = space.similarity(1, 4).unwrap();

        assert!(
            dans_famille > hors_famille,
            "même famille {dans_famille}, autre famille {hors_famille}"
        );
    }

    #[test]
    fn les_plus_proches_voisins_sont_de_la_bonne_famille() {
        let space = bibliotheque();
        let voisins = space.nearest(1, 2);

        let identifiants: Vec<i64> = voisins.iter().map(|(id, _)| *id).collect();
        assert!(
            identifiants.contains(&2) && identifiants.contains(&3),
            "voisins obtenus : {identifiants:?}"
        );
    }

    #[test]
    fn un_morceau_nest_jamais_son_propre_voisin() {
        let space = bibliotheque();
        assert!(space.nearest(1, 10).iter().all(|(id, _)| *id != 1));
    }

    #[test]
    fn les_voisins_sont_ordonnes_du_plus_proche_au_plus_lointain() {
        let space = bibliotheque();
        let voisins = space.nearest(1, 5);

        for pair in voisins.windows(2) {
            assert!(
                pair[0].1 >= pair[1].1,
                "ordre rompu : {:?} avant {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    // ── La normalisation : la propriété qui rend tout comparable ────────

    #[test]
    fn une_dimension_de_grande_amplitude_necrase_pas_les_autres() {
        // La quatrième dimension varie de 200 à 5100, les autres de 0 à 8.
        // Sans centrage-réduction, elle déciderait seule de toute similarité.
        //
        // Deux morceaux identiques partout sauf sur cette dimension doivent
        // être jugés différents ; deux morceaux qui ne diffèrent que d'un
        // souffle sur elle doivent être jugés proches.
        let space = FeatureSpace::build(vec![
            (1, vec![1.0, 1.0, 100.0]),
            (2, vec![1.0, 1.0, 105.0]),  // presque identique
            (3, vec![1.0, 1.0, 5000.0]), // très différent sur la 3ᵉ seulement
            (4, vec![9.0, 9.0, 100.0]),  // différent sur les deux premières
        ]);

        let souffle = space.similarity(1, 2).unwrap();
        let ecart_reel = space.similarity(1, 3).unwrap();

        assert!(
            souffle > ecart_reel,
            "souffle {souffle} doit dépasser écart réel {ecart_reel}"
        );
    }

    #[test]
    fn une_dimension_constante_ne_provoque_pas_de_valeur_invalide() {
        // Écart-type nul : sans garde-fou, la division produirait des infinis.
        let space = FeatureSpace::build(vec![
            (1, vec![1.0, 42.0]),
            (2, vec![2.0, 42.0]),
            (3, vec![3.0, 42.0]),
        ]);

        for (left, right) in [(1, 2), (1, 3), (2, 3)] {
            let similarite = space.similarity(left, right).unwrap();
            assert!(similarite.is_finite(), "obtenu {similarite}");
        }
    }

    // ── Barycentre ──────────────────────────────────────────────────────

    #[test]
    fn le_barycentre_dune_famille_est_proche_de_ses_membres() {
        let space = bibliotheque();
        let centre = space.centroid(&[1, 2, 3]).unwrap();

        let proche = dot(&centre, space.vector(1).unwrap());
        let lointain = dot(&centre, space.vector(4).unwrap());

        assert!(proche > lointain, "proche {proche}, lointain {lointain}");
    }

    #[test]
    fn le_barycentre_dun_seul_morceau_est_ce_morceau() {
        let space = bibliotheque();
        let centre = space.centroid(&[1]).unwrap();

        let similarite = dot(&centre, space.vector(1).unwrap());
        assert!((similarite - 1.0).abs() < 1e-5, "obtenu {similarite}");
    }

    #[test]
    fn un_barycentre_de_morceaux_inconnus_nexiste_pas() {
        let space = bibliotheque();
        assert!(space.centroid(&[999, 1000]).is_none());
    }

    // ── Robustesse ──────────────────────────────────────────────────────

    #[test]
    fn une_bibliotheque_vide_ne_provoque_pas_de_panique() {
        let space = FeatureSpace::build(Vec::new());

        assert!(space.is_empty());
        assert_eq!(space.similarity(1, 2), None);
        assert!(space.nearest(1, 5).is_empty());
    }

    #[test]
    fn les_vecteurs_de_dimension_incoherente_sont_ecartes() {
        // Un vecteur produit par une version antérieure de l'analyseur n'est
        // pas comparable : mieux vaut l'ignorer que produire un score faux.
        let space = FeatureSpace::build(vec![
            (1, vec![1.0, 2.0, 3.0]),
            (2, vec![1.0, 2.0]), // dimension obsolète
            (3, vec![2.0, 3.0, 4.0]),
        ]);

        assert_eq!(space.len(), 2);
        assert!(!space.contains(2));
    }

    #[test]
    fn un_morceau_inconnu_na_pas_de_similarite() {
        let space = bibliotheque();
        assert_eq!(space.similarity(1, 999), None);
        assert_eq!(space.similarity(999, 1), None);
    }

    #[test]
    fn lexclusion_est_respectee() {
        let space = bibliotheque();
        let vecteur = space.vector(1).unwrap().to_vec();

        let voisins = space.nearest_to_vector(&vecteur, 10, &|id| id == 2 || id == 3);
        let identifiants: Vec<i64> = voisins.iter().map(|(id, _)| *id).collect();

        assert!(!identifiants.contains(&2));
        assert!(!identifiants.contains(&3));
    }
}
