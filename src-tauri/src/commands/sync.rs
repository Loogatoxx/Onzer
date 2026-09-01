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
use crate::db::repository;
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
    /// Commande prête à coller dans un terminal.
    pub command: String,
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

    let tracks = spotdl::parse(&raw)?;
    let playlist_name = spotdl::playlist_name(&raw).unwrap_or_else(|| "Playlist".to_string());
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

    let command = download_command(&missing, &playlist_name, &state).await;

    Ok(PlaylistComparison {
        playlist_name,
        present: total - missing.len(),
        total,
        command,
        missing,
    })
}

/// La commande à coller pour récupérer les seuls titres manquants.
///
/// # Pourquoi un fichier et `xargs`
///
/// Sur la playlist de test, 484 titres manquaient. Écrite d'un bloc, la
/// commande aurait fait une vingtaine de kilo-octets — illisible, et proche des
/// limites de longueur d'un shell. Les requêtes partent donc dans un fichier,
/// qu'`xargs` découpe lui-même autant que nécessaire.
///
/// Les requêtes sont en clair — « Artiste - Titre » — plutôt qu'en liens :
/// `spotdl` accepte les deux, et un fichier lisible permet de vérifier, voire
/// d'élaguer à la main, ce qu'on s'apprête à lancer.
///
/// La sortie vise le **dossier de dépôt** : ce qui est récupéré atterrit là où
/// Onzer l'attend, et le reste — dédoublonnage, identification, rangement — se
/// fait tout seul.
async fn download_command(
    missing: &[PlaylistTrack],
    playlist_name: &str,
    state: &State<'_, AppState>,
) -> String {
    if missing.is_empty() {
        return String::new();
    }

    let paths = state.paths.read().await.clone();
    let Some(root) = paths.library_root() else {
        return String::new();
    };

    let inbox = crate::ingest::inbox::inbox_path(root);
    if std::fs::create_dir_all(&inbox).is_err() {
        return String::new();
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
        return String::new();
    }

    format!(
        "xargs -a {} -d '\\n' spotdl download --output {}",
        shell_quote(&list_path.display().to_string()),
        shell_quote(&format!(
            "{}/{{artists}} - {{title}}.{{output-ext}}",
            inbox.display()
        ))
    )
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

    Ok(format!(
        "spotdl save {} --save-file {}",
        shell_quote(url),
        shell_quote(&destination.display().to_string())
    ))
}

/// Protège une valeur pour un shell POSIX.
///
/// Les titres contiennent des apostrophes — « L'été », « Rien qu'un rêve » —
/// et les chemins des espaces. Sans protection, la commande se briserait au
/// premier des deux.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
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
