//! Assemblage du moteur : des données brutes à une playlist.
//!
//! # Le déroulé
//!
//! ```text
//!  1. Chargement    →  vecteurs audio, historique, contexte, transitions, bras
//!  2. Notation      →  chaque stratégie classe TOUTE la bibliothèque
//!  3. Assemblage    →  pour chaque emplacement :
//!                        a. tirage de Thompson  → quelle stratégie ?
//!                        b. meilleur candidat de cette stratégie
//!                        c. filtré par les règles de diversité
//!  4. Traçabilité   →  quelle stratégie a proposé quoi, pour l'apprentissage
//! ```
//!
//! # Pourquoi tout charger en mémoire
//!
//! Cinq mille morceaux et quelques dizaines de milliers d'écoutes tiennent dans
//! quelques mégaoctets. Tout charger permet une notation exhaustive, sans index
//! approché ni approximation. C'est un luxe qu'un service à l'échelle mondiale
//! ne peut pas se payer, et l'un des rares endroits où être local est un
//! avantage technique net.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::analysis::{AudioFeatures, ANALYZER_NAME, ANALYZER_VERSION};
use crate::audio::tracking::EndReason;
use crate::core::{now_ms, Result};

use super::affinity::{self, PlaySignal};
use super::bandit::{self, Arm, Rng, Strategy};
use super::context::{contextual_score, Evidence, ListeningContext, TimePeriod};
use super::diversity::{Candidate, DiversityGuard, DiversityRules, TempoPolicy};
use super::similarity::FeatureSpace;

/// Fenêtre d'historique prise en compte.
///
/// Au-delà d'un an, la décroissance temporelle a réduit le poids d'une écoute à
/// moins de 0,03 % : la charger ne changerait rien au résultat.
const HISTORY_WINDOW_DAYS: f64 = 400.0;

/// Un morceau écouté dans les trois derniers jours est pénalisé : le
/// réentendre aussitôt donne l'impression d'un moteur à court d'idées.
const RECENCY_WINDOW_DAYS: f64 = 3.0;
const RECENCY_PENALTY: f64 = 0.30;

/// En deçà de ce nombre d'écoutes, un morceau relève de la découverte.
const DISCOVERY_MAX_PLAYS: i64 = 2;

/// Un morceau « oublié » doit à la fois avoir été aimé et ne plus avoir été
/// écouté depuis longtemps.
const FORGOTTEN_MIN_AFFINITY: f64 = 0.25;
const FORGOTTEN_MIN_DAYS: f64 = 45.0;

// ════════════════════════════════════════════════════════════════════════════
//  Entrées et sorties
// ════════════════════════════════════════════════════════════════════════════

/// Nature de la playlist demandée.
#[derive(Debug, Clone)]
pub enum PlaylistKind {
    /// Une radio partant d'un morceau : on reste dans son univers sonore.
    Radio { seed_track_id: i64 },
    /// Ce qui convient à ce moment précis de la journée.
    ForNow,
    /// Des morceaux aimés autrefois, plus écoutés depuis longtemps.
    Forgotten,
    /// Ce qui a la meilleure cote en ce moment.
    Loved,
    /// Le fond de bibliothèque jamais lancé une seule fois.
    NeverPlayed,
    /// L'univers sonore d'un artiste : ses morceaux et leurs voisins.
    ArtistMix { artist_id: i64 },
}

