//! Descripteurs spectraux : le vocabulaire dans lequel deux morceaux se
//! ressemblent ou non.
//!
//! # Ce que chaque descripteur capture réellement
//!
//! | Descripteur | Perception | Distingue par exemple |
//! |---|---|---|
//! | **MFCC** | Le *timbre* — la « couleur » du son | Une guitare acoustique d'un synthé |
//! | **Centroïde** | La brillance | Un morceau sourd d'un morceau claquant |
//! | **Platitude** | Bruit contre harmonie | Une caisse claire d'une nappe de cordes |
//! | **Rolloff** | Répartition de l'énergie | Un mix chargé en aigus d'un mix chaud |
//! | **Chroma** | Le contenu harmonique | Do majeur de La mineur |
//! | **Flux** | Le taux de changement | Un morceau statique d'un morceau agité |
//! | **ZCR** | Contenu percussif ou bruité | Une voix chantée d'une voix chuchotée |
//!
//! Les MFCC portent l'essentiel de la ressemblance perçue : ils décrivent
//! l'enveloppe du spectre en écartant la hauteur des notes, ce qui fait que
//! deux morceaux du même artiste se ressemblent même dans des tonalités
//! différentes.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Taille de fenêtre d'analyse. À 22 050 Hz, 2048 échantillons ≈ 93 ms :
/// assez long pour une résolution fréquentielle correcte (≈ 10 Hz), assez
/// court pour que le contenu reste stationnaire.
pub const FRAME_SIZE: usize = 2048;

/// Avance entre deux fenêtres : 512 échantillons ≈ 23 ms, soit environ 43
/// analyses par seconde. Nécessaire pour détecter les attaques rythmiques.
pub const HOP_SIZE: usize = 512;

/// Nombre de bandes mel. 40 est la valeur usuelle en analyse musicale.
const MEL_BANDS: usize = 40;

/// Coefficients cepstraux conservés. Le premier porte l'énergie globale et est
/// écarté : il ferait dépendre la ressemblance du simple niveau sonore.
pub const MFCC_COUNT: usize = 13;

/// Descripteurs calculés sur une fenêtre.
#[derive(Debug, Clone)]
pub struct FrameFeatures {
    pub rms: f32,
    pub centroid: f32,
    pub spread: f32,
    pub rolloff: f32,
    pub flatness: f32,
    pub flux: f32,
    pub zcr: f32,
    pub mfcc: [f32; MFCC_COUNT],
    pub chroma: [f32; 12],
}

/// Analyseur réutilisable : les tables coûteuses (fenêtre, banc de filtres,
/// plan FFT) sont calculées une seule fois pour tout un morceau.
pub struct SpectralAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// Banc de filtres mel, en représentation creuse : pour chaque bande, les
    /// bins concernés et leur poids.
    mel_filters: Vec<Vec<(usize, f32)>>,
    /// Classe de hauteur (0 = Do) associée à chaque bin, quand elle existe.
    chroma_map: Vec<Option<usize>>,
    bin_frequencies: Vec<f32>,
    /// Spectre de la fenêtre précédente, pour le flux.
    previous_magnitudes: Vec<f32>,
    scratch: Vec<Complex<f32>>,
}

