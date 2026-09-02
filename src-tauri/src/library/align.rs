//! Caler des paroles connues sur ce que le modèle entend.
//!
//! # Le problème, et pourquoi il n'est pas celui qu'on croit
//!
//! Deux cent soixante morceaux portent leurs **vraies** paroles, sans aucun
//! horodatage. On sait donc *quoi* afficher ; on ignore *quand*.
//!
//! La tentation serait d'afficher directement la transcription du modèle. Ce
//! serait un mauvais échange : sur du rap français, Whisper se trompe de mot
//! régulièrement, et l'utilisateur verrait défiler un texte approximatif à la
//! place de paroles exactes qu'il possédait déjà.
//!
//! ```text
//!   paroles officielles     transcription du modèle
//!   « J'm'arrache à lire »  « [00:21.8] je marrache alire »
//!            │                          │
//!            └──────────  ALIGNEMENT  ──┘
//!                          │
//!            « [00:21.8] J'm'arrache à lire »
//!              ↑ les mots officiels, la minuterie du modèle
//! ```
//!
//! **Les mots restent ceux de l'auteur, seule la minuterie vient du modèle.**
//! La pire erreur possible devient un décalage de quelques secondes — jamais un
//! mot inventé.
//!
//! # Comment l'alignement décide
//!
//! Un Needleman-Wunsch sur les mots normalisés : le même algorithme qu'en
//! bio-informatique pour aligner deux séquences dont l'une a des trous et des
//! erreurs. Il tolère qu'un mot manque, qu'un autre soit mal entendu, et
//! retrouve la correspondance globale la plus vraisemblable — là où une
//! comparaison ligne à ligne se perdrait dès le premier décalage.
//!
//! Chaque ligne de paroles reçoit l'horodatage du **premier mot** qui lui a été
//! apparié. Les lignes qu'aucun mot n'a atteintes — un refrain que le modèle a
//! sauté, un passage inaudible — sont replacées par interpolation entre leurs
//! voisines : mieux vaut une seconde approximative qu'une ligne qui ne défile
//! jamais.

use crate::library::naming::normalize_key;

/// Un mot entendu par le modèle, avec l'instant où il commence.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardWord {
    pub at_ms: i64,
    pub text: String,
}

/// Récompense d'un mot identique.
const MATCH: i32 = 2;
/// Pénalité d'un mot différent.
const MISMATCH: i32 = -1;
/// Pénalité d'un trou — plus douce que la discordance : le modèle saute des
/// mots bien plus souvent qu'il n'en invente.
const GAP: i32 = -1;

/// Cale des paroles connues sur une transcription horodatée.
///
/// Retourne le texte au format `.lrc`, ou `None` si l'appariement est trop
/// pauvre pour être crédible.
pub fn align(lyrics: &str, heard: &[HeardWord]) -> Option<String> {
    let lines: Vec<&str> = lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() || heard.is_empty() {
        return None;
    }

    // Chaque mot des paroles garde le numéro de sa ligne : c'est ce lien qui
    // permet, une fois l'alignement fait, de reporter l'instant sur la ligne.
    let mut words: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        for word in normalize_key(line).split(' ').filter(|w| !w.is_empty()) {
            words.push((index, word.to_string()));
        }
    }

    let spoken: Vec<String> = heard.iter().map(|word| normalize_key(&word.text)).collect();

    if words.is_empty() || spoken.iter().all(String::is_empty) {
        return None;
    }

    let pairs = needleman_wunsch(&words, &spoken);

    // Instant retenu pour chaque ligne : celui de son premier mot apparié.
    let mut starts: Vec<Option<i64>> = vec![None; lines.len()];
    let mut matched = 0usize;

    for (word_index, heard_index) in pairs {
        let Some((line, _)) = words.get(word_index) else {
            continue;
        };
        let Some(word) = heard.get(heard_index) else {
            continue;
        };

        matched += 1;
        let slot = &mut starts[*line];
        if slot.is_none_or(|current| word.at_ms < current) {
            *slot = Some(word.at_ms);
        }
    }

    // Un appariement trop maigre veut dire que la transcription ne parle pas du
    // même morceau — mauvais fichier, instrumental, langue non reconnue. Une
    // synchronisation fausse serait pire que pas de synchronisation.
    if matched * 4 < words.len() {
        return None;
    }

    let starts = fill_gaps(starts, heard);

    let mut out = String::new();
    for (line, at_ms) in lines.iter().zip(starts) {
        out.push_str(&format!("{}{line}\n", stamp(at_ms)));
    }

    Some(out.trim_end().to_string())
}

