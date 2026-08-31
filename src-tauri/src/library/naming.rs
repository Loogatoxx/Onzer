//! Assainissement des noms et construction des chemins de rangement (ADR-007).
//!
//! Deux responsabilités bien distinctes, souvent confondues :
//!
//! * [`sanitize_segment`] produit un nom **affichable et écrivable sur disque**.
//!   Il préserve la casse et les accents : « Beyoncé » reste « Beyoncé ».
//! * [`normalize_key`] produit une **clé de dédoublonnage**, jamais affichée.
//!   Elle dépouille tout : casse, accents, ponctuation. « Beyoncé » devient
//!   « beyonce », ce qui la fait correspondre à « BEYONCE » et « beyonce ».

use unicode_normalization::UnicodeNormalization;

/// Caractères refusés par exFAT (et par Windows, ce qui rend la bibliothèque
/// portable si le SSD change un jour de machine).
const FORBIDDEN: &[char] = &['"', '*', '/', ':', '<', '>', '?', '\\', '|'];

/// exFAT autorise 255 caractères par segment. On garde une marge confortable
/// pour les suffixes de désambiguïsation (« (2) ») et les extensions.
const MAX_SEGMENT_BYTES: usize = 180;

/// Dossier des morceaux dont l'artiste n'a pas pu être identifié.
pub const UNSORTED_DIR: &str = "_À trier";

/// Dossier des morceaux sans album — cas fréquent des titres récupérés à
/// l'unité par un script d'import.
pub const SINGLES_DIR: &str = "Singles";

/// Dossier de dépôt surveillé, à la racine de la bibliothèque.
///
/// Défini ici plutôt que dans `ingest` parce qu'il s'agit d'une convention de
/// nommage de la bibliothèque : le scanner doit savoir l'exclure, sans quoi il
/// indexerait des fichiers en cours de téléchargement.
pub const INBOX_DIR: &str = "_Inbox";

/// Sous-dossier du dépôt où atterrissent les fichiers déjà connus.
///
/// Onzer ne supprime rien de lui-même, mais laisser un doublon dans le dépôt
/// le condamne à être réexaminé à chaque démarrage. Trente-sept fichiers déjà
/// rangés y ont ainsi tourné en boucle. Le déplacer d'un cran suffit : le
/// fichier reste consultable, il cesse d'être un candidat.
pub const INBOX_DUPLICATES_DIR: &str = "_Doublons";

