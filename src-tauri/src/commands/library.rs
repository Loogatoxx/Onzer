//! Commandes de gestion de la bibliothèque.
//!
//! Rappel de l'ADR-004 : aucune logique ici. Ces fonctions valident, délèguent
//! au module `library`, et convertissent le résultat pour le frontend.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::core::{OnzerError, Result};
use crate::db::{repository, settings};
use crate::library::{scanner, FileHandling};
use crate::AppState;

/// Nom de l'événement de progression écouté par l'interface.
pub const SCAN_PROGRESS_EVENT: &str = "library://scan-progress";

/// Enregistre la racine de bibliothèque choisie par l'utilisateur.
///
/// Le dossier est créé s'il n'existe pas encore : au premier lancement, la
/// bibliothèque est vide et son dossier n'a aucune raison d'exister déjà.
#[tauri::command]
pub async fn set_library_root(state: State<'_, AppState>, path: String) -> Result<()> {
    let root = PathBuf::from(&path);

    if root.exists() && !root.is_dir() {
        return Err(OnzerError::Invalid(format!(
            "« {path} » n'est pas un dossier"
        )));
    }

    std::fs::create_dir_all(&root)?;

    settings::set(&state.pool, settings::LIBRARY_ROOT, &path).await?;

    // Le nom du volume est conservé à titre de diagnostic : il permettra
    // d'afficher « Rebranche Lexar » plutôt qu'un chemin cryptique.
    if let Some(volume) = volume_name(&root) {
        settings::set(&state.pool, settings::LIBRARY_VOLUME, &volume).await?;
    }

    state.paths.write().await.set_library_root(Some(root));

    tracing::info!(racine = %path, "racine de bibliothèque définie");
    Ok(())
}

/// Extrait « Lexar » de « /Volumes/Lexar/Musique ».
fn volume_name(path: &std::path::Path) -> Option<String> {
    let mut components = path.components();
    if components.next()? != std::path::Component::RootDir {
        return None;
    }
    if components.next()?.as_os_str() != "Volumes" {
        return None;
    }
    Some(components.next()?.as_os_str().to_string_lossy().to_string())
}

/// Importe le contenu audio d'un dossier dans la bibliothèque.
///
/// Les fichiers sont **déplacés** et rangés selon l'ADR-007. La progression est
/// émise au fil de l'eau plutôt que retournée à la fin : sur plusieurs milliers
/// de titres, l'interface doit rester vivante.
#[tauri::command]
pub async fn import_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
) -> Result<scanner::ScanSummary> {
    let paths = state.paths.read().await.clone();

    if !paths.is_library_online() {
        return Err(OnzerError::LibraryOffline(
            paths
                .library_root()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "racine non configurée".to_string()),
        ));
    }

    let source = PathBuf::from(&folder);
    if !source.is_dir() {
        return Err(OnzerError::Invalid(format!(
            "« {folder} » n'est pas un dossier"
        )));
    }

    // Importer un dossier déjà situé dans la bibliothèque ne doit pas
    // réorganiser les fichiers, seulement les indexer.
    let handling = if paths.relativize(&source).is_ok() {
        FileHandling::IndexInPlace
    } else {
        FileHandling::Organize
    };

    let summary = scanner::import_folder(
        &state.pool,
        &paths,
        &source,
        handling,
        "scan",
        |progress| {
            // Un échec d'émission (fenêtre fermée) ne doit pas interrompre
            // l'import en cours.
            let _ = app.emit(SCAN_PROGRESS_EVENT, &progress);
        },
    )
    .await?;

    tracing::info!(
        importes = summary.imported,
        doublons = summary.duplicates,
        echecs = summary.failed,
        "import terminé"
    );

    Ok(summary)
}

#[tauri::command]
pub async fn list_tracks(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<repository::TrackSummary>> {
    repository::list_tracks(
        &state.pool,
        limit.unwrap_or(200).clamp(1, 1000),
        offset.unwrap_or(0).max(0),
    )
    .await
}

#[tauri::command]
pub async fn search_tracks(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<repository::TrackSummary>> {
    repository::search_tracks(&state.pool, &query, 200).await
}

#[tauri::command]
pub async fn library_counts(state: State<'_, AppState>) -> Result<repository::LibraryCounts> {
    repository::counts(&state.pool).await
}

/// Recontrôle la présence sur disque de chaque morceau référencé.
///
/// Ne supprime jamais rien : marque les absents comme indisponibles et
/// retourne leur nombre (ADR-006).
#[tauri::command]
pub async fn refresh_availability(state: State<'_, AppState>) -> Result<i64> {
    let paths = state.paths.read().await.clone();
    scanner::refresh_availability(&state.pool, &paths).await
}

/// Retourne la vignette de pochette encodée en *data URI*.
///
/// ⚠️ Provisoire. Encoder l'image en base64 dans une réponse JSON convient
/// à une liste, mais pas à une grille de plusieurs centaines de pochettes.
/// À remplacer par un protocole URI dédié lors de l'étape « interface ».
#[tauri::command]
pub async fn artwork_data_uri(state: State<'_, AppState>, hash: String) -> Result<Option<String>> {
    // Le nom de fichier venant de la base, on refuse tout ce qui n'est pas une
    // empreinte hexadécimale : un « ../ » ne doit pas pouvoir s'y glisser.
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(OnzerError::Invalid("empreinte de pochette invalide".into()));
    }

    let artwork_dir = state.paths.read().await.artwork_dir();
    let path = crate::library::artwork::thumbnail_path(&artwork_dir, &hash);

    if !path.is_file() {
        return Ok(None);
    }

    let bytes = std::fs::read(path)?;
    Ok(Some(format!("data:image/jpeg;base64,{}", encode_base64(&bytes))))
}

/// Encodage base64 standard, sans dépendance supplémentaire.
fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);

    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        output.push(ALPHABET[(triple >> 18 & 0x3F) as usize] as char);
        output.push(ALPHABET[(triple >> 12 & 0x3F) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6 & 0x3F) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrait_le_nom_du_volume() {
        assert_eq!(
            volume_name(std::path::Path::new("/Volumes/Lexar/Musique")).as_deref(),
            Some("Lexar")
        );
        assert_eq!(volume_name(std::path::Path::new("/Users/carlos/Musique")), None);
    }

    #[test]
    fn encode_en_base64_standard() {
        // Vecteurs de la RFC 4648.
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }
}
