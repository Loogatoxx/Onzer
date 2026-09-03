//! Synchronisation entre deux appareils, sur le réseau local.
//!
//! # Ce qui circule, et ce qui ne circule pas
//!
//! Les favoris, les playlists et les paroles passent d'eux-mêmes. Jamais les
//! titres ni les albums — corriger deux mille titres d'un coup, sans qu'on
//! l'ait demandé, est exactement le genre de chose qu'Onzer ne fait pas.
//!
//! Les **fichiers audio**, eux, ne partent qu'à la demande. La fusion annonce
//! ce que l'autre a et que nous n'avons pas, avec son poids ; un clic les fait
//! venir. Les faire descendre d'office remplirait un téléphone en silence.
//!
//! # Pourquoi rien ne sort de la maison
//!
//! Aucun compte, aucun serveur, aucun intermédiaire. Un des deux appareils
//! ouvre un port le temps d'un échange, l'autre s'y connecte, et c'est fini.
//! Le port n'est ouvert que pendant que l'écran de synchronisation est à
//! l'écran, et il est protégé par un secret tiré au sort à chaque fois.

pub mod appairage;
pub mod client;
pub mod etat;
pub mod fusion;

#[cfg(test)]
mod tests_bout_en_bout;

use fusion::Fusion;

/// L'événement qui prévient l'interface qu'une fusion vient d'être appliquée.
pub const EVENEMENT_APPLIQUEE: &str = "sync://appliquee";

/// L'avancement d'un transfert de fichiers.
pub const EVENEMENT_TRANSFERT: &str = "sync://transfert";

/// Ce que l'interface reçoit quand la base a changé sous ses pieds.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avis {
    pub appareil: String,
    pub changements: usize,
}

/// Prévient l'interface. Une notification perdue n'est pas une raison
/// d'interrompre une fusion qui, elle, a réussi.
pub fn prevenir(app: &tauri::AppHandle, appareil: &str, resultat: &Fusion) {
    use tauri::Emitter;

    let _ = app.emit(
        EVENEMENT_APPLIQUEE,
        Avis {
            appareil: appareil.to_string(),
            changements: resultat.changements.len(),
        },
    );
}

/// Ce que le lecteur est en train de jouer, sous la forme que la
/// synchronisation échange.
///
/// # Pourquoi c'est ici et non dans `etat`
///
/// `etat` ne parle qu'à la base ; le lecteur, lui, vit ailleurs. Les faire se
/// rencontrer dans la lecture de l'état obligerait chaque appel à disposer des
/// deux, y compris les tests qui n'ont pas de carte son.
pub fn lecture_courante(instantane: &crate::audio::PlaybackSnapshot) -> Option<fusion::LectureSync> {
    let courant = instantane.current.as_ref()?;

    Some(fusion::LectureSync {
        file: instantane
            .queue
            .iter()
            .map(|item| item.relative_path.clone())
            .collect(),
        position: instantane.queue_index.unwrap_or(0),
        position_ms: instantane.position_ms,
        quand: crate::core::now_ms(),
        titre: courant.title.clone(),
        artiste: courant.artist.clone(),
    })
}
