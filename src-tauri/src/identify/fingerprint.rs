//! Empreinte acoustique Chromaprint.
//!
//! # Pourquoi celle-ci n'est pas écrite à la main
//!
//! Le reste de l'analyse audio d'Onzer est écrit sur mesure, et vérifié
//! descripteur par descripteur sur des signaux dont on connaît la réponse : un
//! La 440 doit illuminer la classe de hauteur du La, une sinusoïde à 1000 Hz
//! doit donner un centroïde à 1000 Hz. Cette vérification est possible parce
//! que ces valeurs n'ont à correspondre à **rien d'extérieur**.
//!
//! Une empreinte Chromaprint, si. Elle doit être **bit-à-bit identique** à ce
//! que produit l'implémentation de référence, sans quoi l'index d'AcoustID ne
//! renverra jamais la moindre correspondance — et l'échec serait silencieux :
//! aucune erreur, simplement « morceau inconnu » sur toute la bibliothèque.
//!
//! Réimplémenter à l'aveugle un algorithme dont on ne peut pas vérifier la
//! justesse localement serait imprudent. On s'appuie donc sur `rusty-chromaprint`,
//! portage Rust maintenu pour cette compatibilité.
//!
//! # Ce que ce module fait quand même
//!
//! Le décodage, le rééchantillonnage, la conversion et l'encodage — soit tout
//! ce qui entoure l'algorithme, et où les erreurs sont fréquentes.

use std::path::Path;

use rusty_chromaprint::{Configuration, FingerprintCompressor, Fingerprinter};

use crate::analysis::decode::{self, DecodeRequest};
use crate::core::{OnzerError, Result};

/// Durée soumise à l'empreinte.
///
/// AcoustID indexe les deux premières minutes. En envoyer davantage n'améliore
/// rien et alourdit la requête ; en envoyer moins dégrade la correspondance.
const FINGERPRINT_SECONDS: f32 = 120.0;

/// Empreinte prête à être envoyée.
#[derive(Debug, Clone)]
pub struct Fingerprint {
    /// Empreinte compressée puis encodée en base64 URL, sans remplissage.
    pub encoded: String,
    /// Durée **totale** du morceau, en secondes.
    ///
    /// Et non celle de l'extrait : AcoustID s'en sert pour départager deux
    /// enregistrements dont les deux premières minutes se ressemblent — une
    /// version album et sa version radio, par exemple.
    pub duration_seconds: u32,
}

