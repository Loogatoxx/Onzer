//! Lecture et normalisation des métadonnées audio.
//!
//! Objectif affiché du projet : **ne jamais avoir à saisir de métadonnées à la
//! main**. Ce module fait donc de son mieux avec ce qu'il trouve, et se rabat
//! sur le nom de fichier quand les tags sont absents — cas très fréquent des
//! titres récupérés par un script de téléchargement.

use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, ItemKey};

use crate::core::{OnzerError, Result};

/// Extensions reconnues comme audio lors du parcours de la bibliothèque.
pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "aiff", "wv"];

pub fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| SUPPORTED_EXTENSIONS.contains(&extension.as_str()))
}

/// Pochette extraite d'un fichier audio.
#[derive(Debug, Clone)]
pub struct Artwork {
    pub data: Vec<u8>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub title: String,
    /// Artistes principaux, hors featurings.
    pub artists: Vec<String>,
    /// Artistes invités, extraits d'une mention « feat. » dans le tag artiste.
    pub featured_artists: Vec<String>,
    /// Artiste de l'album, s'il est renseigné. C'est lui qui sert au rangement.
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u32>,
    pub genres: Vec<String>,

    pub duration_ms: i64,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub format: String,

    pub artwork: Option<Artwork>,
    /// Paroles brutes telles que le fichier les contient.
    ///
    /// Conservées non analysées : le même champ porte indifféremment du texte
    /// simple ou du LRC horodaté, et c'est à l'affichage de trancher.
    pub lyrics: Option<String>,
    /// Vrai si les tags étaient absents ou vides et que le nom de fichier a
    /// servi de source. Permet de signaler ces morceaux dans l'interface.
    pub from_filename: bool,
}

impl TrackMetadata {
    /// Artiste sous lequel ranger le fichier (ADR-007).
    ///
    /// L'*album artist* prime : sans cela, un album avec des invités
    /// éparpillerait ses pistes dans autant de dossiers qu'il y a de
    /// collaborations.
    pub fn filing_artist(&self) -> Option<&str> {
        self.album_artist
            .as_deref()
            .or_else(|| self.artists.first().map(String::as_str))
    }
}

/// Métadonnées fournies par un script externe lors d'un import automatique.
///
/// Un téléchargeur connaît souvent le titre et l'artiste par la page source,
/// alors que le fichier obtenu n'a aucun tag exploitable.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataHint {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub genre: Option<String>,
}

impl MetadataHint {
    /// Applique les indications aux métadonnées lues dans le fichier.
    ///
    /// **Règle d'arbitrage :** un vrai tag présent dans le fichier prime
    /// toujours sur une indication. En revanche, si les métadonnées ont été
    /// **déduites du nom de fichier** (`from_filename`), les indications
    /// l'emportent : le script connaît la source, le nom de fichier n'est
    /// qu'une supposition.
    pub fn apply(&self, metadata: &mut TrackMetadata) {
        let guessed = metadata.from_filename;

        if let Some(title) = self.non_empty(&self.title) {
            if guessed || metadata.title.is_empty() {
                metadata.title = title;
            }
        }

        if let Some(artist) = self.non_empty(&self.artist) {
            if guessed || metadata.artists.is_empty() {
                let (main, featured) = split_featuring(&artist);
                metadata.artists = main;
                metadata.featured_artists = featured;
            }
        }

        if let Some(album) = self.non_empty(&self.album) {
            if guessed || metadata.album.is_none() {
                metadata.album = Some(album);
            }
        }

        if let Some(album_artist) = self.non_empty(&self.album_artist) {
            if guessed || metadata.album_artist.is_none() {
                metadata.album_artist = Some(album_artist);
            }
        }

        if let Some(genre) = self.non_empty(&self.genre) {
            if metadata.genres.is_empty() {
                metadata.genres.push(genre);
            }
        }

        // Les valeurs numériques ne comblent que les manques : une année
        // extraite d'un tag est plus fiable qu'une année devinée d'une page web.
        metadata.year = metadata.year.or(self.year);
        metadata.track_no = metadata.track_no.or(self.track_no);
        metadata.disc_no = metadata.disc_no.or(self.disc_no);
    }

