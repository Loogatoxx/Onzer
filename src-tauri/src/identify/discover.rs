//! Suggestions d'artistes à découvrir.
//!
//! # La frontière avec le premier pilier
//!
//! Onzer ne connaît que ta bibliothèque. Il sait très bien te dire quoi
//! réécouter dedans ; il ne peut pas, par construction, te parler de ce qui
//! n'y est pas. Suggérer un artiste absent suppose donc une source extérieure.
//!
//! Cette source est **ListenBrainz** : données ouvertes, sans clé d'API, sans
//! compte, sans traceur. Ce qui part se limite à des identifiants MusicBrainz
//! d'artistes — pas un titre, pas une écoute, pas un horodatage.
//!
//! Et cela ne se déclenche que sur un geste explicite. Sans réseau, tout le
//! reste d'Onzer fonctionne à l'identique : c'est un supplément, jamais une
//! dépendance.
//!
//! # Comment la liste est construite
//!
//! ```text
//!   tes artistes les plus écoutés
//!            │
//!            ├─► MusicBrainz : leur identifiant (mis en cache, une fois)
//!            │
//!            ├─► ListenBrainz : « qui leur ressemble ? »
//!            │
//!            ├─► on écarte ceux que tu possèdes déjà
//!            │
//!            └─► on additionne les scores : un artiste suggéré par
//!                plusieurs des tiens remonte devant
//! ```
//!
//! Cette addition est le cœur de la méthode. Prendre les meilleurs voisins d'un
//! seul artiste donnerait sa discographie élargie ; les faire voter fait
//! ressortir ce qui se trouve **au centre de tes goûts**, et non à la périphérie
//! d'un seul d'entre eux.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::core::Result;
use crate::library::naming::normalize_key;

use super::http::Service;
use super::ratelimit::MUSICBRAINZ_MIN_INTERVAL;

const MUSICBRAINZ_SEARCH: &str = "https://musicbrainz.org/ws/2/artist";
const MUSICBRAINZ_BROWSE: &str = "https://musicbrainz.org/ws/2/recording";
const LISTENBRAINZ_SIMILAR: &str = "https://labs.api.listenbrainz.org/similar-artists/json";

/// Recette de similarité de ListenBrainz.
///
/// Elle fait partie de l'URL : le service en expose plusieurs, et celle-ci est
/// bâtie sur les sessions d'écoute — deux artistes sont proches s'ils sont
/// souvent écoutés dans une même séance. C'est exactement la notion de
/// proximité qu'on veut ici, plus proche de l'usage que du genre déclaré.
const ALGORITHM: &str =
    "session_based_days_7500_session_300_contribution_5_threshold_10_limit_100_filter_True_skip_30";

/// Cadence d'appel à ListenBrainz.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1_000);

/// Nombre d'artistes de référence interrogés.
const SEEDS: usize = 5;

/// Nombre de suggestions rendues.
const SUGGESTIONS: usize = 12;

/// Un artiste suggéré.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub name: String,
    /// Identifiant MusicBrainz, pour aller vérifier soi-même.
    pub mbid: String,
    /// « Parce que tu écoutes Damso et Népal ».
    pub reason: String,
    pub score: f64,
}

pub struct DiscoveryClient {
    musicbrainz: Service,
    listenbrainz: Service,
}

