//! Décodage d'un fichier audio en signal mono exploitable pour l'analyse.
//!
//! L'analyse ne cherche pas la fidélité mais la **représentativité**. Trois
//! réductions volontaires, chacune divisant le coût sans nuire aux descripteurs :
//!
//! | Réduction | Pourquoi c'est sans conséquence |
//! |---|---|
//! | Mono | Le timbre et le rythme ne dépendent pas de la spatialisation |
//! | 22 050 Hz | Couvre jusqu'à 11 kHz : au-delà, il n'y a plus d'information musicale utile, seulement du sifflement de cymbales |
//! | Extrait de 90 s | Un morceau est stylistiquement homogène ; analyser 6 minutes n'apprend rien de plus que 90 s |
//!
//! L'extrait démarre à 10 % du morceau : les dix premières secondes sont
//! souvent une intro atypique, voire du silence.

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::core::{OnzerError, Result};

/// Fréquence d'échantillonnage de travail.
pub const ANALYSIS_SAMPLE_RATE: u32 = 22_050;

/// Durée maximale analysée.
const MAX_EXCERPT_SECONDS: f32 = 90.0;

/// Proportion du morceau ignorée en tête, pour éviter les intros atypiques.
const SKIP_INTRO_RATIO: f32 = 0.10;

/// Paramètres de décodage.
///
/// L'analyse de features et l'empreinte acoustique ont des besoins opposés sur
/// un point précis : la première saute l'intro pour être représentative, la
/// seconde **doit partir de zéro** pour correspondre à la référence indexée par
/// AcoustID.
#[derive(Debug, Clone, Copy)]
pub struct DecodeRequest {
    pub target_rate: u32,
    pub max_seconds: f32,
    /// Proportion du morceau ignorée en tête. Zéro pour une empreinte.
    pub skip_intro_ratio: f32,
}

/// Signal mono prêt à analyser.
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// Durée totale du fichier, et non de l'extrait.
    pub duration_seconds: f32,
}

/// Décode un fichier en mono, aux paramètres d'analyse.
pub fn decode_for_analysis(path: &Path) -> Result<DecodedAudio> {
    decode(
        path,
        DecodeRequest {
            target_rate: ANALYSIS_SAMPLE_RATE,
            max_seconds: MAX_EXCERPT_SECONDS,
            skip_intro_ratio: SKIP_INTRO_RATIO,
        },
    )
}

/// Décode un fichier en mono, à la demande.
pub fn decode(path: &Path, request: DecodeRequest) -> Result<DecodedAudio> {
    let file = std::fs::File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| OnzerError::Invalid(format!("format illisible : {error}")))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| OnzerError::Invalid("aucune piste audio".to_string()))?;

    let track_id = track.id;
    let source_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let channels = track.codec_params.channels.map_or(2, |ch| ch.count());

    let total_frames = track.codec_params.n_frames.unwrap_or(0);
    let duration_seconds = if total_frames > 0 {
        total_frames as f32 / source_rate as f32
    } else {
        0.0
    };

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| OnzerError::Invalid(format!("codec non géré : {error}")))?;

    // Bornes de l'extrait, exprimées en échantillons de la source.
    let skip_frames = (duration_seconds * request.skip_intro_ratio * source_rate as f32) as u64;
    let wanted_frames = (request.max_seconds * source_rate as f32) as u64;

    let mut mono = Vec::new();
    let mut frames_seen = 0_u64;
    let mut buffer: Option<SampleBuffer<f32>> = None;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Une trame corrompue au milieu d'un morceau ne doit pas faire
            // échouer l'analyse complète.
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(error) => {
                return Err(OnzerError::Invalid(format!("décodage : {error}")));
            }
        };

        let spec = *decoded.spec();
        let buffer = buffer.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)
        });
        buffer.copy_interleaved_ref(decoded);

        let interleaved = buffer.samples();
        let frame_count = interleaved.len() / channels.max(1);

        for frame in 0..frame_count {
            frames_seen += 1;
            if frames_seen < skip_frames {
                continue;
            }

            // Moyenne des canaux : conversion mono la plus simple et la plus
            // neutre pour l'analyse spectrale.
            let mut sum = 0.0_f32;
            for channel in 0..channels {
                sum += interleaved[frame * channels + channel];
            }
            mono.push(sum / channels as f32);

            if mono.len() as u64 >= wanted_frames {
                break;
            }
        }

        if mono.len() as u64 >= wanted_frames {
            break;
        }
    }

    if mono.is_empty() {
        return Err(OnzerError::Invalid(
            "aucun échantillon décodé".to_string(),
        ));
    }

    let samples = resample_linear(&mono, source_rate, request.target_rate);

    Ok(DecodedAudio {
        samples,
        sample_rate: request.target_rate,
        duration_seconds: if duration_seconds > 0.0 {
            duration_seconds
        } else {
            mono.len() as f32 / source_rate as f32
        },
    })
}

/// Rééchantillonnage par interpolation linéaire.
///
/// Un filtre polyphasé serait meilleur pour de l'écoute, mais on descend ici de
/// 44 kHz à 22 kHz **pour mesurer des moyennes spectrales**. Le repliement
/// résiduel se situe au-dessus de 11 kHz, une zone qui ne porte aucune
/// information de timbre ou de rythme exploitable.
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (input.len() as f64 / ratio).floor() as usize;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let right = (left + 1).min(input.len() - 1);
        let fraction = (position - left as f64) as f32;

        output.push(input[left] * (1.0 - fraction) + input[right] * fraction);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_reechantillonnage_divise_la_longueur_par_le_rapport() {
        let input: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect();
        let output = resample_linear(&input, 44_100, 22_050);

        assert_eq!(output.len(), 22_050);
    }

    #[test]
    fn le_reechantillonnage_preserve_une_rampe() {
        // Une rampe linéaire doit rester une rampe linéaire : c'est le cas où
        // l'interpolation linéaire est exacte.
        let input: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let output = resample_linear(&input, 100, 50);

        assert_eq!(output.len(), 50);
        for (index, value) in output.iter().enumerate() {
            assert!((value - index as f32 * 2.0).abs() < 1e-3);
        }
    }

    #[test]
    fn un_reechantillonnage_neutre_ne_change_rien() {
        let input = vec![1.0, 2.0, 3.0];
        assert_eq!(resample_linear(&input, 44_100, 44_100), input);
    }

    #[test]
    fn echoue_proprement_sur_un_fichier_absent() {
        assert!(decode_for_analysis(Path::new("/introuvable.mp3")).is_err());
    }

    #[test]
    fn echoue_proprement_sur_un_fichier_qui_nest_pas_de_laudio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("faux.mp3");
        std::fs::write(&path, b"ceci n'est pas de l'audio").unwrap();

        assert!(decode_for_analysis(&path).is_err());
    }
}
