//! Le côté qui se connecte.
//!
//! Rien de plus qu'un aller-retour : on envoie ce qu'on a, on reçoit l'union,
//! on l'applique. La décision, elle, est prise deux fois — une fois de chaque
//! côté, par le même code.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::core::{OnzerError, Result};

use super::etat;
use super::fusion::{fusionner, Changement, EtatSync};

/// Ce qu'une synchronisation a fait, en clair.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RapportSync {
    /// Le nom de l'appareil d'en face.
    pub appareil: String,
    /// Favoris repris de l'autre côté.
    pub favoris: usize,
    /// Morceaux qui ont gagné des paroles.
    pub paroles: usize,
    /// Playlists créées ou complétées.
    pub playlists: usize,
    /// Décisions où les deux côtés se contredisaient.
    pub arbitrages: usize,
}

impl RapportSync {
    fn compter(appareil: &str, changements: &[Changement], arbitrages: usize) -> Self {
        let mut rapport = Self {
            appareil: appareil.to_string(),
            arbitrages,
            ..Self::default()
        };

        for changement in changements {
            match changement {
                Changement::Aime { .. } => rapport.favoris += 1,
                Changement::Paroles { .. } => rapport.paroles += 1,
                Changement::Playlist { .. } => rapport.playlists += 1,
            }
        }

        rapport
    }
}

/// Se connecte à l'autre appareil et fusionne.
///
/// # Pourquoi un délai généreux
///
/// Six mégaoctets de paroles montent, six redescendent, et l'autre côté fait
/// une transaction SQLite entre les deux. Sur un Wi-Fi domestique c'est
/// l'affaire de quelques secondes ; sur un réseau encombré, trente ne sont pas
/// de trop. Le défaut de `reqwest` — trente secondes — suffit, mais il vaut
/// mieux l'écrire que le supposer.
pub async fn synchroniser(
    pool: &SqlitePool,
    hote: &str,
    port: u16,
    code: &str,
) -> Result<RapportSync> {
    let local = etat::lire(pool).await?;

    let reponse = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|erreur| OnzerError::Invalid(format!("client HTTP : {erreur}")))?
        .post(format!("http://{hote}:{port}/sync/v1/fusion"))
        .bearer_auth(code)
        .json(&local)
        .send()
        .await
        .map_err(|erreur| {
            OnzerError::Invalid(format!(
                "impossible de joindre {hote} : {erreur}. \
                 Les deux appareils sont-ils sur le même Wi-Fi ?"
            ))
        })?;

    if reponse.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(OnzerError::Invalid(
            "code refusé. Vérifie les huit chiffres affichés sur l'autre appareil.".to_string(),
        ));
    }

    if !reponse.status().is_success() {
        let code_http = reponse.status();
        let detail = reponse.text().await.unwrap_or_default();
        return Err(OnzerError::Invalid(format!(
            "l'autre appareil a répondu {code_http} : {detail}"
        )));
    }

    let distant: EtatSync = reponse
        .json()
        .await
        .map_err(|erreur| OnzerError::Invalid(format!("réponse illisible : {erreur}")))?;

    let resultat = fusionner(&local, &distant);

    etat::appliquer(
        pool,
        &distant.appareil,
        &resultat.changements,
        &resultat.arbitrages,
    )
    .await?;

    Ok(RapportSync::compter(
        &distant.appareil,
        &resultat.changements,
        resultat.arbitrages.len(),
    ))
}

/// Découpe `onzer://appairage?hote=…&port=…&code=…`.
///
/// # Pourquoi un lien et pas seulement trois champs
///
/// Le QR doit tenir dans un seul texte, et ce texte doit pouvoir être ouvert
/// par l'appareil photo du téléphone — celui qu'on a déjà, qui sait lire un QR
/// et qui n'a besoin d'aucune permission qu'Onzer devrait demander.
pub fn lire_lien(lien: &str) -> Option<(String, u16, String)> {
    let question = lien.split_once('?')?.1;

    let mut hote = None;
    let mut port = None;
    let mut code = None;

    for paire in question.split('&') {
        match paire.split_once('=') {
            Some(("hote", valeur)) => hote = Some(valeur.to_string()),
            Some(("port", valeur)) => port = valeur.parse::<u16>().ok(),
            Some(("code", valeur)) => code = Some(valeur.to_string()),
            _ => {}
        }
    }

    Some((hote?, port?, code?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_lien_complet_se_lit() {
        let lu = lire_lien("onzer://appairage?hote=192.168.1.42&port=47812&code=12345678");
        assert_eq!(
            lu,
            Some(("192.168.1.42".to_string(), 47812, "12345678".to_string()))
        );
    }

    #[test]
    fn un_lien_incomplet_ne_se_lit_pas() {
        // Mieux vaut ne rien proposer qu'une adresse à moitié devinée.
        assert!(lire_lien("onzer://appairage?hote=192.168.1.42").is_none());
        assert!(lire_lien("onzer://appairage").is_none());
        assert!(lire_lien("bonjour").is_none());
    }

    #[test]
    fn un_port_absurde_ne_se_lit_pas() {
        assert!(lire_lien("onzer://appairage?hote=a&port=999999&code=1").is_none());
    }
}
