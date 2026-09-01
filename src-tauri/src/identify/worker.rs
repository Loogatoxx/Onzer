//! Ouvrier d'identification.
//!
//! Jumeau de l'ouvrier d'analyse, avec une différence de nature : celui-ci
//! **dépend du réseau et de services tiers**. Il en tire trois règles.
//!
//! | Règle | Raison |
//! |---|---|
//! | Séparé de l'analyse | La recommandation doit fonctionner hors ligne ; l'identification non |
//! | Les mal tagués d'abord | C'est là que le gain est le plus visible pour l'utilisateur |
//! | Un échec réseau ne condamne pas un morceau | Il repasse en file, contrairement à un « non trouvé » qui est définitif |
//!
//! La distinction du dernier point est importante. « AcoustID ne connaît pas ce
//! morceau » est une réponse : la répéter chaque jour ne changerait rien et
//! gaspillerait du quota. « AcoustID est injoignable » n'en est pas une : il
//! faut réessayer plus tard.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::core::{PathResolver, Result};
use crate::db::{repository, settings};
use crate::library::importer;

use super::acoustid::AcoustIdClient;
use super::coverart::CoverArtClient;
use super::fingerprint;
use super::musicbrainz::MusicBrainzClient;
use super::tagger;
use super::verdict;

/// Clé de réglage où l'utilisateur dépose sa clé AcoustID.
pub const API_KEY_SETTING: &str = "acoustid_api_key";

/// Dernière erreur de service rencontrée.
///
/// # Pourquoi la conserver
///
/// Une jauge figée à 0/43 sans la moindre explication est le pire des
/// affichages : l'utilisateur ne peut ni comprendre ni agir. Conserver la
/// dernière erreur permet de lui dire « AcoustID a répondu 401 » plutôt que de
/// le laisser deviner.
pub const LAST_ERROR_SETTING: &str = "identification_last_error";

/// Note la dernière erreur, pour l'afficher dans l'interface.
async fn record_error(pool: &SqlitePool, message: String) {
    let _ = settings::set(pool, LAST_ERROR_SETTING, &message).await;
}

/// Efface l'erreur : le service répond de nouveau.
async fn clear_error(pool: &SqlitePool) {
    let _ = settings::set(pool, LAST_ERROR_SETTING, &Option::<String>::None).await;
}

/// Repos entre deux identifications.
///
/// Les limiteurs de chaque service imposent déjà leur cadence ; cette pause
/// supplémentaire garde une marge et évite d'occuper le réseau en continu.
const REST_BETWEEN_TRACKS: Duration = Duration::from_millis(600);

/// Attente quand il n'y a rien à faire, ou pas de clé.
const IDLE_INTERVAL: Duration = Duration::from_secs(30);

/// Plafond de la mise en sommeil après des échecs répétés.
///
/// Cinq minutes : assez long pour ne plus peser sur un service en difficulté,
/// assez court pour repartir seul dès que la situation se rétablit.
const MAX_BACKOFF_MULTIPLIER: u32 = 10;

/// Mise en sommeil après `failures` échecs de service consécutifs.
///
/// # Le défaut que ça corrige
///
/// Chaque requête isolée abandonne correctement sur une erreur 4xx — une clé
/// invalide, par exemple. Mais l'ouvrier repartait aussitôt sur le morceau
/// suivant, et donc sur la même erreur : **une clé mal saisie produisait un
/// martèlement continu du service**, exactement ce qui fait bannir.
///
/// La mise en sommeil croît avec les échecs et se réinitialise au premier
/// succès. L'ouvrier repart donc tout seul dès que la clé est corrigée.
fn cooldown(failures: u32) -> Duration {
    IDLE_INTERVAL * failures.clamp(1, MAX_BACKOFF_MULTIPLIER)
}

/// Issue d'une tentative, du point de vue de l'ouvrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attempt {
    /// Le morceau a reçu une réponse — identifié, introuvable ou en échec.
    /// Dans tous les cas, on peut passer au suivant.
    Settled,
    /// Le service n'a pas répondu. Insister immédiatement serait néfaste.
    ServiceUnavailable,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentificationProgress {
    pub identified: i64,
    pub pending: i64,
    /// Morceaux absents des bases publiques. Ce n'est pas un échec.
    pub not_found: i64,
    pub failed: i64,
    pub total: i64,
}

