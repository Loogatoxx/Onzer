//! Comparaison d'une playlist avec la bibliothèque.
//!
//! # Ce que fait cette commande, et ce qu'elle ne fait pas
//!
//! Elle répond à une question de bibliothécaire : **« qu'est-ce qui manque ? »**
//! Onzer lit une liste de titres, la confronte à ce qu'il possède, et rend la
//! liste des absents. Il ne télécharge rien, n'embarque aucun téléchargeur, et
//! n'exécute aucune commande.
//!
//! # Pourquoi un fichier et non l'API Spotify
//!
//! Le premier jet interrogeait l'API officielle avec des identifiants créés par
//! l'utilisateur. Spotify a répondu **403 — « Active premium subscription
//! required for the owner of the app »** : lire une playlist publique exige
//! désormais un abonnement payant sur le compte propriétaire de l'application.
//!
//! `spotdl`, déjà installé chez l'utilisateur, sait produire la liste avec sa
//! sous-commande `save` — sans rien télécharger et sans aucun identifiant. Le
//! partage des rôles ne change pas : son outil va chercher la liste, Onzer la
//! compare.
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
use crate::identify::spotdl::{self, PlaylistTrack};
use crate::library::naming::normalize_key;
use crate::AppState;

/// Ce que la comparaison a trouvé.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistComparison {
    pub playlist_name: String,
    pub total: usize,
    pub present: usize,
    /// Les absents, dans l'ordre de la playlist.
    pub missing: Vec<PlaylistTrack>,
    /// Commande `spotdl`, quand son accès à Spotify fonctionne.
    pub command: String,
    /// Boucle `yt-dlp`, qui ne dépend d'aucun accès à Spotify.
    pub ytdlp_command: String,
}

/// Compare une liste collée à la main.
///
/// # Pourquoi cette porte existe
///
/// Aller chercher la liste chez Spotify s'est révélé le maillon fragile : l'API
/// est fermée sans abonnement payant, et le contournement anonyme de `spotdl`
/// dépend d'un analyseur des bundles JavaScript de Spotify qui cesse de
/// fonctionner du jour au lendemain — vérifié, il marchait le matin et plus
/// l'après-midi.
///
/// La comparaison, elle, ne dépend de personne. Accepter n'importe quelle
/// liste — un CSV exporté, un copier-coller, trois lignes écrites à la main —
/// rend la fonctionnalité indépendante d'un service tiers.
#[tauri::command]
pub async fn compare_playlist_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<PlaylistComparison> {
    compare(&state, &text, "Ta liste").await
}

/// Compare un fichier `.spotdl` à la bibliothèque.
#[tauri::command]
pub async fn compare_playlist_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<PlaylistComparison> {
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|error| OnzerError::Invalid(format!("fichier illisible : {error}")))?;

    compare(&state, &raw, "Playlist").await
}

/// Le rapprochement lui-même, commun aux deux portes d'entrée.
async fn compare(
    state: &State<'_, AppState>,
    raw: &str,
    fallback_name: &str,
) -> Result<PlaylistComparison> {
    let tracks = spotdl::parse_list(raw)?;
    let playlist_name =
        spotdl::playlist_name(raw).unwrap_or_else(|| fallback_name.to_string());
    let total = tracks.len();

    let mut missing = Vec::new();
    for track in tracks {
        let normalized_title = normalize_key(&track.title);
        let normalized_artist = track.main_artist().map(normalize_key);

        // Une durée absente écarte le critère plutôt que d'inventer une valeur
        // qui exclurait tous les morceaux d'une autre longueur.
        let duration = (track.duration_ms > 0).then_some(track.duration_ms);

        let found = repository::find_by_tags(
            &state.pool,
            &normalized_title,
            normalized_artist.as_deref(),
            duration,
        )
        .await?;

        if found.is_none() {
            missing.push(track);
        }
    }

    let (command, ytdlp_command) = download_commands(&missing, &playlist_name, state).await;

    Ok(PlaylistComparison {
        playlist_name,
        present: total - missing.len(),
        total,
        command,
        ytdlp_command,
        missing,
    })
}

