//! Ouvrir une porte, le temps d'un échange.
//!
//! # Pourquoi un port ouvert sur le réseau local, alors qu'Onzer est hors ligne
//!
//! « Hors ligne » veut dire : rien ne part vers un serveur, aucun compte,
//! aucun intermédiaire. Deux appareils de la même maison qui se parlent
//! directement ne contredisent pas cette règle — c'est même la seule façon de
//! la tenir tout en gardant deux bibliothèques d'accord.
//!
//! Trois garde-fous, parce qu'un port ouvert reste un port ouvert :
//!
//! | | |
//! |---|---|
//! | **Le temps** | La porte n'existe que pendant que l'écran de synchronisation est affiché. La fermer est le comportement par défaut, pas une option |
//! | **Le secret** | Huit chiffres tirés au sort à chaque ouverture. Il n'est jamais réutilisé |
//! | **Les essais** | Cinq erreurs et la session se ferme. Un million de combinaisons ne servent à rien si l'on ne peut en tenter que cinq |
//!
//! # Pourquoi ce n'est pas la boucle locale
//!
//! Le serveur d'import (`ingest::server`) écoute sur `127.0.0.1` : ses clients
//! sont des scripts qui tournent sur la même machine. Ici, le client est un
//! téléphone à l'autre bout du salon. Écouter sur la boucle locale le rendrait
//! injoignable — c'est toute la question.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::core::{OnzerError, PathResolver, Result};

use super::etat;
use super::fusion::{fusionner, EtatSync};

/// Comment prévenir l'interface qu'une fusion vient d'être appliquée.
///
/// # Pourquoi une fonction et non une poignée Tauri
///
/// Le serveur n'a besoin de savoir qu'une chose : « préviens ». Lui donner une
/// `AppHandle` l'attacherait à une application graphique en train de tourner —
/// et rendrait la porte impossible à éprouver sans en lancer une. Ici, le test
/// passe une fermeture qui note l'appel, et vérifie que l'avertissement part.
pub type Avertisseur = Arc<dyn Fn(&str, &super::fusion::Fusion) + Send + Sync>;

/// Ce que la porte a besoin de savoir pour répondre.
pub struct EtatServeur {
    pub pool: SqlitePool,
    /// Pour retrouver les fichiers qu'on nous demande. Sous verrou partagé :
    /// la racine change quand le SSD est rebranché.
    pub paths: Arc<RwLock<PathResolver>>,
    /// Sans cet avertissement, l'écran garde son ancienne vérité : les favoris
    /// arrivent en base et n'apparaissent nulle part — ce qui se voit
    /// exactement comme une synchronisation qui ne marche pas.
    pub prevenir: Avertisseur,
}

/// Port d'écoute souhaité. Voisin de celui de l'API d'import, dans la même
/// plage haute. C'est celui que le client suppose quand on ne lui en donne pas.
pub const PORT: u16 = 47812;

/// Au-delà, la session se ferme. Voir l'en-tête du module.
const ESSAIS_MAX: u32 = 5;

/// Les états échangés pèsent quelques mégaoctets — les paroles, surtout. La
/// limite d'axum est de deux mégaoctets : sans ce relèvement, la fusion échoue
/// sur toute bibliothèque un peu fournie, avec un « 413 » que rien n'explique.
const TAILLE_MAX: usize = 64 * 1024 * 1024;

/// Ce que l'écran d'appairage affiche.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfosAppairage {
    /// L'adresse de cette machine sur le réseau local, la plus probable en
    /// tête. Il peut y en avoir plusieurs : un téléphone a le Wi-Fi **et** la
    /// carte SIM, et rien ne dit de l'extérieur laquelle porte l'échange.
    pub hote: String,
    /// Les autres adresses, quand la machine en a plusieurs.
    pub autres: Vec<String>,
    pub port: u16,
    /// Les huit chiffres à recopier, groupés par quatre à l'affichage.
    pub code: String,
    /// Le lien que le QR encode, et qu'on peut aussi coller à la main.
    pub lien: String,
    /// La matrice du QR, ligne par ligne. L'interface la dessine en SVG.
    pub qr: Vec<Vec<bool>>,
}