impl PlaylistKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Radio { .. } => "radio",
            Self::ForNow => "for_now",
            Self::Forgotten => "forgotten",
            Self::Loved => "loved",
            Self::NeverPlayed => "never_played",
            Self::ArtistMix { .. } => "artist_mix",
        }
    }

    /// Stratégies autorisées, quand la nature du mix les impose.
    ///
    /// # Pourquoi certains mix bridentple bandit
    ///
    /// Le bandit sert à **découvrir ce qui marche** quand la question est
    /// ouverte : « que me proposer maintenant ? ». Mais un mix qui s'appelle
    /// « Jamais écoutés » a déjà répondu à la question. Le laisser piocher dans
    /// l'affinité produirait une playlist de morceaux archi-connus sous un
    /// titre qui promet l'inverse — et rien ne détruit plus vite la confiance
    /// qu'une promesse démentie dès le premier titre.
    ///
    /// `None` laisse le bandit libre, ce qui reste le cas général.
    pub fn forced_strategies(&self) -> Option<&'static [Strategy]> {
        match self {
            Self::Loved => Some(&[Strategy::Affinity]),
            Self::NeverPlayed => Some(&[Strategy::Discovery]),
            // Un mix d'artiste mêle ce qu'on connaît de lui et ce qui lui
            // ressemble : c'est ce mélange qui fait sa saveur.
            Self::ArtistMix { .. } => Some(&[Strategy::Similarity, Strategy::Discovery]),
            _ => None,
        }
    }

    /// Le morceau est-il recevable pour ce type de mix ?
    ///
    /// Filtre appliqué **avant** la notation : un « Jamais écoutés » qui
    /// laisserait passer un morceau écouté trois fois ne serait pas un mix
    /// approximatif, il serait faux.
    pub fn accepts(&self, meta: &TrackMeta) -> bool {
        match self {
            Self::NeverPlayed => meta.play_count == 0,
            Self::ArtistMix { .. } => true,
            _ => true,
        }
    }
}

/// Un morceau retenu, avec la raison de sa présence.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedTrack {
    pub track_id: i64,
    pub strategy: Strategy,
    /// Phrase affichable — l'utilisateur doit pouvoir comprendre pourquoi ce
    /// morceau est là. Une recommandation inexplicable inspire la méfiance.
    pub reason: &'static str,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPlaylist {
    pub session_id: i64,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub tracks: Vec<GeneratedTrack>,
    /// Nombre de morceaux disponibles pour la génération. Permet d'expliquer
    /// une playlist courte plutôt que de laisser l'utilisateur perplexe.
    pub pool_size: usize,
    /// Part de la bibliothèque déjà analysée, entre 0 et 1.
    pub analyzed_ratio: f64,
}

// ════════════════════════════════════════════════════════════════════════════
//  Données chargées
// ════════════════════════════════════════════════════════════════════════════

/// Tout ce que le moteur sait d'un morceau, hors vecteur audio.
#[derive(Debug, Clone, Default)]
pub struct TrackMeta {
    pub artist_id: Option<i64>,
    pub tempo: Option<f32>,
    pub added_at: i64,
    pub is_loved: bool,
    pub play_count: i64,
    pub last_played_at: Option<i64>,
}

/// Instantané complet de la bibliothèque, prêt pour la notation.
pub struct EngineData {
    pub space: FeatureSpace,
    pub meta: HashMap<i64, TrackMeta>,
    pub affinity: HashMap<i64, f64>,
    pub context_evidence: HashMap<i64, Evidence>,
    /// `from_track_id` → (`to_track_id`, score d'enchaînement).
    pub transitions: HashMap<i64, Vec<(i64, f64)>>,
    pub arms: Vec<Arm>,
    pub total_tracks: usize,
}

impl EngineData {
    fn affinity_of(&self, track_id: i64) -> f64 {
        self.affinity.get(&track_id).copied().unwrap_or(0.0)
    }

    fn candidate(&self, track_id: i64) -> Candidate {
        let meta = self.meta.get(&track_id);
        Candidate {
            track_id,
            artist_id: meta.and_then(|m| m.artist_id),
            tempo: meta.and_then(|m| m.tempo),
        }
    }

    /// Nombre de jours depuis la dernière écoute. `None` si jamais écouté.
    fn days_since_played(&self, track_id: i64, now: i64) -> Option<f64> {
        self.meta
            .get(&track_id)?
            .last_played_at
            .map(|at| (now - at) as f64 / 86_400_000.0)
    }
}

/// Ligne de métadonnées telle que SQLite la renvoie.
///
/// Nommée plutôt que laissée en tuple anonyme : à sept colonnes, un tuple brut
/// devient illisible et une inversion de champs passerait inaperçue.
type MetaRow = (i64, Option<i64>, Option<f64>, i64, bool, i64, Option<i64>);

/// Ligne d'événement d'écoute telle que SQLite la renvoie.
type EventRow = (
    i64,            // track_id
    f64,            // completion
    String,         // end_reason
    Option<i64>,    // skip_at_ms
    bool,           // was_manual_selection
    i64,            // started_at
    i64,            // hour_local
    i64,            // weekday
    bool,           // is_weekend
    Option<String>, // output_device
);

