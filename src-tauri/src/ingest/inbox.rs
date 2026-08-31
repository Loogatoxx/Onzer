//! Dossier surveillé.
//!
//! Un fichier déposé dans `<bibliothèque>/_Inbox` est importé automatiquement.
//!
//! # Pourquoi un scrutin, et pas `notify`
//!
//! La surveillance d'événements du système de fichiers paraît plus élégante,
//! mais elle échoue précisément sur ce cas d'usage :
//!
//! | | `notify` | Scrutin |
//! |---|---|---|
//! | Fichier déposé application fermée | ❌ Événement perdu à jamais | ✅ Vu au démarrage suivant |
//! | Téléchargement en cours | ❌ Événement dès la création, fichier incomplet | ✅ Attend la stabilisation |
//! | exFAT | Support inégal des événements | ✅ Indifférent |
//! | Coût | — | Un listage de dossier toutes les 2 s : négligeable |
//!
//! Le second point est le plus important : `yt-dlp` crée le fichier **avant**
//! de le remplir. Importer sur l'événement de création reviendrait à avaler un
//! MP3 tronqué, à le taguer de travers et à le ranger définitivement.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::library::importer;
use crate::library::naming::INBOX_DIR;

/// Intervalle entre deux inspections du dossier.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Durée pendant laquelle la taille d'un fichier doit rester inchangée avant
/// qu'il soit considéré comme complet.
///
/// Trois secondes couvrent une pause réseau passagère sans faire attendre
/// inutilement. Un téléchargement momentanément figé sera simplement importé au
/// tour suivant.
const STABLE_AFTER: Duration = Duration::from_secs(3);

/// Ce qu'on sait d'un fichier vu lors d'une inspection précédente.
#[derive(Debug, Clone, Copy)]
struct Observation {
    size: u64,
    /// Nombre d'inspections consécutives où la taille n'a pas bougé.
    stable_rounds: u32,
    /// Le fichier a déjà été proposé à l'import.
    ///
    /// Indispensable : un import **réussi** déplace le fichier, qui disparaît
    /// donc du dépôt. Mais un import **échoué** l'y laisse. Sans ce drapeau, un
    /// fichier corrompu serait retenté toutes les quelques secondes, à jamais,
    /// en saturant les journaux et la table `import_jobs`.
    signalled: bool,
}

/// Détecteur de fichiers prêts à être importés.
///
/// Sans dépendance au système de fichiers réel : il reçoit une liste de
/// `(chemin, taille)`, ce qui le rend entièrement testable.
#[derive(Debug, Default)]
pub struct StabilityTracker {
    seen: HashMap<PathBuf, Observation>,
}

impl StabilityTracker {
    /// Nombre d'inspections consécutives requises pour déclarer un fichier
    /// stable.
    fn required_rounds() -> u32 {
        // Arrondi au supérieur : mieux vaut attendre un tour de trop que
        // d'importer un fichier incomplet.
        (STABLE_AFTER.as_secs_f64() / POLL_INTERVAL.as_secs_f64()).ceil() as u32
    }

    /// Enregistre une inspection et retourne les fichiers devenus stables.
    ///
    /// Un fichier n'est proposé **qu'une seule fois**. Il ne sera reproposé que
    /// si sa taille change — c'est-à-dire s'il a été réellement remplacé — ou
    /// s'il quitte le dépôt puis y revient.
    pub fn observe(&mut self, entries: &[(PathBuf, u64)]) -> Vec<PathBuf> {
        let mut ready = Vec::new();
        let mut still_present = HashMap::with_capacity(entries.len());

        for (path, size) in entries {
            let mut observation = match self.seen.get(path) {
                // Taille identique au tour précédent : un pas de plus vers la
                // stabilité.
                Some(previous) if previous.size == *size => Observation {
                    stable_rounds: previous.stable_rounds.saturating_add(1),
                    ..*previous
                },
                // Taille différente, ou fichier inconnu : le compte repart, et
                // un fichier remplacé redevient éligible.
                _ => Observation {
                    size: *size,
                    stable_rounds: 0,
                    signalled: false,
                },
            };

            if !observation.signalled && observation.stable_rounds >= Self::required_rounds() {
                observation.signalled = true;
                ready.push(path.clone());
            }

            still_present.insert(path.clone(), observation);
        }

        // Les fichiers disparus sortent de la mémoire : sans cela, un dossier
        // très fréquenté ferait enfler la table indéfiniment. C'est aussi ce
        // qui permet de réimporter un fichier retiré puis redéposé.
        self.seen = still_present;
        ready
    }

    /// Nombre de fichiers en cours d'observation.
    pub fn pending(&self) -> usize {
        self.seen.len()
    }
}

/// Liste les fichiers audio importables du dossier surveillé, avec leur taille.
///
/// Non récursif : un dépôt est un dépôt, pas une arborescence. Cela évite aussi
/// qu'un dossier en cours de copie soit exploré à moitié.
pub fn list_candidates(inbox: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(inbox) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| importer::is_importable(path))
        .filter_map(|path| {
            let size = std::fs::metadata(&path).ok()?.len();
            Some((path, size))
        })
        .collect()
}

