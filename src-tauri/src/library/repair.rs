//! Remise en état d'une bibliothèque abîmée par un défaut corrigé depuis.
//!
//! # Pourquoi ce module existe
//!
//! Corriger un défaut ne répare pas ce qu'il a déjà cassé. Trois exemplaires du
//! même morceau étaient entrés en base avant que le dédoublonnage sache
//! survivre à une réécriture de tags : le nouveau code les empêche d'entrer, il
//! ne les fait pas sortir.
//!
//! Deux passes, exécutées au démarrage :
//!
//! 1. **Rattrapage** — calculer l'empreinte audio des morceaux importés avant
//!    qu'elle existe. Sans elle, le nouveau filet est aveugle sur toute la
//!    bibliothèque historique.
//! 2. **Fusion** — regrouper les lignes qui désignent le même audio, garder la
//!    plus ancienne, effacer les autres.
//! 3. **Récupération des tags d'origine** — les exemplaires écartés n'ont
//!    jamais été retagués par Onzer. Ils portent donc encore ce que le fichier
//!    annonçait avant identification, et c'est la seule chance de retrouver ce
//!    qu'était un morceau mal identifié.
//!
//! # Ce qu'on garde, et pourquoi
//!
//! On conserve **la plus ancienne** ligne : c'est celle que l'historique
//! d'écoute référence, et `play_events` la protège en `ON DELETE RESTRICT`. Les
//! doublons, eux, sont récents et n'ont rien accumulé. Le cas contraire — un
//! doublon qui aurait été écouté — est traité en le marquant supprimé plutôt
//! qu'en le détruisant : perdre un historique pour faire le ménage serait un
//! remède pire que le mal.

use sqlx::SqlitePool;

use crate::core::{PathResolver, Result};
use crate::library::{audio_hash, importer};

/// Ce que la remise en état a changé.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RepairReport {
    /// Morceaux ayant reçu leur empreinte audio.
    pub hashed: u64,
    /// Lignes en doublon retirées de la bibliothèque.
    pub merged: u64,
    /// Fichiers en doublon écartés du disque.
    pub files_set_aside: u64,
    /// Morceaux dont les tags d'origine ont pu être retrouvés.
    pub originals_recovered: u64,
    /// Morceaux sans album sortis d'un dossier d'album.
    pub refiled: u64,
    /// Lignes d'index de recherche remises en accord avec la base.
    pub reindexed: u64,
}

impl RepairReport {
    pub fn is_empty(&self) -> bool {
        self.hashed == 0
            && self.merged == 0
            && self.files_set_aside == 0
            && self.originals_recovered == 0
            && self.refiled == 0
            && self.reindexed == 0
    }
}

/// Rattrape les empreintes manquantes, puis fusionne les doublons.
pub async fn run(pool: &SqlitePool, paths: &PathResolver) -> Result<RepairReport> {
    let hashed = backfill_audio_hashes(pool, paths).await?;
    let (merged, files_set_aside) = merge_duplicates(pool, paths).await?;
    let originals_recovered = recover_original_tags(pool, paths).await?;
    let refiled = refile_albumless(pool, paths).await?;
    let reindexed = resync_search_index(pool).await?;

    Ok(RepairReport {
        hashed,
        merged,
        files_set_aside,
        originals_recovered,
        refiled,
        reindexed,
    })
}

