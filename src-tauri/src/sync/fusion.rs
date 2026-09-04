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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MorceauSync {
    /// Chemin relatif à la racine de bibliothèque. Première clé d'appariement.
    pub chemin: String,
    pub titre: String,
    pub artiste: Option<String>,
    pub album: Option<String>,
    pub duree_ms: i64,
    /// Taille du fichier, en octets. Sert à annoncer ce que coûterait de
    /// récupérer les morceaux qui manquent ici.
    #[serde(default)]
    pub taille: i64,
    pub aime: bool,
    /// Quand le favori a **changé d'état**. Absent pour tout ce qui date
    /// d'avant la synchronisation.
    pub aime_le: Option<i64>,
    pub paroles: Option<String>,
    pub paroles_synchronisees: bool,
    /// Le fichier est-il là ?
    ///
    /// Un morceau **hors ligne** garde sa fiche, ses favoris et sa place dans
    /// les playlists : il participe donc pleinement à la fusion. Mais il n'a
    /// plus de fichier à donner, et l'annoncer comme manquant chez l'autre
    /// revient à promettre ce qu'on ne peut pas tenir — trois morceaux du
    /// premier transfert réel ont échoué pour cette raison.
    #[serde(default = "vrai")]
    pub disponible: bool,
}

/// Un état venu d'une version qui ignorait la disponibilité parle de morceaux
/// présents : c'est ce qu'ils étaient tous avant que la question se pose.
fn vrai() -> bool {
    true
}

/// # Pourquoi `Default` est écrit à la main
///
/// Le dérivé donnerait `disponible: false`, c'est-à-dire « hors ligne » — le
/// contraire de ce qu'un morceau est par défaut. Un test qui construit un
/// morceau sans y penser en obtiendrait un fantôme, et vérifierait autre chose
/// que ce qu'il croit.
impl Default for MorceauSync {
    fn default() -> Self {
        Self {
            chemin: String::new(),
            titre: String::new(),
            artiste: None,
            album: None,
            duree_ms: 0,
            taille: 0,
            aime: false,
            aime_le: None,
            paroles: None,
            paroles_synchronisees: false,
            disponible: true,
        }
    }
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

/// Ce qu'un appareil était en train d'écouter.
///
/// # Pourquoi la file voyage en entier
///
/// « Reprendre où j'en étais » ne veut pas dire « rejoue ce morceau » : cela
/// veut dire retrouver la suite. Sans la file, on reprend un titre isolé et
/// l'écoute s'arrête à sa fin, ce qui est plus déroutant que de ne rien
/// reprendre du tout. Quelques milliers de chemins font une centaine de
/// kilo-octets, à côté des six mégaoctets de paroles déjà échangés.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LectureSync {
    /// La file, par chemins relatifs **de l'émetteur**.
    pub file: Vec<String>,
    /// Place du morceau en cours dans cette file.
    pub position: usize,
    pub position_ms: i64,
    /// Quand cet appareil a joué pour la dernière fois.
    pub quand: i64,
    pub titre: String,
    pub artiste: Option<String>,
}

/// Une écoute qu'on peut reprendre ici.
///
/// # Pourquoi c'est une proposition et non une application
///
/// Prendre la main sur le son de quelqu'un qui écoute déjà est le geste le
/// plus brutal qu'un lecteur puisse faire. La synchronisation dit ce que
/// l'autre appareil écoutait ; c'est un clic qui décide de reprendre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reprise {
    pub appareil: String,
    pub titre: String,
    pub artiste: Option<String>,
    pub quand: i64,
    pub position_ms: i64,
    /// La file, traduite en **nos** chemins.
    pub file: Vec<String>,
    pub position: usize,
}

/// Tout ce qu'un appareil expose à l'autre.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatSync {
    /// Le nom que l'appareil se donne — « MacBook de Carlos », « Honor ».
    pub appareil: String,
    pub morceaux: Vec<MorceauSync>,
    pub playlists: Vec<PlaylistSync>,
    /// Ce que cet appareil écoutait. Absent s'il n'a jamais rien joué.
    #[serde(default)]
    pub lecture: Option<LectureSync>,
}

