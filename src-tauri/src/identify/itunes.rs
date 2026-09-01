//! Client iTunes : le catalogue qui complète les autres.
//!
//! # Pourquoi ce service, alors qu'il s'était trompé
//!
//! iTunes avait été écarté pour les pochettes automatiques après une erreur
//! mesurée : interrogé sur « Damso — Macarena », il rendait « Polokus ». Sa
//! recherche mélange les champs, et une pochette fausse est pire qu'une
//! pochette absente.
//!
//! Ce reproche vaut pour une décision **prise sans témoin**. Il ne vaut plus
//! quand la liste est présentée à l'utilisateur : c'est lui qui reconnaît son
//! morceau, et une proposition de trop ne coûte qu'un regard. Mesuré sur
//! quatorze morceaux sans album, iTunes en trouve six — dont « Drapeau noir »,
//! que Deezer manquait.
//!
//! La règle qui en découle est écrite dans [`super::catalog`] : les fiches
//! d'iTunes entrent dans la liste comme les autres, mais l'acceptation
//! automatique exige que le titre **et** l'artiste concordent.
//!
//! # Ce qui sort de la machine
//!
//! Un artiste et un titre, vers une API publique et anonyme. Ni clé, ni compte,
//! ni identifiant d'appareil.

use std::time::Duration;

use serde::Deserialize;

use crate::core::Result;

use super::catalog::Candidate;
use super::http::Service;

const SEARCH_ENDPOINT: &str = "https://itunes.apple.com/search";

/// Cadence d'appel. Apple limite autour de vingt requêtes par minute ; on reste
/// largement en dessous.
pub const MIN_INTERVAL: Duration = Duration::from_millis(3_500);

pub struct ItunesClient {
    service: Service,
}

impl ItunesClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: Service::new("iTunes", MIN_INTERVAL)?,
        })
    }

    /// Cherche un morceau par son artiste et son titre.
    pub async fn search(&self, artist: Option<&str>, title: &str) -> Result<Vec<Candidate>> {
        let term = match artist.map(str::trim).filter(|name| !name.is_empty()) {
            Some(artist) => format!("{artist} {title}"),
            None => title.trim().to_string(),
        };

        if term.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!(
            "{SEARCH_ENDPOINT}?term={}&entity=song&limit=8",
            super::deezer::encode(&term)
        );

        let Some(response) = self.service.get_json::<SearchResponse>(&url).await? else {
            return Ok(Vec::new());
        };

        Ok(response.results.into_iter().map(Into::into).collect())
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SongResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SongResult {
    #[serde(default)]
    track_name: Option<String>,
    #[serde(default)]
    artist_name: Option<String>,
    #[serde(default)]
    collection_name: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    track_time_millis: Option<i64>,
    #[serde(default)]
    artwork_url100: Option<String>,
}

impl From<SongResult> for Candidate {
    fn from(result: SongResult) -> Self {
        Candidate {
            artist: result.artist_name,
            album: result.collection_name,
            year: result.release_date.as_deref().and_then(parse_year),
            duration_ms: result.track_time_millis.unwrap_or(0),
            cover_url: result.artwork_url100.as_deref().map(upgrade_artwork),
            preview_url: result.artwork_url100,
            ..Candidate::new("iTunes", result.track_name.unwrap_or_default())
        }
    }
}

/// L'année d'une date ISO 8601.
fn parse_year(date: &str) -> Option<u32> {
    date.get(..4).and_then(|year| year.parse().ok())
}

/// Demande la grande version de la pochette.
///
/// L'API ne rend qu'une vignette de 100 px, mais le service sert toutes les
/// tailles à la même adresse : il suffit de la réécrire. Une pochette de 100 px
/// affichée en 208 px serait floue — et l'utilisateur a demandé les meilleures
/// résolutions dès le premier jour.
fn upgrade_artwork(url: &str) -> String {
    url.replace("100x100bb", "1000x1000bb")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_pochette_est_demandee_en_grand() {
        assert_eq!(
            upgrade_artwork("https://is1.mzstatic.com/image/thumb/a.jpg/100x100bb.jpg"),
            "https://is1.mzstatic.com/image/thumb/a.jpg/1000x1000bb.jpg"
        );
    }

    #[test]
    fn une_adresse_inattendue_reste_intacte() {
        // Mieux vaut une vignette qu'une adresse cassée.
        assert_eq!(upgrade_artwork("https://exemple/a.jpg"), "https://exemple/a.jpg");
    }

    #[test]
    fn l_annee_vient_de_la_date_iso() {
        assert_eq!(parse_year("2017-04-28T07:00:00Z"), Some(2017));
        assert_eq!(parse_year("inconnu"), None);
        assert_eq!(parse_year(""), None);
    }

    #[test]
    fn une_fiche_sans_album_reste_exploitable() {
        // Un single n'a pas toujours de `collectionName` : la fiche vaut quand
        // même pour son titre et sa pochette.
        let candidate: Candidate = SongResult {
            track_name: Some("Macarena".into()),
            artist_name: Some("Damso".into()),
            collection_name: None,
            release_date: None,
            track_time_millis: Some(206_000),
            artwork_url100: None,
        }
        .into();

        assert_eq!(candidate.source, "iTunes");
        assert_eq!(candidate.title, "Macarena");
        assert!(candidate.album.is_none());
    }
}
