//! Synchronisation entre deux appareils, sur le réseau local.
//!
//! # Ce qui circule, et ce qui ne circule pas
//!
//! Les favoris, les playlists et les paroles. **Jamais les fichiers audio** :
//! ils font des dizaines de gigaoctets et sont déjà des deux côtés. Jamais les
//! titres ni les albums non plus — corriger deux mille titres d'un coup, sans
//! qu'on l'ait demandé, est exactement le genre de chose qu'Onzer ne fait pas.
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