/// Remet l'index de recherche en accord avec la base.
///
/// # Le défaut que cette passe répare, après coup
///
/// L'index FTS5 est une **copie** : titre, artistes et album y sont dupliqués
/// pour être indexés. Rattacher une pochette, compléter un album ou détacher
/// une compilation changeait la base sans toucher à cette copie.
///
/// Mesuré sur la bibliothèque : **113 morceaux sur 590** étaient trouvables
/// sous un album qu'ils n'avaient plus — « Macarena » répondait encore à
/// « I migliori anni '90 » et restait introuvable sous son vrai album. Un
/// index qui répond à côté est pire qu'un index vide : on croit avoir cherché.
///
/// Corriger les chemins d'écriture ne répare pas ce qu'ils ont déjà écrit,
/// d'où cette passe — comme pour la révision des albums (ADR-032).
async fn resync_search_index(pool: &SqlitePool) -> Result<u64> {
    // Seules les lignes réellement divergentes sont réécrites : réindexer
    // toute la bibliothèque à chaque démarrage coûterait cher pour rien, et
    // rendrait le rapport de remise en état illisible — il annoncerait 590
    // réparations là où il n'y a rien à réparer.
    let drifted: Vec<i64> = sqlx::query_scalar(
        "SELECT t.id
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
           JOIN tracks_fts f ON f.track_id = t.id
          WHERE t.deleted_at IS NULL
            AND (
                 IFNULL(f.album_title, '') <> IFNULL(al.title, '')
              OR f.title <> t.title
            )",
    )
    .fetch_all(pool)
    .await?;

    if drifted.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    for track_id in &drifted {
        crate::db::repository::reindex_track(&mut tx, *track_id).await?;
    }
    tx.commit().await?;

    Ok(drifted.len() as u64)
}

/// Range les morceaux sans album restés dans un dossier d'album.
///
/// Un album de compilation attribué à tort a été effacé en base, mais le
/// fichier, lui, dormait toujours dans un dossier portant son nom : la base et
/// le disque se contredisaient.
///
/// La règle est étroite à dessein. Seuls les morceaux **sans album** rangés
/// ailleurs que dans `Singles` ou `_À trier` sont concernés — déplacer plus
/// largement reviendrait à réorganiser la bibliothèque sans qu'on l'ait
/// demandé.
async fn refile_albumless(pool: &SqlitePool, paths: &PathResolver) -> Result<u64> {
    let orphans: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM tracks
          WHERE album_id IS NULL AND deleted_at IS NULL
            AND relative_path NOT LIKE '%/' || ?1 || '/%'
            AND relative_path NOT LIKE ?2 || '/%'",
    )
    .bind(crate::library::naming::SINGLES_DIR)
    .bind(crate::library::naming::UNSORTED_DIR)
    .fetch_all(pool)
    .await?;

    let mut moved = 0;
    for (track_id, relative_path) in orphans {
        let Ok(path) = importer::absolute_path(paths, &relative_path) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }

        let Some(destination) = importer::refile_without_album(paths, &path) else {
            continue;
        };

        sqlx::query("UPDATE tracks SET relative_path = ? WHERE id = ?")
            .bind(&destination)
            .bind(track_id)
            .execute(pool)
            .await?;

        moved += 1;
    }

    Ok(moved)
}

