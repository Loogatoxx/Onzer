//! La synchronisation, du serveur au client, sur une vraie chaussette.
//!
//! # Pourquoi ce test existe malgré ceux de `fusion`
//!
//! Les tests de fusion prouvent que la **décision** est juste. Ils ne prouvent
//! rien du reste : que les deux bases se lisent, que le JSON traverse, que le
//! code est vérifié, que la transaction s'applique, et surtout que les deux
//! côtés convergent — chacun exécutant la fusion de son point de vue.
//!
//! C'est exactement la couture où les défauts se logent, et la seule façon de
//! l'éprouver est de la faire tourner en entier.

use sqlx::SqlitePool;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::appairage::EtatServeur;
use super::{appairage, client, etat};

/// # Pourquoi les tests se mettent en file
///
/// La porte est **unique** : un seul appairage à la fois, c'est le
/// comportement voulu. Deux tests qui l'ouvrent en parallèle se volent donc la
/// session l'un à l'autre, et échouent pour une raison qui n'a rien à voir avec
/// ce qu'ils vérifient.
static UN_A_LA_FOIS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Une base migrée, avec des morceaux dedans.
async fn base(dossier: &std::path::Path, morceaux: &[(&str, &str, &str, bool)]) -> SqlitePool {
    let pool = crate::db::connect(&dossier.join("onzer.db")).await.unwrap();
    crate::db::migrate(&pool).await.unwrap();

    for (index, (chemin, titre, artiste, aime)) in morceaux.iter().enumerate() {
        let id = index as i64 + 1;

        sqlx::query(
            "INSERT INTO tracks (id, title, normalized_title, duration_ms, relative_path,
                                 file_size, content_hash, format, added_at, is_loved)
             VALUES (?, ?, ?, 180000, ?, 1, ?, 'mp3', 0, ?)",
        )
        .bind(id)
        .bind(titre)
        .bind(titre.to_lowercase())
        .bind(chemin)
        .bind(format!("hash{id}"))
        .bind(*aime)
        .execute(&pool)
        .await
        .unwrap();

        // `created_at` est obligatoire : sans lui, l'insertion échoue en
        // silence sous `OR IGNORE`, et c'est la clé étrangère de la ligne
        // suivante qui vient se plaindre à sa place.
        sqlx::query(
            "INSERT OR IGNORE INTO artists (id, name, normalized_name, created_at)
             VALUES (?, ?, ?, 0)",
        )
        .bind(id)
        .bind(artiste)
        .bind(artiste.to_lowercase())
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO track_artists (track_id, artist_id, role, position) VALUES (?, ?, 'main', 0)",
        )
        .bind(id)
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }

    pool
}

/// Le serveur, avec un avertisseur qui compte ses appels.
fn serveur_de_test(
    pool: SqlitePool,
    dossier: &std::path::Path,
) -> (Arc<EtatServeur>, Arc<AtomicUsize>) {
    let avertis = Arc::new(AtomicUsize::new(0));
    let compteur = Arc::clone(&avertis);

    let mut paths = crate::core::PathResolver::new(dossier.to_path_buf());
    paths.set_library_root(Some(dossier.join("Musique")));

    let etat = Arc::new(EtatServeur {
        pool,
        paths: Arc::new(tokio::sync::RwLock::new(paths)),
        prevenir: Arc::new(move |_, _| {
            compteur.fetch_add(1, Ordering::SeqCst);
        }),
    });

    (etat, avertis)
}