/// Comble les lignes qu'aucun mot n'a atteintes.
///
/// Par interpolation entre les voisines connues. Une ligne sans horodatage ne
/// défilerait jamais : elle resterait affichée pendant que la musique avance,
/// ce qui se remarque bien plus qu'une seconde d'imprécision.
fn fill_gaps(starts: Vec<Option<i64>>, heard: &[HeardWord]) -> Vec<i64> {
    let last = heard.last().map_or(0, |word| word.at_ms);
    let mut out = vec![0_i64; starts.len()];
    let mut previous = 0_i64;

    for index in 0..starts.len() {
        match starts[index] {
            Some(at_ms) => {
                // La monotonie est une garantie de l'affichage : une ligne qui
                // reculerait ferait clignoter le suivi.
                previous = at_ms.max(previous);
                out[index] = previous;
            }
            None => {
                let next = starts[index + 1..]
                    .iter()
                    .flatten()
                    .next()
                    .copied()
                    .unwrap_or(last.max(previous));

                let holes = starts[index..]
                    .iter()
                    .take_while(|slot| slot.is_none())
                    .count() as i64;

                let step = (next - previous).max(0) / (holes + 1);
                previous += step;
                out[index] = previous;
            }
        }
    }

    out
}

/// Alignement global de deux séquences de mots.
///
/// Retourne les couples (indice dans les paroles, indice dans l'entendu) des
/// mots réellement identiques. Les discordances et les trous ne produisent pas
/// de couple : ils ne servent qu'à garder les deux séquences en phase.
fn needleman_wunsch(words: &[(usize, String)], spoken: &[String]) -> Vec<(usize, usize)> {
    let (rows, columns) = (words.len() + 1, spoken.len() + 1);
    let mut score = vec![0_i32; rows * columns];

    for row in 1..rows {
        score[row * columns] = GAP * row as i32;
    }
    for (column, cell) in score.iter_mut().enumerate().take(columns).skip(1) {
        *cell = GAP * column as i32;
    }

    for row in 1..rows {
        for column in 1..columns {
            let same = words[row - 1].1 == spoken[column - 1];
            let diagonal = score[(row - 1) * columns + column - 1]
                + if same { MATCH } else { MISMATCH };
            let up = score[(row - 1) * columns + column] + GAP;
            let left = score[row * columns + column - 1] + GAP;

            score[row * columns + column] = diagonal.max(up).max(left);
        }
    }

    // Remontée : on repart du coin et on reconstitue le chemin.
    //
    // # Pourquoi le trou passe avant l'appariement à égalité
    //
    // Un refrain revient plusieurs fois : le même vers existe alors à
    // plusieurs instants, et l'algorithme n'a aucune raison de préférer l'un à
    // l'autre — les scores sont identiques. Mesuré sur « HS 808 », dont
    // l'introduction est chantée deux fois : à égalité, la remontée retenait
    // la **seconde** occurrence, et la première ligne s'affichait cinq
    // secondes trop tard.
    //
    // Sauter le mot entendu quand c'est aussi bon que l'apparier revient à
    // chercher l'appariement plus tôt. À égalité stricte, on choisit donc
    // toujours **le plus tôt** : c'est ce que l'auditeur attend, puisque la
    // ligne doit apparaître dès qu'elle est chantée pour la première fois.
    let mut pairs = Vec::new();
    let (mut row, mut column) = (words.len(), spoken.len());

    while row > 0 && column > 0 {
        let same = words[row - 1].1 == spoken[column - 1];
        let diagonal = score[(row - 1) * columns + column - 1]
            + if same { MATCH } else { MISMATCH };
        let here = score[row * columns + column];

        if here == score[row * columns + column - 1] + GAP {
            column -= 1;
        } else if here == diagonal {
            if same {
                pairs.push((row - 1, column - 1));
            }
            row -= 1;
            column -= 1;
        } else {
            row -= 1;
        }
    }

    pairs.reverse();
    pairs
}

