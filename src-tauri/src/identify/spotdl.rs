//! Lecture d'un fichier `.spotdl`.
//!
//! # Pourquoi ce détour plutôt que l'API Spotify
//!
//! Le premier jet interrogeait l'API officielle avec des identifiants
//! d'application créés par l'utilisateur. Spotify a répondu :
//!
//! > 403 Forbidden — *Active premium subscription required for the owner of the
//! > app.*
//!
//! Lire la liste d'une playlist **publique** exige désormais un abonnement
//! payant sur le compte propriétaire de l'application. La porte est fermée, et
//! aucune astuce ne la rouvrira honnêtement.
//!
//! # La sortie
//!
//! `spotdl`, que l'utilisateur a déjà installé, sait produire un fichier de
//! métadonnées avec sa sous-commande `save` — **sans rien télécharger**, et sans
//! demander le moindre identifiant. Onzer lit ce fichier.
//!
//! Le partage des rôles reste le même qu'avant, et c'est ce qui compte :
//! l'outil de l'utilisateur va chercher la liste, Onzer la compare à ce qu'il
//! possède. Onzer ne télécharge toujours rien, et n'exécute aucune commande.
//!
//! # Le format
//!
//! Un tableau JSON d'objets, un par titre. On n'en lit que quatre champs, et de
//! façon tolérante : `spotdl` évolue, et une clé renommée ne doit pas rendre le
//! fichier entier illisible.

use serde::{Deserialize, Serialize};

use crate::core::{OnzerError, Result};

/// Garde-fou : au-delà, ce n'est plus une playlist mais un catalogue.
const MAX_TRACKS: usize = 5_000;

/// Un titre lu dans le fichier.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTrack {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    /// Lien Spotify du titre, quand le fichier le porte.
    pub url: String,
    /// « Artiste - Titre », requête prête pour un téléchargeur externe.
    pub query: String,
}

impl PlaylistTrack {
    pub fn main_artist(&self) -> Option<&str> {
        self.artists.first().map(String::as_str)
    }
}

/// Analyse le contenu d'un fichier `.spotdl`.
pub fn parse(raw: &str) -> Result<Vec<PlaylistTrack>> {
    let entries: Vec<Entry> = serde_json::from_str(raw).map_err(|error| {
        OnzerError::Invalid(format!(
            "fichier illisible — est-ce bien un fichier produit par « spotdl save » ? ({error})"
        ))
    })?;

    if entries.is_empty() {
        return Err(OnzerError::Invalid(
            "le fichier ne contient aucun titre".to_string(),
        ));
    }

    Ok(entries
        .into_iter()
        .filter_map(Entry::into_track)
        .take(MAX_TRACKS)
        .collect())
}

/// Extrait le nom de la playlist, quand le fichier le porte.
///
/// `spotdl` inscrit le nom de la liste d'origine sur chaque titre. Le lire sur
/// le premier suffit, et évite d'afficher « playlist » là où l'utilisateur
/// attend le nom qu'il a donné à la sienne.
pub fn playlist_name(raw: &str) -> Option<String> {
    let entries: Vec<Entry> = serde_json::from_str(raw).ok()?;

    entries
        .into_iter()
        .find_map(|entry| entry.song_list.and_then(|list| list.name))
        .filter(|name| !name.trim().is_empty())
}

/// Un objet du fichier, lu avec indulgence.
///
/// Chaque champ a des noms alternatifs : `spotdl` évolue, et une clé renommée
/// ne doit pas rendre tout le fichier inexploitable.
#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(alias = "title", default)]
    name: String,
    #[serde(default)]
    artists: Vec<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(alias = "album", default)]
    album_name: Option<String>,
    /// Durée en **secondes**, telle que `spotdl` l'écrit.
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    song_list: Option<SongList>,
}

#[derive(Debug, Deserialize)]
struct SongList {
    #[serde(default)]
    name: Option<String>,
}

