//! Commandes de gestion de la bibliothèque.
//!
//! Rappel de l'ADR-004 : aucune logique ici. Ces fonctions valident, délèguent
//! au module `library`, et convertissent le résultat pour le frontend.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::core::{OnzerError, Result};
use crate::db::{repository, settings};
use crate::library::{scanner, FileHandling};
use crate::AppState;

/// Nom de l'événement de progression écouté par l'interface.
pub const SCAN_PROGRESS_EVENT: &str = "library://scan-progress";

/// Enregistre la racine de bibliothèque choisie par l'utilisateur.
///
/// Le dossier est créé s'il n'existe pas encore : au premier lancement, la
/// bibliothèque est vide et son dossier n'a aucune raison d'exister déjà.
#[tauri::command]
pub async fn set_library_root(state: State<'_, AppState>, path: String) -> Result<()> {
    let root = PathBuf::from(&path);

    if root.exists() && !root.is_dir() {
        return Err(OnzerError::Invalid(format!(
            "« {path} » n'est pas un dossier"
        )));
    }

    std::fs::create_dir_all(&root)?;

    settings::set(&state.pool, settings::LIBRARY_ROOT, &path).await?;

    // Le nom du volume est conservé à titre de diagnostic : il permettra
    // d'afficher « Rebranche Lexar » plutôt qu'un chemin cryptique.
    if let Some(volume) = volume_name(&root) {
        settings::set(&state.pool, settings::LIBRARY_VOLUME, &volume).await?;
    }

    state.paths.write().await.set_library_root(Some(root));

    tracing::info!(racine = %path, "racine de bibliothèque définie");
    Ok(())
}

/// Extrait « Lexar » de « /Volumes/Lexar/Musique ».
fn volume_name(path: &std::path::Path) -> Option<String> {
    let mut components = path.components();
    if components.next()? != std::path::Component::RootDir {
        return None;
    }
    if components.next()?.as_os_str() != "Volumes" {
        return None;
    }
    Some(components.next()?.as_os_str().to_string_lossy().to_string())
}

/// Importe le contenu audio d'un dossier dans la bibliothèque.
///
/// # `organize` : ranger, ou seulement découvrir
///
/// « Importer un dossier » exprime l'intention de faire **ranger**, et c'est
/// le défaut. « Chercher de nouveaux morceaux » n'exprime que celle de
/// découvrir : lancé sur une bibliothèque déjà en ordre, un rangement complet
/// déplacerait des milliers de fichiers pour retrouver, au mieux, exactement
/// la même arborescence — et au pire en réorganiserait une partie sur la foi
/// de tags qui ont changé depuis. Ce n'est pas ce qu'on a demandé.
///
/// Les fichiers sont **déplacés** et rangés selon l'ADR-007. La progression est
/// émise au fil de l'eau plutôt que retournée à la fin : sur plusieurs milliers
/// de titres, l'interface doit rester vivante.
#[tauri::command]
pub async fn import_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
    organize: Option<bool>,
) -> Result<scanner::ScanSummary> {
    let paths = state.paths.read().await.clone();

    if !paths.is_library_online() {
        return Err(OnzerError::LibraryOffline(
            paths
                .library_root()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "racine non configurée".to_string()),
        ));
    }

    let source = PathBuf::from(&folder);
    if !source.is_dir() {
        return Err(OnzerError::Invalid(format!(
            "« {folder} » n'est pas un dossier"
        )));
    }

    // Ranger par défaut, y compris si le dossier choisi est la bibliothèque
    // elle-même : cliquer « Importer un dossier » exprime l'intention de faire
    // ranger, pas seulement d'indexer. Un fichier déjà à sa place ne bouge pas
    // (voir `resolve_collision`).
    let handling = if organize.unwrap_or(true) {
        FileHandling::Organize
    } else {
        FileHandling::IndexInPlace
    };

    let summary = scanner::import_folder(
        &state.pool,
        &paths,
        &source,
        handling,
        "scan",
        |progress| {
            // Un échec d'émission (fenêtre fermée) ne doit pas interrompre
            // l'import en cours.
            let _ = app.emit(SCAN_PROGRESS_EVENT, &progress);
        },
    )
    .await?;

    tracing::info!(
        importes = summary.imported,
        doublons = summary.duplicates,
        echecs = summary.failed,
        "import terminé"
    );

    Ok(summary)
}

