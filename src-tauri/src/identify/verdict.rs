//! Corroboration d'une identification acoustique.
//!
//! # Le défaut que ce module répare
//!
//! Onzer acceptait toute correspondance dépassant le seuil de confiance, sans
//! jamais la confronter à ce que le fichier annonçait déjà. Un fichier de Damso
//! s'est ainsi retrouvé étiqueté « carmen (Clip Officiel) » de Stromae — une
//! fiche MusicBrainz créée depuis un rip YouTube, **sans aucune parution**. Le
//! fichier a été retagué, déplacé dans `Stromae/`, et ses tags d'origine
//! perdus.
//!
//! # Le principe
//!
//! Une empreinte acoustique est une **présomption**, pas une preuve. Les tags
//! que le fichier portait déjà en sont une autre. Quand les deux concordent, on
//! écrit ; quand elles se contredisent, il faut une raison sérieuse de croire
//! l'empreinte plutôt que le fichier.
//!
//! | Indice | Ce qu'il vaut |
//! |---|---|
//! | Durées incompatibles | Rédhibitoire : ce n'est pas le même enregistrement |
//! | Artiste concordant | La meilleure corroboration qui soit |
//! | Titre concordant | Bonne, mais les titres génériques trompent |
//! | Aucune parution | Signal de fiche douteuse, souvent versée depuis un rip |
//! | Confiance très haute | Suffit à passer outre des tags muets, jamais des tags contradictoires |
//!
//! **Mieux vaut ne rien écrire que d'écrire faux.** Un fichier non identifié se
//! rattrape d'un clic ; un fichier mal identifié, retagué et déplacé demande de
//! retrouver ce qu'il était.

use crate::library::naming::normalize_key;

/// Écart de durée toléré entre le fichier et l'enregistrement.
///
/// Sept secondes couvrent les silences de fin, les fondus et les quelques
/// dixièmes que se permettent les encodeurs. Au-delà, ce sont deux versions
/// différentes — un montage radio, un live, une autre prise.
const DURATION_TOLERANCE_MS: i64 = 7_000;

/// Confiance au-delà de laquelle l'empreinte se suffit à elle-même…
/// **à condition que rien ne la contredise**.
const STRONG_SCORE: f64 = 0.9;

/// Ce que le fichier annonçait avant toute réécriture.
#[derive(Debug, Clone, Default)]
pub struct FileEvidence {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_ms: i64,
}

/// Ce que proposent AcoustID et MusicBrainz.
#[derive(Debug, Clone, Default)]
pub struct CandidateEvidence {
    pub title: String,
    pub artist: Option<String>,
    /// Durée de l'enregistrement selon MusicBrainz, quand elle est connue.
    pub length_ms: Option<i64>,
    /// Nombre de parutions rattachées. Zéro trahit souvent une fiche versée
    /// depuis un rip, sans existence discographique.
    pub release_count: usize,
    /// Confiance de l'empreinte, entre 0 et 1.
    pub score: f64,
}

/// Décision, toujours motivée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Accept { note: String },
    Reject { note: String },
}

impl Verdict {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Verdict::Accept { .. })
    }

    pub fn note(&self) -> &str {
        match self {
            Verdict::Accept { note } | Verdict::Reject { note } => note,
        }
    }
}

/// Ce que les tags du fichier disent de la proposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Agreement {
    /// Artiste et titre concordent.
    Both,
    /// L'artiste concorde — l'indice le plus solide.
    Artist,
    /// Le titre concorde, mais pas l'artiste.
    Title,
    /// Le fichier ne portait rien d'exploitable : il ne dit ni oui ni non.
    Silent,
    /// Le fichier annonçait autre chose.
    Contradicts,
}

/// Tranche, sur les seuls indices disponibles.
pub fn assess(file: &FileEvidence, candidate: &CandidateEvidence) -> Verdict {
    // ── 1. La durée, d'abord ────────────────────────────────────────────
    // C'est le seul critère purement objectif : il ne dépend d'aucune saisie
    // humaine, ni dans le fichier, ni dans MusicBrainz.
    if let Some(length_ms) = candidate.length_ms {
        let ecart = (file.duration_ms - length_ms).abs();
        if ecart > DURATION_TOLERANCE_MS {
            return Verdict::Reject {
                note: format!(
                    "durée incompatible : le fichier dure {}, l'enregistrement {}",
                    format_duration(file.duration_ms),
                    format_duration(length_ms)
                ),
            };
        }
    }

    let agreement = compare(file, candidate);

    // ── 2. La fiche est-elle sérieuse ? ─────────────────────────────────
    // Un enregistrement sans aucune parution n'a pas d'existence
    // discographique. Seul, ce n'est pas disqualifiant — un inédit récent
    // peut être dans ce cas. Doublé d'un désaccord avec les tags, c'est le
    // profil exact de la fiche versée depuis un rip.
    if candidate.release_count == 0 && agreement == Agreement::Contradicts {
        return Verdict::Reject {
            note: format!(
                "fiche sans parution et tags contradictoires : le fichier annonçait « {} », \
                 la correspondance propose « {} »",
                label(file.artist.as_deref(), file.title.as_deref()),
                label(candidate.artist.as_deref(), Some(&candidate.title)),
            ),
        };
    }

    // ── 3. Contradiction franche ────────────────────────────────────────
    // Les tags du fichier ne sont pas parole d'évangile — c'est bien pour ça
    // qu'on identifie. Mais quand ils désignent un tout autre morceau, il faut
    // une empreinte franchement bonne pour passer outre.
    if agreement == Agreement::Contradicts && candidate.score < STRONG_SCORE {
        return Verdict::Reject {
            note: format!(
                "confiance {:.0} % insuffisante pour contredire les tags du fichier ({})",
                candidate.score * 100.0,
                label(file.artist.as_deref(), file.title.as_deref()),
            ),
        };
    }

    // ── 4. Acceptation, motivée ─────────────────────────────────────────
    let raison = match agreement {
        Agreement::Both => "artiste et titre confirmés par les tags du fichier",
        Agreement::Artist => "artiste confirmé par les tags du fichier",
        Agreement::Title => "titre confirmé par les tags du fichier",
        Agreement::Silent => "le fichier ne portait pas de tags exploitables",
        Agreement::Contradicts => "empreinte assez sûre pour corriger les tags du fichier",
    };

    Verdict::Accept {
        note: format!("confiance {:.0} % — {raison}", candidate.score * 100.0),
    }
}

