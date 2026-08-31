//! Accès à la base SQLite.
//!
//! La base vit sur le disque interne, jamais sur le SSD externe (ADR-006) :
//! exFAT n'est pas journalisé et gère mal les verrous POSIX, deux conditions
//! réunies pour corrompre une base SQLite.

pub mod events;
pub mod repository;
pub mod settings;

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::SqlitePool;

use crate::core::Result;

/// Migrations embarquées dans le binaire à la compilation.
/// Aucun fichier SQL n'a donc à être déployé à côté de l'application.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Ouvre (ou crée) la base et configure SQLite pour un usage local intensif.
pub async fn connect(database_file: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(database_file)
        .create_if_missing(true)
        // WAL : le lecteur peut lire pendant que le scanner écrit.
        // Sans ça, un scan de bibliothèque figerait toute l'interface.
        .journal_mode(SqliteJournalMode::Wal)
        // NORMAL est le bon compromis avec WAL : durable face à un plantage
        // applicatif, et bien plus rapide que FULL.
        .synchronous(SqliteSynchronous::Normal)
        // Les contraintes de clé étrangère sont désactivées par défaut dans
        // SQLite. Sans cette ligne, tout le modèle relationnel est décoratif.
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        // 32 Mo de cache : la totalité des index d'une bibliothèque de
        // 5 000 morceaux tient en mémoire.
        .pragma("cache_size", "-32000")
        .pragma("temp_store", "MEMORY")
        .pragma("mmap_size", "268435456");

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await?;

    Ok(pool)
}

/// Applique les migrations en attente. Idempotent.
pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// Version de schéma actuellement appliquée.
pub async fn schema_version(pool: &SqlitePool) -> Result<i64> {
    let version: Option<i64> =
        sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(pool)
            .await?;

    Ok(version.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ouvre une base temporaire migrée, isolée du disque de l'utilisateur.
    async fn base_de_test() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = connect(&dir.path().join("test.db")).await.unwrap();
        migrate(&pool).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn les_migrations_sappliquent() {
        let (_dir, pool) = base_de_test().await;

        // Le numéro suit le dernier fichier de `migrations/`. Ce test échoue
        // volontairement à chaque ajout : c'est le rappel qu'une migration ne
        // se glisse pas en douce.
        assert_eq!(schema_version(&pool).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn les_vues_de_qualite_du_moteur_sont_interrogeables() {
        let (_dir, pool) = base_de_test().await;

        // Sans historique elles sont vides, mais elles doivent être valides :
        // une erreur SQL ne doit pas se découvrir en production.
        for vue in ["reco_quality", "reco_strategy_quality"] {
            sqlx::query(&format!("SELECT * FROM {vue}"))
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|error| panic!("la vue {vue} est invalide : {error}"));
        }
    }

    #[tokio::test]
    async fn les_six_strategies_sont_amorcees() {
        let (_dir, pool) = base_de_test().await;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reco_strategies")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(count, 6);

        // Toutes démarrent sur la loi uniforme : « je ne sais rien ».
        let neutres: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM reco_strategies WHERE alpha = 1 AND beta = 1")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(neutres, 6);
    }

    /// Vérifie que le SQLite embarqué expose bien FTS5 : sans lui, toute la
    /// recherche de la bibliothèque tomberait à l'eau.
    #[tokio::test]
    async fn fts5_est_disponible_et_ignore_les_accents() {
        let (_dir, pool) = base_de_test().await;

        sqlx::query(
            "INSERT INTO tracks_fts (track_id, title, artist_names, album_title)
             VALUES (1, 'Halo', 'Beyoncé', 'I Am')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let found: Option<String> =
            sqlx::query_scalar("SELECT artist_names FROM tracks_fts WHERE tracks_fts MATCH 'beyonce'")
                .fetch_optional(&pool)
                .await
                .unwrap();

        assert_eq!(found.as_deref(), Some("Beyoncé"));
    }

    /// Les clés étrangères doivent être actives : c'est ce qui empêche de
    /// supprimer un morceau dont l'historique d'écoute existe encore.
    #[tokio::test]
    async fn les_cles_etrangeres_sont_actives() {
        let (_dir, pool) = base_de_test().await;

        let result = sqlx::query(
            "INSERT INTO track_artists (track_id, artist_id) VALUES (999, 999)",
        )
        .execute(&pool)
        .await;

        assert!(result.is_err(), "une clé étrangère invalide doit être refusée");
    }

    /// Le journal d'écoute est append-only : sa suppression est bloquée par
    /// un trigger, quelle que soit la requête émise.
    #[tokio::test]
    async fn le_journal_decoute_est_inviolable() {
        let (_dir, pool) = base_de_test().await;

        sqlx::query(
            "INSERT INTO artists (name, normalized_name, created_at) VALUES ('Air', 'air', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO tracks (title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at)
             VALUES ('Sexy Boy', 'sexy boy', 294000, 'Air/1998 - Moon Safari/02.mp3',
                     7000000, 'hash', 'mp3', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO listening_sessions (id, started_at) VALUES ('s1', 0)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO play_events (track_id, session_id, started_at, duration_ms,
                                      hour_local, weekday, is_weekend)
             VALUES (1, 's1', 0, 294000, 22, 4, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let deletion = sqlx::query("DELETE FROM play_events WHERE id = 1")
            .execute(&pool)
            .await;

        assert!(
            deletion.is_err(),
            "le trigger append-only doit refuser toute suppression"
        );
    }
}