/// Retrouve les tags d'origine dans les exemplaires écartés.
///
/// # Pourquoi c'est possible
///
/// Onzer ne retague que les fichiers qu'il a rangés. Les exemplaires écartés
/// dans `_Doublons` — qu'ils viennent du dépôt ou d'une fusion — n'ont jamais
/// été touchés : ils portent encore ce que le fichier annonçait au
/// téléchargement. Rapprochés par leur empreinte audio, ils rendent aux
/// morceaux mal identifiés leur mémoire.
///
/// C'est ainsi qu'un fichier devenu « carmen (Clip Officiel) » de Stromae a pu
/// être reconnu pour ce qu'il était : « Θ. Macarena », de Damso.
///
/// # Toutes les copies ne se valent pas
///
/// Deux provenances aboutissent dans `_Doublons`, et une seule est fiable :
///
/// | Emplacement | Provenance | Tags |
/// |---|---|---|
/// | `_Inbox/_Doublons` | Téléchargement écarté à l'entrée | **Intacts** |
/// | `Artiste/Album/_Doublons` | Exemplaire de bibliothèque fusionné | Déjà réécrits par Onzer |
///
/// Le second ne sait que répéter ce qu'Onzer a écrit. Un premier passage l'a
/// appris à ses dépens : le fichier « carmen » s'est vu attribuer comme tags
/// d'origine… « carmen » de Stromae. Les copies du dépôt sont donc traitées en
/// dernier et **écrasent** ce qu'une copie de bibliothèque aurait déposé.
async fn recover_original_tags(pool: &SqlitePool, paths: &PathResolver) -> Result<u64> {
    let Some(root) = paths.library_root() else {
        return Ok(0);
    };

    // Tous les morceaux, et non les seuls dépourvus de mémoire.
    //
    // Le premier jet filtrait sur `original_title IS NULL`, ce qui rendait la
    // correction impossible : une copie de bibliothèque ayant déposé une
    // mauvaise valeur, la ligne sortait du champ de recherche et la copie du
    // dépôt, elle, n'était plus jamais consultée. Le filtre verrouillait
    // exactement l'erreur qu'il fallait défaire.
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, audio_hash FROM tracks
          WHERE audio_hash IS NOT NULL AND deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let known: std::collections::HashMap<String, i64> =
        rows.into_iter().map(|(id, hash)| (hash, id)).collect();

    let root = root.to_path_buf();
    let mut candidates = tokio::task::spawn_blocking(move || collect_set_aside(&root))
        .await
        .unwrap_or_default();

    // Les copies du dépôt en dernier : elles doivent avoir le dernier mot.
    candidates.sort_by_key(|path| from_inbox(path));

    let mut recovered = 0;
    for path in candidates {
        let authoritative = from_inbox(&path);
        let for_hash = path.clone();
        let Ok(Ok(hash)) =
            tokio::task::spawn_blocking(move || audio_hash::audio_hash(&for_hash)).await
        else {
            continue;
        };

        let Some(&track_id) = known.get(&hash) else {
            continue;
        };

        let Ok(Ok(meta)) =
            tokio::task::spawn_blocking(move || crate::library::metadata::read(&path)).await
        else {
            continue;
        };

        // Une copie de bibliothèque ne comble qu'un vide ; une copie du dépôt
        // corrige aussi ce qu'une copie de bibliothèque aurait mal rempli.
        // La dernière clause évite de recompter à chaque démarrage ce qui n'a
        // pas bougé : sans elle, le rapport annoncerait éternellement du
        // travail accompli.
        let updated = sqlx::query(
            "UPDATE tracks
                SET original_title = ?1, original_artist = ?2, original_album = ?3
              WHERE id = ?4
                AND (?5 OR original_title IS NULL)
                AND (original_title IS NULL
                     OR original_title <> ?1
                     OR COALESCE(original_artist, '') <> COALESCE(?2, ''))",
        )
        .bind(&meta.title)
        .bind(meta.filing_artist())
        .bind(meta.album.as_deref())
        .bind(track_id)
        .bind(authoritative)
        .execute(pool)
        .await?;

        if updated.rows_affected() > 0 {
            recovered += 1;
        }
    }

    Ok(recovered)
}

/// La copie vient-elle du dossier de dépôt ?
///
/// `_Inbox/_Doublons/…` : jamais passée entre les mains d'Onzer, donc seule à
/// porter des tags dignes de foi.
fn from_inbox(path: &std::path::Path) -> bool {
    path.components()
        .any(|part| part.as_os_str() == crate::library::naming::INBOX_DIR)
}

/// Tous les fichiers audio dormant dans un dossier `_Doublons`.
fn collect_set_aside(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|name| name == crate::library::naming::INBOX_DUPLICATES_DIR)
                && importer::is_importable(&path)
                && !importer::should_skip(&path)
            {
                found.push(path);
            }
        }
    }

    found
}

