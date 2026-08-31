//! Réglages persistants, stockés en JSON dans la table `settings`.
//!
//! C'est ici que vit la racine de bibliothèque : la seule information
//! absolue du système (ADR-006).

use sqlx::SqlitePool;

use crate::core::Result;

pub const LIBRARY_ROOT: &str = "library_root";
pub const LIBRARY_VOLUME: &str = "library_volume";

/// Lit un réglage brut (JSON). `None` si la clé est absente ou vaut `null`.
pub async fn get_raw(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;

    Ok(value.filter(|v| v != "null"))
}

/// Lit un réglage typé.
pub async fn get<T: serde::de::DeserializeOwned>(
    pool: &SqlitePool,
    key: &str,
) -> Result<Option<T>> {
    let Some(raw) = get_raw(pool, key).await? else {
        return Ok(None);
    };

    Ok(serde_json::from_str(&raw).ok())
}

/// Écrit un réglage typé. Crée la clé si elle n'existe pas.
pub async fn set<T: serde::Serialize>(pool: &SqlitePool, key: &str, value: &T) -> Result<()> {
    let encoded = serde_json::to_string(value)
        .map_err(|e| crate::core::OnzerError::Invalid(format!("réglage illisible : {e}")))?;

    sqlx::query(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?, ?, unixepoch() * 1000)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                        updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(encoded)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn base_de_test() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::connect(&dir.path().join("test.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn la_racine_est_absente_au_premier_lancement() {
        let (_dir, pool) = base_de_test().await;
        let root: Option<String> = get(&pool, LIBRARY_ROOT).await.unwrap();
        assert_eq!(root, None);
    }

    #[tokio::test]
    async fn ecrit_puis_relit_la_racine() {
        let (_dir, pool) = base_de_test().await;

        set(&pool, LIBRARY_ROOT, &"/Volumes/Lexar/Musique").await.unwrap();
        let root: Option<String> = get(&pool, LIBRARY_ROOT).await.unwrap();

        assert_eq!(root.as_deref(), Some("/Volumes/Lexar/Musique"));
    }

    #[tokio::test]
    async fn ecrase_une_valeur_existante() {
        let (_dir, pool) = base_de_test().await;

        set(&pool, LIBRARY_ROOT, &"/premier").await.unwrap();
        set(&pool, LIBRARY_ROOT, &"/second").await.unwrap();

        let root: Option<String> = get(&pool, LIBRARY_ROOT).await.unwrap();
        assert_eq!(root.as_deref(), Some("/second"));
    }
}
