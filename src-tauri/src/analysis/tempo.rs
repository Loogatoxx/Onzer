//! Estimation du tempo.
//!
//! # Le principe
//!
//! Un morceau rythmé produit des **attaques** régulières : une grosse caisse,
//! une note plaquée, un coup de caisse claire. Chaque attaque se traduit par
//! une montée soudaine d'énergie spectrale — c'est le *flux* calculé fenêtre
//! par fenêtre.
//!
//! Reste à trouver la période de ces montées. L'**autocorrélation** répond
//! exactement à cette question : à quel décalage le signal ressemble-t-il le
//! plus à lui-même ? Le décalage gagnant est la durée d'un temps.
//!
//! # Le piège des octaves rythmiques
//!
//! Un morceau à 128 BPM se corrèle presque aussi bien à 64 et à 256 BPM : un
//! battement sur deux reste un battement. Sans correction, l'estimation part
//! régulièrement au double ou à la moitié.
//!
//! La parade est classique : pondérer par une gaussienne centrée sur 120 BPM en
//! échelle logarithmique. On n'interdit pas les tempos extrêmes, on exige
//! seulement qu'ils soient nettement mieux corrélés pour l'emporter.

/// Bornes de recherche. En deçà de 50 BPM il ne s'agit plus d'une pulsation
/// perçue, au-delà de 200 on compte des doubles croches.
const MIN_BPM: f32 = 50.0;
const MAX_BPM: f32 = 200.0;

/// Centre de la pondération. 120 BPM est la pulsation la plus répandue en
/// musique populaire, et le référent naturel de la perception rythmique.
const PREFERRED_BPM: f32 = 120.0;

/// Largeur de la pondération, en octaves rythmiques. 0,9 laisse passer une
/// plage large : à 60 ou 240 BPM le poids vaut encore environ 0,6.
const TEMPO_SPREAD: f32 = 0.9;

/// Corrélation minimale pour oser annoncer un tempo.
///
/// En dessous, le « pic » trouvé n'est que du bruit numérique.
const MIN_PEAK_CORRELATION: f32 = 0.12;

/// Le pic doit ressortir nettement du fond de la courbe d'autocorrélation.
///
/// C'est le critère décisif : sur une nappe ambiante, toutes les périodes se
/// corrèlent aussi mal les unes que les autres, et le maximum n'a aucun sens.
/// Exiger qu'il domine la médiane distingue une vraie pulsation d'un accident.
const MIN_PEAK_CONTRAST: f32 = 1.6;

/// Estime le tempo à partir de l'enveloppe d'attaques.
///
/// Retourne `None` si le signal ne présente aucune périodicité exploitable —
/// cas d'une nappe ambiante ou d'un enregistrement de parole.
pub fn estimate_tempo(onset_envelope: &[f32], frames_per_second: f32) -> Option<f32> {
    let envelope = sharpen(onset_envelope);

    let min_lag = (frames_per_second * 60.0 / MAX_BPM).round() as usize;
    let max_lag = (frames_per_second * 60.0 / MIN_BPM).round() as usize;

    if envelope.len() < max_lag * 2 || min_lag == 0 {
        return None; // extrait trop court pour observer une périodicité
    }

    let correlations: Vec<f32> = (min_lag..=max_lag)
        .map(|lag| autocorrelation(&envelope, lag))
        .collect();

    let mut best_index = 0;
    let mut best_weighted = 0.0_f32;

    for (index, correlation) in correlations.iter().enumerate() {
        let lag = min_lag + index;
        let bpm = frames_per_second * 60.0 / lag as f32;
        let weighted = correlation * tempo_prior(bpm);

        if weighted > best_weighted {
            best_weighted = weighted;
            best_index = index;
        }
    }

    let peak = correlations[best_index];
    let background = median(&correlations);

    // Un maximum existe toujours ; encore faut-il qu'il signifie quelque chose.
    if peak < MIN_PEAK_CORRELATION || peak < background * MIN_PEAK_CONTRAST {
        return None;
    }

    Some(frames_per_second * 60.0 / (min_lag + best_index) as f32)
}

/// Accentue les attaques : soustraction d'une moyenne glissante, puis
/// redressement.
///
/// Sans cette étape, un morceau au niveau sonore élevé produit une
/// autocorrélation dominée par sa composante continue, et toutes les périodes
/// se valent.
fn sharpen(envelope: &[f32]) -> Vec<f32> {
    const WINDOW: usize = 8;

    if envelope.len() <= WINDOW {
        return envelope.to_vec();
    }

    envelope
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let start = index.saturating_sub(WINDOW);
            let end = (index + WINDOW).min(envelope.len() - 1);
            let local_mean =
                envelope[start..=end].iter().sum::<f32>() / (end - start + 1) as f32;

            // Seules les montées au-dessus de la tendance locale comptent.
            (value - local_mean).max(0.0)
        })
        .collect()
}

/// Corrélation normalisée du signal avec lui-même, décalé de `lag`.
fn autocorrelation(signal: &[f32], lag: usize) -> f32 {
    if lag >= signal.len() {
        return 0.0;
    }

    let overlap = signal.len() - lag;
    let product: f32 = (0..overlap).map(|i| signal[i] * signal[i + lag]).sum();
    let energy: f32 = signal.iter().map(|value| value * value).sum();

    if energy <= 1e-9 {
        return 0.0;
    }

    // Normalisation par l'énergie et par la longueur de recouvrement : sans
    // cela, les petits décalages seraient systématiquement favorisés.
    (product / energy) * (signal.len() as f32 / overlap as f32)
}

fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

/// Pondération gaussienne en échelle logarithmique, centrée sur 120 BPM.
fn tempo_prior(bpm: f32) -> f32 {
    let octaves = (bpm / PREFERRED_BPM).log2();
    (-0.5 * (octaves / TEMPO_SPREAD).powi(2)).exp()
}

/// Régularité de la pulsation, entre 0 et 1.
///
/// Mesure à quel point les attaques tombent réellement sur la grille du tempo
/// estimé. Un morceau de danse s'approche de 1 ; une improvisation libre reste
/// bas. C'est cette valeur qui alimente la « dansabilité ».
pub fn beat_strength(onset_envelope: &[f32], bpm: f32, frames_per_second: f32) -> f32 {
    let envelope = sharpen(onset_envelope);
    let lag = (frames_per_second * 60.0 / bpm).round() as usize;

    if lag == 0 || envelope.len() < lag * 2 {
        return 0.0;
    }

    autocorrelation(&envelope, lag).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: f32 = 43.07; // 22050 Hz / 512 échantillons de saut

    /// Enveloppe d'attaques synthétique : une impulsion tous les `bpm`.
    fn pulses(bpm: f32, seconds: f32, jitter: f32) -> Vec<f32> {
        let frames = (seconds * FPS) as usize;
        let period = FPS * 60.0 / bpm;

        let mut envelope = vec![0.02_f32; frames]; // léger bruit de fond
        let mut beat = 0.0_f32;
        let mut wobble = 0.0_f32;

        while (beat as usize) < frames {
            let index = beat as usize;
            envelope[index] = 1.0;
            // Un peu d'étalement : une attaque réelle n'est jamais une seule
            // fenêtre.
            if index + 1 < frames {
                envelope[index + 1] = 0.5;
            }

            wobble = (wobble + 0.7).fract();
            beat += period + (wobble - 0.5) * 2.0 * jitter;
        }

        envelope
    }

    #[test]
    fn retrouve_un_tempo_de_120_bpm() {
        let tempo = estimate_tempo(&pulses(120.0, 30.0, 0.0), FPS).unwrap();
        assert!((tempo - 120.0).abs() < 5.0, "obtenu {tempo}");
    }

    #[test]
    fn retrouve_un_tempo_lent() {
        let tempo = estimate_tempo(&pulses(75.0, 30.0, 0.0), FPS).unwrap();
        assert!((tempo - 75.0).abs() < 5.0, "obtenu {tempo}");
    }

    #[test]
    fn retrouve_un_tempo_rapide() {
        let tempo = estimate_tempo(&pulses(160.0, 30.0, 0.0), FPS).unwrap();
        assert!((tempo - 160.0).abs() < 6.0, "obtenu {tempo}");
    }

    #[test]
    fn resiste_a_une_imprecision_rythmique_humaine() {
        // Un batteur humain ne tombe jamais exactement sur la grille.
        let tempo = estimate_tempo(&pulses(100.0, 30.0, 0.8), FPS).unwrap();
        assert!((tempo - 100.0).abs() < 8.0, "obtenu {tempo}");
    }

    #[test]
    fn ne_part_pas_a_loctave_sur_un_tempo_courant() {
        // Le piège classique : répondre 65 ou 260 pour un morceau à 130.
        let tempo = estimate_tempo(&pulses(130.0, 30.0, 0.0), FPS).unwrap();

        assert!(
            (tempo - 130.0).abs() < 8.0,
            "erreur d'octave rythmique : obtenu {tempo}"
        );
    }

    #[test]
    fn ne_pretend_rien_sur_un_signal_sans_pulsation() {
        // Enveloppe plate : aucune attaque, donc aucune périodicité.
        let plat = vec![0.5_f32; (30.0 * FPS) as usize];
        assert_eq!(estimate_tempo(&plat, FPS), None);
    }

    #[test]
    fn ne_pretend_rien_sur_du_bruit_sans_periodicite() {
        // Enveloppe aléatoire : un maximum existe, mais il ne ressort pas du
        // fond. C'est précisément le cas d'une nappe ambiante.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let bruit: Vec<f32> = (0..(30.0 * FPS) as usize)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state >> 40) as f32 / 16_777_216.0
            })
            .collect();

        assert_eq!(estimate_tempo(&bruit, FPS), None);
    }

    #[test]
    fn ne_pretend_rien_sur_un_extrait_trop_court() {
        assert_eq!(estimate_tempo(&pulses(120.0, 1.0, 0.0), FPS), None);
    }

    #[test]
    fn la_ponderation_favorise_les_tempos_courants() {
        assert!(tempo_prior(120.0) > tempo_prior(60.0));
        assert!(tempo_prior(120.0) > tempo_prior(240.0));
        assert!((tempo_prior(120.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn une_pulsation_reguliere_est_plus_forte_quune_pulsation_absente() {
        let regulier = beat_strength(&pulses(120.0, 30.0, 0.0), 120.0, FPS);
        let plat = beat_strength(&vec![0.5_f32; (30.0 * FPS) as usize], 120.0, FPS);

        assert!(
            regulier > plat,
            "régulier {regulier}, plat {plat}"
        );
    }

    #[test]
    fn la_force_de_pulsation_reste_bornee() {
        for bpm in [60.0, 120.0, 180.0_f32] {
            let force = beat_strength(&pulses(bpm, 30.0, 0.3), bpm, FPS);
            assert!((0.0..=1.0).contains(&force), "obtenu {force}");
        }
    }
}
