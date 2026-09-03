//! Commandes de synchronisation entre deux appareils.

use serde::Serialize;
use tauri::State;

use crate::core::Result;
use crate::sync::{appairage, client};
use crate::AppState;

/// Ouvre la porte et rend de quoi l'afficher : le code, le lien, le QR.
#[tauri::command]
pub async fn open_pairing(state: State<'_, AppState>) -> Result<appairage::InfosAppairage> {
    appairage::ouvrir(state.pool.clone()).await
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
    state: State<'_, AppState>,
    host: String,
    port: u16,
    code: String,
) -> Result<client::RapportSync> {
    // Les espaces du code affiché — « 1234 5678 » — ne font pas partie du
    // secret : les exiger ferait échouer une saisie pourtant correcte.
    let code = code.chars().filter(|c| c.is_ascii_digit()).collect::<String>();

    client::synchroniser(&state.pool, host.trim(), port, &code).await
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
