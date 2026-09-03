//! File de lecture.
//!
//! Logique pure et entièrement testable : aucune dépendance à l'audio ni à la
//! base. L'aléatoire lui-même est déterministe à graine fixée, faute de quoi le
//! mélange serait intestable.

use serde::Serialize;

use crate::db::repository::TrackSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    /// Reboucle sur la file entière.
    All,
    /// Rejoue indéfiniment le morceau courant.
    One,
}

/// Morceau en file, avec le minimum nécessaire à la lecture et à l'affichage.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub track_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub relative_path: String,
    pub artwork_hash: Option<String>,
}

impl From<TrackSummary> for QueueItem {
    fn from(track: TrackSummary) -> Self {
        Self {
            track_id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            duration_ms: track.duration_ms,
            relative_path: track.relative_path,
            artwork_hash: track.artwork_hash,
        }
    }
}

#[derive(Debug, Default)]
pub struct Queue {
    items: Vec<QueueItem>,
    /// Ordre de lecture : identité en mode normal, permutation en aléatoire.
    /// Le tableau `items` n'est jamais réordonné, pour que l'interface puisse
    /// continuer d'afficher la file dans son ordre d'origine.
    order: Vec<usize>,
    /// Position **dans `order`**, et non dans `items`.
    cursor: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    seed: u64,
}

impl Queue {
    /// Remplace la file et positionne la lecture sur `start_at`.
    pub fn replace(&mut self, items: Vec<QueueItem>, start_at: usize) {
        let start_at = start_at.min(items.len().saturating_sub(1));
        self.items = items;
        self.rebuild_order(Some(start_at));
    }

    /// Ajoute un morceau à la fin, sans perturber la lecture en cours.
    pub fn enqueue(&mut self, item: QueueItem) {
        self.items.push(item);
        self.order.push(self.items.len() - 1);
    }

    /// Insère juste **après** le morceau en cours.
    ///
    /// # Pourquoi ce n'est pas `enqueue`
    ///
    /// Ajouter à la fin d'une file de deux mille morceaux revient à ne jamais
    /// jouer ce qu'on vient d'ajouter. « Lire ensuite » est l'autre moitié du
    /// geste : ce morceau-là, maintenant, sans couper celui qui joue.
    pub fn insert_next(&mut self, item: QueueItem) {
        let Some(courant) = self.current_index() else {
            // Rien ne joue : c'est un simple ajout, et il devient le premier.
            self.items.insert(0, item);
            self.decaler_ordre(0);
            self.order.insert(0, 0);
            self.cursor = Some(0);
            return;
        };

        let position = courant + 1;
        self.items.insert(position, item);
        self.decaler_ordre(position);

        // Dans l'ordre de lecture — qui n'est pas celui de l'affichage en
        // aléatoire —, il vient juste après le curseur.
        let apres = self.cursor.map_or(0, |curseur| curseur + 1);
        self.order.insert(apres.min(self.order.len()), position);
    }

    /// Retire le morceau occupant cette **place dans l'ordre de lecture**.
    ///
    /// Rend `true` si c'était celui qui joue : l'appelant doit alors décider
    /// de la suite, ce que la file ne peut pas faire seule — elle ne sait pas
    /// commander le son.
    pub fn remove_at(&mut self, position: usize) -> bool {
        if position >= self.order.len() {
            return false;
        }

        let etait_courant = self.cursor == Some(position);
        let item_index = self.order.remove(position);
        self.items.remove(item_index);

        // Les positions qui pointaient au-delà du trou pointent maintenant un
        // cran plus tôt.
        for index in &mut self.order {
            if *index > item_index {
                *index -= 1;
            }
        }

        // Le curseur désigne une place dans l'ordre : ce qui disparaît avant
        // lui le décale.
        if let Some(curseur) = self.cursor {
            if position < curseur {
                self.cursor = Some(curseur - 1);
            }
        }

        if self.order.is_empty() {
            self.cursor = None;
        } else if let Some(curseur) = self.cursor {
            self.cursor = Some(curseur.min(self.order.len() - 1));
        }

        etait_courant
    }

