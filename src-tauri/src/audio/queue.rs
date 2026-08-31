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

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.cursor = None;
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
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