/// Les commandes à coller pour récupérer les titres manquants.
///
/// # Le défaut corrigé
///
/// La première version passait par `xargs -a`. Cette option est une **extension
/// GNU** : la version BSD livrée avec macOS ne la connaît pas et répond
/// `xargs: invalid option -- a`. Une commande générée pour macOS par une
/// application macOS n'avait aucune excuse de supposer un `xargs` Linux.
///
/// `spotdl` accepte directement un fichier `.txt` en argument : le détour
/// n'avait même pas lieu d'être.
///
/// # Pourquoi deux commandes
///
/// | Outil | Ce qu'il fait | État |
/// |---|---|---|
/// | `spotdl` | Cherche et tague tout seul | Dépend du scraper Spotify, cassé par intermittence |
/// | `yt-dlp` | Aspire le son brut | Ne dépend de rien côté Spotify |
///
/// La seconde est devenue la voie principale : Onzer possède désormais sa
/// propre identification acoustique, et n'a plus besoin qu'un téléchargeur lui
/// apporte des métadonnées. Le nom de fichier est composé depuis la requête —
/// « Artiste - Titre.mp3 » — ce qui donne au filet de sécurité de l'ouvrier
/// exactement ce qu'il sait lire quand l'empreinte échoue.
async fn download_commands(
    missing: &[PlaylistTrack],
    playlist_name: &str,
    state: &State<'_, AppState>,
) -> (String, String) {
    if missing.is_empty() {
        return (String::new(), String::new());
    }

    let paths = state.paths.read().await.clone();
    let Some(root) = paths.library_root() else {
        return (String::new(), String::new());
    };

    let inbox = crate::ingest::inbox::inbox_path(root);
    if std::fs::create_dir_all(&inbox).is_err() {
        return (String::new(), String::new());
    }

    let list_path = inbox.join(format!(
        "_manquants — {}.txt",
        crate::library::naming::sanitize_segment(playlist_name)
            .unwrap_or_else(|| "playlist".to_string())
    ));

    let body: String = missing
        .iter()
        .map(|track| format!("{}\n", track.query))
        .collect();

    if std::fs::write(&list_path, body).is_err() {
        return (String::new(), String::new());
    }

    (
        spotdl_command(&list_path, &inbox),
        ytdlp_command(&list_path, &inbox),
    )
}

/// `spotdl` lit lui-même un fichier de requêtes, une par ligne.
fn spotdl_command(list_path: &std::path::Path, inbox: &std::path::Path) -> String {
    format!(
        "{} download {} --output {}",
        shell_quote(&spotdl_binary()),
        shell_quote(&list_path.display().to_string()),
        shell_quote(&format!(
            "{}/{{artists}} - {{title}}.{{output-ext}}",
            inbox.display()
        ))
    )
}

/// Boucle `yt-dlp`, qui ne dépend d'aucun accès à Spotify.
///
/// Le nom du fichier vient de la **requête**, pas du titre de la vidéo : les
/// titres YouTube sont bruités (« [Clip Officiel] », « prod. by … ») alors que
/// la requête est déjà propre. Cela donne « Artiste - Titre.mp3 », soit
/// exactement la forme que l'ouvrier d'identification sait relire.
///
/// Les barres obliques sont remplacées : un titre contenant « AC/DC » créerait
/// sinon un dossier au milieu du chemin.
fn ytdlp_command(list_path: &std::path::Path, inbox: &std::path::Path) -> String {
    format!(
        "while IFS= read -r q; do [ -n \"$q\" ] && {} -x --audio-format mp3 \
--embed-thumbnail --add-metadata -o \"{}/${{q//\\//-}}.%(ext)s\" \"ytsearch1:$q\"; done < {}",
        ytdlp_binary(),
        inbox.display(),
        shell_quote(&list_path.display().to_string())
    )
}

/// Chemin de `yt-dlp`, cherché aux mêmes endroits que `spotdl`.
fn ytdlp_binary() -> String {
    binary_path("yt-dlp", &[
        "/Library/Frameworks/Python.framework/Versions/3.14/bin/yt-dlp",
    ])
}

