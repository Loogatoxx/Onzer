//! Page d'accueil : ce qu'Onzer propose sans qu'on lui demande rien.
//!
//! # Ce que cette page doit résoudre
//!
//! Une bibliothèque personnelle a un défaut que les catalogues en ligne n'ont
//! pas : **on l'a déjà toute entendue**. Ouvrir une liste de 253 morceaux qu'on
//! connaît ne donne envie de rien. La page d'accueil ne montre donc pas la
//! bibliothèque : elle propose des **entrées** dedans.
//!
//! # Les trois rayons
//!
//! | Rayon | Question à laquelle il répond |
//! |---|---|
//! | Reprendre | « Qu'est-ce que j'écoutais ? » |
//! | Mix du jour | « Fais-moi écouter un artiste que j'aime, sans que je choisisse » |
//! | Faits pour toi | « Surprends-moi, mais dans une direction que je choisis » |
//!
//! # Pourquoi les mix du jour tournent
//!
//! Un mix identique chaque matin cesse d'être regardé au bout de trois jours.
//! L'ordre des artistes est donc permuté par le **quantième du jour** : la
//! sélection reste la même — ce sont bien les artistes les plus écoutés — mais
//! celui qui s'affiche en tête change tous les jours.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::core::Result;
use crate::db::repository::{TrackSummary, TRACK_COLUMNS};
use crate::AppState;

/// Nombre de morceaux dans la rangée « Reprendre l'écoute ».
const RESUME_COUNT: i64 = 8;

/// Nombre de mix d'artistes proposés.
const DAILY_MIX_COUNT: usize = 6;

/// Fenêtre d'écoute servant à élire les artistes du jour.
const DAILY_MIX_WINDOW_DAYS: i64 = 120;

/// En deçà, un artiste ne fait pas un mix : quatre morceaux qui tournent en
/// boucle ne se distinguent pas d'une playlist ordinaire.
const MIN_TRACKS_FOR_MIX: i64 = 4;

/// Une proposition d'écoute.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeMix {
    /// Correspond à `reco::engine::PlaylistKind::as_str`.
    pub kind: String,
    /// Renseigné pour un mix d'artiste uniquement.
    pub artist_id: Option<i64>,
    pub title: String,
    pub subtitle: String,
    /// Jusqu'à quatre pochettes, pour une mosaïque.
    pub cover_hashes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeShelf {
    pub title: String,
    pub mixes: Vec<HomeMix>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    /// « Bonjour », « Bon après-midi », « Bonsoir ».
    pub greeting: String,
    /// Derniers morceaux écoutés, pour reprendre où l'on s'était arrêté.
    pub resume: Vec<TrackSummary>,
    pub shelves: Vec<HomeShelf>,
}

#[tauri::command]
pub async fn home(state: State<'_, AppState>) -> Result<Home> {
    let pool = &state.pool;

    Ok(Home {
        greeting: greeting(local_hour(), &crate::commands::preferences::display_name(pool).await?),
        resume: resume(pool).await?,
        shelves: vec![
            HomeShelf {
                title: "Tes mix du jour".to_string(),
                mixes: daily_mixes(pool).await?,
            },
            HomeShelf {
                title: "Faits pour toi".to_string(),
                mixes: taste_mixes(pool).await?,
            },
        ],
    })
}

/// Salutation adaptée à l'heure.
///
/// Séparée de sa source pour être testable : une fonction qui lit l'horloge
/// elle-même ne se teste qu'en attendant le bon moment de la journée.
/// La salutation, et le nom de celui qu'elle salue.
///
/// # Pourquoi le nom est facultatif
///
/// « Bonsoir Carlos » n'est chaleureux que si c'est bien son nom. Inventer un
/// prénom, ou en demander un avant de laisser entrer, serait pire que de n'en
/// afficher aucun : « Bonsoir » tout court se suffit.
fn greeting(hour: u32, name: &str) -> String {
    let moment = match hour {
        5..=11 => "Bonjour",
        12..=17 => "Bon après-midi",
        _ => "Bonsoir",
    };

    match name.trim() {
        "" => moment.to_string(),
        nom => format!("{moment} {nom}"),
    }
}

fn local_hour() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

