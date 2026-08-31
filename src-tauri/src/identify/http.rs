//! Client HTTP commun aux services d'identification.
//!
//! Concentre en un seul endroit tout ce qui protège à la fois **nous** et
//! **eux** : limitation de débit, réessais mesurés, délais d'attente, et
//! identification honnête de l'application.
//!
//! AcoustID et MusicBrainz sont des services gratuits maintenus par des
//! bénévoles. Les inonder de requêtes n'est pas seulement le meilleur moyen de
//! se faire bannir : c'est aussi leur coûter de l'argent.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::core::{OnzerError, Result};

use super::ratelimit::{self, Backoff, Decision, Outcome, RateLimiter};

/// Délai maximal d'une requête.
///
/// Généreux : les services d'identification sont parfois lents aux heures de
/// pointe, et l'identification se fait en tâche de fond — personne n'attend.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Identification de l'application.
///
/// MusicBrainz **exige** un en-tête `User-Agent` explicite mentionnant
/// l'application et un moyen de contact. Un client anonyme est rejeté, et à
/// juste titre : c'est ce qui leur permet de joindre l'auteur d'un client
/// défaillant plutôt que de bannir aveuglément une plage d'adresses.
const USER_AGENT: &str = concat!(
    "Onzer/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/Loogatoxx/Onzer )"
);

/// Taille maximale acceptée pour un téléchargement de pochette.
///
/// Certaines pochettes d'archive dépassent les 20 Mo. On refuse au-delà :
/// l'écart visuel est nul, le coût réseau ne l'est pas.
pub const MAX_DOWNLOAD_BYTES: u64 = 12 * 1024 * 1024;

/// Un service distant, avec sa propre cadence.
pub struct Service {
    name: &'static str,
    client: reqwest::Client,
    limiter: RateLimiter,
    backoff: Backoff,
}

impl Service {
    pub fn new(name: &'static str, min_interval: Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| OnzerError::Invalid(format!("client HTTP : {error}")))?;

        Ok(Self {
            name,
            client,
            limiter: RateLimiter::new(min_interval),
            backoff: Backoff::default(),
        })
    }

    /// Récupère et désérialise une réponse JSON.
    ///
    /// `None` signifie « le service a répondu que cette ressource n'existe
    /// pas » — un morceau inconnu de la base, ce qui est un résultat normal et
    /// non une erreur.
    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<Option<T>> {
        let Some(bytes) = self.get_bytes(url).await? else {
            return Ok(None);
        };

        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| OnzerError::Invalid(format!("{} : réponse illisible — {error}", self.name)))
    }

    /// Récupère le corps brut d'une réponse, avec réessais.
    pub async fn get_bytes(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let mut attempt = 0_u32;

        loop {
            attempt += 1;
            self.limiter.acquire().await;

            let (outcome, retry_after, body) = match self.client.get(url).send().await {
                Ok(response) => {
                    let status = response.status();

                    // 404 : la ressource n'existe pas. Ce n'est pas un échec,
                    // c'est une réponse — beaucoup de morceaux sont absents des
                    // bases publiques.
                    if status == reqwest::StatusCode::NOT_FOUND {
                        return Ok(None);
                    }

                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_secs);

                    if status.is_success() {
                        if let Some(length) = response.content_length() {
                            if length > MAX_DOWNLOAD_BYTES {
                                return Err(OnzerError::Invalid(format!(
                                    "{} : réponse de {length} octets, au-delà de la limite",
                                    self.name
                                )));
                            }
                        }

                        match response.bytes().await {
                            Ok(bytes) => return Ok(Some(bytes.to_vec())),
                            Err(_) => (Outcome::NetworkFailure, None, None),
                        }
                    } else {
                        (Outcome::Status(status.as_u16()), retry_after, Some(status))
                    }
                }
                Err(_) => (Outcome::NetworkFailure, None, None),
            };

            match ratelimit::decide(outcome, attempt, retry_after, &self.backoff, jitter_seed()) {
                Decision::Retry(delay) => {
                    tracing::debug!(
                        service = self.name,
                        attempt,
                        delay_ms = delay.as_millis() as u64,
                        "nouvelle tentative"
                    );
                    tokio::time::sleep(delay).await;
                }
                Decision::GiveUp => {
                    return Err(OnzerError::Invalid(match body {
                        Some(status) => format!("{} a répondu {status}", self.name),
                        None => format!("{} injoignable", self.name),
                    }));
                }
            }
        }
    }
}

/// Graine de gigue tirée de l'horloge.
fn jitter_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lidentification_de_lapplication_est_conforme() {
        // MusicBrainz rejette un client anonyme : le User-Agent doit nommer
        // l'application, sa version, et un moyen de contact.
        assert!(USER_AGENT.starts_with("Onzer/"));
        assert!(USER_AGENT.contains("http"), "un contact est exigé");
    }

    #[test]
    fn les_services_se_construisent() {
        assert!(Service::new("test", Duration::from_millis(500)).is_ok());
    }

    /// La limite doit encadrer une pochette de très haute résolution sans
    /// laisser passer une réponse aberrante.
    #[test]
    fn la_limite_de_telechargement_encadre_une_pochette_realiste() {
        let pochette_4000px_jpeg = 6 * 1024 * 1024_u64;
        let reponse_aberrante = 500 * 1024 * 1024_u64;

        assert!(pochette_4000px_jpeg < MAX_DOWNLOAD_BYTES);
        assert!(reponse_aberrante > MAX_DOWNLOAD_BYTES);
    }

    #[test]
    fn la_gigue_varie_dun_appel_a_lautre() {
        let echantillons: std::collections::HashSet<u64> =
            (0..20).map(|_| jitter_seed()).collect();

        assert!(echantillons.len() > 1);
    }
}
