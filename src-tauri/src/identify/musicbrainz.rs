//! Client MusicBrainz : de l'identifiant aux métadonnées complètes.
//!
//! # Le vrai problème : choisir la bonne parution
//!
//! Un même enregistrement figure sur des dizaines de parutions — l'album
//! original, sa réédition remasterisée, trois compilations, un single, une
//! bande originale, une édition japonaise. MusicBrainz les renvoie **toutes**.
//!
//! Choisir la première de la liste donnerait des résultats aberrants : un
//! morceau de Daft Punk rangé sous « Now That's What I Call Music! 48 ». Ce
//! module applique donc une heuristique explicite, et testée :
//!
//! | Critère | Poids |
//! |---|---|
//! | Parution officielle | Fortement privilégiée |
//! | Type « Album » | Privilégié sur single, compilation, bande originale |
//! | Date la plus ancienne | Départage — c'est la parution d'origine qu'on veut |

use serde::Deserialize;

use crate::core::Result;

use super::http::Service;
use super::ratelimit::MUSICBRAINZ_MIN_INTERVAL;

const ENDPOINT: &str = "https://musicbrainz.org/ws/2/recording";

/// Métadonnées résolues, prêtes à être écrites dans le fichier.
#[derive(Debug, Clone, Default)]
pub struct RecordingMetadata {
    pub recording_mbid: String,
    pub title: String,
    /// Artistes principaux, featurings exclus.
    pub artists: Vec<String>,
    pub featured_artists: Vec<String>,
    pub album: Option<String>,
    /// Identifiant de la parution retenue — nécessaire à la pochette.
    pub release_mbid: Option<String>,
    pub release_group_mbid: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub genre: Option<String>,
    /// Durée de l'enregistrement selon MusicBrainz. Sert à corroborer.
    pub length_ms: Option<i64>,
    /// Nombre de parutions rattachées, compilations comprises.
    ///
    /// Zéro trahit souvent une fiche versée depuis un rip, sans existence
    /// discographique — c'est un signal, pas une preuve.
    pub release_count: usize,
}

impl RecordingMetadata {
    /// Artiste sous lequel ranger le morceau.
    ///
    /// L'artiste de l'enregistrement, et non celui de la parution : sur une
    /// compilation, l'artiste de parution vaut « Various Artists », ce qui
    /// entasserait la moitié de la bibliothèque dans un seul dossier.
    pub fn filing_artist(&self) -> Option<&str> {
        self.artists.first().map(String::as_str)
    }
}

pub struct MusicBrainzClient {
    service: Service,
}

impl MusicBrainzClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: Service::new("MusicBrainz", MUSICBRAINZ_MIN_INTERVAL)?,
        })
    }

    /// Récupère les métadonnées d'un enregistrement.
    pub async fn lookup(&self, recording_mbid: &str) -> Result<Option<RecordingMetadata>> {
        let url = format!(
            "{ENDPOINT}/{recording_mbid}?fmt=json&inc=artists+releases+media+release-groups+genres"
        );

        let Some(recording) = self.service.get_json::<Recording>(&url).await? else {
            return Ok(None);
        };

        Ok(Some(build_metadata(recording)))
    }
}

/// Assemble les métadonnées à partir de la réponse brute.
///
/// Séparé de l'appel réseau pour être testable sur des réponses fabriquées —
/// ce qui compte d'autant plus que toute la subtilité est ici.
fn build_metadata(recording: Recording) -> RecordingMetadata {
    let (artists, featured_artists) = split_artist_credit(&recording.artist_credit);
    let release_count = recording.releases.len();

    // `pick_release` ne rend qu'une parution *représentative* : elle vaut `None`
    // quand le morceau n'est connu que par des compilations. Une parution reste
    // alors nécessaire pour aller chercher la pochette — n'importe laquelle fera
    // l'affaire, une compilation reproduit la bonne image dans la majorité des
    // cas et il vaut mieux une pochette approximative que pas de pochette.
    let best = pick_release(&recording.releases);
    let for_cover = best.or_else(|| pick_any_release(&recording.releases));

    let (track_no, disc_no) = best
        .and_then(track_position)
        .unwrap_or((None, None));

    RecordingMetadata {
        recording_mbid: recording.id,
        title: recording.title,
        artists,
        featured_artists,
        album: best.map(|release| release.title.clone()),
        release_mbid: for_cover.map(|release| release.id.clone()),
        release_group_mbid: for_cover.and_then(|release| {
            release.release_group.as_ref().map(|group| group.id.clone())
        }),
        year: best.and_then(release_year),
        track_no,
        disc_no,
        genre: best_genre(&recording.genres),
        length_ms: recording.length.map(i64::from),
        release_count,
    }
}