impl Entry {
    fn into_track(self) -> Option<PlaylistTrack> {
        let title = self.name.trim().to_string();
        if title.is_empty() {
            return None;
        }

        // `artists` d'abord : il porte tous les crédits. `artist` ne contient
        // que le principal, et sert de repli.
        let mut artists: Vec<String> = self
            .artists
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        if artists.is_empty() {
            if let Some(single) = self.artist.filter(|name| !name.trim().is_empty()) {
                artists.push(single.trim().to_string());
            }
        }

        // La durée arrive en secondes ; on la ramène en millisecondes, unité de
        // toute la base.
        let duration_ms = self
            .duration_ms
            .or_else(|| self.duration.map(|seconds| (seconds * 1000.0).round() as i64))
            .unwrap_or(0);

        let query = match artists.first() {
            Some(artist) => format!("{artist} - {title}"),
            None => title.clone(),
        };

        Some(PlaylistTrack {
            album: self.album_name.filter(|name| !name.trim().is_empty()),
            url: self.url.unwrap_or_default(),
            title,
            artists,
            duration_ms,
            query,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEMPLE: &str = r#"[
        {
            "name": "Macarena",
            "artists": ["Damso"],
            "artist": "Damso",
            "album_name": "Ipséité",
            "duration": 206.4,
            "url": "https://open.spotify.com/track/abc",
            "song_list": {"name": "Le reste"}
        },
        {
            "name": "BXL ZOO",
            "artists": ["Damso", "Hamza"],
            "duration": 180.0
        }
    ]"#;

    #[test]
    fn lit_un_fichier_spotdl() {
        let tracks = parse(EXEMPLE).unwrap();

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "Macarena");
        assert_eq!(tracks[0].main_artist(), Some("Damso"));
        assert_eq!(tracks[0].album.as_deref(), Some("Ipséité"));
    }

    #[test]
    fn convertit_les_secondes_en_millisecondes() {
        // `spotdl` écrit des secondes décimales, toute la base est en
        // millisecondes. Se tromper d'unité ici ferait échouer chaque
        // rapprochement par durée, sans erreur visible.
        let tracks = parse(EXEMPLE).unwrap();

        assert_eq!(tracks[0].duration_ms, 206_400);
        assert_eq!(tracks[1].duration_ms, 180_000);
    }

    #[test]
    fn conserve_tous_les_credits() {
        let tracks = parse(EXEMPLE).unwrap();

        assert_eq!(tracks[1].artists, vec!["Damso", "Hamza"]);
        assert_eq!(
            tracks[1].main_artist(),
            Some("Damso"),
            "le premier crédité reste le principal"
        );
    }

    #[test]
    fn compose_une_requete_utilisable() {
        assert_eq!(parse(EXEMPLE).unwrap()[0].query, "Damso - Macarena");
    }

    #[test]
    fn retrouve_le_nom_de_la_playlist() {
        // Sans lui, l'écran afficherait « playlist » là où l'utilisateur attend
        // le nom qu'il a donné à la sienne.
        assert_eq!(playlist_name(EXEMPLE).as_deref(), Some("Le reste"));
    }

    #[test]
    fn se_rabat_sur_le_champ_artist_seul() {
        // Certaines versions n'écrivent que le principal.
        let tracks = parse(r#"[{"name": "Titre", "artist": "Népal", "duration": 120}]"#).unwrap();

        assert_eq!(tracks[0].artists, vec!["Népal"]);
        assert_eq!(tracks[0].query, "Népal - Titre");
    }

    #[test]
    fn ignore_les_entrees_sans_titre() {
        // Une entrée que `spotdl` n'a pas su résoudre — il en produit — ne doit
        // pas apparaître comme un morceau sans nom.
        let tracks =
            parse(r#"[{"name": "", "artist": "X"}, {"name": "Vrai", "artist": "Y"}]"#).unwrap();

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Vrai");
    }

    #[test]
    fn refuse_ce_qui_nest_pas_un_fichier_spotdl() {
        assert!(parse("bonjour").is_err());
        assert!(parse("{}").is_err());
        assert!(parse("[]").is_err(), "un fichier vide n'a rien à comparer");
    }

    #[test]
    fn une_duree_absente_ne_fait_pas_echouer_la_lecture() {
        // Le rapprochement portera alors sur le titre et l'artiste seuls.
        let tracks = parse(r#"[{"name": "Titre", "artist": "X"}]"#).unwrap();
        assert_eq!(tracks[0].duration_ms, 0);
    }
}
