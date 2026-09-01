//! Catégories d'ambiance, déduites de l'analyse audio.
//!
//! # Pourquoi pas des genres
//!
//! Sur les 325 morceaux de la bibliothèque de test, **deux** portaient un
//! genre dans leurs tags. Un classement par genre afficherait donc deux
//! catégories et un grand vide. L'analyse audio, elle, couvre tout : tempo,
//! énergie, positivité perçue, caractère dansant sont calculés sur le signal.
//!
//! # Pourquoi des seuils relatifs, et non absolus
//!
//! Un seuil écrit en dur ne veut rien dire hors du corpus qui l'a inspiré. Sur
//! cette bibliothèque — du rap, essentiellement — l'énergie médiane est à
//! **0,86** : la règle « énergique = au-dessus de 0,7 » y retiendrait les trois
//! quarts des morceaux et ne distinguerait rien.
//!
//! Chaque catégorie est donc définie par un **quartile de la bibliothèque
//! elle-même**. « Ça tape » désigne les 25 % les plus énergiques *de ta
//! musique*, quelle que soit la musique que tu écoutes. La règle se déplace
//! avec la collection, sans jamais devoir être réglée à la main.
//!
//! # Pourquoi ce sont des filtres et non des playlists générées
//!
//! « Montre-moi mes morceaux calmes » est une question à réponse exacte.
//! La faire passer par le moteur de recommandation y ajouterait de la
//! diversité, de l'exploration et du hasard — c'est-à-dire trois façons de ne
//! pas répondre à la question posée.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::core::{OnzerError, Result};
use crate::db::repository::TrackSummary;
use crate::identify::discover::{DiscoveryClient, Suggestion, TrackSuggestion};
use crate::AppState;

/// Part de la bibliothèque retenue par une catégorie.
const QUANTILE: f64 = 0.25;

/// En deçà, découper en quartiles n'a pas de sens.
const MIN_TRACKS: i64 = 12;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub track_count: i64,
    pub cover_hashes: Vec<String>,
}

/// Définition d'une catégorie : une colonne d'analyse, et un sens.
struct Definition {
    key: &'static str,
    title: &'static str,
    subtitle: &'static str,
    column: &'static str,
    /// Vrai pour le haut du classement, faux pour le bas.
    top: bool,
}

const DEFINITIONS: &[Definition] = &[
    Definition {
        key: "energetic",
        title: "Ça tape",
        subtitle: "Le quart le plus énergique de ta bibliothèque",
        column: "energy",
        top: true,
    },
    Definition {
        key: "calm",
        title: "Posé",
        subtitle: "Le quart le plus calme",
        column: "energy",
        top: false,
    },
    Definition {
        key: "fast",
        title: "Rapide",
        subtitle: "Les tempos les plus soutenus",
        column: "tempo",
        top: true,
    },
    Definition {
        key: "slow",
        title: "Lent",
        subtitle: "Les tempos les plus traînants",
        column: "tempo",
        top: false,
    },
    Definition {
        key: "bright",
        title: "Lumineux",
        subtitle: "Ce que l'analyse entend de plus clair",
        column: "valence",
        top: true,
    },
    Definition {
        key: "dark",
        title: "Sombre",
        subtitle: "Ce que l'analyse entend de plus assombri",
        column: "valence",
        top: false,
    },
    Definition {
        key: "danceable",
        title: "Dansant",
        subtitle: "La pulsation la plus régulière et la plus marquée",
        column: "danceability",
        top: true,
    },
];

fn definition(key: &str) -> Option<&'static Definition> {
    DEFINITIONS.iter().find(|entry| entry.key == key)
}