#[tauri::command]
pub async fn list_tracks(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
    sort: Option<repository::Sort>,
) -> Result<Vec<repository::TrackSummary>> {
    repository::list_tracks(
        &state.pool,
        limit.unwrap_or(200).clamp(1, 1000),
        offset.unwrap_or(0).max(0),
        sort.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn search_tracks(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<repository::TrackSummary>> {
    repository::search_tracks(&state.pool, &query, 200).await
}

#[tauri::command]
pub async fn library_counts(state: State<'_, AppState>) -> Result<repository::LibraryCounts> {
    repository::counts(&state.pool).await
}

/// Recontrôle la présence sur disque de chaque morceau référencé.
///
/// Ne supprime jamais rien : marque les absents comme indisponibles et
/// retourne leur nombre (ADR-006).
#[tauri::command]
pub async fn refresh_availability(state: State<'_, AppState>) -> Result<i64> {
    let paths = state.paths.read().await.clone();
    scanner::refresh_availability(&state.pool, &paths).await
}

/// Retourne la vignette de pochette encodée en *data URI*.
///
/// ⚠️ Provisoire. Encoder l'image en base64 dans une réponse JSON convient
/// à une liste, mais pas à une grille de plusieurs centaines de pochettes.
/// À remplacer par un protocole URI dédié lors de l'étape « interface ».
#[tauri::command]
pub async fn artwork_data_uri(state: State<'_, AppState>, hash: String) -> Result<Option<String>> {
    // Le nom de fichier venant de la base, on refuse tout ce qui n'est pas une
    // empreinte hexadécimale : un « ../ » ne doit pas pouvoir s'y glisser.
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(OnzerError::Invalid("empreinte de pochette invalide".into()));
    }

    let artwork_dir = state.paths.read().await.artwork_dir();
    let path = crate::library::artwork::thumbnail_path(&artwork_dir, &hash);

    if !path.is_file() {
        return Ok(None);
    }

    let bytes = std::fs::read(path)?;
    Ok(Some(format!(
        "data:image/jpeg;base64,{}",
        crate::library::artwork::encode_base64(&bytes)
    )))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrait_le_nom_du_volume() {
        assert_eq!(
            volume_name(std::path::Path::new("/Volumes/Lexar/Musique")).as_deref(),
            Some("Lexar")
        );
        assert_eq!(volume_name(std::path::Path::new("/Users/carlos/Musique")), None);
    }
}

/// Les morceaux dont le fichier n'est plus là, sous forme « Artiste - Titre ».
///
/// # À quoi sert cette liste
///
/// Un morceau hors ligne n'est pas perdu : sa ligne, son historique et ses
/// playlists sont intacts, seul le fichier manque. Le retélécharger suppose de
/// savoir **lesquels** — et le lire à l'écran, cent cinquante fois, n'est pas
/// une réponse.
///
/// La forme « Artiste - Titre » n'est pas décorative : c'est celle que lisent
/// `spotdl`, `yt-dlp` et les convertisseurs de playlist. Une liste qu'on ne
/// peut que regarder ne servirait à rien.
#[tauri::command]
pub async fn offline_tracks(state: State<'_, AppState>) -> Result<Vec<String>> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1)
           FROM tracks t
          WHERE t.deleted_at IS NULL AND t.is_available = 0
          ORDER BY 2, 1",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(title, artist)| match artist {
            Some(artist) => format!("{artist} - {title}"),
            None => title,
        })
        .collect())
}