impl SpectralAnalyzer {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FRAME_SIZE);

        let bins = FRAME_SIZE / 2 + 1;
        let bin_frequencies: Vec<f32> = (0..bins)
            .map(|bin| bin as f32 * sample_rate as f32 / FRAME_SIZE as f32)
            .collect();

        Self {
            fft,
            window: hann_window(FRAME_SIZE),
            mel_filters: mel_filterbank(&bin_frequencies, sample_rate),
            chroma_map: chroma_mapping(&bin_frequencies),
            bin_frequencies,
            previous_magnitudes: vec![0.0; bins],
            scratch: vec![Complex::new(0.0, 0.0); FRAME_SIZE],
        }
    }

    /// Analyse une fenêtre de [`FRAME_SIZE`] échantillons.
    pub fn analyze_frame(&mut self, frame: &[f32]) -> FrameFeatures {
        debug_assert_eq!(frame.len(), FRAME_SIZE);

        // ── Domaine temporel ────────────────────────────────────────────
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        let zcr = zero_crossing_rate(frame);

        // ── Transformée ─────────────────────────────────────────────────
        for (index, sample) in frame.iter().enumerate() {
            self.scratch[index] = Complex::new(sample * self.window[index], 0.0);
        }
        self.fft.process(&mut self.scratch);

        let bins = self.bin_frequencies.len();
        let magnitudes: Vec<f32> = self.scratch[..bins].iter().map(|c| c.norm()).collect();

        // ── Descripteurs spectraux ──────────────────────────────────────
        let total: f32 = magnitudes.iter().sum();

        let centroid = if total > 0.0 {
            magnitudes
                .iter()
                .zip(&self.bin_frequencies)
                .map(|(magnitude, frequency)| magnitude * frequency)
                .sum::<f32>()
                / total
        } else {
            0.0
        };

        let spread = if total > 0.0 {
            (magnitudes
                .iter()
                .zip(&self.bin_frequencies)
                .map(|(magnitude, frequency)| magnitude * (frequency - centroid).powi(2))
                .sum::<f32>()
                / total)
                .sqrt()
        } else {
            0.0
        };

        let rolloff = spectral_rolloff(&magnitudes, &self.bin_frequencies, 0.85);
        let flatness = spectral_flatness(&magnitudes);

        // Flux : somme des seules variations **positives**. Une note qui
        // s'éteint n'est pas un événement rythmique ; une note qui apparaît, si.
        let flux = magnitudes
            .iter()
            .zip(&self.previous_magnitudes)
            .map(|(current, previous)| (current - previous).max(0.0))
            .sum::<f32>()
            / bins as f32;
        self.previous_magnitudes.copy_from_slice(&magnitudes);

        // ── Mel, MFCC, chroma ───────────────────────────────────────────
        let mut mel_energies = [0.0_f32; MEL_BANDS];
        for (band, filter) in self.mel_filters.iter().enumerate() {
            let energy: f32 = filter
                .iter()
                .map(|(bin, weight)| magnitudes[*bin] * weight)
                .sum();
            // Le logarithme reproduit la perception : un doublement d'énergie
            // n'est pas perçu comme un doublement de volume.
            mel_energies[band] = (energy + 1e-10).ln();
        }

        let mfcc = discrete_cosine_transform(&mel_energies);

        let mut chroma = [0.0_f32; 12];
        for (bin, class) in self.chroma_map.iter().enumerate() {
            if let Some(class) = class {
                chroma[*class] += magnitudes[bin];
            }
        }
        normalize_in_place(&mut chroma);

        FrameFeatures {
            rms,
            centroid,
            spread,
            rolloff,
            flatness,
            flux,
            zcr,
            mfcc,
            chroma,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Primitives
// ════════════════════════════════════════════════════════════════════════════

/// Fenêtre de Hann : atténue les bords de la fenêtre pour éviter que la
/// troncature brutale du signal ne crée de fausses fréquences.
fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| {
            let phase = std::f32::consts::TAU * index as f32 / size as f32;
            0.5 * (1.0 - phase.cos())
        })
        .collect()
}

fn zero_crossing_rate(frame: &[f32]) -> f32 {
    let crossings = frame
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count();

    crossings as f32 / (frame.len() - 1) as f32
}

/// Fréquence en dessous de laquelle se trouve `ratio` de l'énergie.
fn spectral_rolloff(magnitudes: &[f32], frequencies: &[f32], ratio: f32) -> f32 {
    let threshold = magnitudes.iter().sum::<f32>() * ratio;
    let mut cumulative = 0.0;

    for (bin, magnitude) in magnitudes.iter().enumerate() {
        cumulative += magnitude;
        if cumulative >= threshold {
            return frequencies[bin];
        }
    }

    *frequencies.last().unwrap_or(&0.0)
}