/// Chemin du dossier surveillé pour une racine de bibliothèque donnée.
pub fn inbox_path(library_root: &Path) -> PathBuf {
    library_root.join(INBOX_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chemin(nom: &str) -> PathBuf {
        PathBuf::from(format!("/inbox/{nom}"))
    }

    /// Simule `n` inspections successives d'un fichier de taille constante et
    /// retourne l'ensemble des fichiers signalés sur toute la série.
    fn observer_n_fois(tracker: &mut StabilityTracker, path: &Path, size: u64, n: u32) -> Vec<PathBuf> {
        let mut signales = Vec::new();
        for _ in 0..n {
            signales.extend(tracker.observe(&[(path.to_path_buf(), size)]));
        }
        signales
    }

    #[test]
    fn un_fichier_vu_une_seule_fois_nest_pas_importe() {
        let mut tracker = StabilityTracker::default();
        let ready = tracker.observe(&[(chemin("a.mp3"), 1000)]);

        assert!(
            ready.is_empty(),
            "une seule observation ne prouve pas que le fichier est complet"
        );
    }

    #[test]
    fn un_fichier_de_taille_stable_finit_par_etre_importe() {
        let mut tracker = StabilityTracker::default();
        let ready = observer_n_fois(&mut tracker, &chemin("a.mp3"), 1000, 5);

        assert_eq!(ready, vec![chemin("a.mp3")]);
    }

    #[test]
    fn un_telechargement_en_cours_nest_jamais_importe() {
        let mut tracker = StabilityTracker::default();

        // La taille augmente à chaque tour : le fichier se remplit encore.
        for taille in [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000] {
            let ready = tracker.observe(&[(chemin("gros.mp3"), taille)]);
            assert!(
                ready.is_empty(),
                "un fichier qui grossit encore ne doit jamais être importé"
            );
        }
    }

    #[test]
    fn une_reprise_de_telechargement_remet_le_compteur_a_zero() {
        let mut tracker = StabilityTracker::default();
        let fichier = chemin("pause.mp3");

        // Le réseau s'interrompt : la taille se fige presque assez longtemps…
        tracker.observe(&[(fichier.clone(), 5000)]);
        tracker.observe(&[(fichier.clone(), 5000)]);

        // …puis le téléchargement reprend.
        let ready = tracker.observe(&[(fichier.clone(), 9000)]);
        assert!(ready.is_empty());

        // Il faut de nouveau la durée complète de stabilité.
        let ready = tracker.observe(&[(fichier.clone(), 9000)]);
        assert!(ready.is_empty(), "le compteur doit être reparti de zéro");
    }

    #[test]
    fn un_fichier_dont_limport_echoue_nest_jamais_retente_en_boucle() {
        let mut tracker = StabilityTracker::default();
        let fichier = chemin("corrompu.mp3");

        // Vingt inspections : le fichier reste dans le dépôt parce que son
        // import échoue à chaque fois. Il ne doit être proposé qu'une seule
        // fois, sans quoi les journaux et `import_jobs` se rempliraient sans
        // fin.
        let signales = observer_n_fois(&mut tracker, &fichier, 1000, 20);

        assert_eq!(signales, vec![fichier]);
    }

    #[test]
    fn un_fichier_remplace_redevient_eligible() {
        let mut tracker = StabilityTracker::default();
        let fichier = chemin("a.mp3");

        assert_eq!(observer_n_fois(&mut tracker, &fichier, 1000, 5).len(), 1);

        // L'utilisateur dépose une autre version, de taille différente.
        let signales = observer_n_fois(&mut tracker, &fichier, 4000, 5);

        assert_eq!(signales, vec![fichier], "un fichier remplacé doit être réimporté");
    }

    #[test]
    fn un_fichier_retire_puis_redepose_redevient_eligible() {
        let mut tracker = StabilityTracker::default();
        let fichier = chemin("a.mp3");

        assert_eq!(observer_n_fois(&mut tracker, &fichier, 1000, 5).len(), 1);

        // Le dépôt est vide : le fichier sort de la mémoire.
        tracker.observe(&[]);
        assert_eq!(tracker.pending(), 0);

        assert_eq!(observer_n_fois(&mut tracker, &fichier, 1000, 5).len(), 1);
    }

    #[test]
    fn oublie_les_fichiers_disparus() {
        let mut tracker = StabilityTracker::default();

        tracker.observe(&[(chemin("a.mp3"), 1), (chemin("b.mp3"), 2)]);
        assert_eq!(tracker.pending(), 2);

        // Seul « a » subsiste : « b » a été déplacé ou supprimé.
        tracker.observe(&[(chemin("a.mp3"), 1)]);
        assert_eq!(tracker.pending(), 1, "la table ne doit pas enfler sans fin");
    }

    #[test]
    fn suit_plusieurs_fichiers_independamment() {
        let mut tracker = StabilityTracker::default();
        let stable = chemin("fini.mp3");
        let en_cours = chemin("encours.mp3");

        let mut signales = Vec::new();
        for tour in 0..5_u64 {
            signales.extend(tracker.observe(&[
                (stable.clone(), 1000),
                // Celui-ci grossit encore.
                (en_cours.clone(), 1000 + tour * 500),
            ]));
        }

        assert_eq!(signales, vec![stable]);
    }

    #[test]
    fn le_dossier_surveille_est_a_la_racine_de_la_bibliotheque() {
        assert_eq!(
            inbox_path(Path::new("/Volumes/Lexar/Musique")),
            PathBuf::from("/Volumes/Lexar/Musique/_Inbox")
        );
    }

    #[test]
    fn ne_retient_que_les_fichiers_audio_du_depot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("morceau.mp3"), b"12345").unwrap();
        std::fs::write(dir.path().join("pochette.jpg"), b"x").unwrap();
        // AppleDouble semé par macOS sur exFAT.
        std::fs::write(dir.path().join("._morceau.mp3"), b"x").unwrap();

        let candidats = list_candidates(dir.path());

        assert_eq!(candidats.len(), 1);
        assert_eq!(candidats[0].1, 5, "la taille doit être remontée");
    }

    #[test]
    fn un_dossier_inexistant_ne_provoque_pas_derreur() {
        assert!(list_candidates(Path::new("/introuvable/_Inbox")).is_empty());
    }
}
