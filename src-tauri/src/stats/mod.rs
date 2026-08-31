//! Statistiques d'écoute.
//!
//! # Les quatre règles qui gardent les requêtes rapides
//!
//! **1. Une seule passe par agrégat, jamais de N+1.**
//! Chaque classement est un unique `GROUP BY`. La tentation serait de récupérer
//! les morceaux puis d'interroger la base pour chacun : sur 5 000 titres, cela
//! ferait 5 000 requêtes là où une seule suffit.
//!
//! **2. Filtrer avant d'agréger.**
//! Toutes les requêtes commencent par `started_at >= ?`, servi par
//! `idx_events_time`. SQLite réduit ainsi l'ensemble de travail avant tout
//! calcul, plutôt que de balayer l'historique entier.
//!
//! **3. Enrichir *après* avoir limité.**
//! Le nom de l'artiste d'un morceau demande une sous-requête. Appliquée avant
//! le `LIMIT`, elle s'exécuterait pour chaque titre écouté ; appliquée après,
//! seulement dix fois. D'où la forme systématique :
//!
//! ```sql
//! WITH top AS (SELECT … GROUP BY … ORDER BY … LIMIT 10)
//! SELECT … FROM top JOIN tracks …   -- l'enrichissement ne coûte que 10 fois
//! ```
//!
//! **4. Le contexte horaire est déjà dénormalisé.**
//! `hour_local` et `weekday` sont écrits au moment de l'écoute (ADR-008). Aucun
//! calcul de fuseau horaire n'a lieu à la lecture — ce qui, sur des centaines
//! de milliers de lignes, ferait toute la différence.
//!
//! # Ce qui compte comme une écoute
//!
//! Un morceau abandonné au bout de trois secondes ne doit pas gonfler un
//! classement. Comme les grandes plateformes, on ne compte donc que les écoutes
//! **qualifiées** : au moins 30 secondes, ou la moitié du morceau.
//!
//! Les statistiques de comportement, elles, portent sur **toutes** les écoutes :
//! un taux de zapping calculé en excluant les zappings n'aurait aucun sens.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::core::{now_ms, Result};

/// Durée minimale pour qu'une écoute compte dans les classements.
const QUALIFYING_MS: i64 = 30_000;

/// Ou, pour les morceaux courts, la moitié écoutée.
const QUALIFYING_COMPLETION: f64 = 0.5;

/// Condition SQL d'une écoute qualifiée, dérivée des constantes ci-dessus.
///
/// Construite plutôt qu'écrite en dur : les seuils n'existent ainsi qu'à un
/// seul endroit, et modifier `QUALIFYING_MS` met à jour toutes les requêtes.
/// `alias` désigne la table d'événements dans la requête appelante.
fn qualified(alias: &str) -> String {
    format!(
        "({alias}.listened_ms >= {QUALIFYING_MS} OR {alias}.completion >= {QUALIFYING_COMPLETION})"
    )
}

/// Période observée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Period {
    Days(i64),
    AllTime,
}