/// Charge tout ce dont le moteur a besoin, en une passe.
pub async fn load(pool: &SqlitePool, context: &ListeningContext) -> Result<EngineData> {
    let now = now_ms();

    // ── Métadonnées des morceaux jouables ───────────────────────────────
    let rows: Vec<MetaRow> = sqlx::query_as(
        "SELECT
             t.id,
             (SELECT ta.artist_id FROM track_artists ta
               WHERE ta.track_id = t.id AND ta.role = 'main'
               ORDER BY ta.position LIMIT 1),
             f.tempo,
             t.added_at,
             t.is_loved,
             COALESCE(s.play_count, 0),
             s.last_played_at
         FROM tracks t
         LEFT JOIN track_features f ON f.track_id = t.id
         LEFT JOIN track_stats   s ON s.track_id = t.id
         WHERE t.deleted_at IS NULL AND t.is_available = 1",
    )
    .fetch_all(pool)
    .await?;

    let mut meta = HashMap::with_capacity(rows.len());
    for (id, artist_id, tempo, added_at, is_loved, play_count, last_played_at) in rows {
        meta.insert(
            id,
            TrackMeta {
                artist_id,
                tempo: tempo.map(|value| value as f32),
                added_at,
                is_loved,
                play_count,
                last_played_at,
            },
        );
    }

    // ── Vecteurs audio ──────────────────────────────────────────────────
    // Filtrés sur la version de l'analyseur : mélanger des vecteurs produits
    // par deux algorithmes différents donnerait des distances dénuées de sens.
    let feature_rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT track_id, embedding FROM track_features
         WHERE analyzer = ? AND analyzer_version = ?",
    )
    .bind(ANALYZER_NAME)
    .bind(ANALYZER_VERSION)
    .fetch_all(pool)
    .await?;

    let space = FeatureSpace::build(
        feature_rows
            .into_iter()
            .filter(|(track_id, _)| meta.contains_key(track_id))
            .map(|(track_id, bytes)| (track_id, AudioFeatures::decode_embedding(&bytes)))
            .collect(),
    );

    // ── Historique d'écoute ─────────────────────────────────────────────
    let cutoff = now - (HISTORY_WINDOW_DAYS * 86_400_000.0) as i64;

    let events: Vec<EventRow> = sqlx::query_as(
            "SELECT track_id, completion, end_reason, skip_at_ms, was_manual_selection,
                    started_at, hour_local, weekday, is_weekend, output_device
             FROM play_events
             WHERE started_at >= ?",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

    let mut signals: HashMap<i64, Vec<PlaySignal>> = HashMap::new();
    let mut context_evidence: HashMap<i64, Evidence> = HashMap::new();

    for (
        track_id,
        completion,
        end_reason,
        skip_at_ms,
        was_manual,
        started_at,
        hour,
        _weekday,
        is_weekend,
        device,
    ) in events
    {
        let signal = PlaySignal {
            completion,
            end_reason: parse_end_reason(&end_reason),
            skip_at_ms,
            was_manual_selection: was_manual,
            age_days: (now - started_at) as f64 / 86_400_000.0,
        };

        // La même écoute alimente l'affinité globale et, si elle relève du
        // contexte courant, les preuves contextuelles.
        if context.matches(TimePeriod::from_hour(hour), is_weekend, device.as_deref()) {
            context_evidence
                .entry(track_id)
                .or_default()
                .add(affinity::reward(&signal), affinity::weight(&signal));
        }

        signals.entry(track_id).or_default().push(signal);
    }

    let affinity_scores: HashMap<i64, f64> = meta
        .iter()
        .map(|(track_id, track_meta)| {
            let history = signals.get(track_id).map(Vec::as_slice).unwrap_or(&[]);
            (*track_id, affinity::affinity(history, track_meta.is_loved))
        })
        .collect();

    // ── Enchaînements ───────────────────────────────────────────────────
    let transition_rows: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT from_track_id, to_track_id, play_count, skip_after_count
         FROM track_transitions",
    )
    .fetch_all(pool)
    .await?;

    let mut transitions: HashMap<i64, Vec<(i64, f64)>> = HashMap::new();
    for (from, to, plays, skips) in transition_rows {
        // Score rétréci, comme pour l'affinité : un enchaînement vu une fois ne
        // prouve rien. Un saut juste après compte double en négatif.
        let score = (plays - 2 * skips) as f64 / (plays as f64 + 2.0);
        transitions.entry(from).or_default().push((to, score));
    }

    // ── Bras du bandit ──────────────────────────────────────────────────
    let arm_rows: Vec<(String, f64, f64)> =
        sqlx::query_as("SELECT name, alpha, beta FROM reco_strategies")
            .fetch_all(pool)
            .await?;

    let arms: Vec<Arm> = arm_rows
        .into_iter()
        .filter_map(|(name, alpha, beta)| {
            Strategy::from_key(&name).map(|strategy| Arm {
                strategy,
                alpha,
                beta,
            })
        })
        .collect();

    let total_tracks = meta.len();

    Ok(EngineData {
        space,
        meta,
        affinity: affinity_scores,
        context_evidence,
        transitions,
        arms,
        total_tracks,
    })
}

