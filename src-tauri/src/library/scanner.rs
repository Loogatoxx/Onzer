//! Parcours de dossiers et import en lot.
//!
//! Le scanner ne s'arrête jamais sur une erreur : un fichier corrompu au
//! milieu de 3 000 titres ne doit pas interrompre l'opération. Les échecs sont
//! comptés, journalisés en base, et rapportés à la fin.

use std::path::Path;

use sqlx::SqlitePool;
use walkdir::WalkDir;

use crate::core::{PathResolver, Result};
use crate::library::importer::{self, FileHandling, ImportOutcome};
use crate::library::naming::INBOX_DIR;

/// Progression émise pendant le parcours, à chaque fichier traité.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub processed: usize,
    pub total: usize,
    pub imported: usize,
    pub duplicates: usize,
    pub failed: usize,
    /// Nom du fichier en cours, pour l'affichage.
    pub current: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub total: usize,
    pub imported: usize,
    pub duplicates: usize,
    pub failed: usize,
    /// Les premières erreurs rencontrées, pour diagnostic. Volontairement
    /// plafonné : afficher 3 000 messages identiques n'aide personne.
    pub errors: Vec<String>,
}

const MAX_REPORTED_ERRORS: usize = 20;

/// Recense les fichiers audio importables d'un dossier, récursivement.
///
/// Le comptage est fait en amont de l'import pour pouvoir afficher une
/// progression sur un total connu plutôt qu'un compteur qui monte à l'aveugle.
pub fn collect_audio_files(folder: &Path) -> Vec<std::path::PathBuf> {
    WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        // Élague dès la descente les dossiers cachés — inutile d'explorer
        // « .git » ou « .Spotlight-V100 » — ainsi que le dépôt surveillé, dont
        // les fichiers peuvent être des téléchargements en cours.
        .filter_entry(|entry| {
            entry.depth() == 0
                || (!importer::should_skip(entry.path()) && !is_inbox(entry.path()))
        })
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| importer::is_importable(path))
        .collect()
}

fn is_inbox(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == INBOX_DIR)
}

/// Importe tout le contenu audio d'un dossier.
///
/// `on_progress` est appelé après chaque fichier : c'est ce qui alimente la
/// barre de progression de l'interface.
pub async fn import_folder<F>(
    pool: &SqlitePool,
    paths: &PathResolver,
    folder: &Path,
    handling: FileHandling,
    origin: &str,
    mut on_progress: F,
) -> Result<ScanSummary>
where
    F: FnMut(ScanProgress),
{
    let files = collect_audio_files(folder);
    let total = files.len();

    let mut summary = ScanSummary {
        total,
        ..Default::default()
    };

    for (index, file) in files.iter().enumerate() {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?")
            .to_string();

        match importer::import_file(pool, paths, file, handling, origin).await {
            Ok(ImportOutcome::Imported { .. }) => summary.imported += 1,
            Ok(ImportOutcome::Duplicate { reason, .. }) => {
                summary.duplicates += 1;
                tracing::debug!(fichier = %name, reason, "doublon ignoré");
            }
            Err(error) => {
                summary.failed += 1;
                if summary.errors.len() < MAX_REPORTED_ERRORS {
                    summary.errors.push(format!("{name} : {error}"));
                }
                tracing::warn!(fichier = %name, %error, "import échoué");
            }
        }

        on_progress(ScanProgress {
            processed: index + 1,
            total,
            imported: summary.imported,
            duplicates: summary.duplicates,
            failed: summary.failed,
            current: name,
        });
    }

    Ok(summary)
}

/// Vérifie quels morceaux référencés en base sont encore présents sur disque.
///
/// **Ne supprime jamais rien** (ADR-006) : un morceau introuvable passe à
/// `is_available = 0`, et redevient disponible au rebranchement du SSD.
/// Retourne le nombre de morceaux devenus indisponibles.
pub async fn refresh_availability(pool: &SqlitePool, paths: &PathResolver) -> Result<i64> {
    // Bibliothèque hors ligne : tout est indisponible, mais c'est temporaire
    // et il serait absurde de parcourir la base pour l'écrire.
    if !paths.is_library_online() {
        return Ok(0);
    }

    let rows: Vec<(i64, String, bool)> =
        sqlx::query_as("SELECT id, relative_path, is_available FROM tracks WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;

    let mut became_unavailable = 0_i64;

    for (id, relative_path, was_available) in rows {
        let exists = paths
            .resolve(&relative_path)
            .map(|absolute| absolute.is_file())
            .unwrap_or(false);

        if exists == was_available {
            continue; // rien à écrire
        }

        sqlx::query("UPDATE tracks SET is_available = ?, last_seen_at = ? WHERE id = ?")
            .bind(exists)
            .bind(crate::core::now_ms())
            .bind(id)
            .execute(pool)
            .await?;

        if !exists {
            became_unavailable += 1;
        }
    }

    Ok(became_unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ne_retient_que_les_fichiers_audio() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.mp3"), b"x").unwrap();
        fs::write(dir.path().join("b.flac"), b"x").unwrap();
        fs::write(dir.path().join("pochette.jpg"), b"x").unwrap();
        fs::write(dir.path().join("notes.txt"), b"x").unwrap();

        let found = collect_audio_files(dir.path());
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn descend_dans_les_sous_dossiers() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("Artiste/2001 - Album");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("01 - Titre.mp3"), b"x").unwrap();

        assert_eq!(collect_audio_files(dir.path()).len(), 1);
    }

    #[test]
    fn ignore_les_fichiers_appledouble_du_volume_exfat() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("morceau.mp3"), b"x").unwrap();
        fs::write(dir.path().join("._morceau.mp3"), b"attributs").unwrap();

        let found = collect_audio_files(dir.path());
        assert_eq!(found.len(), 1, "l'AppleDouble ne doit pas être importé");
        assert!(found[0].ends_with("morceau.mp3"));
    }

    #[test]
    fn nexplore_pas_les_dossiers_caches() {
        let dir = tempfile::tempdir().unwrap();
        let hidden = dir.path().join(".Trashes");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(hidden.join("supprime.mp3"), b"x").unwrap();

        assert!(collect_audio_files(dir.path()).is_empty());
    }

    #[test]
    fn nexplore_pas_le_depot_surveille() {
        // Les fichiers du dépôt peuvent être des téléchargements en cours :
        // les indexer donnerait des morceaux tronqués dans la bibliothèque.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("range.mp3"), b"x").unwrap();

        let inbox = dir.path().join(INBOX_DIR);
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("en-cours.mp3"), b"x").unwrap();

        let found = collect_audio_files(dir.path());

        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("range.mp3"));
    }

    #[test]
    fn accepte_un_dossier_vide() {
        let dir = tempfile::tempdir().unwrap();
        assert!(collect_audio_files(dir.path()).is_empty());
    }
}
