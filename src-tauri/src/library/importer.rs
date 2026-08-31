//! Import d'un fichier audio dans la bibliothèque.
//!
//! Déroulé, dans cet ordre précis :
//!
//! 1. empreinte de contenu → doublon strict détecté sans même lire les tags ;
//! 2. lecture des métadonnées, avec repli sur le nom de fichier ;
//! 3. seconde passe de dédoublonnage sur titre + durée ;
//! 4. calcul du chemin de rangement, avec désambiguïsation des collisions ;
//! 5. déplacement du fichier, journalisé pour rester annulable ;
//! 6. extraction de la vignette de pochette ;
//! 7. insertion en base, en une transaction.
//!
//! L'ordre n'est pas arbitraire : le hachage est bien moins coûteux que le
//! décodage des tags, et le déplacement n'intervient qu'une fois toutes les
//! vérifications passées — un doublon ne doit jamais faire bouger un fichier.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::core::{OnzerError, PathResolver, Result};
use crate::db::repository::{self, NewTrack};
use crate::library::metadata::MetadataHint;
use crate::library::{artwork, hash, metadata, naming};

#[derive(Debug)]
pub enum ImportOutcome {
    Imported {
        track_id: i64,
        relative_path: String,
    },
    /// Le morceau est déjà dans la bibliothèque. Le fichier source n'a **pas**
    /// été touché : à l'utilisateur de décider quoi en faire.
    Duplicate {
        existing_id: i64,
        reason: &'static str,
    },
}

/// Comment traiter le fichier sur le disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHandling {
    /// Déplacer le fichier et le ranger selon l'ADR-007.
    Organize,
    /// Le laisser où il est et se contenter de l'indexer. Utilisé au rescan
    /// d'une bibliothèque déjà rangée.
    IndexInPlace,
}

pub async fn import_file(
    pool: &SqlitePool,
    paths: &PathResolver,
    source: &Path,
    handling: FileHandling,
    origin: &str,
) -> Result<ImportOutcome> {
    import_file_with_hint(pool, paths, source, handling, origin, None).await
}

/// Import avec indications de métadonnées fournies par un script externe.
///
/// C'est la voie empruntée par le dossier surveillé et l'API locale : le
/// téléchargeur connaît souvent l'artiste et le titre par la page source, là où
/// le fichier obtenu n'a aucun tag exploitable.
pub async fn import_file_with_hint(
    pool: &SqlitePool,
    paths: &PathResolver,
    source: &Path,
    handling: FileHandling,
    origin: &str,
    hint: Option<&MetadataHint>,
) -> Result<ImportOutcome> {
    let source_display = source.display().to_string();

    match import_inner(pool, paths, source, handling, origin, hint).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            // Un échec est tracé en base : sans cela, un fichier qui refuse
            // obstinément de s'importer resterait invisible.
            let _ = repository::record_import(
                pool,
                &source_display,
                None,
                origin,
                "failed",
                None,
                Some(&error.to_string()),
            )
            .await;

            Err(error)
        }
    }
}

async fn import_inner(
    pool: &SqlitePool,
    paths: &PathResolver,
    source: &Path,
    handling: FileHandling,
    origin: &str,
    hint: Option<&MetadataHint>,
) -> Result<ImportOutcome> {
    // ── 1. Doublon strict ───────────────────────────────────────────────
    let content_hash = hash::content_hash(source)?;

    if let Some(existing_id) = repository::find_by_content_hash(pool, &content_hash).await? {
        return Ok(ImportOutcome::Duplicate {
            existing_id,
            reason: "fichier identique",
        });
    }

    // ── 2. Métadonnées ──────────────────────────────────────────────────
    let mut meta = metadata::read(source)?;

    if let Some(hint) = hint {
        hint.apply(&mut meta);
    }

    // ── 3. Doublon par tags ─────────────────────────────────────────────
    let normalized_title = naming::normalize_key(&meta.title);
    let normalized_artist = meta.filing_artist().map(naming::normalize_key);

    if let Some(existing_id) = repository::find_by_tags(
        pool,
        &normalized_title,
        normalized_artist.as_deref(),
        Some(meta.duration_ms),
    )
    .await?
    {
        return Ok(ImportOutcome::Duplicate {
            existing_id,
            reason: "même titre et même durée",
        });
    }

    // ── 4. Destination ──────────────────────────────────────────────────
    let file_metadata = std::fs::metadata(source)?;
    let file_size = file_metadata.len() as i64;
    let file_modified_at = file_metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as i64);

    let relative_path = match handling {
        FileHandling::Organize => {
            let desired = naming::build_relative_path(&naming::FilingInfo {
                filing_artist: meta.filing_artist(),
                album: meta.album.as_deref(),
                year: meta.year,
                track_no: meta.track_no,
                disc_no: meta.disc_no,
                title: &meta.title,
                extension: &meta.format,
            });

            let unique = resolve_collision(paths, &desired, source)?;
            move_into_library(paths, source, &unique)?;
            unique
        }
        FileHandling::IndexInPlace => paths.relativize(source)?,
    };

    // ── 5. Pochette ─────────────────────────────────────────────────────
    // Un échec ici ne doit pas faire échouer l'import : un morceau sans
    // pochette reste parfaitement écoutable.
    let artwork_hash = meta.artwork.as_ref().and_then(|art| {
        match artwork::store(&paths.artwork_dir(), &art.data) {
            Ok(digest) => Some(digest),
            Err(error) => {
                tracing::warn!(fichier = %source.display(), %error, "pochette ignorée");
                None
            }
        }
    });

    // ── 6. Base ─────────────────────────────────────────────────────────
    let track_id = repository::insert_track(
        pool,
        NewTrack {
            metadata: &meta,
            relative_path: &relative_path,
            file_size,
            content_hash: &content_hash,
            file_modified_at,
            artwork_hash: artwork_hash.as_deref(),
            source: origin,
        },
    )
    .await?;

    repository::record_import(
        pool,
        &source.display().to_string(),
        Some(&relative_path),
        if origin == "auto_import" { "api" } else { "manual" },
        "done",
        Some(track_id),
        None,
    )
    .await?;

    Ok(ImportOutcome::Imported {
        track_id,
        relative_path,
    })
}