impl Period {
    /// Horodatage à partir duquel les écoutes comptent.
    pub fn cutoff(self, now: i64) -> i64 {
        match self {
            Self::Days(days) => now - days.max(0) * 86_400_000,
            Self::AllTime => 0,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Days(7) => "cette semaine".to_string(),
            Self::Days(30) => "ces 30 derniers jours".to_string(),
            Self::Days(365) => "cette année".to_string(),
            Self::Days(days) => format!("ces {days} derniers jours"),
            Self::AllTime => "depuis le début".to_string(),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Formes exposées
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ArtistStat {
    pub id: i64,
    pub name: String,
    pub plays: i64,
    pub listened_ms: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TrackStat {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub artwork_hash: Option<String>,
    pub plays: i64,
    pub listened_ms: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AlbumStat {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub artwork_hash: Option<String>,
    pub plays: i64,
    pub listened_ms: i64,
}

/// Une heure de la journée, et ce qu'on y écoute.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourSlice {
    pub hour: i64,
    pub plays: i64,
    pub listened_ms: i64,
}

/// Comportement d'écoute.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Behaviour {
    pub total_plays: i64,
    pub qualified_plays: i64,
    /// Part d'écoutes abandonnées dans les quinze premières secondes.
    pub early_skip_rate: f64,
    pub completion_rate: f64,
    pub avg_completion: f64,
    /// Heure où l'on écoute le plus. `None` sans historique.
    pub peak_hour: Option<i64>,
    /// Part des écoutes tombant le week-end.
    pub weekend_share: f64,
    /// Part des écoutes choisies délibérément, plutôt que subies dans une file.
    pub manual_share: f64,
}

/// Grands totaux.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub listened_ms: i64,
    pub distinct_tracks: i64,
    pub distinct_artists: i64,
    pub sessions: i64,
    /// Durée de la plus longue session d'écoute.
    pub longest_session_ms: i64,
    /// Morceaux découverts sur la période — première écoute enregistrée.
    pub discoveries: i64,
}

/// Tout ce qu'affiche la page, en un seul aller-retour.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wrapped {
    pub period_label: String,
    pub totals: Totals,
    pub behaviour: Behaviour,
    pub top_artists: Vec<ArtistStat>,
    pub top_tracks: Vec<TrackStat>,
    pub top_albums: Vec<AlbumStat>,
    pub clock: Vec<HourSlice>,
    /// Portrait d'auditeur déduit des chiffres.
    pub persona: Persona,
    /// Faux quand l'historique est trop mince pour être parlant.
    pub has_enough_data: bool,
}

/// Portrait d'auditeur.
///
/// C'est ce qui transforme un tableau de chiffres en quelque chose qu'on a
/// envie de lire — à condition qu'il soit **déduit des données** et non tiré au
/// sort.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub title: String,
    pub description: String,
}

/// Nombre d'écoutes en deçà duquel les statistiques n'ont rien à raconter.
const MIN_PLAYS_FOR_INSIGHT: i64 = 20;

// ════════════════════════════════════════════════════════════════════════════
//  Requêtes
// ════════════════════════════════════════════════════════════════════════════

/// Rassemble toutes les statistiques de la période.
pub async fn wrapped(pool: &SqlitePool, period: Period, top: i64) -> Result<Wrapped> {
    let cutoff = period.cutoff(now_ms());
    let top = top.clamp(1, 50);

    let totals = totals(pool, cutoff).await?;
    let clock = clock(pool, cutoff).await?;
    let behaviour = behaviour(pool, cutoff, &clock).await?;

    Ok(Wrapped {
        period_label: period.label(),
        has_enough_data: behaviour.total_plays >= MIN_PLAYS_FOR_INSIGHT,
        persona: persona(&behaviour, &totals),
        top_artists: top_artists(pool, cutoff, top).await?,
        top_tracks: top_tracks(pool, cutoff, top).await?,
        top_albums: top_albums(pool, cutoff, top).await?,
        totals,
        behaviour,
        clock,
    })
}