struct Session {
    code: String,
    essais: AtomicU32,
    arret: Option<tokio::sync::oneshot::Sender<()>>,
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

fn sessions() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

// ════════════════════════════════════════════════════════════════════════════
//  Ouverture et fermeture
// ════════════════════════════════════════════════════════════════════════════

/// Ouvre la porte et rend de quoi l'afficher.
pub async fn ouvrir(etat: Arc<EtatServeur>) -> Result<InfosAppairage> {
    ouvrir_sur(etat, PORT).await
}

/// # Pourquoi le port n'est pas garanti
///
/// Le port habituel peut être pris — par une autre application, ou par une
/// instance d'Onzer qu'on vient de fermer et dont la chaussette n'est pas
/// encore libérée. Échouer là-dessus serait absurde : n'importe quel port fait
/// l'affaire, puisque le code **et** le QR le transportent. On demande donc le
/// port habituel, et l'on se rabat sur celui que le système veut bien donner.
pub async fn ouvrir_sur(etat: Arc<EtatServeur>, souhaite: u16) -> Result<InfosAppairage> {
    fermer();

    let code = tirer_un_code();
    let mut adresses = adresses_locales();
    if adresses.is_empty() {
        return Err(OnzerError::Invalid(
            "aucun réseau local détecté. Le Wi-Fi est-il allumé ?".to_string(),
        ));
    }
    let hote = adresses.remove(0);

    let ecoute = match tokio::net::TcpListener::bind(SocketAddr::from((
        Ipv4Addr::UNSPECIFIED,
        souhaite,
    )))
    .await
    {
        Ok(ecoute) => ecoute,
        Err(_) => tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .await
            .map_err(|erreur| {
                OnzerError::Invalid(format!("aucun port disponible : {erreur}"))
            })?,
    };

    let port = ecoute
        .local_addr()
        .map_err(|erreur| OnzerError::Invalid(format!("port illisible : {erreur}")))?
        .port();

    let lien = format!("onzer://appairage?hote={hote}&port={port}&code={code}");

    let (arret, attendre_arret) = tokio::sync::oneshot::channel();

    {
        let mut garde = sessions().lock().map_err(|_| verrou_empoisonne())?;
        *garde = Some(Session {
            code: code.clone(),
            essais: AtomicU32::new(0),
            arret: Some(arret),
        });
    }

    let routeur = Router::new()
        .route("/sync/v1/fusion", post(fusion))
        .route("/sync/v1/fichier", get(fichier))
        .layer(DefaultBodyLimit::max(TAILLE_MAX))
        .with_state(etat);

    tokio::spawn(async move {
        let service = axum::serve(ecoute, routeur).with_graceful_shutdown(async {
            let _ = attendre_arret.await;
        });

        if let Err(erreur) = service.await {
            tracing::error!(%erreur, "serveur d'appairage arrêté");
        }

        tracing::info!("porte d'appairage refermée");
    });

    tracing::info!(%hote, port, "porte d'appairage ouverte");

    Ok(InfosAppairage {
        hote,
        autres: adresses,
        port,
        code: code.clone(),
        qr: matrice_qr(&lien),
        lien,
    })
}

/// Referme la porte. Sans effet si elle ne l'était pas.
pub fn fermer() {
    let Ok(mut garde) = sessions().lock() else {
        return;
    };

    if let Some(mut session) = garde.take() {
        if let Some(arret) = session.arret.take() {
            let _ = arret.send(());
        }
    }
}

/// La porte est-elle ouverte ?
pub fn ouverte() -> bool {
    sessions()
        .lock()
        .map(|garde| garde.is_some())
        .unwrap_or(false)
}

// ════════════════════════════════════════════════════════════════════════════
//  La route
// ════════════════════════════════════════════════════════════════════════════

/// Reçoit l'état de l'autre appareil, applique ce qui doit l'être, et rend le
/// sien **après** fusion.
///
/// # Pourquoi l'état d'après et non celui d'avant
///
/// Le client refait de son côté la même fusion, avec ce qu'on lui rend. En lui
/// donnant l'état déjà fusionné, il reçoit directement l'union des deux : un
/// seul aller-retour suffit, et la fusion étant stable, refaire le calcul ne
/// produit plus rien.
async fn fusion(
    State(serveur): State<Arc<EtatServeur>>,
    entetes: HeaderMap,
    Json(distant): Json<EtatSync>,
) -> std::result::Result<Json<EtatSync>, ErreurHttp> {
    verifier(&entetes)?;

    let local = etat::lire(&serveur.pool)
        .await
        .map_err(|erreur| ErreurHttp::interne(&erreur))?;

    let resultat = fusionner(&local, &distant);

    etat::appliquer(
        &serveur.pool,
        &distant.appareil,
        &resultat.changements,
        &resultat.arbitrages,
    )
    .await
    .map_err(|erreur| ErreurHttp::interne(&erreur))?;

    tracing::info!(
        appareil = %distant.appareil,
        changements = resultat.changements.len(),
        "fusion appliquée"
    );

    // # Pourquoi l'interface est prévenue
    //
    // Ce côté-ci n'a rien demandé : c'est l'autre appareil qui s'est connecté,
    // et l'écran affiché ici ne sait pas que sa base vient de changer. Les
    // favoris arrivaient donc bien en base — et n'apparaissaient nulle part,
    // ce qui se voit exactement comme une synchronisation qui ne marche pas.
    (serveur.prevenir)(&distant.appareil, &resultat);

    // Relu après application : c'est l'union que le client doit recevoir.
    let apres = etat::lire(&serveur.pool)
        .await
        .map_err(|erreur| ErreurHttp::interne(&erreur))?;

    Ok(Json(apres))
}

#[derive(Deserialize)]
struct DemandeFichier {
    chemin: String,
}

/// Rend le fichier audio d'un morceau.
///
/// # Pourquoi le chemin est vérifié en base et pas seulement nettoyé
///
/// `PathResolver::resolve` refuse déjà les chemins absolus et les `..`. Mais
/// il ne dit rien de ce qui se trouve **dans** la bibliothèque : un chemin
/// bien formé pourrait désigner n'importe quel fichier qu'on y aurait déposé.
/// Exiger que le morceau existe en base réduit ce qui sort d'ici à ce que la
/// bibliothèque connaît — et c'est tout ce que l'autre appareil peut demander.
async fn fichier(
    State(serveur): State<Arc<EtatServeur>>,
    entetes: HeaderMap,
    Query(demande): Query<DemandeFichier>,
) -> std::result::Result<Vec<u8>, ErreurHttp> {
    verifier(&entetes)?;

    let connu: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tracks WHERE relative_path = ? AND deleted_at IS NULL",
    )
    .bind(&demande.chemin)
    .fetch_optional(&serveur.pool)
    .await
    .map_err(|erreur| ErreurHttp::new(StatusCode::INTERNAL_SERVER_ERROR, &erreur.to_string()))?;

