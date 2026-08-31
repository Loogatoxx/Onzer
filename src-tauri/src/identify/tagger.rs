//! Application d'une identification : tags, pochette, rangement.
//!
//! # L'ordre des opérations n'est pas indifférent
//!
//! ```text
//!  1. écriture des tags dans le fichier   ← le fichier change
//!  2. recalcul de l'empreinte de contenu  ← DONC elle doit être recalculée
//!  3. calcul du nouveau rangement
//!  4. déplacement du fichier
//!  5. pochette haute résolution à côté de l'album
//!  6. mise à jour de la base, en une transaction
//! ```
//!
//! **L'étape 2 est celle qu'on oublie.** L'empreinte de contenu sert à
//! reconnaître un fichier déplacé à la main. Réécrire ses tags la change ; ne
//! pas la recalculer ferait croire au prochain scan qu'il s'agit d'un fichier
//! inconnu, et créerait un doublon.

use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::Tag;
use sqlx::SqlitePool;

use crate::core::{OnzerError, PathResolver, Result};
use crate::db::repository;
use crate::library::{artwork, hash, metadata, naming};

use super::musicbrainz::RecordingMetadata;

/// Nom de la pochette déposée à côté de l'album.
///
/// Convention universelle : tous les autres lecteurs savent la lire. La
/// bibliothèque reste ainsi exploitable hors d'Onzer.
const ALBUM_COVER_FILE: &str = "cover.jpg";

/// Côté maximal de la pochette haute résolution conservée sur le SSD.
///
/// 1 500 px suffit à un affichage plein écran sur un Retina, pour environ
/// 400 Ko. Les scans d'archive atteignent parfois 5 000 px : les conserver tels
/// quels gonflerait la bibliothèque sans bénéfice visible.
const ALBUM_COVER_MAX_SIZE: u32 = 1_500;

const ALBUM_COVER_QUALITY: u8 = 90;

#[derive(Debug)]
pub struct AppliedIdentification {
    pub relative_path: String,
    /// Le fichier a-t-il changé d'emplacement ?
    pub moved: bool,
    pub artwork_hash: Option<String>,
}

/// Applique une identification à un morceau déjà présent en bibliothèque.
pub async fn apply(
    pool: &SqlitePool,
    paths: &PathResolver,
    track_id: i64,
    current_relative_path: &str,
    identified: &RecordingMetadata,
    cover: Option<&[u8]>,
) -> Result<AppliedIdentification> {
    let current_path = paths.resolve(current_relative_path)?;

    if !current_path.is_file() {
        return Err(OnzerError::Invalid(format!(
            "fichier introuvable : {current_relative_path}"
        )));
    }

    // Les caractéristiques techniques (durée, format, débit) restent celles du
    // fichier : MusicBrainz décrit l'œuvre, pas l'encodage qu'on possède.
    let mut merged = metadata::read(&current_path)?;
    merge(&mut merged, identified);

    // ── 1. Tags ─────────────────────────────────────────────────────────
    write_tags(&current_path, &merged, cover)?;

    // ── 2. L'empreinte a changé ─────────────────────────────────────────
    let content_hash = hash::content_hash(&current_path)?;
    let file_size = std::fs::metadata(&current_path)?.len() as i64;

    // ── 3 et 4. Rangement ───────────────────────────────────────────────
    let desired = naming::build_relative_path(&naming::FilingInfo {
        filing_artist: merged.filing_artist(),
        album: merged.album.as_deref(),
        year: merged.year,
        track_no: merged.track_no,
        disc_no: merged.disc_no,
        title: &merged.title,
        extension: &merged.format,
    });

    let (relative_path, moved) = if desired == current_relative_path {
        (desired, false)
    } else {
        let unique = unique_destination(paths, &desired, &current_path)?;
        move_file(paths, &current_path, &unique)?;
        (unique, true)
    };

    // ── 5. Pochette ─────────────────────────────────────────────────────
    // Un échec ici ne doit jamais annuler une identification réussie : les
    // tags sont déjà corrects, ce qui est l'essentiel.
    let artwork_hash = match cover {
        Some(bytes) => {
            let final_path = paths.resolve(&relative_path)?;
            if let Some(album_dir) = final_path.parent() {
                if let Err(error) = store_album_cover(album_dir, bytes) {
                    tracing::warn!(%error, "pochette haute résolution non écrite");
                }
            }

            match artwork::store(&paths.artwork_dir(), bytes) {
                Ok(digest) => Some(digest),
                Err(error) => {
                    tracing::warn!(%error, "vignette de pochette non écrite");
                    None
                }
            }
        }
        None => None,
    };

    // ── 6. Base ─────────────────────────────────────────────────────────
    repository::update_track_identity(
        pool,
        track_id,
        &merged,
        &relative_path,
        &content_hash,
        file_size,
        artwork_hash.as_deref(),
        Some(&identified.recording_mbid),
    )
    .await?;

    Ok(AppliedIdentification {
        relative_path,
        moved,
        artwork_hash,
    })
}