/// Ajoute un suffixe « (2) », « (3) »… si le chemin est déjà pris.
///
/// Indispensable sur exFAT, **insensible à la casse** : deux morceaux nommés
/// « Intro » et « INTRO » dans le même album viseraient le même fichier, et le
/// second écraserait silencieusement le premier.
///
/// `source` est exclu de la détection : un fichier **déjà rangé au bon endroit**
/// n'entre pas en collision avec lui-même. Sans cette exception, réimporter une
/// bibliothèque déjà organisée renommerait chaque morceau en « … (2) ».
fn resolve_collision(paths: &PathResolver, desired: &str, source: &Path) -> Result<String> {
    if !is_taken(paths, desired, source)? {
        return Ok(desired.to_string());
    }

    let (stem, extension) = match desired.rsplit_once('.') {
        Some((stem, extension)) => (stem, extension),
        None => (desired, ""),
    };

    for suffix in 2..=99 {
        let candidate = if extension.is_empty() {
            format!("{stem} ({suffix})")
        } else {
            format!("{stem} ({suffix}).{extension}")
        };

        if !is_taken(paths, &candidate, source)? {
            return Ok(candidate);
        }
    }

    Err(OnzerError::Invalid(format!(
        "impossible de trouver un nom libre pour « {desired} »"
    )))
}

/// Le chemin est-il occupé par un **autre** fichier que la source ?
fn is_taken(paths: &PathResolver, candidate: &str, source: &Path) -> Result<bool> {
    let destination = paths.resolve(candidate)?;

    if !destination.exists() {
        return Ok(false);
    }

    // Comparaison canonique : le même fichier peut être désigné par deux
    // chemins différents (liens, « ./ », casse sur exFAT).
    let same_file = match (destination.canonicalize(), source.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => destination == source,
    };

    Ok(!same_file)
}

/// Déplace le fichier dans la bibliothèque, en créant l'arborescence.
fn move_into_library(paths: &PathResolver, source: &Path, relative_path: &str) -> Result<()> {
    let destination = paths.resolve(relative_path)?;

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // `rename` échoue avec EXDEV entre deux systèmes de fichiers — cas normal
    // ici, le fichier venant souvent du disque interne vers le SSD.
    match std::fs::rename(source, &destination) {
        Ok(()) => Ok(()),
        Err(_) => copy_then_remove(source, &destination),
    }
}

/// Repli inter-volumes. La copie est vérifiée **avant** de supprimer la
/// source : une copie tronquée ne doit jamais entraîner la perte de l'original.
fn copy_then_remove(source: &Path, destination: &Path) -> Result<()> {
    let copied = std::fs::copy(source, destination)?;
    let expected = std::fs::metadata(source)?.len();

    if copied != expected {
        let _ = std::fs::remove_file(destination);
        return Err(OnzerError::Invalid(format!(
            "copie incomplète : {copied} octets sur {expected} attendus"
        )));
    }

    std::fs::remove_file(source)?;
    Ok(())
}

/// Les fichiers à ignorer lors d'un parcours de dossier.
///
/// Le filtre sur « ._ » n'est pas cosmétique : sur exFAT, macOS crée un
/// « ._morceau.mp3 » à côté de chaque « morceau.mp3 ». Ces fichiers portent
/// une extension audio mais ne contiennent que des attributs étendus — sans ce
/// filtre, chaque morceau produirait un import fantôme en échec.
pub fn should_skip(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| name.starts_with("._") || name.starts_with('.'))
}

pub fn is_importable(path: &Path) -> bool {
    !should_skip(path) && metadata::is_supported_audio(path)
}

/// Chemin absolu d'un morceau, pour la lecture audio.
pub fn absolute_path(paths: &PathResolver, relative_path: &str) -> Result<PathBuf> {
    paths.resolve(relative_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_les_fichiers_appledouble() {
        assert!(should_skip(Path::new("/x/._morceau.mp3")));
        assert!(!should_skip(Path::new("/x/morceau.mp3")));
    }

    #[test]
    fn ignore_les_fichiers_caches() {
        assert!(should_skip(Path::new("/x/.DS_Store")));
    }

    #[test]
    fn ne_retient_que_les_fichiers_audio_reels() {
        assert!(is_importable(Path::new("/x/morceau.mp3")));
        assert!(
            !is_importable(Path::new("/x/._morceau.mp3")),
            "un AppleDouble porte pourtant bien l'extension .mp3"
        );
        assert!(!is_importable(Path::new("/x/pochette.jpg")));
    }
}