    if connu.is_none() {
        return Err(ErreurHttp::new(StatusCode::NOT_FOUND, "morceau inconnu"));
    }

    let chemin = serveur
        .paths
        .read()
        .await
        .resolve(&demande.chemin)
        .map_err(|erreur| ErreurHttp::new(StatusCode::BAD_REQUEST, &erreur.to_string()))?;

    tokio::fs::read(&chemin)
        .await
        .map_err(|erreur| ErreurHttp::new(StatusCode::NOT_FOUND, &format!("fichier illisible : {erreur}")))
}

/// Vérifie le code, et compte les erreurs.
fn verifier(entetes: &HeaderMap) -> std::result::Result<(), ErreurHttp> {
    let fourni = entetes
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|valeur| valeur.to_str().ok())
        .and_then(|valeur| valeur.strip_prefix("Bearer "))
        .unwrap_or_default()
        .trim()
        .to_string();

    let echec = {
        let Ok(garde) = sessions().lock() else {
            return Err(ErreurHttp::new(StatusCode::SERVICE_UNAVAILABLE, "session close"));
        };

        let Some(session) = garde.as_ref() else {
            return Err(ErreurHttp::new(StatusCode::SERVICE_UNAVAILABLE, "session close"));
        };

        if crate::ingest::token::constant_time_eq(&fourni, &session.code) {
            return Ok(());
        }

        session.essais.fetch_add(1, Ordering::SeqCst) + 1 >= ESSAIS_MAX
    };

    // La fermeture se fait **hors** du verrou : `fermer` le reprend.
    if echec {
        tracing::warn!("trop de codes erronés : la porte se referme");
        fermer();
    }

    Err(ErreurHttp::new(StatusCode::UNAUTHORIZED, "code invalide"))
}

