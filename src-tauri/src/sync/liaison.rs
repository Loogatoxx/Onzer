//! La liaison qui reste ouverte, et la boucle qui la tient.
//!
//! Le raisonnement est dans `continu` ; ici, il n'y a que le transport.
//!
//! # Une seule boucle pour les deux appareils
//!
//! L'un héberge le tableau, l'autre l'atteint par le réseau — mais ce qu'ils
//! en font est identique : publier ce que je joue, lire ce que dit le tableau,
//! obéir à ce qui m'est adressé, attendre la suite. Écrire deux boucles
//! garantirait qu'un correctif n'en touche qu'une.
//!
//! La différence tient donc dans un seul objet, le canal : direct d'un côté,
//! HTTP de l'autre.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::watch;

use crate::core::Result;

use super::continu::{
    appliquer_publication, deposer_ordre, reagir, Action, Publication, Reaction, Tableau,
};

/// Durée maximale d'une attente avant de rendre la main.
///
/// Vingt-cinq secondes : assez long pour qu'une liaison au repos ne coûte
/// presque rien, assez court pour rester sous le délai des passerelles et des
/// piles réseau, qui coupent volontiers à trente.
const ATTENTE_MAX: Duration = Duration::from_secs(25);

/// Ce qu'on attend entre deux publications quand on tient le son.
const BATTEMENT: Duration = Duration::from_secs(1);

/// De quoi lire l'état du lecteur au moment où on le publie.
pub type Transport = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Publication> + Send>> + Send + Sync,
>;

/// De quoi appliquer un ordre reçu.
pub type Commandant = Arc<
    dyn Fn(Action, Option<i64>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// De quoi prévenir l'interface que l'autre appareil a bougé.
pub type Annonceur = Arc<dyn Fn(Option<super::continu::EtatDistant>) + Send + Sync>;

// ════════════════════════════════════════════════════════════════════════════
//  Le tableau, côté hôte
// ════════════════════════════════════════════════════════════════════════════

fn diffuseur() -> &'static watch::Sender<Tableau> {
    static DIFFUSEUR: OnceLock<watch::Sender<Tableau>> = OnceLock::new();
    DIFFUSEUR.get_or_init(|| watch::Sender::new(Tableau::default()))
}

/// Le tableau tel qu'il est maintenant.
pub fn tableau() -> Tableau {
    diffuseur().borrow().clone()
}

/// Applique une publication au tableau hébergé et rend son nouvel état.
pub fn publier_local(publication: &Publication) -> Tableau {
    let mut suivant = tableau();
    if appliquer_publication(&mut suivant, publication) {
        // `send` **échoue tant que personne n'écoute** : il rend une erreur
        // quand tous les récepteurs ont disparu, ce qui est le cas ordinaire
        // ici — l'hôte publie souvent avant que quiconque n'attende. Le
        // tableau ne se mettait alors jamais à jour. `send_replace` écrit dans
        // tous les cas, et réveille ceux qui écoutent s'il y en a.
        diffuseur().send_replace(suivant.clone());
    }
    suivant
}

/// Dépose un ordre dans le tableau hébergé.
pub fn ordonner_local(pour: &str, action: Action, valeur: Option<i64>) -> Tableau {
    let mut suivant = tableau();
    deposer_ordre(&mut suivant, pour, action, valeur);
    diffuseur().send_replace(suivant.clone());
    suivant
}

/// Attend que le tableau dépasse `depuis`, ou rend la main au bout d'un temps.
///
/// # Pourquoi il répond même sans changement
///
/// Une requête qui ne revient jamais ne se distingue pas d'une requête perdue.
/// En rendant la main toutes les vingt-cinq secondes, on donne au client
/// l'occasion de constater que la liaison tient — et, si elle ne tient plus,
/// de le découvrir en vingt-cinq secondes au lieu de jamais.
pub async fn attendre_local(depuis: u64) -> Tableau {
    let mut recepteur = diffuseur().subscribe();

    if recepteur.borrow().version > depuis {
        return recepteur.borrow().clone();
    }

    let _ = tokio::time::timeout(ATTENTE_MAX, async {
        while recepteur.changed().await.is_ok() {
            if recepteur.borrow().version > depuis {
                return;
            }
        }
    })
    .await;

    // La valeur est extraite avant la fin du bloc : une `Ref` gardée jusqu'au
    // point-virgule final vivrait plus longtemps que le récepteur.
    let derniere = recepteur.borrow().clone();
    derniere
}

/// Efface le tableau. Appelé quand la porte se referme.
pub fn oublier() {
    diffuseur().send_replace(Tableau::default());
}

// ════════════════════════════════════════════════════════════════════════════
//  Le canal
// ════════════════════════════════════════════════════════════════════════════

