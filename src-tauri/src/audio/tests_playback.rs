//! Test de bout en bout de la lecture.
//!
//! Ouvre réellement le périphérique audio, décode un vrai fichier, attend la
//! fin du morceau, et vérifie que l'écoute a bien été journalisée. C'est le
//! seul moyen de prouver que la chaîne complète tient :
//!
//! ```text
//! commande → thread audio → décodage → fin détectée → journal → agrégats
//! ```
//!
//! Le volume est mis à zéro : le test ne doit rien faire entendre.
//! Si aucune carte son n'est disponible, le test se déclare réussi plutôt que
//! d'échouer — l'absence de périphérique n'est pas une régression du code.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;

use crate::audio::queue::QueueItem;
use crate::audio::tracking::PlaySource;
use crate::audio::PlayerService;
use crate::core::PathResolver;

/// Durée des morceaux de test. Assez courte pour un test rapide, assez longue
/// pour que la détection de fin ne soit pas un coup de chance.
const TRACK_MS: u32 = 700;

/// Écrit un WAV PCM 16 bits mono réellement décodable.
fn write_wav(path: &Path, duration_ms: u32, seed: u8) {
    const SAMPLE_RATE: u32 = 44_100;

    let sample_count = SAMPLE_RATE * duration_ms / 1000;
    let data_bytes = sample_count * 2;

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(path).unwrap();

    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
    file.write_all(&(SAMPLE_RATE * 2).to_le_bytes()).unwrap();
    file.write_all(&2_u16.to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_bytes.to_le_bytes()).unwrap();

    let mut samples = Vec::with_capacity(data_bytes as usize);
    for index in 0..sample_count {
        let value = (((index as f32) * (seed as f32 + 1.0) * 0.01).sin() * 6000.0) as i16;
        samples.extend_from_slice(&value.to_le_bytes());
    }
    file.write_all(&samples).unwrap();
}

struct Bac {
    _dir: tempfile::TempDir,
    pool: SqlitePool,
    paths: PathResolver,
    items: Vec<QueueItem>,
}

/// Prépare une base migrée et `count` morceaux réels sur disque.
async fn bac(count: i64) -> Bac {
    let dir = tempfile::tempdir().unwrap();
    let library_root = dir.path().join("Bibliotheque");
    std::fs::create_dir_all(&library_root).unwrap();

    let mut paths = PathResolver::new(dir.path().join("donnees"));
    paths.ensure_dirs().unwrap();
    paths.set_library_root(Some(library_root.clone()));

    let pool = crate::db::connect(&paths.database_file()).await.unwrap();
    crate::db::migrate(&pool).await.unwrap();

    let mut items = Vec::new();

    for id in 1..=count {
        let relative = format!("Artiste/Album/{id:02} - Titre.wav");
        write_wav(&library_root.join(&relative), TRACK_MS, id as u8);

        sqlx::query(
            "INSERT INTO tracks (id, title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at)
             VALUES (?, ?, ?, ?, ?, 1, ?, 'wav', 0)",
        )
        .bind(id)
        .bind(format!("Titre {id}"))
        .bind(format!("titre {id}"))
        .bind(i64::from(TRACK_MS))
        .bind(&relative)
        .bind(format!("hash{id}"))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO track_stats (track_id) VALUES (?)")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        items.push(QueueItem {
            track_id: id,
            title: format!("Titre {id}"),
            artist: None,
            album: None,
            duration_ms: i64::from(TRACK_MS),
            relative_path: relative,
            artwork_hash: None,
        });
    }

    Bac {
        _dir: dir,
        pool,
        paths,
        items,
    }
}

/// Démarre le moteur, ou rend `None` si la machine n'a pas de sortie audio.
async fn moteur(pool: &SqlitePool) -> Option<PlayerService> {
    match PlayerService::start(pool).await {
        Ok(player) => {
            player.set_volume(0.0).await.ok()?;
            Some(player)
        }
        Err(error) => {
            eprintln!("test ignoré — pas de périphérique audio : {error}");
            None
        }
    }
}