/// Les morceaux désignés, dans l'ordre demandé.
///
/// # Pourquoi cette commande existe
///
/// Une playlist générée par le moteur ne donne que des identifiants ; il faut
/// les habiller pour les afficher. Cela se faisait en demandant la première
/// page de la bibliothèque et en y piochant — ce qui marchait tant que la page
/// couvrait toute la bibliothèque, et **cassait silencieusement** dès qu'elle
/// ne la couvrait plus : les morceaux au-delà disparaissaient de la playlist
/// sans erreur ni message.
///
/// L'ordre du moteur est conservé : c'est lui qui porte le sens de la
/// sélection, pas l'ordre d'ajout à la bibliothèque.
#[tauri::command]
pub async fn tracks_by_ids(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<repository::TrackSummary>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT {} FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.id IN ({placeholders}) AND t.deleted_at IS NULL",
        repository::TRACK_COLUMNS
    );

    let mut query = sqlx::query_as::<_, repository::TrackSummary>(&sql);
    for id in &ids {
        query = query.bind(id);
    }

    let found = query.fetch_all(&state.pool).await?;

    // Remise dans l'ordre demandé : `IN` ne le garantit pas.
    let by_id: std::collections::HashMap<i64, repository::TrackSummary> =
        found.into_iter().map(|track| (track.id, track)).collect();

    Ok(ids
        .into_iter()
        .filter_map(|id| by_id.get(&id).cloned())
        .collect())
}

/// Un album de la bibliothèque, tel que la grille l'affiche.
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSummary {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub year: Option<i64>,
    pub track_count: i64,
    pub artwork_hash: Option<String>,
}

/// Tous les albums, du plus fourni au moins fourni.
///
/// # Pourquoi ce classement
///
/// Alphabétique, la liste commencerait par les compilations d'un morceau
/// glanées au fil des identifications. Par nombre de titres, elle commence par
/// les albums qu'on possède vraiment — ceux qu'on cherche.
/// Ce qu'on a écouté, du plus récent au plus ancien.
///
/// # Pourquoi les écoutes éclair sont exclues
///
/// Un morceau zappé au bout de trois secondes n'est pas « ce que j'écoutais » :
/// c'est ce que j'ai refusé. L'historique qui les garde devient une liste de
/// tout ce qu'on a effleuré, où l'on ne retrouve plus rien.
#[tauri::command]
pub async fn listening_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<repository::TrackSummary>> {
    let tracks = sqlx::query_as::<_, repository::TrackSummary>(&format!(
        "SELECT {}
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL
            AND EXISTS (SELECT 1 FROM play_events e
                         WHERE e.track_id = t.id AND e.listened_ms >= 15000)
          ORDER BY (SELECT MAX(e.started_at) FROM play_events e WHERE e.track_id = t.id) DESC
          LIMIT ?",
        repository::TRACK_COLUMNS
    ))
    .bind(limit.unwrap_or(200))
    .fetch_all(&state.pool)
    .await?;

    Ok(tracks)
}

/// Les morceaux dont le fichier a disparu.
///
/// Ils gardent leur fiche, leurs favoris et leur place dans les playlists : ce
/// n'est pas une suppression, c'est une absence. Encore faut-il pouvoir les
/// regarder — un compteur dans un bandeau ne dit pas *lesquels*.
#[tauri::command]
pub async fn unavailable_tracks(
    state: State<'_, AppState>,
) -> Result<Vec<repository::TrackSummary>> {
    let tracks = sqlx::query_as::<_, repository::TrackSummary>(&format!(
        "SELECT {}
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL AND t.is_available = 0
       ORDER BY t.title COLLATE NOCASE",
        repository::TRACK_COLUMNS
    ))
    .fetch_all(&state.pool)
    .await?;

    Ok(tracks)
}

#[tauri::command]
pub async fn list_albums(state: State<'_, AppState>) -> Result<Vec<AlbumSummary>> {
    Ok(sqlx::query_as::<_, AlbumSummary>(
        "SELECT al.id, al.title,
                (SELECT a.name FROM artists a WHERE a.id = al.album_artist_id) AS artist,
                al.year,
                COUNT(t.id) AS track_count,
                al.artwork_hash
           FROM albums al
           JOIN tracks t ON t.album_id = al.id AND t.deleted_at IS NULL
       GROUP BY al.id
       ORDER BY track_count DESC, al.title COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?)
}

/// Les morceaux d'un album, dans l'ordre du disque.
#[tauri::command]
pub async fn album_tracks(
    state: State<'_, AppState>,
    album_id: i64,
) -> Result<Vec<repository::TrackSummary>> {
    let sql = format!(
        "SELECT {} FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.album_id = ? AND t.deleted_at IS NULL
       ORDER BY t.disc_no, t.track_no, t.title",
        repository::TRACK_COLUMNS
    );

    Ok(sqlx::query_as::<_, repository::TrackSummary>(&sql)
        .bind(album_id)
        .fetch_all(&state.pool)
        .await?)
}

/// Ce qu'une reprise des fichiers écartés a donné.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueReport {
    /// Morceaux hors ligne qui ont retrouvé leur fichier.
    pub restored: u64,
    /// Fichiers qui restent des doublons, laissés où ils sont.
    pub kept: u64,
}

