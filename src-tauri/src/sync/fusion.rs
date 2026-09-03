//! Fusionner deux bibliothèques.
//!
//! # Pourquoi cette fonction ne touche à rien
//!
//! Elle ne lit aucune base et n'écrit nulle part : elle reçoit les deux états
//! et rend la liste des changements à appliquer. C'est ce qui la rend
//! vérifiable — les cas tordus d'une fusion (les deux côtés ont changé, un seul
//! a une date, aucun n'en a) se testent en quelques lignes, sans téléphone,
//! sans réseau et sans risquer une bibliothèque.
//!
//! # Pourquoi les deux appareils exécutent le même code
//!
//! Chacun appelle `fusionner(soi, autre)` et applique ce qui en sort. Aucun des
//! deux ne fait autorité, et il n'y a pas un « code du serveur » et un « code
//! du client » qui finiraient par diverger d'une règle.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Un morceau, tel qu'il traverse le réseau.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MorceauSync {
    /// Chemin relatif à la racine de bibliothèque. Première clé d'appariement.
    pub chemin: String,
    pub titre: String,
    pub artiste: Option<String>,
    pub album: Option<String>,
    pub duree_ms: i64,
    pub aime: bool,
    /// Quand le favori a **changé d'état**. Absent pour tout ce qui date
    /// d'avant la synchronisation.
    pub aime_le: Option<i64>,
    pub paroles: Option<String>,
    pub paroles_synchronisees: bool,
}

/// Une playlist, avec l'ordre de ses morceaux.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSync {
    pub nom: String,
    pub modifiee_le: i64,
    /// Les clés d'appariement des morceaux, dans l'ordre de la playlist.
    pub morceaux: Vec<String>,
}

/// Tout ce qu'un appareil expose à l'autre.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatSync {
    /// Le nom que l'appareil se donne — « MacBook de Carlos », « Honor ».
    pub appareil: String,
    pub morceaux: Vec<MorceauSync>,
    pub playlists: Vec<PlaylistSync>,
}

/// Ce qu'un appareil doit appliquer chez lui.
#[derive(Clone, Debug, PartialEq)]
pub enum Changement {
    /// Le favori passe à cet état, daté de ce moment.
    Aime {
        chemin: String,
        aime: bool,
        quand: Option<i64>,
    },
    /// Des paroles arrivent. N'écrase jamais des paroles déjà synchronisées.
    Paroles {
        chemin: String,
        paroles: String,
        synchronisees: bool,
    },
    /// La playlist prend cette composition. Créée si elle n'existe pas.
    Playlist {
        nom: String,
        /// Chemins relatifs **de cet appareil**, dans l'ordre.
        morceaux: Vec<String>,
        quand: i64,
    },
}

/// Un arbitrage : les deux côtés avaient une valeur, il a fallu trancher.
#[derive(Clone, Debug, PartialEq)]
pub struct Arbitrage {
    /// `loved` | `metadata` | `playlist`
    pub genre: &'static str,
    pub sujet: String,
    pub remplace: String,
    pub garde: String,
}

/// Le résultat d'une fusion, du point de vue de celui qui l'a demandée.
#[derive(Debug, Default)]
pub struct Fusion {
    pub changements: Vec<Changement>,
    pub arbitrages: Vec<Arbitrage>,
}

// ════════════════════════════════════════════════════════════════════════════
//  Appariement
// ════════════════════════════════════════════════════════════════════════════

/// Deux morceaux se répondent-ils ?
///
/// # Pourquoi deux clés et non une
///
/// Le chemin relatif est le bon candidat : les deux bibliothèques viennent du
/// même dossier copié, et `21 Savage/2016 - Savage Mode/05 - Bad Guy.mp3` est
/// aussi précis qu'unique. Mais il suffit d'avoir rangé un fichier à la main
/// d'un côté pour le perdre — et le téléphone compte trois cents morceaux de
/// plus que le Mac, preuve que les deux arbres ont divergé.
///
/// La seconde clé — artiste, titre, durée à la seconde près — rattrape ces
/// cas. Elle ne peut pas servir de clé unique à elle seule : deux versions d'un
/// même morceau, un live et un studio de même durée, se confondraient.
fn cle_secondaire(morceau: &MorceauSync) -> String {
    let artiste = morceau.artiste.as_deref().unwrap_or("");
    // La durée est arrondie à la seconde : deux encodages du même morceau
    // diffèrent de quelques millisecondes.
    format!(
        "{}\u{1}{}\u{1}{}",
        normaliser(artiste),
        normaliser(&morceau.titre),
        morceau.duree_ms / 1000,
    )
}

