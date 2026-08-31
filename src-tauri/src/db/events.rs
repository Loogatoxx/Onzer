//! Persistance du journal d'écoute et des agrégats qui en dérivent.
//!
//! Une écoute terminée écrit **quatre choses en une seule transaction** :
//! la ligne de journal (immuable), les compteurs du morceau, la matrice de
//! transitions, et les totaux de la session. Soit tout, soit rien : des
//! compteurs désynchronisés du journal seraient indétectables.

use sqlx::SqlitePool;

use crate::audio::tracking::{EndReason, PlayEventRecord};
use crate::core::{now_ms, Result};

/// Au-delà de cette inactivité, la session d'écoute est considérée close et
/// une nouvelle démarre. Trente minutes séparent bien « je fais une pause
/// café » de « je me remets à écouter ce soir ».
pub const SESSION_IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1_000;

/// Ouvre une session d'écoute et retourne son identifiant.
pub async fn open_session(pool: &SqlitePool) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO listening_sessions (id, started_at) VALUES (?, ?)")
        .bind(&id)
        .bind(now_ms())
        .execute(pool)
        .await?;

    Ok(id)
}

/// Enregistre une écoute terminée.
pub async fn record(pool: &SqlitePool, session_id: &str, event: &PlayEventRecord) -> Result<()> {
    let mut tx = pool.begin().await?;

    // ── 1. Le journal, immuable ─────────────────────────────────────────
    sqlx::query(
        "INSERT INTO play_events (
            track_id, session_id, started_at, ended_at, listened_ms, duration_ms,
            completion, end_reason, skip_at_ms, seek_count, pause_count,
            source, source_id, previous_track_id, was_manual_selection,
            output_device, volume, hour_local, weekday, is_weekend
         ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(event.track_id)
    .bind(session_id)
    .bind(event.started_at)
    .bind(event.ended_at)
    .bind(event.listened_ms)
    .bind(event.duration_ms)
    .bind(event.completion)
    .bind(event.end_reason.as_str())
    .bind(event.skip_at_ms)
    .bind(event.seek_count)
    .bind(event.pause_count)
    .bind(event.source.as_str())
    .bind(event.source_id)
    .bind(event.previous_track_id)
    .bind(event.was_manual_selection)
    .bind(event.output_device.as_deref())
    .bind(f64::from(event.volume))
    .bind(event.hour_local)
    .bind(event.weekday)
    .bind(event.is_weekend)
    .execute(&mut *tx)
    .await?;

    // ── 2. Compteurs du morceau ─────────────────────────────────────────
    // SQLite évalue les expressions de droite avec les valeurs *avant* mise à
    // jour : la moyenne mobile de complétion utilise donc bien l'ancien
    // `play_count`, ce qui la rend exacte.
    let completed = i64::from(event.end_reason == EndReason::Completed);
    let skipped = i64::from(event.end_reason == EndReason::Skipped);
    let early_skip = i64::from(event.is_early_skip());

    sqlx::query(
        "UPDATE track_stats SET
             play_count        = play_count + 1,
             completed_count   = completed_count + ?,
             skip_count        = skip_count + ?,
             early_skip_count  = early_skip_count + ?,
             total_listened_ms = total_listened_ms + ?,
             avg_completion    = (avg_completion * play_count + ?) / (play_count + 1),
             first_played_at   = COALESCE(first_played_at, ?),
             last_played_at    = ?
         WHERE track_id = ?",
    )
    .bind(completed)
    .bind(skipped)
    .bind(early_skip)
    .bind(event.listened_ms)
    .bind(event.completion)
    .bind(event.started_at)
    .bind(event.started_at)
    .bind(event.track_id)
    .execute(&mut *tx)
    .await?;

    // ── 3. Matrice de transitions ───────────────────────────────────────
    // Apprend quels enchaînements fonctionnent. Un saut juste après une
    // transition est le signal négatif correspondant.
    if let Some(previous) = event.previous_track_id {
        if previous != event.track_id {
            sqlx::query(
                "INSERT INTO track_transitions
                     (from_track_id, to_track_id, play_count, skip_after_count, last_at)
                 VALUES (?, ?, 1, ?, ?)
                 ON CONFLICT(from_track_id, to_track_id) DO UPDATE SET
                     play_count       = play_count + 1,
                     skip_after_count = skip_after_count + excluded.skip_after_count,
                     last_at          = excluded.last_at",
            )
            .bind(previous)
            .bind(event.track_id)
            .bind(early_skip)
            .bind(event.ended_at)
            .execute(&mut *tx)
            .await?;
        }
    }

    // ── 4. Totaux de la session ─────────────────────────────────────────
    sqlx::query(
        "UPDATE listening_sessions SET
             track_count       = track_count + 1,
             total_listened_ms = total_listened_ms + ?,
             skip_count        = skip_count + ?,
             ended_at          = ?
         WHERE id = ?",
    )
    .bind(event.listened_ms)
    .bind(skipped)
    .bind(event.ended_at)
    .bind(session_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Reprend la dernière session si elle est encore « chaude », sinon en ouvre
/// une nouvelle. Évite de fragmenter une soirée d'écoute en dix sessions au
/// gré des ouvertures de l'application.
pub async fn resume_or_open_session(pool: &SqlitePool) -> Result<String> {
    let recent: Option<(String, i64)> = sqlx::query_as(
        "SELECT id, COALESCE(ended_at, started_at) FROM listening_sessions
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if let Some((id, last_activity)) = recent {
        if now_ms() - last_activity < SESSION_IDLE_TIMEOUT_MS {
            return Ok(id);
        }
    }

    open_session(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tracking::{EndReason, PlayContext, PlaySource, TrackedPlay};

    async fn bac() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        // Deux morceaux, pour pouvoir tester les transitions.
        for (id, titre) in [(1, "Premier"), (2, "Second")] {
            sqlx::query(
                "INSERT INTO tracks (id, title, normalized_title, duration_ms, relative_path,
                                     file_size, content_hash, format, added_at)
                 VALUES (?, ?, ?, 200000, ?, 1, ?, 'mp3', 0)",
            )
            .bind(id)
            .bind(titre)
            .bind(titre.to_lowercase())
            .bind(format!("a/{titre}.mp3"))
            .bind(format!("hash{id}"))
            .execute(&pool)
            .await
            .unwrap();

            sqlx::query("INSERT INTO track_stats (track_id) VALUES (?)")
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }

        (dir, pool)
    }

    fn contexte(track_id: i64, previous: Option<i64>) -> PlayContext {
        PlayContext {
            track_id,
            duration_ms: 200_000,
            source: PlaySource::Library,
            source_id: None,
            previous_track_id: previous,
            was_manual_selection: true,
            output_device: None,
            volume: 1.0,
        }
    }

    #[tokio::test]
    async fn enregistre_une_ecoute_complete() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        let event = TrackedPlay::start(contexte(1, None), 0).finish(200_000, EndReason::Completed);
        record(&pool, &session, &event).await.unwrap();

        let (plays, completed, listened, avg): (i64, i64, i64, f64) = sqlx::query_as(
            "SELECT play_count, completed_count, total_listened_ms, avg_completion
             FROM track_stats WHERE track_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(plays, 1);
        assert_eq!(completed, 1);
        assert_eq!(listened, 200_000);
        assert!((avg - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn la_moyenne_de_completion_est_exacte_sur_plusieurs_ecoutes() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        // Une écoute complète, puis une écoute à moitié.
        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(1, None), 0).finish(200_000, EndReason::Completed),
        )
        .await
        .unwrap();

        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(1, None), 0).finish(100_000, EndReason::Skipped),
        )
        .await
        .unwrap();

        let avg: f64 = sqlx::query_scalar("SELECT avg_completion FROM track_stats WHERE track_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert!(
            (avg - 0.75).abs() < 1e-9,
            "moyenne de 1.0 et 0.5 attendue à 0.75, obtenu {avg}"
        );
    }

    #[tokio::test]
    async fn compte_separement_les_rejets_francs() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        // Saut à 3 s : rejet franc.
        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(1, None), 0).finish(3_000, EndReason::Skipped),
        )
        .await
        .unwrap();

        // Saut à 2 min : simple changement d'humeur.
        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(1, None), 0).finish(120_000, EndReason::Skipped),
        )
        .await
        .unwrap();

        let (skips, early): (i64, i64) = sqlx::query_as(
            "SELECT skip_count, early_skip_count FROM track_stats WHERE track_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(skips, 2);
        assert_eq!(early, 1, "un seul des deux sauts est un rejet franc");
    }

    #[tokio::test]
    async fn apprend_les_enchainements() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        // Le morceau 2 est écouté juste après le 1, deux fois.
        for _ in 0..2 {
            record(
                &pool,
                &session,
                &TrackedPlay::start(contexte(2, Some(1)), 0).finish(200_000, EndReason::Completed),
            )
            .await
            .unwrap();
        }

        let (count, skips): (i64, i64) = sqlx::query_as(
            "SELECT play_count, skip_after_count FROM track_transitions
             WHERE from_track_id = 1 AND to_track_id = 2",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(count, 2);
        assert_eq!(skips, 0);
    }

    #[tokio::test]
    async fn une_transition_rejetee_est_comptee_negativement() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(2, Some(1)), 0).finish(2_000, EndReason::Skipped),
        )
        .await
        .unwrap();

        let skips: i64 = sqlx::query_scalar(
            "SELECT skip_after_count FROM track_transitions WHERE from_track_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(skips, 1, "l'enchaînement 1 → 2 a été rejeté");
    }

    #[tokio::test]
    async fn ne_cree_pas_de_transition_dun_morceau_vers_lui_meme() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        // Morceau rejoué en boucle : ce n'est pas un enchaînement.
        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(1, Some(1)), 0).finish(200_000, EndReason::Completed),
        )
        .await
        .unwrap();

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM track_transitions")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn tient_les_totaux_de_la_session() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(1, None), 0).finish(200_000, EndReason::Completed),
        )
        .await
        .unwrap();
        record(
            &pool,
            &session,
            &TrackedPlay::start(contexte(2, Some(1)), 0).finish(5_000, EndReason::Skipped),
        )
        .await
        .unwrap();

        let (tracks, listened, skips): (i64, i64, i64) = sqlx::query_as(
            "SELECT track_count, total_listened_ms, skip_count FROM listening_sessions WHERE id = ?",
        )
        .bind(&session)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(tracks, 2);
        assert_eq!(listened, 205_000);
        assert_eq!(skips, 1);
    }

    #[tokio::test]
    async fn reprend_une_session_encore_chaude() {
        let (_dir, pool) = bac().await;

        let premiere = resume_or_open_session(&pool).await.unwrap();
        let seconde = resume_or_open_session(&pool).await.unwrap();

        assert_eq!(premiere, seconde, "deux ouvertures rapprochées = une session");
    }

    #[tokio::test]
    async fn ouvre_une_nouvelle_session_apres_une_longue_inactivite() {
        let (_dir, pool) = bac().await;

        let ancienne = open_session(&pool).await.unwrap();
        // On antidate la session au-delà du seuil d'inactivité.
        sqlx::query("UPDATE listening_sessions SET started_at = ?, ended_at = ? WHERE id = ?")
            .bind(now_ms() - SESSION_IDLE_TIMEOUT_MS - 1)
            .bind(now_ms() - SESSION_IDLE_TIMEOUT_MS - 1)
            .bind(&ancienne)
            .execute(&pool)
            .await
            .unwrap();

        assert_ne!(resume_or_open_session(&pool).await.unwrap(), ancienne);
    }

    #[tokio::test]
    async fn le_journal_conserve_tous_les_signaux() {
        let (_dir, pool) = bac().await;
        let session = open_session(&pool).await.unwrap();

        let mut context = contexte(1, Some(2));
        context.source = PlaySource::Reco;
        context.source_id = Some(9);
        context.was_manual_selection = false;
        context.output_device = Some("AirPods".to_string());

        let mut play = TrackedPlay::start(context, 0);
        play.pause(10_000);
        play.resume(20_000);
        play.seek(5_000, 30_000);
        record(&pool, &session, &play.finish(40_000, EndReason::Skipped))
            .await
            .unwrap();

        // Chaque signal est relu individuellement : un tuple à huit éléments
        // rendrait l'échec illisible.
        async fn lire<T>(pool: &SqlitePool, colonne: &str) -> T
        where
            T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
        {
            sqlx::query_scalar(&format!("SELECT {colonne} FROM play_events"))
                .fetch_one(pool)
                .await
                .unwrap()
        }

        assert_eq!(lire::<String>(&pool, "source").await, "reco");
        assert_eq!(lire::<Option<i64>>(&pool, "source_id").await, Some(9));
        assert_eq!(lire::<Option<i64>>(&pool, "previous_track_id").await, Some(2));
        assert!(!lire::<bool>(&pool, "was_manual_selection").await);
        assert_eq!(
            lire::<Option<String>>(&pool, "output_device").await.as_deref(),
            Some("AirPods")
        );
        assert_eq!(
            lire::<Option<i64>>(&pool, "skip_at_ms").await,
            Some(15_000),
            "position 5 s au saut, puis 10 s de lecture"
        );
        assert_eq!(lire::<i64>(&pool, "seek_count").await, 1);
        assert_eq!(lire::<i64>(&pool, "pause_count").await, 1);
    }
}