    /// Déplace un morceau dans l'ordre de lecture.
    ///
    /// # Pourquoi c'est `order` qu'on remue, et non `items`
    ///
    /// Remonter une ligne dans « À suivre » veut dire une chose et une seule :
    /// « joue-le plus tôt ». C'est donc l'ordre de lecture qu'on réécrit.
    /// Toucher à `items` reviendrait à réordonner la liste d'origine — l'album
    /// dont on est parti — sans rien changer à ce qui va sortir des enceintes.
    pub fn move_position(&mut self, from: usize, to: usize) {
        if from >= self.order.len() || to >= self.order.len() || from == to {
            return;
        }

        let index = self.order.remove(from);
        self.order.insert(to, index);

        // Le morceau en cours ne change pas ; sa place dans l'ordre, si.
        if let Some(curseur) = self.cursor {
            self.cursor = Some(if curseur == from {
                to
            } else if from < curseur && curseur <= to {
                curseur - 1
            } else if to <= curseur && curseur < from {
                curseur + 1
            } else {
                curseur
            });
        }
    }

    /// Décale d'un cran les positions situées à partir de `depuis`.
    fn decaler_ordre(&mut self, depuis: usize) {
        for index in &mut self.order {
            if *index >= depuis {
                *index += 1;
            }
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.cursor = None;
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    /// La file **dans l'ordre où elle sera jouée**.
    ///
    /// # Pourquoi c'est celle-ci qu'on montre
    ///
    /// `items` garde l'ordre d'origine, celui de la liste qu'on a lancée.
    /// C'est le bon ordre pour tout, sauf pour répondre à la seule question
    /// qu'on pose à une file : « qu'est-ce qui vient après ? ». En aléatoire,
    /// les deux n'ont plus rien à voir — « À suivre » annonçait le morceau
    /// suivant de l'album pendant que la lecture partait ailleurs.
    pub fn playback_items(&self) -> Vec<QueueItem> {
        self.order
            .iter()
            .filter_map(|&index| self.items.get(index).cloned())
            .collect()
    }

    /// Position du morceau en cours dans l'ordre de lecture.
    pub fn playback_index(&self) -> Option<usize> {
        self.cursor
    }

    /// Positionne la lecture sur une **place de l'ordre de lecture**.
    pub fn jump_to_position(&mut self, position: usize) -> Option<&QueueItem> {
        if position >= self.order.len() {
            return None;
        }

        self.cursor = Some(position);
        self.current()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current(&self) -> Option<&QueueItem> {
        let position = *self.order.get(self.cursor?)?;
        self.items.get(position)
    }

    /// Index du morceau courant dans `items`, pour la mise en évidence.
    pub fn current_index(&self) -> Option<usize> {
        self.order.get(self.cursor?).copied()
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Bascule le mode aléatoire **en conservant le morceau courant**.
    ///
    /// Sans cette précaution, activer l'aléatoire couperait la musique en
    /// cours, ce qui est le défaut le plus agaçant qu'un lecteur puisse avoir.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let current = self.current_index();
        self.rebuild_order(current);
    }

    /// Positionne la lecture sur un morceau précis de la file.
    pub fn jump_to(&mut self, item_index: usize) -> Option<&QueueItem> {
        let cursor = self.order.iter().position(|&index| index == item_index)?;
        self.cursor = Some(cursor);
        self.current()
    }

    /// Avance. `None` quand la file est terminée et qu'aucune répétition n'est
    /// active.
    ///
    /// `automatic` distingue un enchaînement naturel d'un appui sur « suivant » :
    /// en répétition d'un seul morceau, l'enchaînement rejoue le même titre,
    /// alors que l'utilisateur qui demande le suivant veut vraiment changer.
    pub fn advance(&mut self, automatic: bool) -> Option<&QueueItem> {
        if self.order.is_empty() {
            return None;
        }

        let cursor = self.cursor.unwrap_or(0);

        if automatic && self.repeat == RepeatMode::One {
            return self.current();
        }

        let next = cursor + 1;
        self.cursor = if next < self.order.len() {
            Some(next)
        } else if self.repeat == RepeatMode::All || (!automatic && self.repeat == RepeatMode::One) {
            Some(0)
        } else {
            return None;
        };

        self.current()
    }

    /// Recule d'un morceau. Reste sur le premier si l'on y est déjà.
    pub fn previous(&mut self) -> Option<&QueueItem> {
        let cursor = self.cursor?;

        self.cursor = Some(if cursor > 0 {
            cursor - 1
        } else if self.repeat == RepeatMode::All {
            self.order.len() - 1
        } else {
            0
        });

        self.current()
    }

    /// Le morceau qui suivra, sans déplacer la lecture.
    pub fn peek_next(&self) -> Option<&QueueItem> {
        let cursor = self.cursor?;
        let next = match self.repeat {
            RepeatMode::One => cursor,
            _ if cursor + 1 < self.order.len() => cursor + 1,
            RepeatMode::All => 0,
            RepeatMode::Off => return None,
        };

        self.items.get(*self.order.get(next)?)
    }

    /// Reconstruit l'ordre de lecture, en gardant `keep` en tête du curseur.
    fn rebuild_order(&mut self, keep: Option<usize>) {
        self.order = (0..self.items.len()).collect();

        if self.shuffle {
            self.seed = self.seed.wrapping_add(next_seed());
            shuffle_in_place(&mut self.order, &mut self.seed);

            // Le morceau courant est ramené en tête pour ne pas être coupé.
            if let Some(current) = keep {
                if let Some(position) = self.order.iter().position(|&index| index == current) {
                    self.order.swap(0, position);
                }
            }
        }

        self.cursor = match keep {
            Some(current) => self.order.iter().position(|&index| index == current),
            None if self.items.is_empty() => None,
            None => Some(0),
        };
    }
}

/// Mélange de Fisher-Yates avec un générateur xorshift.
///
/// Un générateur maison évite une dépendance supplémentaire pour un besoin où
/// la qualité cryptographique n'a aucun intérêt, et rend le mélange
/// reproductible à graine fixée — donc testable.
fn shuffle_in_place(slice: &mut [usize], seed: &mut u64) {
    for index in (1..slice.len()).rev() {
        let pick = (next_random(seed) % (index as u64 + 1)) as usize;
        slice.swap(index, pick);
    }
}

fn next_random(state: &mut u64) -> u64 {
    // xorshift64* : suffisant et sans dépendance.
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    if *state == 0 {
        *state = 0x2545_F491_4F6C_DD1D;
    }
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn next_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64) -> QueueItem {
        QueueItem {
            track_id: id,
            title: format!("Titre {id}"),
            artist: None,
            album: None,
            duration_ms: 1000,
            relative_path: format!("a/{id}.mp3"),
            artwork_hash: None,
        }
    }

    fn file(count: i64) -> Queue {
        let mut queue = Queue::default();
        queue.replace((1..=count).map(item).collect(), 0);
        queue
    }

    fn courant(queue: &Queue) -> Option<i64> {
        queue.current().map(|item| item.track_id)
    }

    // ── Manipulation de la file ─────────────────────────────────────────

    /// Les identifiants dans l'ordre d'affichage.
    fn affiches(queue: &Queue) -> Vec<i64> {
        queue.items().iter().map(|item| item.track_id).collect()
    }

    /// Les identifiants dans l'ordre de lecture.
    fn a_jouer(queue: &Queue) -> Vec<i64> {
        queue
            .order
            .iter()
            .map(|&index| queue.items()[index].track_id)
            .collect()
    }

    #[test]
    fn lire_ensuite_se_place_juste_apres_le_courant() {
        let mut queue = file(3);
        queue.jump_to_position(0);

        queue.insert_next(item(99));

        assert_eq!(a_jouer(&queue), vec![1, 99, 2, 3]);
        assert_eq!(courant(&queue), Some(1), "le morceau en cours ne bouge pas");
        assert_eq!(queue.advance(true).map(|i| i.track_id), Some(99));
    }

    #[test]
    fn lire_ensuite_sur_une_file_vide_lance_le_morceau() {
        let mut queue = Queue::default();
        queue.insert_next(item(7));

        assert_eq!(courant(&queue), Some(7));
    }

    #[test]
    fn retirer_un_morceau_a_venir_ne_touche_pas_la_lecture() {
        let mut queue = file(4);
        queue.jump_to_position(1);

        assert!(!queue.remove_at(3), "ce n'était pas le morceau en cours");

        assert_eq!(affiches(&queue), vec![1, 2, 3]);
        assert_eq!(courant(&queue), Some(2), "on écoute toujours le même");
    }

    #[test]
    fn retirer_un_morceau_deja_passe_garde_le_curseur_au_bon_endroit() {
        // Le défaut classique : on retire au-dessus, et la lecture saute.
        let mut queue = file(4);
        queue.jump_to_position(2);

        assert!(!queue.remove_at(0));

        assert_eq!(affiches(&queue), vec![2, 3, 4]);
        assert_eq!(courant(&queue), Some(3), "toujours le même morceau");
    }

    #[test]
    fn retirer_le_morceau_en_cours_se_signale() {
        let mut queue = file(3);
        queue.jump_to_position(1);

        assert!(queue.remove_at(1), "l'appelant doit savoir qu'il faut réagir");
        assert_eq!(affiches(&queue), vec![1, 3]);
    }

    #[test]
    fn remonter_un_morceau_le_fait_jouer_plus_tot() {
        let mut queue = file(4);
        queue.jump_to_position(0);

        // Le quatrième remonte juste après celui qui joue.
        queue.move_position(3, 1);

        assert_eq!(a_jouer(&queue), vec![1, 4, 2, 3]);
        assert_eq!(courant(&queue), Some(1), "on écoute toujours le même");
        assert_eq!(queue.advance(true).map(|i| i.track_id), Some(4));
    }

    #[test]
    fn deplacer_ne_touche_pas_a_la_liste_d_origine() {
        // `items` est l'album dont on est parti : le réordonner n'aurait
        // aucun effet sur ce qui sort des enceintes, et perdrait son ordre.
        let mut queue = file(4);
        let avant = affiches(&queue);

        queue.move_position(3, 0);

        assert_eq!(affiches(&queue), avant);
    }

    #[test]
    fn deplacer_devant_le_courant_ne_le_perd_pas() {
        let mut queue = file(5);
        queue.jump_to_position(2);

        // Un morceau situé après passe devant celui qui joue.
        queue.move_position(4, 0);

        assert_eq!(courant(&queue), Some(3), "toujours le même morceau");
        assert_eq!(a_jouer(&queue), vec![5, 1, 2, 3, 4]);
    }

    #[test]
    fn retirer_conserve_l_ordre_aleatoire() {
        let mut queue = file(5);
        queue.set_shuffle(true);

        let attendu: Vec<i64> = a_jouer(&queue).into_iter().skip(1).take(3).collect();
        queue.remove_at(4);
        queue.remove_at(0);

        assert_eq!(a_jouer(&queue), attendu);
    }

    #[test]
    fn en_aleatoire_ce_qu_on_montre_est_ce_qu_on_joue() {
        // Le défaut : « À suivre » annonçait la suite de l'album pendant que
        // la lecture partait ailleurs.
        let mut queue = file(6);
        queue.set_shuffle(true);

        let joue = a_jouer(&queue);
        let montre: Vec<i64> = queue.playback_items().iter().map(|i| i.track_id).collect();

        assert_eq!(montre, joue);
    }

    // ── Navigation ──────────────────────────────────────────────────────

    #[test]
    fn demarre_sur_le_morceau_demande() {
        let mut queue = Queue::default();
        queue.replace((1..=5).map(item).collect(), 2);

        assert_eq!(courant(&queue), Some(3));
    }

    #[test]
    fn avance_puis_recule() {
        let mut queue = file(3);

        assert_eq!(queue.advance(true).map(|i| i.track_id), Some(2));
        assert_eq!(queue.advance(true).map(|i| i.track_id), Some(3));
        assert_eq!(queue.previous().map(|i| i.track_id), Some(2));
    }

    #[test]
    fn sarrete_a_la_fin_sans_repetition() {
        let mut queue = file(2);
        queue.advance(true);

        assert_eq!(queue.advance(true).map(|i| i.track_id), None);
    }

    #[test]
    fn reste_sur_le_premier_en_reculant_depuis_le_debut() {
        let mut queue = file(3);
        assert_eq!(queue.previous().map(|i| i.track_id), Some(1));
    }

    #[test]
    fn une_file_vide_ne_fait_rien() {
        let mut queue = Queue::default();

        assert!(queue.current().is_none());
        assert!(queue.advance(true).is_none());
        assert!(queue.previous().is_none());
        assert!(queue.peek_next().is_none());
    }

    // ── Répétition ──────────────────────────────────────────────────────

    #[test]
    fn la_repetition_totale_reboucle() {
        let mut queue = file(2);
        queue.set_repeat(RepeatMode::All);
        queue.advance(true);

        assert_eq!(queue.advance(true).map(|i| i.track_id), Some(1));
    }

    #[test]
    fn la_repetition_totale_recule_vers_la_fin() {
        let mut queue = file(3);
        queue.set_repeat(RepeatMode::All);

        assert_eq!(queue.previous().map(|i| i.track_id), Some(3));
    }

    #[test]
    fn la_repetition_unitaire_ne_boucle_que_sur_lenchainement_naturel() {
        let mut queue = file(3);
        queue.set_repeat(RepeatMode::One);

        assert_eq!(
            queue.advance(true).map(|i| i.track_id),
            Some(1),
            "la fin naturelle rejoue le même morceau"
        );
        assert_eq!(
            queue.advance(false).map(|i| i.track_id),
            Some(2),
            "un appui sur « suivant » doit malgré tout changer de morceau"
        );
    }

    // ── Aléatoire ───────────────────────────────────────────────────────

    #[test]
    fn le_mode_aleatoire_ne_coupe_pas_la_musique_en_cours() {
        let mut queue = file(50);
        queue.advance(true);
        queue.advance(true); // morceau 3

        queue.set_shuffle(true);

        assert_eq!(
            courant(&queue),
            Some(3),
            "activer l'aléatoire ne doit jamais interrompre la lecture"
        );
    }

    #[test]
    fn le_mode_aleatoire_conserve_tous_les_morceaux() {
        let mut queue = file(30);
        queue.set_shuffle(true);

        let mut vus = vec![courant(&queue).unwrap()];
        while let Some(item) = queue.advance(true) {
            vus.push(item.track_id);
        }
        vus.sort_unstable();

        assert_eq!(vus, (1..=30).collect::<Vec<_>>(), "aucun morceau perdu ni dupliqué");
    }

    #[test]
    fn le_mode_aleatoire_change_reellement_lordre() {
        let mut queue = file(60);
        queue.set_shuffle(true);

        let mut ordre = vec![courant(&queue).unwrap()];
        while let Some(item) = queue.advance(true) {
            ordre.push(item.track_id);
        }

        // Sur 60 éléments, retomber sur l'ordre naturel est indiscernable de
        // l'impossible.
        assert_ne!(ordre, (1..=60).collect::<Vec<_>>());
    }

    #[test]
    fn revenir_en_mode_normal_retablit_lordre() {
        let mut queue = file(10);
        queue.set_shuffle(true);
        queue.set_shuffle(false);

        let mut ordre = vec![courant(&queue).unwrap()];
        while let Some(item) = queue.advance(true) {
            ordre.push(item.track_id);
        }

        assert_eq!(ordre, (1..=10).collect::<Vec<_>>());
    }

    // ── Divers ──────────────────────────────────────────────────────────

    #[test]
    fn ajoute_sans_perturber_la_lecture() {
        let mut queue = file(2);
        queue.advance(true); // sur le 2

        queue.enqueue(item(99));

        assert_eq!(courant(&queue), Some(2), "la lecture ne bouge pas");
        assert_eq!(queue.advance(true).map(|i| i.track_id), Some(99));
    }

    #[test]
    fn saute_directement_a_un_morceau() {
        let mut queue = file(5);
        assert_eq!(queue.jump_to(3).map(|i| i.track_id), Some(4));
        assert!(queue.jump_to(99).is_none());
    }

    #[test]
    fn annonce_le_morceau_suivant_sans_avancer() {
        let queue = file(3);

        assert_eq!(queue.peek_next().map(|i| i.track_id), Some(2));
        assert_eq!(courant(&queue), Some(1), "peek ne doit rien déplacer");
    }

    #[test]
    fn le_suivant_annonce_respecte_la_repetition() {
        let mut queue = file(2);
        queue.advance(true); // dernier morceau

        assert_eq!(queue.peek_next().map(|i| i.track_id), None);
        queue.set_repeat(RepeatMode::All);
        assert_eq!(queue.peek_next().map(|i| i.track_id), Some(1));
    }

    #[test]
    fn un_index_de_depart_hors_bornes_est_ramene_dans_la_file() {
        let mut queue = Queue::default();
        queue.replace((1..=3).map(item).collect(), 99);

        assert_eq!(courant(&queue), Some(3));
    }
}
