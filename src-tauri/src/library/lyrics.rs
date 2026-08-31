//! Paroles : lecture et analyse du format LRC.
//!
//! # Deux natures de paroles
//!
//! | Forme | Contenu | Ce qu'on peut en faire |
//! |---|---|---|
//! | Simple | Du texte brut | L'afficher |
//! | **Synchronisée** | Chaque ligne horodatée | La faire défiler en cadence |
//!
//! Les deux vivent dans le même champ de tags. Rien ne les distingue au
//! stockage : c'est à la lecture qu'on reconnaît le format LRC à ses préfixes
//! entre crochets.
//!
//! ```text
//! [ar:Damso]                ← métadonnée, ignorée
//! [00:12.34]Première ligne  ← horodatée
//! [01:05.00]Deuxième ligne
//! [02:10.00][02:40.00]Refrain   ← deux passages, même texte
//! ```

use serde::Serialize;

/// Une ligne horodatée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub at_ms: i64,
    pub text: String,
}

/// Paroles analysées.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    /// Lignes horodatées, triées. Vide si les paroles ne sont pas synchronisées.
    pub synced: Vec<LyricLine>,
    /// Texte brut, toujours renseigné — il sert de repli.
    pub plain: Vec<String>,
}

impl Lyrics {
    pub fn is_synced(&self) -> bool {
        !self.synced.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.synced.is_empty() && self.plain.is_empty()
    }

    /// Index de la ligne en cours à l'instant donné.
    ///
    /// Recherche dichotomique : sur des paroles de cent lignes, appelée
    /// plusieurs fois par seconde, un parcours linéaire ferait travailler
    /// l'interface pour rien.
    pub fn line_at(&self, position_ms: i64) -> Option<usize> {
        if self.synced.is_empty() || position_ms < self.synced[0].at_ms {
            return None;
        }

        // `partition_point` renvoie le nombre de lignes déjà commencées ;
        // la dernière d'entre elles est celle qui est chantée.
        Some(
            self.synced
                .partition_point(|line| line.at_ms <= position_ms)
                .saturating_sub(1),
        )
    }
}

/// Analyse un bloc de paroles brut.
pub fn parse(raw: &str) -> Lyrics {
    let mut synced = Vec::new();
    let mut plain = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (timestamps, text) = split_timestamps(trimmed);

        if timestamps.is_empty() {
            // Ligne sans horodatage : soit des paroles simples, soit une
            // métadonnée LRC qu'il ne faut pas afficher.
            if !is_metadata_tag(trimmed) {
                plain.push(trimmed.to_string());
            }
            continue;
        }

        // Une ligne peut porter plusieurs horodatages : c'est ainsi qu'un
        // refrain répété est écrit une seule fois.
        for at_ms in timestamps {
            synced.push(LyricLine {
                at_ms,
                text: text.to_string(),
            });
        }

        if !text.is_empty() {
            plain.push(text.to_string());
        }
    }

    // Rien ne garantit que le fichier soit trié — un refrain répété en fin de
    // fichier casserait tout l'affichage.
    synced.sort_by_key(|line| line.at_ms);

    Lyrics { synced, plain }
}

/// Sépare les horodatages de tête du texte qui suit.
fn split_timestamps(line: &str) -> (Vec<i64>, &str) {
    let mut timestamps = Vec::new();
    let mut rest = line;

    while rest.starts_with('[') {
        let Some(end) = rest.find(']') else { break };
        let inside = &rest[1..end];

        let Some(at_ms) = parse_timestamp(inside) else {
            break; // ce n'est pas un horodatage : on s'arrête là
        };

        timestamps.push(at_ms);
        rest = rest[end + 1..].trim_start();
    }

    (timestamps, rest.trim())
}

/// `mm:ss`, `mm:ss.xx` ou `mm:ss.xxx` → millisecondes.
fn parse_timestamp(value: &str) -> Option<i64> {
    let (minutes, remainder) = value.split_once(':')?;
    let minutes: i64 = minutes.trim().parse().ok()?;

    let (seconds, fraction) = match remainder.split_once(['.', ':']) {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (remainder, None),
    };

    let seconds: i64 = seconds.trim().parse().ok()?;
    if !(0..60).contains(&seconds) {
        return None;
    }

    // Deux chiffres valent des centièmes, trois des millièmes.
    let millis = match fraction {
        Some(fraction) => {
            let digits: String = fraction.chars().filter(char::is_ascii_digit).collect();
            let parsed: i64 = digits.parse().ok()?;
            match digits.len() {
                0 => 0,
                1 => parsed * 100,
                2 => parsed * 10,
                _ => parsed % 1000,
            }
        }
        None => 0,
    };

    Some(minutes * 60_000 + seconds * 1_000 + millis)
}

