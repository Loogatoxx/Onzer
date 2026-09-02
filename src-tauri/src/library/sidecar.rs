//! Recueillir les `.lrc` que le téléchargeur a laissés au dépôt.
//!
//! # Le fichier que personne ne réclamait
//!
//! Un téléchargeur configuré pour produire des paroles synchronisées dépose
//! **deux** fichiers : le morceau, et un `.lrc` du même nom. Le dépôt d'Onzer
//! n'accepte que l'audio — il ignore poliment le reste. Résultat : le morceau
//! part se ranger dans la bibliothèque, et son `.lrc` reste seul au dépôt,
//! indéfiniment. Cent dix-huit d'entre eux s'y étaient accumulés.
//!
//! Ce n'est pas un défaut du dépôt : y accepter n'importe quoi serait pire. Il
//! manquait simplement l'étape qui **rapproche** les deux.
//!
//! # Comment un `.lrc` retrouve son morceau
//!
//! Par son nom, seule information dont il dispose :
//!
//! ```text
//!   « 4Keus Gang - Midi dans le ghetto.lrc »
//!         │                  │
//!      artiste             titre
//!         └──────┬───────────┘
//!         recherche en base, sur les deux
//! ```
//!
//! L'artiste **et** le titre doivent concorder. Se contenter du titre
//! attacherait les paroles d'une reprise à l'original, ou celles d'un
//! « Intro » à l'« Intro » d'un autre album — le genre d'erreur silencieuse
//! qui ne se découvre qu'en écoutant.
//!
//! # Ce qu'il advient du fichier
//!
//! Il est **déplacé à côté de son morceau**, jamais supprimé. Onzer le relira
//! au prochain import (`lyrics::read_sidecar`), et un autre lecteur le trouvera
//! là où il s'attend à le trouver. Ce qui ne trouve pas son morceau reste au
//! dépôt : c'est peut-être un morceau pas encore téléchargé.

use std::path::Path;

use sqlx::SqlitePool;

use crate::core::{PathResolver, Result};
use crate::library::naming::normalize_key;
use crate::library::{importer, lyrics};

/// Ce que la collecte a fait.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptionReport {
    /// Fichiers rattachés à leur morceau.
    pub adopted: u64,
    /// Fichiers restés au dépôt, faute de morceau correspondant.
    pub orphans: u64,
}

/// Rattache les `.lrc` du dépôt aux morceaux de la bibliothèque.
pub async fn adopt(pool: &SqlitePool, paths: &PathResolver) -> Result<AdoptionReport> {
    let Some(root) = paths.library_root() else {
        return Ok(AdoptionReport::default());
    };

    let inbox = crate::ingest::inbox::inbox_path(root);
    let Ok(entries) = std::fs::read_dir(&inbox) else {
        return Ok(AdoptionReport::default());
    };

    let mut report = AdoptionReport::default();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "lrc") {
            continue;
        }

        match adopt_one(pool, paths, &path).await {
            Ok(true) => report.adopted += 1,
            Ok(false) => report.orphans += 1,
            Err(error) => {
                tracing::warn!(?path, %error, "paroles non rattachées");
                report.orphans += 1;
            }
        }
    }

    Ok(report)
}

/// Rattache un fichier. `false` quand aucun morceau ne lui correspond.
async fn adopt_one(pool: &SqlitePool, paths: &PathResolver, lrc: &Path) -> Result<bool> {
    let Some(stem) = lrc.file_stem().map(|stem| stem.to_string_lossy().to_string()) else {
        return Ok(false);
    };

    let Some(track_id) = find_track(pool, &split_name(&stem)).await? else {
        return Ok(false);
    };

    let text = std::fs::read_to_string(lrc)?;
    let text = text.trim();

    // Un `.lrc` sans horodatage n'apporte rien que le tag ne porte déjà : le
    // rattacher ferait remplacer un texte par le même texte.
    if text.is_empty() || !lyrics::is_synced_text(text) {
        return Ok(false);
    }

    sqlx::query("UPDATE tracks SET lyrics = ? WHERE id = ?")
        .bind(text)
        .bind(track_id)
        .execute(pool)
        .await?;

    // Le fichier suit son morceau. Si le déplacement échoue — disque plein,
    // volume en lecture seule — les paroles sont déjà en base : on ne perd
    // rien, le fichier reste simplement au dépôt.
    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(pool)
            .await?;

    if let Some(relative_path) = relative_path {
        if let Ok(audio) = importer::absolute_path(paths, &relative_path) {
            let destination = audio.with_extension("lrc");
            if let Err(error) = std::fs::rename(lrc, &destination) {
                tracing::warn!(?destination, %error, "fichier .lrc non déplacé");
            }
        }
    }

    Ok(true)
}

