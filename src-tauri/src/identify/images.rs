//! Téléchargement des images, quel que soit le service qui les sert.
//!
//! # Pourquoi un seul chemin pour toutes les pochettes
//!
//! Chaque catalogue a sa cadence : MusicBrainz est fragile et demande une
//! seconde entre deux requêtes, iTunes plafonne autour de vingt appels par
//! minute. Ces limites protègent leurs **API**, pas leurs serveurs d'images,
//! qui sont des CDN dimensionnés pour tout autre chose. Faire passer une
//! vignette par la cadence de l'API d'iTunes ferait attendre trois secondes et
//! demie pour une image de dix kilo-octets.
//!
//! Les images ont donc leur propre file, commune à tous les services.
//!
//! # Pourquoi cela ne remonte pas jusqu'à l'interface
//!
//! La politique de sécurité du contenu n'autorise le webview à charger que ses
//! propres ressources (ADR-005). Une balise `<img src="https://…">` serait
//! bloquée — et c'est voulu : le webview d'un lecteur hors ligne n'a rien à
//! demander à l'extérieur. Les octets passent par ici, en Rust, et arrivent à
//! l'interface sous forme de `data:` URI.

use std::sync::OnceLock;
use std::time::Duration;

use crate::core::Result;

use super::http::Service;

/// Cadence des téléchargements d'images.
///
/// Assez lente pour ne pas ressembler à une aspiration, assez rapide pour
/// qu'une liste de huit propositions s'affiche en moins d'une seconde.
const MIN_INTERVAL: Duration = Duration::from_millis(100);

static SERVICE: OnceLock<Service> = OnceLock::new();

fn service() -> Result<&'static Service> {
    if let Some(service) = SERVICE.get() {
        return Ok(service);
    }

    let service = Service::new("Pochettes", MIN_INTERVAL)?;
    Ok(SERVICE.get_or_init(|| service))
}

/// Récupère les octets d'une image.
pub async fn fetch(url: &str) -> Result<Option<Vec<u8>>> {
    service()?.get_bytes(url).await
}