#[tauri::command]
pub async fn categories(state: State<'_, AppState>) -> Result<Vec<Category>> {
    let pool = &state.pool;
    let mut found = Vec::new();

    for definition in DEFINITIONS {
        let Some(threshold) = threshold(pool, definition).await? else {
            continue;
        };

        let (track_count,): (i64,) = sqlx::query_as(&count_sql(definition))
            .bind(threshold)
            .fetch_one(pool)
            .await?;

        // Une catégorie vide n'a rien à faire à l'écran : la colonne d'analyse
        // n'est peut-être pas renseignée par l'analyseur courant.
        if track_count == 0 {
            continue;
        }

        found.push(Category {
            key: definition.key.to_string(),
            title: definition.title.to_string(),
            subtitle: definition.subtitle.to_string(),
            track_count,
            cover_hashes: sqlx::query_scalar(&covers_sql(definition))
                .bind(threshold)
                .fetch_all(pool)
                .await?,
        });
    }

    Ok(found)
}

#[tauri::command]
pub async fn category_tracks(state: State<'_, AppState>, key: String) -> Result<Vec<TrackSummary>> {
    let Some(definition) = definition(&key) else {
        return Err(OnzerError::Invalid(format!("catégorie inconnue : {key}")));
    };

    let Some(threshold) = threshold(&state.pool, definition).await? else {
        return Ok(Vec::new());
    };

    let tracks = sqlx::query_as::<_, TrackSummary>(&tracks_sql(definition))
        .bind(threshold)
        .fetch_all(&state.pool)
        .await?;

    Ok(tracks)
}

/// Valeur au quartile demandé, calculée sur la bibliothèque.
///
/// `None` quand la colonne n'est pas renseignée, ou que la bibliothèque est
/// trop petite pour qu'un quartile veuille dire quelque chose.
async fn threshold(pool: &SqlitePool, definition: &Definition) -> Result<Option<f64>> {
    let column = definition.column;

    let (available,): (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM track_features f
           JOIN tracks t ON t.id = f.track_id AND t.deleted_at IS NULL
          WHERE f.{column} IS NOT NULL"
    ))
    .fetch_one(pool)
    .await?;

    if available < MIN_TRACKS {
        return Ok(None);
    }

    // Le quartile se lit en sautant la bonne fraction du classement. SQLite
    // n'a pas de fonction de percentile, et une fonction de fenêtrage coûterait
    // plus cher qu'un `OFFSET` sur un index.
    let offset = (available as f64 * QUANTILE) as i64;
    let direction = if definition.top { "DESC" } else { "ASC" };

    let value: Option<f64> = sqlx::query_scalar(&format!(
        "SELECT f.{column} FROM track_features f
           JOIN tracks t ON t.id = f.track_id AND t.deleted_at IS NULL
          WHERE f.{column} IS NOT NULL
          ORDER BY f.{column} {direction}
          LIMIT 1 OFFSET ?"
    ))
    .bind(offset)
    .fetch_optional(pool)
    .await?;

    Ok(value)
}

/// Clause de sélection d'une catégorie.
///
/// Les noms de colonnes viennent d'une table constante du programme, jamais de
/// l'utilisateur : l'interpolation est sans risque, et le seuil, lui, reste un
/// paramètre lié.
fn where_clause(definition: &Definition) -> String {
    let column = definition.column;
    let comparison = if definition.top { ">=" } else { "<=" };

    format!("f.{column} IS NOT NULL AND f.{column} {comparison} ?")
}

fn count_sql(definition: &Definition) -> String {
    format!(
        "SELECT COUNT(*) FROM track_features f
           JOIN tracks t ON t.id = f.track_id AND t.deleted_at IS NULL
          WHERE {}",
        where_clause(definition)
    )
}

fn covers_sql(definition: &Definition) -> String {
    format!(
        "SELECT DISTINCT al.artwork_hash
           FROM track_features f
           JOIN tracks t ON t.id = f.track_id AND t.deleted_at IS NULL
           JOIN albums al ON al.id = t.album_id
          WHERE {} AND al.artwork_hash IS NOT NULL
          LIMIT 4",
        where_clause(definition)
    )
}

fn tracks_sql(definition: &Definition) -> String {
    let order = if definition.top { "DESC" } else { "ASC" };

    format!(
        "SELECT t.id, t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist,
                al.title AS album, t.year, t.track_no, t.duration_ms, t.format,
                t.relative_path, t.is_available, al.artwork_hash, t.is_loved, t.added_at,
                (t.lyrics IS NOT NULL AND t.lyrics <> '') AS has_lyrics
           FROM track_features f
           JOIN tracks t ON t.id = f.track_id AND t.deleted_at IS NULL
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE {}
          ORDER BY f.{} {order}",
        where_clause(definition),
        definition.column
    )
}

