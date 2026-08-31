//! Identification des morceaux par empreinte acoustique.
//!
//! # Le principe
//!
//! ```text
//!   fichier mal tagué
//!        │
//!        ├─► empreinte Chromaprint du signal
//!        │
//!        ├─► AcoustID       →  identifiant MusicBrainz de l'enregistrement
//!        │
//!        ├─► MusicBrainz    →  artiste, album, année, piste, disque
//!        │
//!        ├─► Cover Art Archive →  pochette haute résolution
//!        │
//!        ├─► réécriture des tags dans le fichier
//!        │
//!        └─► rangement à sa vraie place
//! ```
//!
//! # Pourquoi l'empreinte plutôt que les tags
//!
//! Un nom de fichier est une supposition. Une empreinte acoustique identifie
//! **l'enregistrement précis** : elle distingue une version album de sa version
//! radio, un remaster de l'original, un live d'un studio. Aucune base de
//! métadonnées textuelles ne sait faire ça.

pub mod acoustid;
pub mod fingerprint;
pub mod http;
pub mod coverart;
pub mod musicbrainz;
pub mod ratelimit;
pub mod tagger;
pub mod worker;