/// Les derniers morceaux réellement écoutés.
///
/// Les écoutes éclair sont exclues : un morceau zappé au bout de trois secondes
/// n'est pas « ce que j'écoutais », c'est ce que j'ai refusé.
async fn resume(pool: &SqlitePool) -> Result<Vec<TrackSummary>> {
    let tracks = sqlx::query_as::<_, TrackSummary>(&format!(
        "SELECT {TRACK_COLUMNS}
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL
            AND EXISTS (SELECT 1 FROM play_events e
                         WHERE e.track_id = t.id AND e.listened_ms >= 15000)
          ORDER BY (SELECT MAX(e.started_at) FROM play_events e WHERE e.track_id = t.id) DESC
          LIMIT ?",
    ))
    .bind(RESUME_COUNT)
    .fetch_all(pool)
    .await?;

    Ok(tracks)
}

/// Les mix d'artistes du jour.
async fn daily_mixes(pool: &SqlitePool) -> Result<Vec<HomeMix>> {
    let since = crate::core::now_ms() - DAILY_MIX_WINDOW_DAYS * 86_400_000;

    // Les artistes les plus écoutés sur la fenêtre.
    let mut artists: Vec<(i64, String)> = sqlx::query_as(
        "SELECT a.id, a.name
           FROM play_events e
           JOIN track_artists ta ON ta.track_id = e.track_id AND ta.role = 'main'
           JOIN artists a ON a.id = ta.artist_id
          WHERE e.started_at >= ?
          GROUP BY a.id
          ORDER BY SUM(e.listened_ms) DESC
          LIMIT ?",
    )
    .bind(since)
    .bind(DAILY_MIX_COUNT as i64)
    .fetch_all(pool)
    .await?;

    // # Pourquoi compléter, et pas seulement se rabattre
    //
    // Un premier jet ne consultait la bibliothèque que si l'historique était
    // *vide*. Or sur une bibliothèque jeune il ne l'est pas : il compte deux
    // artistes. Le rayon affichait donc deux cartes au lieu de six, et donnait
    // l'impression d'une page à moitié chargée.
    //
    // L'historique reste prioritaire — c'est le meilleur signal — mais les
    // artistes les mieux fournis complètent la rangée.
    if artists.len() < DAILY_MIX_COUNT {
        let known: std::collections::HashSet<i64> =
            artists.iter().map(|(id, _)| *id).collect();

        let filler: Vec<(i64, String)> = sqlx::query_as(
            "SELECT a.id, a.name
               FROM artists a
               JOIN track_artists ta ON ta.artist_id = a.id AND ta.role = 'main'
               JOIN tracks t ON t.id = ta.track_id AND t.deleted_at IS NULL
              GROUP BY a.id
             HAVING COUNT(*) >= ?
              ORDER BY COUNT(*) DESC
              LIMIT ?",
        )
        // Un artiste représenté par un seul morceau ne fait pas un mix.
        .bind(MIN_TRACKS_FOR_MIX)
        .bind(DAILY_MIX_COUNT as i64 * 2)
        .fetch_all(pool)
        .await?;

        for candidate in filler {
            if artists.len() >= DAILY_MIX_COUNT {
                break;
            }
            if !known.contains(&candidate.0) {
                artists.push(candidate);
            }
        }
    }

    rotate(&mut artists, day_of_epoch());

    let mut mixes = Vec::with_capacity(artists.len());
    for (artist_id, name) in artists {
        mixes.push(HomeMix {
            kind: "artist_mix".to_string(),
            artist_id: Some(artist_id),
            title: format!("Mix {name}"),
            subtitle: "Son univers, et ce qui lui ressemble".to_string(),
            cover_hashes: artist_covers(pool, artist_id).await?,
        });
    }

    Ok(mixes)
}

