//! Vocabulaire commun aux catalogues commerciaux.
//!
//! # Pourquoi une couche par-dessus les services
//!
//! Deezer, iTunes et MusicBrainz répondent trois formes différentes pour la
//! même question — « quel morceau est-ce ? ». Les comparer suppose de les
//! ramener à une seule fiche, puis de les **noter selon la même règle** : sans
//! cela, mettre leurs réponses côte à côte reviendrait à comparer des
//! confiances qui ne veulent pas dire la même chose.
//!
//! # Ce que la mesure a montré
//!
//! Sur quatorze morceaux sans album de la bibliothèque, chacun pris seul en
//! trouve six. Ensemble, et en écartant une correspondance fausse d'iTunes,
//! ils en trouvent sept — les recouvrements sont partiels, les trous ne sont
//! pas aux mêmes endroits.
//!
//! Deux enseignements en sont tirés, tous deux mesurés :
//!
//! | Constat | Conséquence |
//! |---|---|
//! | La requête stricte `artist:"…" track:"…"` manque « Υ. 2 DIAMANTS » (upsilon grec) et « L'étrangère » | On interroge en **texte libre**, et on trie soi-même |
//! | Le texte libre remonte trois autres titres du même album, dont deux passent le filtre de durée | La **ressemblance des titres** décide avant la durée |
//!
//! Autrement dit : filtrer sur la seule durée attacherait « L'insolence des
//! élus » à « Drapeau noir ». C'est le titre qui identifie, la durée qui
//! corrobore — jamais l'inverse.

use serde::{Deserialize, Serialize};

use crate::library::naming::normalize_key;

/// Une fiche proposée par un catalogue, avant décision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// Le service qui la propose, affiché tel quel à l'utilisateur.
    ///
    /// Savoir d'où vient une proposition fait partie du choix : l'utilisateur
    /// qui reconnaît un catalogue lui fait confiance différemment.
    ///
    /// Une `String` et non un `&'static str` : la fiche revient de l'interface
    /// lorsqu'elle est retenue, et une donnée qui traverse la frontière IPC ne
    /// peut pas emprunter à qui que ce soit.
    pub source: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<u32>,
    /// Durée annoncée par le service, en millisecondes. `0` quand elle manque.
    pub duration_ms: i64,
    pub cover_url: Option<String>,
    /// Vignette, servie plus petite que la pochette définitive.
    ///
    /// Afficher huit pochettes de 1000 px pour un choix qui se fait en 48 px
    /// téléchargerait trois mégaoctets pour rien.
    pub preview_url: Option<String>,
    /// Référence opaque, propre au service, permettant d'en apprendre plus.
    ///
    /// Deezer ne donne pas l'année dans ses résultats de recherche : il faut
    /// un second aller-retour sur l'album, et donc son identifiant. On ne le
    /// dépense qu'au moment où l'utilisateur retient la fiche.
    pub album_ref: Option<String>,
    /// Note de 0 à 1, calculée par [`rank`]. Vaut 0 tant que le tri n'a pas eu
    /// lieu.
    #[serde(default)]
    pub score: f64,
}

impl Candidate {
    /// Fiche minimale, avant enrichissement.
    pub fn new(source: &str, title: String) -> Self {
        Self {
            source: source.to_string(),
            title,
            artist: None,
            album: None,
            year: None,
            duration_ms: 0,
            cover_url: None,
            preview_url: None,
            album_ref: None,
            score: 0.0,
        }
    }
}

/// Ce que l'on cherche : ce que le fichier prétend être.
#[derive(Debug, Clone, Copy)]
pub struct Query<'a> {
    pub artist: Option<&'a str>,
    pub title: &'a str,
    /// Durée du fichier. `0` si inconnue.
    pub duration_ms: i64,
}

/// Écart de durée au-delà duquel une fiche cesse d'être crédible.
///
/// Trente secondes, et non quinze comme pour les pochettes seules : la mesure
/// a montré que « Drapeau noir » dure 215 s dans la bibliothèque et 196 s chez
/// Deezer — dix-neuf secondes d'écart pour le **bon** morceau, parce qu'un
/// fichier téléchargé porte souvent une intro ou une outro que la version
/// commerciale n'a pas. Le titre, lui, ne varie pas ainsi : c'est à lui de
/// trancher, et à la durée de corroborer.
const DURATION_TOLERANCE_MS: i64 = 30_000;

/// Ressemblance minimale des titres pour une acceptation **automatique**.
const AUTO_TITLE: f64 = 0.85;

/// Ressemblance minimale des artistes pour une acceptation **automatique**.
const AUTO_ARTIST: f64 = 0.8;