/// Ce qu'on accepte de faire traverser.
///
/// # Pourquoi ce n'est pas tout ou rien
///
/// Une synchronisation complète est ce qu'on veut la plupart du temps. Mais
/// « la plupart du temps » n'est pas « toujours » : on rentre de vacances avec
/// deux cents morceaux sur le téléphone et l'on ne veut que ceux-là, ou l'on
/// veut ses favoris sans toucher aux playlists qu'on est en train de refaire.
/// Sans réglage, il faut alors tout prendre ou ne rien prendre.
///
/// # Pourquoi elle voyage avec la demande
///
/// Les deux appareils fusionnent, chacun de son côté. Si seul celui qui
/// demande respectait la portée, l'autre appliquerait quand même tout — et l'on
/// se retrouverait avec deux bibliothèques qui ne disent pas la même chose,
/// ce que la synchronisation est censée empêcher.
///
/// Elle est `default` à la désérialisation : un appareil qui n'a pas encore la
/// mise à jour n'envoie rien, et « rien » veut dire « tout », qui est le
/// comportement qu'il connaît.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Portee {
    pub favoris: bool,
    pub playlists: bool,
    /// Les fichiers qui manquent d'un côté.
    pub morceaux: bool,
    /// Les paroles, et la reprise d'écoute.
    pub autre: bool,
    /// Quand `morceaux` est vrai : n'accepter que ceux de cet artiste.
    ///
    /// Comparé sans accents ni casse — on tape « nepal », pas « Népal ».
    #[serde(default)]
    pub artiste: Option<String>,
    /// Et, plus précis encore : n'accepter que ce titre-là.
    ///
    /// # Pourquoi il s'ajoute à l'artiste au lieu de le remplacer
    ///
    /// « Un seul morceau, celui-là » est une demande courante — on rentre avec
    /// un titre qu'on veut donner, et pas les quatre-vingts autres. L'artiste
    /// seul ne sait pas la formuler.
    ///
    /// Les deux se cumulent : un titre porté par deux artistes différents se
    /// départage en nommant l'artiste, et l'on n'a jamais à choisir entre
    /// préciser trop et pas assez.
    #[serde(default)]
    pub titre: Option<String>,
}

impl Default for Portee {
    /// Tout, parce que c'est ce qu'on veut la plupart du temps et parce qu'un
    /// appareil qui ne dit rien doit se comporter comme avant.
    fn default() -> Self {
        Self {
            favoris: true,
            playlists: true,
            morceaux: true,
            autre: true,
            artiste: None,
            titre: None,
        }
    }
}

impl Portee {
    /// Ce morceau entre-t-il dans la portée ?
    fn accepte(&self, morceau: &MorceauSync) -> bool {
        correspond(self.artiste.as_deref(), morceau.artiste.as_deref())
            && correspond(self.titre.as_deref(), Some(morceau.titre.as_str()))
    }
}

/// Le champ saisi désigne-t-il cette valeur ?
///
/// Un champ vide ne veut pas dire « aucun » : il veut dire qu'on n'a rien
/// précisé, et tout passe.
fn correspond(saisi: Option<&str>, valeur: Option<&str>) -> bool {
    let Some(vise) = saisi else {
        return true;
    };

    let vise = aplatir(vise);
    if vise.is_empty() {
        return true;
    }

    valeur.is_some_and(|valeur| aplatir(valeur).contains(&vise))
}

/// Minuscules, sans accents : « Népal » et « nepal » désignent le même artiste.
fn aplatir(valeur: &str) -> String {
    valeur
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            autre => autre,
        })
        .collect()
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

/// Un morceau que l'autre appareil possède et que nous n'avons pas.
///
/// # Pourquoi ils ne sont pas rapatriés d'office
///
/// Ce sont des fichiers : quelques mégaoctets chacun, plusieurs gigaoctets
/// pour une bibliothèque entière. Les faire descendre sans prévenir remplirait
/// un téléphone en silence. Ils sont donc **annoncés**, avec leur poids, et
/// c'est un clic qui les fait venir.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manquant {
    /// Le chemin **chez l'autre** : c'est par lui qu'on le demandera.
    pub chemin: String,
    pub titre: String,
    pub artiste: Option<String>,
    pub taille: i64,
}

/// Le résultat d'une fusion, du point de vue de celui qui l'a demandée.
#[derive(Debug, Default)]
pub struct Fusion {
    pub changements: Vec<Changement>,
    pub arbitrages: Vec<Arbitrage>,
    /// Ce que l'autre a et que nous n'avons pas.
    pub manquants: Vec<Manquant>,
    /// L'écoute de l'autre, quand elle est plus récente que la nôtre.
    pub reprise: Option<Reprise>,
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
        "a\u{1}{}\u{1}{}\u{1}{}",
        normaliser(artiste),
        normaliser(&morceau.titre),
        morceau.duree_ms / 1000,
    )
}

