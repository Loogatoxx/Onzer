//! Écriture et lecture de la bibliothèque en base.
//!
//! Toute insertion de morceau passe par une **transaction unique** : un import
//! interrompu ne doit jamais laisser un artiste orphelin ou une ligne de
//! recherche pointant vers un morceau inexistant.

use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::core::{now_ms, Result};
use crate::library::naming::normalize_key;
use crate::library::TrackMetadata;

/// Tolérance de durée pour le dédoublonnage par tags.
///
/// Deux encodages du même morceau diffèrent souvent de quelques centaines de
/// millisecondes (silence de fin, gestion des trames). Deux secondes couvrent
/// ce bruit sans risquer de confondre deux titres réellement distincts.
const DURATION_TOLERANCE_MS: i64 = 2_000;

/// Tout ce qu'il faut pour faire entrer un fichier dans la bibliothèque.
pub struct NewTrack<'a> {
    pub metadata: &'a TrackMetadata,
    /// Chemin relatif à la racine, séparateur `/`.
    pub relative_path: &'a str,
    pub file_size: i64,
    pub content_hash: &'a str,
    /// Empreinte des octets audio seuls. Survit à une réécriture des tags.
    pub audio_hash: &'a str,
    pub file_modified_at: Option<i64>,
    /// Empreinte de la vignette de pochette, si une a pu être extraite.
    pub artwork_hash: Option<&'a str>,
    /// `scan`, `manual` ou `auto_import`.
    pub source: &'a str,
}

// ════════════════════════════════════════════════════════════════════════════
//  Détection de doublons
// ════════════════════════════════════════════════════════════════════════════

/// Le fichier est-il déjà connu, à l'octet près ?
pub async fn find_by_content_hash(pool: &SqlitePool, hash: &str) -> Result<Option<i64>> {
    let id = sqlx::query_scalar("SELECT id FROM tracks WHERE content_hash = ? LIMIT 1")
        .bind(hash)
        .fetch_optional(pool)
        .await?;

    Ok(id)
}