/// La commande qui produit le fichier à comparer.
///
/// `save` ne télécharge aucun audio : elle ne fait que rassembler les
/// métadonnées de la playlist. C'est l'étape que l'API Spotify ne nous laisse
/// plus faire nous-mêmes.
#[tauri::command]
pub async fn playlist_save_command(state: State<'_, AppState>, url: String) -> Result<String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(OnzerError::Invalid("colle d'abord un lien".to_string()));
    }

    let paths = state.paths.read().await.clone();
    let destination = match paths.library_root() {
        Some(root) => crate::ingest::inbox::inbox_path(root).join("playlist.spotdl"),
        None => std::path::PathBuf::from("playlist.spotdl"),
    };

    // Sans options d'identifiants : mesuré, elles ne changent rien.
    // `spotdl save` passe par un contournement anonyme qui les ignore, et
    // `--use-official-api` retombe sur le 403 de l'abonnement. Les afficher
    // exposerait le client secret à l'écran pour aucun bénéfice.
    let _ = &state;

    Ok(format!(
        "{} save {} --save-file {}",
        shell_quote(&spotdl_binary()),
        shell_quote(url),
        shell_quote(&destination.display().to_string())
    ))
}

/// Chemin de l'exécutable `spotdl`.
///
/// # Pourquoi un chemin absolu
///
/// `spotdl` s'installe le plus souvent via `pipx`, dans un dossier que le
/// `PATH` d'un shell non interactif ne contient pas toujours. Une commande qui
/// échoue sur « command not found » alors que l'outil est installé est la
/// pire des réponses : elle envoie chercher un problème qui n'existe pas.
///
/// On teste donc les emplacements habituels, du plus spécifique au plus
/// général, et l'on retombe sur le nom nu si aucun ne répond — auquel cas le
/// `PATH` de l'utilisateur fera son office.
fn spotdl_binary() -> String {
    binary_path("spotdl", &[])
}

/// Cherche un exécutable aux emplacements habituels d'installation.
///
/// `extra` permet d'ajouter un chemin propre à un outil — l'installation
/// Python encadrée de `yt-dlp`, par exemple.
fn binary_path(name: &str, extra: &[&str]) -> String {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    if let Some(home) = dirs_home() {
        candidates.push(home.join(".local/bin").join(name));
    }
    candidates.push(std::path::PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(std::path::PathBuf::from(format!("/usr/local/bin/{name}")));
    candidates.extend(extra.iter().map(std::path::PathBuf::from));

    for candidate in candidates {
        if candidate.is_file() {
            return candidate.display().to_string();
        }
    }

    name.to_string()
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Réglages : identifiants d'application Spotify.
pub const SPOTIFY_ID: &str = "spotify_client_id";
pub const SPOTIFY_SECRET: &str = "spotify_client_secret";

/// État des identifiants, sans jamais révéler le secret.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyStatus {
    pub configured: bool,
    /// Aperçu masqué de l'identifiant client, pour repérer un collage tronqué.
    pub id_hint: Option<String>,
}

#[tauri::command]
pub async fn spotify_status(state: State<'_, AppState>) -> Result<SpotifyStatus> {
    let id: Option<String> = settings::get(&state.pool, SPOTIFY_ID).await?;
    let secret: Option<String> = settings::get(&state.pool, SPOTIFY_SECRET).await?;

    let configured = id.as_deref().is_some_and(|value| !value.trim().is_empty())
        && secret.as_deref().is_some_and(|value| !value.trim().is_empty());

    Ok(SpotifyStatus {
        id_hint: id.as_deref().filter(|v| !v.is_empty()).map(mask),
        configured,
    })
}

/// `6847••••••` : de quoi reconnaître une clé, jamais de quoi la réutiliser.
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

/// Protège une valeur pour un shell POSIX.
///
/// Les titres contiennent des apostrophes — « L'été », « Rien qu'un rêve » —
/// et les chemins des espaces. Sans protection, la commande se briserait au
/// premier des deux.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Écrit une liste de requêtes dans le dossier de dépôt, et rend sa boucle.
///
/// # Pourquoi le même chemin que la comparaison
///
/// Les recommandations et les manquants d'une playlist posent la même
/// question — « comment je récupère tout ça ? ». Leur répondre différemment
/// obligerait l'utilisateur à réapprendre deux fois la même chose, et
/// dédoublerait la logique de nommage des fichiers.
#[tauri::command]
pub async fn export_queries(
    state: State<'_, AppState>,
    queries: Vec<String>,
    file_name: String,
) -> Result<ExportedList> {
    if queries.is_empty() {
        return Err(OnzerError::Invalid("rien à exporter".to_string()));
    }

    let paths = state.paths.read().await.clone();
    let root = paths
        .library_root()
        .ok_or_else(|| OnzerError::Invalid("aucune bibliothèque configurée".to_string()))?;

    let inbox = crate::ingest::inbox::inbox_path(root);
    std::fs::create_dir_all(&inbox)?;

    let safe_name = crate::library::naming::sanitize_segment(&file_name)
        .unwrap_or_else(|| "liste".to_string());
    let list_path = inbox.join(format!("{safe_name}.txt"));

    let body: String = queries
        .iter()
        .map(|query| format!("{}\n", query.trim()))
        .filter(|line| line.trim() != "")
        .collect();

    std::fs::write(&list_path, body)?;

    Ok(ExportedList {
        command: ytdlp_command(&list_path, &inbox),
        spotdl_command: spotdl_command(&list_path, &inbox),
        path: list_path.display().to_string(),
        count: queries.len(),
    })
}

/// Une liste écrite, avec de quoi la récupérer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedList {
    pub path: String,
    pub count: usize,
    /// Boucle `yt-dlp` : ne dépend d'aucun accès à Spotify.
    pub command: String,
    /// Variante `spotdl` : tague et récupère la pochette d'elle-même, quand son
    /// accès à Spotify fonctionne.
    pub spotdl_command: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protege_les_apostrophes_des_titres() {
        // Sans cela, « L'été » fermerait la citation et le shell interpréterait
        // la suite comme des commandes.
        assert_eq!(shell_quote("Damso - L'été"), r"'Damso - L'\''été'");
        assert_eq!(shell_quote("/Volumes/Ma musique"), "'/Volumes/Ma musique'");
    }
}

