//! Client LRCLIB : les paroles qui manquent aux fichiers.
//!
//! # Pourquoi ce module existe
//!
//! Un relevé des 192 fichiers de la bibliothèque de test n'a trouvé **aucune**
//! frame `USLT` ni `SYLT` : pas un seul ne portait de paroles. Les afficher
//! supposait donc d'aller les chercher.
//!
//! # Pourquoi LRCLIB plutôt qu'un autre
//!
//! | Critère | LRCLIB |
//! |---|---|
//! | Clé d'API | Aucune |
//! | Paroles synchronisées | Oui, au format LRC |
//! | Compte utilisateur | Aucun |
//! | Ce qu'on envoie | Artiste, titre, album, durée — rien de personnel |
//!
//! # La frontière avec le premier pilier
//!
//! Onzer est un lecteur hors ligne, et le reste : cet appel enrichit, il ne
//! conditionne rien. Sans réseau, la musique s'écoute exactement pareil ; seules
//! les paroles manquent, et elles peuvent toujours être collées à la main.
//!
//! Les paroles récupérées sont **écrites dans le fichier**, pas seulement en
//! base : une fois trouvées, elles ne dépendent plus du réseau.

use std::time::Duration;

use serde::Deserialize;

use crate::core::Result;

use super::http::Service;

const ENDPOINT: &str = "https://lrclib.net/api/get";

/// Cadence d'appel.
///
/// LRCLIB ne publie pas de limite. Une requête par seconde est la politesse
/// minimale envers un service gratuit qu'on interroge en rafale — c'est la
/// même prudence que celle appliquée à MusicBrainz.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1_000);

/// Écart de durée toléré par le service, en secondes.
///
/// LRCLIB apparie sur la durée : au-delà de deux secondes d'écart, il considère
/// qu'il ne s'agit pas du même enregistrement et ne renvoie rien. On ne le
/// contredit pas — des paroles décalées sont pires que pas de paroles.
pub const DURATION_TOLERANCE_S: i64 = 2;

/// Ce qu'on demande.
#[derive(Debug, Clone)]
pub struct LyricsQuery {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: i64,
}

/// Ce qu'on obtient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedLyrics {
    /// Contenu brut, LRC horodaté quand le service en dispose.
    pub raw: String,
    pub synced: bool,
}

pub struct LrcLibClient {
    service: Service,
}

impl LrcLibClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: Service::new("LRCLIB", MIN_INTERVAL)?,
        })
    }

    /// Cherche les paroles d'un morceau.
    ///
    /// `None` signifie « pas dans la base », ce qui est courant et normal — et
    /// n'est pas une erreur.
    pub async fn fetch(&self, query: &LyricsQuery) -> Result<Option<FetchedLyrics>> {
        let url = build_url(query);

        let Some(response) = self.service.get_json::<GetResponse>(&url).await? else {
            return Ok(None);
        };

        Ok(pick(&response))
    }
}

/// Construit l'URL de requête.
///
/// Séparé de l'appel réseau pour être testable : l'encodage des noms d'artistes
/// est exactement le genre de détail qui casse en silence sur un accent.
fn build_url(query: &LyricsQuery) -> String {
    let mut url = format!(
        "{ENDPOINT}?track_name={}&artist_name={}&duration={}",
        encode(&query.title),
        encode(&query.artist),
        query.duration_ms / 1000,
    );

    // L'album affine l'appariement, mais un album faux ferait échouer une
    // recherche qui aurait abouti sans lui : on ne l'ajoute que s'il existe.
    if let Some(album) = query.album.as_deref().filter(|a| !a.trim().is_empty()) {
        url.push_str(&format!("&album_name={}", encode(album)));
    }

    url
}

/// Encodage de composant d'URL.
///
/// Écrit ici plutôt qu'ajouté en dépendance : la règle tient en dix lignes, et
/// une bibliothèque de plus pour trois caractères réservés ne se justifie pas.
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

