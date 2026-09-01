//! Client Deezer : les pochettes que MusicBrainz n'a pas.
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
//! # Pourquoi Deezer plutôt qu'iTunes
//!
//! Testés côte à côte sur « Damso — Macarena » :
//!
//! | Service | Résultat |
//! |---|---|
//! | iTunes | « Polokus » de Damso — mauvais morceau |
//! | Deezer | « Θ. Macarena » de Damso — exact, pochette 1000 px |
//!
//! La recherche d'iTunes mélange les champs ; celle de Deezer les respecte.
//!
//! # Ce qui sort de la machine
//!
//! Un artiste et un titre. Pas de compte, pas de clé, pas d'identifiant
//! d'appareil — l'API de recherche de Deezer est publique et anonyme.

use std::time::Duration;

use serde::Deserialize;

use crate::core::Result;

use super::http::Service;

const SEARCH_ENDPOINT: &str = "https://api.deezer.com/search";

/// Cadence d'appel. Deezer tolère bien plus, mais rien ne presse.
pub const MIN_INTERVAL: Duration = Duration::from_millis(300);

/// Écart de durée au-delà duquel on refuse la correspondance.
///
/// Sans ce garde-fou, une reprise ou un live homonyme fournirait sa pochette au
/// morceau d'origine — une image fausse, c'est-à-dire pire que pas d'image.
const DURATION_TOLERANCE_MS: i64 = 15_000;

pub struct DeezerClient {
    service: Service,
}

impl DeezerClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: Service::new("Deezer", MIN_INTERVAL)?,
        })
    }

    /// Cherche la pochette d'un morceau, en haute résolution.
    ///
    /// Retourne les octets de l'image, ou `None` si rien de crédible.
    pub async fn cover(
        &self,
        artist: Option<&str>,
        title: &str,
        duration_ms: i64,
    ) -> Result<Option<Vec<u8>>> {
        let Some(url) = self.cover_url(artist, title, duration_ms).await? else {
            return Ok(None);
        };

        self.service.get_bytes(&url).await
    }

    /// L'adresse de la pochette, avant téléchargement.
    async fn cover_url(
        &self,
        artist: Option<&str>,
        title: &str,
        duration_ms: i64,
    ) -> Result<Option<String>> {
        let query = build_query(artist, title);
        if query.is_empty() {
            return Ok(None);
        }

        let url = format!("{SEARCH_ENDPOINT}?q={}&limit=5", encode(&query));

        let Some(response) = self.service.get_json::<SearchResponse>(&url).await? else {
            return Ok(None);
        };

        Ok(pick_cover(&response.data, duration_ms))
    }
}

/// Construit la requête, champ par champ.
///
/// La syntaxe `artist:"…" track:"…"` de Deezer évite qu'un nom d'artiste
/// contenant un mot du titre ne brouille l'appariement — c'est précisément là
/// que la recherche d'iTunes se trompait.
fn build_query(artist: Option<&str>, title: &str) -> String {
    let title = clean(title);
    if title.is_empty() {
        return String::new();
    }

    match artist.map(clean).filter(|name| !name.is_empty()) {
        Some(artist) => format!("artist:\"{artist}\" track:\"{title}\""),
        None => format!("track:\"{title}\""),
    }
}

/// Retire ce qui casserait la requête, garde tout le reste.
///
/// Les accents et la ponctuation ordinaire sont conservés : « Ipséité » ne se
/// trouve pas sous « Ipsit ».
fn clean(value: &str) -> String {
    value.replace(['"', '\\'], " ").trim().to_string()
}