#[cfg(test)]
mod tests_commandes {
    use super::*;
    use std::path::Path;

    #[test]
    fn la_commande_spotdl_ne_passe_plus_par_xargs() {
        // `xargs -a` est une extension GNU : la version BSD de macOS répond
        // « invalid option -- a ». `spotdl` lit lui-même un fichier de
        // requêtes, le détour n'avait pas lieu d'être.
        let commande = spotdl_command(Path::new("/M/_Inbox/liste.txt"), Path::new("/M/_Inbox"));

        assert!(!commande.contains("xargs"), "{commande}");
        assert!(commande.contains("download '/M/_Inbox/liste.txt'"), "{commande}");
    }

    #[test]
    fn la_boucle_ytdlp_nomme_les_fichiers_depuis_la_requete() {
        // C'est ce qui donne « Artiste - Titre.mp3 », la forme exacte que le
        // filet de sécurité de l'ouvrier sait relire quand l'empreinte échoue.
        let commande = ytdlp_command(Path::new("/M/_Inbox/liste.txt"), Path::new("/M/_Inbox"));

        assert!(commande.contains("ytsearch1:$q"), "{commande}");
        assert!(commande.contains("${q//\\//-}"), "les barres obliques doivent être neutralisées");
        assert!(commande.contains("< '/M/_Inbox/liste.txt'"), "{commande}");
    }

    #[test]
    fn les_deux_commandes_visent_le_dossier_de_depot() {
        // C'est ce qui ferme la boucle : ce qui est récupéré atterrit là où
        // Onzer l'attend, et le rangement suit tout seul.
        for commande in [
            spotdl_command(Path::new("/M/l.txt"), Path::new("/M/_Inbox")),
            ytdlp_command(Path::new("/M/l.txt"), Path::new("/M/_Inbox")),
        ] {
            assert!(commande.contains("/M/_Inbox"), "{commande}");
        }
    }
}