/// Confronte les tags du fichier à la proposition.
fn compare(file: &FileEvidence, candidate: &CandidateEvidence) -> Agreement {
    let artiste = agrees(file.artist.as_deref(), candidate.artist.as_deref());
    let titre = agrees(file.title.as_deref(), Some(&candidate.title));

    match (artiste, titre) {
        (Some(true), Some(true)) => Agreement::Both,
        (Some(true), _) => Agreement::Artist,
        (_, Some(true)) => Agreement::Title,
        // Aucun des deux champs n'était exploitable : le fichier se tait.
        (None, None) => Agreement::Silent,
        _ => Agreement::Contradicts,
    }
}

/// `None` quand la comparaison n'a pas de sens, faute de valeur exploitable.
fn agrees(gauche: Option<&str>, droite: Option<&str>) -> Option<bool> {
    let gauche = normalize_key(gauche?);
    let droite = normalize_key(droite?);

    if gauche.is_empty() || droite.is_empty() {
        return None;
    }

    // L'inclusion, et pas seulement l'égalité : « carmen » et
    // « carmen (Clip Officiel) » sont le même morceau, « Damso » et
    // « Damso, Hamza » le même artiste principal. Un préfixe de deux lettres
    // suffirait en revanche à tout rapprocher, d'où la longueur minimale.
    const MIN_INCLUSION: usize = 4;

    Some(
        gauche == droite
            || (gauche.len() >= MIN_INCLUSION && droite.contains(&gauche))
            || (droite.len() >= MIN_INCLUSION && gauche.contains(&droite)),
    )
}

/// « Damso — Macarena », pour un message lisible.
fn label(artiste: Option<&str>, titre: Option<&str>) -> String {
    match (artiste, titre) {
        (Some(a), Some(t)) => format!("{a} — {t}"),
        (Some(a), None) => a.to_string(),
        (None, Some(t)) => t.to_string(),
        (None, None) => "sans tags".to_string(),
    }
}