/// Retient la meilleure pochette parmi les résultats.
///
/// La durée départage : Deezer classe par popularité, ce qui remonte volontiers
/// un remix célèbre devant l'original qu'on cherche.
fn pick_cover(hits: &[SearchHit], duration_ms: i64) -> Option<String> {
    hits.iter()
        .filter(|hit| {
            // Une durée inconnue des deux côtés ne disqualifie pas : c'est une
            // corroboration en moins, pas une contre-indication.
            duration_ms == 0
                || hit.duration == 0
                || (hit.duration * 1000 - duration_ms).abs() <= DURATION_TOLERANCE_MS
        })
        .filter_map(|hit| {
            hit.album
                .as_ref()
                .and_then(|album| album.best_cover())
                .map(|url| (hit, url))
        })
        .min_by_key(|(hit, _)| {
            if duration_ms == 0 || hit.duration == 0 {
                i64::MAX / 2
            } else {
                (hit.duration * 1000 - duration_ms).abs()
            }
        })
        .map(|(_, url)| url)
}

/// Encodage de composant d'URL.
fn encode(value: &str) -> String {
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

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    /// Durée en secondes.
    #[serde(default)]
    duration: i64,
    #[serde(default)]
    album: Option<Album>,
}

#[derive(Debug, Deserialize)]
struct Album {
    #[serde(default)]
    cover_xl: Option<String>,
    #[serde(default)]
    cover_big: Option<String>,
    #[serde(default)]
    cover_medium: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn album(url: &str) -> Album {
        Album {
            cover_xl: Some(url.to_string()),
            cover_big: None,
            cover_medium: None,
        }
    }

    fn hit(duration: i64, url: &str) -> SearchHit {
        SearchHit {
            duration,
            album: Some(album(url)),
        }
    }

    #[test]
    fn la_requete_separe_les_champs() {
        // C'est là que la recherche d'iTunes se trompait : elle mélangeait
        // artiste et titre, et rendait « Polokus » pour « Damso — Macarena ».
        assert_eq!(
            build_query(Some("Damso"), "Macarena"),
            "artist:\"Damso\" track:\"Macarena\""
        );
        assert_eq!(build_query(None, "Macarena"), "track:\"Macarena\"");
    }

    #[test]
    fn la_requete_conserve_les_accents() {
        // « Ipséité » ne se trouve pas sous « Ipsit ».
        assert!(build_query(Some("Népal"), "À l'extérieur").contains("Népal"));
        assert!(build_query(Some("X"), "Ipséité").contains("Ipséité"));
    }

    #[test]
    fn un_titre_vide_ne_produit_aucune_requete() {
        assert!(build_query(Some("Damso"), "   ").is_empty());
    }

    #[test]
    fn la_duree_ecarte_une_reprise() {
        // Deezer classe par popularité : un remix célèbre remonte volontiers
        // devant l'original. Une image fausse est pire que pas d'image.
        let hits = vec![hit(400, "remix.jpg")];
        assert_eq!(pick_cover(&hits, 200_000), None);
    }

    #[test]
    fn la_duree_la_plus_proche_gagne() {
        let hits = vec![hit(210, "loin.jpg"), hit(201, "proche.jpg")];
        assert_eq!(pick_cover(&hits, 200_000).as_deref(), Some("proche.jpg"));
    }

    #[test]
    fn une_duree_inconnue_ne_disqualifie_pas() {
        assert!(pick_cover(&[hit(0, "sans-duree.jpg")], 200_000).is_some());
        assert!(pick_cover(&[hit(200, "quelconque.jpg")], 0).is_some());
    }

    #[test]
    fn on_prend_toujours_la_plus_grande_taille() {
        let album = Album {
            cover_xl: Some("xl.jpg".into()),
            cover_big: Some("big.jpg".into()),
            cover_medium: Some("medium.jpg".into()),
        };
        assert_eq!(album.best_cover().as_deref(), Some("xl.jpg"));

        let sans_xl = Album {
            cover_xl: None,
            cover_big: Some("big.jpg".into()),
            cover_medium: Some("medium.jpg".into()),
        };
        assert_eq!(sans_xl.best_cover().as_deref(), Some("big.jpg"));
    }

    #[test]
    fn un_resultat_sans_pochette_est_ignore() {
        let hits = vec![SearchHit {
            duration: 200,
            album: None,
        }];
        assert_eq!(pick_cover(&hits, 200_000), None);
    }
}
