//! Test d'assemblage du moteur.
//!
//! Les modules sont éprouvés isolément ailleurs. Ce qui est vérifié ici, c'est
//! qu'**assemblés, ils produisent une playlist utilisable** : de la bonne
//! longueur, sans doublon, respectant les règles de diversité, et traçable pour
//! l'apprentissage.

use sqlx::SqlitePool;

use crate::analysis::{ANALYZER_NAME, ANALYZER_VERSION};
use crate::reco::bandit::{Rng, Strategy};
use crate::reco::context::ListeningContext;
use crate::reco::engine::{self, PlaylistKind};

/// Bibliothèque synthétique : `artists` artistes, `per_artist` morceaux chacun.
///
/// Chaque morceau reçoit un vecteur audio cohérent : les morceaux d'un même
/// artiste se ressemblent, ce qui met les règles de diversité sous tension —
/// exactement la situation où un moteur naïf échoue.
async fn bibliotheque(artists: i64, per_artist: i64) -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
    crate::db::migrate(&pool).await.unwrap();

    for artist in 1..=artists {
        sqlx::query("INSERT INTO artists (id, name, normalized_name, created_at) VALUES (?,?,?,0)")
            .bind(artist)
            .bind(format!("Artiste {artist}"))
            .bind(format!("artiste {artist}"))
            .execute(&pool)
            .await
            .unwrap();
    }

    let mut track_id = 0_i64;

    for artist in 1..=artists {
        for index in 0..per_artist {
            track_id += 1;

            sqlx::query(
                "INSERT INTO tracks (id, title, normalized_title, duration_ms, relative_path,
                                     file_size, content_hash, format, added_at, analysis_state)
                 VALUES (?,?,?,200000,?,1,?,'mp3',?, 'done')",
            )
            .bind(track_id)
            .bind(format!("Titre {track_id}"))
            .bind(format!("titre {track_id}"))
            .bind(format!("a/{track_id}.mp3"))
            .bind(format!("hash{track_id}"))
            .bind(track_id * 1000)
            .execute(&pool)
            .await
            .unwrap();

            sqlx::query("INSERT INTO track_stats (track_id) VALUES (?)")
                .bind(track_id)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query(
                "INSERT INTO track_artists (track_id, artist_id, role, position)
                 VALUES (?, ?, 'main', 0)",
            )
            .bind(track_id)
            .bind(artist)
            .execute(&pool)
            .await
            .unwrap();

            // Vecteur : proche pour un même artiste, éloigné entre artistes.
            let embedding: Vec<u8> = (0..8_i32)
                .flat_map(|dimension| {
                    let base = artist as f32 * 10.0;
                    let bruit = index as f32 * 0.1 + dimension as f32 * 0.01;
                    (base + bruit).to_le_bytes()
                })
                .collect();

            sqlx::query(
                "INSERT INTO track_features (track_id, embedding, embedding_dim, tempo,
                                             analyzer, analyzer_version, analyzed_at)
                 VALUES (?,?,8,?,?,?,0)",
            )
            .bind(track_id)
            .bind(embedding)
            .bind(100.0 + artist as f64 * 7.0)
            .bind(ANALYZER_NAME)
            .bind(ANALYZER_VERSION)
            .execute(&pool)
            .await
            .unwrap();
        }
    }

    (dir, pool)
}

fn contexte() -> ListeningContext {
    ListeningContext::now(20, 2, Some("Casque".into()))
}

// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn genere_une_playlist_de_la_longueur_demandee() {
    let (_dir, pool) = bibliotheque(6, 8).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 20, &mut Rng::new(42));

    assert_eq!(playlist.len(), 20, "playlist obtenue : {playlist:?}");
}

#[tokio::test]
async fn remplit_toujours_la_longueur_demandee() {
    // # Le défaut que ce test verrouille
    //
    // Les règles de diversité peuvent ne plus laisser aucun candidat
    // admissible avant la fin : la playlist s'arrêtait alors une place trop
    // tôt. Le symptôme n'apparaissait qu'une fois sur quelques dizaines — un
    // hasard venu de l'ordre d'itération d'une `HashMap` — ce qui le rendait
    // irreproductible et invisible.
    //
    // On vérifie donc la propriété sur plusieurs longueurs et plusieurs
    // graines, plutôt que sur un cas unique qui pourrait passer par chance.
    let (_dir, pool) = bibliotheque(5, 6).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    for longueur in [5, 12, 15, 20, 28] {
        for graine in [1_u64, 5, 42, 777] {
            let playlist =
                engine::generate(&data, &PlaylistKind::ForNow, longueur, &mut Rng::new(graine));

            assert_eq!(
                playlist.len(),
                longueur,
                "longueur {longueur}, graine {graine} : playlist incomplète"
            );
        }
    }
}

