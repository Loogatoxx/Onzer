//! Révision des albums écrits par une identification antérieure.
//!
//! # Le défaut que ce module répare, après coup
//!
//! `pick_release` retenait « à qualité égale, la plus ancienne parution —
//! c'est l'originale ». La règle est juste entre deux albums, absurde entre
//! deux compilations : « Macarena » de Damso, dont l'enregistrement
//! MusicBrainz n'est rattaché qu'à neuf compilations, s'est vu attribuer
//! *I migliori anni '90*, paru en **2009** — huit ans avant le morceau — avec
//! sa pochette de compilation italienne.
//!
//! La règle est corrigée, mais **corriger un défaut ne répare pas ce qu'il a
//! déjà écrit** : l'album, l'année et la pochette sont dans la base, dans les
//! tags du fichier, et jusque dans son chemin sur le disque.
//!
//! # Comment on reconnaît un album fautif
//!
//! Un seul aller-retour MusicBrainz par **album**, et non par morceau : tous
//! les titres d'un même album partagent le même verdict, et 253 requêtes là où
//! 60 suffisent ne feraient qu'user la patience du service.
//!
//! ```text
//!   album en base ──► un de ses morceaux ──► son recording_mbid
//!                                                   │
//!                              MusicBrainz ◄────────┘
//!                                     │
//!            ┌────────────────────────┼────────────────────────┐
//!            ▼                        ▼                        ▼
//!   parution correcte        que des compilations       album introuvable
//!   → on la substitue        → on efface l'album        → on ne touche à rien
//! ```
//!
//! # Pourquoi effacer plutôt que laisser
//!
//! Une pochette fausse est pire qu'une pochette absente : elle affirme quelque
//! chose. Un carré gris n'affirme rien, et n'induit personne en erreur.

use sqlx::SqlitePool;

use crate::core::{PathResolver, Result};
use crate::library::naming::normalize_key;

use super::musicbrainz::MusicBrainzClient;

/// Version de la règle de révision.
///
/// L'incrémenter rejoue la passe sur toute la bibliothèque — à ne faire que si
/// la façon de trancher change réellement.
pub const VERSION: i64 = 1;

/// Ce que la révision a changé.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RevisionReport {
    /// Albums examinés auprès de MusicBrainz.
    pub examined: u64,
    /// Albums de compilation détachés de leurs morceaux.
    pub cleared: u64,
    /// Morceaux dont l'album a changé.
    pub tracks_touched: u64,
}

impl RevisionReport {
    pub fn is_empty(&self) -> bool {
        self.cleared == 0 && self.tracks_touched == 0
    }
}

/// Verdict porté sur un album.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlbumVerdict {
    /// Rien à changer.
    Keep,
    /// L'album vient d'une compilation : à effacer.
    Clear,
    /// Une parution correcte existe et porte un autre nom.
    Replace { title: String, year: Option<u32> },
}

/// Tranche sur un album, d'après ce que MusicBrainz dit de l'enregistrement.
///
/// Séparé de tout appel réseau et de toute base : c'est ici que se trouve la
/// subtilité, et c'est donc ici qu'il faut pouvoir écrire des tests.
///
/// * `current` — l'album tel qu'il figure en base ;
/// * `best` — la parution retenue par la règle corrigée, `None` si le morceau
///   n'est connu que par des compilations ;
/// * `all_titles` — toutes les parutions rattachées à l'enregistrement.
pub fn assess_album(
    current: &str,
    best: Option<(&str, Option<u32>)>,
    all_titles: &[String],
) -> AlbumVerdict {
    match best {
        // Une parution correcte existe. On ne la substitue que si elle diffère
        // vraiment : réécrire un album pour un accent en plus ferait bouger des
        // centaines de fichiers pour rien.
        Some((title, year)) => {
            if normalize_key(title) == normalize_key(current) {
                AlbumVerdict::Keep
            } else {
                AlbumVerdict::Replace {
                    title: title.to_string(),
                    year,
                }
            }
        }

        // Aucune parution acceptable. L'album en base est fautif **s'il vient
        // de cette fiche** — c'est-à-dire s'il figure parmi ses compilations.
        // Un album que l'utilisateur avait lui-même renseigné et que
        // MusicBrainz ignore ne doit surtout pas être effacé.
        None => {
            let current_key = normalize_key(current);
            let came_from_here = all_titles
                .iter()
                .any(|title| normalize_key(title) == current_key);

            if came_from_here {
                AlbumVerdict::Clear
            } else {
                AlbumVerdict::Keep
            }
        }
    }
}

/// Album à examiner, avec un morceau représentatif.
#[derive(Debug, sqlx::FromRow)]
struct AlbumUnderReview {
    album_id: i64,
    title: String,
    recording_mbid: String,
}

/// Passe en revue les albums issus d'une identification.
pub async fn run(
    pool: &SqlitePool,
    paths: &PathResolver,
    musicbrainz: &MusicBrainzClient,
) -> Result<RevisionReport> {
    let albums: Vec<AlbumUnderReview> = sqlx::query_as(
        // Un seul morceau par album suffit : ils partagent le même verdict.
        "SELECT al.id AS album_id, al.title,
                (SELECT t.recording_mbid FROM tracks t
                  WHERE t.album_id = al.id
                    AND t.recording_mbid IS NOT NULL
                    AND t.deleted_at IS NULL
                  LIMIT 1) AS recording_mbid
           FROM albums al
          WHERE EXISTS (SELECT 1 FROM tracks t
                         WHERE t.album_id = al.id
                           AND t.recording_mbid IS NOT NULL
                           AND t.deleted_at IS NULL)
          ORDER BY al.id",
    )
    .fetch_all(pool)
    .await?;

    let mut report = RevisionReport::default();

    for album in albums {
        let Ok(Some(metadata)) = musicbrainz.lookup(&album.recording_mbid).await else {
            // Service indisponible ou fiche disparue : on passe. La révision
            // reprendra au prochain démarrage.
            continue;
        };
        report.examined += 1;

        let best = metadata
            .album
            .as_deref()
            .map(|title| (title, metadata.year));

        match assess_album(&album.title, best, &metadata.release_titles) {
            AlbumVerdict::Keep => {}
            AlbumVerdict::Clear => {
                report.tracks_touched += detach_album(pool, paths, album.album_id).await?;
                report.cleared += 1;
            }
            AlbumVerdict::Replace { title, year } => {
                report.tracks_touched += rename_album(pool, album.album_id, &title, year).await?;
            }
        }
    }

    Ok(report)
}