impl IdentificationProgress {
    pub fn is_complete(&self) -> bool {
        self.pending == 0
    }
}

pub async fn progress(pool: &SqlitePool) -> Result<IdentificationProgress> {
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
             COUNT(*),
             SUM(CASE WHEN identification_state = 'done'      THEN 1 ELSE 0 END),
             SUM(CASE WHEN identification_state = 'pending'   THEN 1 ELSE 0 END),
             SUM(CASE WHEN identification_state = 'not_found' THEN 1 ELSE 0 END),
             SUM(CASE WHEN identification_state = 'failed'    THEN 1 ELSE 0 END)
         FROM tracks WHERE deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await?;

    Ok(IdentificationProgress {
        total: row.0,
        identified: row.1,
        pending: row.2,
        not_found: row.3,
        failed: row.4,
    })
}

/// Démarre l'ouvrier. Ne retourne jamais.
pub fn spawn(pool: SqlitePool, paths: Arc<RwLock<PathResolver>>) {
    tauri::async_runtime::spawn(async move {
        let musicbrainz = match MusicBrainzClient::new() {
            Ok(client) => client,
            Err(error) => {
                tracing::error!(%error, "identification désactivée");
                return;
            }
        };

        let cover_art = CoverArtClient::new().ok();
        let mut acoustid: Option<(String, AcoustIdClient)> = None;
        let mut consecutive_failures = 0_u32;

        loop {
            let resolver = paths.read().await.clone();

            if !resolver.is_library_online() {
                tokio::time::sleep(IDLE_INTERVAL).await;
                continue;
            }

            // La clé est relue à chaque tour : l'utilisateur peut la saisir
            // pendant que l'application tourne, sans avoir à la redémarrer.
            let key: Option<String> = settings::get(&pool, API_KEY_SETTING).await.ok().flatten();

            let Some(key) = key.filter(|value| !value.trim().is_empty()) else {
                tokio::time::sleep(IDLE_INTERVAL).await;
                continue;
            };

            if acoustid.as_ref().is_none_or(|(current, _)| *current != key) {
                match AcoustIdClient::new(key.clone()) {
                    Ok(client) => acoustid = Some((key, client)),
                    Err(error) => {
                        tracing::warn!(%error, "clé AcoustID inutilisable");
                        tokio::time::sleep(IDLE_INTERVAL).await;
                        continue;
                    }
                }
            }

            let Some((_, client)) = acoustid.as_ref() else {
                tokio::time::sleep(IDLE_INTERVAL).await;
                continue;
            };

            match next_pending(&pool).await {
                Ok(Some((track_id, relative_path))) => {
                    let attempt = identify_one(
                        &pool,
                        &resolver,
                        client,
                        &musicbrainz,
                        cover_art.as_ref(),
                        track_id,
                        &relative_path,
                    )
                    .await;

                    match attempt {
                        Attempt::Settled => {
                            consecutive_failures = 0;
                            tokio::time::sleep(REST_BETWEEN_TRACKS).await;
                        }
                        Attempt::ServiceUnavailable => {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                            let pause = cooldown(consecutive_failures);

                            tracing::warn!(
                                consecutive_failures,
                                pause_s = pause.as_secs(),
                                "service d'identification en difficulté, mise en sommeil"
                            );
                            tokio::time::sleep(pause).await;
                        }
                    }
                }
                Ok(None) => tokio::time::sleep(IDLE_INTERVAL).await,
                Err(error) => {
                    tracing::warn!(%error, "file d'identification illisible");
                    tokio::time::sleep(IDLE_INTERVAL).await;
                }
            }
        }
    });
}