// ── Découverte d'artistes ───────────────────────────────────────────────────

/// Client de découverte, partagé : les limiteurs de cadence vivent dedans.
static DISCOVERY: std::sync::OnceLock<Option<DiscoveryClient>> = std::sync::OnceLock::new();

/// Suggère des artistes absents de la bibliothèque.
///
/// **Sur action explicite uniquement.** Ce qui part se limite à des
/// identifiants MusicBrainz d'artistes — pas un titre, pas une écoute, pas un
/// horodatage. Sans réseau, tout le reste d'Onzer fonctionne à l'identique.
#[tauri::command]
pub async fn discover_artists(state: State<'_, AppState>) -> Result<Vec<Suggestion>> {
    let client = DISCOVERY
        .get_or_init(|| DiscoveryClient::new().ok())
        .as_ref()
        .ok_or_else(|| OnzerError::Invalid("client de découverte indisponible".to_string()))?;

    client.suggest(&state.pool).await
}

/// Suggère des titres d'artistes déjà aimés, mais absents de la collection.
///
/// **Sur action explicite uniquement**, comme le reste de la découverte.
#[tauri::command]
pub async fn discover_tracks(state: State<'_, AppState>) -> Result<Vec<TrackSuggestion>> {
    let client = DISCOVERY
        .get_or_init(|| DiscoveryClient::new().ok())
        .as_ref()
        .ok_or_else(|| OnzerError::Invalid("client de découverte indisponible".to_string()))?;

    client.suggest_tracks(&state.pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_categorie_a_une_cle_unique() {
        // Deux catégories homonymes se masqueraient l'une l'autre à l'écran,
        // et `definition` en retournerait une au hasard.
        let mut keys: Vec<&str> = DEFINITIONS.iter().map(|entry| entry.key).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();

        assert_eq!(keys.len(), total);
    }

    #[test]
    fn les_colonnes_citees_existent_dans_le_schema() {
        // Les noms sont interpolés dans le SQL : une faute de frappe ne se
        // verrait qu'à l'exécution, sur la machine de l'utilisateur.
        const SCHEMA: &[&str] = &[
            "tempo",
            "energy",
            "loudness",
            "danceability",
            "valence",
            "instrumentalness",
        ];

        for definition in DEFINITIONS {
            assert!(
                SCHEMA.contains(&definition.column),
                "colonne inconnue : {}",
                definition.column
            );
        }
    }

    #[test]
    fn le_sens_de_comparaison_suit_le_sens_de_la_categorie() {
        let haut = DEFINITIONS.iter().find(|d| d.key == "energetic").unwrap();
        let bas = DEFINITIONS.iter().find(|d| d.key == "calm").unwrap();

        assert!(where_clause(haut).contains(">="));
        assert!(where_clause(bas).contains("<="));
    }

    #[test]
    fn une_categorie_inconnue_est_refusee() {
        assert!(definition("energetic").is_some());
        assert!(definition("n'existe pas").is_none());
        // Le nom vient de l'interface : il ne doit jamais atteindre le SQL.
        assert!(definition("energy; DROP TABLE tracks").is_none());
    }

    #[test]
    fn les_paires_opposees_couvrent_les_deux_extremites() {
        // « Ça tape » et « Posé » doivent porter sur la même mesure, en sens
        // inverse : sans quoi l'une des deux extrémités ne serait pas atteignable.
        for column in ["energy", "tempo", "valence"] {
            let sens: Vec<bool> = DEFINITIONS
                .iter()
                .filter(|d| d.column == column)
                .map(|d| d.top)
                .collect();

            assert!(sens.contains(&true), "{column} : pas de catégorie haute");
            assert!(sens.contains(&false), "{column} : pas de catégorie basse");
        }
    }
}
