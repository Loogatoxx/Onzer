//! Empreinte des **octets audio seuls**, tags exclus.
//!
//! # Le défaut que ce module répare
//!
//! `hash::content_hash` couvre le fichier entier. Or Onzer réécrit les tags
//! après identification acoustique : **le fichier change d'octets sans changer
//! de musique**. L'empreinte stockée cesse alors de correspondre au fichier
//! d'origine, et un second exemplaire du même téléchargement n'est plus reconnu
//! comme doublon.
//!
//! Le dédoublonnage par tags ne rattrapait pas la chute : la ligne en base
//! portait déjà les tags corrigés (« Stromae »), l'entrant portait encore les
//! siens (« Damso »). Les deux filets se trouaient en même temps, et chaque
//! passage du dossier de dépôt ajoutait un exemplaire.
//!
//! # Ce qu'on hache
//!
//! ```text
//!   ┌────────────┬───────────────────────────────┬──────────────┐
//!   │  ID3v2     │          AUDIO                │ ID3v1 / APE  │
//!   └────────────┴───────────────────────────────┴──────────────┘
//!        ignoré      ◄── seul ceci est haché ──►      ignoré
//! ```
//!
//! Retaguer déplace les bornes, jamais le contenu entre elles.
//!
//! # Formats
//!
//! | Format | Traitement |
//! |---|---|
//! | MP3 | En-tête ID3v2 sauté, queues ID3v1 et APEv2 retirées |
//! | FLAC | Blocs de métadonnées parcourus jusqu'au dernier |
//! | MP4 / M4A | Contenu de l'atome `mdat` |
//! | WAV | Contenu du bloc `data` |
//! | OGG | **Fichier entier** — les commentaires Vorbis vivent dans une page du
//!         flux, les isoler demanderait de dérouler la pagination Ogg |
//!
//! Le repli sur le fichier entier reste correct : il dédoublonne toujours des
//! copies intactes, il perd seulement l'immunité au retag.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::core::Result;

/// En deçà de ce seuil, l'étendue audio est hachée intégralement.
const FULL_HASH_THRESHOLD: u64 = 4 * 1024 * 1024;

/// Taille des fenêtres lues en tête et en queue des grandes étendues.
const WINDOW: usize = 2 * 1024 * 1024;

/// Étendue des octets audio, bornes en octets depuis le début du fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioRange {
    pub start: u64,
    /// Exclusive.
    pub end: u64,
}

impl AudioRange {
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Empreinte BLAKE3 des octets audio.
///
/// La **longueur de l'étendue** entre dans l'empreinte, pour la même raison que
/// la taille du fichier entre dans `content_hash` : deux morceaux distincts
/// partageant leurs deux mégaoctets de tête et de queue n'existent pas, mais on
/// ne laisse pas la porte ouverte.
pub fn audio_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let range = audio_range(&mut file, size)?;

    let mut hasher = blake3::Hasher::new();
    hasher.update(&range.len().to_le_bytes());

    file.seek(SeekFrom::Start(range.start))?;