/// Prend le prochain morceau à identifier.
///
/// Les mal tagués passent devant : un morceau sans album est celui pour lequel
/// l'identification apporte le plus, et l'utilisateur en verra l'effet tout de
/// suite.
async fn next_pending(pool: &SqlitePool) -> Result<Option<(i64, String)>> {
    let candidate: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM tracks
         WHERE identification_state = 'pending'
           AND deleted_at IS NULL
           AND is_available = 1
         ORDER BY (album_id IS NULL) DESC, added_at DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(candidate)
}

/// Identifie un morceau, de l'empreinte au rangement.
async fn identify_one(
    pool: &SqlitePool,
    paths: &PathResolver,
    acoustid: &AcoustIdClient,
    musicbrainz: &MusicBrainzClient,
    cover_art: Option<&CoverArtClient>,
    track_id: i64,
    relative_path: &str,
) -> Attempt {
    let Ok(path) = importer::absolute_path(paths, relative_path) else {
        let _ = repository::mark_identification(pool, track_id, "failed").await;
        return Attempt::Settled;
    };

    // ── Empreinte ───────────────────────────────────────────────────────
    // Calcul intensif : sur un fil bloquant, comme l'analyse.
    let fingerprint = match tokio::task::spawn_blocking(move || fingerprint::fingerprint_file(&path))
        .await
    {
        Ok(Ok(fingerprint)) => fingerprint,
        Ok(Err(error)) => {
            tracing::warn!(track_id, %error, "empreinte impossible");
            let _ = repository::mark_identification(pool, track_id, "failed").await;
            return Attempt::Settled;
        }
        Err(error) => {
            tracing::warn!(track_id, %error, "empreinte interrompue");
            let _ = repository::mark_identification(pool, track_id, "failed").await;
            return Attempt::Settled;
        }
    };

    // ── AcoustID ────────────────────────────────────────────────────────
    let identification = match acoustid.lookup(&fingerprint).await {
        Ok(Some(identification)) => identification,
        Ok(None) => {
            // L'index ne reconnaît pas ce signal. C'est fréquent sur une source
            // vidéo : intro parlée, jingle, outro — le morceau est décalé au
            // point que l'empreinte ne colle plus.
            //
            // Le nom du fichier, lui, dit « Artiste - Titre ». C'est une
            // information de moindre qualité, mais ce n'est pas rien : on la
            // soumet au **même juge**, qui exigera la même corroboration.
            tracing::debug!(track_id, "inconnu des bases publiques, essai par le nom");

            return fallback_by_name(
                pool, paths, musicbrainz, cover_art, track_id, relative_path,
            )
            .await;
        }
        Err(error) => {
            // Panne réseau, service en difficulté ou clé invalide : le morceau
            // **reste en file**. L'ouvrier se met en sommeil plutôt que
            // d'enchaîner sur la même erreur.
            tracing::warn!(track_id, %error, "AcoustID indisponible, morceau conservé en file");
            record_error(pool, format!("AcoustID — {error}")).await;
            return Attempt::ServiceUnavailable;
        }
    };

    // ── MusicBrainz ─────────────────────────────────────────────────────
    let metadata = match musicbrainz.lookup(&identification.recording_mbid).await {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            let _ = repository::mark_identification(pool, track_id, "not_found").await;
            return Attempt::Settled;
        }
        Err(error) => {
            tracing::warn!(track_id, %error, "MusicBrainz indisponible, morceau conservé en file");
            record_error(pool, format!("MusicBrainz — {error}")).await;
            return Attempt::ServiceUnavailable;
        }
    };

    // ── Corroboration ───────────────────────────────────────────────────
    //
    // L'empreinte a désigné une fiche ; reste à savoir si cette fiche décrit
    // bien CE fichier. C'est l'étape qui manquait quand un morceau de Damso
    // s'est retrouvé étiqueté « carmen (Clip Officiel) » de Stromae.
    let evidence = match file_evidence(pool, track_id).await {
        Ok(evidence) => evidence,
        Err(error) => {
            tracing::warn!(track_id, %error, "tags d'origine illisibles");
            verdict::FileEvidence::default()
        }
    };

    let candidate = verdict::CandidateEvidence {
        title: metadata.title.clone(),
        artist: metadata.filing_artist().map(str::to_string),
        length_ms: metadata.length_ms,
        release_count: metadata.release_count,
        score: identification.score,
    };

    let decision = verdict::assess(&evidence, &candidate);

    if !decision.is_accepted() {
        // Refus définitif, et non « à réessayer » : la même empreinte donnera
        // la même réponse demain. Le morceau garde ses tags, et la raison du
        // refus est conservée pour être affichée.
        tracing::info!(
            track_id,
            propose = %metadata.title,
            raison = decision.note(),
            "identification refusée"
        );
        clear_error(pool).await;
        let _ = repository::mark_identification_rejected(pool, track_id, decision.note()).await;
        return Attempt::Settled;
    }

    // ── Pochette ────────────────────────────────────────────────────────
    // Facultative : son absence ne remet pas en cause l'identification.
    let cover = match cover_art {
        Some(client) => client
            .fetch_front(
                metadata.release_mbid.as_deref(),
                metadata.release_group_mbid.as_deref(),
            )
            .await
            .unwrap_or(None),
        None => None,
    };

    // ── Application ─────────────────────────────────────────────────────
    match tagger::apply(
        pool,
        paths,
        track_id,
        relative_path,
        &metadata,
        cover.as_deref(),
        &tagger::IdentificationTrace {
            score: identification.score,
            note: decision.note(),
        },
    )
    .await
    {
        Ok(applied) => {
            clear_error(pool).await;
            tracing::info!(
            track_id,
            titre = %metadata.title,
            artiste = metadata.filing_artist().unwrap_or("?"),
            score = identification.score,
            deplace = applied.moved,
                destination = %applied.relative_path,
                "morceau identifié"
            );
        }
        Err(error) => {
            tracing::warn!(track_id, %error, "identification non appliquée");
            let _ = repository::mark_identification(pool, track_id, "failed").await;
        }
    }

    Attempt::Settled
}