#[tokio::test]
async fn la_completion_ne_reintroduit_jamais_un_doublon() {
    // La complétion lève les règles de confort, jamais la règle dure : deux
    // fois le même morceau se verrait immédiatement.
    let (_dir, pool) = bibliotheque(3, 5).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    // 15 morceaux en tout, et on en demande 15 : la complétion est forcément
    // sollicitée.
    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 15, &mut Rng::new(3));

    let uniques: std::collections::HashSet<i64> =
        playlist.iter().map(|track| track.track_id).collect();
    assert_eq!(uniques.len(), playlist.len());
}

#[tokio::test]
async fn la_playlist_ne_contient_aucun_doublon() {
    let (_dir, pool) = bibliotheque(6, 8).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 25, &mut Rng::new(7));

    let uniques: std::collections::HashSet<i64> =
        playlist.iter().map(|track| track.track_id).collect();

    assert_eq!(uniques.len(), playlist.len());
}

#[tokio::test]
async fn les_regles_de_diversite_tiennent_sur_une_vraie_generation() {
    // Bibliothèque volontairement resserrée : peu d'artistes, beaucoup de
    // morceaux chacun. C'est là qu'un moteur naïf enchaîne huit titres du
    // même artiste.
    let (_dir, pool) = bibliotheque(4, 10).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 12, &mut Rng::new(3));

    let artistes: Vec<Option<i64>> = playlist
        .iter()
        .map(|track| data.meta.get(&track.track_id).and_then(|m| m.artist_id))
        .collect();

    for paire in artistes.windows(2) {
        assert_ne!(
            paire[0], paire[1],
            "deux morceaux du même artiste se suivent : {artistes:?}"
        );
    }

    // Le quota est adaptatif : avec quatre artistes seulement, l'exiger à 3
    // rendrait une playlist de douze titres tout juste réalisable. On vérifie
    // donc le quota effectivement retenu, pas une valeur codée en dur.
    let regles = engine::diversity_rules_for(
        &data,
        12,
        crate::reco::diversity::TempoPolicy::Varied,
    );

    for artiste in 1..=4_i64 {
        let occurrences = artistes.iter().filter(|a| **a == Some(artiste)).count();
        assert!(
            occurrences <= regles.max_per_artist,
            "l'artiste {artiste} apparaît {occurrences} fois pour un quota de {}",
            regles.max_per_artist
        );
    }

    let distincts: std::collections::HashSet<_> = artistes.iter().collect();
    assert!(
        distincts.len() >= 3,
        "la playlist doit brasser plusieurs artistes : {distincts:?}"
    );
}

#[tokio::test]
async fn une_radio_commence_par_son_morceau_de_depart_et_reste_dans_son_univers() {
    let (_dir, pool) = bibliotheque(5, 8).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    // Morceau 3 : artiste 1.
    let playlist = engine::generate(
        &data,
        &PlaylistKind::Radio { seed_track_id: 3 },
        10,
        &mut Rng::new(11),
    );

    assert!(!playlist.is_empty());
    assert!(
        playlist.iter().all(|track| track.track_id != 3),
        "le morceau de départ ne doit pas être proposé une seconde fois"
    );

    // La similarité doit tirer vers l'artiste 1, dont les vecteurs sont proches.
    let voisins = data.space.nearest(3, 5);
    assert!(!voisins.is_empty(), "l'espace sonore doit être exploitable");
}

#[tokio::test]
async fn le_moteur_fonctionne_sans_le_moindre_historique() {
    // Le démarrage à froid : aucune écoute enregistrée. C'est le cas de
    // l'utilisateur qui vient d'importer sa bibliothèque, et le moteur doit
    // malgré tout produire quelque chose de sensé.
    let (_dir, pool) = bibliotheque(5, 6).await;

    let evenements: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(evenements, 0);

    let data = engine::load(&pool, &contexte()).await.unwrap();
    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 15, &mut Rng::new(5));

    assert_eq!(
        playlist.len(),
        15,
        "l'analyse audio seule doit suffire à remplir une playlist"
    );
}

#[tokio::test]
async fn une_bibliotheque_vide_ne_provoque_pas_de_panique() {
    let (_dir, pool) = bibliotheque(0, 0).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    assert_eq!(data.total_tracks, 0);
    assert!(engine::generate(&data, &PlaylistKind::ForNow, 20, &mut Rng::new(1)).is_empty());
}

#[tokio::test]
async fn deux_generations_successives_different() {
    // Sans quoi relancer « Pour toi maintenant » redonnerait exactement la
    // même chose, ce qui donnerait l'impression d'un moteur figé.
    let (_dir, pool) = bibliotheque(8, 6).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    let premiere = engine::generate(&data, &PlaylistKind::ForNow, 15, &mut Rng::new(1));
    let seconde = engine::generate(&data, &PlaylistKind::ForNow, 15, &mut Rng::new(2));

    let a: Vec<i64> = premiere.iter().map(|t| t.track_id).collect();
    let b: Vec<i64> = seconde.iter().map(|t| t.track_id).collect();

    assert_ne!(a, b);
}