/// Trie les fiches de la plus crédible à la moins crédible.
///
/// Rien n'est écarté : c'est un tri, pas un filtre. L'utilisateur qui ouvre la
/// liste doit voir aussi les réponses médiocres — parfois la bonne est la
/// quatrième, et une liste vide n'apprend rien.
pub fn rank(mut candidates: Vec<Candidate>, query: &Query<'_>) -> Vec<Candidate> {
    for candidate in &mut candidates {
        candidate.score = score(candidate, query);
    }

    // Tri déterministe : à note égale, l'ordre ne doit pas dépendre du hasard
    // d'un parcours de table, sans quoi deux appels identiques rendraient deux
    // listes différentes.
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.title.cmp(&right.title))
    });

    candidates
}

/// La fiche que l'on peut appliquer **sans demander**.
///
/// Les seuils sont volontairement plus durs que le tri : une proposition
/// affichée n'engage personne, une proposition appliquée engage la
/// bibliothèque.
pub fn auto_pick<'a>(candidates: &'a [Candidate], query: &Query<'_>) -> Option<&'a Candidate> {
    candidates.iter().find(|candidate| {
        similarity(&candidate.title, query.title) >= AUTO_TITLE
            && artist_similarity(candidate, query) >= AUTO_ARTIST
            && duration_agrees(candidate.duration_ms, query.duration_ms)
    })
}

/// Note globale d'une fiche.
fn score(candidate: &Candidate, query: &Query<'_>) -> f64 {
    let title = similarity(&candidate.title, query.title);
    let artist = artist_similarity(candidate, query);
    let duration = duration_score(candidate.duration_ms, query.duration_ms);

    // Le titre pèse le plus : c'est lui qui identifie. La durée ne fait que
    // départager des fiches déjà plausibles.
    0.55 * title + 0.30 * artist + 0.15 * duration
}

/// Ressemblance des artistes, neutre quand l'un des deux manque.
///
/// Un fichier sans artiste ne doit pas faire chuter toutes les fiches à zéro :
/// il n'apporte simplement pas cette corroboration-là.
fn artist_similarity(candidate: &Candidate, query: &Query<'_>) -> f64 {
    match (candidate.artist.as_deref(), query.artist) {
        (Some(left), Some(right)) => similarity(left, right),
        _ => 0.5,
    }
}

/// La durée est-elle compatible ?
fn duration_agrees(candidate_ms: i64, query_ms: i64) -> bool {
    candidate_ms == 0 || query_ms == 0 || (candidate_ms - query_ms).abs() <= DURATION_TOLERANCE_MS
}

/// Note de proximité des durées, entre 0 et 1.
fn duration_score(candidate_ms: i64, query_ms: i64) -> f64 {
    if candidate_ms == 0 || query_ms == 0 {
        return 0.5; // inconnue : ni preuve ni contre-preuve
    }

    let gap = (candidate_ms - query_ms).abs() as f64;
    let tolerance = DURATION_TOLERANCE_MS as f64;

    if gap >= tolerance {
        0.0
    } else {
        1.0 - gap / tolerance
    }
}

/// Ressemblance de deux libellés, entre 0 et 1.
///
/// Un coefficient de Dice sur les mots, après normalisation et retrait des
/// mentions parasites. Ce n'est pas une distance d'édition : « De la lune » et
/// « De la lune (feat. Shadi) » doivent se reconnaître, alors que « Drapeau
/// noir » et « L'insolence des élus » n'ont rien en commun — ce que le partage
/// de mots exprime mieux que le nombre de caractères à corriger.
pub fn similarity(left: &str, right: &str) -> f64 {
    let left = normalize_key(&strip_noise(left));
    let right = normalize_key(&strip_noise(right));

    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }

    let mut right_words: Vec<&str> = right.split(' ').collect();
    let left_words: Vec<&str> = left.split(' ').collect();
    let mut common = 0usize;

    for word in &left_words {
        if let Some(index) = right_words.iter().position(|other| other == word) {
            right_words.remove(index);
            common += 1;
        }
    }

    2.0 * common as f64 / (left_words.len() + right.split(' ').count()) as f64
}

/// Retire ce qu'un titre porte en plus de lui-même.
///
/// « (feat. X) », « [Clip Officiel] », « (prod. by Y) » : ces mentions varient
/// d'un catalogue à l'autre pour le même enregistrement. Les garder ferait
/// baisser la note du bon résultat.
fn strip_noise(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0usize;
    let mut group = String::new();

    for character in value.chars() {
        match character {
            '(' | '[' => {
                depth += 1;
                group.clear();
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                // Une parenthèse qui ne contient pas de mention parasite fait
                // partie du titre : « Ipséité (Part. 2) » n'est pas du bruit.
                if !is_noise(&group) {
                    out.push(' ');
                    out.push_str(&group);
                }
                group.clear();
            }
            _ if depth > 0 => group.push(character),
            _ => out.push(character),
        }
    }

    out
}

