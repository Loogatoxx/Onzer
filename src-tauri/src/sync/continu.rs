//! La télécommande : ce que chaque appareil sait de l'autre, en continu.
//!
//! # Ce que « synchronisé » veut dire ici, et ce qu'il ne veut pas dire
//!
//! Deux appareils avec chacun ses haut-parleurs ne peuvent pas jouer le même
//! morceau en même temps : on entendrait un écho, et aucune horloge partagée ne
//! le rattraperait. **Un seul tient le son.** L'autre voit ce qui passe, le
//! commande, et peut le reprendre — c'est ce que fait n'importe quelle
//! télécommande, et c'est la seule chose qui ait du sens à deux mètres près.
//!
//! # Pourquoi une longue attente HTTP plutôt qu'un WebSocket
//!
//! Le besoin est : « préviens-moi dès que ça change », sur un réseau local,
//! entre deux pairs qui se parlent déjà en HTTP. Un WebSocket demanderait deux
//! dépendances de plus — l'extension du serveur, et un client qui sache la
//! parler — à compiler pour quatre architectures Android.
//!
//! Une requête `GET` qui **ne répond pas tout de suite** fait le même travail :
//! le serveur la garde ouverte jusqu'à ce que le tableau change, puis répond.
//! Un changement traverse en un aller-retour — quelques millisecondes sur un
//! réseau local — et une liaison au repos ne coûte qu'une requête toutes les
//! vingt-cinq secondes.
//!
//! # Pourquoi le tableau ne porte pas la file
//!
//! Il est republié à chaque seconde. Y mettre deux mille chemins reviendrait à
//! envoyer six mégaoctets par minute pour afficher un titre. Reprendre la
//! lecture chez soi a déjà sa route — celle de la fusion, qui rend la file une
//! fois, au moment où on la demande.

use serde::{Deserialize, Serialize};

/// Ce que les deux appareils lisent au même endroit.
///
/// Un seul document, une seule version qui monte. Chacun y écrit ce qui le
/// concerne : celui qui joue y met son état, celui qui commande y met son
/// ordre.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tableau {
    /// Monte à chaque écriture. C'est elle qui réveille les attentes.
    pub version: u64,
    /// L'appareil qui tient le son, s'il y en a un.
    pub source: Option<String>,
    pub titre: Option<String>,
    pub artiste: Option<String>,
    pub position_ms: i64,
    pub duree_ms: i64,
    pub en_lecture: bool,
    /// Le dernier ordre émis, quel qu'en soit le destinataire.
    pub ordre: Option<Ordre>,
}

/// Un ordre adressé à l'appareil qui tient le son.
///
/// # Pourquoi il porte un destinataire et un numéro
///
/// Le destinataire, parce qu'un ordre n'est pas un souhait général : « pause »
/// ne veut rien dire pour celui qui ne joue pas, et deux appareils qui
/// s'appliqueraient le même ordre feraient deux fois le travail.
///
/// Le numéro, parce que le tableau est relu en boucle : sans lui, la même
/// pause serait appliquée à chaque lecture, et l'on ne pourrait plus jamais
/// repartir.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ordre {
    pub numero: u64,
    pub pour: String,
    pub action: Action,
    /// La seconde visée, pour un déplacement. Ignorée ailleurs.
    pub valeur: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Lecture,
    Pause,
    Suivant,
    Precedent,
    Position,
    /// « Je prends la main » : celui qui joue se tait, l'autre reprend.
    Rendre,
}

/// Ce qu'un appareil doit faire après avoir lu le tableau.
#[derive(Clone, Debug, PartialEq)]
pub enum Reaction {
    /// Rien à faire : le tableau ne me concerne pas.
    Rien,
    /// Un ordre m'est adressé, et je ne l'ai pas encore appliqué.
    Appliquer(Ordre),
    /// L'autre appareil tient le son : l'interface le montre.
    Montrer(EtatDistant),
}

/// Ce que l'interface affiche de l'autre appareil.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatDistant {
    pub appareil: String,
    pub titre: String,
    pub artiste: Option<String>,
    pub position_ms: i64,
    pub duree_ms: i64,
    pub en_lecture: bool,
}

/// Ce qu'un appareil fait d'un tableau qu'il vient de lire.
///
/// Fonction pure : c'est ici que vivraient les bugs, et c'est ici qu'on les
/// éprouve sans réseau, sans lecteur et sans horloge.
///
/// * `moi` — le nom de cet appareil, celui qu'un ordre peut viser ;
/// * `dernier_ordre` — le numéro du dernier ordre déjà appliqué.
pub fn reagir(tableau: &Tableau, moi: &str, dernier_ordre: u64) -> Reaction {
    if let Some(ordre) = &tableau.ordre {
        if ordre.pour == moi && ordre.numero > dernier_ordre {
            return Reaction::Appliquer(ordre.clone());
        }
    }

    let (Some(source), Some(titre)) = (&tableau.source, &tableau.titre) else {
        return Reaction::Rien;
    };

    // Ce que je joue moi-même, je le sais déjà : mon propre lecteur en est la
    // vérité, et le tableau n'en est qu'un écho vieux d'un aller-retour.
    if source == moi {
        return Reaction::Rien;
    }

    Reaction::Montrer(EtatDistant {
        appareil: source.clone(),
        titre: titre.clone(),
        artiste: tableau.artiste.clone(),
        position_ms: tableau.position_ms,
        duree_ms: tableau.duree_ms,
        en_lecture: tableau.en_lecture,
    })
}