impl DiscoveryClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            musicbrainz: Service::new("MusicBrainz", MUSICBRAINZ_MIN_INTERVAL)?,
            listenbrainz: Service::new("ListenBrainz", MIN_INTERVAL)?,
        })
    }

    /// Suggère des artistes absents de la bibliothèque.
    pub async fn suggest(&self, pool: &SqlitePool) -> Result<Vec<Suggestion>> {
        let seeds = self.seed_artists(pool).await?;
        if seeds.is_empty() {
            return Ok(Vec::new());
        }

        let owned = owned_artists(pool).await?;
        let mut votes: HashMap<String, Vote> = HashMap::new();

        for (name, mbid) in &seeds {
            let Ok(similar) = self.similar_to(mbid).await else {
                continue; // un artiste inconnu du service n'arrête pas les autres
            };

            for candidate in similar {
                if candidate.name.trim().is_empty()
                    || owned.contains(&normalize_key(&candidate.name))
                {
                    continue;
                }

                let entry = votes.entry(candidate.artist_mbid.clone()).or_insert(Vote {
                    name: candidate.name.clone(),
                    score: 0.0,
                    because: Vec::new(),
                });

                entry.score += candidate.score;
                if !entry.because.contains(name) {
                    entry.because.push(name.clone());
                }
            }
        }

        Ok(rank(votes))
    }

    /// Les artistes les plus écoutés, avec leur identifiant MusicBrainz.
    async fn seed_artists(&self, pool: &SqlitePool) -> Result<Vec<(String, String)>> {
        let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT a.id, a.name, a.mbid
               FROM artists a
               JOIN track_artists ta ON ta.artist_id = a.id AND ta.role = 'main'
               JOIN tracks t ON t.id = ta.track_id AND t.deleted_at IS NULL
              GROUP BY a.id
              ORDER BY COUNT(*) DESC
              LIMIT ?",
        )
        .bind(SEEDS as i64)
        .fetch_all(pool)
        .await?;

        let mut seeds = Vec::new();
        for (artist_id, name, cached) in rows {
            let mbid = match cached {
                Some(mbid) if !mbid.trim().is_empty() => mbid,
                _ => {
                    let Ok(Some(found)) = self.resolve_mbid(&name).await else {
                        continue;
                    };

                    // Mis en cache : l'identifiant d'un artiste ne change pas,
                    // et le redemander à chaque fois userait le service.
                    let _ = sqlx::query("UPDATE artists SET mbid = ? WHERE id = ?")
                        .bind(&found)
                        .bind(artist_id)
                        .execute(pool)
                        .await;

                    found
                }
            };

            seeds.push((name, mbid));
        }

        Ok(seeds)
    }

    /// Retrouve l'identifiant MusicBrainz d'un artiste par son nom.
    async fn resolve_mbid(&self, name: &str) -> Result<Option<String>> {
        let url = format!(
            "{MUSICBRAINZ_SEARCH}?query=artist:{}&fmt=json&limit=1",
            encode(name)
        );

        let Some(response) = self.musicbrainz.get_json::<ArtistSearch>(&url).await? else {
            return Ok(None);
        };

        Ok(response
            .artists
            .into_iter()
            .next()
            .map(|artist| artist.id)
            .filter(|id| !id.is_empty()))
    }

    async fn similar_to(&self, mbid: &str) -> Result<Vec<SimilarArtist>> {
        let url = format!("{LISTENBRAINZ_SIMILAR}?artist_mbids={mbid}&algorithm={ALGORITHM}");

        Ok(self
            .listenbrainz
            .get_json::<Vec<SimilarArtist>>(&url)
            .await?
            .unwrap_or_default())
    }
}

/// Un vote en cours de dépouillement.
#[derive(Debug, Clone)]
struct Vote {
    name: String,
    score: f64,
    /// Ceux de tes artistes qui l'ont suggéré.
    because: Vec<String>,
}

/// Classe les suggestions et rédige leur justification.
///
/// Séparé du réseau pour être testable : c'est ici que se joue la qualité de la
/// liste, pas dans l'appel HTTP.
fn rank(votes: HashMap<String, Vote>) -> Vec<Suggestion> {
    let mut ranked: Vec<(String, Vote)> = votes.into_iter().collect();

    // L'identifiant départage les ex æquo : sans lui, l'ordre viendrait de
    // celui d'une `HashMap`, différent à chaque exécution.
    ranked.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));

    ranked
        .into_iter()
        .take(SUGGESTIONS)
        .map(|(mbid, vote)| Suggestion {
            name: vote.name,
            mbid,
            reason: reason(&vote.because),
            score: vote.score,
        })
        .collect()
}

/// « Parce que tu écoutes Damso et Népal ».
///
/// Au-delà de deux noms la phrase devient illisible : on cite les deux
/// premiers et on compte le reste.
fn reason(because: &[String]) -> String {
    match because {
        [] => "Proche de ce que tu écoutes".to_string(),
        [one] => format!("Parce que tu écoutes {one}"),
        [first, second] => format!("Parce que tu écoutes {first} et {second}"),
        [first, second, rest @ ..] => format!(
            "Parce que tu écoutes {first}, {second} et {} autre{}",
            rest.len(),
            if rest.len() > 1 { "s" } else { "" }
        ),
    }
}