async fn aime(pool: &SqlitePool, chemin: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT is_loved FROM tracks WHERE relative_path = ?")
        .bind(chemin)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn deux_bibliotheques_convergent() {
    let _file = UN_A_LA_FOIS.lock().await;
    let dossier_serveur = tempfile::tempdir().unwrap();
    let dossier_client = tempfile::tempdir().unwrap();

    // Le serveur aime le premier morceau, le client aime le second. Après
    // l'échange, les deux doivent aimer les deux : c'est tout l'intérêt.
    let serveur = base(
        dossier_serveur.path(),
        &[
            ("un.mp3", "Un", "A", true),
            ("deux.mp3", "Deux", "B", false),
        ],
    )
    .await;

    let client_pool = base(
        dossier_client.path(),
        &[
            ("un.mp3", "Un", "A", false),
            ("deux.mp3", "Deux", "B", true),
        ],
    )
    .await;

    let (etat_serveur, avertis) = serveur_de_test(serveur.clone(), dossier_serveur.path());
    let infos = appairage::ouvrir_sur(etat_serveur, 0).await.unwrap();

    let rapport = client::synchroniser(&client_pool, "127.0.0.1", infos.port, &infos.code)
        .await
        .unwrap();

    assert_eq!(rapport.favoris, 1, "le client reprend le favori du serveur");
    assert_eq!(
        avertis.load(Ordering::SeqCst),
        1,
        "le serveur doit prévenir son interface : sa base vient de changer sans qu'il l'ait demandé"
    );

    assert!(aime(&serveur, "un.mp3").await);
    assert!(aime(&serveur, "deux.mp3").await, "le serveur a pris celui du client");
    assert!(aime(&client_pool, "un.mp3").await, "le client a pris celui du serveur");
    assert!(aime(&client_pool, "deux.mp3").await);

    // Refaire l'échange ne doit plus rien produire.
    let second = client::synchroniser(&client_pool, "127.0.0.1", infos.port, &infos.code)
        .await
        .unwrap();
    assert_eq!(second.favoris, 0, "la synchronisation doit être stable");

    appairage::fermer();
}

#[tokio::test(flavor = "multi_thread")]
async fn un_code_faux_est_refuse() {
    let _file = UN_A_LA_FOIS.lock().await;
    let dossier_serveur = tempfile::tempdir().unwrap();
    let dossier_client = tempfile::tempdir().unwrap();

    let serveur = base(dossier_serveur.path(), &[("un.mp3", "Un", "A", true)]).await;
    let client_pool = base(dossier_client.path(), &[("un.mp3", "Un", "A", false)]).await;

    let (etat_serveur, _avertis) = serveur_de_test(serveur.clone(), dossier_serveur.path());
    let infos = appairage::ouvrir_sur(etat_serveur, 0).await.unwrap();

    let erreur = client::synchroniser(&client_pool, "127.0.0.1", infos.port, "00000000")
        .await
        .unwrap_err();

    assert!(
        erreur.to_string().contains("code"),
        "le message doit dire ce qui ne va pas : {erreur}"
    );
    assert!(
        !aime(&client_pool, "un.mp3").await,
        "rien ne doit avoir traversé"
    );

    appairage::fermer();
}

#[tokio::test(flavor = "multi_thread")]
async fn la_porte_fermee_ne_repond_plus() {
    let _file = UN_A_LA_FOIS.lock().await;
    let dossier_serveur = tempfile::tempdir().unwrap();
    let dossier_client = tempfile::tempdir().unwrap();

    let serveur = base(dossier_serveur.path(), &[("un.mp3", "Un", "A", true)]).await;
    let client_pool = base(dossier_client.path(), &[("un.mp3", "Un", "A", false)]).await;

    let (etat_serveur, _avertis) = serveur_de_test(serveur, dossier_serveur.path());
    let infos = appairage::ouvrir_sur(etat_serveur, 0).await.unwrap();
    assert!(appairage::ouverte());

    appairage::fermer();
    assert!(!appairage::ouverte());

    // Le temps que la chaussette se libère.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(
        client::synchroniser(&client_pool, "127.0.0.1", infos.port, &infos.code)
            .await
            .is_err(),
        "une porte fermée ne doit rien accepter"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn les_paroles_traversent() {
    let _file = UN_A_LA_FOIS.lock().await;
    let dossier_serveur = tempfile::tempdir().unwrap();
    let dossier_client = tempfile::tempdir().unwrap();

    let serveur = base(dossier_serveur.path(), &[("un.mp3", "Un", "A", false)]).await;
    let client_pool = base(dossier_client.path(), &[("un.mp3", "Un", "A", false)]).await;

    sqlx::query("UPDATE tracks SET lyrics = ? WHERE relative_path = 'un.mp3'")
        .bind("[00:12.00] une ligne")
        .execute(&serveur)
        .await
        .unwrap();

    let (etat_serveur, _avertis) = serveur_de_test(serveur, dossier_serveur.path());
    let infos = appairage::ouvrir_sur(etat_serveur, 0).await.unwrap();

    let rapport = client::synchroniser(&client_pool, "127.0.0.1", infos.port, &infos.code)
        .await
        .unwrap();

    assert_eq!(rapport.paroles, 1);

    let recues: Option<String> =
        sqlx::query_scalar("SELECT lyrics FROM tracks WHERE relative_path = 'un.mp3'")
            .fetch_one(&client_pool)
            .await
            .unwrap();

    assert_eq!(recues.as_deref(), Some("[00:12.00] une ligne"));

    // Et l'état complet se relit sans erreur après application.
    etat::lire(&client_pool).await.unwrap();

    appairage::fermer();
}

#[tokio::test(flavor = "multi_thread")]
async fn un_fichier_connu_se_telecharge_et_un_autre_non() {
    let _file = UN_A_LA_FOIS.lock().await;
    let dossier_serveur = tempfile::tempdir().unwrap();

    let serveur = base(dossier_serveur.path(), &[("Un/un.mp3", "Un", "A", false)]).await;

    // Le fichier existe vraiment, à sa place dans la bibliothèque.
    let racine = dossier_serveur.path().join("Musique");
    std::fs::create_dir_all(racine.join("Un")).unwrap();
    std::fs::write(racine.join("Un/un.mp3"), b"des octets").unwrap();

    let (etat_serveur, _avertis) = serveur_de_test(serveur, dossier_serveur.path());
    let infos = appairage::ouvrir_sur(etat_serveur, 0).await.unwrap();

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/sync/v1/fichier", infos.port);

    let bon = client
        .get(format!("{url}?chemin=Un%2Fun.mp3"))
        .bearer_auth(&infos.code)
        .send()
        .await
        .unwrap();
    assert!(bon.status().is_success());
    assert_eq!(bon.bytes().await.unwrap().as_ref(), b"des octets");

    // Un chemin que la base ne connaît pas ne sort pas d'ici, même s'il
    // existe sur le disque : c'est la bibliothèque qui borne ce qu'on sert.
    let inconnu = client
        .get(format!("{url}?chemin=Un%2Fautre.mp3"))
        .bearer_auth(&infos.code)
        .send()
        .await
        .unwrap();
    assert_eq!(inconnu.status(), reqwest::StatusCode::NOT_FOUND);

    // Et sans le code, rien du tout.
    let sans_code = client.get(format!("{url}?chemin=Un%2Fun.mp3")).send().await.unwrap();
    assert_eq!(sans_code.status(), reqwest::StatusCode::UNAUTHORIZED);

    appairage::fermer();
}
