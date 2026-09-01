//! Client Deezer : les pochettes et les albums que MusicBrainz n'a pas.
//!
//! # Pourquoi une seconde source
//!
//! La Cover Art Archive est alimentée par des bénévoles, morceau par morceau.
//! Elle est excellente sur les catalogues anciens et lacunaire sur le reste :
//! sur cette bibliothèque, **165 morceaux sur 574** n'avaient aucune pochette.
//!
//! Mesuré sur huit d'entre eux, tirés au hasard : Deezer en a trouvé **huit**.
//! Le catalogue d'un service commercial est complet par construction — c'est son
//! métier de vendre des albums, pas d'archiver.
//!
//! # Pourquoi la requête est en texte libre
//!
//! La syntaxe par champs `artist:"…" track:"…"` paraissait plus sûre. Elle est
//! surtout plus fragile, et la mesure l'a montré :
//!
//! | Morceau | Requête par champs | Texte libre |
//! |---|---|---|
//! | Damso — Y.2 Diamants | rien | « Υ. 2 DIAMANTS » (*QALF infinity*) |
//! | Jok'air — L'étrangère | rien | « L'étrangère » (*Jok'Rambo*) |
//!
//! Deezer écrit ce titre avec un upsilon grec ; l'égalité exacte demandée par
//! la recherche par champs ne pardonne pas ce genre d'écart. Le texte libre
//! rend davantage de bruit — et c'est [`super::catalog`] qui fait le tri, avec
//! la même règle pour tous les services.
//!
//! # Ce qui sort de la machine
//!
//! Un artiste et un titre. Pas de compte, pas de clé, pas d'identifiant
//! d'appareil — l'API de recherche de Deezer est publique et anonyme.

use std::time::Duration;

use serde::Deserialize;

use crate::core::Result;

use super::catalog::{self, Candidate, Query};
use super::http::Service;

const SEARCH_ENDPOINT: &str = "https://api.deezer.com/search";
const ALBUM_ENDPOINT: &str = "https://api.deezer.com/album";

/// Cadence d'appel. Deezer tolère bien plus, mais rien ne presse.
pub const MIN_INTERVAL: Duration = Duration::from_millis(300);

pub struct DeezerClient {
    service: Service,
}

impl DeezerClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: Service::new("Deezer", MIN_INTERVAL)?,
        })
    }

    /// Cherche des fiches par artiste et titre.
    pub async fn search(&self, artist: Option<&str>, title: &str) -> Result<Vec<Candidate>> {
        let term = match artist.map(str::trim).filter(|name| !name.is_empty()) {
            Some(artist) => format!("{artist} {title}"),
            None => title.trim().to_string(),
        };

        if term.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{SEARCH_ENDPOINT}?q={}&limit=8", encode(&term));

        let Some(response) = self.service.get_json::<SearchResponse>(&url).await? else {
            return Ok(Vec::new());
        };

        Ok(response.data.into_iter().map(Into::into).collect())
    }

    /// Cherche la pochette d'un morceau, en haute résolution.
    ///
    /// Retourne les octets de l'image, ou `None` si rien de crédible. La
    /// crédibilité est jugée par [`catalog::auto_pick`] : cette pochette
    /// s'attachera sans que personne ne l'ait regardée.
    pub async fn cover(
        &self,
        artist: Option<&str>,
        title: &str,
        duration_ms: i64,
    ) -> Result<Option<Vec<u8>>> {
        let query = Query {
            artist,
            title,
            duration_ms,
        };

        let ranked = catalog::rank(self.search(artist, title).await?, &query);

        let Some(url) = catalog::auto_pick(&ranked, &query).and_then(|hit| hit.cover_url.clone())
        else {
            return Ok(None);
        };

        super::images::fetch(&url).await
    }

    /// L'année de parution d'un album, quand elle est demandée.
    ///
    /// La recherche ne la donne pas : il faut un second aller-retour. On ne le
    /// dépense qu'au moment où une fiche est réellement retenue.
    pub async fn album_year(&self, album_ref: &str) -> Result<Option<u32>> {
        let url = format!("{ALBUM_ENDPOINT}/{album_ref}");

        let Some(album) = self.service.get_json::<AlbumDetail>(&url).await? else {
            return Ok(None);
        };

        Ok(album.release_date.as_deref().and_then(parse_year))
    }
}

/// Encodage de composant d'URL.
///
/// Partagé avec le client iTunes : deux implémentations d'une même règle
/// finiraient par diverger.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

