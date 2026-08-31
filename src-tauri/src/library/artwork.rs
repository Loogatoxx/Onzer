//! Cache de pochettes.
//!
//! **Aucune pochette pleine résolution n'est stockée** (ADR-009) : elle vit
//! déjà dans le fichier audio, sur le SSD. On ne conserve qu'une vignette,
//! sur le disque interne, pour que l'interface reste illustrée même SSD
//! débranché.
//!
//! Les vignettes sont nommées d'après l'empreinte de l'image d'origine : les
//! douze pistes d'un album partagent la même pochette, donc le même fichier.

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::core::{OnzerError, Result};

/// Côté maximal d'une vignette. 512 px suffit à une grande pochette dans
/// l'interface, y compris sur écran Retina.
const THUMBNAIL_SIZE: u32 = 512;

/// Qualité JPEG. À 82, l'artefact est invisible sur une pochette et le
/// fichier tourne autour de 40 Ko.
const JPEG_QUALITY: u8 = 82;

/// Enregistre une vignette et retourne son empreinte, qui sert de nom de
/// fichier. Si la vignette existe déjà, rien n'est réécrit.
pub fn store(artwork_dir: &Path, image_data: &[u8]) -> Result<String> {
    let digest = blake3::hash(image_data).to_hex().to_string();
    let destination = thumbnail_path(artwork_dir, &digest);

    // Pochette déjà connue : c'est le cas de toutes les pistes d'un album à
    // partir de la deuxième.
    if destination.exists() {
        return Ok(digest);
    }

    let decoded = image::load_from_memory(image_data)
        .map_err(|error| OnzerError::Invalid(format!("pochette illisible : {error}")))?;

    // `thumbnail` préserve les proportions et ne fait qu'agrandir si l'image
    // est déjà plus petite — ce qu'on évite en ne redimensionnant que si
    // nécessaire.
    let resized = if decoded.width() > THUMBNAIL_SIZE || decoded.height() > THUMBNAIL_SIZE {
        decoded.resize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Lanczos3)
    } else {
        decoded
    };

    // Le JPEG n'a pas de canal alpha : on aplatit en RGB pour éviter qu'un PNG
    // transparent ne produise une image corrompue.
    let rgb = resized.to_rgb8();

    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|error| OnzerError::Invalid(format!("encodage de la vignette : {error}")))?;

    std::fs::create_dir_all(artwork_dir)?;
    std::fs::write(&destination, encoded)?;

    Ok(digest)
}

pub fn thumbnail_path(artwork_dir: &Path, digest: &str) -> PathBuf {
    artwork_dir.join(format!("{digest}.jpg"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};

    /// Fabrique une image PNG de test, en mémoire.
    fn png(width: u32, height: u32, teinte: u8) -> Vec<u8> {
        let image = RgbImage::from_fn(width, height, |x, _| {
            image::Rgb([teinte, (x % 256) as u8, 128])
        });

        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn enregistre_une_vignette_et_retourne_son_empreinte() {
        let dir = tempfile::tempdir().unwrap();
        let digest = store(dir.path(), &png(800, 800, 200)).unwrap();

        assert!(thumbnail_path(dir.path(), &digest).exists());
    }

    #[test]
    fn reduit_les_grandes_pochettes() {
        let dir = tempfile::tempdir().unwrap();
        let digest = store(dir.path(), &png(2000, 2000, 200)).unwrap();

        let vignette = image::open(thumbnail_path(dir.path(), &digest)).unwrap();
        assert_eq!(vignette.width(), THUMBNAIL_SIZE);
        assert_eq!(vignette.height(), THUMBNAIL_SIZE);
    }

    #[test]
    fn nagrandit_pas_une_petite_pochette() {
        let dir = tempfile::tempdir().unwrap();
        let digest = store(dir.path(), &png(120, 120, 90)).unwrap();

        let vignette = image::open(thumbnail_path(dir.path(), &digest)).unwrap();
        assert_eq!(vignette.width(), 120, "agrandir n'ajouterait aucun détail");
    }

    #[test]
    fn mutualise_la_pochette_partagee_par_tout_un_album() {
        let dir = tempfile::tempdir().unwrap();
        let pochette = png(600, 600, 42);

        let premier = store(dir.path(), &pochette).unwrap();
        let second = store(dir.path(), &pochette).unwrap();

        assert_eq!(premier, second);
        let fichiers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(fichiers, 1, "un seul fichier pour tout l'album");
    }

    #[test]
    fn deux_pochettes_differentes_ne_se_confondent_pas() {
        let dir = tempfile::tempdir().unwrap();

        let a = store(dir.path(), &png(400, 400, 10)).unwrap();
        let b = store(dir.path(), &png(400, 400, 250)).unwrap();

        assert_ne!(a, b);
    }

    #[test]
    fn rejette_proprement_des_donnees_qui_ne_sont_pas_une_image() {
        let dir = tempfile::tempdir().unwrap();
        assert!(store(dir.path(), b"ceci n'est pas une image").is_err());
    }

    #[test]
    fn la_vignette_reste_legere() {
        let dir = tempfile::tempdir().unwrap();
        let digest = store(dir.path(), &png(1500, 1500, 77)).unwrap();

        let taille = std::fs::metadata(thumbnail_path(dir.path(), &digest))
            .unwrap()
            .len();

        assert!(
            taille < 200_000,
            "une vignette doit rester sous 200 Ko, obtenu {taille} octets"
        );
    }
}