/// La troisième clé : le titre et la durée, **sans l'artiste**.
///
/// # Pourquoi renoncer à l'artiste
///
/// Mesuré sur les deux bibliothèques : cent quatre morceaux que le téléphone
/// croyait ne pas avoir, et qu'il avait. « Brand New Draco [ChopNotSlop
/// Remix] » est crédité à 21 Savage d'un côté, à Metro Boomin de l'autre —
/// l'un et l'autre ont raison, c'est un disque à deux noms. Le chemin en
/// découle et diffère donc aussi, si bien qu'aucune des deux premières clés ne
/// les rapproche. Sans cette troisième, on retéléchargeait 1,37 Go pour en
/// jeter la moitié comme doublons.
///
/// C'est la clé la plus permissive des trois, et elle est essayée en dernier :
/// deux morceaux différents portant le même titre à la même seconde près
/// existent. L'exigence d'unicité **des deux côtés** les écarte — s'il y a le
/// moindre doute sur qui est qui, la clé ne désigne personne.
fn cle_titre(morceau: &MorceauSync) -> String {
    format!(
        "t\u{1}{}\u{1}{}",
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

/// Index des morceaux distants par une clé, **quand elle ne désigne qu'eux**.
///
/// L'unicité est exigée des deux côtés : si deux morceaux distants partagent la
/// clé, elle ne désigne personne ; et si deux morceaux locaux la partagent,
/// l'un des deux se ferait apparier à tort. Apparier au hasard serait pire que
/// ne pas apparier — le favori d'un morceau atterrirait sur un autre.
fn index_sans_ambiguite<'a>(
    locaux: &[MorceauSync],
    distants: &'a [MorceauSync],
    cle: fn(&MorceauSync) -> String,
) -> HashMap<String, &'a MorceauSync> {
    let mut ici: HashMap<String, usize> = HashMap::new();
    for morceau in locaux {
        *ici.entry(cle(morceau)).or_default() += 1;
    }

    let mut la_bas: HashMap<String, usize> = HashMap::new();
    for morceau in distants {
        *la_bas.entry(cle(morceau)).or_default() += 1;
    }

    distants
        .iter()
        .filter(|morceau| {
            let valeur = cle(morceau);
            la_bas.get(&valeur) == Some(&1) && ici.get(&valeur).copied().unwrap_or(0) <= 1
        })
        .map(|morceau| (cle(morceau), morceau))
        .collect()
}

/// Table d'appariement : pour chaque morceau local, son homologue distant.
///
/// Trois clés, de la plus sûre à la plus permissive. La première qui répond
/// gagne : le chemin, puis artiste + titre + durée, puis titre + durée.
fn apparier<'a>(
    locaux: &'a [MorceauSync],
    distants: &'a [MorceauSync],
    alias: &HashMap<String, String>,
) -> Vec<(&'a MorceauSync, &'a MorceauSync)> {
    let par_chemin: HashMap<&str, &MorceauSync> = distants
        .iter()
        .map(|morceau| (morceau.chemin.as_str(), morceau))
        .collect();

    // L'alias va du chemin distant vers le nôtre ; l'appariement part du
    // nôtre. On le retourne une fois plutôt que de le parcourir à chaque ligne.
    let par_alias: HashMap<&str, &MorceauSync> = alias
        .iter()
        .filter_map(|(distant, local)| {
            par_chemin.get(distant.as_str()).map(|trouve| (local.as_str(), *trouve))
        })
        .collect();

    let par_tags = index_sans_ambiguite(locaux, distants, cle_secondaire);
    let par_titre = index_sans_ambiguite(locaux, distants, cle_titre);

    locaux
        .iter()
        .filter_map(|local| {
            // L'alias passe devant : c'est un fait constaté en lisant le
            // contenu des fichiers, là où les autres clés sont des indices.
            let distant = par_alias
                .get(local.chemin.as_str())
                .copied()
                .or_else(|| par_chemin.get(local.chemin.as_str()).copied())
                .or_else(|| par_tags.get(&cle_secondaire(local)).copied())
                .or_else(|| par_titre.get(&cle_titre(local)).copied())?;

            Some((local, distant))
        })
        .collect()
}

// ════════════════════════════════════════════════════════════════════════════
//  Fusion
// ════════════════════════════════════════════════════════════════════════════

