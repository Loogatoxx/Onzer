//! Détection de tonalité par l'algorithme de Krumhansl-Schmuckler.
//!
//! # Le principe
//!
//! Carol Krumhansl a mesuré expérimentalement, dans les années 1980, à quel
//! point chaque note « sonne juste » dans une tonalité donnée. En do majeur,
//! le do est très stable, le sol l'est presque autant, le fa dièse ne l'est pas
//! du tout. Ces mesures forment un **profil** de douze valeurs.
//!
//! L'algorithme est alors d'une simplicité remarquable : on compare le chroma
//! observé aux vingt-quatre profils possibles (douze majeurs, douze mineurs)
//! par corrélation, et on retient le meilleur.
//!
//! # Pourquoi c'est utile ici
//!
//! Le mode majeur ou mineur est **le meilleur prédicteur objectif disponible de
//! la couleur émotionnelle** d'un morceau. Il ne suffit pas à lui seul — un
//! morceau mineur rapide peut être euphorique — mais combiné au tempo et à la
//! brillance, il donne une estimation de valence honnête.

/// Profils expérimentaux de Krumhansl-Kessler, à partir de do.
const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];

const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

pub const NOTE_NAMES: [&str; 12] = [
    "Do", "Do♯", "Ré", "Ré♯", "Mi", "Fa", "Fa♯", "Sol", "Sol♯", "La", "La♯", "Si",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedKey {
    /// 0 = Do, 1 = Do♯, … 11 = Si.
    pub tonic: u8,
    /// `true` pour majeur.
    pub is_major: bool,
}

impl DetectedKey {
    pub fn label(&self) -> String {
        format!(
            "{} {}",
            NOTE_NAMES[self.tonic as usize],
            if self.is_major { "majeur" } else { "mineur" }
        )
    }
}

/// Détermine la tonalité la plus probable d'un chroma moyenné.
///
/// Retourne `None` si le chroma est vide ou uniforme : mieux vaut ne rien
/// affirmer que de désigner une tonalité au hasard sur un morceau bruité.
pub fn detect_key(chroma: &[f32; 12]) -> Option<DetectedKey> {
    let total: f32 = chroma.iter().sum();
    if total <= 1e-6 {
        return None;
    }

    // Un chroma parfaitement plat ne porte aucune information tonale.
    let mean = total / 12.0;
    let variance = chroma.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 12.0;
    if variance < 1e-8 {
        return None;
    }

    let mut best: Option<(DetectedKey, f32)> = None;

    for tonic in 0..12_u8 {
        for (profile, is_major) in [(&MAJOR_PROFILE, true), (&MINOR_PROFILE, false)] {
            // Le profil est tourné pour aligner sa tonique sur la note testée.
            let rotated: Vec<f32> = (0..12)
                .map(|degree| profile[(degree + 12 - tonic as usize) % 12])
                .collect();

            let score = pearson_correlation(chroma, &rotated);
            let candidate = DetectedKey { tonic, is_major };

            if best.is_none_or(|(_, current)| score > current) {
                best = Some((candidate, score));
            }
        }
    }

    best.map(|(key, _)| key)
}

/// Coefficient de corrélation de Pearson.
///
/// Insensible à l'échelle : un chroma faible mais bien profilé obtient le même
/// score qu'un chroma fort de même forme, ce qui est exactement le comportement
/// voulu — la tonalité ne dépend pas du volume.
fn pearson_correlation(left: &[f32], right: &[f32]) -> f32 {
    let n = left.len() as f32;
    let mean_left = left.iter().sum::<f32>() / n;
    let mean_right = right.iter().sum::<f32>() / n;

    let mut covariance = 0.0;
    let mut variance_left = 0.0;
    let mut variance_right = 0.0;

    for (a, b) in left.iter().zip(right) {
        let da = a - mean_left;
        let db = b - mean_right;
        covariance += da * db;
        variance_left += da * da;
        variance_right += db * db;
    }

    let denominator = (variance_left * variance_right).sqrt();
    if denominator <= 1e-9 {
        return 0.0;
    }

    covariance / denominator
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chroma d'un accord parfait : trois notes présentes, le reste à zéro.
    fn triad(root: usize, third: usize, fifth: usize) -> [f32; 12] {
        let mut chroma = [0.02_f32; 12]; // fond harmonique réaliste
        chroma[root] = 1.0;
        chroma[third] = 0.7;
        chroma[fifth] = 0.8;
        chroma
    }

    #[test]
    fn reconnait_un_accord_de_do_majeur() {
        // Do – Mi – Sol
        let key = detect_key(&triad(0, 4, 7)).unwrap();

        assert_eq!(key.tonic, 0, "tonique attendue Do, obtenue {}", key.label());
        assert!(key.is_major, "mode attendu majeur, obtenu {}", key.label());
    }

    #[test]
    fn reconnait_un_accord_de_la_mineur() {
        // La – Do – Mi
        let key = detect_key(&triad(9, 0, 4)).unwrap();

        assert_eq!(key.tonic, 9, "obtenu {}", key.label());
        assert!(!key.is_major, "obtenu {}", key.label());
    }

    #[test]
    fn reconnait_un_accord_de_sol_majeur() {
        // Sol – Si – Ré
        let key = detect_key(&triad(7, 11, 2)).unwrap();

        assert_eq!(key.tonic, 7, "obtenu {}", key.label());
        assert!(key.is_major, "obtenu {}", key.label());
    }

    #[test]
    fn reconnait_un_accord_de_mi_mineur() {
        // Mi – Sol – Si
        let key = detect_key(&triad(4, 7, 11)).unwrap();

        assert_eq!(key.tonic, 4, "obtenu {}", key.label());
        assert!(!key.is_major, "obtenu {}", key.label());
    }

    #[test]
    fn la_detection_est_insensible_au_volume() {
        let faible: [f32; 12] = triad(0, 4, 7).map(|value| value * 0.001);
        let fort: [f32; 12] = triad(0, 4, 7).map(|value| value * 100.0);

        assert_eq!(detect_key(&faible), detect_key(&fort));
    }

    #[test]
    fn la_gamme_complete_de_do_majeur_est_reconnue() {
        // Toutes les notes de la gamme, pondérées selon leur importance réelle.
        let mut chroma = [0.05_f32; 12];
        for (degree, poids) in [(0, 1.0), (2, 0.5), (4, 0.8), (5, 0.6), (7, 0.9), (9, 0.5), (11, 0.4)] {
            chroma[degree] = poids;
        }

        let key = detect_key(&chroma).unwrap();
        assert_eq!(key.tonic, 0, "obtenu {}", key.label());
        assert!(key.is_major);
    }

    #[test]
    fn naffirme_rien_sur_un_chroma_uniforme() {
        // Bruit blanc : toutes les classes de hauteur également présentes.
        assert_eq!(detect_key(&[1.0 / 12.0; 12]), None);
    }

    #[test]
    fn naffirme_rien_sur_un_chroma_vide() {
        assert_eq!(detect_key(&[0.0; 12]), None);
    }

    #[test]
    fn les_libelles_sont_lisibles() {
        assert_eq!(
            DetectedKey { tonic: 0, is_major: true }.label(),
            "Do majeur"
        );
        assert_eq!(
            DetectedKey { tonic: 9, is_major: false }.label(),
            "La mineur"
        );
    }

    #[test]
    fn la_correlation_reconnait_lidentite_et_loppose() {
        let profil = MAJOR_PROFILE.to_vec();
        assert!((pearson_correlation(&profil, &profil) - 1.0).abs() < 1e-5);

        let inverse: Vec<f32> = profil.iter().map(|v| -v).collect();
        assert!((pearson_correlation(&profil, &inverse) + 1.0).abs() < 1e-5);
    }
}