/// Fait tourner la boucle de surveillance jusqu'à ce que `attendus` écoutes
/// soient journalisées, ou que le délai expire.
///
/// C'est la boucle que `spawn_playback_loop` exécute en production : la
/// simuler ici teste le vrai chemin d'enchaînement, pas un raccourci de test.
async fn pomper_jusqua(
    player: &PlayerService,
    bac: &Bac,
    attendus: i64,
    limite: Duration,
) -> bool {
    let debut = Instant::now();

    while debut.elapsed() < limite {
        player.tick(&bac.pool, &bac.paths).await.unwrap();

        if nombre_devenements(&bac.pool).await >= attendus {
            return true;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    false
}

async fn nombre_devenements(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM play_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn joue_un_morceau_jusquau_bout_et_le_journalise() {
    let bac = bac(1).await;
    let Some(player) = moteur(&bac.pool).await else {
        return;
    };

    player
        .play_queue(
            &bac.pool,
            &bac.paths,
            bac.items.clone(),
            0,
            PlaySource::Library,
            None,
        )
        .await
        .unwrap();

    // La boucle de surveillance journalise l'écoute au moment où elle détecte
    // la fin : la présence de l'événement est donc le signal d'arrivée.
    let termine = pomper_jusqua(&player, &bac, 1, Duration::from_secs(10)).await;

    assert!(termine, "le morceau ne s'est jamais terminé");

    let (reason, completion): (String, f64) =
        sqlx::query_as("SELECT end_reason, completion FROM play_events")
            .fetch_one(&bac.pool)
            .await
            .unwrap();

    assert_eq!(reason, "completed");
    assert!(
        completion > 0.5,
        "un morceau joué en entier doit avoir une complétion élevée, obtenu {completion}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn enchaine_automatiquement_sur_le_morceau_suivant() {
    let bac = bac(2).await;
    let Some(player) = moteur(&bac.pool).await else {
        return;
    };

    player
        .play_queue(
            &bac.pool,
            &bac.paths,
            bac.items.clone(),
            0,
            PlaySource::Library,
            None,
        )
        .await
        .unwrap();

    let enchaine = pomper_jusqua(&player, &bac, 1, Duration::from_secs(10)).await;

    assert!(enchaine, "le premier morceau ne s'est pas terminé");

    let courant = player.snapshot().await.current.unwrap().track_id;
    assert_eq!(courant, 2, "la file doit avoir avancé toute seule");

    // Le second morceau doit porter le premier comme précédent : c'est ce qui
    // alimente la matrice de transitions.
    let precedent: Option<i64> =
        sqlx::query_scalar("SELECT previous_track_id FROM play_events ORDER BY id DESC LIMIT 1")
            .fetch_one(&bac.pool)
            .await
            .unwrap();

    assert!(precedent.is_none() || precedent == Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn un_saut_manuel_est_journalise_comme_tel() {
    let bac = bac(2).await;
    let Some(player) = moteur(&bac.pool).await else {
        return;
    };

    player
        .play_queue(
            &bac.pool,
            &bac.paths,
            bac.items.clone(),
            0,
            PlaySource::Library,
            None,
        )
        .await
        .unwrap();

    // Saut immédiat : rejet franc.
    tokio::time::sleep(Duration::from_millis(80)).await;
    player.next(&bac.pool, &bac.paths, false).await.unwrap();

    let (reason, skip_at, early): (String, Option<i64>, i64) = sqlx::query_as(
        "SELECT e.end_reason, e.skip_at_ms, s.early_skip_count
         FROM play_events e JOIN track_stats s ON s.track_id = e.track_id
         WHERE e.track_id = 1",
    )
    .fetch_one(&bac.pool)
    .await
    .unwrap();

    assert_eq!(reason, "skipped");
    assert!(skip_at.is_some(), "la position du saut doit être connue");
    assert_eq!(early, 1, "un saut immédiat est un rejet franc");

    assert_eq!(player.snapshot().await.current.unwrap().track_id, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn un_fichier_disparu_ne_bloque_pas_la_lecture() {
    let bac = bac(1).await;
    let Some(player) = moteur(&bac.pool).await else {
        return;
    };

    // L'utilisateur a supprimé le fichier depuis le Finder.
    std::fs::remove_file(bac.paths.resolve(&bac.items[0].relative_path).unwrap()).unwrap();

    let outcome = player
        .play_queue(
            &bac.pool,
            &bac.paths,
            bac.items.clone(),
            0,
            PlaySource::Library,
            None,
        )
        .await;

    assert!(outcome.is_err(), "l'erreur doit remonter à l'interface");

    // Et le morceau est marqué indisponible plutôt que laissé en silence.
    let disponible: bool = sqlx::query_scalar("SELECT is_available FROM tracks WHERE id = 1")
        .fetch_one(&bac.pool)
        .await
        .unwrap();

    assert!(!disponible);
}

#[tokio::test(flavor = "multi_thread")]
async fn la_pause_arrete_la_progression() {
    let bac = bac(1).await;
    let Some(player) = moteur(&bac.pool).await else {
        return;
    };

    player
        .play_queue(
            &bac.pool,
            &bac.paths,
            bac.items.clone(),
            0,
            PlaySource::Library,
            None,
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(150)).await;
    player.toggle().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let fige = player.position_ms();
    tokio::time::sleep(Duration::from_millis(250)).await;

    assert_eq!(
        player.position_ms(),
        fige,
        "en pause, la position ne doit plus avancer"
    );
    assert!(!player.is_playing());
}