/// Rend une chaîne utilisable comme nom de dossier ou de fichier.
///
/// Retourne `None` si rien d'exploitable ne subsiste, à charge de l'appelant
/// de fournir une valeur de repli.
pub fn sanitize_segment(raw: &str) -> Option<String> {
    let mut cleaned = String::with_capacity(raw.len());

    for character in raw.chars() {
        if FORBIDDEN.contains(&character) || character.is_control() {
            // Remplacé par une espace plutôt que supprimé : « Album:Suite »
            // donne « Album Suite » et non « AlbumSuite ».
            cleaned.push(' ');
        } else {
            cleaned.push(character);
        }
    }

    // Espaces multiples réduits à un seul.
    let mut collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");

    // exFAT et Windows n'acceptent ni point ni espace en fin de segment.
    while collapsed.ends_with('.') || collapsed.ends_with(' ') {
        collapsed.pop();
    }

    truncate_on_char_boundary(&mut collapsed, MAX_SEGMENT_BYTES);

    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

/// Clé de dédoublonnage : minuscules, sans accents ni ponctuation.
///
/// Quelques substitutions courantes sont appliquées avant dépouillement, sans
/// quoi « A$AP Rocky » et « ASAP Rocky » resteraient deux artistes distincts.
/// C'est une heuristique, pas une science : elle couvre les cas fréquents
/// (`$`, `@`, `!`) et laisse le reste tranquille.
pub fn normalize_key(raw: &str) -> String {
    let substituted: String = raw
        .chars()
        .map(|character| match character {
            '$' => 's',
            '@' => 'a',
            '!' => 'i',
            '&' => '+', // « Simon & Garfunkel » ≈ « Simon and Garfunkel »
            other => other,
        })
        .collect();

    // La décomposition NFD sépare « é » en « e » + accent aigu ; il suffit
    // ensuite de jeter les marques combinantes.
    let stripped: String = substituted
        .nfd()
        .filter(|character| !is_combining_mark(*character))
        .collect();

    let mut key = String::with_capacity(stripped.len());
    let mut previous_was_space = true; // évite l'espace de tête

    for character in stripped.chars() {
        let lowered = character.to_lowercase().next().unwrap_or(character);

        if lowered.is_alphanumeric() {
            key.push(lowered);
            previous_was_space = false;
        } else if lowered == '+' {
            key.push_str("and");
            previous_was_space = false;
        } else if !previous_was_space {
            key.push(' ');
            previous_was_space = true;
        }
    }

    key.trim_end().to_string()
}

/// Les marques combinantes Unicode couvrent les diacritiques latins, grecs et
/// cyrilliques — soit tout ce que la décomposition NFD isole.
fn is_combining_mark(character: char) -> bool {
    matches!(character as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Tronque sans jamais couper au milieu d'un caractère multi-octets.
fn truncate_on_char_boundary(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }

    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    text.truncate(end);
    while text.ends_with(' ') {
        text.pop();
    }
}

/// Description minimale nécessaire pour ranger un morceau.
#[derive(Debug, Clone)]
pub struct FilingInfo<'a> {
    /// Artiste de rangement : l'*album artist* de préférence, jamais
    /// « X feat. Y » — sinon un featuring crée un dossier par collaboration.
    pub filing_artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub title: &'a str,
    /// Sans le point (« mp3 », « flac »…).
    pub extension: &'a str,
}

/// Construit le chemin **relatif** de rangement, séparateur `/`.
///
/// Applique la convention de l'ADR-007 et ses quatre cas particuliers :
///
/// ```text
/// nominal       →  Daft Punk/2001 - Discovery/03 - Digital Love.mp3
/// sans année    →  Daft Punk/Discovery/03 - Digital Love.mp3
/// sans album    →  Daft Punk/Singles/Digital Love.mp3
/// sans artiste  →  _À trier/Digital Love.mp3
/// ```
pub fn build_relative_path(info: &FilingInfo<'_>) -> String {
    let title = sanitize_segment(info.title).unwrap_or_else(|| "Sans titre".to_string());

    let Some(artist) = info.filing_artist.and_then(sanitize_segment) else {
        return format!("{UNSORTED_DIR}/{title}.{}", info.extension);
    };

    let Some(album) = info.album.and_then(sanitize_segment) else {
        return format!("{artist}/{SINGLES_DIR}/{title}.{}", info.extension);
    };

    let album_dir = match info.year {
        Some(year) => format!("{year} - {album}"),
        None => album,
    };

    let filename = match info.track_no {
        // Le numéro de disque n'apparaît que sur les albums multi-disques,
        // pour ne pas alourdir le cas courant.
        Some(track) => match info.disc_no.filter(|disc| *disc > 1) {
            Some(disc) => format!("{disc}-{track:02} - {title}"),
            None => format!("{track:02} - {title}"),
        },
        None => title,
    };

    format!("{artist}/{album_dir}/{filename}.{}", info.extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Assainissement ──────────────────────────────────────────────────

    #[test]
    fn conserve_accents_et_casse() {
        assert_eq!(sanitize_segment("Beyoncé").as_deref(), Some("Beyoncé"));
        assert_eq!(sanitize_segment("Björk").as_deref(), Some("Björk"));
    }

    #[test]
    fn remplace_les_caracteres_interdits_par_une_espace() {
        assert_eq!(
            sanitize_segment("AC/DC").as_deref(),
            Some("AC DC"),
            "une barre oblique créerait un dossier fantôme"
        );
        assert_eq!(
            sanitize_segment("Album: la suite").as_deref(),
            Some("Album la suite")
        );
    }

    #[test]
    fn supprime_points_et_espaces_finaux() {
        // exFAT et Windows refusent ces terminaisons.
        assert_eq!(sanitize_segment("Album...").as_deref(), Some("Album"));
        assert_eq!(sanitize_segment("Album   ").as_deref(), Some("Album"));
    }

    #[test]
    fn rejette_un_nom_entierement_illegal() {
        assert_eq!(sanitize_segment("///"), None);
        assert_eq!(sanitize_segment("   "), None);
    }

    #[test]
    fn tronque_sans_couper_un_caractere_multioctet() {
        let long = "é".repeat(200); // 400 octets
        let result = sanitize_segment(&long).unwrap();

        assert!(result.len() <= MAX_SEGMENT_BYTES);
        assert!(
            result.chars().all(|c| c == 'é'),
            "la troncature ne doit pas produire de caractère invalide"
        );
    }

    // ── Clé de dédoublonnage ────────────────────────────────────────────

    #[test]
    fn dedoublonne_malgre_la_casse_et_les_accents() {
        assert_eq!(normalize_key("Beyoncé"), "beyonce");
        assert_eq!(normalize_key("BEYONCE"), "beyonce");
        assert_eq!(normalize_key("beyoncé"), "beyonce");
    }

    #[test]
    fn dedoublonne_les_graphies_stylisees() {
        assert_eq!(normalize_key("A$AP Rocky"), normalize_key("ASAP Rocky"));
        assert_eq!(normalize_key("P!nk"), normalize_key("Pink"));
        assert_eq!(
            normalize_key("Simon & Garfunkel"),
            normalize_key("Simon and Garfunkel")
        );
    }

    #[test]
    fn ignore_la_ponctuation_et_les_espaces_multiples() {
        assert_eq!(normalize_key("Guns N' Roses"), normalize_key("Guns N Roses"));
        assert_eq!(normalize_key("  Air   "), "air");
    }

    #[test]
    fn ne_confond_pas_deux_artistes_distincts() {
        assert_ne!(normalize_key("Air"), normalize_key("Air France"));
        assert_ne!(normalize_key("The Cure"), normalize_key("Cure"));
    }

    // ── Chemins de rangement ────────────────────────────────────────────

    fn info<'a>(
        artist: Option<&'a str>,
        album: Option<&'a str>,
        year: Option<u32>,
        track: Option<u32>,
        title: &'a str,
    ) -> FilingInfo<'a> {
        FilingInfo {
            filing_artist: artist,
            album,
            year,
            track_no: track,
            disc_no: None,
            title,
            extension: "mp3",
        }
    }

    #[test]
    fn range_selon_la_convention_nominale() {
        let path = build_relative_path(&info(
            Some("Daft Punk"),
            Some("Discovery"),
            Some(2001),
            Some(3),
            "Digital Love",
        ));

        assert_eq!(path, "Daft Punk/2001 - Discovery/03 - Digital Love.mp3");
    }

    #[test]
    fn omet_le_prefixe_dannee_si_elle_est_inconnue() {
        let path = build_relative_path(&info(
            Some("Daft Punk"),
            Some("Discovery"),
            None,
            Some(3),
            "Digital Love",
        ));

        assert_eq!(path, "Daft Punk/Discovery/03 - Digital Love.mp3");
    }

    #[test]
    fn isole_les_singles_dans_leur_dossier() {
        let path = build_relative_path(&info(Some("Daft Punk"), None, None, None, "Digital Love"));
        assert_eq!(path, "Daft Punk/Singles/Digital Love.mp3");
    }

    #[test]
    fn parque_les_morceaux_sans_artiste_a_trier() {
        let path = build_relative_path(&info(None, None, None, None, "Inconnu"));
        assert_eq!(path, "_À trier/Inconnu.mp3");
    }

    #[test]
    fn prefixe_le_numero_de_disque_uniquement_si_multidisque() {
        let mut multi = info(Some("The Beatles"), Some("Blanc"), Some(1968), Some(4), "Ob-La-Di");
        multi.disc_no = Some(2);
        assert_eq!(
            build_relative_path(&multi),
            "The Beatles/1968 - Blanc/2-04 - Ob-La-Di.mp3"
        );

        // Disque 1 sur un album simple : le préfixe serait du bruit.
        let mut single = multi.clone();
        single.disc_no = Some(1);
        assert_eq!(
            build_relative_path(&single),
            "The Beatles/1968 - Blanc/04 - Ob-La-Di.mp3"
        );
    }

    #[test]
    fn assainit_chaque_segment_du_chemin() {
        let path = build_relative_path(&info(
            Some("AC/DC"),
            Some("Back In Black"),
            Some(1980),
            Some(1),
            "Hells Bells: intro",
        ));

        assert_eq!(
            path, "AC DC/1980 - Back In Black/01 - Hells Bells intro.mp3",
            "aucun segment ne doit introduire de séparateur parasite"
        );
    }

    #[test]
    fn le_chemin_ne_remonte_jamais_hors_de_la_bibliotheque() {
        let path = build_relative_path(&info(
            Some("../../../etc"),
            Some(".."),
            None,
            None,
            "passwd",
        ));

        assert!(
            !path.split('/').any(|segment| segment == ".."),
            "un tag malveillant ne doit pas produire de remontée : {path}"
        );
    }
}