/// Les artistes déjà présents, sous forme normalisée.
async fn owned_artists(pool: &SqlitePool) -> Result<std::collections::HashSet<String>> {
    let names: Vec<String> = sqlx::query_scalar("SELECT normalized_name FROM artists")
        .fetch_all(pool)
        .await?;

    Ok(names.into_iter().collect())
}

/// Encodage de composant d'URL.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

// ── Réponses des services ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ArtistSearch {
    #[serde(default)]
    artists: Vec<ArtistRef>,
}

#[derive(Debug, Deserialize)]
struct ArtistRef {
    #[serde(default)]
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SimilarArtist {
    #[serde(default)]
    artist_mbid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    score: f64,
}

// ════════════════════════════════════════════════════════════════════════════
//  Titres à découvrir
// ════════════════════════════════════════════════════════════════════════════

/// Nombre de titres suggérés.
const TRACK_SUGGESTIONS: usize = 30;

/// Enregistrements demandés par artiste. C'est le maximum d'une page.
const RECORDINGS_PER_ARTIST: usize = 100;

/// Un titre absent de la bibliothèque.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSuggestion {
    pub title: String,
    pub artist: String,
    /// Durée annoncée par MusicBrainz, quand elle est connue.
    pub duration_ms: Option<i64>,
    /// Requête prête pour un téléchargeur externe : « Artiste - Titre ».
    pub query: String,
}

impl DiscoveryClient {
    /// Suggère des titres d'artistes déjà aimés, mais absents de la collection.
    ///
    /// # Pourquoi la discographie plutôt que la similarité
    ///
    /// ListenBrainz sait dire quels titres se ressemblent — à condition que
    /// quelqu'un les ait écoutés chez eux. Interrogé sur les enregistrements de
    /// cette bibliothèque, il répond **systématiquement une liste vide** : le
    /// rap francophone y est trop peu représenté. Livrer une section vide
    /// aurait été pire que de ne rien livrer.
    ///
    /// La discographie, elle, est complète et fiable : MusicBrainz connaît 194
    /// enregistrements de Damso. Comparer ce catalogue à ce qu'on possède
    /// répond exactement à la question posée — « des titres que je pourrais
    /// aimer et que je n'ai pas » — sans rien inventer.
    pub async fn suggest_tracks(&self, pool: &SqlitePool) -> Result<Vec<TrackSuggestion>> {
        let seeds = self.seed_artists(pool).await?;
        let owned = owned_titles(pool).await?;

        let mut found = Vec::new();

        for (name, mbid) in &seeds {
            let url = format!(
                "{MUSICBRAINZ_BROWSE}?artist={mbid}&limit={RECORDINGS_PER_ARTIST}&fmt=json"
            );

            let Ok(Some(page)) = self.musicbrainz.get_json::<RecordingBrowse>(&url).await else {
                continue; // un artiste indisponible n'arrête pas les autres
            };

            // Un catalogue contient plusieurs enregistrements du même titre —
            // live, remix, réédition. Sous le même nom, ils n'apprennent rien
            // de plus à l'utilisateur.
            let mut seen = std::collections::HashSet::new();

            for recording in page.recordings {
                let key = normalize_key(&recording.title);
                if key.is_empty() || owned.contains(&key) || !seen.insert(key) {
                    continue;
                }

                found.push(TrackSuggestion {
                    query: format!("{name} - {}", recording.title),
                    title: recording.title,
                    artist: name.clone(),
                    duration_ms: recording.length.map(i64::from),
                });
            }
        }

        // Entrelacé par artiste plutôt que groupé : une liste qui commence par
        // trente titres du même artiste ressemble à une discographie, pas à une
        // découverte.
        Ok(interleave(found, &seeds).into_iter().take(TRACK_SUGGESTIONS).collect())
    }
}

/// Alterne les artistes, en préservant l'ordre interne de chacun.
fn interleave(
    suggestions: Vec<TrackSuggestion>,
    seeds: &[(String, String)],
) -> Vec<TrackSuggestion> {
    let mut par_artiste: Vec<Vec<TrackSuggestion>> = seeds.iter().map(|_| Vec::new()).collect();

    for suggestion in suggestions {
        if let Some(index) = seeds.iter().position(|(name, _)| *name == suggestion.artist) {
            par_artiste[index].push(suggestion);
        }
    }

    let mut sortie = Vec::new();
    let mut rang = 0;

    loop {
        let mut ajoute = false;
        for liste in &mut par_artiste {
            if rang < liste.len() {
                sortie.push(liste[rang].clone());
                ajoute = true;
            }
        }
        if !ajoute {
            break;
        }
        rang += 1;
    }

    sortie
}