/// Une métadonnée LRC (`[ar:…]`, `[ti:…]`, `[by:…]`) n'est pas une parole.
fn is_metadata_tag(line: &str) -> bool {
    let Some(inside) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) else {
        return false;
    };

    inside
        .split_once(':')
        .is_some_and(|(key, _)| !key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_des_paroles_synchronisees() {
        let lyrics = parse("[00:12.34]Première ligne\n[01:05.50]Deuxième ligne");

        assert!(lyrics.is_synced());
        assert_eq!(lyrics.synced.len(), 2);
        assert_eq!(lyrics.synced[0].at_ms, 12_340);
        assert_eq!(lyrics.synced[0].text, "Première ligne");
        assert_eq!(lyrics.synced[1].at_ms, 65_500);
    }

    #[test]
    fn accepte_les_trois_precisions_dhorodatage() {
        assert_eq!(parse_timestamp("00:12"), Some(12_000));
        assert_eq!(parse_timestamp("00:12.5"), Some(12_500));
        assert_eq!(parse_timestamp("00:12.34"), Some(12_340));
        assert_eq!(parse_timestamp("00:12.345"), Some(12_345));
    }

    #[test]
    fn refuse_ce_qui_nest_pas_un_horodatage() {
        assert_eq!(parse_timestamp("ar:Damso"), None);
        assert_eq!(parse_timestamp("bonjour"), None);
        assert_eq!(parse_timestamp("00:99"), None, "99 secondes n'existe pas");
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn les_metadonnees_ne_sont_pas_des_paroles() {
        let lyrics = parse("[ar:Damso]\n[ti:Macarena]\n[00:05.00]Vraie ligne");

        assert_eq!(lyrics.synced.len(), 1);
        assert_eq!(lyrics.plain, vec!["Vraie ligne"]);
    }

    #[test]
    fn un_refrain_repete_est_place_a_chacun_de_ses_passages() {
        // C'est la raison d'être des horodatages multiples : écrire le refrain
        // une seule fois.
        let lyrics = parse("[00:30.00][01:30.00][02:30.00]Refrain");

        assert_eq!(lyrics.synced.len(), 3);
        assert_eq!(lyrics.synced[2].at_ms, 150_000);
        assert!(lyrics.synced.iter().all(|line| line.text == "Refrain"));
    }

    #[test]
    fn les_lignes_sont_triees_meme_si_le_fichier_ne_lest_pas() {
        // Un refrain écrit en fin de fichier casserait sinon tout l'affichage.
        let lyrics = parse("[02:00.00]Fin\n[00:10.00]Début\n[01:00.00]Milieu");

        let ordre: Vec<&str> = lyrics.synced.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(ordre, vec!["Début", "Milieu", "Fin"]);
    }

    #[test]
    fn des_paroles_simples_restent_exploitables() {
        let lyrics = parse("Première ligne\nDeuxième ligne\n\nTroisième");

        assert!(!lyrics.is_synced());
        assert_eq!(lyrics.plain.len(), 3, "les lignes vides sont écartées");
    }

    #[test]
    fn un_texte_vide_ne_donne_rien() {
        assert!(parse("").is_empty());
        assert!(parse("   \n\n  ").is_empty());
    }

    #[test]
    fn une_ligne_horodatee_sans_texte_est_conservee_dans_la_synchronisation() {
        // Les silences instrumentaux s'écrivent ainsi, et il faut les garder :
        // sans eux, la ligne précédente resterait surlignée pendant l'intermède.
        let lyrics = parse("[00:00.00]\n[00:20.00]Première parole");

        assert_eq!(lyrics.synced.len(), 2);
        assert!(lyrics.synced[0].text.is_empty());
    }

    // ── Suivi de la lecture ─────────────────────────────────────────────

    fn chanson() -> Lyrics {
        parse("[00:10.00]Un\n[00:20.00]Deux\n[00:30.00]Trois")
    }

    #[test]
    fn trouve_la_ligne_en_cours() {
        let lyrics = chanson();

        assert_eq!(lyrics.line_at(15_000), Some(0));
        assert_eq!(lyrics.line_at(25_000), Some(1));
        assert_eq!(lyrics.line_at(999_000), Some(2));
    }

    #[test]
    fn la_ligne_bascule_exactement_a_son_horodatage() {
        let lyrics = chanson();

        assert_eq!(lyrics.line_at(19_999), Some(0));
        assert_eq!(lyrics.line_at(20_000), Some(1));
    }

    #[test]
    fn aucune_ligne_avant_la_premiere() {
        // L'introduction instrumentale ne doit surligner personne.
        assert_eq!(chanson().line_at(0), None);
        assert_eq!(chanson().line_at(9_999), None);
    }

    #[test]
    fn des_paroles_non_synchronisees_nont_pas_de_ligne_courante() {
        assert_eq!(parse("Juste du texte").line_at(10_000), None);
    }

    #[test]
    fn le_suivi_reste_correct_sur_des_paroles_longues() {
        let raw: String = (0..300)
            .map(|index| format!("[{:02}:{:02}.00]Ligne {index}\n", index / 60, index % 60))
            .collect();

        let lyrics = parse(&raw);
        assert_eq!(lyrics.synced.len(), 300);
        assert_eq!(lyrics.line_at(150_000), Some(150));
    }
}