    fn non_empty(&self, value: &Option<String>) -> Option<String> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    }
}

/// Lit les métadonnées d'un fichier audio.
pub fn read(path: &Path) -> Result<TrackMetadata> {
    let tagged = lofty::read_from_path(path)
        .map_err(|error| OnzerError::Invalid(format!("lecture audio impossible : {error}")))?;

    let properties = tagged.properties();
    let duration_ms = properties.duration().as_millis() as i64;

    if duration_ms == 0 {
        return Err(OnzerError::Invalid(
            "durée nulle : fichier vraisemblablement corrompu".to_string(),
        ));
    }

    let format = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("inconnu")
        .to_ascii_lowercase();

    let mut metadata = TrackMetadata {
        title: String::new(),
        artists: Vec::new(),
        featured_artists: Vec::new(),
        album_artist: None,
        album: None,
        track_no: None,
        disc_no: None,
        year: None,
        genres: Vec::new(),
        duration_ms,
        bitrate: properties.audio_bitrate(),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        format,
        artwork: None,
        lyrics: None,
        from_filename: false,
    };

    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        metadata.title = tag.title().map(clean).unwrap_or_default();
        metadata.album = tag.album().map(clean).filter(|value| !value.is_empty());
        metadata.track_no = tag.track();
        metadata.disc_no = tag.disk();
        metadata.year = tag.year();

        if let Some(genre) = tag.genre().map(clean).filter(|value| !value.is_empty()) {
            metadata.genres.push(genre);
        }

        metadata.album_artist = tag
            .get_string(&ItemKey::AlbumArtist)
            .map(clean)
            .filter(|value| !value.is_empty());

        if let Some(raw_artist) = tag.artist().map(clean).filter(|value| !value.is_empty()) {
            let (main, featured) = split_featuring(&raw_artist);
            metadata.artists = main;
            metadata.featured_artists = featured;
        }

        metadata.artwork = extract_artwork(tag.pictures());

        metadata.lyrics = tag
            .get_string(&ItemKey::Lyrics)
            .map(clean)
            .filter(|value| !value.is_empty());
    }

    // Repli sur le nom de fichier si les tags n'ont rien donné d'utile.
    if metadata.title.is_empty() || metadata.artists.is_empty() {
        apply_filename_fallback(path, &mut metadata);
    }

    if metadata.title.is_empty() {
        metadata.title = "Sans titre".to_string();
    }

    Ok(metadata)
}

fn clean<S: AsRef<str>>(value: S) -> String {
    value.as_ref().trim().to_string()
}

/// Choisit la meilleure pochette disponible : la couverture avant en priorité.
fn extract_artwork(pictures: &[lofty::picture::Picture]) -> Option<Artwork> {
    let picture = pictures
        .iter()
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())?;

    Some(Artwork {
        data: picture.data().to_vec(),
        mime_type: picture.mime_type().map(ToString::to_string),
    })
}

/// Sépare les artistes invités de l'artiste principal.
///
/// Volontairement conservateur : **seules** les mentions explicites de
/// featuring sont découpées. On ne coupe pas sur « & » ni sur la virgule, car
/// cela démantèlerait des noms de groupes légitimes comme « Earth, Wind & Fire »
/// ou « Simon & Garfunkel ». Mieux vaut un artiste composé qu'un faux découpage.
fn split_featuring(raw: &str) -> (Vec<String>, Vec<String>) {
    const MARKERS: &[&str] = &[
        " feat. ", " feat ", " ft. ", " ft ", " featuring ", " avec ", " w/ ",
    ];

    let lowered = raw.to_lowercase();

    for marker in MARKERS {
        if let Some(position) = lowered.find(marker) {
            let main = clean(&raw[..position]);
            let guests = &raw[position + marker.len()..];

            let featured: Vec<String> = guests
                .split(&[',', '&'][..])
                .map(clean)
                .filter(|value| !value.is_empty())
                .collect();

            if !main.is_empty() {
                return (vec![main], featured);
            }
        }
    }

    (vec![clean(raw)], Vec::new())
}

