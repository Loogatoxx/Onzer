//! Repartir de zéro sans rien détruire.
//!
//! # Pourquoi cette opération existe
//!
//! Une bibliothèque constituée au fil de téléchargements hétéroclites finit par
//! porter les traces de chacun d'eux : des titres approximatifs, des albums
//! absents, des pochettes manquantes. Le jour où l'on retélécharge tout depuis
//! une source qui fournit des métadonnées exactes, le plus propre n'est pas de
//! corriger l'ancien — c'est de le remplacer.
//!
//! # Pourquoi on ne peut pas se contenter de « marquer supprimé »
//!
//! `tracks.relative_path` est **UNIQUE**, y compris pour les lignes marquées
//! supprimées. Un morceau retélécharché atterrirait au même endroit —
//! `Damso/Ipséité/03 - Macarena.mp3` — et l'insertion serait refusée par la
//! base. La bibliothèque paraîtrait vide et l'import échouerait sans que rien
//! n'explique pourquoi.
//!
//! Chaque ligne écartée voit donc ses champs uniques **neutralisés** :
//!
//! ```text
//!   relative_path : Damso/Ipséité/03 - Macarena.mp3  →  ancien:412
//!   content_hash  : 9f3c…                            →  ancien:412
//!   audio_hash    : 7a10…                            →  NULL
//! ```
//!
//! La place est rendue, et la ligne ne peut plus être confondue avec un fichier
//! réel par le dédoublonnage à l'import.
//!
//! # Ce qui survit, et pourquoi
//!
//! | Donnée | Sort | Raison |
//! |---|---|---|
//! | Historique d'écoute | **Conservé** | C'est la matière des statistiques ; il ne se reconstitue pas |
//! | Playlists | **Conservées** — nom, image, description | Ce sont des créations de l'utilisateur, pas des données de fichiers |
//! | Contenu des playlists | Vidé | Les morceaux qu'elles pointaient n'existent plus |
//! | Fichiers audio | **Mis de côté**, jamais supprimés | Onzer ne détruit rien sur le disque (ADR-007) |
//!
//! Le dossier `_Ancien` reste à la racine, entier et rangé comme il l'était.
//! L'utilisateur le jette lui-même quand il est sûr de lui — c'est une décision
//! qui ne se délègue pas.

use sqlx::SqlitePool;

use crate::core::{now_ms, PathResolver, Result};

/// Dossier où l'ancienne bibliothèque est mise à l'abri.
pub const ARCHIVE_DIR: &str = "_Ancien";

/// Ce que la reconstruction a fait.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildReport {
    /// Morceaux retirés de la bibliothèque.
    pub tracks: u64,
    /// Dossiers déplacés dans `_Ancien`.
    pub folders_set_aside: u64,
    /// Albums devenus vides, effacés.
    pub albums: u64,
    /// Artistes devenus vides, effacés.
    pub artists: u64,
}

/// Vide la bibliothèque et met les fichiers de côté.
///
/// L'ordre compte : la base d'abord, le disque ensuite. Si le déplacement des
/// fichiers échoue à mi-chemin, la base est déjà cohérente et un nouvel import
/// repartira proprement ; l'inverse laisserait des lignes pointant vers des
/// fichiers déplacés.
pub async fn run(pool: &SqlitePool, paths: &PathResolver) -> Result<RebuildReport> {
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM tracks WHERE deleted_at IS NULL ORDER BY id")
            .fetch_all(pool)
            .await?;

    let mut tx = pool.begin().await?;

    // L'index de recherche et les playlists d'abord : ils référencent des
    // morceaux qui n'existeront plus.
    sqlx::query("DELETE FROM tracks_fts WHERE track_id IN (SELECT id FROM tracks WHERE deleted_at IS NULL)")
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM playlist_tracks WHERE track_id IN (SELECT id FROM tracks WHERE deleted_at IS NULL)")
        .execute(&mut *tx)
        .await?;

    // Puis les morceaux, dont les champs uniques sont neutralisés pour rendre
    // la place à ceux qui vont arriver.
    sqlx::query(
        "UPDATE tracks
            SET deleted_at    = ?,
                relative_path = 'ancien:' || id,
                content_hash  = 'ancien:' || id,
                audio_hash    = NULL,
                is_available  = 0
          WHERE deleted_at IS NULL",
    )
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;

    // Un album ou un artiste sans morceau n'est plus qu'un nom qui traîne.
    let albums = sqlx::query(
        "DELETE FROM albums WHERE NOT EXISTS (
             SELECT 1 FROM tracks t WHERE t.album_id = albums.id AND t.deleted_at IS NULL)",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let artists = sqlx::query(
        "DELETE FROM artists WHERE NOT EXISTS (
             SELECT 1 FROM track_artists ta
               JOIN tracks t ON t.id = ta.track_id AND t.deleted_at IS NULL
              WHERE ta.artist_id = artists.id)",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;

    let folders_set_aside = archive_files(paths)?;

    Ok(RebuildReport {
        tracks: ids.len() as u64,
        folders_set_aside,
        albums,
        artists,
    })
}