/// Ce qu'un appareil publie quand il tient le son.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub appareil: String,
    pub titre: Option<String>,
    pub artiste: Option<String>,
    pub position_ms: i64,
    pub duree_ms: i64,
    pub en_lecture: bool,
    /// Faux quand cet appareil ne joue plus rien : il rend alors la place.
    pub tient_le_son: bool,
}

/// Applique une publication au tableau, et fait monter la version.
///
/// # Pourquoi celui qui ne joue pas ne peut pas effacer l'autre
///
/// Les deux appareils publient. Sans cette règle, celui qui est à l'arrêt
/// écraserait une seconde sur deux l'état de celui qui joue, et l'interface
/// clignoterait entre « en lecture » et « rien ».
pub fn appliquer_publication(tableau: &mut Tableau, publication: &Publication) -> bool {
    let source_actuelle = tableau.source.as_deref();

    if !publication.tient_le_son {
        // Je ne joue plus : je n'efface le tableau que s'il parlait de moi.
        if source_actuelle == Some(publication.appareil.as_str()) {
            tableau.source = None;
            tableau.titre = None;
            tableau.artiste = None;
            tableau.position_ms = 0;
            tableau.duree_ms = 0;
            tableau.en_lecture = false;
            tableau.version += 1;
            return true;
        }
        return false;
    }

    // # Pourquoi celui qui joue prime sur celui qui est en pause
    //
    // « Tenir le son » veut dire avoir un morceau chargé, pas forcément en
    // train de jouer : c'est ce qui permet à l'autre appareil d'afficher une
    // pause et de la relancer. Mais deux appareils arrêtés chacun sur un
    // morceau se disputeraient alors le tableau à chaque seconde, et
    // l'interface clignoterait entre les deux.
    if let Some(source) = &tableau.source {
        if source != &publication.appareil && tableau.en_lecture && !publication.en_lecture {
            return false;
        }
    }

    let suivant = Tableau {
        version: tableau.version,
        source: Some(publication.appareil.clone()),
        titre: publication.titre.clone(),
        artiste: publication.artiste.clone(),
        position_ms: publication.position_ms,
        duree_ms: publication.duree_ms,
        en_lecture: publication.en_lecture,
        ordre: tableau.ordre.clone(),
    };

    if suivant == *tableau {
        return false;
    }

    *tableau = suivant;
    tableau.version += 1;
    true
}