/// Le même **audio** est-il déjà connu, quels que soient ses tags ?
///
/// Premier filet du dédoublonnage, et le seul qui résiste à une réécriture des
/// tags : c'est précisément ce qui manquait quand trois exemplaires du même
/// fichier ont fini par entrer en base.
pub async fn find_by_audio_hash(pool: &SqlitePool, hash: &str) -> Result<Option<i64>> {
    let id = sqlx::query_scalar(
        "SELECT id FROM tracks WHERE audio_hash = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;

    Ok(id)
}

/// Le même morceau existe-t-il sous un autre encodage ?
///
/// Seconde passe du dédoublonnage : elle rattrape le même titre ré-encodé en
/// 128 puis en 320 kbps, que l'empreinte de contenu ne peut pas rapprocher.
///
/// **L'artiste fait partie du critère.** S'en tenir au titre et à la durée
/// produirait des faux positifs sur les titres génériques : deux albums
/// différents possèdent très souvent chacun une piste « Intro » de durée
/// voisine, et elles seraient fusionnées à tort.
///
/// Quand l'artiste est inconnu (`None`), la comparaison ne porte que sur le
/// titre et la durée — c'est délibérément plus permissif, faute de mieux.
///
/// `duration_ms` vaut `None` lorsque l'appelant ignore la durée — cas d'un
/// script externe interrogeant la bibliothèque avant de télécharger. Le critère
/// de durée est alors **entièrement écarté**, et non remplacé par une valeur
/// arbitraire qui exclurait tous les morceaux d'une autre longueur.
pub async fn find_by_tags(
    pool: &SqlitePool,
    normalized_title: &str,
    normalized_artist: Option<&str>,
    duration_ms: Option<i64>,
) -> Result<Option<i64>> {
    let lower = duration_ms.map(|duration| duration - DURATION_TOLERANCE_MS);
    let upper = duration_ms.map(|duration| duration + DURATION_TOLERANCE_MS);

    let id = sqlx::query_scalar(
        // Paramètres numérotés : `?4` est réutilisé deux fois dans la clause.
        "SELECT t.id FROM tracks t
         WHERE t.normalized_title = ?1
           AND (?2 IS NULL OR t.duration_ms >= ?2)
           AND (?3 IS NULL OR t.duration_ms <= ?3)
           AND t.deleted_at IS NULL
           AND (
                ?4 IS NULL
                OR EXISTS (
                    SELECT 1 FROM track_artists ta
                      JOIN artists a ON a.id = ta.artist_id
                     WHERE ta.track_id = t.id AND a.normalized_name = ?4
                )
           )
         LIMIT 1",
    )
    .bind(normalized_title)
    .bind(lower)
    .bind(upper)
    .bind(normalized_artist)
    .fetch_optional(pool)
    .await?;

    Ok(id)
}

/// Le chemin est-il déjà occupé en base ? Utilisé au rescan pour distinguer un
/// fichier déjà indexé d'un nouveau venu.
pub async fn find_by_relative_path(pool: &SqlitePool, relative_path: &str) -> Result<Option<i64>> {
    let id = sqlx::query_scalar("SELECT id FROM tracks WHERE relative_path = ? LIMIT 1")
        .bind(relative_path)
        .fetch_optional(pool)
        .await?;

    Ok(id)
}

// ════════════════════════════════════════════════════════════════════════════
//  Insertion
// ════════════════════════════════════════════════════════════════════════════

/// Insère un morceau et tout son entourage, en une seule transaction.
pub async fn insert_track(pool: &SqlitePool, new: NewTrack<'_>) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let now = now_ms();
    let metadata = new.metadata;

    // ── Artiste de rangement et album ───────────────────────────────────
    let filing_artist_id = match metadata.filing_artist() {
        Some(name) => Some(upsert_artist(&mut tx, name, now).await?),
        None => None,
    };

    let album_id = match metadata.album.as_deref() {
        Some(title) => Some(
            upsert_album(
                &mut tx,
                title,
                filing_artist_id,
                metadata.year,
                new.artwork_hash,
                now,
            )
            .await?,
        ),
        None => None,
    };

    // ── Le morceau ──────────────────────────────────────────────────────
    let normalized_title = normalize_key(&metadata.title);

    let track_id: i64 = sqlx::query_scalar(
        "INSERT INTO tracks (
            title, normalized_title, album_id, track_no, disc_no, year, duration_ms,
            relative_path, file_size, content_hash, audio_hash, file_modified_at,
            format, bitrate, sample_rate, channels,
            added_at, last_seen_at, source, lyrics,
            original_title, original_artist, original_album
         ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
         RETURNING id",
    )
    .bind(&metadata.title)
    .bind(&normalized_title)
    .bind(album_id)
    .bind(metadata.track_no)
    .bind(metadata.disc_no)
    .bind(metadata.year)
    .bind(metadata.duration_ms)
    .bind(new.relative_path)
    .bind(new.file_size)
    .bind(new.content_hash)
    .bind(new.audio_hash)
    .bind(new.file_modified_at)
    .bind(&metadata.format)
    .bind(metadata.bitrate)
    .bind(metadata.sample_rate)
    .bind(metadata.channels.map(i64::from))
    .bind(now)
    .bind(now)
    .bind(new.source)
    .bind(metadata.lyrics.as_deref())
    // Ce que le fichier annonçait avant toute réécriture : sans cette mémoire,
    // une identification erronée serait irréversible.
    .bind(&metadata.title)
    .bind(metadata.filing_artist())
    .bind(metadata.album.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    // ── Artistes, avec leur rôle ────────────────────────────────────────
    for (position, name) in metadata.artists.iter().enumerate() {
        let artist_id = upsert_artist(&mut tx, name, now).await?;
        link_artist(&mut tx, track_id, artist_id, "main", position as i64).await?;
    }

    for (position, name) in metadata.featured_artists.iter().enumerate() {
        let artist_id = upsert_artist(&mut tx, name, now).await?;
        link_artist(&mut tx, track_id, artist_id, "featuring", position as i64).await?;
    }

    // ── Genres ──────────────────────────────────────────────────────────
    for genre in &metadata.genres {
        let genre_id = upsert_genre(&mut tx, genre).await?;
        sqlx::query("INSERT OR IGNORE INTO track_genres (track_id, genre_id) VALUES (?, ?)")
            .bind(track_id)
            .bind(genre_id)
            .execute(&mut *tx)
            .await?;
    }

    // ── Index de recherche ──────────────────────────────────────────────
    let searchable_artists = metadata
        .artists
        .iter()
        .chain(metadata.featured_artists.iter())
        .chain(metadata.album_artist.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    sqlx::query(
        "INSERT INTO tracks_fts (track_id, title, artist_names, album_title)
         VALUES (?, ?, ?, ?)",
    )
    .bind(track_id)
    .bind(&metadata.title)
    .bind(searchable_artists)
    .bind(metadata.album.as_deref().unwrap_or(""))
    .execute(&mut *tx)
    .await?;

    // Ligne de statistiques créée d'emblée : évite un LEFT JOIN dans toutes
    // les requêtes de lecture.
    sqlx::query("INSERT INTO track_stats (track_id) VALUES (?)")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(track_id)
}

async fn upsert_artist(tx: &mut Transaction<'_, Sqlite>, name: &str, now: i64) -> Result<i64> {
    let normalized = normalize_key(name);

    // `DO UPDATE SET name = name` est une mise à jour sans effet : elle sert
    // uniquement à rendre la clause RETURNING active lors d'un conflit.
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO artists (name, normalized_name, created_at) VALUES (?, ?, ?)
         ON CONFLICT(normalized_name) DO UPDATE SET name = name
         RETURNING id",
    )
    .bind(name)
    .bind(&normalized)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

async fn upsert_album(
    tx: &mut Transaction<'_, Sqlite>,
    title: &str,
    album_artist_id: Option<i64>,
    year: Option<u32>,
    artwork_hash: Option<&str>,
    now: i64,
) -> Result<i64> {
    let normalized = normalize_key(title);

    // La contrainte UNIQUE porte sur un trio dont deux membres peuvent être
    // NULL, et SQLite considère deux NULL comme distincts : `ON CONFLICT` ne
    // se déclencherait donc pas. D'où la recherche explicite avec `IS`.
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM albums
         WHERE normalized_title = ? AND album_artist_id IS ? AND year IS ?",
    )
    .bind(&normalized)
    .bind(album_artist_id)
    .bind(year)
    .fetch_optional(&mut **tx)
    .await?;

    if let Some(id) = existing {
        // La pochette n'est renseignée qu'une fois : la première piste
        // importée qui en porte une donne son visuel à tout l'album.
        if artwork_hash.is_some() {
            sqlx::query("UPDATE albums SET artwork_hash = COALESCE(artwork_hash, ?) WHERE id = ?")
                .bind(artwork_hash)
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
        return Ok(id);
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO albums (title, normalized_title, album_artist_id, year, artwork_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(title)
    .bind(&normalized)
    .bind(album_artist_id)
    .bind(year)
    .bind(artwork_hash)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

async fn upsert_genre(tx: &mut Transaction<'_, Sqlite>, name: &str) -> Result<i64> {
    let normalized = normalize_key(name);

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO genres (name, normalized_name) VALUES (?, ?)
         ON CONFLICT(normalized_name) DO UPDATE SET name = name
         RETURNING id",
    )
    .bind(name)
    .bind(&normalized)
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

async fn link_artist(
    tx: &mut Transaction<'_, Sqlite>,
    track_id: i64,
    artist_id: i64,
    role: &str,
    position: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO track_artists (track_id, artist_id, role, position)
         VALUES (?, ?, ?, ?)",
    )
    .bind(track_id)
    .bind(artist_id)
    .bind(role)
    .bind(position)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Réécrit l'identité d'un morceau après identification.
///
/// # Pourquoi une mise à jour et non une réinsertion
///
/// Supprimer puis réinsérer serait plus simple à écrire, mais **impossible** :
/// `play_events` référence le morceau en `ON DELETE RESTRICT`, précisément pour
/// que l'historique d'écoute survive. Et c'est heureux — perdre l'historique
/// d'un morceau parce qu'on vient de corriger son titre serait absurde.
///
/// Tout se joue donc en une transaction : artistes, album, genres, index de
/// recherche et ligne du morceau, ou rien.
#[allow(clippy::too_many_arguments)]
pub async fn update_track_identity(
    pool: &SqlitePool,
    track_id: i64,
    metadata: &TrackMetadata,
    relative_path: &str,
    content_hash: &str,
    file_size: i64,
    artwork_hash: Option<&str>,
    recording_mbid: Option<&str>,
    // `score` : confiance de l'empreinte acoustique, entre 0 et 1.
    // `note`  : ce qui a emporté la décision, en clair, affiché à l'utilisateur.
    score: Option<f64>,
    note: Option<&str>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let now = now_ms();

    let filing_artist_id = match metadata.filing_artist() {
        Some(name) => Some(upsert_artist(&mut tx, name, now).await?),
        None => None,
    };

    let album_id = match metadata.album.as_deref() {
        Some(title) => Some(
            upsert_album(&mut tx, title, filing_artist_id, metadata.year, artwork_hash, now).await?,
        ),
        None => None,
    };

    let normalized_title = normalize_key(&metadata.title);

    sqlx::query(
        "UPDATE tracks SET
             title = ?, normalized_title = ?, album_id = ?, track_no = ?, disc_no = ?,
             year = ?, relative_path = ?, content_hash = ?, file_size = ?,
             recording_mbid = ?, identification_state = 'done', identified_at = ?,
             identification_score = ?, identification_note = ?,
             lyrics = COALESCE(?, lyrics), analysis_error = NULL
         WHERE id = ?",
    )
    .bind(&metadata.title)
    .bind(&normalized_title)
    .bind(album_id)
    .bind(metadata.track_no)
    .bind(metadata.disc_no)
    .bind(metadata.year)
    .bind(relative_path)
    .bind(content_hash)
    .bind(file_size)
    .bind(recording_mbid)
    .bind(now)
    .bind(score)
    .bind(note)
    .bind(metadata.lyrics.as_deref())
    .bind(track_id)
    .execute(&mut *tx)
    .await?;

    // Les crédits sont remplacés et non complétés : les anciens venaient de
    // tags approximatifs, les nouveaux d'une base vérifiée.
    sqlx::query("DELETE FROM track_artists WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    for (position, name) in metadata.artists.iter().enumerate() {
        let artist_id = upsert_artist(&mut tx, name, now).await?;
        link_artist(&mut tx, track_id, artist_id, "main", position as i64).await?;
    }
    for (position, name) in metadata.featured_artists.iter().enumerate() {
        let artist_id = upsert_artist(&mut tx, name, now).await?;
        link_artist(&mut tx, track_id, artist_id, "featuring", position as i64).await?;
    }

    sqlx::query("DELETE FROM track_genres WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    for genre in &metadata.genres {
        let genre_id = upsert_genre(&mut tx, genre).await?;
        sqlx::query("INSERT OR IGNORE INTO track_genres (track_id, genre_id) VALUES (?, ?)")
            .bind(track_id)
            .bind(genre_id)
            .execute(&mut *tx)
            .await?;
    }

    // L'index de recherche est reconstruit : il porte des noms d'artistes
    // agrégés qu'aucune mise à jour partielle ne saurait rattraper.
    sqlx::query("DELETE FROM tracks_fts WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    let searchable_artists = metadata
        .artists
        .iter()
        .chain(metadata.featured_artists.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    sqlx::query(
        "INSERT INTO tracks_fts (track_id, title, artist_names, album_title)
         VALUES (?, ?, ?, ?)",
    )
    .bind(track_id)
    .bind(&metadata.title)
    .bind(searchable_artists)
    .bind(metadata.album.as_deref().unwrap_or(""))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Marque une identification comme infructueuse.
pub async fn mark_identification(pool: &SqlitePool, track_id: i64, state: &str) -> Result<()> {
    sqlx::query("UPDATE tracks SET identification_state = ?, identified_at = ? WHERE id = ?")
        .bind(state)
        .bind(now_ms())
        .bind(track_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Consigne le refus d'une correspondance, avec sa raison.
///
/// L'état `rejected` se distingue de `not_found` : le morceau **a** une
/// correspondance dans les bases publiques, mais elle ne résiste pas à la
/// confrontation avec les tags du fichier. Réessayer n'y changerait rien —
/// l'empreinte est déterministe. La raison est conservée pour être montrée :
/// « pourquoi ce morceau n'a-t-il pas été identifié ? » mérite une réponse.
pub async fn mark_identification_rejected(
    pool: &SqlitePool,
    track_id: i64,
    note: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE tracks SET identification_state = 'rejected', identified_at = ?,
                           identification_note = ?
          WHERE id = ?",
    )
    .bind(now_ms())
    .bind(note)
    .bind(track_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Rétablit l'identité d'origine d'un morceau.
///
/// L'état passe à `rejected` et non à `pending` : sans cela l'ouvrier
/// reprendrait le morceau au tour suivant et réappliquerait exactement la
/// correspondance que l'utilisateur vient de refuser.
pub async fn restore_identity(
    pool: &SqlitePool,
    track_id: i64,
    title: &str,
    artist: Option<&str>,
    album: Option<&str>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let now = now_ms();

    let artist_id = match artist.filter(|name| !name.trim().is_empty()) {
        Some(name) => Some(upsert_artist(&mut tx, name, now).await?),
        None => None,
    };

    let album_id = match album.filter(|name| !name.trim().is_empty()) {
        Some(name) => Some(upsert_album(&mut tx, name, artist_id, None, None, now).await?),
        None => None,
    };

    sqlx::query(
        "UPDATE tracks SET title = ?, normalized_title = ?, album_id = ?,
                           identification_state = 'rejected',
                           identification_note = 'tags d''origine rétablis à la main',
                           recording_mbid = NULL
          WHERE id = ?",
    )
    .bind(title)
    .bind(normalize_key(title))
    .bind(album_id)
    .bind(track_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM track_artists WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    if let Some(artist_id) = artist_id {
        link_artist(&mut tx, track_id, artist_id, "main", 0).await?;
    }

    // L'index de recherche doit suivre, sans quoi le morceau resterait
    // trouvable sous le nom qu'on vient d'effacer.
    sqlx::query(
        "UPDATE tracks_fts SET title = ?, artists = ?, album = ? WHERE track_id = ?",
    )
    .bind(title)
    .bind(artist.unwrap_or(""))
    .bind(album.unwrap_or(""))
    .bind(track_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
//  Lecture
// ════════════════════════════════════════════════════════════════════════════

/// Vue d'un morceau destinée à l'interface.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TrackSummary {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub duration_ms: i64,
    pub format: String,
    pub relative_path: String,
    pub is_available: bool,
    pub artwork_hash: Option<String>,
    pub is_loved: bool,
    /// Date d'ajout à la bibliothèque, en secondes Unix. Affichée en colonne.
    pub added_at: i64,
}

/// Liste les morceaux, du plus récemment ajouté au plus ancien.
pub async fn list_tracks(pool: &SqlitePool, limit: i64, offset: i64) -> Result<Vec<TrackSummary>> {
    let tracks = sqlx::query_as::<_, TrackSummary>(
        "SELECT
             t.id,
             t.title,
             -- L'artiste principal, choisi par sa position dans le crédit.
             (SELECT a.name FROM track_artists ta
                JOIN artists a ON a.id = ta.artist_id
               WHERE ta.track_id = t.id AND ta.role = 'main'
               ORDER BY ta.position LIMIT 1)          AS artist,
             al.title                                 AS album,
             t.year,
             t.track_no,
             t.duration_ms,
             t.format,
             t.relative_path,
             t.is_available,
             al.artwork_hash,
             t.is_loved,
             t.added_at
         FROM tracks t
         LEFT JOIN albums al ON al.id = t.album_id
         WHERE t.deleted_at IS NULL
         ORDER BY t.added_at DESC, t.id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(tracks)
}

/// Charge des morceaux par identifiant, **en préservant l'ordre demandé**.
///
/// L'ordre compte : c'est celui de la file de lecture. `IN (…)` ne le garantit
/// pas, le tri est donc refait côté Rust.
pub async fn tracks_by_ids(pool: &SqlitePool, ids: &[i64]) -> Result<Vec<TrackSummary>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", ids.len()).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT
             t.id, t.title,
             (SELECT a.name FROM track_artists ta
                JOIN artists a ON a.id = ta.artist_id
               WHERE ta.track_id = t.id AND ta.role = 'main'
               ORDER BY ta.position LIMIT 1)          AS artist,
             al.title AS album, t.year, t.track_no, t.duration_ms, t.format,
             t.relative_path, t.is_available, al.artwork_hash, t.is_loved, t.added_at
         FROM tracks t
         LEFT JOIN albums al ON al.id = t.album_id
         WHERE t.id IN ({placeholders}) AND t.deleted_at IS NULL"
    );

    let mut query = sqlx::query_as::<_, TrackSummary>(&sql);
    for id in ids {
        query = query.bind(id);
    }

    let found = query.fetch_all(pool).await?;

    let mut by_id: std::collections::HashMap<i64, TrackSummary> =
        found.into_iter().map(|track| (track.id, track)).collect();

    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// Recherche plein texte. Le terme est protégé pour que la ponctuation saisie
/// par l'utilisateur ne soit pas interprétée comme de la syntaxe FTS5.
pub async fn search_tracks(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<TrackSummary>> {
    let sanitized = query.replace('"', " ");
    let terms = sanitized
        .split_whitespace()
        .map(|term| format!("\"{term}\"*"))
        .collect::<Vec<_>>()
        .join(" ");

    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let tracks = sqlx::query_as::<_, TrackSummary>(
        "SELECT
             t.id, t.title,
             (SELECT a.name FROM track_artists ta
                JOIN artists a ON a.id = ta.artist_id
               WHERE ta.track_id = t.id AND ta.role = 'main'
               ORDER BY ta.position LIMIT 1)          AS artist,
             al.title AS album, t.year, t.track_no, t.duration_ms, t.format,
             t.relative_path, t.is_available, al.artwork_hash, t.is_loved, t.added_at
         FROM tracks_fts f
         JOIN tracks t ON t.id = f.track_id
         LEFT JOIN albums al ON al.id = t.album_id
         WHERE tracks_fts MATCH ? AND t.deleted_at IS NULL
         ORDER BY rank
         LIMIT ?",
    )
    .bind(terms)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(tracks)
}

/// Compteurs affichés dans l'interface.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCounts {
    pub tracks: i64,
    pub artists: i64,
    pub albums: i64,
    pub unavailable: i64,
}

pub async fn counts(pool: &SqlitePool) -> Result<LibraryCounts> {
    Ok(LibraryCounts {
        tracks: scalar(pool, "SELECT COUNT(*) FROM tracks WHERE deleted_at IS NULL").await?,
        artists: scalar(pool, "SELECT COUNT(*) FROM artists").await?,
        albums: scalar(pool, "SELECT COUNT(*) FROM albums").await?,
        unavailable: scalar(
            pool,
            "SELECT COUNT(*) FROM tracks WHERE is_available = 0 AND deleted_at IS NULL",
        )
        .await?,
    })
}

async fn scalar(pool: &SqlitePool, query: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(query).fetch_one(pool).await?)
}

/// Journalise un import, en conservant le chemin d'origine.
///
/// L'import déplaçant les fichiers (ADR-007), c'est cette trace qui rend
/// l'opération annulable.
pub async fn record_import(
    pool: &SqlitePool,
    source_path: &str,
    destination_path: Option<&str>,
    origin: &str,
    state: &str,
    track_id: Option<i64>,
    error: Option<&str>,
) -> Result<()> {
    let now = now_ms();

    sqlx::query(
        "INSERT INTO import_jobs
             (source_path, destination_path, origin, state, track_id, error, created_at, completed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(source_path)
    .bind(destination_path)
    .bind(origin)
    .bind(state)
    .bind(track_id)
    .bind(error)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}