/// Sépare artistes principaux et invités, d'après les liaisons du crédit.
///
/// MusicBrainz exprime « Daft Punk feat. Pharrell » par une liste d'artistes
/// séparés par des `joinphrase`. Le texte de liaison dit tout : « feat. »
/// introduit un invité, « & » un co-titulaire.
fn split_artist_credit(credits: &[ArtistCredit]) -> (Vec<String>, Vec<String>) {
    let mut main = Vec::new();
    let mut featured = Vec::new();
    let mut in_featuring = false;

    for credit in credits {
        let name = credit
            .artist
            .as_ref()
            .map(|artist| artist.name.clone())
            .unwrap_or_else(|| credit.name.clone());

        if !name.trim().is_empty() {
            if in_featuring {
                featured.push(name);
            } else {
                main.push(name);
            }
        }

        // La liaison qui SUIT cet artiste détermine le statut du suivant.
        let join = credit.joinphrase.to_lowercase();
        if join.contains("feat") || join.contains("with") {
            in_featuring = true;
        }
    }

    (main, featured)
}

/// Choisit la parution qui peut légitimement servir d'**album**.
///
/// # Pourquoi les compilations sont écartées, et non simplement reléguées
///
/// « Macarena » de Damso paraît sur *Ipséité* en 2017. Dans MusicBrainz, cet
/// enregistrement n'est rattaché qu'à **neuf compilations**, aucune n'étant
/// l'album. Le classement départageait alors « à qualité égale, la plus
/// ancienne — c'est la parution d'origine », et retenait *I migliori anni '90*,
/// paru en **2009** : huit ans avant que le morceau existe.
///
/// La règle est juste entre deux albums, absurde entre deux compilations. Quand
/// il n'y a que des compilations, il n'y a **pas d'album connu** : on ne renvoie
/// rien, et les tags que le fichier portait déjà sont conservés. Une case vide
/// vaut mieux qu'une fausse réponse.
fn pick_release(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|release| !is_compilation(release))
        .max_by(|a, b| release_rank(a).total_cmp(&release_rank(b)))
}

/// Parution de repli, compilations comprises. Sert uniquement à la pochette.
fn pick_any_release(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .max_by(|a, b| release_rank(a).total_cmp(&release_rank(b)))
}

/// Une compilation contient le morceau, mais n'est pas son album.
fn is_compilation(release: &Release) -> bool {
    let Some(group) = &release.release_group else {
        return false;
    };

    group.primary_type.as_deref() == Some("Compilation")
        || group
            .secondary_types
            .iter()
            .any(|kind| kind.eq_ignore_ascii_case("compilation"))
}

/// Note une parution. Plus c'est haut, plus c'est le bon choix.
fn release_rank(release: &Release) -> f64 {
    let mut score = 0.0;

    // Une parution officielle prime largement sur un bootleg ou une
    // pseudo-parution.
    if release.status.as_deref() == Some("Official") {
        score += 100.0;
    }

    score += match release
        .release_group
        .as_ref()
        .and_then(|group| group.primary_type.as_deref())
    {
        Some("Album") => 50.0,
        Some("EP") => 30.0,
        Some("Single") => 20.0,
        // Une compilation contient le morceau, mais n'est pas son album.
        Some("Compilation") => 5.0,
        _ => 10.0,
    };

    // Une parution secondaire (compilation, live, remix) est reléguée même si
    // son type primaire est « Album ».
    if let Some(group) = &release.release_group {
        if !group.secondary_types.is_empty() {
            score -= 40.0;
        }
    }

    // À qualité égale, la plus ancienne : c'est la parution d'origine.
    // Bornée pour rester très inférieure aux critères de nature.
    if let Some(year) = release_year(release) {
        score += (2100.0 - f64::from(year)) / 100.0;
    }

    score
}