fn format_duration(ms: i64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier(artiste: &str, titre: &str, duree_ms: i64) -> FileEvidence {
        FileEvidence {
            title: Some(titre.to_string()),
            artist: Some(artiste.to_string()),
            duration_ms: duree_ms,
        }
    }

    fn candidat(artiste: &str, titre: &str, duree_ms: i64) -> CandidateEvidence {
        CandidateEvidence {
            title: titre.to_string(),
            artist: Some(artiste.to_string()),
            length_ms: Some(duree_ms),
            release_count: 3,
            score: 0.95,
        }
    }

    // ── Le cas réel qui a motivé ce module ──────────────────────────────

    #[test]
    fn refuse_la_fiche_youtube_qui_a_renomme_damso_en_stromae() {
        // Observé en production : l'enregistrement MusicBrainz retenu
        // s'appelait « carmen (Clip Officiel) », de Stromae, sans aucune
        // parution. Le fichier était un morceau de Damso. Les durées étaient
        // trop proches pour trancher — c'est l'absence de parution jointe au
        // désaccord des tags qui doit disqualifier.
        let fichier = fichier("Damso", "Dieu ne ment jamais", 215_000);
        let mut candidat = candidat("Stromae", "carmen (Clip Officiel)", 217_000);
        candidat.release_count = 0;

        let verdict = assess(&fichier, &candidat);
        assert!(!verdict.is_accepted());
        assert!(verdict.note().contains("sans parution"), "{}", verdict.note());
    }

    #[test]
    fn accepte_macarena_de_damso() {
        // Même situation de départ, mais la correspondance est cohérente :
        // l'artiste concorde, la durée aussi. Rien ne justifie de refuser.
        let verdict = assess(
            &fichier("Damso", "Macarena", 206_000),
            &candidat("Damso", "Macarena", 208_000),
        );

        assert!(verdict.is_accepted());
    }

    // ── La durée ────────────────────────────────────────────────────────

    #[test]
    fn refuse_une_duree_incompatible_meme_avec_des_tags_concordants() {
        // Un montage radio de trois minutes n'est pas la version album de sept,
        // même si tout le reste concorde.
        let verdict = assess(
            &fichier("Daft Punk", "Aerodynamic", 212_000),
            &candidat("Daft Punk", "Aerodynamic", 420_000),
        );

        assert!(!verdict.is_accepted());
        assert!(verdict.note().contains("durée"));
    }

    #[test]
    fn tolere_les_ecarts_de_quelques_secondes() {
        // Silence de fin, fondu, arrondi de l'encodeur : rien de suspect.
        for ecart in [0, 1_000, 4_000, 6_999] {
            let verdict = assess(
                &fichier("Damso", "Macarena", 206_000),
                &candidat("Damso", "Macarena", 206_000 + ecart),
            );
            assert!(verdict.is_accepted(), "écart de {ecart} ms refusé à tort");
        }
    }

    #[test]
    fn une_duree_inconnue_nempeche_pas_didentifier() {
        // MusicBrainz ne renseigne pas toujours la longueur. C'est une
        // corroboration en moins, pas un motif de refus.
        let mut sans_duree = candidat("Damso", "Macarena", 0);
        sans_duree.length_ms = None;

        assert!(assess(&fichier("Damso", "Macarena", 206_000), &sans_duree).is_accepted());
    }

    // ── Les tags ────────────────────────────────────────────────────────

    #[test]
    fn un_fichier_sans_tags_sen_remet_a_lempreinte() {
        // C'est le cas d'usage même de l'identification : le fichier ne sait
        // rien, l'empreinte sait.
        let muet = FileEvidence {
            title: None,
            artist: None,
            duration_ms: 206_000,
        };

        let verdict = assess(&muet, &candidat("Damso", "Macarena", 206_000));
        assert!(verdict.is_accepted());
        assert!(verdict.note().contains("pas de tags"));
    }

    #[test]
    fn une_confiance_ecrasante_corrige_des_tags_faux() {
        // Un fichier mal nommé au téléchargement doit pouvoir être corrigé.
        // C'est toute l'utilité de l'identification, et la raison pour laquelle
        // la contradiction n'est pas rédhibitoire en soi.
        let mut sur = candidat("Damso", "Macarena", 206_000);
        sur.score = 0.98;

        let verdict = assess(&fichier("Inconnu", "Piste 03", 206_000), &sur);
        assert!(verdict.is_accepted());
        assert!(verdict.note().contains("corriger"));
    }

    #[test]
    fn une_confiance_moyenne_ne_suffit_pas_a_contredire_le_fichier() {
        // La comparaison est ici plus parlante qu'un seuil : entre deux
        // candidats contredisant le fichier, seul le très sûr passe.
        let mut moyen = candidat("Stromae", "Alors on danse", 206_000);
        moyen.score = 0.7;

        let mut tres_sur = moyen.clone();
        tres_sur.score = 0.95;

        let fichier = fichier("Damso", "Macarena", 206_000);
        assert!(!assess(&fichier, &moyen).is_accepted());
        assert!(assess(&fichier, &tres_sur).is_accepted());
    }

    #[test]
    fn le_titre_seul_suffit_a_corroborer() {
        // Les tags d'artiste sont les plus souvent saisis n'importe comment
        // (« Damso feat. Hamza », « DAMSO », « damso officiel »).
        let verdict = assess(
            &fichier("", "Macarena", 206_000),
            &candidat("Damso", "Macarena", 206_000),
        );

        assert!(verdict.is_accepted());
    }

    // ── La comparaison des chaînes ──────────────────────────────────────

    #[test]
    fn les_suffixes_de_rip_ne_cassent_pas_la_correspondance() {
        assert_eq!(agrees(Some("carmen"), Some("carmen (Clip Officiel)")), Some(true));
        assert_eq!(agrees(Some("Macarena"), Some("MACARENA")), Some(true));
    }

    #[test]
    fn deux_titres_courts_ne_se_confondent_pas_par_inclusion() {
        // Sans longueur minimale, « Go » rapprocherait « Go » de « Django ».
        assert_eq!(agrees(Some("Go"), Some("Django")), Some(false));
    }

    #[test]
    fn une_valeur_absente_ne_dit_ni_oui_ni_non() {
        assert_eq!(agrees(None, Some("Macarena")), None);
        assert_eq!(agrees(Some("Macarena"), None), None);
        assert_eq!(agrees(Some("   "), Some("Macarena")), None);
    }

    #[test]
    fn une_fiche_sans_parution_passe_si_les_tags_confirment() {
        // Un inédit très récent n'a pas encore de parution : ce n'est pas un
        // motif de refus quand tout le reste concorde.
        let mut inedit = candidat("Damso", "Macarena", 206_000);
        inedit.release_count = 0;

        assert!(assess(&fichier("Damso", "Macarena", 206_000), &inedit).is_accepted());
    }
}