/// Déduit titre, artiste et numéro de piste du nom de fichier.
///
/// Motifs reconnus, du plus riche au plus pauvre :
///
/// ```text
/// 03 - Daft Punk - Digital Love   →  piste 3, Daft Punk, Digital Love
/// 03 - Digital Love               →  piste 3, Digital Love
/// 03. Digital Love                →  piste 3, Digital Love
/// Daft Punk - Digital Love        →  Daft Punk, Digital Love
/// Digital Love                    →  Digital Love
/// ```
fn apply_filename_fallback(path: &Path, metadata: &mut TrackMetadata) {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return;
    };

    metadata.from_filename = true;

    let mut remaining = stem.trim();

    // Numéro de piste en tête : « 03 - », « 03. », « 03 ».
    if let Some(parsed) = leading_track_number(remaining) {
        if metadata.track_no.is_none() {
            metadata.track_no = Some(parsed.0);
        }
        remaining = parsed.1;
    }

    // Le premier « - » restant sépare l'artiste du titre.
    let (artist, title) = match remaining.split_once(" - ") {
        Some((left, right)) if !left.trim().is_empty() && !right.trim().is_empty() => {
            (Some(clean(left)), clean(right))
        }
        _ => (None, clean(remaining)),
    };

    if metadata.title.is_empty() {
        metadata.title = title;
    }

    if metadata.artists.is_empty() {
        if let Some(artist) = artist {
            let (main, featured) = split_featuring(&artist);
            metadata.artists = main;
            metadata.featured_artists = featured;
        }
    }
}