/// Plan B : chercher par le nom du fichier quand l'empreinte échoue.
///
/// # Pourquoi ce filet existe
///
/// L'empreinte acoustique échoue sur environ un fichier sur trois quand la
/// source est un clip vidéo : les intros parlées et les outros décalent le
/// signal au point que l'index ne reconnaît plus rien. Le morceau reste alors
/// sans pochette et sans album, alors que son nom de fichier dit exactement de
/// quoi il s'agit.
///
/// # Ce qui ne change pas
///
/// La corroboration. Une correspondance textuelle passe par le **même juge**
/// que l'acoustique : durée compatible, tags non contredits. Un filet de
/// sécurité qui accepterait n'importe quoi serait pire que pas de filet — il
/// écrirait de faux tags là où il n'y en avait aucun.
async fn fallback_by_name(
    pool: &SqlitePool,
    paths: &PathResolver,
    musicbrainz: &MusicBrainzClient,
    cover_art: Option<&CoverArtClient>,
    track_id: i64,
    relative_path: &str,
) -> Attempt {
    let evidence = file_evidence(pool, track_id).await.unwrap_or_default();

    let Some(title) = evidence.title.as_deref().filter(|t| !t.trim().is_empty()) else {
        let _ = repository::mark_identification(pool, track_id, "not_found").await;
        return Attempt::Settled;
    };

    let hits = match musicbrainz.search(evidence.artist.as_deref(), title).await {
        Ok(hits) => hits,
        Err(error) => {
            tracing::warn!(track_id, %error, "recherche par nom impossible");
            record_error(pool, format!("MusicBrainz — {error}")).await;
            return Attempt::ServiceUnavailable;
        }
    };

    // Le premier candidat que le juge accepte. Les résultats arrivent triés par
    // pertinence : descendre plus bas reviendrait à préférer un moins bon.
    for hit in hits {
        let candidate = verdict::CandidateEvidence {
            title: hit.title.clone(),
            artist: hit.artist.clone(),
            length_ms: hit.length_ms,
            // Une fiche trouvée par le texte a forcément une existence
            // discographique : c'est ainsi qu'elle a été indexée.
            release_count: 1,
            score: hit.score,
        };

        if !verdict::assess(&evidence, &candidate).is_accepted() {
            continue;
        }

        let Ok(Some(metadata)) = musicbrainz.lookup(&hit.recording_mbid).await else {
            continue;
        };

        let cover = match cover_art {
            Some(client) => client
                .fetch_front(
                    metadata.release_mbid.as_deref(),
                    metadata.release_group_mbid.as_deref(),
                )
                .await
                .unwrap_or(None),
            None => None,
        };

        let trace = tagger::IdentificationTrace {
            score: hit.score,
            note: "trouvé par le nom du fichier, l'empreinte ayant échoué",
        };

        match tagger::apply(pool, paths, track_id, relative_path, &metadata, cover.as_deref(), &trace)
            .await
        {
            Ok(applied) => {
                clear_error(pool).await;
                tracing::info!(
                    track_id,
                    titre = %metadata.title,
                    destination = %applied.relative_path,
                    "identifié par le nom du fichier"
                );
                return Attempt::Settled;
            }
            Err(error) => {
                tracing::warn!(track_id, %error, "identification par nom non appliquée");
                break;
            }
        }
    }

    clear_error(pool).await;
    let _ = repository::mark_identification(pool, track_id, "not_found").await;
    Attempt::Settled
}