// ════════════════════════════════════════════════════════════════════════════
//  Outils
// ════════════════════════════════════════════════════════════════════════════

/// Huit chiffres, tirés de l'aléa du système.
fn tirer_un_code() -> String {
    let brut = uuid::Uuid::new_v4();
    let octets = brut.as_bytes();

    let nombre = u32::from_be_bytes([octets[0], octets[1], octets[2], octets[3]]) % 100_000_000;
    format!("{nombre:08}")
}

/// Les adresses de cette machine sur le réseau local, la plus probable en tête.
///
/// # Pourquoi pas la chaussette UDP
///
/// La première version demandait au noyau « par où sortirais-tu ? », en
/// ouvrant une chaussette UDP vers une adresse publique. Élégant, sans
/// dépendance — et **faux sur un téléphone** : l'itinéraire vers l'extérieur
/// passe par la carte SIM. L'écran affichait donc fièrement `10.25.86.83`, une
/// adresse de l'opérateur que le Mac ne peut évidemment pas joindre.
///
/// Le noyau répondait à la bonne question. Ce n'était pas la nôtre : on ne
/// cherche pas la route vers Internet, on cherche l'adresse que le voisin de
/// salon peut composer.
///
/// # Comment les candidates sont classées
///
/// Seules les adresses IPv4 privées comptent — le reste n'est pas un réseau
/// local. Parmi elles, le nom de l'interface tranche : `wlan0` et `en0` avant
/// `rmnet` (la 5G), `utun` (les tunnels) ou `bridge` (les machines virtuelles).
/// Et comme la certitude n'existe pas, les autres restent affichées.
fn adresses_locales() -> Vec<String> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };

    let mut candidates: Vec<(u8, String)> = interfaces
        .into_iter()
        .filter(|interface| !interface.is_loopback())
        .filter_map(|interface| match interface.addr.ip() {
            std::net::IpAddr::V4(adresse) => classer(&interface.name, adresse),
            std::net::IpAddr::V6(_) => None,
        })
        .collect();

    candidates.sort();
    candidates.dedup();
    candidates.into_iter().map(|(_, adresse)| adresse).collect()
}

/// La matrice du QR, ligne par ligne.
fn matrice_qr(contenu: &str) -> Vec<Vec<bool>> {
    let Ok(code) = qrcode::QrCode::new(contenu.as_bytes()) else {
        return Vec::new();
    };

    let largeur = code.width();
    let couleurs = code.to_colors();

    couleurs
        .chunks(largeur)
        .map(|ligne| {
            ligne
                .iter()
                .map(|couleur| *couleur == qrcode::Color::Dark)
                .collect()
        })
        .collect()
}

