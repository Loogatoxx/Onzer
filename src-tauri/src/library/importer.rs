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
use crate::library::{artwork, audio_hash, hash, metadata, naming};

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
    /// Le morceau existait, mais son fichier avait disparu : celui-ci reprend
    /// la place.
    ///
    /// # Pourquoi ce n'est pas un doublon
    ///
    /// Un morceau hors ligne garde sa ligne, son historique, ses favoris et sa
    /// place dans les playlists — seul le fichier manque. Le retéléchargement
    /// arrivait avec les mêmes tags et se faisait écarter comme doublon : le
    /// morceau restait grisé, et son fichier finissait dans `_Doublons` sans
    /// que rien ne puisse plus les rapprocher. Quatre cent sept fichiers s'y
    /// étaient accumulés.
    ///
    /// Le morceau ne perd donc rien de ce qu'il était : il **retrouve son
    /// fichier**.
    Restored {
        track_id: i64,
        relative_path: String,
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

/// Le fichier de ce morceau a-t-il disparu du disque ?
///
/// La question est posée au **disque**, pas à la colonne `is_available` : cette
/// dernière ne vaut que ce que vaut le dernier balayage, et se tromper ici
/// écraserait le chemin d'un morceau parfaitement présent.
async fn file_missing(pool: &SqlitePool, paths: &PathResolver, track_id: i64) -> bool {
    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ? AND deleted_at IS NULL")
            .bind(track_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    match relative_path {
        Some(relative_path) => match absolute_path(paths, &relative_path) {
            Ok(path) => !path.is_file(),
            Err(_) => true,
        },
        // Ligne supprimée ou introuvable : ce n'est pas un morceau à réparer.
        None => false,
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
    //
    // Deux empreintes, dans cet ordre, parce qu'elles ne rattrapent pas la
    // même chose :
    //
    // * `audio_hash` ne couvre que les octets audio. C'est la seule qui
    //   survive à une réécriture des tags — et donc la seule capable de
    //   reconnaître un fichier qu'Onzer a lui-même retagué après
    //   identification. Son absence est ce qui a laissé entrer trois
    //   exemplaires du même morceau.
    // * `content_hash` couvre le fichier entier. Elle reste utile là où
    //   l'audio ne peut pas être isolé — un OGG, par exemple.
    let content_hash = hash::content_hash(source)?;
    let audio_hash = audio_hash::audio_hash(source)?;

    // Un doublon dont le fichier a disparu n'est pas un doublon : c'est un
    // morceau qui attend le sien. `restore` retient ce cas et laisse l'import
    // suivre son cours jusqu'au rangement.
    let mut restore = None;

    if let Some(existing_id) = repository::find_by_audio_hash(pool, &audio_hash).await? {
        if !file_missing(pool, paths, existing_id).await {
            return Ok(ImportOutcome::Duplicate {
                existing_id,
                reason: "même audio, tags différents",
            });
        }
        restore = Some(existing_id);
    }

    if restore.is_none() {
        if let Some(existing_id) = repository::find_by_content_hash(pool, &content_hash).await? {
            if !file_missing(pool, paths, existing_id).await {
                return Ok(ImportOutcome::Duplicate {
                    existing_id,
                    reason: "fichier identique",
                });
            }
            restore = Some(existing_id);
        }
    }

    // ── 2. Métadonnées ──────────────────────────────────────────────────
    let mut meta = metadata::read(source)?;

    if let Some(hint) = hint {
        hint.apply(&mut meta);
    }

    // ── 3. Doublon par tags ─────────────────────────────────────────────
    let normalized_title = naming::normalize_key(&meta.title);
    let normalized_artist = meta.filing_artist().map(naming::normalize_key);

    if restore.is_none() {
        if let Some(existing_id) = repository::find_by_tags(
            pool,
            &normalized_title,
            normalized_artist.as_deref(),
            Some(meta.duration_ms),
        )
        .await?
        {
            if !file_missing(pool, paths, existing_id).await {
                return Ok(ImportOutcome::Duplicate {
                    existing_id,
                    reason: "même titre et même durée",
                });
            }
            restore = Some(existing_id);
        }
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
    if let Some(track_id) = restore {
        repository::reattach_file(
            pool,
            track_id,
            &relative_path,
            file_size,
            &content_hash,
            &audio_hash,
            file_modified_at,
        )
        .await?;

        return Ok(ImportOutcome::Restored {
            track_id,
            relative_path,
        });
    }

    let track_id = repository::insert_track(
        pool,
        NewTrack {
            metadata: &meta,
            relative_path: &relative_path,
            file_size,
            content_hash: &content_hash,
            audio_hash: &audio_hash,
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
pub(crate) fn resolve_collision(
    paths: &PathResolver,
    desired: &str,
    source: &Path,
) -> Result<String> {
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
pub(crate) fn move_into_library(
    paths: &PathResolver,
    source: &Path,
    relative_path: &str,
) -> Result<()> {
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

// ════════════════════════════════════════════════════════════════════════════
//  Rangement des morceaux devenus sans album
// ════════════════════════════════════════════════════════════════════════════

/// Nom de la pochette déposée à côté d'un album par `identify::tagger`.
const ALBUM_COVER_FILE: &str = "cover.jpg";

/// Range un morceau sans album sous `Artiste/Singles/`.
///
/// # Quand cela sert
///
/// Un album de compilation attribué à tort a laissé le fichier dans un dossier
/// portant son nom — « Damso/2009 - I migliori anni '90/ » pour un morceau de
/// 2017. Effacer l'album en base ne suffit pas : le disque continue d'affirmer
/// le contraire, et l'utilisateur qui l'ouvre dans le Finder le voit.
///
/// Retourne le nouveau chemin relatif, ou `None` si rien n'a bougé — auquel cas
/// l'ancien reste valable.
pub fn refile_without_album(paths: &PathResolver, path: &Path) -> Option<String> {
    let meta = metadata::read(path).ok()?;

    let desired = naming::build_relative_path(&naming::FilingInfo {
        filing_artist: meta.filing_artist(),
        album: None,
        year: None,
        track_no: None,
        disc_no: None,
        title: &meta.title,
        extension: &meta.format,
    });

    if paths.relativize(path).ok().as_deref() == Some(desired.as_str()) {
        return None; // déjà à sa place
    }

    let unique = resolve_collision(paths, &desired, path).ok()?;
    move_into_library(paths, path, &unique).ok()?;

    if let Some(parent) = path.parent() {
        prune_album_dir(parent);
    }

    Some(unique)
}

/// Supprime un dossier d'album vidé, et la pochette qu'Onzer y avait écrite.
///
/// On n'y retire que ce qu'Onzer a lui-même déposé. Tout autre fichier arrête
/// le nettoyage : ce n'est pas à lui d'en décider.
fn prune_album_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();

        // Les fichiers de service de macOS ne comptent pas comme du contenu.
        if name == ALBUM_COVER_FILE || name == ".DS_Store" || name.starts_with("._") {
            continue;
        }

        return; // il reste du contenu : on ne touche à rien
    }

    // `remove_dir_all` est ici sans danger : on vient de vérifier qu'il ne
    // reste que ce qu'Onzer a déposé.
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn base() -> (tempfile::TempDir, SqlitePool, PathResolver) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::connect(&dir.path().join("onzer.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        let root = dir.path().join("Musique");
        std::fs::create_dir_all(&root).unwrap();

        let mut paths = PathResolver::new(dir.path().join("data"));
        paths.set_library_root(Some(root));
        (dir, pool, paths)
    }

    #[tokio::test]
    async fn un_morceau_sans_fichier_est_reconnu_comme_tel() {
        // C'est la question qui décide entre « doublon » et « retrouvailles ».
        let (_dir, pool, paths) = base().await;

        let track_id: i64 = sqlx::query_scalar(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at, source)
             VALUES ('Été avec toi','ete avec toi',200000,'Adele/01.mp3',1,'h','mp3',0,'scan')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(
            file_missing(&pool, &paths, track_id).await,
            "le fichier n'existe pas sur le disque"
        );

        // Le même morceau, fichier présent cette fois.
        let root = paths.library_root().unwrap();
        std::fs::create_dir_all(root.join("Adele")).unwrap();
        std::fs::write(root.join("Adele/01.mp3"), b"audio").unwrap();

        assert!(!file_missing(&pool, &paths, track_id).await);
    }

    #[tokio::test]
    async fn la_question_est_posee_au_disque_pas_a_la_colonne() {
        // `is_available` ne vaut que ce que vaut le dernier balayage : s'y
        // fier écraserait le chemin d'un morceau parfaitement présent.
        let (_dir, pool, paths) = base().await;
        let root = paths.library_root().unwrap().to_path_buf();

        std::fs::create_dir_all(root.join("Adele")).unwrap();
        std::fs::write(root.join("Adele/01.mp3"), b"audio").unwrap();

        let track_id: i64 = sqlx::query_scalar(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at, source, is_available)
             VALUES ('x','x',1,'Adele/01.mp3',1,'h','mp3',0,'scan',0)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(
            !file_missing(&pool, &paths, track_id).await,
            "la colonne dit absent, le disque dit présent : c'est le disque qui tranche"
        );
    }

    #[tokio::test]
    async fn le_morceau_garde_son_identite_en_retrouvant_son_fichier() {
        // Ce qui distingue des retrouvailles d'un nouvel import : la ligne, son
        // historique et ses playlists survivent.
        let (_dir, pool, _paths) = base().await;

        let track_id: i64 = sqlx::query_scalar(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at, source,
                                 is_available, is_loved, lyrics)
             VALUES ('Été avec toi','ete avec toi',200000,'ancien.mp3',1,'vieux','mp3',0,'scan',
                     0, 1, '[00:01.00]x')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        crate::db::repository::reattach_file(
            &pool,
            track_id,
            "Adèle Castillon/01 - Été avec toi.mp3",
            4242,
            "neuf",
            "audio-neuf",
            Some(1),
        )
        .await
        .unwrap();

        let (chemin, dispo, aime, paroles): (String, bool, bool, Option<String>) =
            sqlx::query_as("SELECT relative_path, is_available, is_loved, lyrics FROM tracks WHERE id = ?")
                .bind(track_id)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(chemin, "Adèle Castillon/01 - Été avec toi.mp3");
        assert!(dispo, "le morceau redevient jouable");
        assert!(aime, "le favori ne se perd pas");
        assert_eq!(paroles.as_deref(), Some("[00:01.00]x"), "les paroles non plus");
    }

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
