//! Lecture d'une playlist Spotify publique.
//!
//! # Ce qu'Onzer fait, et ce qu'il ne fait pas
//!
//! Il **lit une liste de titres** et la compare à la bibliothèque. Il ne
//! télécharge rien, n'embarque aucun téléchargeur, et ne contourne aucune
//! protection. Le résultat est une liste de ce qui manque — un travail de
//! bibliothécaire, pas de graveur.
//!
//! # Pourquoi l'API officielle plutôt que la page web
//!
//! La page publique d'une playlist contient bien la liste, noyée dans un bloc
//! JSON de son application. La lire reviendrait à écrire un analyseur qui
//! casserait au premier changement de leur front-end, et à s'introduire là où
//! une porte documentée existe.
//!
//! L'API officielle demande des identifiants d'application — gratuits, créés en
//! deux minutes sur `developer.spotify.com`. C'est exactement le schéma déjà
//! retenu pour AcoustID : l'utilisateur fournit ses propres identifiants, Onzer
//! n'en distribue aucun.
//!
//! # Le flux « client credentials »
//!
//! Aucun compte utilisateur n'est nécessaire : on ne lit que des playlists
//! **publiques**, et ce flux ne donne accès à rien d'autre. Onzer ne voit ni ne
//! demande le mot de passe Spotify de quiconque.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{OnzerError, Result};

use super::http::Service;

const TOKEN_ENDPOINT: &str = "https://accounts.spotify.com/api/token";
const API: &str = "https://api.spotify.com/v1";

/// Cadence d'appel. Spotify tolère bien plus, mais rien ne presse.
const MIN_INTERVAL: Duration = Duration::from_millis(200);

/// Nombre de titres par page. 50 est le maximum accepté.
const PAGE_SIZE: usize = 50;

/// Garde-fou : au-delà, c'est une bibliothèque entière, pas une playlist.
const MAX_TRACKS: usize = 2_000;

/// Un titre tel que Spotify le décrit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyTrack {
    pub title: String,
    /// Artistes crédités, le principal en premier.
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    /// Lien du titre, utilisable tel quel par un téléchargeur externe.
    pub url: String,
}

impl SpotifyTrack {
    pub fn main_artist(&self) -> Option<&str> {
        self.artists.first().map(String::as_str)
    }
}

/// Une playlist lue.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyPlaylist {
    pub name: String,
    pub tracks: Vec<SpotifyTrack>,
}

pub struct SpotifyClient {
    service: Service,
    client_id: String,
    client_secret: String,
}

impl SpotifyClient {
    pub fn new(client_id: String, client_secret: String) -> Result<Self> {
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            return Err(OnzerError::Invalid(
                "identifiants Spotify absents — à renseigner dans les réglages".to_string(),
            ));
        }

        Ok(Self {
            service: Service::new("Spotify", MIN_INTERVAL)?,
            client_id: client_id.trim().to_string(),
            client_secret: client_secret.trim().to_string(),
        })
    }

    /// Lit une playlist publique, page par page.
    pub async fn playlist(&self, url_or_id: &str) -> Result<SpotifyPlaylist> {
        let Some(id) = playlist_id(url_or_id) else {
            return Err(OnzerError::Invalid(
                "lien de playlist non reconnu — colle l'adresse complète, du genre \
                 https://open.spotify.com/playlist/…"
                    .to_string(),
            ));
        };

        let token = self.token().await?;

        let details: PlaylistDetails = self
            .service
            .get_json_authed(&format!("{API}/playlists/{id}?fields=name"), &token)
            .await?
            .ok_or_else(|| {
                OnzerError::Invalid(
                    "playlist introuvable — est-elle bien publique ?".to_string(),
                )
            })?;

        let mut tracks = Vec::new();
        let mut offset = 0;

        loop {
            let url = format!(
                "{API}/playlists/{id}/tracks?limit={PAGE_SIZE}&offset={offset}\
                 &fields=items(track(name,duration_ms,artists(name),album(name),external_urls(spotify)))"
            );

            let Some(page) = self.service.get_json_authed::<TrackPage>(&url, &token).await? else {
                break;
            };

            let received = page.items.len();
            tracks.extend(page.items.into_iter().filter_map(|item| item.into_track()));

            // Une page incomplète est la dernière : Spotify n'a plus rien à
            // donner, et redemander ferait une requête pour rien.
            if received < PAGE_SIZE || tracks.len() >= MAX_TRACKS {
                break;
            }

            offset += PAGE_SIZE;
        }

        Ok(SpotifyPlaylist {
            name: details.name,
            tracks,
        })
    }

    /// Obtient un jeton d'accès par le flux « client credentials ».
    ///
    /// Redemandé à chaque comparaison plutôt que mis en cache : une lecture de
    /// playlist est un geste occasionnel, et un jeton conservé en mémoire est
    /// un secret de plus à ne pas laisser fuir dans un journal.
    async fn token(&self) -> Result<String> {
        self.service.throttle().await;

        let response = self
            .service
            .client()
            .post(TOKEN_ENDPOINT)
            .basic_auth(&self.client_id, Some(&self.client_secret))
            // Corps écrit à la main plutôt que via `form` : cela évite
            // d'activer une fonctionnalité de reqwest pour une seule requête,
            // dont le corps ne comporte qu'un champ sans caractère à échapper.
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body("grant_type=client_credentials")
            .send()
            .await
            .map_err(|error| OnzerError::Invalid(format!("Spotify injoignable : {error}")))?;

        if !response.status().is_success() {
            let status = response.status();
            // Le corps porte le vrai message — « invalid_client » notamment,
            // qui dit tout de suite qu'un identifiant est mal collé.
            let body = response.text().await.unwrap_or_default();
            return Err(OnzerError::Invalid(format!(
                "Spotify a refusé les identifiants ({status}) : {}",
                summarize(&body)
            )));
        }

        let token: TokenResponse = response
            .json()
            .await
            .map_err(|error| OnzerError::Invalid(format!("Spotify : réponse illisible — {error}")))?;

        Ok(token.access_token)
    }
}

