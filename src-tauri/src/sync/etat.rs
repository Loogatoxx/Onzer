//! Lire l'état local, et lui appliquer ce qu'une fusion a décidé.
//!
//! C'est la seule moitié de la synchronisation qui touche la base. Toute la
//! logique de décision vit dans [`super::fusion`], où elle se teste sans base
//! ni réseau.

use sqlx::SqlitePool;

use crate::core::{now_ms, Result};

use super::fusion::{Arbitrage, Changement, EtatSync, MorceauSync, PlaylistSync};

/// Le nom sous lequel cet appareil se présente à l'autre.
///
/// Il n'a aucune valeur d'identité — rien n'en dépend, personne ne s'y fie pour
/// autoriser quoi que ce soit. Il sert à écrire « repris de Honor » dans le
/// journal, pour qu'un arbitrage relu trois semaines plus tard veuille dire
/// quelque chose.
pub fn nom_appareil() -> String {
    #[cfg(target_os = "android")]
    {
        "Téléphone".to_string()
    }

    #[cfg(not(target_os = "android"))]
    {
        std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
            .ok()
            .and_then(|sortie| String::from_utf8(sortie.stdout).ok())
            .map(|nom| nom.trim().to_string())
            .filter(|nom| !nom.is_empty())
            .unwrap_or_else(|| "Ordinateur".to_string())
    }
}