fn release_year(release: &Release) -> Option<u32> {
    // La date du groupe de parution est la date d'origine ; celle de la
    // parution peut être une réédition tardive.
    release
        .release_group
        .as_ref()
        .and_then(|group| group.first_release_date.as_deref())
        .or(release.date.as_deref())
        .and_then(parse_year)
}

/// Extrait l'année d'une date MusicBrainz (`2001`, `2001-03`, `2001-03-12`).
fn parse_year(date: &str) -> Option<u32> {
    let year: u32 = date.get(..4)?.parse().ok()?;
    (1900..=2200).contains(&year).then_some(year)
}

/// Numéro de piste et de disque, tels que la parution retenue les donne.
fn track_position(release: &Release) -> Option<(Option<u32>, Option<u32>)> {
    let medium = release.media.first()?;
    let track = medium.track.first();

    let number = track.and_then(|track| {
        // Le champ `number` est textuel : sur un vinyle il vaut « A1 ».
        // On retombe alors sur la position, toujours numérique.
        track
            .number
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
            .or(track.position)
    });

    Some((number, medium.position))
}

/// Genre le plus fréquemment attribué par la communauté.
fn best_genre(genres: &[Genre]) -> Option<String> {
    genres
        .iter()
        .filter(|genre| !genre.name.trim().is_empty())
        .max_by_key(|genre| genre.count)
        .map(|genre| genre.name.clone())
}

// ── Réponse de l'API ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Recording {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
    #[serde(default)]
    releases: Vec<Release>,
    #[serde(default)]
    genres: Vec<Genre>,
    /// Durée en millisecondes. Absente sur les fiches incomplètes.
    #[serde(default)]
    length: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ArtistCredit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    joinphrase: String,
    #[serde(default)]
    artist: Option<Artist>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "release-group", default)]
    release_group: Option<ReleaseGroup>,
    #[serde(default)]
    media: Vec<Medium>,
}

#[derive(Debug, Deserialize)]
struct ReleaseGroup {
    #[serde(default)]
    id: String,
    #[serde(rename = "primary-type", default)]
    primary_type: Option<String>,
    #[serde(rename = "secondary-types", default)]
    secondary_types: Vec<String>,
    #[serde(rename = "first-release-date", default)]
    first_release_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Medium {
    #[serde(default)]
    position: Option<u32>,
    #[serde(default)]
    track: Vec<Track>,
}

#[derive(Debug, Deserialize)]
struct Track {
    #[serde(default)]
    number: Option<String>,
    #[serde(default)]
    position: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(
        id: &str,
        title: &str,
        status: Option<&str>,
        primary: Option<&str>,
        secondary: &[&str],
        date: Option<&str>,
    ) -> Release {
        Release {
            id: id.to_string(),
            title: title.to_string(),
            date: date.map(str::to_string),
            status: status.map(str::to_string),
            release_group: Some(ReleaseGroup {
                id: format!("{id}-group"),
                primary_type: primary.map(str::to_string),
                secondary_types: secondary.iter().map(|s| (*s).to_string()).collect(),
                first_release_date: date.map(str::to_string),
            }),
            media: Vec::new(),
        }
    }

    fn credit(name: &str, join: &str) -> ArtistCredit {
        ArtistCredit {
            name: name.to_string(),
            joinphrase: join.to_string(),
            artist: Some(Artist {
                name: name.to_string(),
            }),
        }
    }

    // ── Choix de la parution : le cœur du module ────────────────────────

    #[test]
    fn prefere_lalbum_original_a_une_compilation() {
        // LE cas qui compte : sans cette règle, un morceau de Daft Punk se
        // retrouverait rangé sous « Now That's What I Call Music! ».
        let parutions = vec![
            release("comp", "Now That's What I Call Music 48", Some("Official"),
                    Some("Album"), &["Compilation"], Some("2001-11-01")),
            release("album", "Discovery", Some("Official"), Some("Album"), &[], Some("2001-03-12")),
        ];

        assert_eq!(pick_release(&parutions).unwrap().title, "Discovery");
    }