/// Détache les morceaux d'un album fautif, et efface ce qu'il avait écrit.
///
/// La pochette part avec : c'est elle qui gênait le plus, une compilation
/// italienne des années 90 sur un morceau de rap belge.
async fn detach_album(pool: &SqlitePool, paths: &PathResolver, album_id: i64) -> Result<u64> {
    let tracks: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM tracks WHERE album_id = ? AND deleted_at IS NULL",
    )
    .bind(album_id)
    .fetch_all(pool)
    .await?;

    for (track_id, relative_path) in &tracks {
        // Le fichier d'abord : si le disque refuse, la base ne doit pas
        // prétendre le contraire.
        let mut final_path = relative_path.clone();

        if let Ok(path) = crate::library::importer::absolute_path(paths, relative_path) {
            if path.is_file() {
                let owned = path.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    crate::library::metadata::clear_album(&owned)
                })
                .await;

                // Le fichier dormait dans un dossier nommé d'après la
                // compilation. Le laisser là contredirait la base, et
                // l'utilisateur qui ouvre son SSD dans le Finder verrait
                // encore « 2009 - I migliori anni '90 ».
                if let Some(moved) = crate::library::importer::refile_without_album(paths, &path) {
                    final_path = moved;
                }
            }
        }

        sqlx::query("UPDATE tracks SET album_id = NULL, year = NULL, relative_path = ? WHERE id = ?")
            .bind(&final_path)
            .bind(track_id)
            .execute(pool)
            .await?;
    }

    // L'album devenu vide n'a plus de raison d'exister.
    sqlx::query("DELETE FROM albums WHERE id = ?")
        .bind(album_id)
        .execute(pool)
        .await?;

    Ok(tracks.len() as u64)
}

/// Corrige le nom et l'année d'un album, sans toucher aux fichiers.
///
/// Le renommage seul ne justifie pas de déplacer des dizaines de fichiers :
/// le rangement se corrigera à la prochaine identification.
async fn rename_album(
    pool: &SqlitePool,
    album_id: i64,
    title: &str,
    year: Option<u32>,
) -> Result<u64> {
    sqlx::query("UPDATE albums SET title = ?, normalized_title = ?, year = ? WHERE id = ?")
        .bind(title)
        .bind(normalize_key(title))
        .bind(year)
        .bind(album_id)
        .execute(pool)
        .await?;

    let touched = sqlx::query("UPDATE tracks SET year = ? WHERE album_id = ? AND deleted_at IS NULL")
        .bind(year)
        .bind(album_id)
        .execute(pool)
        .await?;

    Ok(touched.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titres(list: &[&str]) -> Vec<String> {
        list.iter().map(|title| (*title).to_string()).collect()
    }

    #[test]
    fn efface_lalbum_venu_dune_compilation() {
        // Le cas réel : « Macarena » de Damso, dont l'enregistrement n'est
        // rattaché qu'à des compilations. Onzer avait retenu la plus ancienne,
        // parue huit ans avant le morceau.
        let verdict = assess_album(
            "I migliori anni '90",
            None,
            &titres(&["I migliori anni '90", "Planète Rap 2017", "#Hits été 2017"]),
        );

        assert_eq!(verdict, AlbumVerdict::Clear);
    }

    #[test]
    fn nefface_pas_un_album_que_musicbrainz_ignore() {
        // L'utilisateur a pu renseigner un album lui-même. MusicBrainz ne le
        // connaît pas ; ce n'est pas une raison de le lui retirer.
        let verdict = assess_album(
            "Mon album à moi",
            None,
            &titres(&["I migliori anni '90", "Planète Rap 2017"]),
        );

        assert_eq!(verdict, AlbumVerdict::Keep);
    }

    #[test]
    fn substitue_une_parution_correcte() {
        let verdict = assess_album(
            "Now That's What I Call Music 48",
            Some(("Discovery", Some(2001))),
            &titres(&["Now That's What I Call Music 48", "Discovery"]),
        );

        assert_eq!(
            verdict,
            AlbumVerdict::Replace {
                title: "Discovery".to_string(),
                year: Some(2001),
            }
        );
    }

    #[test]
    fn ne_bouge_pas_pour_une_difference_de_casse_ou_daccent() {
        // Réécrire un album pour un accent ferait bouger des dizaines de
        // fichiers sans rien apporter.
        assert_eq!(
            assess_album("ipseite", Some(("Ipséité", Some(2017))), &titres(&["Ipséité"])),
            AlbumVerdict::Keep
        );
        assert_eq!(
            assess_album("DISCOVERY", Some(("Discovery", None)), &titres(&["Discovery"])),
            AlbumVerdict::Keep
        );
    }

    #[test]
    fn un_enregistrement_sans_aucune_parution_ne_touche_a_rien() {
        // Rien ne permet de dire que l'album en base est faux.
        assert_eq!(assess_album("Ipséité", None, &[]), AlbumVerdict::Keep);
    }
}
