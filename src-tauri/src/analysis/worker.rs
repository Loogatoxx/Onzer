//! Ouvrier d'analyse : il travaille en fond, et **on ne doit jamais le sentir**.
//!
//! # Le risque
//!
//! Analyser un morceau, c'est décoder 90 secondes d'audio puis exécuter environ
//! 3 800 transformées de Fourier. Mesuré : **267 ms par morceau**. C'est du
//! calcul pur, qui sature un cœur pendant tout ce temps. Fait naïvement, sur
//! cinq mille morceaux, l'interface se fige et la lecture se met à hoqueter.
//!
//! > **Note de performance.** La même analyse prenait 6 591 ms avant que les
//! > profils de compilation ne soient corrigés : les dépendances DSP tournaient
//! > sans optimisation, et le profil de publication visait la taille du binaire
//! > plutôt que la vitesse. Un facteur 24,7, pour deux lignes de `Cargo.toml`.
//!
//! # Les quatre protections
//!
//! | Protection | Ce qu'elle empêche |
//! |---|---|
//! | Exécution sur un fil bloquant dédié | De bloquer la boucle asynchrone qui sert l'interface et la lecture |
//! | **Un seul morceau à la fois** | De saturer tous les cœurs et d'affamer le fil audio |
//! | Pause entre deux morceaux | De monopoliser le processeur, et d'échauffer la machine |
//! | Reprise depuis la base | De tout recommencer après un redémarrage |
//!
//! Le choix d'un seul morceau à la fois est délibéré. Paralléliser diviserait
//! le temps total par quatre, mais rendrait l'application désagréable pendant
//! toute la durée du traitement. Une analyse invisible qui prend une heure vaut
//! mieux qu'une analyse rapide qui rend l'application inutilisable.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::core::{now_ms, PathResolver, Result};
use crate::library::importer;

use super::{AudioFeatures, FeatureExtractor, NativeExtractor, ANALYZER_NAME, ANALYZER_VERSION};

/// Repos entre deux morceaux.
///
/// # Calibré sur une mesure réelle
///
/// L'analyse d'un MP3 de 8 Mo prend **267 ms** (mesuré sur Apple Silicon,
/// dépendances optimisées). Une pause de 250 ms maintient donc l'ouvrier à
/// environ **50 % d'un seul cœur** — imperceptible sur une machine
/// multicœur, tout en laissant le processeur respirer.
///
/// Ordre de grandeur pour une bibliothèque complète :
///
/// | Morceaux | Durée totale |
/// |---|---|
/// | 500 | ~4 minutes |
/// | 5 000 | ~45 minutes |
///
/// Une seule fois, en tâche de fond, sans que personne n'attende le résultat.
const REST_BETWEEN_TRACKS: Duration = Duration::from_millis(250);

/// Attente quand il n'y a plus rien à analyser.
const IDLE_INTERVAL: Duration = Duration::from_secs(20);

/// Avancement, publié vers l'interface.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProgress {
    pub analyzed: i64,
    pub pending: i64,
    pub failed: i64,
    pub total: i64,
}

impl AnalysisProgress {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            return 1.0;
        }
        self.analyzed as f64 / self.total as f64
    }

    pub fn is_complete(&self) -> bool {
        self.pending == 0
    }
}

/// État d'avancement de l'analyse de la bibliothèque.
pub async fn progress(pool: &SqlitePool) -> Result<AnalysisProgress> {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
             COUNT(*),
             SUM(CASE WHEN analysis_state = 'done'    THEN 1 ELSE 0 END),
             SUM(CASE WHEN analysis_state IN ('pending', 'running') THEN 1 ELSE 0 END),
             SUM(CASE WHEN analysis_state = 'failed'  THEN 1 ELSE 0 END)
         FROM tracks
         WHERE deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await?;

    Ok(AnalysisProgress {
        total: row.0,
        analyzed: row.1,
        pending: row.2,
        failed: row.3,
    })
}

/// Démarre l'ouvrier. Ne retourne jamais.
pub fn spawn(pool: SqlitePool, paths: Arc<RwLock<PathResolver>>) {
    tauri::async_runtime::spawn(async move {
        // Un morceau resté « running » vient d'un arrêt brutal pendant son
        // analyse : il est remis en attente plutôt que d'être perdu.
        if let Err(error) = reset_interrupted(&pool).await {
            tracing::warn!(%error, "reprise de l'analyse impossible");
        }

        let extractor = NativeExtractor;

        loop {
            let resolver = paths.read().await.clone();

            // Sans bibliothèque accessible, il n'y a rien à lire : on attend
            // plutôt que d'échouer en boucle sur chaque morceau.
            if !resolver.is_library_online() {
                tokio::time::sleep(IDLE_INTERVAL).await;
                continue;
            }

            match next_pending(&pool).await {
                Ok(Some((track_id, relative_path))) => {
                    analyze_one(&pool, &resolver, &extractor, track_id, &relative_path).await;
                    tokio::time::sleep(REST_BETWEEN_TRACKS).await;
                }
                Ok(None) => tokio::time::sleep(IDLE_INTERVAL).await,
                Err(error) => {
                    tracing::warn!(%error, "file d'analyse illisible");
                    tokio::time::sleep(IDLE_INTERVAL).await;
                }
            }
        }
    });
}

/// Remet en attente les analyses interrompues par un arrêt brutal.
async fn reset_interrupted(pool: &SqlitePool) -> Result<()> {
    let restored = sqlx::query(
        "UPDATE tracks SET analysis_state = 'pending' WHERE analysis_state = 'running'",
    )
    .execute(pool)
    .await?
    .rows_affected();

    if restored > 0 {
        tracing::info!(restored, "analyses interrompues remises en file");
    }

    Ok(())
}

