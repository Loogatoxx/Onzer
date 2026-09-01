//! Comparaison d'une playlist Spotify avec la bibliothèque.
//!
//! # Ce que fait cette commande, et ce qu'elle ne fait pas
//!
//! Elle répond à une question de bibliothécaire : **« qu'est-ce qui manque ? »**
//! Onzer lit une liste de titres, la confronte à ce qu'il possède, et rend la
//! liste des absents. Il ne télécharge rien et n'embarque aucun téléchargeur.
//!
//! La formule proposée pour le terminal est un service rendu, pas un moteur :
//! elle assemble des noms de titres que l'utilisateur taperait autrement à la
//! main, pour l'outil qu'il utilise déjà. Ce qu'il en fait, et depuis quelle
//! source, reste sa décision et sa responsabilité.
//!
//! # Le rapprochement
//!
//! Il réutilise `find_by_tags`, le second filet du dédoublonnage à l'import :
//! titre normalisé, artiste normalisé, durée à deux secondes près. La règle est
//! déjà éprouvée, et surtout elle est **la même** — un morceau considéré comme
//! présent ici doit être exactement celui que l'import aurait refusé comme
//! doublon. Deux règles différentes finiraient par se contredire.

use serde::Serialize;
use tauri::State;

use crate::core::{OnzerError, Result};
use crate::db::{repository, settings};
use crate::identify::spotify::{SpotifyClient, SpotifyTrack};
use crate::library::naming::normalize_key;
use crate::AppState;

/// Réglages : identifiants d'application Spotify.
pub const SPOTIFY_ID: &str = "spotify_client_id";
pub const SPOTIFY_SECRET: &str = "spotify_client_secret";

/// Ce que la comparaison a trouvé.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistComparison {
    pub playlist_name: String,
    pub total: usize,
    pub present: usize,
    /// Les absents, dans l'ordre de la playlist.
    pub missing: Vec<SpotifyTrack>,
    /// Commande prête à coller dans un terminal.
    pub command: String,
    /// Chemin du fichier de liens écrit à côté de la bibliothèque.
    pub list_path: Option<String>,
}

/// Compare une playlist publique à la bibliothèque.
#[tauri::command]
pub async fn compare_playlist(
    state: State<'_, AppState>,
    url: String,
) -> Result<PlaylistComparison> {
    let client_id: Option<String> = settings::get(&state.pool, SPOTIFY_ID).await?;
    let client_secret: Option<String> = settings::get(&state.pool, SPOTIFY_SECRET).await?;

    let client = SpotifyClient::new(
        client_id.unwrap_or_default(),
        client_secret.unwrap_or_default(),
    )?;

    let playlist = client.playlist(&url).await?;
    let total = playlist.tracks.len();

    let mut missing = Vec::new();
    for track in playlist.tracks {
        let normalized_title = normalize_key(&track.title);
        let normalized_artist = track.main_artist().map(normalize_key);

        let found = repository::find_by_tags(
            &state.pool,
            &normalized_title,
            normalized_artist.as_deref(),
            Some(track.duration_ms),
        )
        .await?;

        if found.is_none() {
            missing.push(track);
        }
    }

    let paths = state.paths.read().await.clone();
    let inbox = paths
        .library_root()
        .map(crate::ingest::inbox::inbox_path);

    let list_path = write_list(inbox.as_deref(), &playlist.name, &missing);

    Ok(PlaylistComparison {
        playlist_name: playlist.name,
        present: total - missing.len(),
        total,
        command: command_for(list_path.as_deref(), inbox.as_deref()),
        list_path: list_path.map(|path| path.display().to_string()),
        missing,
    })
}

/// Écrit la liste des liens manquants à côté du dossier de dépôt.
///
/// Un fichier plutôt qu'une commande de plusieurs kilo-octets : une playlist de
/// cent titres produirait une ligne impossible à lire, et que certains shells
/// refuseraient.
fn write_list(
    inbox: Option<&std::path::Path>,
    playlist_name: &str,
    missing: &[SpotifyTrack],
) -> Option<std::path::PathBuf> {
    if missing.is_empty() {
        return None;
    }

    let inbox = inbox?;
    std::fs::create_dir_all(inbox).ok()?;

    let file_name = format!(
        "_manquants — {}.txt",
        crate::library::naming::sanitize_segment(playlist_name)
            .unwrap_or_else(|| "playlist".to_string())
    );
    let path = inbox.join(file_name);

    let body: String = missing
        .iter()
        .filter(|track| !track.url.is_empty())
        .map(|track| format!("{}\n", track.url))
        .collect();

    std::fs::write(&path, body).ok()?;
    Some(path)
}

