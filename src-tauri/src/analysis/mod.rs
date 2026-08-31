//! Analyse du signal audio — la seule source de recommandation qui fonctionne
//! avec **zéro historique d'écoute**.
//!
//! # Pourquoi ce module est le socle du moteur
//!
//! Spotify recommande grâce au filtrage collaboratif : « les gens qui aiment X
//! aiment aussi Y », calculé sur des centaines de millions d'utilisateurs.
//! Onzer n'a qu'un utilisateur. Cette approche est donc structurellement hors
//! d'atteinte.
//!
//! Ce qui reste, et qui fonctionne dès le premier jour, c'est **le son
//! lui-même**. Deux morceaux dont les enveloppes spectrales se ressemblent se
//! ressemblent aussi à l'oreille — indépendamment de tout historique.
//!
//! # Le vecteur produit
//!
//! | Composantes | Dimensions | Ce qu'elles portent |
//! |---|---|---|
//! | MFCC moyens et écarts-types | 26 | Le timbre, et sa variabilité |
//! | Descripteurs spectraux (moy. + é.-t.) | 16 | Brillance, bruitage, dynamique |
//! | Chroma moyen | 12 | Le contenu harmonique |
//! | **Total** | **54** | |
//!
//! Les valeurs sont stockées **brutes**. Leur normalisation dépend de la
//! bibliothèque entière et n'a donc de sens qu'au moment de la comparaison,
//! dans le moteur de recommandation.

pub mod decode;
pub mod key;
pub mod spectral;
pub mod tempo;

use std::path::Path;

use crate::core::{OnzerError, Result};

use key::DetectedKey;
use spectral::{FrameFeatures, SpectralAnalyzer, FRAME_SIZE, HOP_SIZE, MFCC_COUNT};

/// Dimension du vecteur de similarité.
pub const EMBEDDING_DIM: usize = 2 * MFCC_COUNT + 16 + 12;

/// Version de l'algorithme d'extraction.
///
/// Incrémentée à chaque changement modifiant les valeurs produites : elle
/// permet de savoir exactement quels morceaux réanalyser, sans jamais mélanger
/// des vecteurs incomparables.
pub const ANALYZER_VERSION: i64 = 1;

pub const ANALYZER_NAME: &str = "onzer-native";

/// Descripteurs d'un morceau.
#[derive(Debug, Clone)]
pub struct AudioFeatures {
    /// Vecteur de similarité, valeurs brutes.
    pub embedding: Vec<f32>,

    /// Tempo en BPM. `None` quand aucune pulsation n'est décelable — une nappe
    /// ambiante n'a pas de tempo, et en inventer un serait pire que rien.
    pub tempo: Option<f32>,

    /// Niveau sonore moyen, en dBFS. Valeur physique, non heuristique.
    pub loudness: f32,

    /// Tonalité détectée, ou `None` sur un morceau trop bruité.
    pub key: Option<DetectedKey>,

    /// ⚠️ Les trois valeurs suivantes sont des **heuristiques perceptives**,
    /// pas des mesures. Elles combinent des grandeurs physiques réelles selon
    /// des pondérations choisies à la main, sans modèle appris. Elles sont
    /// utiles pour trier et filtrer, mais ne prétendent pas à l'exactitude d'un
    /// jugement humain.
    pub energy: f32,
    pub danceability: f32,
    pub valence: f32,
}

impl AudioFeatures {
    /// Sérialise le vecteur pour la base (f32 en petit-boutiste).
    pub fn embedding_bytes(&self) -> Vec<u8> {
        self.embedding
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    /// Relit un vecteur depuis la base.
    pub fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    }
}

/// Abstraction de l'extraction (ADR-002).
///
/// Le moteur de recommandation ne connaît **que** ce trait : changer d'algorithme
/// d'analyse ne touchera aucune de ses lignes.
pub trait FeatureExtractor: Send + Sync {
    fn name(&self) -> &'static str;
    fn version(&self) -> i64;
    fn extract(&self, path: &Path) -> Result<AudioFeatures>;
}

/// Extracteur natif, 100 % Rust.
///
/// Écrit sur mesure plutôt que d'utiliser `bliss-audio`, qui aurait imposé
/// d'empaqueter FFmpeg dans l'application.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeExtractor;