/// Repasse sur les fichiers mis de côté dans `_Inbox/_Doublons`.
///
/// # Pourquoi ils s'y sont accumulés
///
/// Un morceau hors ligne garde sa ligne en base. Le retéléchargement arrivait
/// avec les mêmes tags, la détection de doublon le reconnaissait — et
/// l'écartait. Le morceau restait grisé, son fichier finissait dans
/// `_Doublons`, et plus rien ne pouvait les rapprocher. Quatre cent sept
/// fichiers s'y étaient accumulés.
///
/// La règle est corrigée à l'import, mais **corriger un défaut ne répare pas
/// ce qu'il a déjà écarté** (ADR-029). Cette passe rejoue donc l'import sur ce
/// dossier : ce qui correspond à un morceau sans fichier le rejoint, le reste
/// ne bouge pas.
#[tauri::command]
pub async fn rescue_set_aside(state: State<'_, AppState>) -> Result<RescueReport> {
    let paths = state.paths.read().await.clone();
    let root = paths
        .library_root()
        .ok_or_else(|| OnzerError::Invalid("aucune bibliothèque configurée".to_string()))?;

    let aside = crate::ingest::inbox::inbox_path(root)
        .join(crate::library::naming::INBOX_DUPLICATES_DIR);

    let Ok(entries) = std::fs::read_dir(&aside) else {
        return Ok(RescueReport::default());
    };

    let files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| crate::library::importer::is_importable(path))
        .collect();

    let mut report = RescueReport::default();

    for file in files {
        match crate::library::importer::import_file(
            &state.pool,
            &paths,
            &file,
            crate::library::importer::FileHandling::Organize,
            "rescue",
        )
        .await
        {
            Ok(crate::library::importer::ImportOutcome::Restored { .. })
            | Ok(crate::library::importer::ImportOutcome::Imported { .. }) => report.restored += 1,
            Ok(crate::library::importer::ImportOutcome::Duplicate { .. }) => report.kept += 1,
            Err(error) => {
                tracing::warn!(fichier = %file.display(), %error, "reprise impossible");
                report.kept += 1;
            }
        }
    }

    Ok(report)
}

/// Retire un morceau de la bibliothèque.
///
/// # Retirer n'est pas détruire
///
/// La ligne est **marquée** supprimée, jamais effacée : `play_events` la
/// référence en `ON DELETE RESTRICT`, et perdre l'historique d'écoute d'un
/// morceau parce qu'on ne veut plus le voir serait absurde — les statistiques
/// et le moteur de recommandation s'appuient dessus.
///
/// Le fichier, lui, n'est pas touché. Onzer ne détruit rien sur le disque :
/// s'il fallait aussi supprimer les octets, il faudrait pouvoir revenir en
/// arrière, et ce n'est pas le rôle d'un lecteur.
#[tauri::command]
pub async fn remove_track(state: State<'_, AppState>, track_id: i64) -> Result<()> {
    sqlx::query("UPDATE tracks SET deleted_at = ? WHERE id = ?")
        .bind(crate::core::now_ms())
        .bind(track_id)
        .execute(&state.pool)
        .await?;

    // Retiré de l'index de recherche, sans quoi il continuerait de ressortir.
    sqlx::query("DELETE FROM tracks_fts WHERE track_id = ?")
        .bind(track_id)
        .execute(&state.pool)
        .await?;

    Ok(())
}