/// Sépare « Artiste - Titre » en ses deux moitiés.
///
/// Sur le **premier** tiret entouré d'espaces : un titre peut en contenir
/// d'autres — « Ma go - Remix » —, un nom d'artiste bien plus rarement.
/// Sans séparateur, tout est pris pour un titre : c'est moins précis, mais
/// mieux que de renoncer.
fn split_name(stem: &str) -> (Option<String>, String) {
    match stem.split_once(" - ") {
        Some((artist, title)) => {
            let artist = artist.trim();
            let title = title.trim();

            if artist.is_empty() || title.is_empty() {
                (None, stem.trim().to_string())
            } else {
                (Some(artist.to_string()), title.to_string())
            }
        }
        None => (None, stem.trim().to_string()),
    }
}

/// Cherche le morceau désigné par un nom de fichier.
async fn find_track(pool: &SqlitePool, (artist, title): &(Option<String>, String)) -> Result<Option<i64>> {
    let normalized_title = normalize_key(title);
    if normalized_title.is_empty() {
        return Ok(None);
    }

    let normalized_artist = artist.as_deref().map(normalize_key);

    // Un seul résultat exigé : deux morceaux du même nom, et l'on ne saurait
    // pas auquel appartiennent ces paroles.
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT t.id FROM tracks t
          WHERE t.deleted_at IS NULL
            AND t.normalized_title = ?1
            AND (
                 ?2 IS NULL
                 OR EXISTS (
                     SELECT 1 FROM track_artists ta
                       JOIN artists a ON a.id = ta.artist_id
                      WHERE ta.track_id = t.id AND a.normalized_name = ?2
                 )
            )
          LIMIT 2",
    )
    .bind(&normalized_title)
    .bind(normalized_artist.as_deref())
    .fetch_all(pool)
    .await?;

    Ok(match ids.as_slice() {
        [id] => Some(*id),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn separe_l_artiste_du_titre() {
        assert_eq!(
            split_name("4Keus Gang - Midi dans le ghetto"),
            (Some("4Keus Gang".into()), "Midi dans le ghetto".into())
        );
    }

    #[test]
    fn le_premier_tiret_fait_la_coupure() {
        // « Ma go - Remix » : le second tiret appartient au titre.
        assert_eq!(
            split_name("Damso - Ma go - Remix"),
            (Some("Damso".into()), "Ma go - Remix".into())
        );
    }

    #[test]
    fn un_nom_sans_separateur_est_un_titre() {
        assert_eq!(split_name("Macarena"), (None, "Macarena".into()));
    }

    #[test]
    fn un_tiret_sans_espaces_ne_coupe_pas() {
        // « Jay-Z » n'est pas un artiste nommé « Jay » avec un titre « Z ».
        assert_eq!(split_name("Jay-Z"), (None, "Jay-Z".into()));
    }

    async fn base() -> (tempfile::TempDir, SqlitePool, PathResolver) {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::connect(&dir.path().join("onzer.db")).await.unwrap();
        db::migrate(&pool).await.unwrap();

        let root = dir.path().join("Musique");
        std::fs::create_dir_all(root.join("_Inbox")).unwrap();

        let mut paths = PathResolver::new(dir.path().join("data"));
        paths.set_library_root(Some(root));
        (dir, pool, paths)
    }

    async fn morceau(pool: &SqlitePool, titre: &str, artiste: &str, chemin: &str) -> i64 {
        let track_id: i64 = sqlx::query_scalar(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at, source)
             VALUES (?, ?, 200000, ?, 1, ?, 'mp3', 0, 'scan') RETURNING id",
        )
        .bind(titre)
        .bind(normalize_key(titre))
        .bind(chemin)
        .bind(chemin)
        .fetch_one(pool)
        .await
        .unwrap();

        let artist_id: i64 = sqlx::query_scalar(
            "INSERT INTO artists (name, normalized_name, created_at) VALUES (?, ?, 0) RETURNING id",
        )
        .bind(artiste)
        .bind(normalize_key(artiste))
        .fetch_one(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO track_artists (track_id, artist_id, role, position) VALUES (?,?,'main',0)",
        )
        .bind(track_id)
        .bind(artist_id)
        .execute(pool)
        .await
        .unwrap();

        track_id
    }

    #[tokio::test]
    async fn les_paroles_rejoignent_leur_morceau() {
        let (_dir, pool, paths) = base().await;
        let root = paths.library_root().unwrap().to_path_buf();

        std::fs::create_dir_all(root.join("Damso")).unwrap();
        std::fs::write(root.join("Damso/Macarena.mp3"), b"audio").unwrap();
        let track_id = morceau(&pool, "Macarena", "Damso", "Damso/Macarena.mp3").await;

        let depot = root.join("_Inbox/Damso - Macarena.lrc");
        std::fs::write(&depot, "[00:01.00]Première ligne").unwrap();

        let rapport = adopt(&pool, &paths).await.unwrap();

        assert_eq!(rapport.adopted, 1);
        let paroles: Option<String> = sqlx::query_scalar("SELECT lyrics FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(paroles.as_deref(), Some("[00:01.00]Première ligne"));

        assert!(!depot.exists(), "le fichier ne doit plus traîner au dépôt");
        assert!(
            root.join("Damso/Macarena.lrc").is_file(),
            "il doit suivre son morceau"
        );
    }

    #[tokio::test]
    async fn un_fichier_sans_morceau_reste_au_depot() {
        // C'est peut-être un morceau pas encore téléchargé : le jeter serait
        // perdre des paroles qui serviront demain.
        let (_dir, pool, paths) = base().await;
        let root = paths.library_root().unwrap().to_path_buf();

        let depot = root.join("_Inbox/Inconnu - Jamais entendu.lrc");
        std::fs::write(&depot, "[00:01.00]Ligne").unwrap();

        let rapport = adopt(&pool, &paths).await.unwrap();

        assert_eq!(rapport.adopted, 0);
        assert_eq!(rapport.orphans, 1);
        assert!(depot.is_file());
    }

    #[tokio::test]
    async fn un_homonyme_bloque_le_rattachement() {
        // Deux « Intro », et l'on ne saurait pas à laquelle appartiennent ces
        // paroles. Ne rien faire vaut mieux que de se tromper.
        let (_dir, pool, paths) = base().await;
        let root = paths.library_root().unwrap().to_path_buf();

        morceau(&pool, "Intro", "Damso", "a/Intro.mp3").await;
        let second: i64 = sqlx::query_scalar(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at, source)
             VALUES ('Intro', 'intro', 1, 'b/Intro.mp3', 1, 'b', 'mp3', 0, 'scan') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(second > 0);

        std::fs::write(root.join("_Inbox/Intro.lrc"), "[00:01.00]Ligne").unwrap();

        let rapport = adopt(&pool, &paths).await.unwrap();
        assert_eq!(rapport.adopted, 0);
    }

    #[tokio::test]
    async fn un_fichier_sans_horodatage_est_ignore() {
        // Il n'apporterait rien que le tag ne porte déjà.
        let (_dir, pool, paths) = base().await;
        let root = paths.library_root().unwrap().to_path_buf();

        morceau(&pool, "Macarena", "Damso", "Damso/Macarena.mp3").await;
        std::fs::write(root.join("_Inbox/Damso - Macarena.lrc"), "Du texte brut").unwrap();

        let rapport = adopt(&pool, &paths).await.unwrap();
        assert_eq!(rapport.adopted, 0);
    }
}