/// Ce que **cet** appareil doit appliquer pour rejoindre l'autre.
///
/// `alias` associe un chemin **distant** au nôtre, pour les morceaux qu'aucune
/// clé ne rapproche et que l'import a pourtant reconnus en lisant leur
/// contenu. Sans lui, ces morceaux-là repartaient en téléchargement à chaque
/// synchronisation, pour être jetés à l'arrivée.
pub fn fusionner(
    soi: &EtatSync,
    autre: &EtatSync,
    alias: &HashMap<String, String>,
    portee: &Portee,
) -> Fusion {
    let mut fusion = Fusion::default();
    let paires = apparier(&soi.morceaux, &autre.morceaux, alias);

    for (local, distant) in &paires {
        // Le filtre par artiste porte sur **notre** fiche : c'est celle qu'on
        // modifie, et c'est son artiste qu'on a saisi.
        if !portee.accepte(local) {
            continue;
        }

        if portee.favoris {
            fusionner_favori(local, distant, &autre.appareil, &mut fusion);
        }
        if portee.autre {
            fusionner_paroles(local, distant, &mut fusion);
        }
    }

    if portee.playlists {
        fusionner_playlists(soi, autre, &paires, &mut fusion);
    }

    if portee.morceaux {
        fusion.manquants = manquants(&paires, autre)
            .into_iter()
            .filter(|manquant| {
                correspond(portee.artiste.as_deref(), manquant.artiste.as_deref())
                    && correspond(portee.titre.as_deref(), Some(manquant.titre.as_str()))
            })
            .collect();
    }

    if portee.autre {
        fusion.reprise = reprise(soi, autre, &paires);
    }

    fusion
}

/// L'écoute de l'autre, si elle est plus récente que la nôtre.
///
/// # Pourquoi la file est traduite et non transmise telle quelle
///
/// Elle est faite des chemins de l'autre appareil. Les rejouer ici
/// désignerait des fichiers qui n'existent pas — les deux bibliothèques
/// rangent les mêmes morceaux sous des noms qui ne concordent pas toujours.
/// Ce qu'on ne sait pas traduire est simplement laissé de côté, et la place du
/// morceau en cours suit ce qui a disparu devant lui.
fn reprise(
    soi: &EtatSync,
    autre: &EtatSync,
    paires: &[(&MorceauSync, &MorceauSync)],
) -> Option<Reprise> {
    let la_bas = autre.lecture.as_ref()?;

    // Une écoute plus ancienne que la nôtre n'a rien à proposer : c'est nous
    // qui sommes en avance.
    if soi.lecture.as_ref().is_some_and(|ici| ici.quand >= la_bas.quand) {
        return None;
    }

    let traduire: HashMap<&str, &str> = paires
        .iter()
        .map(|(local, distant)| (distant.chemin.as_str(), local.chemin.as_str()))
        .collect();

    let mut file = Vec::with_capacity(la_bas.file.len());
    let mut position = 0;

    for (rang, chemin) in la_bas.file.iter().enumerate() {
        let Some(ici) = traduire.get(chemin.as_str()) else {
            continue;
        };

        if rang <= la_bas.position {
            position = file.len();
        }

        file.push((*ici).to_string());
    }

    if file.is_empty() {
        return None;
    }

    Some(Reprise {
        appareil: autre.appareil.clone(),
        titre: la_bas.titre.clone(),
        artiste: la_bas.artiste.clone(),
        quand: la_bas.quand,
        position_ms: la_bas.position_ms,
        file,
        position,
    })
}