/// Fusionne les métadonnées identifiées avec celles du fichier.
///
/// L'identification **fait autorité** sur tout ce qui décrit l'œuvre : c'est
/// tout l'intérêt de l'avoir demandée. Les caractéristiques techniques du
/// fichier, elles, sont conservées.
fn merge(local: &mut metadata::TrackMetadata, identified: &RecordingMetadata) {
    if !identified.title.trim().is_empty() {
        local.title = identified.title.clone();
    }
    if !identified.artists.is_empty() {
        local.artists = identified.artists.clone();
    }

    local.featured_artists = identified.featured_artists.clone();
    local.album_artist = identified.filing_artist().map(str::to_string);

    if identified.album.is_some() {
        local.album = identified.album.clone();
    }
    if identified.year.is_some() {
        local.year = identified.year;
    }
    if identified.track_no.is_some() {
        local.track_no = identified.track_no;
    }
    if identified.disc_no.is_some() {
        local.disc_no = identified.disc_no;
    }
    if let Some(genre) = &identified.genre {
        local.genres = vec![genre.clone()];
    }

    // Les métadonnées ne viennent plus d'une supposition sur le nom de fichier.
    local.from_filename = false;
}

/// Écrit les tags dans le fichier.
fn write_tags(
    path: &Path,
    metadata: &metadata::TrackMetadata,
    cover: Option<&[u8]>,
) -> Result<()> {
    let tagged = lofty::read_from_path(path)
        .map_err(|error| OnzerError::Invalid(format!("lecture des tags : {error}")))?;

    // Chaque format a son porteur de tags naturel : ID3v2 pour un MP3,
    // Vorbis Comments pour un FLAC, iTunes pour un M4A.
    let mut tag = Tag::new(tagged.primary_tag_type());

    tag.set_title(metadata.title.clone());

    // Le champ artiste porte le crédit complet, featurings compris : c'est ce
    // qu'attendent les autres lecteurs.
    let mut credit = metadata.artists.join(", ");
    if !metadata.featured_artists.is_empty() {
        credit = format!("{credit} feat. {}", metadata.featured_artists.join(", "));
    }
    if !credit.is_empty() {
        tag.set_artist(credit);
    }

    if let Some(album) = &metadata.album {
        tag.set_album(album.clone());
    }
    if let Some(album_artist) = &metadata.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.clone());
    }
    if let Some(year) = metadata.year {
        tag.set_year(year);
    }
    if let Some(track) = metadata.track_no {
        tag.set_track(track);
    }
    if let Some(disc) = metadata.disc_no {
        tag.set_disk(disc);
    }
    if let Some(genre) = metadata.genres.first() {
        tag.set_genre(genre.clone());
    }

    // Les paroles sont réécrites telles quelles.
    //
    // Ce n'est pas un détail : on remplace ici le bloc de tags par un neuf.
    // Omettre ce champ effacerait des paroles synchronisées que l'utilisateur
    // possédait, et rien ne permettrait de les retrouver.
    if let Some(lyrics) = &metadata.lyrics {
        tag.insert_text(ItemKey::Lyrics, lyrics.clone());
    }

    if let Some(bytes) = cover {
        // Embarquée en plus du fichier `cover.jpg` : un lecteur mobile qui ne
        // lit que le fichier audio trouvera quand même la pochette.
        match embedded_picture(bytes) {
            Ok(picture) => tag.push_picture(picture),
            Err(error) => tracing::warn!(%error, "pochette non embarquée"),
        }
    }

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|error| OnzerError::Invalid(format!("écriture des tags : {error}")))
}