/// Calcule l'empreinte acoustique d'un fichier.
pub fn fingerprint_file(path: &Path) -> Result<Fingerprint> {
    // `preset_test2` correspond à l'algorithme employé par AcoustID. En changer
    // rendrait toutes les empreintes inexploitables.
    let config = Configuration::preset_test2();

    let audio = decode::decode(
        path,
        DecodeRequest {
            target_rate: config.sample_rate(),
            max_seconds: FINGERPRINT_SECONDS,
            // Zéro : l'empreinte doit partir du tout début du morceau pour
            // correspondre à la référence indexée.
            skip_intro_ratio: 0.0,
        },
    )?;

    if audio.samples.is_empty() {
        return Err(OnzerError::Invalid(
            "aucun échantillon à empreindre".to_string(),
        ));
    }

    // Chromaprint travaille sur des entiers 16 bits signés.
    let samples: Vec<i16> = audio
        .samples
        .iter()
        .map(|value| (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();

    let mut printer = Fingerprinter::new(&config);

    printer
        .start(audio.sample_rate, 1)
        .map_err(|error| OnzerError::Invalid(format!("empreinte impossible : {error}")))?;
    printer.consume(&samples);
    printer.finish();

    let raw = printer.fingerprint();
    if raw.is_empty() {
        return Err(OnzerError::Invalid(
            "empreinte vide — extrait trop court ou silencieux".to_string(),
        ));
    }

    let compressed = FingerprintCompressor::from(&config).compress(raw);

    Ok(Fingerprint {
        encoded: base64_url_encode(&compressed),
        duration_seconds: audio.duration_seconds.round().max(1.0) as u32,
    })
}

/// Encodage base64 en variante **URL**, sans remplissage.
///
/// AcoustID exige cette variante précise : l'alphabet standard utilise `+` et
/// `/`, qui seraient réinterprétés dans une chaîne de requête HTTP. Le
/// remplissage `=` est également refusé.
fn base64_url_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);

    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        output.push(ALPHABET[(triple >> 18 & 0x3F) as usize] as char);
        output.push(ALPHABET[(triple >> 12 & 0x3F) as usize] as char);

        // Sans remplissage : on n'émet que les caractères porteurs
        // d'information.
        if chunk.len() > 1 {
            output.push(ALPHABET[(triple >> 6 & 0x3F) as usize] as char);
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(triple & 0x3F) as usize] as char);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Écrit un WAV décodable, avec un contenu harmonique riche : une
    /// empreinte calculée sur du silence ne prouverait rien.
    fn wav(path: &Path, seconds: f32, base_hz: f32) {
        const RATE: u32 = 44_100;
        let count = (seconds * RATE as f32) as u32;
        let bytes = count * 2;

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(path).unwrap();

        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&RATE.to_le_bytes()).unwrap();
        file.write_all(&(RATE * 2).to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();

        let mut samples = Vec::with_capacity(bytes as usize);
        for index in 0..count {
            let t = index as f32 / RATE as f32;
            // Fondamentale plus deux harmoniques, avec un vibrato lent : de
            // quoi produire une empreinte réellement variée.
            let value = ((std::f32::consts::TAU * base_hz * t).sin() * 0.5
                + (std::f32::consts::TAU * base_hz * 2.0 * t).sin() * 0.3
                + (std::f32::consts::TAU * (base_hz * 1.5 + (t * 2.0).sin() * 20.0) * t).sin()
                    * 0.2)
                * 8000.0;
            samples.extend_from_slice(&(value as i16).to_le_bytes());
        }
        file.write_all(&samples).unwrap();
    }

    fn empreinte(seconds: f32, base_hz: f32) -> (tempfile::TempDir, Fingerprint) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piste.wav");
        wav(&path, seconds, base_hz);
        let fingerprint = fingerprint_file(&path).unwrap();
        (dir, fingerprint)
    }

    // ── Encodage ────────────────────────────────────────────────────────

    #[test]
    fn lencodage_suit_la_variante_url_sans_remplissage() {
        // Vecteurs de la RFC 4648, section 5.
        assert_eq!(base64_url_encode(b""), "");
        assert_eq!(base64_url_encode(b"f"), "Zg");
        assert_eq!(base64_url_encode(b"fo"), "Zm8");
        assert_eq!(base64_url_encode(b"foo"), "Zm9v");
        assert_eq!(base64_url_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn lencodage_nutilise_jamais_de_caractere_interdit_en_url() {
        // Les octets 0xFB et 0xFF produisent `+` et `/` en base64 standard.
        let encode = base64_url_encode(&[0xFB, 0xFF, 0xBF, 0xFE, 0xFF]);

        assert!(!encode.contains('+'), "obtenu {encode}");
        assert!(!encode.contains('/'), "obtenu {encode}");
        assert!(!encode.contains('='), "obtenu {encode}");
        assert!(encode.contains('-') || encode.contains('_'));
    }

    #[test]
    fn tous_les_octets_possibles_sencodent_sans_panique() {
        let tous: Vec<u8> = (0..=255).collect();
        let encode = base64_url_encode(&tous);

        assert!(encode
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    // ── Empreinte ───────────────────────────────────────────────────────

    #[test]
    fn produit_une_empreinte_non_vide() {
        let (_dir, fingerprint) = empreinte(15.0, 440.0);

        assert!(!fingerprint.encoded.is_empty());
        assert!(
            fingerprint.encoded.len() > 40,
            "empreinte suspecte de brièveté : {}",
            fingerprint.encoded.len()
        );
    }

    #[test]
    fn lempreinte_est_deterministe() {
        // Deux fichiers au contenu identique doivent produire exactement la
        // même empreinte : c'est la condition même de son utilité.
        let (_a, premiere) = empreinte(15.0, 440.0);
        let (_b, seconde) = empreinte(15.0, 440.0);

        assert_eq!(premiere.encoded, seconde.encoded);
    }

    #[test]
    fn deux_sons_differents_ont_des_empreintes_differentes() {
        let (_a, premiere) = empreinte(15.0, 440.0);
        let (_b, seconde) = empreinte(15.0, 587.0);

        assert_ne!(premiere.encoded, seconde.encoded);
    }

    #[test]
    fn la_duree_transmise_est_celle_du_morceau_entier() {
        // Et non celle de l'extrait : AcoustID s'en sert pour départager deux
        // enregistrements aux deux premières minutes semblables.
        let (_dir, fingerprint) = empreinte(20.0, 440.0);

        assert!(
            (19..=21).contains(&fingerprint.duration_seconds),
            "durée obtenue {}",
            fingerprint.duration_seconds
        );
    }

    #[test]
    fn lempreinte_reste_url_safe() {
        let (_dir, fingerprint) = empreinte(15.0, 440.0);

        assert!(
            fingerprint
                .encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "caractère invalide dans {}",
            fingerprint.encoded
        );
    }

    #[test]
    fn echoue_proprement_sur_un_fichier_absent() {
        assert!(fingerprint_file(Path::new("/introuvable.mp3")).is_err());
    }

    #[test]
    fn echoue_proprement_sur_un_extrait_trop_court() {
        // Chromaprint a besoin de plusieurs secondes pour produire quoi que ce
        // soit d'exploitable.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bref.wav");
        wav(&path, 0.5, 440.0);

        assert!(fingerprint_file(&path).is_err());
    }
}