/// Par où passe le tableau.
pub enum Canal {
    /// J'héberge : je lis et j'écris directement.
    Local,
    /// L'autre héberge : je passe par le réseau.
    Distant {
        base: String,
        code: String,
        client: reqwest::Client,
    },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Envoi<'a> {
    publication: Option<&'a Publication>,
    ordre: Option<(String, Action, Option<i64>)>,
}

impl Canal {
    /// Ouvre un canal vers l'appareil qui héberge.
    pub fn vers(hote: &str, port: u16, code: &str) -> Result<Self> {
        Ok(Self::Distant {
            base: format!("http://{hote}:{port}"),
            code: code.to_string(),
            // Le délai couvre la longue attente, et un peu plus : c'est le
            // serveur qui décide quand répondre, pas nous.
            client: reqwest::Client::builder()
                .timeout(ATTENTE_MAX + Duration::from_secs(10))
                .build()
                .map_err(|erreur| {
                    crate::core::OnzerError::Invalid(format!("client HTTP : {erreur}"))
                })?,
        })
    }

    async fn publier(&self, publication: &Publication) -> Result<Tableau> {
        match self {
            Self::Local => Ok(publier_local(publication)),
            Self::Distant { base, code, client } => {
                let reponse = client
                    .post(format!("{base}/sync/v1/continu"))
                    .bearer_auth(code)
                    .json(&Envoi {
                        publication: Some(publication),
                        ordre: None,
                    })
                    .send()
                    .await
                    .map_err(reseau)?;

                reponse.json::<Tableau>().await.map_err(reseau)
            }
        }
    }

    /// Envoie un ordre à l'autre appareil.
    pub async fn ordonner(&self, pour: &str, action: Action, valeur: Option<i64>) -> Result<()> {
        match self {
            Self::Local => {
                ordonner_local(pour, action, valeur);
                Ok(())
            }
            Self::Distant { base, code, client } => {
                client
                    .post(format!("{base}/sync/v1/continu"))
                    .bearer_auth(code)
                    .json(&Envoi {
                        publication: None,
                        ordre: Some((pour.to_string(), action, valeur)),
                    })
                    .send()
                    .await
                    .map_err(reseau)?;
                Ok(())
            }
        }
    }

    async fn attendre(&self, depuis: u64) -> Result<Tableau> {
        match self {
            Self::Local => Ok(attendre_local(depuis).await),
            Self::Distant { base, code, client } => {
                // L'URL est écrite à la main : `query` demande à `reqwest` une
                // fonctionnalité que ce projet n'active pas, et un entier ne
                // pose aucun problème d'échappement.
                let reponse = client
                    .get(format!("{base}/sync/v1/continu?depuis={depuis}"))
                    .bearer_auth(code)
                    .send()
                    .await
                    .map_err(reseau)?;

                reponse.json::<Tableau>().await.map_err(reseau)
            }
        }
    }
}

fn reseau(erreur: reqwest::Error) -> crate::core::OnzerError {
    crate::core::OnzerError::Invalid(format!("liaison interrompue : {erreur}"))
}

// ════════════════════════════════════════════════════════════════════════════
//  La boucle
// ════════════════════════════════════════════════════════════════════════════

/// Une liaison ouverte : la boucle qui la tient, et par où elle passe.
struct Liaison {
    tache: tokio::task::JoinHandle<()>,
    /// Gardé pour les ordres venus de l'interface, qui n'appartiennent pas à
    /// la boucle : on appuie sur « pause » quand on le décide, pas au prochain
    /// battement.
    canal: Arc<Canal>,
}

fn en_cours() -> &'static Mutex<Option<Liaison>> {
    static EN_COURS: OnceLock<Mutex<Option<Liaison>>> = OnceLock::new();
    EN_COURS.get_or_init(|| Mutex::new(None))
}

/// Y a-t-il une liaison ouverte ?
pub fn ouverte() -> bool {
    en_cours()
        .lock()
        .map(|garde| garde.as_ref().is_some_and(|l| !l.tache.is_finished()))
        .unwrap_or(false)
}

/// Le canal de la liaison en cours, pour y glisser un ordre.
pub fn canal_courant() -> Option<Arc<Canal>> {
    en_cours()
        .lock()
        .ok()
        .and_then(|garde| garde.as_ref().map(|l| l.canal.clone()))
}

/// Coupe la liaison. Sans effet s'il n'y en avait pas.
pub fn couper() {
    if let Ok(mut garde) = en_cours().lock() {
        if let Some(liaison) = garde.take() {
            liaison.tache.abort();
        }
    }
}