/// Prépare la pochette à embarquer : redimensionnée, en JPEG.
fn embedded_picture(bytes: &[u8]) -> Result<Picture> {
    let resized = resize_jpeg(bytes, 600, 85)?;

    Ok(Picture::new_unchecked(
        PictureType::CoverFront,
        Some(MimeType::Jpeg),
        None,
        resized,
    ))
}

/// Dépose la pochette haute résolution à côté de l'album.
fn store_album_cover(album_dir: &Path, bytes: &[u8]) -> Result<()> {
    let encoded = resize_jpeg(bytes, ALBUM_COVER_MAX_SIZE, ALBUM_COVER_QUALITY)?;

    std::fs::create_dir_all(album_dir)?;
    std::fs::write(album_dir.join(ALBUM_COVER_FILE), encoded)?;

    Ok(())
}

/// Redimensionne et réencode en JPEG, sans jamais agrandir.
fn resize_jpeg(bytes: &[u8], max_side: u32, quality: u8) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::imageops::FilterType;

    let decoded = image::load_from_memory(bytes)
        .map_err(|error| OnzerError::Invalid(format!("pochette illisible : {error}")))?;

    let resized = if decoded.width() > max_side || decoded.height() > max_side {
        decoded.resize(max_side, max_side, FilterType::Lanczos3)
    } else {
        decoded
    };

    // Le JPEG n'a pas de canal alpha : aplatir évite qu'un PNG transparent ne
    // produise une image corrompue.
    let rgb = resized.to_rgb8();

    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, quality)
        .encode_image(&rgb)
        .map_err(|error| OnzerError::Invalid(format!("encodage de la pochette : {error}")))?;

    Ok(output)
}

/// Trouve un chemin libre, en s'excluant soi-même.
///
/// Sans cette exclusion, un fichier déjà au bon endroit entrerait en collision
/// avec lui-même et se verrait renommé « … (2) » à chaque identification.
fn unique_destination(paths: &PathResolver, desired: &str, source: &Path) -> Result<String> {
    if !is_taken(paths, desired, source)? {
        return Ok(desired.to_string());
    }

    let (stem, extension) = match desired.rsplit_once('.') {
        Some(parts) => parts,
        None => (desired, ""),
    };

    for suffix in 2..=99 {
        let candidate = if extension.is_empty() {
            format!("{stem} ({suffix})")
        } else {
            format!("{stem} ({suffix}).{extension}")
        };

        if !is_taken(paths, &candidate, source)? {
            return Ok(candidate);
        }
    }

    Err(OnzerError::Invalid(format!(
        "aucun nom libre pour « {desired} »"
    )))
}

fn is_taken(paths: &PathResolver, candidate: &str, source: &Path) -> Result<bool> {
    let destination = paths.resolve(candidate)?;

    if !destination.exists() {
        return Ok(false);
    }

    let same = match (destination.canonicalize(), source.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => destination == source,
    };

    Ok(!same)
}