/// Les morceaux de l'autre qu'aucun des nôtres ne rejoint.
///
/// # Pourquoi c'est le complément de l'appariement et non une comparaison
///
/// « Manquant » n'a de sens qu'au regard de la règle qui décide que deux
/// fichiers sont le même morceau. La recalculer autrement ici garantirait
/// qu'un jour les deux réponses divergent : un morceau annoncé manquant que la
/// fusion, elle, considère présent — et qu'on téléchargerait en double.
fn manquants(paires: &[(&MorceauSync, &MorceauSync)], autre: &EtatSync) -> Vec<Manquant> {
    let apparies: std::collections::HashSet<&str> = paires
        .iter()
        .map(|(_, distant)| distant.chemin.as_str())
        .collect();

    autre
        .morceaux
        .iter()
        .filter(|morceau| !apparies.contains(morceau.chemin.as_str()))
        // Un morceau dont l'autre appareil a perdu le fichier ne peut pas
        // être donné. L'annoncer, c'est promettre un téléchargement qui
        // échouera — et faire passer pour une panne un cas parfaitement normal.
        .filter(|morceau| morceau.disponible)
        .map(|morceau| Manquant {
            chemin: morceau.chemin.clone(),
            titre: morceau.titre.clone(),
            artiste: morceau.artiste.clone(),
            taille: morceau.taille,
        })
        .collect()
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
            lecture: None,
        }
    }

    // ── La portée ───────────────────────────────────────────────────────

    /// Le défaut doit rester « tout » : un appareil qui ne dit rien se comporte
    /// comme avant, et c'est ce qui permet aux deux versions de coexister.
    #[test]
    fn la_portee_par_defaut_prend_tout() {
        let portee = Portee::default();
        assert!(portee.favoris && portee.playlists && portee.morceaux && portee.autre);
        assert_eq!(portee.artiste, None);
    }

    #[test]
    fn sans_les_favoris_aucun_coeur_ne_traverse() {
        let mac = etat("Mac", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", true)]);

        let portee = Portee {
            favoris: false,
            ..Portee::default()
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert!(fusion.changements.is_empty(), "{:?}", fusion.changements);
    }

    #[test]
    fn sans_les_morceaux_il_n_y_a_rien_a_rapatrier() {
        let mac = etat("Mac", vec![]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);

        let portee = Portee {
            morceaux: false,
            ..Portee::default()
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert!(fusion.manquants.is_empty());
    }

    /// Le cas qui motive tout : on rentre avec deux cents morceaux d'un seul
    /// artiste et l'on ne veut que ceux-là.
    #[test]
    fn un_artiste_vise_ecarte_les_autres() {
        let mac = etat("Mac", vec![]);
        let tel = etat(
            "Honor",
            vec![
                morceau("n/adios.mp3", "Adios Bahamas", "Népal", false),
                morceau("d/macarena.mp3", "Macarena", "Damso", false),
            ],
        );

        let portee = Portee {
            artiste: Some("Népal".into()),
            ..Portee::default()
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert_eq!(fusion.manquants.len(), 1);
        assert_eq!(fusion.manquants[0].titre, "Adios Bahamas");
    }

    /// Le cas le plus précis : un seul morceau, celui-là. On rentre avec un
    /// titre qu'on veut donner, et pas les quatre-vingts autres.
    #[test]
    fn un_titre_vise_ne_laisse_passer_que_lui() {
        let mac = etat("Mac", vec![]);
        let tel = etat(
            "Honor",
            vec![
                morceau("n/adios.mp3", "Adios Bahamas", "Népal", false),
                morceau("n/2am.mp3", "2 AM", "Népal", false),
            ],
        );

        let portee = Portee {
            titre: Some("Adios".into()),
            ..Portee::default()
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert_eq!(fusion.manquants.len(), 1);
        assert_eq!(fusion.manquants[0].titre, "Adios Bahamas");
    }

    /// Les deux se cumulent : un titre porté par deux artistes se départage en
    /// nommant l'artiste.
    #[test]
    fn l_artiste_et_le_titre_se_cumulent() {
        let mac = etat("Mac", vec![]);
        let tel = etat(
            "Honor",
            vec![
                morceau("a/intro.mp3", "Intro", "Damso", false),
                morceau("b/intro.mp3", "Intro", "Népal", false),
            ],
        );

        let portee = Portee {
            artiste: Some("nepal".into()),
            titre: Some("intro".into()),
            ..Portee::default()
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert_eq!(fusion.manquants.len(), 1);
        assert_eq!(fusion.manquants[0].chemin, "b/intro.mp3");
    }

    /// On tape « nepal », pas « Népal » : exiger la typographie exacte ne sert
    /// qu'à celui qui connaît déjà la réponse.
    #[test]
    fn l_artiste_se_reconnait_sans_accent_ni_casse() {
        let mac = etat("Mac", vec![]);
        let tel = etat("Honor", vec![morceau("n/adios.mp3", "Adios", "Népal", false)]);

        for saisi in ["nepal", "NÉPAL", "népal", "Nepa"] {
            let portee = Portee {
                artiste: Some(saisi.into()),
                ..Portee::default()
            };
            let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);
            assert_eq!(fusion.manquants.len(), 1, "saisi : {saisi}");
        }
    }

    /// Un champ laissé vide ne veut pas dire « aucun artiste » : il veut dire
    /// qu'on n'a rien précisé.
    #[test]
    fn un_artiste_vide_ne_filtre_rien() {
        let mac = etat("Mac", vec![]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);

        let portee = Portee {
            artiste: Some("   ".into()),
            ..Portee::default()
        };
        // Les espaces sont aplatis, pas rognés : c'est la comparaison qui doit
        // les ignorer.
        let portee = Portee {
            artiste: portee.artiste.map(|a| a.trim().to_string()),
            ..portee
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert_eq!(fusion.manquants.len(), 1);
    }

    /// Le favori d'un morceau qui n'est pas de l'artiste visé ne traverse pas
    /// non plus : la portée vaut pour tout, pas seulement pour les fichiers.
    #[test]
    fn l_artiste_vise_filtre_aussi_les_favoris() {
        let mac = etat(
            "Mac",
            vec![
                morceau("n/adios.mp3", "Adios", "Népal", false),
                morceau("d/macarena.mp3", "Macarena", "Damso", false),
            ],
        );
        let tel = etat(
            "Honor",
            vec![
                morceau("n/adios.mp3", "Adios", "Népal", true),
                morceau("d/macarena.mp3", "Macarena", "Damso", true),
            ],
        );

        let portee = Portee {
            artiste: Some("nepal".into()),
            ..Portee::default()
        };
        let fusion = fusionner(&mac, &tel, &HashMap::new(), &portee);

        assert_eq!(fusion.changements.len(), 1, "{:?}", fusion.changements);
        assert!(matches!(
            &fusion.changements[0],
            Changement::Aime { chemin, .. } if chemin == "n/adios.mp3"
        ));
    }

    #[test]
    fn sans_date_les_favoris_se_reunissent() {
        // C'est le cas de la toute première synchronisation : personne n'a
        // d'horodatage, et pourtant les cœurs des deux côtés doivent survivre.
        let mac = etat("Mac", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", true)]);

        let vers_le_mac = fusionner(&mac, &tel, &HashMap::new(), &Portee::default());
        assert_eq!(
            vers_le_mac.changements,
            vec![Changement::Aime {
                chemin: "a.mp3".to_string(),
                aime: true,
                quand: None,
            }]
        );

        // Et l'inverse ne retire rien : le téléphone garde son favori.
        let vers_le_tel = fusionner(&tel, &mac, &HashMap::new(), &Portee::default());
        assert!(vers_le_tel.changements.is_empty());
    }

    #[test]
    fn la_date_la_plus_recente_tranche() {
        let mut ici = morceau("a.mp3", "Rêve", "Adèle", true);
        ici.aime_le = Some(100);

        let mut la_bas = morceau("a.mp3", "Rêve", "Adèle", false);
        la_bas.aime_le = Some(200);

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]), &HashMap::new(), &Portee::default());

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

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]), &HashMap::new(), &Portee::default());

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

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]), &HashMap::new(), &Portee::default());

        assert_eq!(fusion.changements.len(), 1, "les tags doivent apparier");
    }

    #[test]
    fn un_meme_morceau_credite_a_deux_artistes_se_retrouve() {
        // Mesuré sur les vraies bibliothèques : le même disque est crédité à
        // 21 Savage d'un côté, à Metro Boomin de l'autre. Le chemin en découle
        // et diffère donc aussi.
        let ici = morceau(
            "21 Savage/2020 - SAVAGE MODE II/12 - Brand New Draco.mp3",
            "Brand New Draco",
            "21 Savage",
            false,
        );
        let la_bas = morceau(
            "Metro Boomin/2020 - SAVAGE MODE II/12 - Brand New Draco.mp3",
            "Brand New Draco",
            "Metro Boomin",
            true,
        );

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![la_bas]), &HashMap::new(), &Portee::default());

        assert_eq!(fusion.changements.len(), 1, "le favori doit traverser");
        assert!(
            fusion.manquants.is_empty(),
            "le morceau est là : le retélécharger le mettrait en double"
        );
    }

    #[test]
    fn deux_titres_homonymes_ne_s_apparient_pas_par_le_titre() {
        // Deux « Intro » de la même durée : la clé du titre ne peut pas les
        // départager, et deviner serait pire que renoncer.
        let ici_un = morceau("a/intro.mp3", "Intro", "A", false);
        let ici_deux = morceau("b/intro.mp3", "Intro", "B", false);
        let la_bas = morceau("c/intro.mp3", "Intro", "C", true);

        let fusion = fusionner(
            &etat("Mac", vec![ici_un, ici_deux]),
            &etat("Honor", vec![la_bas]),
            &HashMap::new(),
            &Portee::default(),
        );

        assert!(
            fusion.changements.is_empty(),
            "aucun des deux homonymes ne doit hériter du favori de l'autre"
        );
    }

    #[test]
    fn une_cle_ambigue_n_apparie_personne() {
        // Deux morceaux distants portant les mêmes tags : apparier au hasard
        // serait pire que ne pas apparier.
        let ici = morceau("live.mp3", "Rêve", "Adèle", false);
        let studio = morceau("a.mp3", "Rêve", "Adèle", true);
        let live = morceau("b.mp3", "Rêve", "Adèle", true);

        let fusion = fusionner(&etat("Mac", vec![ici]), &etat("Honor", vec![studio, live]), &HashMap::new(), &Portee::default());

        assert!(fusion.changements.is_empty());
    }

    #[test]
    fn les_paroles_arrivent_mais_n_ecrasent_pas() {
        let mut nu = morceau("a.mp3", "Rêve", "Adèle", false);
        let mut avec = morceau("a.mp3", "Rêve", "Adèle", false);
        avec.paroles = Some("le texte".to_string());

        // Rien → quelque chose : on prend.
        let fusion = fusionner(&etat("Mac", vec![nu.clone()]), &etat("Honor", vec![avec.clone()]), &HashMap::new(), &Portee::default());
        assert_eq!(fusion.changements.len(), 1);

        // Quelque chose → autre chose, sans synchronisation : on garde le nôtre.
        nu.paroles = Some("mon texte".to_string());
        let fusion = fusionner(&etat("Mac", vec![nu.clone()]), &etat("Honor", vec![avec.clone()]), &HashMap::new(), &Portee::default());
        assert!(fusion.changements.is_empty());

        // Mais un texte qui défile l'emporte sur un texte fixe.
        let mut calees = avec.clone();
        calees.paroles_synchronisees = true;
        let fusion = fusionner(&etat("Mac", vec![nu]), &etat("Honor", vec![calees]), &HashMap::new(), &Portee::default());
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

        let fusion = fusionner(&etat("Mac", vec![ici]), &tel, &HashMap::new(), &Portee::default());

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

        let fusion = fusionner(&mac, &tel, &HashMap::new(), &Portee::default());

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

        assert!(fusionner(&mac, &tel, &HashMap::new(), &Portee::default()).changements.is_empty());
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

        let fusion = fusionner(&mac, &tel, &HashMap::new(), &Portee::default());
        assert_eq!(fusion.changements.len(), 1, "« Nouvelle » doit être créée");
    }

    #[test]
    fn un_morceau_absent_ici_est_annonce() {
        // Le défaut observé : « tout est bon » alors que le Mac venait de
        // télécharger des morceaux que le téléphone n'a pas.
        let mac = etat("Mac", vec![morceau("un.mp3", "Un", "A", false)]);

        let mut nouveau = morceau("deux.mp3", "Deux", "B", false);
        nouveau.taille = 4_000_000;
        let tel = etat(
            "Honor",
            vec![morceau("un.mp3", "Un", "A", false), nouveau],
        );

        let fusion = fusionner(&mac, &tel, &HashMap::new(), &Portee::default());

        assert_eq!(fusion.manquants.len(), 1);
        assert_eq!(fusion.manquants[0].chemin, "deux.mp3");
        assert_eq!(fusion.manquants[0].taille, 4_000_000);
    }

    #[test]
    fn la_reprise_traduit_la_file_et_ajuste_la_place() {
        // Le Mac écoutait le troisième morceau d'une file de quatre. Le
        // téléphone n'en a que trois — celui qui manque est **avant** le
        // morceau en cours, donc la place recule d'un cran.
        let ici = vec![
            morceau("un.mp3", "Un", "A", false),
            morceau("trois.mp3", "Trois", "C", false),
            morceau("quatre.mp3", "Quatre", "D", false),
        ];
        let la_bas = vec![
            morceau("un.mp3", "Un", "A", false),
            morceau("deux.mp3", "Deux", "B", false),
            morceau("trois.mp3", "Trois", "C", false),
            morceau("quatre.mp3", "Quatre", "D", false),
        ];

        let mut tel = etat("Honor", ici);
        tel.lecture = None;

        let mut mac = etat("Mac", la_bas);
        mac.lecture = Some(LectureSync {
            file: vec![
                "un.mp3".into(),
                "deux.mp3".into(),
                "trois.mp3".into(),
                "quatre.mp3".into(),
            ],
            position: 2,
            position_ms: 42_000,
            quand: 1_000,
            titre: "Trois".into(),
            artiste: Some("C".into()),
        });

        let reprise = fusionner(&tel, &mac, &HashMap::new(), &Portee::default()).reprise.unwrap();

        assert_eq!(reprise.file, vec!["un.mp3", "trois.mp3", "quatre.mp3"]);
        assert_eq!(reprise.position, 1, "le morceau absent devant a décalé la place");
        assert_eq!(reprise.position_ms, 42_000);
    }

    #[test]
    fn une_ecoute_plus_ancienne_ne_se_propose_pas() {
        // Celui qui a écouté en dernier n'a rien à reprendre de l'autre.
        let un = morceau("un.mp3", "Un", "A", false);

        let mut tel = etat("Honor", vec![un.clone()]);
        tel.lecture = Some(LectureSync {
            file: vec!["un.mp3".into()],
            position: 0,
            position_ms: 0,
            quand: 5_000,
            titre: "Un".into(),
            artiste: None,
        });

        let mut mac = etat("Mac", vec![un]);
        mac.lecture = Some(LectureSync {
            file: vec!["un.mp3".into()],
            position: 0,
            position_ms: 0,
            quand: 1_000,
            titre: "Un".into(),
            artiste: None,
        });

        assert!(fusionner(&tel, &mac, &HashMap::new(), &Portee::default()).reprise.is_none());
    }

    #[test]
    fn un_alias_appris_evite_de_retelecharger() {
        // Le cas réel : onze morceaux revenaient à chaque synchronisation.
        // Ni le chemin ni les tags ne les rapprochent — seul l'import les
        // reconnaît, en lisant leur contenu. Il le dit une fois, la fusion
        // s'en souvient.
        let ici = morceau("Rangé/ici.mp3", "Un titre", "Un artiste", false);
        let la_bas = morceau("Rangé/autrement.mp3", "Autre titre", "Autre artiste", true);

        let mac = etat("Mac", vec![ici]);
        let tel = etat("Honor", vec![la_bas]);

        // Sans alias : le morceau paraît manquant, et son favori ne traverse pas.
        let sans = fusionner(&mac, &tel, &HashMap::new(), &Portee::default());
        assert_eq!(sans.manquants.len(), 1);
        assert!(sans.changements.is_empty());

        // Avec : il est reconnu, et tout suit.
        let alias = HashMap::from([(
            "Rangé/autrement.mp3".to_string(),
            "Rangé/ici.mp3".to_string(),
        )]);
        let avec = fusionner(&mac, &tel, &alias, &Portee::default());

        assert!(avec.manquants.is_empty(), "il est là : ne pas le redemander");
        assert_eq!(avec.changements.len(), 1, "le favori doit traverser");
    }

    #[test]
    fn un_morceau_hors_ligne_chez_l_autre_n_est_pas_propose() {
        // Trois morceaux ont échoué au premier transfert réel pour cette
        // raison : leur fiche existait chez l'expéditeur, leur fichier non.
        let mac = etat("Mac", vec![morceau("un.mp3", "Un", "A", false)]);

        let mut fantome = morceau("deux.mp3", "Deux", "B", false);
        fantome.disponible = false;
        let tel = etat("Honor", vec![morceau("un.mp3", "Un", "A", false), fantome]);

        assert!(
            fusionner(&mac, &tel, &HashMap::new(), &Portee::default()).manquants.is_empty(),
            "on ne propose pas un fichier que l'autre n'a plus"
        );
    }

    #[test]
    fn un_morceau_range_autrement_n_est_pas_manquant() {
        // Il est là, sous un autre chemin : le télécharger le mettrait en
        // double. C'est l'appariement qui tranche, pas une comparaison à part.
        let mac = etat(
            "Mac",
            vec![morceau("Adele/2024 - Inversions/03 - Reve.mp3", "Rêve", "Adèle", false)],
        );
        let tel = etat("Honor", vec![morceau("Musique/Reve.mp3", "reve", "ADELE", false)]);

        assert!(fusionner(&mac, &tel, &HashMap::new(), &Portee::default()).manquants.is_empty());
    }

    #[test]
    fn la_fusion_est_stable() {
        // Appliquer, puis refusionner, ne doit plus rien produire : sans quoi
        // deux appareils se renverraient indéfiniment les mêmes changements.
        let mut mac = etat("Mac", vec![morceau("a.mp3", "Rêve", "Adèle", false)]);
        let tel = etat("Honor", vec![morceau("a.mp3", "Rêve", "Adèle", true)]);

        assert_eq!(fusionner(&mac, &tel, &HashMap::new(), &Portee::default()).changements.len(), 1);

        // On applique.
        mac.morceaux[0].aime = true;
        assert!(fusionner(&mac, &tel, &HashMap::new(), &Portee::default()).changements.is_empty());
    }
}