    if range.len() <= FULL_HASH_THRESHOLD {
        let mut buffer = vec![0_u8; range.len() as usize];
        file.read_exact(&mut buffer)?;
        hasher.update(&buffer);
    } else {
        let mut window = vec![0_u8; WINDOW];

        file.read_exact(&mut window)?;
        hasher.update(&window);

        file.seek(SeekFrom::Start(range.end - WINDOW as u64))?;
        file.read_exact(&mut window)?;
        hasher.update(&window);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Détermine l'étendue audio d'un fichier ouvert.
///
/// Toute anomalie ramène au fichier entier. Une empreinte un peu trop large
/// vaut mieux qu'une erreur d'import : on ne refuse pas un morceau parce que
/// son en-tête est inhabituel.
pub fn audio_range(file: &mut File, size: u64) -> Result<AudioRange> {
    let whole = AudioRange {
        start: 0,
        end: size,
    };

    let mut magic = [0_u8; 12];
    file.seek(SeekFrom::Start(0))?;
    if file.read(&mut magic)? < 12 {
        return Ok(whole);
    }

    // Un ID3v2 peut précéder n'importe quel conteneur, y compris FLAC.
    let after_id3 = if &magic[0..3] == b"ID3" {
        id3v2_length(&magic).unwrap_or(0)
    } else {
        0
    };

    if after_id3 >= size {
        return Ok(whole);
    }

    let mut container = [0_u8; 12];
    file.seek(SeekFrom::Start(after_id3))?;
    let read = file.read(&mut container)?;

    if read >= 4 && &container[0..4] == b"fLaC" {
        return Ok(flac_range(file, after_id3, size).unwrap_or(whole));
    }
    if read >= 12 && &container[0..4] == b"RIFF" && &container[8..12] == b"WAVE" {
        return Ok(chunk_range(file, after_id3 + 12, size, b"data", true).unwrap_or(whole));
    }
    if read >= 8 && matches!(&container[4..8], b"ftyp" | b"moov" | b"mdat" | b"free") {
        return Ok(chunk_range(file, after_id3, size, b"mdat", false).unwrap_or(whole));
    }
    if read >= 4 && &container[0..4] == b"OggS" {
        // Les commentaires Vorbis sont noyés dans la pagination : on ne sait
        // pas les isoler sans dérouler tout le flux.
        return Ok(whole);
    }

    // MP3, ou format inconnu : en-tête ID3v2 écarté, queues retirées.
    Ok(AudioRange {
        start: after_id3,
        end: trailing_tags_start(file, after_id3, size)?,
    })
}

/// Longueur totale d'un en-tête ID3v2, drapeau de pied de page compris.
fn id3v2_length(header: &[u8]) -> Option<u64> {
    // Taille sur quatre octets « synchsafe » : sept bits utiles chacun, le
    // huitième restant à zéro pour ne jamais imiter une synchronisation MPEG.
    let mut size: u64 = 0;
    for byte in header.get(6..10)? {
        if byte & 0x80 != 0 {
            return None; // ce n'est pas un entier synchsafe valide
        }
        size = (size << 7) | u64::from(byte & 0x7F);
    }

    // Bit 4 des drapeaux : un pied de page de dix octets suit les données.
    let footer = if header.get(5)? & 0x10 != 0 { 10 } else { 0 };

    Some(size + 10 + footer)
}

/// Début des tags de queue (ID3v1, APEv2), ou la fin du fichier s'il n'y en a pas.
fn trailing_tags_start(file: &mut File, floor: u64, size: u64) -> Result<u64> {
    let mut end = size;

    // ID3v1 : exactement 128 octets, préfixés « TAG ».
    if end >= floor + 128 {
        let mut marker = [0_u8; 3];
        file.seek(SeekFrom::Start(end - 128))?;
        if file.read(&mut marker)? == 3 && &marker == b"TAG" {
            end -= 128;
        }
    }

    // APEv2 : pied de page de 32 octets, dont la taille annoncée couvre le tag
    // entier sauf son éventuel en-tête.
    if end >= floor + 32 {
        let mut footer = [0_u8; 32];
        file.seek(SeekFrom::Start(end - 32))?;
        if file.read(&mut footer)? == 32 && &footer[0..8] == b"APETAGEX" {
            let tag_size = u64::from(u32::from_le_bytes([
                footer[12], footer[13], footer[14], footer[15],
            ]));
            // Bit 31 des drapeaux : le tag possède aussi un en-tête de 32 octets.
            let has_header = footer[23] & 0x80 != 0;
            let total = tag_size + if has_header { 32 } else { 0 };

            if end >= floor + total {
                end -= total;
            }
        }
    }

    Ok(end.max(floor))
}

/// Étendue audio d'un FLAC : après le dernier bloc de métadonnées.
fn flac_range(file: &mut File, start: u64, size: u64) -> Option<AudioRange> {
    let mut cursor = start + 4; // « fLaC »

    loop {
        let mut header = [0_u8; 4];
        file.seek(SeekFrom::Start(cursor)).ok()?;
        if file.read(&mut header).ok()? < 4 {
            return None;
        }

        let is_last = header[0] & 0x80 != 0;
        let length = u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]));

        cursor = cursor.checked_add(4)?.checked_add(length)?;
        if cursor > size {
            return None;
        }

        if is_last {
            return Some(AudioRange {
                start: cursor,
                end: size,
            });
        }
    }
}