/// Classement des artistes.
///
/// `ta.position = 0` ne retient que l'artiste principal : sans cette clause, un
/// morceau crédité à trois artistes compterait trois fois.
async fn top_artists(pool: &SqlitePool, cutoff: i64, limit: i64) -> Result<Vec<ArtistStat>> {
    let qualified = qualified("e");

    let rows = sqlx::query_as::<_, ArtistStat>(&format!(
        "SELECT a.id, a.name,
                COUNT(*)              AS plays,
                SUM(e.listened_ms)    AS listened_ms
           FROM play_events e
           JOIN track_artists ta ON ta.track_id = e.track_id
                                AND ta.role = 'main' AND ta.position = 0
           JOIN artists a ON a.id = ta.artist_id
          WHERE e.started_at >= ? AND {qualified}
          GROUP BY a.id
          ORDER BY listened_ms DESC
          LIMIT ?"
    ))
    .bind(cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Classement des morceaux.
///
/// L'agrégation est faite d'abord, l'enrichissement ensuite : le nom de
/// l'artiste et la pochette ne sont résolus que pour les titres retenus.
async fn top_tracks(pool: &SqlitePool, cutoff: i64, limit: i64) -> Result<Vec<TrackStat>> {
    let qualified = qualified("e");

    let rows = sqlx::query_as::<_, TrackStat>(&format!(
        "WITH top AS (
             SELECT e.track_id,
                    COUNT(*)           AS plays,
                    SUM(e.listened_ms) AS listened_ms
               FROM play_events e
              WHERE e.started_at >= ? AND {qualified}
              GROUP BY e.track_id
              ORDER BY listened_ms DESC
              LIMIT ?
         )
         SELECT t.id,
                t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist,
                al.artwork_hash,
                top.plays,
                top.listened_ms
           FROM top
           JOIN tracks t  ON t.id = top.track_id
      LEFT JOIN albums al ON al.id = t.album_id
          ORDER BY top.listened_ms DESC"
    ))
    .bind(cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

async fn top_albums(pool: &SqlitePool, cutoff: i64, limit: i64) -> Result<Vec<AlbumStat>> {
    let qualified = qualified("e");

    let rows = sqlx::query_as::<_, AlbumStat>(&format!(
        "WITH top AS (
             SELECT t.album_id,
                    COUNT(*)           AS plays,
                    SUM(e.listened_ms) AS listened_ms
               FROM play_events e
               JOIN tracks t ON t.id = e.track_id
              WHERE e.started_at >= ? AND {qualified} AND t.album_id IS NOT NULL
              GROUP BY t.album_id
              ORDER BY listened_ms DESC
              LIMIT ?
         )
         SELECT al.id,
                al.title,
                ar.name AS artist,
                al.artwork_hash,
                top.plays,
                top.listened_ms
           FROM top
           JOIN albums al  ON al.id = top.album_id
      LEFT JOIN artists ar ON ar.id = al.album_artist_id
          ORDER BY top.listened_ms DESC"
    ))
    .bind(cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Répartition des écoutes sur les 24 heures.
///
/// Les heures sans écoute sont complétées côté Rust : une horloge à trous
/// serait illisible, et laisser le SQL générer les heures manquantes coûterait
/// plus cher qu'une boucle sur 24 éléments.
async fn clock(pool: &SqlitePool, cutoff: i64) -> Result<Vec<HourSlice>> {
    let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT hour_local, COUNT(*), COALESCE(SUM(listened_ms), 0)
           FROM play_events
          WHERE started_at >= ?
          GROUP BY hour_local",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;

    let mut clock: Vec<HourSlice> = (0..24)
        .map(|hour| HourSlice {
            hour,
            plays: 0,
            listened_ms: 0,
        })
        .collect();

    for (hour, plays, listened_ms) in rows {
        if let Some(slice) = clock.get_mut(hour.clamp(0, 23) as usize) {
            slice.plays = plays;
            slice.listened_ms = listened_ms;
        }
    }

    Ok(clock)
}

/// Comportement d'écoute.
///
/// Porte sur **toutes** les écoutes, y compris les plus brèves : un taux de
/// zapping calculé en excluant les zappings ne voudrait rien dire.
async fn behaviour(pool: &SqlitePool, cutoff: i64, clock: &[HourSlice]) -> Result<Behaviour> {
    let row: (i64, i64, i64, i64, i64, Option<f64>) = sqlx::query_as(&format!(
        "SELECT
             COUNT(*),
             SUM(CASE WHEN {} THEN 1 ELSE 0 END),
             SUM(CASE WHEN end_reason = 'skipped'
                       AND skip_at_ms IS NOT NULL
                       AND skip_at_ms < 15000 THEN 1 ELSE 0 END),
             SUM(CASE WHEN end_reason = 'completed' THEN 1 ELSE 0 END),
             SUM(CASE WHEN is_weekend THEN 1 ELSE 0 END),
             AVG(completion)
           FROM play_events e
          WHERE started_at >= ?",
        qualified("e")
    ))
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let (total, qualified, early_skips, completed, weekend, avg_completion) = row;

    let manual: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM play_events
          WHERE started_at >= ? AND was_manual_selection = 1",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let share = |part: i64| {
        if total == 0 {
            0.0
        } else {
            part as f64 / total as f64
        }
    };

    // L'heure de pointe se mesure en temps écouté, non en nombre de lancements :
    // vingt morceaux zappés à midi ne font pas de midi un moment d'écoute.
    let peak_hour = clock
        .iter()
        .filter(|slice| slice.listened_ms > 0)
        .max_by_key(|slice| slice.listened_ms)
        .map(|slice| slice.hour);

    Ok(Behaviour {
        total_plays: total,
        qualified_plays: qualified,
        early_skip_rate: share(early_skips),
        completion_rate: share(completed),
        avg_completion: avg_completion.unwrap_or(0.0),
        peak_hour,
        weekend_share: share(weekend),
        manual_share: share(manual),
    })
}

async fn totals(pool: &SqlitePool, cutoff: i64) -> Result<Totals> {
    let qualified = qualified("e");

    let row: (i64, i64, i64) = sqlx::query_as(&format!(
        "SELECT COALESCE(SUM(e.listened_ms), 0),
                COUNT(DISTINCT e.track_id),
                COUNT(DISTINCT e.session_id)
           FROM play_events e
          WHERE e.started_at >= ? AND {qualified}"
    ))
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let distinct_artists: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT ta.artist_id)
           FROM play_events e
           JOIN track_artists ta ON ta.track_id = e.track_id
                                AND ta.role = 'main' AND ta.position = 0
          WHERE e.started_at >= ? AND {qualified}"
    ))
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let longest_session: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(total_listened_ms) FROM listening_sessions WHERE started_at >= ?",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    // Une découverte est un morceau dont la toute première écoute tombe dans la
    // période. `track_stats.first_played_at` évite d'avoir à parcourir le
    // journal pour le savoir.
    let discoveries: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM track_stats WHERE first_played_at >= ?",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    Ok(Totals {
        listened_ms: row.0,
        distinct_tracks: row.1,
        sessions: row.2,
        distinct_artists,
        longest_session_ms: longest_session.unwrap_or(0),
        discoveries,
    })
}