/// Les mix qui ne dépendent pas d'un artiste.
async fn taste_mixes(pool: &SqlitePool) -> Result<Vec<HomeMix>> {
    // L'ordre est celui de l'envie décroissante : ce qui colle à l'instant,
    // puis ce qu'on aime, puis ce qu'on a laissé de côté, puis l'inconnu.
    let definitions: [(&str, &str, &str, &str); 4] = [
        (
            "for_now",
            "Pour toi maintenant",
            "Ce que tu écoutes habituellement à cette heure-ci",
            "SELECT al.artwork_hash FROM tracks t
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.deleted_at IS NULL AND al.artwork_hash IS NOT NULL
             ORDER BY (SELECT COUNT(*) FROM play_events e WHERE e.track_id = t.id) DESC
             LIMIT 4",
        ),
        (
            "loved",
            "En boucle en ce moment",
            "Ce que tu réécoutes le plus ces temps-ci",
            "SELECT al.artwork_hash FROM tracks t
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.deleted_at IS NULL AND al.artwork_hash IS NOT NULL
             ORDER BY t.is_loved DESC,
                      (SELECT COUNT(*) FROM play_events e WHERE e.track_id = t.id) DESC
             LIMIT 4",
        ),
        (
            "forgotten",
            "Tu les avais oubliés",
            "Aimés autrefois, plus écoutés depuis longtemps",
            "SELECT al.artwork_hash FROM tracks t
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.deleted_at IS NULL AND al.artwork_hash IS NOT NULL
               AND EXISTS (SELECT 1 FROM play_events e WHERE e.track_id = t.id)
             ORDER BY (SELECT MAX(e.started_at) FROM play_events e WHERE e.track_id = t.id) ASC
             LIMIT 4",
        ),
        (
            "never_played",
            "Jamais écoutés",
            "Le fond de ta bibliothèque, jamais lancé",
            "SELECT al.artwork_hash FROM tracks t
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.deleted_at IS NULL AND al.artwork_hash IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM play_events e WHERE e.track_id = t.id)
             ORDER BY t.added_at DESC
             LIMIT 4",
        ),
    ];

    let mut mixes = Vec::with_capacity(definitions.len());
    for (kind, title, subtitle, sql) in definitions {
        let covers: Vec<String> = sqlx::query_scalar(sql).fetch_all(pool).await?;

        mixes.push(HomeMix {
            kind: kind.to_string(),
            artist_id: None,
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            cover_hashes: covers,
        });
    }

    Ok(mixes)
}

/// Jusqu'à quatre pochettes distinctes d'un artiste.
async fn artist_covers(pool: &SqlitePool, artist_id: i64) -> Result<Vec<String>> {
    let covers: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT al.artwork_hash
           FROM track_artists ta
           JOIN tracks t ON t.id = ta.track_id AND t.deleted_at IS NULL
           JOIN albums al ON al.id = t.album_id
          WHERE ta.artist_id = ? AND ta.role = 'main' AND al.artwork_hash IS NOT NULL
          LIMIT 4",
    )
    .bind(artist_id)
    .fetch_all(pool)
    .await?;

    Ok(covers)
}

/// Nombre de jours écoulés depuis l'époque Unix.
fn day_of_epoch() -> usize {
    (crate::core::now_ms() / 86_400_000).max(0) as usize
}

/// Décale une liste circulairement.
///
/// C'est ce qui fait « tourner » les mix : la sélection ne change pas d'un jour
/// à l'autre — ce sont bien les artistes les plus écoutés — mais celui qui
/// s'affiche en tête, oui. Un mix figé cesse d'être regardé au bout de trois
/// jours.
fn rotate<T>(items: &mut [T], offset: usize) {
    let len = items.len();
    if len > 1 {
        items.rotate_left(offset % len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_salutation_suit_lheure() {
        assert_eq!(greeting(8, ""), "Bonjour");
        assert_eq!(greeting(8, "Carlos"), "Bonjour Carlos");
        // Un nom fait d'espaces ne nomme personne.
        assert_eq!(greeting(20, "   "), "Bonsoir");
        assert_eq!(greeting(14, ""), "Bon après-midi");
        assert_eq!(greeting(21, ""), "Bonsoir");
        assert_eq!(greeting(3, ""), "Bonsoir", "trois heures du matin reste le soir");
    }

    #[test]
    fn la_salutation_couvre_les_vingt_quatre_heures() {
        // Une heure sans salutation afficherait un titre vide en page d'accueil.
        for hour in 0..24 {
            assert!(!greeting(hour, "").is_empty(), "aucune salutation à {hour} h");
        }
    }

    #[test]
    fn la_rotation_change_la_tete_de_liste() {
        let mut hier = vec!["A", "B", "C", "D"];
        let mut aujourdhui = hier.clone();

        rotate(&mut hier, 10);
        rotate(&mut aujourdhui, 11);

        assert_ne!(hier[0], aujourdhui[0], "la tête doit changer chaque jour");
    }

    #[test]
    fn la_rotation_conserve_tout_le_monde() {
        // Faire tourner ne doit écarter personne : ce sont les mêmes artistes,
        // dans un ordre différent.
        let mut artistes = vec!["A", "B", "C", "D"];
        rotate(&mut artistes, 7);

        artistes.sort_unstable();
        assert_eq!(artistes, vec!["A", "B", "C", "D"]);
    }

    #[test]
    fn la_rotation_supporte_les_listes_degenerees() {
        // Un seul artiste, ou aucun : le modulo ne doit pas paniquer.
        let mut vide: Vec<&str> = Vec::new();
        rotate(&mut vide, 5);
        assert!(vide.is_empty());

        let mut seul = vec!["A"];
        rotate(&mut seul, 5);
        assert_eq!(seul, vec!["A"]);
    }
}