/// Déplace le contenu de la bibliothèque dans `_Ancien`.
///
/// # Pourquoi par dossier et non fichier par fichier
///
/// Six cents renommages coûtent six cents appels système là où une vingtaine
/// suffisent — et sur le même volume, déplacer un dossier est instantané quel
/// que soit son poids. Surtout, la hiérarchie est **conservée telle quelle** :
/// l'utilisateur qui ouvre `_Ancien` dans le Finder retrouve sa bibliothèque
/// exactement comme il l'avait laissée, et non six cents fichiers en vrac.
///
/// Le dépôt est épargné : c'est là que les nouveaux fichiers attendent, et les
/// emporter reviendrait à annuler le travail qu'on vient de faire.
fn archive_files(paths: &PathResolver) -> Result<u64> {
    let Some(root) = paths.library_root() else {
        return Ok(0);
    };

    let archive = root.join(ARCHIVE_DIR);
    let mut moved = 0;

    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(0);
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();

        if is_spared(&name) {
            continue;
        }

        std::fs::create_dir_all(&archive)?;

        let destination = free_name(&archive, &name);
        if std::fs::rename(entry.path(), &destination).is_ok() {
            moved += 1;
        }
    }

    Ok(moved)
}

/// Ce qui ne bouge pas : le dépôt, l'archive elle-même, et les fichiers cachés
/// du système — un `.DS_Store` déplacé ne rendrait service à personne.
fn is_spared(name: &str) -> bool {
    name == crate::library::naming::INBOX_DIR
        || name == ARCHIVE_DIR
        || name.starts_with('.')
}