/// Minuscules, sans accents ni ponctuation : « Adèle Castillon » et
/// « adele castillon » sont la même personne.
fn normaliser(texte: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    texte
        .nfd()
        .filter(|caractere| !matches!(*caractere as u32, 0x300..=0x36f))
        .flat_map(char::to_lowercase)
        .filter(|caractere| caractere.is_alphanumeric() || *caractere == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Table d'appariement : pour chaque morceau local, son homologue distant.
fn apparier<'a>(
    locaux: &'a [MorceauSync],
    distants: &'a [MorceauSync],
) -> Vec<(&'a MorceauSync, &'a MorceauSync)> {
    let par_chemin: HashMap<&str, &MorceauSync> = distants
        .iter()
        .map(|morceau| (morceau.chemin.as_str(), morceau))
        .collect();

    // La clé secondaire ne retient que les valeurs **sans ambiguïté** : si deux
    // morceaux distants la partagent, elle ne désigne personne. Apparier au
    // hasard serait pire que ne pas apparier.
    let mut compte: HashMap<String, usize> = HashMap::new();
    for morceau in distants {
        *compte.entry(cle_secondaire(morceau)).or_default() += 1;
    }

    let par_tags: HashMap<String, &MorceauSync> = distants
        .iter()
        .filter(|morceau| compte.get(&cle_secondaire(morceau)) == Some(&1))
        .map(|morceau| (cle_secondaire(morceau), morceau))
        .collect();

    locaux
        .iter()
        .filter_map(|local| {
            let distant = par_chemin
                .get(local.chemin.as_str())
                .copied()
                .or_else(|| par_tags.get(&cle_secondaire(local)).copied())?;

            Some((local, distant))
        })
        .collect()
}

// ════════════════════════════════════════════════════════════════════════════
//  Fusion
// ════════════════════════════════════════════════════════════════════════════

/// Ce que **cet** appareil doit appliquer pour rejoindre l'autre.
pub fn fusionner(soi: &EtatSync, autre: &EtatSync) -> Fusion {
    let mut fusion = Fusion::default();
    let paires = apparier(&soi.morceaux, &autre.morceaux);

    for (local, distant) in &paires {
        fusionner_favori(local, distant, &autre.appareil, &mut fusion);
        fusionner_paroles(local, distant, &mut fusion);
    }

    fusionner_playlists(soi, autre, &paires, &mut fusion);
    fusion
}

/// # Pourquoi un favori sans date ne se perd jamais
///
/// Avant cette version, aimer un morceau ne laissait aucune date : le champ
/// n'existait pas. À la première synchronisation, **aucun** des deux côtés n'a
/// donc d'horodatage, et « le plus récent gagne » n'a rien à comparer.
///
/// Trancher en faveur du local ferait que la première synchronisation
/// n'apporterait aucun favori — exactement ce pour quoi on la lance. Trancher
/// en faveur du distant effacerait les favoris locaux. La seule décision qui
/// ne détruit rien est de **réunir** : sans information, un cœur posé quelque
/// part est un cœur posé.
///
/// Dès qu'un côté a une date, elle l'emporte : elle dit quelque chose que
/// l'autre ignore.
fn fusionner_favori(
    local: &MorceauSync,
    distant: &MorceauSync,
    appareil: &str,
    fusion: &mut Fusion,
) {
    if local.aime == distant.aime {
        return;
    }

    let prendre_le_distant = match (local.aime_le, distant.aime_le) {
        (Some(ici), Some(la_bas)) => la_bas > ici,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        // Aucune date : on réunit. Le distant ne l'emporte donc que s'il aime.
        (None, None) => distant.aime,
    };

    if !prendre_le_distant {
        return;
    }

    fusion.changements.push(Changement::Aime {
        chemin: local.chemin.clone(),
        aime: distant.aime,
        quand: distant.aime_le,
    });

    // Un arbitrage n'est consigné que si les deux côtés avaient une opinion
    // **datée** : ailleurs, il n'y a pas eu de conflit, seulement une absence.
    if local.aime_le.is_some() && distant.aime_le.is_some() {
        fusion.arbitrages.push(Arbitrage {
            genre: "loved",
            sujet: sujet(local),
            remplace: if local.aime { "aimé".into() } else { "pas aimé".into() },
            garde: format!(
                "{} ({appareil})",
                if distant.aime { "aimé" } else { "pas aimé" }
            ),
        });
    }
}

/// # Pourquoi les paroles ne s'arbitrent pas
///
/// Elles ne se contredisent pas : un morceau a des paroles, ou n'en a pas. Le
/// seul cas où l'on remplace ce qu'on a, c'est quand l'autre les a **calées sur
/// la musique** et nous non — un texte qui défile vaut mieux qu'un texte fixe,
/// et l'inverse n'est jamais vrai.
fn fusionner_paroles(local: &MorceauSync, distant: &MorceauSync, fusion: &mut Fusion) {
    let Some(texte) = distant.paroles.as_ref() else {
        return;
    };

    let a_prendre = match local.paroles.as_ref() {
        None => true,
        Some(_) => distant.paroles_synchronisees && !local.paroles_synchronisees,
    };

    if a_prendre {
        fusion.changements.push(Changement::Paroles {
            chemin: local.chemin.clone(),
            paroles: texte.clone(),
            synchronisees: distant.paroles_synchronisees,
        });
    }
}

/// # Pourquoi les playlists se réunissent au lieu de se remplacer
///
/// Une playlist est une liste ordonnée, pas une valeur : « la plus récente
/// gagne » y ferait disparaître d'un coup les dix morceaux ajoutés sur l'autre
/// appareil, sans que rien ne le signale. Les morceaux absents d'un côté y sont
/// donc **ajoutés à la fin**, dans l'ordre où l'autre les avait.
///
/// L'ordre du plus récemment modifié sert de base : c'est lui qu'on a vu en
/// dernier, et le retrouver bousculé serait déroutant.
fn fusionner_playlists(
    soi: &EtatSync,
    autre: &EtatSync,
    paires: &[(&MorceauSync, &MorceauSync)],
    fusion: &mut Fusion,
) {
    // Du chemin distant vers le chemin local : une playlist distante désigne
    // ses morceaux par **ses** chemins, qui ne sont pas les nôtres.
    let traduire: HashMap<&str, &str> = paires
        .iter()
        .map(|(local, distant)| (distant.chemin.as_str(), local.chemin.as_str()))
        .collect();

    let miennes: HashMap<&str, &PlaylistSync> = soi
        .playlists
        .iter()
        .map(|playlist| (playlist.nom.as_str(), playlist))
        .collect();

    for distante in &autre.playlists {
        let traduits: Vec<String> = distante
            .morceaux
            .iter()
            .filter_map(|chemin| traduire.get(chemin.as_str()).map(|local| (*local).to_string()))
            .collect();

        match miennes.get(distante.nom.as_str()) {
            // Elle n'existe pas ici : on la crée telle quelle.
            None => {
                if !traduits.is_empty() {
                    fusion.changements.push(Changement::Playlist {
                        nom: distante.nom.clone(),
                        morceaux: traduits,
                        quand: distante.modifiee_le,
                    });
                }
            }

            Some(mienne) => {
                let deja: std::collections::HashSet<&str> =
                    mienne.morceaux.iter().map(String::as_str).collect();

                let ajouts: Vec<&String> = traduits
                    .iter()
                    .filter(|chemin| !deja.contains(chemin.as_str()))
                    .collect();

                // `continue` et non `return` : les playlists suivantes ont
                // encore le droit d'être examinées.
                if ajouts.is_empty() {
                    continue;
                }

                // La plus récemment modifiée donne l'ordre de base.
                let (base, suite) = if distante.modifiee_le > mienne.modifiee_le {
                    let connus: std::collections::HashSet<&str> =
                        traduits.iter().map(String::as_str).collect();

                    let restants: Vec<String> = mienne
                        .morceaux
                        .iter()
                        .filter(|chemin| !connus.contains(chemin.as_str()))
                        .cloned()
                        .collect();

                    (traduits.clone(), restants)
                } else {
                    (
                        mienne.morceaux.clone(),
                        ajouts.iter().map(|chemin| (*chemin).clone()).collect(),
                    )
                };

                let mut finale = base;
                finale.extend(suite);

                fusion.arbitrages.push(Arbitrage {
                    genre: "playlist",
                    sujet: format!("Playlist : {}", distante.nom),
                    remplace: format!("{} morceaux", mienne.morceaux.len()),
                    garde: format!("{} morceaux ({})", finale.len(), autre.appareil),
                });

                fusion.changements.push(Changement::Playlist {
                    nom: distante.nom.clone(),
                    morceaux: finale,
                    quand: distante.modifiee_le.max(mienne.modifiee_le),
                });
            }
        }
    }
}

/// « Adèle Castillon — Rêve », pour le journal.
fn sujet(morceau: &MorceauSync) -> String {
    match morceau.artiste.as_deref() {
        Some(artiste) => format!("{artiste} — {}", morceau.titre),
        None => morceau.titre.clone(),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn morceau(chemin: &str, titre: &str, artiste: &str, aime: bool) -> MorceauSync {
        MorceauSync {
            chemin: chemin.to_string(),
            titre: titre.to_string(),
            artiste: Some(artiste.to_string()),
            duree_ms: 180_000,
            aime,
            ..MorceauSync::default()
        }
    }

    fn etat(nom: &str, morceaux: Vec<MorceauSync>) -> EtatSync {
        EtatSync {
            appareil: nom.to_string(),
            morceaux,
            playlists: Vec::new(),
        }
    }

    #[test]
    fn sans_date_les_favoris_se_reunissent() {
        // C'est le cas de la toute première synchronisation : personne n'a
        // d'horodatage, et pourtant les cœurs des deux côtés doivent survivre.
        let mac = etat("Mac", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", true)]);

        let vers_le_mac = fusionner(&mac, &tel);
        assert_eq!(
            vers_le_mac.changements,
            vec![Changement::Aime {
                chemin: "a.mp3".to_string(),
                aime: true,
                quand: None,
            }]
        );

        // Et l'inverse ne retire rien : le téléphone garde son favori.
        let vers_le_tel = fusionner(&tel, &mac);
        assert!(vers_le_tel.changements.is_empty());
    }

    #[test]
    fn la_date_la_plus_recente_tranche() {
        let mut ici = morceau("a.mp3", "Rêve", "Adèle", true);
        ici.aime_le = Some(100);

        let mut la_bas = morceau("a.mp3", "Rêve", "Adèle", false);
        la_bas.aime_le = Some(200);

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]));

        assert_eq!(
            fusion.changements,
            vec![Changement::Aime {
                chemin: "a.mp3".to_string(),
                aime: false,
                quand: Some(200),
            }]
        );
        assert_eq!(fusion.arbitrages.len(), 1, "le retrait doit laisser une trace");
    }

    #[test]
    fn une_date_face_a_rien_l_emporte() {
        // Le côté daté sait *quand* il a changé d'avis ; l'autre ne sait rien.
        let ici = morceau("a.mp3", "Rêve", "Adèle", true);

        let mut la_bas = morceau("a.mp3", "Rêve", "Adèle", false);
        la_bas.aime_le = Some(50);

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]));

        assert_eq!(
            fusion.changements,
            vec![Changement::Aime {
                chemin: "a.mp3".to_string(),
                aime: false,
                quand: Some(50),
            }]
        );
        assert!(
            fusion.arbitrages.is_empty(),
            "un seul côté avait une date : ce n'est pas un conflit"
        );
    }

    #[test]
    fn un_chemin_different_se_rattrape_par_les_tags() {
        let ici = morceau("Adele/2024 - Inversions/03 - Reve.mp3", "Rêve", "Adèle", false);
        let la_bas = morceau("Musique/Reve.mp3", "reve", "ADELE", true);

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]));

        assert_eq!(fusion.changements.len(), 1, "les tags doivent apparier");
    }

    #[test]
    fn une_cle_ambigue_n_apparie_personne() {
        // Deux morceaux distants portant les mêmes tags : apparier au hasard
        // serait pire que ne pas apparier.
        let ici = morceau("live.mp3", "Rêve", "Adèle", false);
        let studio = morceau("a.mp3", "Rêve", "Adèle", true);
        let live = morceau("b.mp3", "Rêve", "Adèle", true);

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![studio, live]));

        assert!(fusion.changements.is_empty());
    }

    #[test]
    fn les_paroles_arrivent_mais_n_ecrasent_pas() {
        let mut nu = morceau("a.mp3", "Rêve", "Adèle", false);
        let mut avec = morceau("a.mp3", "Rêve", "Adèle", false);
        avec.paroles = Some("le texte".to_string());

        // Rien → quelque chose : on prend.
        let fusion = fusionner(&etat("Mac", vec![nu.clone()]), &etat("Honor", vec![avec.clone()]));
        assert_eq!(fusion.changements.len(), 1);

        // Quelque chose → autre chose, sans synchronisation : on garde le nôtre.
        nu.paroles = Some("mon texte".to_string());
        let fusion = fusionner(&etat("Mac", vec![nu.clone()]), &etat("Honor", vec![avec.clone()]));
        assert!(fusion.changements.is_empty());

        // Mais un texte qui défile l'emporte sur un texte fixe.
        let mut calees = avec.clone();
        calees.paroles_synchronisees = true;
        let fusion = fusionner(&etat("Mac", vec![nu]), &etat("Honor", vec![calees]));
        assert_eq!(fusion.changements.len(), 1);
    }

    #[test]
    fn une_playlist_inconnue_se_cree_avec_nos_chemins() {
        let ici = morceau("Adele/03 - Reve.mp3", "Rêve", "Adèle", false);
        let la_bas = morceau("Musique/Reve.mp3", "Rêve", "Adèle", false);

        let mut tel = etat("Honor", vec![la_bas]);
        tel.playlists.push(PlaylistSync {
            nom: "Été".to_string(),
            modifiee_le: 10,
            morceaux: vec!["Musique/Reve.mp3".to_string()],
        });

        let fusion = fusionner(&etat("Mac", vec![ici]), &tel);

        assert_eq!(
            fusion.changements,
            vec![Changement::Playlist {
                nom: "Été".to_string(),
                // Traduits : la playlist distante parlait de *ses* chemins.
                morceaux: vec!["Adele/03 - Reve.mp3".to_string()],
                quand: 10,
            }]
        );
    }

    #[test]
    fn une_playlist_commune_ne_perd_aucun_morceau() {
        let un = morceau("un.mp3", "Un", "A", false);
        let deux = morceau("deux.mp3", "Deux", "B", false);

        let mut mac = etat("Mac", vec![un.clone(), deux.clone()]);
        mac.playlists.push(PlaylistSync {
            nom: "Été".to_string(),
            modifiee_le: 5,
            morceaux: vec!["un.mp3".to_string()],
        });

        let mut tel = etat("Honor", vec![un, deux]);
        tel.playlists.push(PlaylistSync {
            nom: "Été".to_string(),
            modifiee_le: 9,
            morceaux: vec!["deux.mp3".to_string()],
        });

        let fusion = fusionner(&mac, &tel);

        let Some(Changement::Playlist { morceaux, .. }) = fusion.changements.first() else {
            panic!("la playlist devait être fusionnée");
        };

        assert_eq!(morceaux.len(), 2, "aucun morceau ne doit disparaître");
        assert!(morceaux.contains(&"un.mp3".to_string()));
        assert!(morceaux.contains(&"deux.mp3".to_string()));
    }

    #[test]
    fn une_playlist_identique_ne_produit_rien() {
        let un = morceau("un.mp3", "Un", "A", false);

        let mut mac = etat("Mac", vec![un.clone()]);
        mac.playlists.push(PlaylistSync {
            nom: "Été".to_string(),
            modifiee_le: 5,
            morceaux: vec!["un.mp3".to_string()],
        });

        let mut tel = etat("Honor", vec![un]);
        tel.playlists.push(PlaylistSync {
            nom: "Été".to_string(),
            modifiee_le: 9,
            morceaux: vec!["un.mp3".to_string()],
        });

        assert!(fusionner(&mac, &tel).changements.is_empty());
    }

    #[test]
    fn plusieurs_playlists_sont_toutes_examinees() {
        // Une playlist identique ne doit pas interrompre l'examen des suivantes.
        let un = morceau("un.mp3", "Un", "A", false);

        let mut mac = etat("Mac", vec![un.clone()]);
        mac.playlists.push(PlaylistSync {
            nom: "Identique".to_string(),
            modifiee_le: 5,
            morceaux: vec!["un.mp3".to_string()],
        });

        let mut tel = etat("Honor", vec![un]);
        tel.playlists.push(PlaylistSync {
            nom: "Identique".to_string(),
            modifiee_le: 9,
            morceaux: vec!["un.mp3".to_string()],
        });
        tel.playlists.push(PlaylistSync {
            nom: "Nouvelle".to_string(),
            modifiee_le: 9,
            morceaux: vec!["un.mp3".to_string()],
        });

        let fusion = fusionner(&mac, &tel);
        assert_eq!(fusion.changements.len(), 1, "« Nouvelle » doit être créée");
    }

    #[test]
    fn la_fusion_est_stable() {
        // Appliquer, puis refusionner, ne doit plus rien produire : sans quoi
        // deux appareils se renverraient indéfiniment les mêmes changements.
        let mut mac = etat("Mac", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", true)]);

        assert_eq!(fusionner(&mac, &tel).changements.len(), 1);

        // On applique.
        mac.morceaux[0].aime = true;
        assert!(fusionner(&mac, &tel).changements.is_empty());
    }
}