/// Le contenu d'une parenthèse est-il une mention de production ?
fn is_noise(group: &str) -> bool {
    const MARKERS: [&str; 8] = [
        "feat", "ft.", "avec", "prod", "clip", "official", "officiel", "audio",
    ];

    let lowered = group.to_lowercase();
    MARKERS.iter().any(|marker| lowered.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(title: &str, artist: &str, duration_ms: i64) -> Candidate {
        Candidate {
            artist: Some(artist.to_string()),
            duration_ms,
            ..Candidate::new("Test", title.to_string())
        }
    }

    #[test]
    fn un_featuring_ne_fait_pas_baisser_la_note() {
        // Les catalogues n'écrivent pas les invités de la même façon ; le
        // morceau, lui, est le même.
        assert!(similarity("De la lune (feat. Shadi)", "De la lune") > 0.95);
        assert!(similarity("Macarena (Clip Officiel)", "Macarena") > 0.95);
    }

    #[test]
    fn une_parenthese_utile_est_conservee() {
        // Toute parenthèse n'est pas du bruit : celle-ci distingue deux
        // morceaux réellement différents.
        assert!(similarity("Ipséité (Part. 2)", "Ipséité") < 0.9);
    }

    #[test]
    fn deux_titres_etrangers_ne_se_ressemblent_pas() {
        assert!(similarity("Drapeau noir", "L'insolence des élus") < 0.2);
    }

    #[test]
    fn les_accents_ne_separent_pas() {
        assert_eq!(similarity("L'étrangère", "L'etrangere"), 1.0);
    }

    #[test]
    fn le_titre_decide_avant_la_duree() {
        // Le cas mesuré : sur « Drapeau noir », deux autres titres du même
        // album passaient le filtre de durée. Sans la ressemblance des titres,
        // l'un d'eux aurait été retenu.
        let query = Query {
            artist: Some("Lord Esperanza"),
            title: "Drapeau noir",
            duration_ms: 215_064,
        };

        let ranked = rank(
            vec![
                candidate("L'insolence des élus", "Lord Esperanza", 224_000),
                candidate("Drapeau noir", "Lord Esperanza", 196_000),
            ],
            &query,
        );

        assert_eq!(ranked[0].title, "Drapeau noir");
    }

    #[test]
    fn une_duree_ecartee_de_dix_neuf_secondes_reste_acceptable() {
        // Un fichier téléchargé porte souvent une intro que la version
        // commerciale n'a pas. Refuser ici, c'était refuser le bon morceau.
        let query = Query {
            artist: Some("Lord Esperanza"),
            title: "Drapeau noir",
            duration_ms: 215_064,
        };
        let ranked = rank(vec![candidate("Drapeau noir", "Lord Esperanza", 196_000)], &query);

        assert!(auto_pick(&ranked, &query).is_some());
    }

    #[test]
    fn un_autre_morceau_du_meme_album_est_refuse_en_automatique() {
        let query = Query {
            artist: Some("Lord Esperanza"),
            title: "Drapeau noir",
            duration_ms: 215_064,
        };
        let ranked = rank(
            vec![candidate("L'insolence des élus", "Lord Esperanza", 224_000)],
            &query,
        );

        assert!(auto_pick(&ranked, &query).is_none());
    }

    #[test]
    fn un_artiste_different_est_refuse_en_automatique() {
        // Le piège mesuré chez iTunes : le bon titre, le mauvais interprète.
        let query = Query {
            artist: Some("Damso"),
            title: "Macarena",
            duration_ms: 206_890,
        };
        let ranked = rank(vec![candidate("Macarena", "Los del Río", 206_000)], &query);

        assert!(auto_pick(&ranked, &query).is_none());
    }

    #[test]
    fn une_duree_inconnue_ne_disqualifie_pas() {
        let query = Query {
            artist: Some("Damso"),
            title: "Macarena",
            duration_ms: 0,
        };
        let ranked = rank(vec![candidate("Macarena", "Damso", 206_000)], &query);

        assert!(auto_pick(&ranked, &query).is_some());
    }

    #[test]
    fn le_tri_ne_jette_rien() {
        // Une liste vide n'apprend rien à l'utilisateur ; une mauvaise
        // proposition en dernière position, si.
        let query = Query {
            artist: Some("Damso"),
            title: "Macarena",
            duration_ms: 206_890,
        };
        let ranked = rank(
            vec![
                candidate("Rien à voir", "Quelqu'un", 100_000),
                candidate("Macarena", "Damso", 206_000),
            ],
            &query,
        );

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].title, "Macarena");
    }

    #[test]
    fn le_tri_est_deterministe() {
        let query = Query {
            artist: None,
            title: "Intro",
            duration_ms: 0,
        };
        let fiches = vec![
            candidate("Intro", "A", 0),
            candidate("Intro", "B", 0),
            candidate("Intro", "C", 0),
        ];

        let first = rank(fiches.clone(), &query);
        let second = rank(fiches, &query);

        let titles = |list: &[Candidate]| {
            list.iter()
                .map(|c| c.artist.clone().unwrap_or_default())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(&first), titles(&second));
    }
}
