//! Cover Art Archive : les pochettes en haute résolution.
//!
//! Service adossé à MusicBrainz et hébergé par l'Internet Archive. Les images
//! sont fournies par la communauté, souvent scannées à très haute définition.
//!
//! # Deux niveaux de recherche
//!
//! | Niveau | Ce qu'on obtient |
//! |---|---|
//! | **Parution** | La pochette exacte de cette édition |
//! | **Groupe de parution** | La pochette de référence de l'album, toutes éditions confondues |
//!
//! On tente la parution précise d'abord : une édition japonaise n'a pas la même
//! pochette que l'européenne. À défaut, la pochette de référence fait très bien
//! l'affaire — mieux vaut la bonne image d'un album voisin que pas d'image.

use std::time::Duration;

use crate::core::Result;

use super::http::Service;

const ENDPOINT: &str = "https://coverartarchive.org";

/// Cadence volontairement prudente : le service est hébergé gracieusement par
/// l'Internet Archive, et une pochette pèse bien plus lourd qu'une réponse JSON.
const MIN_INTERVAL: Duration = Duration::from_millis(1_000);

pub struct CoverArtClient {
    service: Service,
}

impl CoverArtClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: Service::new("Cover Art Archive", MIN_INTERVAL)?,
        })
    }

    /// Récupère la pochette avant, dans la meilleure résolution disponible.
    ///
    /// `None` est un résultat normal : une grande partie du catalogue n'a pas
    /// de pochette déposée.
    pub async fn fetch_front(
        &self,
        release_mbid: Option<&str>,
        release_group_mbid: Option<&str>,
    ) -> Result<Option<Vec<u8>>> {
        // L'édition précise d'abord.
        if let Some(mbid) = release_mbid.filter(|value| !value.is_empty()) {
            let url = format!("{ENDPOINT}/release/{mbid}/front");

            match self.service.get_bytes(&url).await {
                Ok(Some(bytes)) if looks_like_image(&bytes) => return Ok(Some(bytes)),
                Ok(_) => {}
                // Une pochette manquante ne doit jamais faire échouer une
                // identification par ailleurs réussie.
                Err(error) => tracing::debug!(%error, "pochette de parution indisponible"),
            }
        }

        // À défaut, la pochette de référence de l'album.
        if let Some(mbid) = release_group_mbid.filter(|value| !value.is_empty()) {
            let url = format!("{ENDPOINT}/release-group/{mbid}/front");

            match self.service.get_bytes(&url).await {
                Ok(Some(bytes)) if looks_like_image(&bytes) => return Ok(Some(bytes)),
                Ok(_) => {}
                Err(error) => tracing::debug!(%error, "pochette de groupe indisponible"),
            }
        }

        Ok(None)
    }
}

/// Vérifie l'entête du fichier reçu.
///
/// Le service peut renvoyer une page d'erreur avec un code 200. Écrire ça dans
/// le cache de pochettes produirait des vignettes cassées, difficiles à
/// diagnostiquer bien plus tard.
fn looks_like_image(bytes: &[u8]) -> bool {
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G'];
    const GIF: &[u8] = b"GIF8";

    bytes.starts_with(JPEG)
        || bytes.starts_with(PNG)
        || bytes.starts_with(GIF)
        // WebP : « RIFF » … « WEBP »
        || (bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_les_formats_dimage_courants() {
        assert!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]));
        assert!(looks_like_image(&[0x89, b'P', b'N', b'G', 0x0D]));
        assert!(looks_like_image(b"GIF89a....."));

        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPVP8 ");
        assert!(looks_like_image(&webp));
    }

    #[test]
    fn rejette_une_page_derreur_deguisee_en_image() {
        // Le cas réel : un service qui répond 200 avec du HTML. Sans ce
        // contrôle, on stockerait une vignette cassée sans jamais savoir
        // pourquoi.
        assert!(!looks_like_image(b"<!DOCTYPE html><html>404</html>"));
        assert!(!looks_like_image(b"{\"error\":\"not found\"}"));
    }

    #[test]
    fn rejette_une_reponse_vide_ou_tronquee() {
        assert!(!looks_like_image(&[]));
        assert!(!looks_like_image(&[0xFF]));
        assert!(!looks_like_image(b"RIFF"));
    }

    #[test]
    fn le_client_se_construit() {
        assert!(CoverArtClient::new().is_ok());
    }

    #[test]
    fn la_cadence_menage_un_service_beneficie() {
        assert!(MIN_INTERVAL >= Duration::from_millis(1_000));
    }
}