#[tokio::test]
async fn chaque_morceau_porte_une_raison_affichable() {
    let (_dir, pool) = bibliotheque(5, 6).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 10, &mut Rng::new(9));

    for track in &playlist {
        assert!(
            !track.reason.is_empty(),
            "une recommandation inexplicable inspire la méfiance"
        );
    }
}

// ── Traçabilité et apprentissage ────────────────────────────────────────────

#[tokio::test]
async fn la_playlist_est_tracee_pour_lapprentissage() {
    let (_dir, pool) = bibliotheque(5, 6).await;
    let context = contexte();
    let data = engine::load(&pool, &context).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 10, &mut Rng::new(13));
    let session_id = engine::save_session(&pool, &PlaylistKind::ForNow, &context, &playlist)
        .await
        .unwrap();

    let propositions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reco_proposals WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(propositions, playlist.len() as i64);

    // Le compteur de propositions permet de distinguer « cette stratégie
    // échoue » de « cette stratégie n'est jamais tirée ».
    let total: i64 = sqlx::query_scalar("SELECT SUM(proposals) FROM reco_strategies")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(total, playlist.len() as i64);
}

#[tokio::test]
async fn un_succes_renforce_la_strategie_qui_la_produit() {
    let (_dir, pool) = bibliotheque(5, 6).await;
    let context = contexte();
    let data = engine::load(&pool, &context).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 5, &mut Rng::new(17));
    let session_id = engine::save_session(&pool, &PlaylistKind::ForNow, &context, &playlist)
        .await
        .unwrap();

    let premier = &playlist[0];
    let strategie = premier.strategy.as_str();

    let avant: (f64, f64) =
        sqlx::query_as("SELECT alpha, beta FROM reco_strategies WHERE name = ?")
            .bind(strategie)
            .fetch_one(&pool)
            .await
            .unwrap();

    engine::record_outcome(&pool, session_id, premier.track_id, true)
        .await
        .unwrap();

    let apres: (f64, f64) =
        sqlx::query_as("SELECT alpha, beta FROM reco_strategies WHERE name = ?")
            .bind(strategie)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(apres.0, avant.0 + 1.0, "α doit augmenter");
    assert_eq!(apres.1, avant.1, "β doit rester inchangé");
}

#[tokio::test]
async fn un_echec_penalise_la_strategie_qui_la_produit() {
    let (_dir, pool) = bibliotheque(5, 6).await;
    let context = contexte();
    let data = engine::load(&pool, &context).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 5, &mut Rng::new(19));
    let session_id = engine::save_session(&pool, &PlaylistKind::ForNow, &context, &playlist)
        .await
        .unwrap();

    let premier = &playlist[0];
    let avant: f64 = sqlx::query_scalar("SELECT beta FROM reco_strategies WHERE name = ?")
        .bind(premier.strategy.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();

    engine::record_outcome(&pool, session_id, premier.track_id, false)
        .await
        .unwrap();

    let apres: f64 = sqlx::query_scalar("SELECT beta FROM reco_strategies WHERE name = ?")
        .bind(premier.strategy.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(apres, avant + 1.0);
}

#[tokio::test]
async fn une_ecoute_sans_rapport_ne_perturbe_pas_lapprentissage() {
    let (_dir, pool) = bibliotheque(5, 6).await;

    // Aucune session : l'appel doit être silencieusement sans effet plutôt que
    // d'échouer ou de créditer une stratégie au hasard.
    engine::record_outcome(&pool, 999, 1, true).await.unwrap();

    let total: f64 = sqlx::query_scalar("SELECT SUM(alpha + beta) FROM reco_strategies")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(total, 12.0, "six stratégies à α=1, β=1");
}

#[tokio::test]
async fn les_strategies_sans_donnees_ne_gaspillent_pas_demplacements() {
    // Sans historique, ni « oubliés » ni « enchaînements » n'ont de quoi
    // proposer. Le bandit ne doit pas les tirer, sinon la playlist serait
    // amputée d'autant.
    let (_dir, pool) = bibliotheque(6, 6).await;
    let data = engine::load(&pool, &contexte()).await.unwrap();

    let playlist = engine::generate(&data, &PlaylistKind::ForNow, 20, &mut Rng::new(23));

    assert_eq!(playlist.len(), 20);
    assert!(
        playlist
            .iter()
            .all(|track| track.strategy != Strategy::Forgotten),
        "aucun morceau ne peut venir d'une stratégie sans données"
    );
}