    #[test]
    fn prefere_une_parution_officielle() {
        let parutions = vec![
            release("boot", "Bootleg", None, Some("Album"), &[], Some("2000")),
            release("off", "Officielle", Some("Official"), Some("Album"), &[], Some("2005")),
        ];

        assert_eq!(pick_release(&parutions).unwrap().title, "Officielle");
    }

    #[test]
    fn prefere_un_album_a_un_single() {
        let parutions = vec![
            release("s", "Le single", Some("Official"), Some("Single"), &[], Some("2001")),
            release("a", "L'album", Some("Official"), Some("Album"), &[], Some("2001")),
        ];

        assert_eq!(pick_release(&parutions).unwrap().title, "L'album");
    }

    #[test]
    fn departage_deux_albums_par_lanciennete() {
        // Le remaster de 2015 contient le même enregistrement, mais ce n'est
        // pas la parution d'origine.
        let parutions = vec![
            release("re", "Discovery (Remastered)", Some("Official"), Some("Album"), &[], Some("2015")),
            release("or", "Discovery", Some("Official"), Some("Album"), &[], Some("2001")),
        ];

        assert_eq!(pick_release(&parutions).unwrap().title, "Discovery");
    }

    #[test]
    fn un_single_officiel_bat_un_album_non_officiel() {
        let parutions = vec![
            release("x", "Album pirate", None, Some("Album"), &[], Some("2001")),
            release("y", "Single officiel", Some("Official"), Some("Single"), &[], Some("2001")),
        ];

        assert_eq!(pick_release(&parutions).unwrap().title, "Single officiel");
    }

    #[test]
    fn une_liste_vide_ne_donne_aucune_parution() {
        assert!(pick_release(&[]).is_none());
    }

    // ── Dates ───────────────────────────────────────────────────────────

    #[test]
    fn extrait_lannee_de_tous_les_formats_de_date() {
        assert_eq!(parse_year("2001"), Some(2001));
        assert_eq!(parse_year("2001-03"), Some(2001));
        assert_eq!(parse_year("2001-03-12"), Some(2001));
    }

    #[test]
    fn rejette_les_dates_aberrantes() {
        assert_eq!(parse_year(""), None);
        assert_eq!(parse_year("abcd"), None);
        assert_eq!(parse_year("12"), None);
        assert_eq!(parse_year("0001"), None);
    }

    #[test]
    fn lannee_dorigine_prime_sur_celle_de_la_reedition() {
        let mut reedition = release("r", "Album", Some("Official"), Some("Album"), &[], None);
        reedition.date = Some("2015-06-01".to_string());
        reedition.release_group.as_mut().unwrap().first_release_date =
            Some("1998-01-16".to_string());

        assert_eq!(release_year(&reedition), Some(1998));
    }

    // ── Crédits d'artistes ──────────────────────────────────────────────

    #[test]
    fn separe_les_invites_du_titulaire() {
        let credits = vec![
            credit("Daft Punk", " feat. "),
            credit("Pharrell Williams", ""),
        ];

        let (principaux, invites) = split_artist_credit(&credits);
        assert_eq!(principaux, vec!["Daft Punk"]);
        assert_eq!(invites, vec!["Pharrell Williams"]);
    }

    #[test]
    fn conserve_les_co_titulaires_comme_principaux() {
        // « & » n'introduit pas un invité : les deux sont titulaires.
        let credits = vec![credit("Simon", " & "), credit("Garfunkel", "")];

        let (principaux, invites) = split_artist_credit(&credits);
        assert_eq!(principaux, vec!["Simon", "Garfunkel"]);
        assert!(invites.is_empty());
    }

    #[test]
    fn gere_plusieurs_invites() {
        let credits = vec![
            credit("Calvin Harris", " feat. "),
            credit("Dua Lipa", " & "),
            credit("Young Thug", ""),
        ];

        let (principaux, invites) = split_artist_credit(&credits);
        assert_eq!(principaux, vec!["Calvin Harris"]);
        assert_eq!(invites, vec!["Dua Lipa", "Young Thug"]);
    }