/// Horodatage au format `.lrc`.
pub fn stamp(at_ms: i64) -> String {
    let at_ms = at_ms.max(0);
    format!(
        "[{:02}:{:02}.{:02}]",
        at_ms / 60_000,
        (at_ms % 60_000) / 1000,
        (at_ms % 1000) / 10
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sonde manuelle : cale de vraies paroles sur un vrai morceau.
    ///
    /// Ignorée par défaut — elle demande whisper, ffmpeg, le modèle et le SSD.
    #[test]
    #[ignore]
    fn sonde_un_alignement_reel() {
        let audio = std::env::var("ONZER_AUDIO").expect("ONZER_AUDIO=<fichier>");
        let paroles = std::fs::read_to_string(
            std::env::var("ONZER_PAROLES").expect("ONZER_PAROLES=<fichier>"),
        )
        .unwrap();

        let modele = std::env::var("ONZER_MODELE").ok();
        let transcriber = crate::library::transcribe::Transcriber::detect(modele.as_deref())
            .expect("whisper, ffmpeg et le modèle");

        let debut = std::time::Instant::now();
        let entendu = transcriber.hear(std::path::Path::new(&audio)).unwrap();
        println!("{} mots entendus en {:?}", entendu.len(), debut.elapsed());
        for mot in entendu.iter().take(12) {
            print!("{}:{} ", mot.at_ms, mot.text);
        }
        println!();

        match align(&paroles, &entendu) {
            Some(lrc) => {
                println!("--- {} lignes calées ---", lrc.lines().count());
                for line in lrc.lines().take(8) {
                    println!("{line}");
                }
            }
            None => println!("appariement refusé"),
        }
    }

    fn heard(mots: &[(i64, &str)]) -> Vec<HeardWord> {
        mots.iter()
            .map(|(at_ms, text)| HeardWord {
                at_ms: *at_ms,
                text: (*text).to_string(),
            })
            .collect()
    }

    #[test]
    fn un_refrain_est_date_a_son_premier_passage() {
        // Le cas mesuré sur « HS 808 », dont l'introduction est chantée deux
        // fois : à égalité de score, l'algorithme retenait la seconde
        // occurrence et la ligne s'affichait cinq secondes trop tard.
        let paroles = "Refrain ici\nCouplet apres";
        let entendu = heard(&[
            (1000, "refrain"),
            (1400, "ici"),
            (20000, "refrain"),
            (20400, "ici"),
            (25000, "couplet"),
            (25400, "apres"),
        ]);

        let lrc = align(paroles, &entendu).unwrap();
        assert!(lrc.starts_with("[00:01.00]"), "{lrc}");
    }

    #[test]
    fn les_mots_affiches_restent_ceux_des_paroles() {
        // C'est toute la raison d'être de ce module : le modèle entend
        // « je marrache alire », l'utilisateur doit lire « J'm'arrache à lire ».
        let paroles = "J'm'arrache à lire\nDes livres anciens";
        let entendu = heard(&[
            (1000, "je"),
            (1200, "marrache"),
            (1400, "a"),
            (1600, "lire"),
            (3000, "des"),
            (3200, "livres"),
            (3400, "anciens"),
        ]);

        let lrc = align(paroles, &entendu).expect("alignement");

        assert!(lrc.contains("J'm'arrache à lire"), "{lrc}");
        assert!(!lrc.contains("marrache"), "{lrc}");
    }

    #[test]
    fn chaque_ligne_recoit_l_instant_de_son_premier_mot() {
        let paroles = "Premiere ligne\nDeuxieme ligne";
        let entendu = heard(&[
            (1000, "premiere"),
            (1500, "ligne"),
            (4000, "deuxieme"),
            (4500, "ligne"),
        ]);

        let lrc = align(paroles, &entendu).unwrap();
        let lignes: Vec<&str> = lrc.lines().collect();

        assert!(lignes[0].starts_with("[00:01.00]"), "{lrc}");
        assert!(lignes[1].starts_with("[00:04.00]"), "{lrc}");
    }

    #[test]
    fn un_mot_mal_entendu_ne_decale_pas_la_suite() {
        // Whisper se trompe : l'alignement doit rester en phase malgré tout.
        let paroles = "Le soleil brille\nLa nuit tombe";
        let entendu = heard(&[
            (1000, "le"),
            (1200, "soleil"),
            (1400, "brillait"), // mal entendu
            (5000, "la"),
            (5200, "nuit"),
            (5400, "tombe"),
        ]);

        let lrc = align(paroles, &entendu).unwrap();
        assert!(lrc.lines().nth(1).unwrap().starts_with("[00:05.00]"), "{lrc}");
    }

    #[test]
    fn une_ligne_sautee_est_replacee_entre_ses_voisines() {
        // Un refrain que le modèle n'a pas entendu ne doit pas rester figé à
        // zéro : il défilerait au tout début et plus jamais ensuite.
        let paroles = "Debut\nRefrain inaudible\nFin";
        let entendu = heard(&[(1000, "debut"), (9000, "fin")]);

        let lrc = align(paroles, &entendu).unwrap();
        let milieu = lrc.lines().nth(1).unwrap();

        assert!(milieu.starts_with("[00:05."), "interpolé au milieu : {lrc}");
    }

    #[test]
    fn les_horodatages_ne_reculent_jamais() {
        let paroles = "Une\nDeux\nTrois";
        let entendu = heard(&[(5000, "une"), (2000, "deux"), (9000, "trois")]);

        let lrc = align(paroles, &entendu).unwrap();
        let temps: Vec<&str> = lrc.lines().map(|l| &l[1..9]).collect();

        assert!(temps.windows(2).all(|pair| pair[0] <= pair[1]), "{lrc}");
    }

    #[test]
    fn une_transcription_etrangere_est_refusee() {
        // Mauvais fichier, instrumental, langue non reconnue : mieux vaut
        // rendre None que d'inventer une synchronisation.
        let paroles = "J'm'arrache à lire des livres anciens";
        let entendu = heard(&[(1000, "hello"), (2000, "world"), (3000, "goodbye")]);

        assert!(align(paroles, &entendu).is_none());
    }

    #[test]
    fn des_entrees_vides_ne_produisent_rien() {
        assert!(align("", &heard(&[(0, "mot")])).is_none());
        assert!(align("Des paroles", &[]).is_none());
    }

    #[test]
    fn l_horodatage_respecte_le_format_lrc() {
        assert_eq!(stamp(0), "[00:00.00]");
        assert_eq!(stamp(61_230), "[01:01.23]");
        assert_eq!(stamp(-5), "[00:00.00]");
    }

    #[test]
    fn le_resultat_est_relisible_par_l_analyseur() {
        // La boucle doit se refermer : ce qu'on écrit ici, `lyrics::parse` doit
        // le reconnaître comme synchronisé.
        let paroles = "Premiere ligne\nDeuxieme ligne";
        let entendu = heard(&[(1000, "premiere"), (1200, "ligne"), (4000, "deuxieme")]);

        let lrc = align(paroles, &entendu).unwrap();
        assert!(crate::library::lyrics::is_synced_text(&lrc), "{lrc}");
    }
}