fn parse_end_reason(value: &str) -> EndReason {
    match value {
        "completed" => EndReason::Completed,
        "skipped" => EndReason::Skipped,
        "replaced" => EndReason::Replaced,
        "error" => EndReason::Error,
        _ => EndReason::Stopped,
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Notation
// ════════════════════════════════════════════════════════════════════════════

/// Ramène un score de [−1, 1] vers [0, 1].
fn to_unit(value: f64) -> f64 {
    (value + 1.0) / 2.0
}

/// Classements produits par chaque stratégie, calculés une seule fois.
struct ScoredPools {
    similarity: Vec<(Candidate, f64)>,
    affinity: Vec<(Candidate, f64)>,
    context: Vec<(Candidate, f64)>,
    discovery: Vec<(Candidate, f64)>,
    forgotten: Vec<(Candidate, f64)>,
}

impl ScoredPools {
    fn for_strategy(&self, strategy: Strategy) -> &[(Candidate, f64)] {
        match strategy {
            Strategy::Similarity => &self.similarity,
            Strategy::Affinity => &self.affinity,
            Strategy::Context => &self.context,
            Strategy::Discovery => &self.discovery,
            Strategy::Forgotten => &self.forgotten,
            // Les enchaînements dépendent du morceau précédent : ils sont
            // recalculés à chaque emplacement, jamais ici.
            Strategy::Transition => &[],
        }
    }
}

/// Note toute la bibliothèque, une fois par stratégie.
fn score_pools(data: &EngineData, kind: &PlaylistKind, now: i64) -> ScoredPools {
    // Point de référence sonore : le morceau de départ, ou le barycentre des
    // morceaux les mieux notés — « le son que tu aimes en ce moment ».
    let reference = match kind {
        PlaylistKind::Radio { seed_track_id } => data
            .space
            .vector(*seed_track_id)
            .map(<[f32]>::to_vec)
            .or_else(|| centroid_of_favourites(data)),
        // Le centre de gravité sonore de l'artiste, et non un seul de ses
        // morceaux : un artiste qui alterne les registres serait sinon réduit
        // à celui du titre tiré au sort.
        PlaylistKind::ArtistMix { artist_id } => {
            centroid_of_artist(data, *artist_id).or_else(|| centroid_of_favourites(data))
        }
        _ => centroid_of_favourites(data),
    };

    let mut similarity = Vec::new();
    let mut affinity_pool = Vec::new();
    let mut context_pool = Vec::new();
    let mut discovery = Vec::new();
    let mut forgotten = Vec::new();

    for (&track_id, meta) in &data.meta {
        if !kind.accepts(meta) {
            continue;
        }

        let candidate = data.candidate(track_id);
        let global_affinity = data.affinity_of(track_id);

        // Pénalité de fraîcheur : réentendre aujourd'hui ce qu'on a entendu
        // hier donne l'impression d'un moteur à court d'idées.
        let recency = match data.days_since_played(track_id, now) {
            Some(days) if days < RECENCY_WINDOW_DAYS => {
                RECENCY_PENALTY * (1.0 - days / RECENCY_WINDOW_DAYS)
            }
            _ => 0.0,
        };

        // ── Similarité sonore ───────────────────────────────────────────
        if let (Some(reference), Some(vector)) = (&reference, data.space.vector(track_id)) {
            let cosine: f32 = reference.iter().zip(vector).map(|(a, b)| a * b).sum();
            similarity.push((candidate, to_unit(f64::from(cosine)) - recency));
        }

        // ── Affinité pure ───────────────────────────────────────────────
        // Pas de pénalité de fraîcheur : demander « mes morceaux préférés »
        // et se voir refuser celui écouté hier serait absurde.
        affinity_pool.push((candidate, to_unit(global_affinity)));

        // ── Contexte ────────────────────────────────────────────────────
        let evidence = data
            .context_evidence
            .get(&track_id)
            .copied()
            .unwrap_or_default();
        context_pool.push((
            candidate,
            to_unit(contextual_score(evidence, global_affinity)) - recency,
        ));

        // ── Découverte ──────────────────────────────────────────────────
        // Le fond de bibliothèque, classé par ressemblance avec ce qui est
        // aimé : découvrir ne veut pas dire tirer au hasard.
        if meta.play_count <= DISCOVERY_MAX_PLAYS {
            let kinship = reference
                .as_ref()
                .zip(data.space.vector(track_id))
                .map(|(reference, vector)| {
                    let cosine: f32 = reference.iter().zip(vector).map(|(a, b)| a * b).sum();
                    to_unit(f64::from(cosine))
                })
                // Sans analyse disponible, la nouveauté seule fait foi.
                .unwrap_or(0.5);

            let freshness = if meta.play_count == 0 { 0.15 } else { 0.0 };
            discovery.push((candidate, kinship + freshness));
        }

        // ── Oubliés ─────────────────────────────────────────────────────
        if global_affinity >= FORGOTTEN_MIN_AFFINITY {
            if let Some(days) = data.days_since_played(track_id, now) {
                if days >= FORGOTTEN_MIN_DAYS {
                    // Plus c'est ancien, plus la redécouverte a de saveur —
                    // mais l'affinité reste le critère dominant.
                    let staleness = (days / 365.0).clamp(0.0, 1.0);
                    forgotten.push((candidate, to_unit(global_affinity) * 0.75 + staleness * 0.25));
                }
            }
        }
    }

    ScoredPools {
        similarity,
        affinity: affinity_pool,
        context: context_pool,
        discovery,
        forgotten,
    }
}

/// Barycentre des morceaux les mieux notés : « le son que tu aimes en ce moment ».
fn centroid_of_favourites(data: &EngineData) -> Option<Vec<f32>> {
    let mut ranked: Vec<(i64, f64)> = data
        .affinity
        .iter()
        .filter(|(track_id, _)| data.space.contains(**track_id))
        .map(|(track_id, score)| (*track_id, *score))
        .collect();

    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));

    let favourites: Vec<i64> = ranked
        .into_iter()
        .filter(|(_, score)| *score > 0.0)
        .take(20)
        .map(|(track_id, _)| track_id)
        .collect();

    if favourites.is_empty() {
        // Bibliothèque sans historique : le centre de l'espace sonore fait un
        // point de départ neutre acceptable.
        let all: Vec<i64> = data.meta.keys().copied().collect();
        return data.space.centroid(&all);
    }

    data.space.centroid(&favourites)
}

