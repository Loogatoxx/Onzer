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
use super::fusion::{fusionner, EtatSync, Portee};

/// Ce que l'autre appareil envoie : son état, et ce qu'il accepte d'échanger.
///
/// `flatten` garde le format d'avant : un appareil qui n'a pas encore la mise
/// à jour n'envoie que l'état, et la portée prend sa valeur par défaut — tout,
/// c'est-à-dire le comportement qu'il connaît.
#[derive(Deserialize)]
struct DemandeFusion {
    #[serde(flatten)]
    etat: EtatSync,
    #[serde(default)]
    portee: Portee,
}

/// Comment prévenir l'interface qu'une fusion vient d'être appliquée.
///
/// # Pourquoi une fonction et non une poignée Tauri
///
/// Le serveur n'a besoin de savoir qu'une chose : « préviens ». Lui donner une
/// `AppHandle` l'attacherait à une application graphique en train de tourner —
/// et rendrait la porte impossible à éprouver sans en lancer une. Ici, le test
/// passe une fermeture qui note l'appel, et vérifie que l'avertissement part.
pub type Avertisseur = Arc<dyn Fn(&str, &super::fusion::Fusion) + Send + Sync>;

/// De quoi demander, **au moment où l'on répond**, ce que le lecteur joue.
///
/// # Pourquoi une fonction et non une valeur
///
/// La porte reste ouverte plusieurs minutes. Une valeur figée à l'ouverture
/// annoncerait le morceau d'il y a cinq minutes — et l'autre appareil
/// reprendrait une écoute périmée, ce qui est exactement ce qu'on essaie
/// d'éviter.
pub type SourceLecture = Arc<
    dyn Fn() -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<super::fusion::LectureSync>> + Send>,
        > + Send
        + Sync,
>;

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
    /// Ce que cet appareil écoute, demandé à chaque échange.
    pub lecture: SourceLecture,
    /// Appelé quand la porte se referme, y compris **d'elle-même**. Sans cela,
    /// l'écran garde son QR affiché après une fermeture de sûreté, et l'on
    /// scanne un code que plus personne n'écoute.
    pub fermeture: Annonce,
}

/// Ce qu'on appelle pour dire que la porte s'est refermée.
pub type Annonce = Arc<dyn Fn() + Send + Sync>;

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
    /// Gardée ici plutôt que dans l'état du serveur : `fermer` ne voit que la
    /// session, et c'est bien de là que part la fermeture.
    fermeture: Option<Annonce>,
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
            fermeture: Some(etat.fermeture.clone()),
        });
    }

    let routeur = Router::new()
        .route("/sync/v1/fusion", post(fusion))
        .route("/sync/v1/fichier", get(fichier))
        .route("/sync/v1/continu", get(continu_lire).post(continu_ecrire))
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

    let mut annoncer = None;
    if let Some(mut session) = garde.take() {
        if let Some(arret) = session.arret.take() {
            let _ = arret.send(());
        }
        annoncer = session.fermeture.take();
    }

    // La liaison continue vit dans la porte : la refermer sans la couper
    // laisserait une boucle interroger un serveur qui n'écoute plus.
    super::liaison::couper();
    super::liaison::oublier();

    // Après avoir lâché le verrou : l'interface, prévenue, peut redemander
    // l'état de la porte, et le ferait sur un verrou encore tenu.
    drop(garde);
    if let Some(annoncer) = annoncer {
        annoncer();
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
    Json(demande): Json<DemandeFusion>,
) -> std::result::Result<Json<EtatSync>, ErreurHttp> {
    verifier(&entetes)?;

    // Quelqu'un est venu, code en main : la porte a rempli son office, et la
    // liaison continue peut désormais survivre à la fermeture de l'écran.
    super::liaison::noter_pair();

    let DemandeFusion { etat: distant, portee } = demande;

    let lecture = (serveur.lecture)().await;
    let local = etat::lire(&serveur.pool, lecture)
        .await
        .map_err(|erreur| ErreurHttp::interne(&erreur))?;

    let alias = etat::alias(&serveur.pool)
        .await
        .map_err(|erreur| ErreurHttp::interne(&erreur))?;

    // La portée est celle du demandeur : c'est lui qui a choisi, et les deux
    // appareils doivent appliquer le même choix — sans quoi l'un prendrait ce
    // que l'autre a refusé, et les deux bibliothèques cesseraient de dire la
    // même chose.
    let resultat = fusionner(&local, &distant, &alias, &portee);

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
    let apres = etat::lire(&serveur.pool, (serveur.lecture)().await)
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

    // # Pourquoi ce cas mérite son propre message
    //
    // Le morceau est en base et son fichier n'y est plus : c'est un morceau
    // **hors ligne**, cas courant et parfaitement normal. Le client n'en
    // voyait que « 404 Not Found », qui se lit comme une panne. Trois
    // morceaux ont manqué au premier transfert réel pour cette raison, et le
    // message ne le disait pas.
    tokio::fs::read(&chemin).await.map_err(|_| {
        ErreurHttp::new(
            StatusCode::NOT_FOUND,
            "ce morceau est hors ligne sur l'autre appareil : sa fiche existe, son fichier non",
        )
    })
}