/// Calcule l'empreinte audio des morceaux qui n'en ont pas.
///
/// Un fichier absent n'est pas une erreur : le SSD peut être débranché, ou le
/// morceau déplacé à la main. On passe, et le rattrapage se fera au prochain
/// démarrage.
async fn backfill_audio_hashes(pool: &SqlitePool, paths: &PathResolver) -> Result<u64> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM tracks
          WHERE audio_hash IS NULL AND deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let mut done = 0;
    for (track_id, relative_path) in rows {
        let Ok(path) = importer::absolute_path(paths, &relative_path) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }

        // Lecture disque : sur un fil bloquant, comme partout ailleurs.
        let Ok(Ok(hash)) =
            tokio::task::spawn_blocking(move || audio_hash::audio_hash(&path)).await
        else {
            continue;
        };

        sqlx::query("UPDATE tracks SET audio_hash = ? WHERE id = ?")
            .bind(&hash)
            .bind(track_id)
            .execute(pool)
            .await?;

        done += 1;
    }

    Ok(done)
}

/// Fusionne les lignes qui désignent le même audio.
///
/// Retourne le nombre de lignes retirées et de fichiers écartés.
async fn merge_duplicates(pool: &SqlitePool, paths: &PathResolver) -> Result<(u64, u64)> {
    // Les doublons, groupés, l'original d'abord. `min(id)` désigne le plus
    // ancien : celui que l'historique d'écoute référence.
    let groups: Vec<(String,)> = sqlx::query_as(
        "SELECT audio_hash FROM tracks
          WHERE audio_hash IS NOT NULL AND deleted_at IS NULL
          GROUP BY audio_hash HAVING COUNT(*) > 1",
    )
    .fetch_all(pool)
    .await?;

    let mut merged = 0;
    let mut set_aside = 0;

    for (hash,) in groups {
        let copies: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, relative_path FROM tracks
              WHERE audio_hash = ? AND deleted_at IS NULL
              ORDER BY id",
        )
        .bind(&hash)
        .fetch_all(pool)
        .await?;

        // La première est l'originale : on ne touche ni à sa ligne ni à son
        // fichier.
        for (track_id, relative_path) in copies.into_iter().skip(1) {
            if remove_duplicate(pool, track_id).await? {
                merged += 1;
            }

            if let Ok(path) = importer::absolute_path(paths, &relative_path) {
                if path.is_file() && set_aside_file(&path).is_ok() {
                    set_aside += 1;
                }
            }
        }
    }

    Ok((merged, set_aside))
}