/// Centre de gravité sonore d'un artiste.
///
/// `None` si aucun de ses morceaux n'est encore analysé — le mix retombe alors
/// sur le goût général, ce qui vaut mieux qu'un mix vide.
fn centroid_of_artist(data: &EngineData, artist_id: i64) -> Option<Vec<f32>> {
    let tracks: Vec<i64> = data
        .meta
        .iter()
        .filter(|(_, meta)| meta.artist_id == Some(artist_id))
        .map(|(&track_id, _)| track_id)
        .collect();

    if tracks.is_empty() {
        return None;
    }

    data.space.centroid(&tracks)
}

// ════════════════════════════════════════════════════════════════════════════
//  Assemblage
// ════════════════════════════════════════════════════════════════════════════

/// Construit une playlist.
///
/// `rng` est injecté pour rendre la génération reproductible dans les tests.
pub fn generate(
    data: &EngineData,
    kind: &PlaylistKind,
    length: usize,
    rng: &mut Rng,
) -> Vec<GeneratedTrack> {
    let now = now_ms();
    let pools = score_pools(data, kind, now);

    // Une radio partant d'un morceau reste dans son tempo ; une playlist
    // générale alterne.
    let tempo_policy = match kind {
        PlaylistKind::Radio { seed_track_id } => data
            .meta
            .get(seed_track_id)
            .and_then(|meta| meta.tempo)
            .map(|target_bpm| TempoPolicy::Cohesive { target_bpm })
            .unwrap_or_default(),
        _ => TempoPolicy::Varied,
    };

    let mut guard = DiversityGuard::new(diversity_rules_for(data, length, tempo_policy));

    // Le morceau de départ ouvre sa propre radio : l'utilisateur s'attend à
    // l'entendre en premier.
    if let PlaylistKind::Radio { seed_track_id } = kind {
        if data.meta.contains_key(seed_track_id) {
            guard.push(data.candidate(*seed_track_id));
        }
    }

    let mut chosen = Vec::with_capacity(length);
    let mut previous_track_id = match kind {
        PlaylistKind::Radio { seed_track_id } => Some(*seed_track_id),
        _ => None,
    };

    // Les stratégies inapplicables faute de données sont écartées d'emblée :
    // laisser le bandit tirer une stratégie vide gaspillerait des emplacements.
    let allowed = kind.forced_strategies();
    let arms: Vec<Arm> = data
        .arms
        .iter()
        .copied()
        .filter(|arm| allowed.is_none_or(|list| list.contains(&arm.strategy)))
        .filter(|arm| strategy_is_usable(arm.strategy, &pools, data))
        .collect();

    if arms.is_empty() {
        return chosen;
    }

    // Marge de tentatives : une stratégie peut ne rien proposer d'admissible
    // sur un emplacement donné, sans que la playlist doive s'arrêter là.
    let mut attempts = 0;
    let max_attempts = length * 6;

    while chosen.len() < length && attempts < max_attempts {
        attempts += 1;

        let Some(strategy) = bandit::select(&arms, rng) else {
            break;
        };

        let candidates = match strategy {
            Strategy::Transition => transition_candidates(data, previous_track_id),
            other => pools.for_strategy(other).to_vec(),
        };

        let Some((candidate, score)) = guard.best(&candidates) else {
            continue; // cette stratégie n'a rien d'admissible ici
        };

        guard.push(candidate);
        previous_track_id = Some(candidate.track_id);

        chosen.push(GeneratedTrack {
            track_id: candidate.track_id,
            strategy,
            reason: strategy.label(),
            score,
        });
    }

    chosen
}