/// Ce que le fichier annonçait avant toute réécriture.
///
/// Les colonnes `original_*` sont renseignées à l'import et **jamais touchées
/// ensuite** : c'est la seule mémoire de ce qu'était le morceau. Sur une
/// bibliothèque importée avant leur existence, elles sont vides, et l'on se
/// rabat sur les valeurs courantes — qui n'ont pas encore été réécrites
/// puisque l'identification est en cours.
async fn file_evidence(pool: &SqlitePool, track_id: i64) -> Result<verdict::FileEvidence> {
    let row: Option<(Option<String>, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT t.original_title,
                COALESCE(t.original_artist,
                         (SELECT a.name FROM track_artists ta
                            JOIN artists a ON a.id = ta.artist_id
                           WHERE ta.track_id = t.id AND ta.role = 'main'
                           ORDER BY ta.position LIMIT 1)),
                t.title,
                t.duration_ms
           FROM tracks t WHERE t.id = ?",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await?;

    let Some((original_title, artist, current_title, duration_ms)) = row else {
        return Ok(verdict::FileEvidence::default());
    };

    Ok(verdict::FileEvidence {
        title: original_title.or(Some(current_title)),
        artist,
        duration_ms,
    })
}

/// Remet toute la bibliothèque en file d'identification.
pub async fn requeue_all(pool: &SqlitePool) -> Result<u64> {
    let affected = sqlx::query(
        "UPDATE tracks SET identification_state = 'pending' WHERE deleted_at IS NULL",
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Remet en file les seuls morceaux qui avaient échoué.
///
/// Sans toucher aux « non trouvés » : eux ont eu leur réponse.
pub async fn retry_failed(pool: &SqlitePool) -> Result<u64> {
    let affected = sqlx::query(
        "UPDATE tracks SET identification_state = 'pending'
         WHERE identification_state = 'failed' AND deleted_at IS NULL",
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn base() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();
        (dir, pool)
    }

    async fn ajouter(pool: &SqlitePool, id: i64, avec_album: bool, etat: &str) {
        if avec_album {
            sqlx::query(
                "INSERT OR IGNORE INTO albums (id, title, normalized_title, created_at)
                 VALUES (1, 'Album', 'album', 0)",
            )
            .execute(pool)
            .await
            .unwrap();
        }

        sqlx::query(
            "INSERT INTO tracks (id, title, normalized_title, album_id, duration_ms,
                                 relative_path, file_size, content_hash, format,
                                 added_at, identification_state)
             VALUES (?, ?, ?, ?, 1000, ?, 1, ?, 'mp3', ?, ?)",
        )
        .bind(id)
        .bind(format!("Titre {id}"))
        .bind(format!("titre {id}"))
        .bind(avec_album.then_some(1_i64))
        .bind(format!("a/{id}.mp3"))
        .bind(format!("hash{id}"))
        .bind(id)
        .bind(etat)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn les_morceaux_mal_tagues_passent_en_premier() {
        // C'est là que l'identification apporte le plus, et l'utilisateur en
        // verra l'effet tout de suite.
        let (_dir, pool) = base().await;
        ajouter(&pool, 1, true, "pending").await; // a un album
        ajouter(&pool, 2, false, "pending").await; // n'en a pas

        let (id, _) = next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(id, 2);
    }

    #[tokio::test]
    async fn seuls_les_morceaux_en_attente_sont_pris() {
        let (_dir, pool) = base().await;
        ajouter(&pool, 1, false, "done").await;
        ajouter(&pool, 2, false, "not_found").await;
        ajouter(&pool, 3, false, "failed").await;

        assert!(next_pending(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn un_morceau_indisponible_nest_pas_tente() {
        let (_dir, pool) = base().await;
        ajouter(&pool, 1, false, "pending").await;
        sqlx::query("UPDATE tracks SET is_available = 0")
            .execute(&pool)
            .await
            .unwrap();

        assert!(next_pending(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn les_nouveaux_morceaux_sont_en_attente_par_defaut() {
        let (_dir, pool) = base().await;
        ajouter(&pool, 1, false, "pending").await;

        let progression = progress(&pool).await.unwrap();
        assert_eq!(progression.pending, 1);
        assert!(!progression.is_complete());
    }

    #[tokio::test]
    async fn un_reessai_ne_touche_pas_les_morceaux_inconnus() {
        // « Inconnu des bases » est une réponse définitive : la répéter
        // gaspillerait du quota sans rien changer.
        let (_dir, pool) = base().await;
        ajouter(&pool, 1, false, "failed").await;
        ajouter(&pool, 2, false, "not_found").await;
        ajouter(&pool, 3, false, "done").await;

        assert_eq!(retry_failed(&pool).await.unwrap(), 1);

        let progression = progress(&pool).await.unwrap();
        assert_eq!(progression.pending, 1);
        assert_eq!(progression.not_found, 1);
        assert_eq!(progression.identified, 1);
    }

    #[tokio::test]
    async fn une_remise_en_file_complete_reprend_tout() {
        let (_dir, pool) = base().await;
        for (id, etat) in [(1, "done"), (2, "not_found"), (3, "failed")] {
            ajouter(&pool, id, false, etat).await;
        }

        assert_eq!(requeue_all(&pool).await.unwrap(), 3);
        assert_eq!(progress(&pool).await.unwrap().pending, 3);
    }

    #[test]
    fn la_mise_en_sommeil_croit_avec_les_echecs() {
        // Sans elle, une clé mal saisie produirait un martèlement continu du
        // service — le meilleur moyen de se faire bannir.
        assert!(cooldown(2) > cooldown(1));
        assert!(cooldown(5) > cooldown(2));
    }

    #[test]
    fn la_mise_en_sommeil_reste_plafonnee() {
        // Assez long pour ne plus peser, assez court pour repartir seul dès
        // que la situation se rétablit.
        let plafond = cooldown(1_000);

        assert_eq!(plafond, cooldown(MAX_BACKOFF_MULTIPLIER));
        assert!(plafond <= Duration::from_secs(600));
    }

    #[test]
    fn le_premier_echec_attend_deja_un_peu() {
        assert!(cooldown(0) >= IDLE_INTERVAL);
        assert!(cooldown(1) >= IDLE_INTERVAL);
    }

    #[tokio::test]
    async fn la_progression_distingue_lechec_de_labsence() {
        let (_dir, pool) = base().await;
        ajouter(&pool, 1, false, "done").await;
        ajouter(&pool, 2, false, "not_found").await;
        ajouter(&pool, 3, false, "failed").await;
        ajouter(&pool, 4, false, "pending").await;

        let progression = progress(&pool).await.unwrap();

        assert_eq!(progression.total, 4);
        assert_eq!(progression.identified, 1);
        assert_eq!(progression.not_found, 1);
        assert_eq!(progression.failed, 1);
        assert_eq!(progression.pending, 1);
    }
}