/// Prend le prochain morceau à analyser et le marque comme en cours.
///
/// Les plus récemment ajoutés passent d'abord : ce sont ceux que l'utilisateur
/// a le plus de chances de vouloir écouter tout de suite.
async fn next_pending(pool: &SqlitePool) -> Result<Option<(i64, String)>> {
    let candidate: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM tracks
         WHERE analysis_state = 'pending'
           AND deleted_at IS NULL
           AND is_available = 1
         ORDER BY added_at DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if let Some((track_id, _)) = &candidate {
        sqlx::query("UPDATE tracks SET analysis_state = 'running' WHERE id = ?")
            .bind(track_id)
            .execute(pool)
            .await?;
    }

    Ok(candidate)
}

/// Analyse un morceau et enregistre le résultat.
///
/// N'échoue jamais bruyamment : un fichier illisible est marqué comme tel, et
/// l'ouvrier passe au suivant.
async fn analyze_one(
    pool: &SqlitePool,
    paths: &PathResolver,
    extractor: &NativeExtractor,
    track_id: i64,
    relative_path: &str,
) {
    let Ok(path) = importer::absolute_path(paths, relative_path) else {
        mark_failed(pool, track_id, "chemin invalide").await;
        return;
    };

    if !path.is_file() {
        mark_failed(pool, track_id, "fichier introuvable").await;
        return;
    }

    // Le calcul part sur un fil bloquant : exécuté directement, il figerait la
    // boucle asynchrone qui sert l'interface et pilote la lecture.
    let extractor = *extractor;
    let started = std::time::Instant::now();
    let outcome =
        tokio::task::spawn_blocking(move || extractor.extract(&path)).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    match outcome {
        Ok(Ok(features)) => {
            if let Err(error) = store_features(pool, track_id, &features).await {
                tracing::warn!(track_id, %error, "features non enregistrées");
                mark_failed(pool, track_id, "écriture impossible").await;
            } else {
                // La durée est journalisée en permanence : c'est ce qui
                // permettra de savoir si une évolution de l'algorithme a rendu
                // l'analyse trop coûteuse.
                tracing::debug!(
                    track_id,
                    tempo = features.tempo,
                    elapsed_ms,
                    "analyse terminée"
                );
            }
        }
        Ok(Err(error)) => mark_failed(pool, track_id, &error.to_string()).await,
        // La tâche a paniqué : ne jamais laisser tomber l'ouvrier pour autant.
        Err(error) => mark_failed(pool, track_id, &format!("analyse interrompue : {error}")).await,
    }
}

async fn store_features(
    pool: &SqlitePool,
    track_id: i64,
    features: &AudioFeatures,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO track_features (
             track_id, embedding, embedding_dim, tempo, energy, loudness,
             danceability, valence, musical_key, musical_mode,
             analyzer, analyzer_version, analyzed_at
         ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(track_id) DO UPDATE SET
             embedding = excluded.embedding,
             embedding_dim = excluded.embedding_dim,
             tempo = excluded.tempo,
             energy = excluded.energy,
             loudness = excluded.loudness,
             danceability = excluded.danceability,
             valence = excluded.valence,
             musical_key = excluded.musical_key,
             musical_mode = excluded.musical_mode,
             analyzer = excluded.analyzer,
             analyzer_version = excluded.analyzer_version,
             analyzed_at = excluded.analyzed_at",
    )
    .bind(track_id)
    .bind(features.embedding_bytes())
    .bind(features.embedding.len() as i64)
    .bind(features.tempo.map(f64::from))
    .bind(f64::from(features.energy))
    .bind(f64::from(features.loudness))
    .bind(f64::from(features.danceability))
    .bind(f64::from(features.valence))
    .bind(features.key.map(|key| i64::from(key.tonic)))
    .bind(features.key.map(|key| i64::from(key.is_major)))
    .bind(ANALYZER_NAME)
    .bind(ANALYZER_VERSION)
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE tracks SET analysis_state = 'done', analysis_error = NULL WHERE id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

async fn mark_failed(pool: &SqlitePool, track_id: i64, reason: &str) {
    tracing::warn!(track_id, reason, "analyse échouée");

    let _ = sqlx::query(
        "UPDATE tracks SET analysis_state = 'failed', analysis_error = ? WHERE id = ?",
    )
    .bind(reason)
    .bind(track_id)
    .execute(pool)
    .await;
}

/// Remet toute la bibliothèque en file d'analyse.
///
/// Utile après une évolution de l'algorithme : les anciens vecteurs ne sont
/// plus comparables aux nouveaux, et les mélanger produirait des distances
/// dénuées de sens.
pub async fn requeue_all(pool: &SqlitePool) -> Result<u64> {
    let affected = sqlx::query(
        "UPDATE tracks SET analysis_state = 'pending', analysis_error = NULL
         WHERE deleted_at IS NULL",
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Remet en file les seuls morceaux analysés par une version périmée.
pub async fn requeue_outdated(pool: &SqlitePool) -> Result<u64> {
    let affected = sqlx::query(
        "UPDATE tracks SET analysis_state = 'pending'
         WHERE deleted_at IS NULL
           AND analysis_state = 'done'
           AND id NOT IN (
               SELECT track_id FROM track_features
                WHERE analyzer = ? AND analyzer_version = ?
           )",
    )
    .bind(ANALYZER_NAME)
    .bind(ANALYZER_VERSION)
    .execute(pool)
    .await?
    .rows_affected();

    if affected > 0 {
        tracing::info!(affected, "morceaux à réanalyser (version d'algorithme)");
    }

    Ok(affected)
}