/// Assemble la ligne de commande à coller dans un terminal.
///
/// # Pourquoi `xargs` et un fichier
///
/// Les liens sont passés en arguments par `xargs`, qui découpe lui-même s'ils
/// sont trop nombreux. Une commande écrite d'un bloc atteindrait la limite de
/// longueur du shell sur une playlist un peu fournie.
///
/// La sortie vise le **dossier de dépôt** : les fichiers atterrissent là où
/// Onzer les attend, et le reste — dédoublonnage, identification, rangement —
/// se fait tout seul. C'est ce qui rend la boucle presque automatique.
fn command_for(list_path: Option<&std::path::Path>, inbox: Option<&std::path::Path>) -> String {
    let (Some(list_path), Some(inbox)) = (list_path, inbox) else {
        return String::new();
    };

    format!(
        "xargs -a {} spotdl download --output {}",
        shell_quote(&list_path.display().to_string()),
        shell_quote(&format!(
            "{}/{{artists}} - {{title}}.{{output-ext}}",
            inbox.display()
        ))
    )
}

/// Protège un chemin pour un shell POSIX.
///
/// Les chemins de la bibliothèque contiennent des espaces et des accents ; sans
/// protection, la commande se briserait au premier dossier « Mes musiques ».
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// État des identifiants Spotify, sans jamais les révéler.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyStatus {
    pub configured: bool,
    /// Aperçu masqué de l'identifiant client, pour repérer un collage tronqué.
    pub id_hint: Option<String>,
}

#[tauri::command]
pub async fn spotify_status(state: State<'_, AppState>) -> Result<SpotifyStatus> {
    let client_id: Option<String> = settings::get(&state.pool, SPOTIFY_ID).await?;
    let secret: Option<String> = settings::get(&state.pool, SPOTIFY_SECRET).await?;

    let configured = client_id.as_deref().is_some_and(|value| !value.trim().is_empty())
        && secret.as_deref().is_some_and(|value| !value.trim().is_empty());

    Ok(SpotifyStatus {
        id_hint: client_id.as_deref().map(mask),
        configured,
    })
}

/// `a1b2••••••` : de quoi reconnaître une clé, jamais de quoi la réutiliser.
fn mask(value: &str) -> String {
    let visible: String = value.chars().take(4).collect();
    format!("{visible}{}", "•".repeat(6))
}

#[tauri::command]
pub async fn set_spotify_credentials(
    state: State<'_, AppState>,
    client_id: String,
    client_secret: String,
) -> Result<()> {
    let client_id = client_id.trim().to_string();
    let client_secret = client_secret.trim().to_string();

    if client_id.is_empty() != client_secret.is_empty() {
        return Err(OnzerError::Invalid(
            "les deux identifiants sont nécessaires".to_string(),
        ));
    }

    settings::set(&state.pool, SPOTIFY_ID, &client_id).await?;
    settings::set(&state.pool, SPOTIFY_SECRET, &client_secret).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn protege_les_chemins_a_espaces_et_a_accents() {
        // Sans protection, la commande se briserait au premier dossier
        // « Mes musiques ».
        assert_eq!(shell_quote("/Volumes/Lexar/Ma musique"), "'/Volumes/Lexar/Ma musique'");
        assert_eq!(shell_quote("/tmp/Ipséité"), "'/tmp/Ipséité'");
    }

    #[test]
    fn protege_une_apostrophe() {
        // Un dossier « L'été » fermerait la citation et laisserait le shell
        // interpréter la suite.
        assert_eq!(shell_quote("/tmp/L'été"), r"'/tmp/L'\''été'");
    }

    #[test]
    fn la_commande_vise_le_dossier_de_depot() {
        // C'est ce qui rend la boucle presque automatique : les fichiers
        // atterrissent là où Onzer les attend.
        let commande = command_for(
            Some(Path::new("/M/_Inbox/_manquants.txt")),
            Some(Path::new("/M/_Inbox")),
        );

        assert!(commande.starts_with("xargs -a "));
        assert!(commande.contains("spotdl download"));
        assert!(commande.contains("/M/_Inbox/{artists} - {title}.{output-ext}"));
    }

    #[test]
    fn pas_de_commande_sans_bibliotheque() {
        // Sans racine configurée, il n'y a nulle part où déposer : proposer une
        // commande qui écrirait n'importe où serait pire que de n'en proposer
        // aucune.
        assert!(command_for(None, Some(Path::new("/M/_Inbox"))).is_empty());
        assert!(command_for(Some(Path::new("/M/x.txt")), None).is_empty());
    }

    #[test]
    fn lapercu_masque_ne_revele_pas_la_cle() {
        let masque = mask("a1b2c3d4e5f6g7h8");

        assert!(masque.starts_with("a1b2"));
        assert!(!masque.contains("c3d4"), "la suite ne doit pas transparaître");
    }
}