/// Rapport entre moyenne géométrique et moyenne arithmétique du spectre.
///
/// Proche de 1 pour un bruit blanc (énergie répartie partout), proche de 0 pour
/// un son harmonique (énergie concentrée sur quelques pics). C'est le
/// discriminant le plus net entre percussion et instrument tenu.
fn spectral_flatness(magnitudes: &[f32]) -> f32 {
    let usable: Vec<f32> = magnitudes.iter().map(|m| m + 1e-10).collect();

    let arithmetic = usable.iter().sum::<f32>() / usable.len() as f32;
    if arithmetic <= 1e-9 {
        return 0.0;
    }

    // Moyenne géométrique calculée en logarithmes : le produit direct de 1025
    // termes déborderait immédiatement.
    let log_mean = usable.iter().map(|m| m.ln()).sum::<f32>() / usable.len() as f32;

    (log_mean.exp() / arithmetic).clamp(0.0, 1.0)
}

/// Conversion Hertz → mel (échelle perceptive des hauteurs).
fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

/// Banc de filtres triangulaires régulièrement espacés sur l'échelle mel.
fn mel_filterbank(bin_frequencies: &[f32], sample_rate: u32) -> Vec<Vec<(usize, f32)>> {
    let min_hz = 20.0;
    let max_hz = (sample_rate as f32 / 2.0).min(11_025.0);

    let min_mel = hz_to_mel(min_hz);
    let max_mel = hz_to_mel(max_hz);

    // MELS_BANDS + 2 points : chaque filtre s'appuie sur trois points
    // consécutifs (début, sommet, fin).
    let points: Vec<f32> = (0..MEL_BANDS + 2)
        .map(|index| {
            let mel = min_mel + (max_mel - min_mel) * index as f32 / (MEL_BANDS + 1) as f32;
            mel_to_hz(mel)
        })
        .collect();

    (0..MEL_BANDS)
        .map(|band| {
            let (left, center, right) = (points[band], points[band + 1], points[band + 2]);

            bin_frequencies
                .iter()
                .enumerate()
                .filter_map(|(bin, &frequency)| {
                    let weight = if frequency >= left && frequency <= center && center > left {
                        (frequency - left) / (center - left)
                    } else if frequency > center && frequency <= right && right > center {
                        (right - frequency) / (right - center)
                    } else {
                        0.0
                    };

                    (weight > 0.0).then_some((bin, weight))
                })
                .collect()
        })
        .collect()
}

/// Transformée en cosinus discrète (type II).
///
/// Décorrèle les bandes mel : les coefficients obtenus décrivent la *forme* de
/// l'enveloppe spectrale plutôt que l'énergie de chaque bande.
fn discrete_cosine_transform(mel_energies: &[f32; MEL_BANDS]) -> [f32; MFCC_COUNT] {
    let mut coefficients = [0.0_f32; MFCC_COUNT];
    let bands = MEL_BANDS as f32;

    for (k, coefficient) in coefficients.iter_mut().enumerate() {
        *coefficient = mel_energies
            .iter()
            .enumerate()
            .map(|(n, energy)| {
                let phase =
                    std::f32::consts::PI * k as f32 * (n as f32 + 0.5) / bands;
                energy * phase.cos()
            })
            .sum::<f32>()
            / bands;
    }

    coefficients
}

/// Associe chaque bin à une classe de hauteur (0 = Do, 9 = La).
///
/// La référence est le La 440 Hz, note MIDI 69.
fn chroma_mapping(bin_frequencies: &[f32]) -> Vec<Option<usize>> {
    bin_frequencies
        .iter()
        .map(|&frequency| {
            // En deçà de 55 Hz la résolution fréquentielle est trop grossière
            // pour identifier une note ; au-delà de 5 kHz il n'y a plus de
            // fondamentales, seulement des harmoniques.
            if !(55.0..=5_000.0).contains(&frequency) {
                return None;
            }

            let midi = 69.0 + 12.0 * (frequency / 440.0).log2();
            Some((midi.round() as i32).rem_euclid(12) as usize)
        })
        .collect()
}