    #[test]
    fn ignore_les_credits_vides() {
        let credits = vec![credit("", ""), credit("Air", "")];

        let (principaux, _) = split_artist_credit(&credits);
        assert_eq!(principaux, vec!["Air"]);
    }

    // ── Numéro de piste ─────────────────────────────────────────────────

    #[test]
    fn lit_le_numero_de_piste_et_de_disque() {
        let mut parution = release("a", "Album", Some("Official"), Some("Album"), &[], Some("2001"));
        parution.media = vec![Medium {
            position: Some(2),
            track: vec![Track {
                number: Some("7".to_string()),
                position: Some(7),
            }],
        }];

        assert_eq!(track_position(&parution), Some((Some(7), Some(2))));
    }

    #[test]
    fn retombe_sur_la_position_quand_le_numero_nest_pas_numerique() {
        // Sur un vinyle, le numéro de piste vaut « A1 ».
        let mut parution = release("a", "Album", Some("Official"), Some("Album"), &[], Some("1975"));
        parution.media = vec![Medium {
            position: Some(1),
            track: vec![Track {
                number: Some("A1".to_string()),
                position: Some(1),
            }],
        }];

        assert_eq!(track_position(&parution), Some((Some(1), Some(1))));
    }

    #[test]
    fn une_parution_sans_support_ne_donne_pas_de_numero() {
        let parution = release("a", "Album", Some("Official"), Some("Album"), &[], Some("2001"));
        assert_eq!(track_position(&parution), None);
    }

    // ── Genre ───────────────────────────────────────────────────────────

    #[test]
    fn retient_le_genre_le_plus_attribue() {
        let genres = vec![
            Genre { name: "house".into(), count: 3 },
            Genre { name: "french house".into(), count: 12 },
            Genre { name: "electronic".into(), count: 7 },
        ];

        assert_eq!(best_genre(&genres).as_deref(), Some("french house"));
    }

    #[test]
    fn aucun_genre_ne_donne_rien() {
        assert!(best_genre(&[]).is_none());
    }

    // ── Assemblage ──────────────────────────────────────────────────────

    #[test]
    fn assemble_des_metadonnees_completes() {
        let mut album = release("rel-1", "Discovery", Some("Official"), Some("Album"), &[], Some("2001-03-12"));
        album.media = vec![Medium {
            position: Some(1),
            track: vec![Track {
                number: Some("3".to_string()),
                position: Some(3),
            }],
        }];

        let recording = Recording {
            id: "rec-1".to_string(),
            title: "Digital Love".to_string(),
            artist_credit: vec![credit("Daft Punk", "")],
            releases: vec![
                release("comp", "Compilation", Some("Official"), Some("Album"), &["Compilation"], Some("2003")),
                album,
            ],
            genres: vec![Genre { name: "french house".into(), count: 9 }],
            length: Some(301_000),
        };

        let meta = build_metadata(recording);

        assert_eq!(meta.title, "Digital Love");
        assert_eq!(meta.filing_artist(), Some("Daft Punk"));
        assert_eq!(meta.album.as_deref(), Some("Discovery"));
        assert_eq!(meta.year, Some(2001));
        assert_eq!(meta.track_no, Some(3));
        assert_eq!(meta.disc_no, Some(1));
        assert_eq!(meta.genre.as_deref(), Some("french house"));
        assert_eq!(meta.release_mbid.as_deref(), Some("rel-1"));
    }

    #[test]
    fn un_enregistrement_sans_parution_reste_exploitable() {
        // Le titre et l'artiste suffisent déjà à ranger correctement.
        let recording = Recording {
            id: "rec".to_string(),
            title: "Inédit".to_string(),
            artist_credit: vec![credit("Artiste", "")],
            releases: Vec::new(),
            genres: Vec::new(),
            length: None,
        };

        let meta = build_metadata(recording);

        assert_eq!(meta.title, "Inédit");
        assert_eq!(meta.filing_artist(), Some("Artiste"));
        assert!(meta.album.is_none());
        assert!(meta.year.is_none());
    }
}

#[derive(Debug, Deserialize)]
struct Genre {
    #[serde(default)]
    name: String,
    #[serde(default)]
    count: u32,
}