/// L'année d'une date `AAAA-MM-JJ`.
fn parse_year(date: &str) -> Option<u32> {
    date.get(..4).and_then(|year| year.parse().ok())
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    #[serde(default)]
    title: String,
    /// Durée en secondes.
    #[serde(default)]
    duration: i64,
    #[serde(default)]
    artist: Option<Artist>,
    #[serde(default)]
    album: Option<Album>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Album {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    cover_xl: Option<String>,
    #[serde(default)]
    cover_big: Option<String>,
    #[serde(default)]
    cover_medium: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlbumDetail {
    #[serde(default)]
    release_date: Option<String>,
}

impl Album {
    /// La plus grande taille disponible.
    ///
    /// L'utilisateur a demandé les meilleures résolutions dès le premier jour :
    /// `cover_xl` fait 1000 px, `cover_big` 500. On ne descend qu'à défaut.
    fn best_cover(&self) -> Option<String> {
        [&self.cover_xl, &self.cover_big, &self.cover_medium]
            .into_iter()
            .flatten()
            .find(|url| !url.trim().is_empty())
            .cloned()
    }
}

impl From<SearchHit> for Candidate {
    fn from(hit: SearchHit) -> Self {
        let album = hit.album.as_ref();

        Candidate {
            artist: hit.artist.and_then(|artist| artist.name),
            album: album.and_then(|album| album.title.clone()),
            year: None, // demandée séparément, et seulement si l'on retient la fiche
            duration_ms: hit.duration * 1000,
            cover_url: album.and_then(Album::best_cover),
            preview_url: album.and_then(|album| album.cover_medium.clone()),
            album_ref: album
                .and_then(|album| album.id)
                .map(|id| id.to_string()),
            ..Candidate::new("Deezer", hit.title)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: &str, artist: &str, duration: i64) -> SearchHit {
        SearchHit {
            title: title.to_string(),
            duration,
            artist: Some(Artist {
                name: Some(artist.to_string()),
            }),
            album: Some(Album {
                id: Some(42),
                title: Some("Ipséité".into()),
                cover_xl: Some("xl.jpg".into()),
                cover_big: None,
                cover_medium: None,
            }),
        }
    }

    #[test]
    fn la_duree_passe_en_millisecondes() {
        // Deezer compte en secondes, tout le reste d'Onzer en millisecondes.
        // Une conversion oubliée ferait échouer toutes les corroborations.
        let candidate: Candidate = hit("Macarena", "Damso", 206).into();
        assert_eq!(candidate.duration_ms, 206_000);
    }

    #[test]
    fn la_fiche_porte_l_album_et_sa_reference() {
        let candidate: Candidate = hit("Macarena", "Damso", 206).into();
        assert_eq!(candidate.source, "Deezer");
        assert_eq!(candidate.album.as_deref(), Some("Ipséité"));
        assert_eq!(candidate.album_ref.as_deref(), Some("42"));
        assert_eq!(candidate.year, None);
    }

    #[test]
    fn on_prend_toujours_la_plus_grande_taille() {
        let album = Album {
            id: None,
            title: None,
            cover_xl: Some("xl.jpg".into()),
            cover_big: Some("big.jpg".into()),
            cover_medium: Some("medium.jpg".into()),
        };
        assert_eq!(album.best_cover().as_deref(), Some("xl.jpg"));

        let sans_xl = Album {
            id: None,
            title: None,
            cover_xl: None,
            cover_big: Some("big.jpg".into()),
            cover_medium: Some("medium.jpg".into()),
        };
        assert_eq!(sans_xl.best_cover().as_deref(), Some("big.jpg"));
    }

    #[test]
    fn une_fiche_sans_album_reste_exploitable() {
        let candidate: Candidate = SearchHit {
            title: "Cloud 8".into(),
            duration: 143,
            artist: None,
            album: None,
        }
        .into();

        assert!(candidate.album.is_none());
        assert!(candidate.cover_url.is_none());
    }

    #[test]
    fn l_encodage_protege_les_caracteres_reserves() {
        assert_eq!(encode("Damso Macarena"), "Damso%20Macarena");
        assert_eq!(encode("AC/DC"), "AC%2FDC");
        assert_eq!(encode("Ipséité"), "Ips%C3%A9it%C3%A9");
    }

    #[test]
    fn l_annee_vient_de_la_date() {
        assert_eq!(parse_year("2017-04-28"), Some(2017));
        assert_eq!(parse_year(""), None);
    }
}