/// Extrait l'identifiant d'une playlist depuis ce que l'utilisateur a collé.
///
/// Accepte l'adresse web, l'URI `spotify:playlist:…`, ou l'identifiant nu.
/// Les paramètres de suivi (`?si=…`) sont écartés — ils sont systématiquement
/// présents dans un lien copié depuis l'application.
pub fn playlist_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = if let Some(rest) = trimmed.strip_prefix("spotify:playlist:") {
        rest
    } else if let Some(position) = trimmed.find("/playlist/") {
        &trimmed[position + "/playlist/".len()..]
    } else {
        trimmed
    };

    // On s'arrête au premier séparateur : requête, ancre, ou segment suivant.
    let id: String = candidate
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();

    // Un identifiant Spotify fait 22 caractères en base 62. On reste tolérant
    // sur la longueur exacte, mais on refuse ce qui n'y ressemble pas du tout.
    (id.len() >= 16).then_some(id)
}

/// Réduit un corps d'erreur à sa phrase utile.
fn summarize(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error_description")
                .or_else(|| value.get("error"))
                .and_then(|error| error.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.chars().take(160).collect())
}

// ── Réponses de l'API ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct PlaylistDetails {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct TrackPage {
    #[serde(default)]
    items: Vec<PlaylistItem>,
}

#[derive(Debug, Deserialize)]
struct PlaylistItem {
    #[serde(default)]
    track: Option<TrackObject>,
}

impl PlaylistItem {
    fn into_track(self) -> Option<SpotifyTrack> {
        let track = self.track?;
        if track.name.trim().is_empty() {
            return None;
        }

        Some(SpotifyTrack {
            title: track.name,
            artists: track
                .artists
                .into_iter()
                .map(|artist| artist.name)
                .filter(|name| !name.trim().is_empty())
                .collect(),
            album: track.album.map(|album| album.name),
            duration_ms: track.duration_ms,
            url: track.external_urls.and_then(|urls| urls.spotify).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct TrackObject {
    #[serde(default)]
    name: String,
    #[serde(default)]
    duration_ms: i64,
    #[serde(default)]
    artists: Vec<NamedObject>,
    #[serde(default)]
    album: Option<NamedObject>,
    #[serde(default)]
    external_urls: Option<ExternalUrls>,
}

#[derive(Debug, Deserialize)]
struct NamedObject {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ExternalUrls {
    #[serde(default)]
    spotify: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_une_adresse_copiee_depuis_lapplication() {
        // C'est la forme qu'on obtient par « Copier le lien » : elle traîne
        // toujours un paramètre de suivi.
        assert_eq!(
            playlist_id("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc123"),
            Some("37i9dQZF1DXcBWIGoYBM5M".to_string())
        );
    }

    #[test]
    fn reconnait_les_autres_formes() {
        for entree in [
            "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
            "37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/intl-fr/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "  https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M/  ",
        ] {
            assert_eq!(
                playlist_id(entree),
                Some("37i9dQZF1DXcBWIGoYBM5M".to_string()),
                "forme non reconnue : {entree}"
            );
        }
    }

    #[test]
    fn refuse_ce_qui_nest_pas_une_playlist() {
        // Un lien d'album ou d'artiste ne doit pas être pris pour une playlist :
        // la requête échouerait plus loin, avec un message incompréhensible.
        assert_eq!(playlist_id(""), None);
        assert_eq!(playlist_id("   "), None);
        assert_eq!(playlist_id("bonjour"), None);
        assert_eq!(
            playlist_id("https://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3"),
            None,
            "un album n'est pas une playlist"
        );
    }

    #[test]
    fn extrait_la_phrase_utile_dune_erreur() {
        // « invalid_client » dit tout de suite qu'un identifiant est mal collé ;
        // le JSON brut, non.
        assert_eq!(
            summarize(r#"{"error":"invalid_client","error_description":"Invalid client"}"#),
            "Invalid client"
        );
        assert_eq!(summarize(r#"{"error":"invalid_grant"}"#), "invalid_grant");
        assert_eq!(summarize("panne"), "panne");
    }
}
