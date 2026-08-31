//! Client AcoustID : de l'empreinte à l'identifiant du morceau.
//!
//! AcoustID détient un index géant d'empreintes acoustiques associées à des
//! identifiants MusicBrainz. On lui envoie une empreinte et une durée, il
//! répond par une liste de correspondances classées par confiance.
//!
//! # La clé d'API
//!
//! Le service est gratuit mais exige une clé, obtenue en une minute sur
//! <https://acoustid.org/new-application>. Elle sert à identifier les clients
//! et à couper ceux qui abusent — c'est ce qui permet au service de rester
//! gratuit pour tout le monde.
//!
//! Onzer ne peut pas en fournir une : une clé partagée entre tous les
//! utilisateurs serait bannie dès qu'un seul d'entre eux importerait une grosse
//! bibliothèque. L'utilisateur saisit donc la sienne.

use std::time::Duration;

use serde::Deserialize;

use crate::core::{OnzerError, Result};

use super::fingerprint::Fingerprint;
use super::http::Service;
use super::ratelimit::ACOUSTID_MIN_INTERVAL;

const ENDPOINT: &str = "https://api.acoustid.org/v2/lookup";

/// Confiance minimale pour accepter une correspondance.
///
/// AcoustID note de 0 à 1. En dessous de 0,5, la correspondance relève souvent
/// d'un extrait commun — un même silence d'intro, un même échantillon.
/// **Mieux vaut ne rien écrire que d'écrire de faux tags** : un fichier mal
/// identifié est plus pénible qu'un fichier non identifié.
const MIN_SCORE: f64 = 0.5;

/// Correspondance retenue.
#[derive(Debug, Clone)]
pub struct Identification {
    /// Identifiant MusicBrainz de l'**enregistrement** — pas de l'œuvre.
    ///
    /// C'est cette précision qui fait la valeur de l'empreinte : elle distingue
    /// une version album de sa version radio, un live d'un studio.
    pub recording_mbid: String,
    pub score: f64,
}

pub struct AcoustIdClient {
    service: Service,
    api_key: String,
}

impl AcoustIdClient {
    pub fn new(api_key: String) -> Result<Self> {
        if api_key.trim().is_empty() {
            return Err(OnzerError::Invalid(
                "clé AcoustID absente — à renseigner dans les réglages".to_string(),
            ));
        }

        Ok(Self {
            service: Service::new("AcoustID", ACOUSTID_MIN_INTERVAL)?,
            api_key: api_key.trim().to_string(),
        })
    }

    /// Identifie un morceau à partir de son empreinte.
    ///
    /// `None` signifie « inconnu de l'index », ce qui est fréquent et normal —
    /// notamment pour les productions confidentielles ou très récentes.
    pub async fn lookup(&self, fingerprint: &Fingerprint) -> Result<Option<Identification>> {
        // `meta=recordingids` demande le strict nécessaire. Les métadonnées
        // riches viendront de MusicBrainz, dont c'est le métier.
        let url = format!(
            "{ENDPOINT}?client={}&format=json&meta=recordingids&duration={}&fingerprint={}",
            self.api_key, fingerprint.duration_seconds, fingerprint.encoded
        );

        let Some(response) = self.service.get_json::<LookupResponse>(&url).await? else {
            return Ok(None);
        };

        if response.status != "ok" {
            return Err(OnzerError::Invalid(format!(
                "AcoustID : {}",
                response.error.map(|e| e.message).unwrap_or_default()
            )));
        }

        Ok(best_match(&response.results))
    }
}

/// Retient la meilleure correspondance exploitable.
///
/// Séparé de l'appel réseau pour être testable sur des réponses fabriquées.
fn best_match(results: &[LookupResult]) -> Option<Identification> {
    results
        .iter()
        .filter(|result| result.score >= MIN_SCORE)
        .filter_map(|result| {
            result
                .recordings
                .as_ref()?
                .iter()
                .find(|recording| !recording.id.is_empty())
                .map(|recording| Identification {
                    recording_mbid: recording.id.clone(),
                    score: result.score,
                })
        })
        // Les résultats sont censés arriver triés, mais rien ne l'impose :
        // on ne fait pas confiance à un ordre non contractuel.
        .max_by(|a, b| a.score.total_cmp(&b.score))
}