/// Les titres déjà possédés, sous forme normalisée.
async fn owned_titles(pool: &SqlitePool) -> Result<std::collections::HashSet<String>> {
    let titles: Vec<String> =
        sqlx::query_scalar("SELECT normalized_title FROM tracks WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;

    Ok(titles.into_iter().collect())
}

#[derive(Debug, Deserialize)]
struct RecordingBrowse {
    #[serde(default)]
    recordings: Vec<BrowsedRecording>,
}

#[derive(Debug, Deserialize)]
struct BrowsedRecording {
    #[serde(default)]
    title: String,
    #[serde(default)]
    length: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vote(name: &str, score: f64, because: &[&str]) -> Vote {
        Vote {
            name: name.to_string(),
            score,
            because: because.iter().map(|n| (*n).to_string()).collect(),
        }
    }

    #[test]
    fn un_artiste_suggere_par_plusieurs_des_tiens_passe_devant() {
        // C'est le cœur de la méthode : l'addition des voix fait ressortir ce
        // qui est au centre des goûts, pas à la périphérie d'un seul artiste.
        let mut votes = HashMap::new();
        votes.insert("a".to_string(), vote("Consensuel", 0.6, &["Damso", "Népal"]));
        votes.insert("b".to_string(), vote("Périphérique", 0.9, &["Damso"]));

        let classement = rank(votes);
        assert_eq!(classement[0].name, "Périphérique", "0,9 reste devant 0,6");

        // Avec deux voix moyennes, le consensus l'emporte sur une voix forte.
        let mut votes = HashMap::new();
        votes.insert("a".to_string(), vote("Consensuel", 0.6, &["Damso"]));
        votes.get_mut("a").unwrap().score += 0.5; // seconde voix
        votes.insert("b".to_string(), vote("Périphérique", 0.9, &["Damso"]));

        assert_eq!(rank(votes)[0].name, "Consensuel");
    }

    #[test]
    fn le_classement_est_reproductible() {
        // Deux ex æquo ne doivent pas changer de place d'une exécution à
        // l'autre : l'ordre d'une `HashMap` est randomisé par Rust.
        let construire = || {
            let mut votes = HashMap::new();
            votes.insert("mbid-b".to_string(), vote("B", 0.5, &["Damso"]));
            votes.insert("mbid-a".to_string(), vote("A", 0.5, &["Damso"]));
            rank(votes)
        };

        let noms: Vec<String> = construire().into_iter().map(|s| s.name).collect();
        for _ in 0..5 {
            let encore: Vec<String> = construire().into_iter().map(|s| s.name).collect();
            assert_eq!(noms, encore);
        }
    }

    #[test]
    fn la_justification_reste_lisible() {
        assert_eq!(reason(&[]), "Proche de ce que tu écoutes");
        assert_eq!(
            reason(&["Damso".into()]),
            "Parce que tu écoutes Damso"
        );
        assert_eq!(
            reason(&["Damso".into(), "Népal".into()]),
            "Parce que tu écoutes Damso et Népal"
        );
        assert_eq!(
            reason(&["Damso".into(), "Népal".into(), "Josman".into()]),
            "Parce que tu écoutes Damso, Népal et 1 autre"
        );
        assert_eq!(
            reason(&["A".into(), "B".into(), "C".into(), "D".into()]),
            "Parce que tu écoutes A, B et 2 autres"
        );
    }

    #[test]
    fn la_liste_est_plafonnee() {
        let votes: HashMap<String, Vote> = (0..40)
            .map(|index| {
                (
                    format!("mbid-{index:02}"),
                    vote(&format!("Artiste {index}"), f64::from(index), &["Damso"]),
                )
            })
            .collect();

        assert_eq!(rank(votes).len(), SUGGESTIONS);
    }

    #[test]
    fn encode_les_noms_dartistes_exotiques() {
        // Un nom mal encodé casse la recherche en silence.
        assert_eq!(encode("Népal"), "N%C3%A9pal");
        assert_eq!(encode("A$AP Rocky"), "A%24AP%20Rocky");
        assert_eq!(encode("Luv Resval"), "Luv%20Resval");
    }
}