/// Écart de durée en deçà duquel deux morceaux homonymes sont *soupçonnés*
/// d'être le même.
///
/// Le dédoublonnage automatique s'arrête à deux secondes — au-delà, l'audio est
/// réellement différent et Onzer n'a pas à trancher. Mais un clip et sa version
/// album diffèrent souvent de cinq à quinze secondes, et l'utilisateur, lui,
/// voit bien deux fois le même morceau. Vingt secondes couvrent ces cas sans
/// rapprocher deux titres homonymes sans rapport.
const NEAR_DUPLICATE_TOLERANCE_MS: i64 = 20_000;

/// Deux versions d'un même titre, à l'appréciation de l'utilisateur.
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NearDuplicate {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    /// Clé de regroupement : les lignes d'un même groupe la partagent.
    pub group_key: String,
    pub relative_path: String,
    pub play_count: i64,
}

/// Morceaux qui se ressemblent assez pour mériter un coup d'œil.
///
/// # Pourquoi Onzer ne les fusionne pas lui-même
///
/// Leur audio diffère : ce sont bel et bien deux fichiers distincts, et souvent
/// deux versions légitimes — le clip et la version album, un live et un studio.
/// Les fusionner d'autorité ferait disparaître une version que l'utilisateur
/// voulait garder, sans qu'il l'ait demandé.
///
/// Onzer se contente donc de les **rapprocher**, avec ce qu'il faut pour
/// décider : la durée, l'album, et le nombre d'écoutes — c'est presque toujours
/// celui qu'on a écouté qu'on garde.
#[tauri::command]
pub async fn near_duplicates(state: State<'_, AppState>) -> Result<Vec<NearDuplicate>> {
    let rows = sqlx::query_as::<_, NearDuplicate>(
        "SELECT t.id, t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist,
                al.title AS album,
                t.duration_ms,
                t.normalized_title AS group_key,
                t.relative_path,
                (SELECT COUNT(*) FROM play_events e WHERE e.track_id = t.id) AS play_count
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL
            AND EXISTS (
                SELECT 1 FROM tracks other
                 WHERE other.id <> t.id
                   AND other.deleted_at IS NULL
                   AND other.normalized_title = t.normalized_title
                   AND ABS(other.duration_ms - t.duration_ms) <= ?
            )
          ORDER BY t.normalized_title, t.duration_ms",
    )
    .bind(NEAR_DUPLICATE_TOLERANCE_MS)
    .fetch_all(&state.pool)
    .await?;

    // Les groupes déclarés légitimes disparaissent : sans cela, le panneau
    // redemanderait éternellement pour une reprise ou deux interludes
    // homonymes.
    let ignored = ignored_groups(&state.pool).await?;

    Ok(rows
        .into_iter()
        .filter(|row| !ignored.contains(&row.group_key))
        .collect())
}

/// Groupes de doublons que l'utilisateur a déclarés légitimes.
///
/// Stockés dans les réglages plutôt que dans une table : c'est une poignée de
/// clés, et leur cycle de vie suit celui d'une préférence, pas d'une donnée.
const IGNORED_DUPLICATES: &str = "duplicate_groups_ignored";

async fn ignored_groups(pool: &sqlx::SqlitePool) -> Result<Vec<String>> {
    Ok(crate::db::settings::get(pool, IGNORED_DUPLICATES)
        .await?
        .unwrap_or_default())
}

/// Déclare qu'un groupe n'est pas un doublon.
///
/// # Pourquoi c'est nécessaire
///
/// Deux morceaux peuvent porter le même titre et durer presque pareil sans
/// avoir le moindre rapport — une reprise, un interlude homonyme sur deux
/// albums. Sans moyen de le dire, le panneau redemanderait éternellement.
///
/// La décision est réversible : c'est un réglage, pas une suppression.
#[tauri::command]
pub async fn ignore_duplicate_group(state: State<'_, AppState>, group_key: String) -> Result<()> {
    let mut ignored = ignored_groups(&state.pool).await?;

    if !ignored.contains(&group_key) {
        ignored.push(group_key);
    }

    crate::db::settings::set(&state.pool, IGNORED_DUPLICATES, &ignored).await
}