/// Ramène un vecteur à une somme de 1, ou le laisse nul.
fn normalize_in_place(values: &mut [f32]) {
    let total: f32 = values.iter().sum();
    if total > 1e-9 {
        for value in values.iter_mut() {
            *value /= total;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::decode::ANALYSIS_SAMPLE_RATE;

    /// Sinusoïde pure à la fréquence demandée.
    fn sine(frequency: f32) -> Vec<f32> {
        (0..FRAME_SIZE)
            .map(|index| {
                let phase = std::f32::consts::TAU * frequency * index as f32
                    / ANALYSIS_SAMPLE_RATE as f32;
                phase.sin()
            })
            .collect()
    }

    /// Bruit blanc déterministe, pour que le test soit reproductible.
    fn white_noise() -> Vec<f32> {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        (0..FRAME_SIZE)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0 - 1.0
            })
            .collect()
    }

    fn analyze(frame: &[f32]) -> FrameFeatures {
        SpectralAnalyzer::new(ANALYSIS_SAMPLE_RATE).analyze_frame(frame)
    }

    // ── Centroïde : la mesure de brillance ──────────────────────────────

    #[test]
    fn le_centroide_dune_sinusoide_tombe_sur_sa_frequence() {
        let features = analyze(&sine(1000.0));

        assert!(
            (features.centroid - 1000.0).abs() < 60.0,
            "centroïde attendu proche de 1000 Hz, obtenu {}",
            features.centroid
        );
    }

    #[test]
    fn un_son_aigu_a_un_centroide_plus_haut_quun_son_grave() {
        let grave = analyze(&sine(200.0)).centroid;
        let aigu = analyze(&sine(4000.0)).centroid;

        assert!(aigu > grave * 5.0, "grave {grave}, aigu {aigu}");
    }

    // ── Platitude : harmonique contre bruité ────────────────────────────

    #[test]
    fn le_bruit_blanc_est_bien_plus_plat_quune_sinusoide() {
        let harmonique = analyze(&sine(440.0)).flatness;
        let bruit = analyze(&white_noise()).flatness;

        assert!(
            bruit > harmonique * 10.0,
            "platitude — sinusoïde {harmonique}, bruit {bruit}"
        );
    }

    #[test]
    fn la_platitude_reste_bornee() {
        for frame in [sine(440.0), white_noise(), vec![0.0; FRAME_SIZE]] {
            let flatness = analyze(&frame).flatness;
            assert!((0.0..=1.0).contains(&flatness), "obtenu {flatness}");
        }
    }

    // ── Chroma : le contenu harmonique ──────────────────────────────────

    #[test]
    fn un_la_440_illumine_la_classe_de_hauteur_du_la() {
        let features = analyze(&sine(440.0));

        // 9 = La, en partant de Do = 0.
        let dominante = features
            .chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(class, _)| class)
            .unwrap();

        assert_eq!(dominante, 9, "chroma obtenu : {:?}", features.chroma);
    }

    #[test]
    fn un_do_illumine_la_classe_du_do() {
        // Do5 = 523,25 Hz.
        let features = analyze(&sine(523.25));
        let dominante = features
            .chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(class, _)| class)
            .unwrap();

        assert_eq!(dominante, 0);
    }

    #[test]
    fn le_chroma_est_normalise() {
        let features = analyze(&sine(440.0));
        let somme: f32 = features.chroma.iter().sum();

        assert!((somme - 1.0).abs() < 1e-4, "somme obtenue {somme}");
    }

    // ── Énergie et taux de passages par zéro ────────────────────────────

    #[test]
    fn le_silence_na_ni_energie_ni_brillance() {
        let features = analyze(&vec![0.0; FRAME_SIZE]);

        assert_eq!(features.rms, 0.0);
        assert_eq!(features.centroid, 0.0);
    }

    #[test]
    fn le_rms_dune_sinusoide_vaut_environ_0_707() {
        // Valeur exacte pour une sinusoïde d'amplitude 1 : 1/√2.
        let rms = analyze(&sine(440.0)).rms;
        assert!((rms - 0.707).abs() < 0.02, "obtenu {rms}");
    }

    #[test]
    fn le_taux_de_passages_par_zero_croit_avec_la_frequence() {
        let grave = analyze(&sine(100.0)).zcr;
        let aigu = analyze(&sine(3000.0)).zcr;

        assert!(aigu > grave * 10.0, "grave {grave}, aigu {aigu}");
    }

    // ── MFCC : la signature de timbre ───────────────────────────────────

    fn ecart_mfcc(a: &[f32; MFCC_COUNT], b: &[f32; MFCC_COUNT]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum()
    }

    /// La propriété qui fonde toute la recommandation par similarité : deux
    /// sons de même nature doivent être **nettement** plus proches que deux
    /// sons de natures différentes.
    ///
    /// Formulée en comparaison plutôt qu'en seuil absolu : un seuil chiffré
    /// dépendrait de choix d'implémentation, la relation d'ordre non.
    #[test]
    fn le_timbre_domine_la_hauteur_dans_les_mfcc() {
        let reference = analyze(&sine(1000.0)).mfcc;
        let meme_timbre = analyze(&sine(1020.0)).mfcc; // hauteur voisine
        let autre_timbre = analyze(&white_noise()).mfcc;

        let proche = ecart_mfcc(&reference, &meme_timbre);
        let lointain = ecart_mfcc(&reference, &autre_timbre);

        assert!(
            lointain > proche * 3.0,
            "le timbre doit dominer : même timbre {proche}, autre timbre {lointain}"
        );
    }

    #[test]
    fn les_mfcc_ne_produisent_jamais_de_valeur_invalide() {
        for frame in [sine(440.0), white_noise(), vec![0.0; FRAME_SIZE]] {
            let mfcc = analyze(&frame).mfcc;
            assert!(mfcc.iter().all(|value| value.is_finite()), "{mfcc:?}");
        }
    }

    // ── Flux : la mesure de changement ──────────────────────────────────

    #[test]
    fn un_signal_stable_produit_un_flux_faible() {
        let mut analyzer = SpectralAnalyzer::new(ANALYSIS_SAMPLE_RATE);
        let frame = sine(440.0);

        analyzer.analyze_frame(&frame);
        let stable = analyzer.analyze_frame(&frame).flux;

        // Après le silence → son, un son identique ne crée aucune montée.
        assert!(stable < 0.01, "flux obtenu {stable}");
    }

    #[test]
    fn une_attaque_produit_un_flux_eleve() {
        let mut analyzer = SpectralAnalyzer::new(ANALYSIS_SAMPLE_RATE);

        analyzer.analyze_frame(&vec![0.0; FRAME_SIZE]);
        let attaque = analyzer.analyze_frame(&white_noise()).flux;

        assert!(attaque > 0.01, "flux obtenu {attaque}");
    }

    // ── Banc de filtres ─────────────────────────────────────────────────

    #[test]
    fn le_banc_mel_couvre_tout_le_spectre_sans_bande_vide() {
        let frequencies: Vec<f32> = (0..FRAME_SIZE / 2 + 1)
            .map(|bin| bin as f32 * ANALYSIS_SAMPLE_RATE as f32 / FRAME_SIZE as f32)
            .collect();

        let filters = mel_filterbank(&frequencies, ANALYSIS_SAMPLE_RATE);

        assert_eq!(filters.len(), MEL_BANDS);
        for (band, filter) in filters.iter().enumerate() {
            assert!(!filter.is_empty(), "la bande mel {band} est vide");
        }
    }

    #[test]
    fn la_conversion_mel_est_reversible() {
        for hz in [50.0, 440.0, 1000.0, 8000.0_f32] {
            let retour = mel_to_hz(hz_to_mel(hz));
            assert!((retour - hz).abs() < hz * 0.01, "{hz} → {retour}");
        }
    }
}