/// Un nom libre dans l'archive.
///
/// Deux reconstructions successives ne doivent pas écraser la première : la
/// seconde archive se range à côté, numérotée.
fn free_name(archive: &std::path::Path, name: &str) -> std::path::PathBuf {
    let direct = archive.join(name);
    if !direct.exists() {
        return direct;
    }

    for index in 2..1000 {
        let candidate = archive.join(format!("{name} ({index})"));
        if !candidate.exists() {
            return candidate;
        }
    }

    direct
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::db;

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

    async fn inserer(pool: &SqlitePool, relative: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, audio_hash, format, added_at, source)
             VALUES (?, ?, 200000, ?, 100, ?, ?, 'mp3', 0, 'scan') RETURNING id",
        )
        .bind(relative)
        .bind(relative.to_lowercase())
        .bind(relative)
        .bind(format!("hash-{relative}"))
        .bind(format!("audio-{relative}"))
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn le_chemin_est_rendu_a_celui_qui_arrive() {
        // Le piège : `relative_path` est UNIQUE même pour une ligne supprimée.
        // Sans neutralisation, le même morceau retéléchargé ne pourrait plus
        // entrer, et l'import échouerait sans rien expliquer.
        let (_dir, pool, paths) = bibliotheque().await;
        inserer(&pool, "Damso/Ipséité/03 - Macarena.mp3").await;

        run(&pool, &paths).await.unwrap();

        let reinsere = inserer(&pool, "Damso/Ipséité/03 - Macarena.mp3").await;
        assert!(reinsere > 0, "le chemin doit être de nouveau libre");
    }

    #[tokio::test]
    async fn l_historique_survit() {
        // Il ne se reconstitue pas : c'est la seule donnée irremplaçable.
        let (_dir, pool, paths) = bibliotheque().await;
        let track_id = inserer(&pool, "A/B/01.mp3").await;

        sqlx::query("INSERT INTO listening_sessions (id, started_at) VALUES ('s1', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO play_events (track_id, session_id, started_at, ended_at, listened_ms,
                                      duration_ms, completion, source, hour_local, weekday)
             VALUES (?, 's1', 0, 1000, 1000, 200000, 0.005, 'library', 12, 3)",
        )
        .bind(track_id)
        .execute(&pool)
        .await
        .unwrap();

        run(&pool, &paths).await.unwrap();

        let ecoutes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(ecoutes, 1);
    }

    #[tokio::test]
    async fn la_playlist_garde_son_nom_et_perd_son_contenu() {
        // « pas que ça commence à me changer les noms de mes playlists » : une
        // playlist est une création, pas une donnée de fichier.
        let (_dir, pool, paths) = bibliotheque().await;
        let track_id = inserer(&pool, "A/B/01.mp3").await;

        let playlist_id = db::playlists::create(&pool, "Soirée").await.unwrap();
        db::playlists::add_tracks(&pool, playlist_id, &[track_id]).await.unwrap();

        run(&pool, &paths).await.unwrap();

        let nom: String = sqlx::query_scalar("SELECT name FROM playlists WHERE id = ?")
            .bind(playlist_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(nom, "Soirée");

        let entrees: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM playlist_tracks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(entrees, 0, "les morceaux pointés n'existent plus");
    }

    #[tokio::test]
    async fn les_fichiers_sont_mis_de_cote_sans_toucher_au_depot() {
        let (_dir, pool, paths) = bibliotheque().await;
        let root = paths.library_root().unwrap().to_path_buf();

        std::fs::create_dir_all(root.join("Damso/Ipséité")).unwrap();
        std::fs::File::create(root.join("Damso/Ipséité/03 - Macarena.mp3"))
            .unwrap()
            .write_all(b"audio")
            .unwrap();

        let inbox = root.join(crate::library::naming::INBOX_DIR);
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::File::create(inbox.join("nouveau.mp3")).unwrap();

        let rapport = run(&pool, &paths).await.unwrap();

        assert_eq!(rapport.folders_set_aside, 1);
        assert!(
            root.join(format!("{ARCHIVE_DIR}/Damso/Ipséité/03 - Macarena.mp3")).is_file(),
            "la hiérarchie doit être conservée telle quelle"
        );
        assert!(!root.join("Damso").exists());
        assert!(
            inbox.join("nouveau.mp3").is_file(),
            "le dépôt contient ce qui va être importé : il ne bouge pas"
        );
    }

    #[tokio::test]
    async fn une_seconde_reconstruction_n_ecrase_pas_la_premiere() {
        let (_dir, pool, paths) = bibliotheque().await;
        let root = paths.library_root().unwrap().to_path_buf();

        std::fs::create_dir_all(root.join("Damso")).unwrap();
        run(&pool, &paths).await.unwrap();

        std::fs::create_dir_all(root.join("Damso")).unwrap();
        std::fs::File::create(root.join("Damso/second.mp3")).unwrap();
        run(&pool, &paths).await.unwrap();

        assert!(root.join(format!("{ARCHIVE_DIR}/Damso")).is_dir());
        assert!(
            root.join(format!("{ARCHIVE_DIR}/Damso (2)/second.mp3")).is_file(),
            "la seconde archive se range à côté de la première"
        );
    }

    #[tokio::test]
    async fn les_albums_et_artistes_vides_disparaissent() {
        let (_dir, pool, paths) = bibliotheque().await;
        let track_id = inserer(&pool, "A/B/01.mp3").await;

        let artist_id: i64 = sqlx::query_scalar(
            "INSERT INTO artists (name, normalized_name, created_at) VALUES ('Damso','damso',0)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO track_artists (track_id, artist_id, role, position) VALUES (?,?,'main',0)")
            .bind(track_id)
            .bind(artist_id)
            .execute(&pool)
            .await
            .unwrap();

        let rapport = run(&pool, &paths).await.unwrap();

        assert_eq!(rapport.artists, 1);
        let restants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artists")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(restants, 0);
    }
}