// ════════════════════════════════════════════════════════════════════════════
//  La liaison continue
// ════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct Depuis {
    /// La version déjà connue du demandeur. La réponse n'arrive qu'au-delà.
    depuis: Option<u64>,
}

/// Rend le tableau dès qu'il dépasse la version connue du demandeur.
///
/// La requête **ne répond pas tout de suite** : c'est tout le mécanisme. Elle
/// reste ouverte jusqu'à ce qu'il y ait quelque chose à dire, et un changement
/// traverse alors en un aller-retour au lieu d'attendre le prochain sondage.
async fn continu_lire(
    entetes: HeaderMap,
    Query(depuis): Query<Depuis>,
) -> std::result::Result<Json<super::continu::Tableau>, ErreurHttp> {
    verifier(&entetes)?;
    Ok(Json(
        super::liaison::attendre_local(depuis.depuis.unwrap_or(0)).await,
    ))
}

#[derive(Deserialize)]
struct Envoi {
    publication: Option<super::continu::Publication>,
    /// Destinataire, action, valeur.
    ordre: Option<(String, super::continu::Action, Option<i64>)>,
}

/// Reçoit ce que l'autre appareil publie, ou l'ordre qu'il adresse.
async fn continu_ecrire(
    entetes: HeaderMap,
    Json(envoi): Json<Envoi>,
) -> std::result::Result<Json<super::continu::Tableau>, ErreurHttp> {
    verifier(&entetes)?;
    super::liaison::noter_pair();

    let mut tableau = super::liaison::tableau();

    if let Some(publication) = &envoi.publication {
        tableau = super::liaison::publier_local(publication);
    }

    if let Some((pour, action, valeur)) = envoi.ordre {
        tableau = super::liaison::ordonner_local(&pour, action, valeur);
    }

    Ok(Json(tableau))
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

    /// Le scénario complet, celui qui a échoué à l'usage : un appareil pose un
    /// ordre **par le réseau**, et l'appareil qui tient le son l'exécute.
    ///
    /// # Pourquoi il ne suffisait pas d'éprouver les pièces
    ///
    /// Le tableau savait recevoir un ordre, la boucle savait en prendre un, et
    /// la porte savait répondre — chacun sous son propre test. Personne ne
    /// disait que la porte et la boucle partagent bien le même tableau, ni que
    /// la boucle se réveille d'une longue attente quand l'ordre arrive d'une
    /// requête HTTP plutôt que du même fil. C'est précisément là que la
    /// synchronisation a manqué.
    #[tokio::test]
    async fn un_ordre_venu_du_reseau_est_execute() {
        use crate::sync::continu::{Action, Publication};
        use crate::sync::liaison::{self, Canal};
        use std::time::Duration;

        let _garde = liaison::un_a_la_fois().lock().await;
        liaison::oublier();

        // Une session posée à la main : ouvrir la vraie porte demanderait une
        // base de données, dont ce chemin ne se sert pas.
        let code = "12345678".to_string();
        {
            let mut garde = sessions().lock().expect("verrou sain");
            *garde = Some(Session {
                code: code.clone(),
                essais: AtomicU32::new(0),
                arret: None,
                fermeture: None,
            });
        }

        let routeur =
            Router::new().route("/sync/v1/continu", get(continu_lire).post(continu_ecrire));
        let ecoute = tokio::net::TcpListener::bind(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            0,
        )))
        .await
        .expect("port libre");
        let port = ecoute.local_addr().expect("adresse lisible").port();
        let serveur = tokio::spawn(async move {
            let _ = axum::serve(ecoute, routeur).await;
        });

        // L'hôte : il tient le son, et c'est lui qui doit obéir.
        let (rapport, mut recu) = tokio::sync::mpsc::unbounded_channel();
        liaison::tenir(
            Canal::Local,
            "Mac".to_string(),
            Arc::new(|| {
                Box::pin(async {
                    Publication {
                        appareil: "Mac".into(),
                        titre: Some("A".into()),
                        tient_le_son: true,
                        ..Publication::default()
                    }
                })
            }),
            Arc::new(move |action, _valeur| {
                let rapport = rapport.clone();
                Box::pin(async move {
                    let _ = rapport.send(action);
                })
            }),
            Arc::new(|_| {}),
        );

        // Le temps que la boucle publie et se mette en longue attente.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Le pair, tel que le téléphone le fait : une requête, un ordre.
        let reponse = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/sync/v1/continu"))
            .bearer_auth(&code)
            .json(&serde_json::json!({
                "publication": null,
                "ordre": ["Mac", "suivant", null],
            }))
            .send()
            .await
            .expect("la porte répond");
        assert_eq!(reponse.status(), StatusCode::OK);

        let action = tokio::time::timeout(Duration::from_secs(3), recu.recv())
            .await
            .expect("l'ordre doit être exécuté sans faire attendre")
            .expect("un ordre reçu");
        assert_eq!(action, Action::Suivant);

        liaison::couper();
        serveur.abort();
        let _ = sessions().lock().map(|mut garde| garde.take());
    }

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
