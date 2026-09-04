//! Commandes de synchronisation entre deux appareils.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::core::Result;
use crate::sync::appairage::EtatServeur;
use crate::sync::continu::{Action, EtatDistant, Publication};
use crate::sync::fusion::{Manquant, Portee};
use crate::sync::liaison::{self, Canal};
use crate::sync::{appairage, client};
use crate::AppState;

/// L'événement qui porte l'état de l'autre appareil jusqu'à l'interface.
pub const LIAISON_EVENT: &str = "sync://distant";

/// Ouvre la porte et rend de quoi l'afficher : le code, le lien, le QR.
#[tauri::command]
pub async fn open_pairing(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<appairage::InfosAppairage> {
    let infos = appairage::ouvrir(Arc::new(EtatServeur {
        pool: state.pool.clone(),
        paths: state.paths.clone(),
        prevenir: Arc::new({
            let app = app.clone();
            move |appareil, resultat| crate::sync::prevenir(&app, appareil, resultat)
        }),
        lecture: Arc::new({
            let app = app.clone();
            move || {
                let app = app.clone();
                Box::pin(async move { lecture_courante(&app).await })
                    as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
            }
        }),
    }))
    .await?;

    // Celui qui héberge tient la liaison sans passer par le réseau : le
    // tableau est chez lui.
    tenir(&app, Canal::Local);

    Ok(infos)
}

/// Ouvre la liaison continue, des deux côtés, avec le même code.
///
/// # Pourquoi la même boucle des deux côtés
///
/// Publier ce que je joue, lire le tableau, obéir à ce qui m'est adressé,
/// attendre la suite : c'est le même travail que l'on héberge ou non. Seul le
/// chemin d'accès au tableau change.
fn tenir(app: &AppHandle, canal: Canal) {
    let moi = crate::sync::etat::nom_appareil();

    let transport: crate::sync::liaison::Transport = Arc::new({
        let app = app.clone();
        move || {
            let app = app.clone();
            Box::pin(async move { publication(&app).await })
        }
    });

    let commandant: crate::sync::liaison::Commandant = Arc::new({
        let app = app.clone();
        move |action, valeur| {
            let app = app.clone();
            Box::pin(async move { obeir(&app, action, valeur).await })
        }
    });

    let annonceur: crate::sync::liaison::Annonceur = Arc::new({
        let app = app.clone();
        move |etat: Option<EtatDistant>| {
            use tauri::Emitter;
            let _ = app.emit(LIAISON_EVENT, etat);
        }
    });

    liaison::tenir(canal, moi, transport, commandant, annonceur);
}

/// Ce que ce lecteur publie sur le tableau.
async fn publication(app: &AppHandle) -> Publication {
    use tauri::Manager;

    let moi = crate::sync::etat::nom_appareil();
    let vide = Publication {
        appareil: moi.clone(),
        tient_le_son: false,
        ..Publication::default()
    };

    let etat = app.state::<AppState>();
    let Ok(player) = etat.player() else {
        return vide;
    };

    let instantane = player.snapshot().await;
    let Some(courant) = instantane.current else {
        return vide;
    };

    Publication {
        appareil: moi,
        titre: Some(courant.title),
        artiste: courant.artist,
        position_ms: instantane.position_ms,
        duree_ms: instantane.duration_ms,
        en_lecture: instantane.is_playing,
        // Un morceau chargé mais en pause compte : c'est ce qui permet à
        // l'autre appareil d'afficher la pause et de la relancer.
        tient_le_son: true,
    }
}

/// Applique un ordre venu de l'autre appareil.
async fn obeir(app: &AppHandle, action: Action, valeur: Option<i64>) {
    use tauri::Manager;

    let etat = app.state::<AppState>();
    let Ok(player) = etat.player() else {
        return;
    };

    let paths = etat.paths.read().await.clone();

    let resultat = match action {
        // `toggle` bascule : appelé sur un lecteur déjà en marche, il
        // l'arrêterait. Un ordre dit ce qu'il veut, pas le contraire de ce
        // qu'il trouve.
        Action::Lecture => {
            if player.is_playing() {
                Ok(())
            } else {
                player.toggle().await
            }
        }
        Action::Pause | Action::Rendre => player.pause().await,
        // `false` : ce saut vient d'une main, même si elle est à l'autre bout
        // du salon. Le compter comme automatique fausserait le journal
        // d'écoute, qui distingue un morceau passé d'un morceau rejeté.
        Action::Suivant => player.next(&etat.pool, &paths, false).await,
        Action::Precedent => player.previous(&etat.pool, &paths).await,
        Action::Position => match valeur {
            Some(position) => player.seek(position).await,
            None => Ok(()),
        },
    };

    if let Err(erreur) = resultat {
        tracing::warn!(%erreur, "ordre distant refusé");
    }
}

/// Envoie un ordre à l'appareil qui tient le son.
#[tauri::command]
pub async fn link_command(
    state: State<'_, AppState>,
    device: String,
    action: Action,
    value: Option<i64>,
) -> Result<()> {
    let _ = state;
    liaison::canal_courant()
        .ok_or_else(|| {
            crate::core::OnzerError::Invalid("aucune liaison ouverte".into())
        })?
        .ordonner(&device, action, value)
        .await
}

/// La liaison continue est-elle ouverte ?
#[tauri::command]
pub async fn link_open() -> Result<bool> {
    Ok(liaison::ouverte())
}

/// Coupe la liaison continue sans refermer la porte.
#[tauri::command]
pub async fn stop_link() -> Result<()> {
    liaison::couper();
    Ok(())
}

/// Referme la porte. Appelée en quittant l'écran — et c'est le point : une
/// porte qu'on oublie de fermer est une porte qui reste ouverte.
#[tauri::command]
pub async fn close_pairing() -> Result<()> {
    appairage::fermer();
    Ok(())
}

#[tauri::command]
pub async fn pairing_open() -> Result<bool> {
    Ok(appairage::ouverte())
}

/// Se connecte à l'autre appareil et fusionne.
#[tauri::command]
pub async fn sync_with_device(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    port: u16,
    code: String,
    // `scope` absent veut dire tout, comme avant.
    scope: Option<Portee>,
) -> Result<client::RapportSync> {
    // Les espaces du code affiché — « 1234 5678 » — ne font pas partie du
    // secret : les exiger ferait échouer une saisie pourtant correcte.
    let code = code.chars().filter(|c| c.is_ascii_digit()).collect::<String>();

    let lecture = lecture_courante(&app).await;
    let portee = scope.unwrap_or_default();
    let rapport =
        client::synchroniser(&state.pool, host.trim(), port, &code, lecture, &portee).await?;

    // La porte de l'autre appareil est ouverte et le code vient de servir :
    // c'est le seul moment où l'on est sûr de pouvoir tenir la liaison.
    if let Ok(canal) = Canal::vers(host.trim(), port, &code) {
        tenir(&app, canal);
    }

    Ok(rapport)
}

/// Ce que ce lecteur joue, s'il joue.
///
/// Rendre `None` plutôt qu'une erreur quand il n'y a pas de carte son : une
/// synchronisation n'a aucune raison d'échouer parce qu'aucun son ne sort.
async fn lecture_courante(app: &AppHandle) -> Option<crate::sync::fusion::LectureSync> {
    use tauri::Manager;

    let etat = app.state::<AppState>();
    let player = etat.player().ok()?;

    crate::sync::lecture_courante(&player.snapshot().await)
}

/// Découpe un lien `onzer://appairage?…` collé ou scanné.
#[tauri::command]
pub async fn read_pairing_link(link: String) -> Result<Option<LienAppairage>> {
    Ok(client::lire_lien(link.trim()).map(|(host, port, code)| LienAppairage {
        host,
        port,
        code,
    }))
}

/// Rapatrie les fichiers que l'autre appareil possède et pas nous.
///
/// # Pourquoi c'est une commande à part
///
/// La fusion est instantanée : quelques mégaoctets, une seconde. Un transfert
/// de fichiers, lui, peut durer un quart d'heure et remplir un téléphone. Les
/// enchaîner d'office ferait d'un geste anodin une décision lourde, prise sans
/// qu'on la voie venir.
#[tauri::command]
pub async fn fetch_missing_files(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    port: u16,
    code: String,
    tracks: Vec<Manquant>,
) -> Result<client::RapportTransfert> {
    let code = code.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    let paths = state.paths.read().await.clone();

    client::telecharger(&app, &state.pool, &paths, host.trim(), port, &code, &tracks).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LienAppairage {
    pub host: String,
    pub port: u16,
    pub code: String,
}

/// Les arbitrages passés, du plus récent au plus ancien.
///
/// # Pourquoi ils sont consultables
///
/// Une fusion qui tranche en silence est une fusion à laquelle on ne peut pas
/// faire confiance : le jour où un favori disparaît, il n'y a rien à consulter,
/// et le doute s'étend à tout le reste.
#[tauri::command]
pub async fn sync_journal(state: State<'_, AppState>) -> Result<Vec<LigneJournal>> {
    let lignes = sqlx::query_as::<_, LigneJournal>(
        "SELECT at, pair, kind, subject, replaced, kept
           FROM sync_journal
       ORDER BY at DESC, id DESC
          LIMIT 200",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(lignes)
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LigneJournal {
    pub at: i64,
    pub pair: String,
    pub kind: String,
    pub subject: String,
    pub replaced: Option<String>,
    pub kept: Option<String>,
}