/// Extrait un numéro de piste en tête de chaîne et retourne le reste.
fn leading_track_number(text: &str) -> Option<(u32, &str)> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();

    // Deux chiffres au plus : « 1999 - Titre » est une année, pas une piste.
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }

    let number: u32 = digits.parse().ok()?;
    let rest = text[digits.len()..].trim_start();
    let rest = rest
        .strip_prefix('-')
        .or_else(|| rest.strip_prefix('.'))
        .unwrap_or(rest)
        .trim_start();

    if rest.is_empty() {
        return None;
    }

    Some((number, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_metadata() -> TrackMetadata {
        TrackMetadata {
            title: String::new(),
            artists: Vec::new(),
            featured_artists: Vec::new(),
            album_artist: None,
            album: None,
            track_no: None,
            disc_no: None,
            year: None,
            genres: Vec::new(),
            duration_ms: 1000,
            bitrate: None,
            sample_rate: None,
            channels: None,
            format: "mp3".to_string(),
            artwork: None,
            lyrics: None,
            from_filename: false,
        }
    }

    fn fallback(filename: &str) -> TrackMetadata {
        let mut metadata = empty_metadata();
        apply_filename_fallback(&PathBuf::from(filename), &mut metadata);
        metadata
    }

    // ── Featurings ──────────────────────────────────────────────────────

    #[test]
    fn isole_les_artistes_invites() {
        let (main, featured) = split_featuring("Daft Punk feat. Pharrell Williams");
        assert_eq!(main, vec!["Daft Punk"]);
        assert_eq!(featured, vec!["Pharrell Williams"]);
    }

    #[test]
    fn reconnait_les_abreviations_de_featuring() {
        for raw in ["Drake ft. Rihanna", "Drake ft Rihanna", "Drake featuring Rihanna"] {
            let (main, featured) = split_featuring(raw);
            assert_eq!(main, vec!["Drake"], "échec sur « {raw} »");
            assert_eq!(featured, vec!["Rihanna"], "échec sur « {raw} »");
        }
    }

    #[test]
    fn ne_demantele_pas_les_noms_de_groupes() {
        // Le piège classique : découper sur « & » ou la virgule casserait ces
        // noms. On préfère un artiste composé à un faux découpage.
        let (main, featured) = split_featuring("Simon & Garfunkel");
        assert_eq!(main, vec!["Simon & Garfunkel"]);
        assert!(featured.is_empty());

        let (main, _) = split_featuring("Earth, Wind & Fire");
        assert_eq!(main, vec!["Earth, Wind & Fire"]);
    }

    #[test]
    fn separe_plusieurs_invites() {
        let (_, featured) = split_featuring("Calvin Harris feat. Dua Lipa & Young Thug");
        assert_eq!(featured, vec!["Dua Lipa", "Young Thug"]);
    }

    // ── Repli sur le nom de fichier ─────────────────────────────────────

    #[test]
    fn deduit_piste_artiste_et_titre() {
        let metadata = fallback("03 - Daft Punk - Digital Love.mp3");
        assert_eq!(metadata.track_no, Some(3));
        assert_eq!(metadata.artists, vec!["Daft Punk"]);
        assert_eq!(metadata.title, "Digital Love");
    }

    #[test]
    fn deduit_piste_et_titre_sans_artiste() {
        let metadata = fallback("03 - Digital Love.mp3");
        assert_eq!(metadata.track_no, Some(3));
        assert_eq!(metadata.title, "Digital Love");
        assert!(metadata.artists.is_empty());
    }

    #[test]
    fn accepte_le_point_comme_separateur_de_piste() {
        let metadata = fallback("07. Veridis Quo.flac");
        assert_eq!(metadata.track_no, Some(7));
        assert_eq!(metadata.title, "Veridis Quo");
    }

    #[test]
    fn deduit_artiste_et_titre_sans_numero() {
        let metadata = fallback("Daft Punk - Digital Love.mp3");
        assert_eq!(metadata.track_no, None);
        assert_eq!(metadata.artists, vec!["Daft Punk"]);
        assert_eq!(metadata.title, "Digital Love");
    }

    #[test]
    fn ne_prend_pas_une_annee_pour_un_numero_de_piste() {
        // « 1999 - Titre » : quatre chiffres, donc une année.
        let metadata = fallback("1999 - Party.mp3");
        assert_eq!(metadata.track_no, None);
        assert_eq!(metadata.artists, vec!["1999"]);
        assert_eq!(metadata.title, "Party");
    }

    #[test]
    fn se_contente_du_titre_seul() {
        let metadata = fallback("Digital Love.mp3");
        assert_eq!(metadata.title, "Digital Love");
        assert!(metadata.artists.is_empty());
    }

    #[test]
    fn extrait_le_featuring_depuis_le_nom_de_fichier() {
        let metadata = fallback("Daft Punk feat. Pharrell - Get Lucky.mp3");
        assert_eq!(metadata.artists, vec!["Daft Punk"]);
        assert_eq!(metadata.featured_artists, vec!["Pharrell"]);
        assert_eq!(metadata.title, "Get Lucky");
    }

    #[test]
    fn signale_que_les_donnees_viennent_du_nom_de_fichier() {
        assert!(fallback("Digital Love.mp3").from_filename);
    }

    // ── Artiste de rangement ────────────────────────────────────────────

    #[test]
    fn range_sous_lartiste_de_lalbum_quand_il_existe() {
        let mut metadata = empty_metadata();
        metadata.album_artist = Some("Various Artists".to_string());
        metadata.artists = vec!["Daft Punk".to_string()];

        assert_eq!(metadata.filing_artist(), Some("Various Artists"));
    }

    #[test]
    fn se_rabat_sur_lartiste_principal() {
        let mut metadata = empty_metadata();
        metadata.artists = vec!["Daft Punk".to_string()];

        assert_eq!(metadata.filing_artist(), Some("Daft Punk"));
    }

    // ── Détection des fichiers audio ────────────────────────────────────

    #[test]
    fn reconnait_les_extensions_audio_quelle_que_soit_la_casse() {
        assert!(is_supported_audio(Path::new("a.mp3")));
        assert!(is_supported_audio(Path::new("a.FLAC")));
        assert!(!is_supported_audio(Path::new("cover.jpg")));
        assert!(!is_supported_audio(Path::new("notes.txt")));
        assert!(!is_supported_audio(Path::new("sans-extension")));
    }
}