/// Dépose un ordre dans le tableau et fait monter la version.
pub fn deposer_ordre(tableau: &mut Tableau, pour: &str, action: Action, valeur: Option<i64>) {
    let numero = tableau.ordre.as_ref().map_or(0, |o| o.numero) + 1;
    tableau.ordre = Some(Ordre {
        numero,
        pour: pour.to_string(),
        action,
        valeur,
    });
    tableau.version += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tableau_du_mac() -> Tableau {
        Tableau {
            version: 4,
            source: Some("Mac de Carlos".into()),
            titre: Some("Sem Limite".into()),
            artiste: Some("Bispo".into()),
            position_ms: 25_000,
            duree_ms: 200_000,
            en_lecture: true,
            ordre: None,
        }
    }

    #[test]
    fn le_telephone_voit_ce_que_le_mac_joue() {
        let reaction = reagir(&tableau_du_mac(), "Téléphone", 0);
        let Reaction::Montrer(etat) = reaction else {
            panic!("attendu : montrer, obtenu {reaction:?}");
        };
        assert_eq!(etat.titre, "Sem Limite");
        assert!(etat.en_lecture);
    }

    /// Mon propre lecteur est la vérité sur ce que je joue ; le tableau n'en
    /// est qu'un écho vieux d'un aller-retour.
    #[test]
    fn on_ne_se_montre_pas_a_soi_meme() {
        assert_eq!(reagir(&tableau_du_mac(), "Mac de Carlos", 0), Reaction::Rien);
    }

    #[test]
    fn un_ordre_adresse_se_reconnait() {
        let mut tableau = tableau_du_mac();
        deposer_ordre(&mut tableau, "Mac de Carlos", Action::Pause, None);

        let Reaction::Appliquer(ordre) = reagir(&tableau, "Mac de Carlos", 0) else {
            panic!("le Mac doit appliquer");
        };
        assert_eq!(ordre.action, Action::Pause);
    }

    /// Sans le numéro, le tableau relu en boucle rejouerait la même pause à
    /// l'infini — on ne pourrait plus jamais repartir.
    #[test]
    fn un_ordre_deja_applique_ne_revient_pas() {
        let mut tableau = tableau_du_mac();
        deposer_ordre(&mut tableau, "Mac de Carlos", Action::Pause, None);
        let numero = tableau.ordre.as_ref().unwrap().numero;

        assert!(matches!(
            reagir(&tableau, "Mac de Carlos", numero),
            Reaction::Rien
        ));
    }

    #[test]
    fn un_ordre_pour_l_autre_ne_me_concerne_pas() {
        let mut tableau = tableau_du_mac();
        deposer_ordre(&mut tableau, "Téléphone", Action::Suivant, None);

        assert!(matches!(reagir(&tableau, "Mac de Carlos", 0), Reaction::Rien));
    }

    #[test]
    fn publier_fait_monter_la_version() {
        let mut tableau = Tableau::default();
        let change = appliquer_publication(
            &mut tableau,
            &Publication {
                appareil: "Mac".into(),
                titre: Some("A".into()),
                artiste: None,
                position_ms: 0,
                duree_ms: 1000,
                en_lecture: true,
                tient_le_son: true,
            },
        );

        assert!(change);
        assert_eq!(tableau.version, 1);
        assert_eq!(tableau.source.as_deref(), Some("Mac"));
    }

    /// Republier deux fois la même chose ne réveille personne : sans cette
    /// garde, chaque battement du cœur ferait un aller-retour réseau pour rien.
    #[test]
    fn republier_a_l_identique_ne_change_rien() {
        let mut tableau = Tableau::default();
        let publication = Publication {
            appareil: "Mac".into(),
            titre: Some("A".into()),
            artiste: None,
            position_ms: 0,
            duree_ms: 1000,
            en_lecture: true,
            tient_le_son: true,
        };

        assert!(appliquer_publication(&mut tableau, &publication));
        assert!(!appliquer_publication(&mut tableau, &publication));
        assert_eq!(tableau.version, 1);
    }

    /// Le défaut qu'on éviterait mal autrement : les deux appareils publient,
    /// et celui qui est à l'arrêt effacerait une seconde sur deux l'état de
    /// celui qui joue.
    #[test]
    fn celui_qui_ne_joue_pas_n_efface_pas_l_autre() {
        let mut tableau = tableau_du_mac();
        let avant = tableau.clone();

        let change = appliquer_publication(
            &mut tableau,
            &Publication {
                appareil: "Téléphone".into(),
                tient_le_son: false,
                ..Publication::default()
            },
        );

        assert!(!change);
        assert_eq!(tableau, avant);
    }

    #[test]
    fn celui_qui_jouait_peut_rendre_la_place() {
        let mut tableau = tableau_du_mac();
        let change = appliquer_publication(
            &mut tableau,
            &Publication {
                appareil: "Mac de Carlos".into(),
                tient_le_son: false,
                ..Publication::default()
            },
        );

        assert!(change);
        assert_eq!(tableau.source, None);
        assert!(!tableau.en_lecture);
    }

    /// Deux appareils arrêtés chacun sur un morceau se disputeraient le
    /// tableau à chaque seconde, et l'interface clignoterait entre les deux.
    #[test]
    fn celui_qui_joue_garde_le_tableau() {
        let mut tableau = tableau_du_mac();
        let avant = tableau.clone();

        let change = appliquer_publication(
            &mut tableau,
            &Publication {
                appareil: "Téléphone".into(),
                titre: Some("Autre chose".into()),
                en_lecture: false,
                tient_le_son: true,
                ..Publication::default()
            },
        );

        assert!(!change);
        assert_eq!(tableau, avant);
    }

    /// Mais dès que l'autre lance vraiment la musique, il prend la main : c'est
    /// ce qui fait qu'on peut reprendre l'écoute sur le téléphone.
    #[test]
    fn celui_qui_lance_prend_la_main() {
        let mut tableau = tableau_du_mac();

        let change = appliquer_publication(
            &mut tableau,
            &Publication {
                appareil: "Téléphone".into(),
                titre: Some("Autre chose".into()),
                en_lecture: true,
                tient_le_son: true,
                ..Publication::default()
            },
        );

        assert!(change);
        assert_eq!(tableau.source.as_deref(), Some("Téléphone"));
    }

    #[test]
    fn les_numeros_d_ordre_se_suivent() {
        let mut tableau = Tableau::default();
        deposer_ordre(&mut tableau, "Mac", Action::Pause, None);
        deposer_ordre(&mut tableau, "Mac", Action::Lecture, None);
        deposer_ordre(&mut tableau, "Mac", Action::Position, Some(42_000));

        let ordre = tableau.ordre.unwrap();
        assert_eq!(ordre.numero, 3);
        assert_eq!(ordre.valeur, Some(42_000));
    }
}
