//! Empreinte de contenu des fichiers audio.
//!
//! Sert à deux choses :
//! * détecter les doublons à l'import ;
//! * **ré-identifier** un fichier que l'utilisateur aurait déplacé ou renommé
//!   lui-même dans le Finder, sans quoi Onzer le croirait perdu et en créerait
//!   un doublon au scan suivant.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::core::Result;

/// En deçà de ce seuil, le fichier est haché intégralement.
const FULL_HASH_THRESHOLD: u64 = 4 * 1024 * 1024;

/// Taille des fenêtres lues en tête et en queue des gros fichiers.
const WINDOW: usize = 2 * 1024 * 1024;

/// Calcule l'empreinte BLAKE3 d'un fichier audio.
///
/// **Stratégie d'échantillonnage.** Hacher intégralement une bibliothèque de
/// 5 000 morceaux imposerait de lire des dizaines de gigaoctets sur un SSD USB
/// à chaque scan. Au-delà de 4 Mo, on ne hache donc que les 2 premiers et les
/// 2 derniers mégaoctets, **taille du fichier incluse** dans l'empreinte.
///
/// Le compromis est sans risque en pratique : deux fichiers audio distincts
/// partageant exactement la même taille, les mêmes 2 Mo de tête *et* les mêmes
/// 2 Mo de queue n'existent pas hors d'une collision fabriquée à dessein.
pub fn content_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();

    let mut hasher = blake3::Hasher::new();
    // La taille discrimine deux fichiers aux extrémités identiques.
    hasher.update(&size.to_le_bytes());

    if size <= FULL_HASH_THRESHOLD {
        let mut buffer = Vec::with_capacity(size as usize);
        file.read_to_end(&mut buffer)?;
        hasher.update(&buffer);
    } else {
        let mut window = vec![0_u8; WINDOW];

        file.read_exact(&mut window)?;
        hasher.update(&window);

        file.seek(SeekFrom::End(-(WINDOW as i64)))?;
        file.read_exact(&mut window)?;
        hasher.update(&window);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fichier_avec(contenu: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piste.mp3");
        File::create(&path).unwrap().write_all(contenu).unwrap();
        (dir, path)
    }

    #[test]
    fn deux_fichiers_identiques_ont_la_meme_empreinte() {
        let (_a, chemin_a) = fichier_avec(b"des octets audio");
        let (_b, chemin_b) = fichier_avec(b"des octets audio");

        assert_eq!(
            content_hash(&chemin_a).unwrap(),
            content_hash(&chemin_b).unwrap()
        );
    }

    #[test]
    fn un_octet_de_difference_change_lempreinte() {
        let (_a, chemin_a) = fichier_avec(b"des octets audio");
        let (_b, chemin_b) = fichier_avec(b"des octets audiO");

        assert_ne!(
            content_hash(&chemin_a).unwrap(),
            content_hash(&chemin_b).unwrap()
        );
    }

    #[test]
    fn la_taille_discrimine_deux_fichiers_aux_extremites_identiques() {
        // Deux fichiers de tailles différentes partageant tête et queue :
        // sans la taille dans l'empreinte, ils seraient confondus.
        let mut court = vec![0xAA_u8; WINDOW];
        court.extend(vec![0xBB_u8; WINDOW]);

        let mut long = vec![0xAA_u8; WINDOW];
        long.extend(vec![0x11_u8; 4096]); // milieu différent, donc taille aussi
        long.extend(vec![0xBB_u8; WINDOW]);

        let (_a, chemin_a) = fichier_avec(&court);
        let (_b, chemin_b) = fichier_avec(&long);

        assert_ne!(
            content_hash(&chemin_a).unwrap(),
            content_hash(&chemin_b).unwrap()
        );
    }

    #[test]
    fn lempreinte_est_stable_entre_deux_lectures() {
        let (_dir, chemin) = fichier_avec(&vec![0x42_u8; 6 * 1024 * 1024]);

        assert_eq!(
            content_hash(&chemin).unwrap(),
            content_hash(&chemin).unwrap()
        );
    }

    #[test]
    fn echoue_proprement_sur_un_fichier_absent() {
        assert!(content_hash(Path::new("/introuvable/x.mp3")).is_err());
    }
}