/// Adapte les règles de diversité à la bibliothèque réellement disponible.
///
/// # Le piège que ça évite
///
/// Un quota fixe de trois morceaux par artiste rend une playlist de vingt
/// titres **mathématiquement impossible** dans une bibliothèque de six
/// artistes : 6 × 3 = 18. Le moteur produirait alors des playlists
/// mystérieusement courtes, sans que rien ne l'explique.
///
/// Le quota est donc calculé pour que la longueur demandée reste atteignable,
/// avec une marge de trois emplacements — la sélection étant gloutonne, elle a
/// besoin d'un peu de jeu pour ne pas se retrouver coincée.
///
/// Le délai de carence subit la même contrainte : avec deux artistes en tout,
/// exiger deux morceaux d'écart est irréalisable.
pub fn diversity_rules_for(
    data: &EngineData,
    length: usize,
    tempo_policy: TempoPolicy,
) -> DiversityRules {
    let distinct_artists = data
        .meta
        .values()
        .filter_map(|meta| meta.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .len()
        .max(1);

    let defaults = DiversityRules::default();

    // Point de départ : le quota par défaut, ou la part nécessaire si la
    // bibliothèque est peu diversifiée.
    let mut max_per_artist = defaults.max_per_artist.max(length.div_ceil(distinct_artists));

    // Marge de manœuvre pour la sélection gloutonne.
    while distinct_artists * max_per_artist < length + 3 {
        max_per_artist += 1;
    }

    DiversityRules {
        artist_cooldown: defaults.artist_cooldown.min(distinct_artists.saturating_sub(1)),
        max_per_artist,
        tempo_policy,
        tempo_window: defaults.tempo_window,
    }
}

/// Une stratégie a-t-elle de quoi proposer quelque chose ?
fn strategy_is_usable(strategy: Strategy, pools: &ScoredPools, data: &EngineData) -> bool {
    match strategy {
        Strategy::Transition => !data.transitions.is_empty(),
        Strategy::Similarity => !pools.similarity.is_empty(),
        Strategy::Discovery => !pools.discovery.is_empty(),
        Strategy::Forgotten => !pools.forgotten.is_empty(),
        Strategy::Affinity => !pools.affinity.is_empty(),
        Strategy::Context => !pools.context.is_empty(),
    }
}

/// Candidats issus des enchaînements observés après un morceau donné.
fn transition_candidates(data: &EngineData, from: Option<i64>) -> Vec<(Candidate, f64)> {
    let Some(from) = from else {
        return Vec::new();
    };

    data.transitions
        .get(&from)
        .map(|targets| {
            targets
                .iter()
                .filter(|(to, _)| data.meta.contains_key(to))
                .map(|(to, score)| (data.candidate(*to), to_unit(*score)))
                .collect()
        })
        .unwrap_or_default()
}

// ════════════════════════════════════════════════════════════════════════════
//  Persistance et apprentissage
// ════════════════════════════════════════════════════════════════════════════

/// Enregistre la playlist et ses propositions.
///
/// Sans cette trace, impossible d'attribuer plus tard un succès ou un échec à
/// la bonne stratégie : le bandit n'aurait rien pour apprendre.
pub async fn save_session(
    pool: &SqlitePool,
    kind: &PlaylistKind,
    context: &ListeningContext,
    tracks: &[GeneratedTrack],
) -> Result<i64> {
    let mut tx = pool.begin().await?;

    let seed = match kind {
        PlaylistKind::Radio { seed_track_id } => Some(*seed_track_id),
        _ => None,
    };

    let session_id: i64 = sqlx::query_scalar(
        "INSERT INTO reco_sessions (kind, seed_track_id, context, track_count, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(kind.as_str())
    .bind(seed)
    .bind(serde_json::to_string(context).unwrap_or_default())
    .bind(tracks.len() as i64)
    .bind(now_ms())
    .fetch_one(&mut *tx)
    .await?;

    for (position, track) in tracks.iter().enumerate() {
        sqlx::query(
            "INSERT INTO reco_proposals (session_id, track_id, position, strategy, score)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(track.track_id)
        .bind(position as i64)
        .bind(track.strategy.as_str())
        .bind(track.score)
        .execute(&mut *tx)
        .await?;
    }

    // Le compteur de propositions sert au diagnostic : il permet de distinguer
    // « cette stratégie échoue » de « cette stratégie n'est jamais tirée ».
    for track in tracks {
        sqlx::query(
            "UPDATE reco_strategies SET proposals = proposals + 1, updated_at = ? WHERE name = ?",
        )
        .bind(now_ms())
        .bind(track.strategy.as_str())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(session_id)
}

/// Enregistre le verdict d'une écoute issue d'une recommandation.
///
/// C'est l'unique boucle d'apprentissage du moteur : sans elle, les bras du
/// bandit resteraient éternellement à leur valeur initiale.
pub async fn record_outcome(
    pool: &SqlitePool,
    session_id: i64,
    track_id: i64,
    success: bool,
) -> Result<()> {
    let strategy: Option<String> = sqlx::query_scalar(
        "SELECT strategy FROM reco_proposals WHERE session_id = ? AND track_id = ? LIMIT 1",
    )
    .bind(session_id)
    .bind(track_id)
    .fetch_optional(pool)
    .await?;

    let Some(strategy) = strategy else {
        return Ok(()); // écoute sans rapport avec une proposition
    };

    let mut tx = pool.begin().await?;

    sqlx::query(
        "UPDATE reco_proposals SET outcome = ? WHERE session_id = ? AND track_id = ?",
    )
    .bind(i64::from(success))
    .bind(session_id)
    .bind(track_id)
    .execute(&mut *tx)
    .await?;

    // Un succès renforce α, un échec renforce β : c'est toute la mise à jour
    // bayésienne d'une loi Beta.
    let column = if success { "alpha" } else { "beta" };
    sqlx::query(&format!(
        "UPDATE reco_strategies SET {column} = {column} + 1, updated_at = ? WHERE name = ?"
    ))
    .bind(now_ms())
    .bind(&strategy)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