/// Cadence d'appel, exposée pour la documentation et les tests.
pub const MIN_INTERVAL: Duration = ACOUSTID_MIN_INTERVAL;

// ── Réponse de l'API ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LookupResponse {
    status: String,
    #[serde(default)]
    results: Vec<LookupResult>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct LookupResult {
    score: f64,
    #[serde(default)]
    recordings: Option<Vec<RecordingRef>>,
}

#[derive(Debug, Deserialize)]
struct RecordingRef {
    #[serde(default)]
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(score: f64, ids: &[&str]) -> LookupResult {
        LookupResult {
            score,
            recordings: Some(
                ids.iter()
                    .map(|id| RecordingRef {
                        id: (*id).to_string(),
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn retient_la_correspondance_la_mieux_notee() {
        let resultats = vec![
            result(0.62, &["faible"]),
            result(0.97, &["forte"]),
            result(0.71, &["moyenne"]),
        ];

        let retenu = best_match(&resultats).unwrap();
        assert_eq!(retenu.recording_mbid, "forte");
        assert!((retenu.score - 0.97).abs() < 1e-9);
    }

    #[test]
    fn ne_fait_pas_confiance_a_lordre_de_la_reponse() {
        // Rien dans le contrat n'impose que les résultats arrivent triés.
        let resultats = vec![result(0.55, &["premier"]), result(0.99, &["meilleur"])];

        assert_eq!(best_match(&resultats).unwrap().recording_mbid, "meilleur");
    }

    #[test]
    fn ecarte_les_correspondances_peu_sures() {
        // Mieux vaut un fichier non identifié qu'un fichier mal identifié :
        // de faux tags écrits dans le fichier sont bien plus pénibles à
        // rattraper qu'une absence de tags.
        let resultats = vec![result(0.49, &["douteuse"]), result(0.2, &["hasardeuse"])];

        assert!(best_match(&resultats).is_none());
    }

    #[test]
    fn le_seuil_est_inclusif() {
        assert!(best_match(&[result(MIN_SCORE, &["limite"])]).is_some());
    }

    #[test]
    fn ignore_les_resultats_sans_enregistrement() {
        // Une correspondance d'empreinte sans identifiant MusicBrainz est
        // inexploitable : il n'y a rien à aller chercher derrière.
        let resultats = vec![
            LookupResult {
                score: 0.99,
                recordings: None,
            },
            result(0.6, &["utilisable"]),
        ];

        assert_eq!(best_match(&resultats).unwrap().recording_mbid, "utilisable");
    }

    #[test]
    fn ignore_les_identifiants_vides() {
        let resultats = vec![result(0.99, &["", "valide"])];
        assert_eq!(best_match(&resultats).unwrap().recording_mbid, "valide");
    }

    #[test]
    fn une_reponse_vide_ne_donne_aucune_correspondance() {
        assert!(best_match(&[]).is_none());
    }

    #[test]
    fn une_cle_vide_est_refusee_des_la_construction() {
        // Échouer tout de suite, plutôt que d'envoyer une requête vouée à un
        // 401 et de consommer du quota pour rien.
        assert!(AcoustIdClient::new(String::new()).is_err());
        assert!(AcoustIdClient::new("   ".to_string()).is_err());
        assert!(AcoustIdClient::new("cle-valide".to_string()).is_ok());
    }

    #[test]
    fn la_cadence_reste_sous_la_limite_publiee() {
        // AcoustID tolère trois requêtes par seconde.
        assert!(MIN_INTERVAL >= Duration::from_millis(334));
    }
}