/// Le rang d'une adresse, ou `None` si elle n'est pas un réseau local.
///
/// Extrait de l'énumération pour pouvoir être éprouvé : la logique tient
/// entièrement ici, et elle se trompait de la façon la plus coûteuse qui
/// soit — silencieusement, avec une adresse parfaitement plausible.
fn classer(nom: &str, adresse: Ipv4Addr) -> Option<(u8, String)> {
    // `is_private` couvre 10/8, 172.16/12 et 192.168/16. `is_link_local`
    // écarte le 169.254 que macOS invente quand rien ne répond.
    if !adresse.is_private() || adresse.is_link_local() {
        return None;
    }

    let nom = nom.to_lowercase();
    let rang = if nom.starts_with("wlan") || nom.starts_with("wl") {
        0
    } else if nom.starts_with("en") || nom.starts_with("eth") {
        1
    } else if nom.starts_with("rmnet") || nom.starts_with("pdp") {
        // La carte SIM : joignable de nulle part sur le réseau local.
        3
    } else if nom.starts_with("utun") || nom.starts_with("bridge") || nom.starts_with("ap") {
        4
    } else {
        2
    };

    Some((rang, adresse.to_string()))
}

fn verrou_empoisonne() -> OnzerError {
    OnzerError::Invalid("session d'appairage corrompue".to_string())
}

// ════════════════════════════════════════════════════════════════════════════
//  Erreurs HTTP
// ════════════════════════════════════════════════════════════════════════════

pub struct ErreurHttp {
    code: StatusCode,
    message: String,
}

impl ErreurHttp {
    fn new(code: StatusCode, message: &str) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }

    fn interne(erreur: &OnzerError) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, &erreur.to_string())
    }
}

impl axum::response::IntoResponse for ErreurHttp {
    fn into_response(self) -> axum::response::Response {
        (self.code, self.message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_code_fait_huit_chiffres() {
        for _ in 0..100 {
            let code = tirer_un_code();
            assert_eq!(code.len(), 8, "{code}");
            assert!(code.chars().all(|c| c.is_ascii_digit()), "{code}");
        }
    }

    #[test]
    fn deux_codes_ne_se_ressemblent_pas() {
        // Un code réutilisé serait un code devinable.
        let premier = tirer_un_code();
        let second = tirer_un_code();
        assert_ne!(premier, second);
    }

    #[test]
    fn le_wifi_passe_devant_la_carte_sim() {
        // Le défaut observé sur le téléphone : l'écran affichait l'adresse de
        // l'opérateur, que le Mac ne peut pas joindre.
        let wifi = classer("wlan1", Ipv4Addr::new(10, 112, 60, 26)).unwrap();
        let sim = classer("rmnet_data2", Ipv4Addr::new(10, 25, 86, 83)).unwrap();

        assert!(wifi.0 < sim.0, "le Wi-Fi doit passer devant la 5G");
    }

    #[test]
    fn une_adresse_publique_n_est_pas_un_reseau_local() {
        assert!(classer("en0", Ipv4Addr::new(93, 184, 216, 34)).is_none());
        // Celle que macOS invente quand rien ne répond.
        assert!(classer("en0", Ipv4Addr::new(169, 254, 1, 2)).is_none());
    }

    #[test]
    fn les_tunnels_passent_en_dernier() {
        let ethernet = classer("en0", Ipv4Addr::new(192, 168, 1, 42)).unwrap();
        let tunnel = classer("utun4", Ipv4Addr::new(192, 168, 9, 9)).unwrap();
        let machine_virtuelle = classer("bridge100", Ipv4Addr::new(192, 168, 64, 1)).unwrap();

        assert!(ethernet.0 < tunnel.0);
        assert!(ethernet.0 < machine_virtuelle.0);
    }

    #[test]
    fn le_qr_est_carre_et_non_vide() {
        let matrice = matrice_qr("onzer://appairage?hote=192.168.1.42&port=47812&code=12345678");

        assert!(!matrice.is_empty());
        for ligne in &matrice {
            assert_eq!(ligne.len(), matrice.len(), "le QR doit être carré");
        }
        assert!(
            matrice.iter().flatten().any(|point| *point),
            "un QR sans un seul point noir n'est pas un QR"
        );
    }
}