fn move_file(paths: &PathResolver, source: &Path, relative_path: &str) -> Result<()> {
    let destination: PathBuf = paths.resolve(relative_path)?;

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::rename(source, &destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};

    fn png(side: u32) -> Vec<u8> {
        let image = RgbImage::from_fn(side, side, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });

        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn identifie() -> RecordingMetadata {
        RecordingMetadata {
            recording_mbid: "rec-1".into(),
            title: "Digital Love".into(),
            artists: vec!["Daft Punk".into()],
            featured_artists: Vec::new(),
            album: Some("Discovery".into()),
            release_mbid: Some("rel-1".into()),
            release_group_mbid: Some("grp-1".into()),
            year: Some(2001),
            track_no: Some(3),
            disc_no: Some(1),
            genre: Some("french house".into()),
        }
    }

    fn locale() -> metadata::TrackMetadata {
        metadata::TrackMetadata {
            title: "piste 03".into(),
            artists: vec!["Inconnu".into()],
            featured_artists: Vec::new(),
            album_artist: None,
            album: None,
            track_no: None,
            disc_no: None,
            year: None,
            genres: Vec::new(),
            duration_ms: 301_000,
            bitrate: Some(320),
            sample_rate: Some(44_100),
            channels: Some(2),
            format: "mp3".into(),
            artwork: None,
            lyrics: None,
            from_filename: true,
        }
    }

    // ── Fusion ──────────────────────────────────────────────────────────

    #[test]
    fn lidentification_fait_autorite_sur_loeuvre() {
        let mut local = locale();
        merge(&mut local, &identifie());

        assert_eq!(local.title, "Digital Love");
        assert_eq!(local.artists, vec!["Daft Punk"]);
        assert_eq!(local.album.as_deref(), Some("Discovery"));
        assert_eq!(local.year, Some(2001));
        assert_eq!(local.track_no, Some(3));
        assert_eq!(local.genres, vec!["french house"]);
    }

    #[test]
    fn les_caracteristiques_du_fichier_sont_conservees() {
        // MusicBrainz décrit l'œuvre, pas l'encodage qu'on possède.
        let mut local = locale();
        merge(&mut local, &identifie());

        assert_eq!(local.duration_ms, 301_000);
        assert_eq!(local.bitrate, Some(320));
        assert_eq!(local.format, "mp3");
    }

    #[test]
    fn lidentification_ne_touche_pas_aux_paroles() {
        // MusicBrainz n'en fournit pas : celles du fichier doivent survivre.
        let mut local = locale();
        local.lyrics = Some("[00:10.00]Une ligne".to_string());

        merge(&mut local, &identifie());

        assert_eq!(local.lyrics.as_deref(), Some("[00:10.00]Une ligne"));
    }

    #[test]
    fn la_deduction_par_nom_de_fichier_est_annulee() {
        let mut local = locale();
        assert!(local.from_filename);

        merge(&mut local, &identifie());
        assert!(
            !local.from_filename,
            "les métadonnées ne sont plus une supposition"
        );
    }

    #[test]
    fn une_identification_partielle_ne_detruit_rien() {
        let mut local = locale();
        local.album = Some("Album connu".into());
        local.year = Some(1999);

        let mut partielle = identifie();
        partielle.album = None;
        partielle.year = None;

        merge(&mut local, &partielle);

        assert_eq!(local.album.as_deref(), Some("Album connu"));
        assert_eq!(local.year, Some(1999));
    }

    #[test]
    fn lartiste_de_rangement_devient_celui_de_lenregistrement() {
        // Et non celui de la parution : sur une compilation, ce serait
        // « Various Artists », ce qui entasserait tout dans un seul dossier.
        let mut local = locale();
        merge(&mut local, &identifie());

        assert_eq!(local.album_artist.as_deref(), Some("Daft Punk"));
    }

    // ── Images ──────────────────────────────────────────────────────────

    #[test]
    fn la_pochette_haute_resolution_est_bornee() {
        let encoded = resize_jpeg(&png(3000), ALBUM_COVER_MAX_SIZE, ALBUM_COVER_QUALITY).unwrap();
        let image = image::load_from_memory(&encoded).unwrap();

        assert_eq!(image.width(), ALBUM_COVER_MAX_SIZE);
        assert!(
            encoded.len() < 2_000_000,
            "poids obtenu {} octets",
            encoded.len()
        );
    }

    #[test]
    fn une_petite_pochette_nest_jamais_agrandie() {
        // Agrandir n'ajouterait aucun détail, seulement du flou et des octets.
        let encoded = resize_jpeg(&png(300), ALBUM_COVER_MAX_SIZE, ALBUM_COVER_QUALITY).unwrap();
        let image = image::load_from_memory(&encoded).unwrap();

        assert_eq!(image.width(), 300);
    }

    #[test]
    fn la_pochette_embarquee_reste_legere() {
        // Elle voyage dans chaque fichier : la garder en 1500 px multiplierait
        // le poids de la bibliothèque.
        let picture = embedded_picture(&png(3000)).unwrap();

        assert!(
            picture.data().len() < 300_000,
            "poids obtenu {} octets",
            picture.data().len()
        );
        assert_eq!(picture.pic_type(), PictureType::CoverFront);
    }

    #[test]
    fn une_image_invalide_est_refusee_sans_panique() {
        assert!(resize_jpeg(b"pas une image", 500, 80).is_err());
        assert!(embedded_picture(b"<html>404</html>").is_err());
    }

    #[test]
    fn la_pochette_est_deposee_sous_un_nom_universel() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("Daft Punk/2001 - Discovery");

        store_album_cover(&album, &png(1000)).unwrap();

        let cover = album.join(ALBUM_COVER_FILE);
        assert!(cover.is_file(), "les autres lecteurs cherchent ce nom");
        assert!(image::open(&cover).is_ok());
    }
}