/// Étendue d'un bloc nommé dans un conteneur à blocs préfixés.
///
/// Sert à deux formats aux boutismes opposés : RIFF/WAVE annonce ses tailles en
/// petit-boutiste et ne compte pas l'en-tête, MP4 en gros-boutiste et le compte.
fn chunk_range(
    file: &mut File,
    start: u64,
    size: u64,
    wanted: &[u8; 4],
    little_endian: bool,
) -> Option<AudioRange> {
    let mut cursor = start;

    while cursor + 8 <= size {
        let mut header = [0_u8; 8];
        file.seek(SeekFrom::Start(cursor)).ok()?;
        if file.read(&mut header).ok()? < 8 {
            return None;
        }

        let (name, raw) = if little_endian {
            (
                [header[0], header[1], header[2], header[3]],
                u64::from(u32::from_le_bytes([
                    header[4], header[5], header[6], header[7],
                ])),
            )
        } else {
            (
                [header[4], header[5], header[6], header[7]],
                u64::from(u32::from_be_bytes([
                    header[0], header[1], header[2], header[3],
                ])),
            )
        };

        // MP4 compte l'en-tête dans la taille annoncée, RIFF non.
        let payload = if little_endian {
            raw
        } else {
            raw.checked_sub(8)?
        };

        if &name == wanted {
            let end = cursor.checked_add(8)?.checked_add(payload)?.min(size);
            return Some(AudioRange {
                start: cursor + 8,
                end,
            });
        }

        // Les blocs RIFF sont alignés sur deux octets.
        let advance = 8 + payload + u64::from(little_endian && payload % 2 == 1);
        cursor = cursor.checked_add(advance)?;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Fabrique un fichier et retourne son étendue audio détectée.
    fn range_of(bytes: &[u8]) -> AudioRange {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piste.bin");
        std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();

        let mut file = File::open(&path).unwrap();
        let size = file.metadata().unwrap().len();
        audio_range(&mut file, size).unwrap()
    }

    fn hash_of(bytes: &[u8]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piste.bin");
        std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();
        audio_hash(&path).unwrap()
    }

    /// En-tête ID3v2 de la taille demandée, contenu compris.
    fn id3v2(payload: usize) -> Vec<u8> {
        let mut header = vec![b'I', b'D', b'3', 4, 0, 0];
        let size = payload as u32;
        header.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        header.extend(std::iter::repeat_n(0xAA, payload));
        header
    }

    fn audio(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn saute_len_tete_id3v2() {
        let mut fichier = id3v2(100);
        fichier.extend(audio(500));

        assert_eq!(range_of(&fichier), AudioRange { start: 110, end: 610 });
    }

    #[test]
    fn retire_la_queue_id3v1() {
        let mut fichier = audio(500);
        fichier.extend_from_slice(b"TAG");
        fichier.extend(std::iter::repeat_n(0_u8, 125));

        assert_eq!(range_of(&fichier), AudioRange { start: 0, end: 500 });
    }

    #[test]
    fn retire_la_queue_apev2() {
        let mut fichier = audio(500);

        // Pied de page APEv2 : 32 octets, taille annoncée = pied compris.
        let mut pied = b"APETAGEX".to_vec();
        pied.extend_from_slice(&2000_u32.to_le_bytes()); // version
        pied.extend_from_slice(&32_u32.to_le_bytes()); // taille du tag
        pied.extend_from_slice(&0_u32.to_le_bytes()); // nombre d'éléments
        pied.extend_from_slice(&0_u32.to_le_bytes()); // drapeaux, sans en-tête
        pied.extend_from_slice(&[0; 8]);
        assert_eq!(pied.len(), 32);

        fichier.extend(pied);
        assert_eq!(range_of(&fichier), AudioRange { start: 0, end: 500 });
    }

    #[test]
    fn le_retag_ne_change_pas_lempreinte() {
        // C'est toute la raison d'être du module : deux tailles de tags
        // différentes autour du même audio doivent donner la même empreinte.
        let musique = audio(4000);

        let mut avant = id3v2(50);
        avant.extend(musique.clone());

        let mut apres = id3v2(9000);
        apres.extend(musique.clone());
        apres.extend_from_slice(b"TAG");
        apres.extend(std::iter::repeat_n(0_u8, 125));

        assert_eq!(hash_of(&avant), hash_of(&apres));
    }

    #[test]
    fn deux_musiques_differentes_ne_se_confondent_pas() {
        let mut premier = id3v2(50);
        premier.extend(audio(4000));

        let mut second = id3v2(50);
        second.extend(audio(4000).iter().map(|b| b ^ 0xFF).collect::<Vec<_>>());

        assert_ne!(hash_of(&premier), hash_of(&second));
    }

    #[test]
    fn distingue_deux_durees_proches() {
        // La longueur de l'étendue entre dans l'empreinte : un morceau tronqué
        // ne doit pas passer pour son original.
        assert_ne!(hash_of(&audio(4000)), hash_of(&audio(4001)));
    }

    #[test]
    fn trouve_le_bloc_data_dun_wav() {
        let mut fichier = b"RIFF".to_vec();
        fichier.extend_from_slice(&0_u32.to_le_bytes());
        fichier.extend_from_slice(b"WAVE");

        // Un bloc « fmt  » de 16 octets, puis les données.
        fichier.extend_from_slice(b"fmt ");
        fichier.extend_from_slice(&16_u32.to_le_bytes());
        fichier.extend(std::iter::repeat_n(0_u8, 16));

        fichier.extend_from_slice(b"data");
        fichier.extend_from_slice(&200_u32.to_le_bytes());
        let debut = fichier.len() as u64;
        fichier.extend(audio(200));

        assert_eq!(
            range_of(&fichier),
            AudioRange {
                start: debut,
                end: debut + 200
            }
        );
    }

    #[test]
    fn trouve_latome_mdat_dun_mp4() {
        let mut fichier = Vec::new();

        // ftyp
        fichier.extend_from_slice(&16_u32.to_be_bytes());
        fichier.extend_from_slice(b"ftyp");
        fichier.extend(std::iter::repeat_n(0_u8, 8));

        // moov, qui contient les tags — et qui grossit quand on retague
        fichier.extend_from_slice(&40_u32.to_be_bytes());
        fichier.extend_from_slice(b"moov");
        fichier.extend(std::iter::repeat_n(0x11_u8, 32));

        fichier.extend_from_slice(&208_u32.to_be_bytes());
        fichier.extend_from_slice(b"mdat");
        let debut = fichier.len() as u64;
        fichier.extend(audio(200));

        assert_eq!(
            range_of(&fichier),
            AudioRange {
                start: debut,
                end: debut + 200
            }
        );
    }

    #[test]
    fn trouve_le_debut_audio_dun_flac() {
        let mut fichier = b"fLaC".to_vec();

        // Un bloc STREAMINFO de 34 octets, non final.
        fichier.push(0);
        fichier.extend_from_slice(&[0, 0, 34]);
        fichier.extend(std::iter::repeat_n(0_u8, 34));

        // Un bloc VORBIS_COMMENT final de 10 octets.
        fichier.push(0x84);
        fichier.extend_from_slice(&[0, 0, 10]);
        fichier.extend(std::iter::repeat_n(0_u8, 10));

        let debut = fichier.len() as u64;
        fichier.extend(audio(300));

        assert_eq!(
            range_of(&fichier),
            AudioRange {
                start: debut,
                end: debut + 300
            }
        );
    }

    #[test]
    fn un_ogg_est_hache_entierement() {
        // Repli assumé : on ne sait pas isoler les commentaires Vorbis.
        let mut fichier = b"OggS".to_vec();
        fichier.extend(audio(500));

        assert_eq!(
            range_of(&fichier),
            AudioRange {
                start: 0,
                end: fichier.len() as u64
            }
        );
    }

    #[test]
    fn un_entete_aberrant_ramene_au_fichier_entier() {
        // Mieux vaut une empreinte trop large qu'un import refusé.
        let mut fichier = b"ID3".to_vec();
        fichier.extend_from_slice(&[4, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
        fichier.extend(audio(100));

        assert_eq!(range_of(&fichier).start, 0);
    }

    #[test]
    fn un_fichier_minuscule_ne_provoque_pas_de_panique() {
        assert_eq!(range_of(b"ID3"), AudioRange { start: 0, end: 3 });
        assert_eq!(range_of(b""), AudioRange { start: 0, end: 0 });
    }
}