impl FeatureExtractor for NativeExtractor {
    fn name(&self) -> &'static str {
        ANALYZER_NAME
    }

    fn version(&self) -> i64 {
        ANALYZER_VERSION
    }

    fn extract(&self, path: &Path) -> Result<AudioFeatures> {
        let audio = decode::decode_for_analysis(path)?;
        analyze_samples(&audio.samples, audio.sample_rate)
    }
}

/// Analyse un signal mono déjà décodé.
///
/// Séparé de la lecture de fichier pour pouvoir être éprouvé sur des signaux
/// synthétiques dont la réponse attendue est connue.
pub fn analyze_samples(samples: &[f32], sample_rate: u32) -> Result<AudioFeatures> {
    if samples.len() < FRAME_SIZE * 4 {
        return Err(OnzerError::Invalid(
            "extrait trop court pour être analysé".to_string(),
        ));
    }

    let mut analyzer = SpectralAnalyzer::new(sample_rate);
    let mut frames: Vec<FrameFeatures> = Vec::new();

    for start in (0..samples.len().saturating_sub(FRAME_SIZE)).step_by(HOP_SIZE) {
        frames.push(analyzer.analyze_frame(&samples[start..start + FRAME_SIZE]));
    }

    if frames.is_empty() {
        return Err(OnzerError::Invalid("aucune fenêtre analysable".to_string()));
    }

    let frames_per_second = sample_rate as f32 / HOP_SIZE as f32;
    let onset_envelope: Vec<f32> = frames.iter().map(|frame| frame.flux).collect();

    let tempo = tempo::estimate_tempo(&onset_envelope, frames_per_second);
    let beat_strength = tempo
        .map(|bpm| tempo::beat_strength(&onset_envelope, bpm, frames_per_second))
        .unwrap_or(0.0);

    // ── Agrégation ──────────────────────────────────────────────────────
    let mut embedding = Vec::with_capacity(EMBEDDING_DIM);

    for coefficient in 0..MFCC_COUNT {
        let values: Vec<f32> = frames.iter().map(|f| f.mfcc[coefficient]).collect();
        embedding.push(mean(&values));
        embedding.push(standard_deviation(&values));
    }

    let rms: Vec<f32> = frames.iter().map(|f| f.rms).collect();
    let centroid: Vec<f32> = frames.iter().map(|f| f.centroid).collect();
    let spread: Vec<f32> = frames.iter().map(|f| f.spread).collect();
    let rolloff: Vec<f32> = frames.iter().map(|f| f.rolloff).collect();
    let flatness: Vec<f32> = frames.iter().map(|f| f.flatness).collect();
    let flux: Vec<f32> = frames.iter().map(|f| f.flux).collect();
    let zcr: Vec<f32> = frames.iter().map(|f| f.zcr).collect();

    for series in [&rms, &centroid, &spread, &rolloff, &flatness, &flux, &zcr] {
        embedding.push(mean(series));
        embedding.push(standard_deviation(series));
    }
    // Deux dimensions pour le tempo : sa valeur et le fait de le connaître.
    embedding.push(tempo.unwrap_or(0.0));
    embedding.push(beat_strength);

    let mut chroma_mean = [0.0_f32; 12];
    for frame in &frames {
        for (class, value) in frame.chroma.iter().enumerate() {
            chroma_mean[class] += value;
        }
    }
    for value in chroma_mean.iter_mut() {
        *value /= frames.len() as f32;
    }
    embedding.extend_from_slice(&chroma_mean);

    debug_assert_eq!(embedding.len(), EMBEDDING_DIM);

    // ── Grandeurs dérivées ──────────────────────────────────────────────
    let rms_mean = mean(&rms);
    let loudness = 20.0 * (rms_mean.max(1e-6)).log10();
    let detected_key = key::detect_key(&chroma_mean);

    Ok(AudioFeatures {
        embedding,
        tempo,
        loudness,
        key: detected_key,
        energy: estimate_energy(rms_mean, mean(&rolloff), mean(&flux)),
        danceability: estimate_danceability(tempo, beat_strength),
        valence: estimate_valence(detected_key, mean(&centroid), tempo),
        })
}

// ════════════════════════════════════════════════════════════════════════════
//  Heuristiques perceptives
//
//  Ces trois fonctions combinent des grandeurs physiques mesurées selon des
//  pondérations choisies à la main. Ce ne sont pas des modèles appris, et elles
//  sont documentées comme telles : l'objectif est de trier correctement, pas de
//  prétendre lire dans les pensées de l'auditeur.
// ════════════════════════════════════════════════════════════════════════════