/// Retire un doublon de la bibliothèque.
///
/// Suppression franche s'il n'a jamais été écouté, marquage sinon. La
/// distinction n'est pas cosmétique : `play_events` référence `tracks` en
/// `ON DELETE RESTRICT`, et un doublon écouté emporterait son historique.
async fn remove_duplicate(pool: &SqlitePool, track_id: i64) -> Result<bool> {
    let plays: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_events WHERE track_id = ?")
        .bind(track_id)
        .fetch_one(pool)
        .await?;

    if plays == 0 {
        // Les tables liées sont en cascade ; l'index de recherche aussi.
        sqlx::query("DELETE FROM tracks WHERE id = ?")
            .bind(track_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("UPDATE tracks SET deleted_at = ? WHERE id = ?")
            .bind(crate::core::now_ms())
            .bind(track_id)
            .execute(pool)
            .await?;
    }

    Ok(true)
}

/// Déplace un fichier en doublon dans `_Doublons`, à la racine de la bibliothèque.
///
/// Jamais de suppression : l'utilisateur reste seul juge de ce qu'il jette.
fn set_aside_file(path: &std::path::Path) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("fichier sans dossier parent"));
    };

    let aside = parent.join(crate::library::naming::INBOX_DUPLICATES_DIR);
    std::fs::create_dir_all(&aside)?;

    let name = path.file_name().unwrap_or_default();
    let mut destination = aside.join(name);

    if destination.exists() {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let extension = path
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();

        for index in 2..1000 {
            let candidate = aside.join(format!("{stem} ({index}){extension}"));
            if !candidate.exists() {
                destination = candidate;
                break;
            }
        }
    }

    std::fs::rename(path, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::library::metadata::TrackMetadata;
    use std::io::Write;

    /// Bibliothèque de test : une base, une racine, des fichiers réels.
    async fn bibliotheque() -> (tempfile::TempDir, SqlitePool, PathResolver) {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::connect(&dir.path().join("onzer.db")).await.unwrap();
        db::migrate(&pool).await.unwrap();

        let root = dir.path().join("Musique");
        std::fs::create_dir_all(&root).unwrap();

        let mut paths = PathResolver::new(dir.path().join("data"));
        paths.set_library_root(Some(root));
        (dir, pool, paths)
    }

    /// Écrit un MP3 factice : un en-tête ID3v2 de la taille demandée, puis
    /// toujours le même audio. Deux tailles de tags différentes donnent donc
    /// deux fichiers d'octets différents, mais le même audio.
    fn ecrire(paths: &PathResolver, relative: &str, tag_size: usize) -> String {
        let path = paths.resolve(relative).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let mut octets = vec![b'I', b'D', b'3', 4, 0, 0];
        let size = tag_size as u32;
        octets.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        octets.extend(std::iter::repeat_n(0x33, tag_size));
        octets.extend((0..5000).map(|i| (i % 251) as u8));

        std::fs::File::create(&path).unwrap().write_all(&octets).unwrap();
        relative.to_string()
    }

    async fn inserer(pool: &SqlitePool, paths: &PathResolver, relative: &str) -> i64 {
        let meta = TrackMetadata {
            title: "Titre".to_string(),
            artists: vec!["Artiste".to_string()],
            featured_artists: Vec::new(),
            album_artist: None,
            album: None,
            track_no: None,
            disc_no: None,
            year: None,
            genres: Vec::new(),
            duration_ms: 200_000,
            bitrate: None,
            sample_rate: None,
            channels: None,
            format: "mp3".to_string(),
            artwork: None,
            lyrics: None,
            from_filename: false,
        };

        let path = paths.resolve(relative).unwrap();
        let content = crate::library::hash::content_hash(&path).unwrap();

        crate::db::repository::insert_track(
            pool,
            crate::db::repository::NewTrack {
                metadata: &meta,
                relative_path: relative,
                file_size: std::fs::metadata(&path).unwrap().len() as i64,
                content_hash: &content,
                // Volontairement absente : on simule une bibliothèque importée
                // avant que l'empreinte audio existe.
                audio_hash: "",
                file_modified_at: None,
                artwork_hash: None,
                source: "scan",
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn rattrape_les_empreintes_manquantes() {
        let (_dir, pool, paths) = bibliotheque().await;
        let relative = ecrire(&paths, "A/Album/01 - Titre.mp3", 100);
        let id = inserer(&pool, &paths, &relative).await;

        sqlx::query("UPDATE tracks SET audio_hash = NULL WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        let rapport = run(&pool, &paths).await.unwrap();
        assert_eq!(rapport.hashed, 1);

        let hash: Option<String> =
            sqlx::query_scalar("SELECT audio_hash FROM tracks WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(hash.is_some_and(|h| !h.is_empty()));
    }

    #[tokio::test]
    async fn fusionne_deux_exemplaires_retagues_differemment() {
        // Le cas réel : le même morceau entré deux fois parce que la réécriture
        // des tags avait changé les octets du fichier.
        let (_dir, pool, paths) = bibliotheque().await;

        let original = ecrire(&paths, "A/Album/01 - Titre.mp3", 100);
        let copie = ecrire(&paths, "A/Album/01 - Titre (2).mp3", 9000);

        let garde = inserer(&pool, &paths, &original).await;
        let doublon = inserer(&pool, &paths, &copie).await;
        sqlx::query("UPDATE tracks SET audio_hash = NULL")
            .execute(&pool)
            .await
            .unwrap();

        let rapport = run(&pool, &paths).await.unwrap();

        assert_eq!(rapport.hashed, 2, "les deux doivent être empreintés");
        assert_eq!(rapport.merged, 1, "un seul des deux doit disparaître");

        let restants: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM tracks WHERE deleted_at IS NULL ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(restants, vec![garde], "c'est la plus ancienne qui reste");
        assert!(!restants.contains(&doublon));

        // Le fichier n'est pas détruit, seulement écarté.
        assert!(!paths.resolve(&copie).unwrap().exists());
        assert!(paths.resolve(&original).unwrap().exists());
    }

    #[tokio::test]
    async fn ne_touche_pas_a_une_bibliotheque_saine() {
        let (_dir, pool, paths) = bibliotheque().await;

        let premier = ecrire(&paths, "A/Album/01 - Un.mp3", 100);
        inserer(&pool, &paths, &premier).await;

        // Un second morceau, au contenu réellement différent.
        let second = paths.resolve("A/Album/02 - Deux.mp3").unwrap();
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::File::create(&second)
            .unwrap()
            .write_all(&(0..6000).map(|i| (i % 97) as u8).collect::<Vec<u8>>())
            .unwrap();
        inserer(&pool, &paths, "A/Album/02 - Deux.mp3").await;

        sqlx::query("UPDATE tracks SET audio_hash = NULL")
            .execute(&pool)
            .await
            .unwrap();

        let rapport = run(&pool, &paths).await.unwrap();
        assert_eq!(rapport.merged, 0);
        assert_eq!(rapport.files_set_aside, 0);

        // Une seconde exécution ne doit plus rien avoir à faire.
        assert!(run(&pool, &paths).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn un_doublon_deja_ecoute_conserve_son_historique() {
        // Supprimer la ligne emporterait `play_events` — ou serait refusé par
        // la contrainte. On la marque supprimée, l'historique survit.
        let (_dir, pool, paths) = bibliotheque().await;

        let original = ecrire(&paths, "A/Album/01 - Titre.mp3", 100);
        let copie = ecrire(&paths, "A/Album/01 - Titre (2).mp3", 9000);
        inserer(&pool, &paths, &original).await;
        let doublon = inserer(&pool, &paths, &copie).await;

        sqlx::query("INSERT INTO listening_sessions (id, started_at) VALUES ('s1', 0)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO play_events
                 (track_id, session_id, started_at, ended_at, listened_ms,
                  duration_ms, completion, source, hour_local, weekday)
             VALUES (?, 's1', 0, 1000, 1000, 200000, 0.005, 'library', 12, 3)",
        )
        .bind(doublon)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("UPDATE tracks SET audio_hash = NULL")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(run(&pool, &paths).await.unwrap().merged, 1);

        let evenements: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM play_events WHERE track_id = ?")
                .bind(doublon)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(evenements, 1, "l'historique doit survivre au ménage");
    }
    #[tokio::test]
    async fn remet_l_index_en_accord_avec_la_base() {
        // Le cas mesuré : 113 morceaux répondaient encore à un album de
        // compilation effacé depuis, et restaient introuvables sous le leur.
        let (_dir, pool, paths) = bibliotheque().await;

        let chemin = ecrire(&paths, "A/Album/01 - Un.mp3", 100);
        let track_id = inserer(&pool, &paths, &chemin).await;

        sqlx::query("UPDATE tracks_fts SET album_title = 'I migliori anni 90' WHERE track_id = ?")
            .bind(track_id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(run(&pool, &paths).await.unwrap().reindexed, 1);

        let indexe: Option<String> =
            sqlx::query_scalar("SELECT album_title FROM tracks_fts WHERE track_id = ?")
                .bind(track_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let en_base: Option<String> = sqlx::query_scalar(
            "SELECT al.title FROM tracks t LEFT JOIN albums al ON al.id = t.album_id WHERE t.id = ?",
        )
        .bind(track_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(indexe.unwrap_or_default(), en_base.unwrap_or_default());

        // Une seconde exécution n'a plus rien à réparer.
        assert_eq!(run(&pool, &paths).await.unwrap().reindexed, 0);
    }
}