/// Retient la meilleure forme disponible.
///
/// Les paroles synchronisées priment toujours : elles contiennent le texte
/// simple **et** la cadence. Un morceau instrumental est écarté — LRCLIB le
/// signale explicitement, et afficher un cadre vide serait pire que d'annoncer
/// l'absence.
fn pick(response: &GetResponse) -> Option<FetchedLyrics> {
    if response.instrumental {
        return None;
    }

    let synced = response
        .synced_lyrics
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());

    if let Some(raw) = synced {
        return Some(FetchedLyrics {
            raw: raw.to_string(),
            synced: true,
        });
    }

    let plain = response
        .plain_lyrics
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())?;

    Some(FetchedLyrics {
        raw: plain.to_string(),
        synced: false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetResponse {
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requete() -> LyricsQuery {
        LyricsQuery {
            title: "Macarena".to_string(),
            artist: "Damso".to_string(),
            album: Some("Ipséité".to_string()),
            duration_ms: 206_400,
        }
    }

    #[test]
    fn construit_une_url_complete() {
        let url = build_url(&requete());

        assert!(url.contains("track_name=Macarena"));
        assert!(url.contains("artist_name=Damso"));
        assert!(url.contains("duration=206"), "la durée est en secondes");
        assert!(url.contains("album_name="));
    }

    #[test]
    fn un_album_absent_nest_pas_envoye() {
        // Un champ vide ferait échouer un appariement qui aurait abouti sans.
        let mut sans_album = requete();
        sans_album.album = None;
        assert!(!build_url(&sans_album).contains("album_name"));

        sans_album.album = Some("   ".to_string());
        assert!(!build_url(&sans_album).contains("album_name"));
    }

    #[test]
    fn encode_les_accents_et_les_caracteres_reserves() {
        // Un accent non encodé casse la requête en silence.
        assert_eq!(encode("Ipséité"), "Ips%C3%A9it%C3%A9");
        assert_eq!(encode("Rock & Roll"), "Rock%20%26%20Roll");
        assert_eq!(encode("a+b=c"), "a%2Bb%3Dc");
        assert_eq!(encode("deja-vu_1.0~"), "deja-vu_1.0~");
    }

    #[test]
    fn prefere_les_paroles_synchronisees() {
        // Le LRC contient le texte ET la cadence : il n'y a aucune raison de
        // retenir la version simple quand les deux sont proposées.
        let reponse = GetResponse {
            plain_lyrics: Some("Première ligne".to_string()),
            synced_lyrics: Some("[00:12.34]Première ligne".to_string()),
            instrumental: false,
        };

        let retenu = pick(&reponse).unwrap();
        assert!(retenu.synced);
        assert!(retenu.raw.starts_with("[00:12.34]"));
    }

    #[test]
    fn se_rabat_sur_le_texte_simple() {
        let reponse = GetResponse {
            plain_lyrics: Some("Première ligne".to_string()),
            synced_lyrics: None,
            instrumental: false,
        };

        let retenu = pick(&reponse).unwrap();
        assert!(!retenu.synced);
        assert_eq!(retenu.raw, "Première ligne");
    }

    #[test]
    fn un_instrumental_ne_rend_rien() {
        // Afficher un cadre vide serait pire que d'annoncer l'absence.
        let reponse = GetResponse {
            plain_lyrics: Some(String::new()),
            synced_lyrics: None,
            instrumental: true,
        };

        assert!(pick(&reponse).is_none());
    }

    #[test]
    fn des_champs_vides_valent_une_absence() {
        let reponse = GetResponse {
            plain_lyrics: Some("   \n  ".to_string()),
            synced_lyrics: Some(String::new()),
            instrumental: false,
        };

        assert!(pick(&reponse).is_none());
    }

    #[test]
    fn la_cadence_reste_courtoise() {
        // Un service gratuit interrogé en rafale mérite au moins une seconde
        // entre deux requêtes.
        assert!(MIN_INTERVAL >= Duration::from_millis(1_000));
    }
}
