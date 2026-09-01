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
//!        ├─► CORROBORATION  →  la proposition tient-elle face aux tags
//!        │                     que le fichier portait déjà ? (`verdict`)
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
//!
//! # Pourquoi l'empreinte ne suffit pas non plus
//!
//! Elle désigne une **fiche**, et les fiches publiques contiennent aussi des
//! rips YouTube versés à la va-vite. Un morceau de Damso s'est retrouvé étiqueté
//! « carmen (Clip Officiel) » de Stromae par ce chemin. D'où l'étape de
//! corroboration : une empreinte est une présomption, les tags du fichier en
//! sont une autre, et c'est leur confrontation qui décide.

pub mod acoustid;
pub mod fingerprint;
pub mod http;
pub mod lrclib;
pub mod coverart;
pub mod discover;
pub mod musicbrainz;
pub mod ratelimit;
pub mod revise;
pub mod spotdl;
pub mod tagger;
pub mod verdict;
pub mod worker;