/// Intensité perçue : volume, richesse en aigus, agitation.
fn estimate_energy(rms_mean: f32, rolloff_mean: f32, flux_mean: f32) -> f32 {
    let volume = (rms_mean * 4.0).clamp(0.0, 1.0);
    let brightness = (rolloff_mean / 6_000.0).clamp(0.0, 1.0);
    let agitation = (flux_mean * 20.0).clamp(0.0, 1.0);

    (volume * 0.5 + brightness * 0.3 + agitation * 0.2).clamp(0.0, 1.0)
}

/// Aptitude à la danse : une pulsation à la fois **régulière** et dans une
/// plage de tempo qui invite au mouvement.
fn estimate_danceability(tempo: Option<f32>, beat_strength: f32) -> f32 {
    let Some(bpm) = tempo else {
        // Sans pulsation décelable, on ne danse pas.
        return 0.0;
    };

    // Cloche centrée sur 120 BPM : à 90 ou 150 on danse encore, à 50 ou 200
    // beaucoup moins.
    let tempo_fit = (-0.5 * ((bpm - 120.0) / 45.0_f32).powi(2)).exp();

    (beat_strength.clamp(0.0, 1.0) * 0.65 + tempo_fit * 0.35).clamp(0.0, 1.0)
}

/// Positivité perçue.
///
/// Le mode majeur ou mineur en est le meilleur prédicteur objectif disponible,
/// complété par la brillance et le tempo. Reste une approximation : un morceau
/// mineur rapide peut être euphorique.
fn estimate_valence(key: Option<DetectedKey>, centroid_mean: f32, tempo: Option<f32>) -> f32 {
    let mode = match key {
        Some(k) if k.is_major => 0.22,
        Some(_) => -0.22,
        None => 0.0,
    };

    let brightness = ((centroid_mean / 3_000.0).clamp(0.0, 1.0) - 0.5) * 0.24;
    let drive = tempo
        .map(|bpm| ((bpm - 100.0) / 100.0).clamp(-1.0, 1.0) * 0.14)
        .unwrap_or(0.0);

    (0.5 + mode + brightness + drive).clamp(0.0, 1.0)
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

fn standard_deviation(values: &[f32]) -> f32 {
    if values.len() < 2 {
        return 0.0;
    }
    let average = mean(values);
    (values.iter().map(|v| (v - average).powi(2)).sum::<f32>() / values.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = decode::ANALYSIS_SAMPLE_RATE;

    /// Signal synthétique : somme de sinusoïdes, éventuellement pulsée.
    fn synth(seconds: f32, partials: &[(f32, f32)], pulse_bpm: Option<f32>) -> Vec<f32> {
        let count = (seconds * RATE as f32) as usize;

        (0..count)
            .map(|index| {
                let t = index as f32 / RATE as f32;
                let mut sample: f32 = partials
                    .iter()
                    .map(|(frequency, amplitude)| {
                        (std::f32::consts::TAU * frequency * t).sin() * amplitude
                    })
                    .sum();

                if let Some(bpm) = pulse_bpm {
                    // Enveloppe percussive : décroissance exponentielle à chaque
                    // temps.
                    let period = 60.0 / bpm;
                    let phase = (t % period) / period;
                    sample *= (-phase * 12.0).exp() + 0.05;
                }

                sample
            })
            .collect()
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm_a * norm_b).max(1e-9)
    }

    #[test]
    fn produit_un_vecteur_de_la_dimension_annoncee() {
        let features = analyze_samples(&synth(5.0, &[(440.0, 0.5)], None), RATE).unwrap();

        assert_eq!(features.embedding.len(), EMBEDDING_DIM);
        assert!(features.embedding.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn refuse_un_extrait_trop_court() {
        assert!(analyze_samples(&[0.0; 100], RATE).is_err());
    }

    // ── La propriété qui fait tout le moteur ────────────────────────────

    #[test]
    fn deux_sons_semblables_sont_plus_proches_que_deux_sons_differents() {
        // Même timbre (harmoniques identiques), hauteur voisine.
        let a = analyze_samples(&synth(5.0, &[(220.0, 0.5), (440.0, 0.25)], None), RATE).unwrap();
        let b = analyze_samples(&synth(5.0, &[(233.0, 0.5), (466.0, 0.25)], None), RATE).unwrap();

        // Timbre radicalement différent : spectre dense et aigu.
        let c = analyze_samples(
            &synth(5.0, &[(3000.0, 0.4), (4500.0, 0.4), (6000.0, 0.4)], None),
            RATE,
        )
        .unwrap();

        let proche = cosine_similarity(&a.embedding, &b.embedding);
        let lointain = cosine_similarity(&a.embedding, &c.embedding);

        assert!(
            proche > lointain,
            "similarité — proche {proche}, lointain {lointain}"
        );
    }

    #[test]
    fn un_morceau_est_maximalement_similaire_a_lui_meme() {
        let features = analyze_samples(&synth(5.0, &[(440.0, 0.5)], None), RATE).unwrap();
        let similarite = cosine_similarity(&features.embedding, &features.embedding);

        assert!((similarite - 1.0).abs() < 1e-4, "obtenu {similarite}");
    }

    // ── Grandeurs dérivées ──────────────────────────────────────────────

    #[test]
    fn un_son_fort_est_plus_energique_quun_son_faible() {
        let fort = analyze_samples(&synth(5.0, &[(440.0, 0.9)], None), RATE).unwrap();
        let faible = analyze_samples(&synth(5.0, &[(440.0, 0.05)], None), RATE).unwrap();

        assert!(fort.energy > faible.energy);
        assert!(fort.loudness > faible.loudness);
    }

    #[test]
    fn le_niveau_sonore_est_exprime_en_dbfs_negatifs() {
        let features = analyze_samples(&synth(5.0, &[(440.0, 0.5)], None), RATE).unwrap();

        // Un signal d'amplitude 0,5 se situe autour de −9 dBFS.
        assert!(
            (-20.0..0.0).contains(&features.loudness),
            "obtenu {}",
            features.loudness
        );
    }

    #[test]
    fn un_morceau_pulse_est_plus_dansant_quune_nappe_continue() {
        let pulse = analyze_samples(&synth(20.0, &[(200.0, 0.6)], Some(120.0)), RATE).unwrap();
        let nappe = analyze_samples(&synth(20.0, &[(200.0, 0.6)], None), RATE).unwrap();

        assert!(
            pulse.danceability > nappe.danceability,
            "pulsé {}, nappe {}",
            pulse.danceability,
            nappe.danceability
        );
    }

    #[test]
    fn nattribue_pas_de_tempo_a_une_nappe_continue() {
        let nappe = analyze_samples(&synth(20.0, &[(200.0, 0.6)], None), RATE).unwrap();

        assert_eq!(
            nappe.danceability, 0.0,
            "sans pulsation décelable, la dansabilité doit être nulle"
        );
    }

    #[test]
    fn retrouve_le_tempo_dun_signal_pulse() {
        let features = analyze_samples(&synth(25.0, &[(200.0, 0.6)], Some(128.0)), RATE).unwrap();

        let tempo = features.tempo.expect("un signal pulsé doit avoir un tempo");
        assert!((tempo - 128.0).abs() < 12.0, "obtenu {tempo}");
    }

    #[test]
    fn toutes_les_grandeurs_perceptives_restent_bornees() {
        for partials in [
            vec![(440.0_f32, 0.5_f32)],
            vec![(60.0, 0.9)],
            vec![(8000.0, 0.3)],
        ] {
            let features = analyze_samples(&synth(6.0, &partials, Some(100.0)), RATE).unwrap();

            for (nom, valeur) in [
                ("energy", features.energy),
                ("danceability", features.danceability),
                ("valence", features.valence),
            ] {
                assert!(
                    (0.0..=1.0).contains(&valeur),
                    "{nom} hors bornes : {valeur}"
                );
            }
        }
    }

    // ── Sérialisation ───────────────────────────────────────────────────

    #[test]
    fn le_vecteur_survit_a_un_aller_retour_en_base() {
        let features = analyze_samples(&synth(5.0, &[(440.0, 0.5)], None), RATE).unwrap();
        let restitue = AudioFeatures::decode_embedding(&features.embedding_bytes());

        assert_eq!(restitue, features.embedding);
    }

    #[test]
    fn un_vecteur_tronque_ne_provoque_pas_de_panique() {
        // Trois octets : moins qu'un f32 complet.
        assert!(AudioFeatures::decode_embedding(&[1, 2, 3]).is_empty());
    }
}