/// Déduit un portrait d'auditeur des chiffres observés.
///
/// Chaque portrait correspond à un **seuil explicite**, jamais à un tirage : un
/// portrait qui changerait à chaque affichage ne voudrait rien dire.
fn persona(behaviour: &Behaviour, totals: &Totals) -> Persona {
    let (title, description) = if behaviour.total_plays < MIN_PLAYS_FOR_INSIGHT {
        (
            "Le Nouveau Venu",
            "Encore trop peu d'écoutes pour dresser un portrait. Reviens dans quelques jours.",
        )
    } else if behaviour.early_skip_rate > 0.30 {
        (
            "Le Chasseur",
            "Tu zappes vite et sans remords. Trois secondes suffisent à juger un morceau.",
        )
    } else if behaviour.completion_rate > 0.80 {
        (
            "Le Fidèle",
            "Tu écoutes les morceaux jusqu'au bout. Rare, et précieux pour le moteur de recommandation.",
        )
    } else if behaviour.manual_share > 0.70 {
        (
            "Le Sélectionneur",
            "Tu choisis presque toujours ce que tu écoutes. Les files automatiques ne te suffisent pas.",
        )
    } else if totals.discoveries > totals.distinct_tracks / 3 {
        (
            "L'Explorateur",
            "Un tiers de tes écoutes sont des découvertes. Ta bibliothèque bouge sans arrêt.",
        )
    } else if matches!(behaviour.peak_hour, Some(hour) if !(6..22).contains(&hour)) {
        (
            "Le Noctambule",
            "Tes meilleures écoutes se passent quand les autres dorment.",
        )
    } else {
        (
            "L'Équilibré",
            "Ni zappeur ni intégriste. Tu écoutes ce qui te plaît, quand ça te plaît.",
        )
    };

    Persona {
        title: title.to_string(),
        description: description.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn base() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();

        for artiste in 1..=3_i64 {
            sqlx::query("INSERT INTO artists (id, name, normalized_name, created_at) VALUES (?,?,?,0)")
                .bind(artiste)
                .bind(format!("Artiste {artiste}"))
                .bind(format!("artiste {artiste}"))
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query(
                "INSERT INTO albums (id, title, normalized_title, album_artist_id, created_at)
                 VALUES (?,?,?,?,0)",
            )
            .bind(artiste)
            .bind(format!("Album {artiste}"))
            .bind(format!("album {artiste}"))
            .bind(artiste)
            .execute(&pool)
            .await
            .unwrap();
        }

        for track in 1..=9_i64 {
            let artiste = (track - 1) % 3 + 1;

            sqlx::query(
                "INSERT INTO tracks (id, title, normalized_title, album_id, duration_ms,
                                     relative_path, file_size, content_hash, format, added_at)
                 VALUES (?,?,?,?,200000,?,1,?,'mp3',0)",
            )
            .bind(track)
            .bind(format!("Titre {track}"))
            .bind(format!("titre {track}"))
            .bind(artiste)
            .bind(format!("a/{track}.mp3"))
            .bind(format!("hash{track}"))
            .execute(&pool)
            .await
            .unwrap();

            sqlx::query("INSERT INTO track_stats (track_id) VALUES (?)")
                .bind(track)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query(
                "INSERT INTO track_artists (track_id, artist_id, role, position)
                 VALUES (?,?,'main',0)",
            )
            .bind(track)
            .bind(artiste)
            .execute(&pool)
            .await
            .unwrap();
        }

        sqlx::query("INSERT INTO listening_sessions (id, started_at, total_listened_ms) VALUES ('s1', 0, 500000)")
            .execute(&pool)
            .await
            .unwrap();

        (dir, pool)
    }

    #[allow(clippy::too_many_arguments)]
    async fn ecoute(
        pool: &SqlitePool,
        track_id: i64,
        listened_ms: i64,
        completion: f64,
        reason: &str,
        skip_at: Option<i64>,
        hour: i64,
        manual: bool,
    ) {
        sqlx::query(
            "INSERT INTO play_events (track_id, session_id, started_at, listened_ms,
                                      duration_ms, completion, end_reason, skip_at_ms,
                                      was_manual_selection, hour_local, weekday, is_weekend)
             VALUES (?, 's1', ?, ?, 200000, ?, ?, ?, ?, ?, 1, 0)",
        )
        .bind(track_id)
        .bind(now_ms() - 3_600_000)
        .bind(listened_ms)
        .bind(completion)
        .bind(reason)
        .bind(skip_at)
        .bind(manual)
        .bind(hour)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Écoute complète, qui compte dans les classements.
    async fn complete(pool: &SqlitePool, track_id: i64, hour: i64) {
        ecoute(pool, track_id, 200_000, 1.0, "completed", None, hour, false).await;
    }

    /// Zapping immédiat, qui ne compte pas.
    async fn zapping(pool: &SqlitePool, track_id: i64, hour: i64) {
        ecoute(pool, track_id, 3_000, 0.015, "skipped", Some(3_000), hour, false).await;
    }

    // ── Périodes ────────────────────────────────────────────────────────

    #[test]
    fn la_periode_calcule_la_bonne_borne() {
        let now = 1_000_000_000_000_i64;

        assert_eq!(Period::AllTime.cutoff(now), 0);
        assert_eq!(Period::Days(1).cutoff(now), now - 86_400_000);
        assert_eq!(Period::Days(30).cutoff(now), now - 30 * 86_400_000);
    }

    #[test]
    fn les_periodes_ont_un_libelle_lisible() {
        assert_eq!(Period::Days(7).label(), "cette semaine");
        assert_eq!(Period::Days(365).label(), "cette année");
        assert_eq!(Period::AllTime.label(), "depuis le début");
    }

    // ── Écoutes qualifiées ──────────────────────────────────────────────

    #[tokio::test]
    async fn un_zapping_ne_gonfle_pas_les_classements() {
        // LE principe : un morceau abandonné en trois secondes n'a pas sa place
        // dans un top, même lancé cent fois.
        let (_dir, pool) = base().await;

        complete(&pool, 1, 20).await;
        for _ in 0..30 {
            zapping(&pool, 2, 20).await;
        }

        let tops = top_tracks(&pool, 0, 10).await.unwrap();

        assert_eq!(tops.len(), 1, "seul le morceau réellement écouté compte");
        assert_eq!(tops[0].id, 1);
    }

    #[tokio::test]
    async fn une_ecoute_a_moitie_compte() {
        let (_dir, pool) = base().await;
        ecoute(&pool, 1, 100_000, 0.5, "skipped", Some(100_000), 20, false).await;

        assert_eq!(top_tracks(&pool, 0, 10).await.unwrap().len(), 1);
    }

    #[test]
    fn la_clause_de_qualification_derive_des_constantes() {
        // Les seuils n'existent qu'à un seul endroit : modifier la constante
        // doit suffire à changer toutes les requêtes.
        let clause = qualified("e");

        assert!(clause.contains(&QUALIFYING_MS.to_string()), "obtenu {clause}");
        assert!(clause.contains(&QUALIFYING_COMPLETION.to_string()));
        assert!(clause.contains("e.listened_ms"));
    }

    #[test]
    fn la_clause_sadapte_a_lalias_de_la_requete() {
        assert!(qualified("ev").contains("ev.completion"));
    }

    // ── Classements ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn les_artistes_sont_classes_par_temps_ecoute() {
        let (_dir, pool) = base().await;

        // Artiste 1 : morceaux 1, 4, 7 — trois écoutes.
        for track in [1, 4, 7] {
            complete(&pool, track, 20).await;
        }
        // Artiste 2 : morceau 2 — une seule.
        complete(&pool, 2, 20).await;

        let tops = top_artists(&pool, 0, 10).await.unwrap();

        assert_eq!(tops[0].name, "Artiste 1");
        assert_eq!(tops[0].plays, 3);
        assert_eq!(tops[0].listened_ms, 600_000);
    }

    #[tokio::test]
    async fn un_morceau_a_plusieurs_artistes_ne_compte_quune_fois() {
        // Sans la clause sur la position, un morceau crédité à trois artistes
        // gonflerait trois classements.
        let (_dir, pool) = base().await;

        for (artiste, position) in [(2_i64, 1_i64), (3, 2)] {
            sqlx::query(
                "INSERT INTO track_artists (track_id, artist_id, role, position)
                 VALUES (1, ?, 'main', ?)",
            )
            .bind(artiste)
            .bind(position)
            .execute(&pool)
            .await
            .unwrap();
        }

        complete(&pool, 1, 20).await;
        let tops = top_artists(&pool, 0, 10).await.unwrap();

        assert_eq!(tops.len(), 1, "un seul artiste principal : {tops:?}");
    }

    #[tokio::test]
    async fn les_albums_sont_classes_et_enrichis() {
        let (_dir, pool) = base().await;
        for track in [1, 4] {
            complete(&pool, track, 20).await;
        }

        let tops = top_albums(&pool, 0, 10).await.unwrap();

        assert_eq!(tops[0].title, "Album 1");
        assert_eq!(tops[0].artist.as_deref(), Some("Artiste 1"));
        assert_eq!(tops[0].plays, 2);
    }

    #[tokio::test]
    async fn les_classements_respectent_la_limite() {
        let (_dir, pool) = base().await;
        for track in 1..=9 {
            complete(&pool, track, 20).await;
        }

        assert_eq!(top_tracks(&pool, 0, 3).await.unwrap().len(), 3);
        assert_eq!(top_artists(&pool, 0, 2).await.unwrap().len(), 2);
    }

    // ── Horloge ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn lhorloge_couvre_les_vingt_quatre_heures() {
        // Une horloge à trous serait illisible.
        let (_dir, pool) = base().await;
        complete(&pool, 1, 20).await;

        let clock = clock(&pool, 0).await.unwrap();

        assert_eq!(clock.len(), 24);
        assert_eq!(clock[20].plays, 1);
        assert_eq!(clock[3].plays, 0);
    }

    #[tokio::test]
    async fn lheure_de_pointe_se_mesure_en_temps_ecoute() {
        // Vingt morceaux zappés à midi ne font pas de midi un moment d'écoute.
        let (_dir, pool) = base().await;

        for _ in 0..20 {
            zapping(&pool, 1, 12).await;
        }
        complete(&pool, 2, 23).await;

        let clock = clock(&pool, 0).await.unwrap();
        let comportement = behaviour(&pool, 0, &clock).await.unwrap();

        assert_eq!(comportement.peak_hour, Some(23));
    }

    // ── Comportement ────────────────────────────────────────────────────

    #[tokio::test]
    async fn le_taux_de_zapping_porte_sur_toutes_les_ecoutes() {
        // L'exclure des écoutes qualifiées le rendrait toujours nul.
        let (_dir, pool) = base().await;

        for _ in 0..3 {
            zapping(&pool, 1, 20).await;
        }
        complete(&pool, 2, 20).await;

        let clock = clock(&pool, 0).await.unwrap();
        let comportement = behaviour(&pool, 0, &clock).await.unwrap();

        assert_eq!(comportement.total_plays, 4);
        assert_eq!(comportement.qualified_plays, 1);
        assert!((comportement.early_skip_rate - 0.75).abs() < 1e-9);
        assert!((comportement.completion_rate - 0.25).abs() < 1e-9);
    }

    #[tokio::test]
    async fn un_historique_vide_ne_provoque_pas_de_division_par_zero() {
        let (_dir, pool) = base().await;
        let resume = wrapped(&pool, Period::AllTime, 10).await.unwrap();

        assert_eq!(resume.behaviour.total_plays, 0);
        assert_eq!(resume.behaviour.early_skip_rate, 0.0);
        assert_eq!(resume.behaviour.peak_hour, None);
        assert_eq!(resume.totals.listened_ms, 0);
        assert!(!resume.has_enough_data);
    }

    // ── Portrait ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn le_portrait_attend_davoir_de_quoi_parler() {
        let (_dir, pool) = base().await;
        complete(&pool, 1, 20).await;

        let resume = wrapped(&pool, Period::AllTime, 10).await.unwrap();

        assert_eq!(resume.persona.title, "Le Nouveau Venu");
        assert!(!resume.has_enough_data);
    }

    #[tokio::test]
    async fn un_zappeur_est_reconnu_comme_tel() {
        let (_dir, pool) = base().await;

        for _ in 0..20 {
            zapping(&pool, 1, 20).await;
        }
        for _ in 0..10 {
            complete(&pool, 2, 20).await;
        }

        let resume = wrapped(&pool, Period::AllTime, 10).await.unwrap();

        assert_eq!(resume.persona.title, "Le Chasseur");
        assert!(resume.has_enough_data);
    }

    #[tokio::test]
    async fn un_auditeur_fidele_est_reconnu_comme_tel() {
        let (_dir, pool) = base().await;
        for _ in 0..25 {
            complete(&pool, 1, 20).await;
        }

        assert_eq!(
            wrapped(&pool, Period::AllTime, 10).await.unwrap().persona.title,
            "Le Fidèle"
        );
    }

    #[tokio::test]
    async fn le_portrait_est_stable_dun_affichage_a_lautre() {
        // Un portrait qui changerait à chaque consultation ne voudrait rien
        // dire : il est déduit de seuils, jamais tiré au sort.
        let (_dir, pool) = base().await;
        for _ in 0..25 {
            complete(&pool, 1, 20).await;
        }

        let premier = wrapped(&pool, Period::AllTime, 10).await.unwrap();
        let second = wrapped(&pool, Period::AllTime, 10).await.unwrap();

        assert_eq!(premier.persona.title, second.persona.title);
    }

    // ── Totaux ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn les_totaux_comptent_ce_quil_faut() {
        let (_dir, pool) = base().await;
        for track in [1, 2, 4] {
            complete(&pool, track, 20).await;
        }

        let totaux = totals(&pool, 0).await.unwrap();

        assert_eq!(totaux.listened_ms, 600_000);
        assert_eq!(totaux.distinct_tracks, 3);
        assert_eq!(totaux.distinct_artists, 2, "morceaux 1 et 4 : même artiste");
        assert_eq!(totaux.sessions, 1);
        assert_eq!(totaux.longest_session_ms, 500_000);
    }
}