/// Tout ce que cet appareil expose.
///
/// # Pourquoi les paroles voyagent en entier
///
/// Sur la bibliothèque de référence, mille neuf cent seize morceaux en portent,
/// pour cinq mégaoctets et demi — une seconde de Wi-Fi. Un protocole en deux
/// temps, qui n'enverrait que les manquantes, économiserait quelques secondes
/// au prix d'un aller-retour de plus et d'un état intermédiaire à tenir. Le
/// jour où la bibliothèque en portera cent fois plus, il faudra y revenir ;
/// aujourd'hui ce serait de la complexité sans contrepartie.
pub async fn lire(pool: &SqlitePool) -> Result<EtatSync> {
    let lignes = sqlx::query_as::<_, LigneMorceau>(
        "SELECT t.relative_path, t.title,
                (SELECT a.name FROM track_artists ta
                   JOIN artists a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'main'
                  ORDER BY ta.position LIMIT 1) AS artist,
                al.title AS album, t.duration_ms, t.file_size, t.is_loved, t.loved_at, t.lyrics
           FROM tracks t
      LEFT JOIN albums al ON al.id = t.album_id
          WHERE t.deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    let morceaux = lignes
        .into_iter()
        .map(|ligne| {
            // Une chaîne vide n'est pas des paroles : la traiter comme telles
            // ferait croire à l'autre appareil qu'il a quelque chose à
            // apporter, et remplacerait un texte par du vide.
            let paroles = ligne.lyrics.filter(|texte| !texte.trim().is_empty());

            // Les mêmes crochets d'horodatage que partout ailleurs : un `.lrc`
            // sans `[mm:ss]` est un texte, pas des paroles qui défilent.
            let synchronisees = paroles
                .as_deref()
                .is_some_and(|texte| texte.contains('[') && texte.contains(':'));

            MorceauSync {
                chemin: ligne.relative_path,
                titre: ligne.title,
                artiste: ligne.artist,
                album: ligne.album,
                duree_ms: ligne.duration_ms,
                taille: ligne.file_size,
                aime: ligne.is_loved,
                aime_le: ligne.loved_at,
                paroles,
                paroles_synchronisees: synchronisees,
            }
        })
        .collect();

    Ok(EtatSync {
        appareil: nom_appareil(),
        morceaux,
        playlists: lire_playlists(pool).await?,
    })
}

#[derive(sqlx::FromRow)]
struct LigneMorceau {
    relative_path: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: i64,
    file_size: i64,
    is_loved: bool,
    loved_at: Option<i64>,
    lyrics: Option<String>,
}

async fn lire_playlists(pool: &SqlitePool) -> Result<Vec<PlaylistSync>> {
    let entetes = sqlx::query_as::<_, (i64, String, i64)>(
        "SELECT id, name, updated_at FROM playlists WHERE kind = 'manual' ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let mut playlists = Vec::with_capacity(entetes.len());

    for (id, nom, modifiee_le) in entetes {
        let morceaux = sqlx::query_scalar::<_, String>(
            "SELECT t.relative_path
               FROM playlist_tracks pt
               JOIN tracks t ON t.id = pt.track_id
              WHERE pt.playlist_id = ? AND t.deleted_at IS NULL
           ORDER BY pt.position",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        playlists.push(PlaylistSync {
            nom,
            modifiee_le,
            morceaux,
        });
    }

    Ok(playlists)
}

/// Applique les changements, et consigne les arbitrages.
///
/// # Pourquoi une transaction
///
/// Une fusion interrompue à mi-chemin — le Wi-Fi coupe, l'application est
/// tuée — laisserait une bibliothèque à moitié fusionnée, sans moyen de savoir
/// laquelle. Ou tout, ou rien.
pub async fn appliquer(
    pool: &SqlitePool,
    appareil: &str,
    changements: &[Changement],
    arbitrages: &[Arbitrage],
) -> Result<usize> {
    let mut transaction = pool.begin().await?;
    let maintenant = now_ms();
    let mut appliques = 0;

    for changement in changements {
        match changement {
            Changement::Aime {
                chemin,
                aime,
                quand,
            } => {
                let touches = sqlx::query(
                    "UPDATE tracks SET is_loved = ?, loved_at = ?
                      WHERE relative_path = ? AND deleted_at IS NULL",
                )
                .bind(*aime)
                .bind(quand.unwrap_or(maintenant))
                .bind(chemin)
                .execute(&mut *transaction)
                .await?
                .rows_affected();

                appliques += touches as usize;
            }

            Changement::Paroles {
                chemin, paroles, ..
            } => {
                let touches = sqlx::query(
                    "UPDATE tracks SET lyrics = ? WHERE relative_path = ? AND deleted_at IS NULL",
                )
                .bind(paroles)
                .bind(chemin)
                .execute(&mut *transaction)
                .await?
                .rows_affected();

                appliques += touches as usize;
            }

            Changement::Playlist {
                nom,
                morceaux,
                quand,
            } => {
                let id: i64 = match sqlx::query_scalar::<_, i64>(
                    "SELECT id FROM playlists WHERE name = ? AND kind = 'manual'",
                )
                .bind(nom)
                .fetch_optional(&mut *transaction)
                .await?
                {
                    Some(existant) => existant,
                    None => {
                        sqlx::query_scalar::<_, i64>(
                            "INSERT INTO playlists (name, kind, created_at, updated_at)
                             VALUES (?, 'manual', ?, ?) RETURNING id",
                        )
                        .bind(nom)
                        .bind(maintenant)
                        .bind(*quand)
                        .fetch_one(&mut *transaction)
                        .await?
                    }
                };

                // La composition est réécrite d'un bloc : la fusion a déjà
                // décidé de l'ordre **complet**, et appliquer des différences
                // ligne à ligne dans une table dont la clé primaire est
                // (playlist, position) est une source de collisions inutile.
                sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?")
                    .bind(id)
                    .execute(&mut *transaction)
                    .await?;

                for (position, chemin) in morceaux.iter().enumerate() {
                    sqlx::query(
                        "INSERT INTO playlist_tracks (playlist_id, track_id, position, added_at)
                         SELECT ?, t.id, ?, ?
                           FROM tracks t
                          WHERE t.relative_path = ? AND t.deleted_at IS NULL",
                    )
                    .bind(id)
                    .bind(position as i64)
                    .bind(maintenant)
                    .bind(chemin)
                    .execute(&mut *transaction)
                    .await?;
                }

                sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
                    .bind(*quand)
                    .bind(id)
                    .execute(&mut *transaction)
                    .await?;

                appliques += 1;
            }
        }
    }

    for arbitrage in arbitrages {
        sqlx::query(
            "INSERT INTO sync_journal (at, pair, kind, subject, replaced, kept)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(maintenant)
        .bind(appareil)
        .bind(arbitrage.genre)
        .bind(&arbitrage.sujet)
        .bind(&arbitrage.remplace)
        .bind(&arbitrage.garde)
        .execute(&mut *transaction)
        .await?;
    }

    transaction.commit().await?;
    Ok(appliques)
}