/// Ouvre la liaison et la tient jusqu'à ce qu'on la coupe.
///
/// # Pourquoi une erreur ne l'arrête pas
///
/// Un téléphone qui sort du Wi-Fi, une mise en veille, un routeur qui hoquette :
/// la liaison casse pour des raisons qui se réparent seules. S'arrêter à la
/// première coupure obligerait à revenir dans l'écran de synchronisation pour
/// tout relancer — c'est-à-dire à ne jamais s'en servir.
pub fn tenir(
    canal: Canal,
    moi: String,
    transport: Transport,
    commandant: Commandant,
    annonceur: Annonceur,
) {
    couper();

    let canal = Arc::new(canal);
    let boucle = canal.clone();

    let tache = tokio::spawn(async move {
        let canal = boucle;
        let mut dernier_ordre = 0u64;
        let mut version = 0u64;
        let mut precedente: Option<Publication> = None;

        loop {
            let publication = transport().await;

            // On ne parle que quand on a quelque chose à dire : republier une
            // position identique réveillerait l'autre appareil pour rien.
            let tableau = if precedente.as_ref() == Some(&publication) {
                match canal.attendre(version).await {
                    Ok(tableau) => tableau,
                    Err(erreur) => {
                        tracing::debug!(%erreur, "attente interrompue");
                        tokio::time::sleep(BATTEMENT).await;
                        continue;
                    }
                }
            } else {
                match canal.publier(&publication).await {
                    Ok(tableau) => {
                        precedente = Some(publication);
                        tableau
                    }
                    Err(erreur) => {
                        tracing::debug!(%erreur, "publication perdue");
                        tokio::time::sleep(BATTEMENT).await;
                        continue;
                    }
                }
            };

            version = tableau.version;

            match reagir(&tableau, &moi, dernier_ordre) {
                Reaction::Appliquer(ordre) => {
                    dernier_ordre = ordre.numero;
                    commandant(ordre.action, ordre.valeur).await;
                    // L'ordre change ce que je joue : la prochaine publication
                    // doit repartir, même si le lecteur met un instant à
                    // répondre.
                    precedente = None;
                }
                Reaction::Montrer(etat) => annonceur(Some(etat)),
                Reaction::Rien => {
                    if tableau.source.is_none() {
                        annonceur(None);
                    }
                }
            }

            // Le battement borne le trafic quand on tient le son : sans lui,
            // la position changeant en continu, on publierait aussi vite que
            // le réseau le permet.
            if publication_active(&precedente) {
                tokio::time::sleep(BATTEMENT).await;
            }
        }
    });

    if let Ok(mut garde) = en_cours().lock() {
        *garde = Some(Liaison { tache, canal });
    }
}

fn publication_active(precedente: &Option<Publication>) -> bool {
    precedente.as_ref().is_some_and(|p| p.tient_le_son)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le tableau est unique au processus — c'est tout son intérêt en
    /// production, et c'est ce qui fait que deux tests parallèles se marchent
    /// dessus. Ils passent donc l'un après l'autre, et affirment ce qu'ils
    /// veulent vraiment affirmer plutôt que ce qui survit à la concurrence.
    fn un_a_la_fois() -> &'static tokio::sync::Mutex<()> {
        static VERROU: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        VERROU.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[tokio::test]
    async fn le_tableau_local_monte_et_se_lit() {
        let _garde = un_a_la_fois().lock().await;
        oublier();

        let tableau = publier_local(&Publication {
            appareil: "Mac".into(),
            titre: Some("A".into()),
            tient_le_son: true,
            ..Publication::default()
        });

        assert_eq!(tableau.version, 1);
        assert_eq!(super::tableau().source.as_deref(), Some("Mac"));
    }

    #[tokio::test]
    async fn une_attente_deja_depassee_repond_tout_de_suite() {
        let _garde = un_a_la_fois().lock().await;
        oublier();
        publier_local(&Publication {
            appareil: "Mac".into(),
            titre: Some("A".into()),
            tient_le_son: true,
            ..Publication::default()
        });

        let tableau = tokio::time::timeout(Duration::from_millis(200), attendre_local(0))
            .await
            .expect("la réponse ne doit pas attendre");

        assert!(tableau.version > 0);
    }

    /// Le point de la longue attente : on ne répond pas tant que rien ne
    /// change, et l'on répond dès que quelque chose change.
    #[tokio::test]
    async fn une_attente_se_reveille_quand_le_tableau_change() {
        let _garde = un_a_la_fois().lock().await;
        oublier();
        let depart = super::tableau().version;

        let attente = tokio::spawn(async move { attendre_local(depart).await });
        tokio::time::sleep(Duration::from_millis(30)).await;

        publier_local(&Publication {
            appareil: "Téléphone".into(),
            titre: Some("B".into()),
            tient_le_son: true,
            ..Publication::default()
        });

        let tableau = tokio::time::timeout(Duration::from_secs(2), attente)
            .await
            .expect("réveillée à temps")
            .expect("tâche saine");

        assert_eq!(tableau.source.as_deref(), Some("Téléphone"));
    }

    #[tokio::test]
    async fn un_ordre_local_se_depose() {
        let _garde = un_a_la_fois().lock().await;
        oublier();
        let tableau = ordonner_local("Mac", Action::Pause, None);
        let ordre = tableau.ordre.expect("ordre déposé");
        assert_eq!(ordre.pour, "Mac");
        assert_eq!(ordre.action, Action::Pause);
    }
}
