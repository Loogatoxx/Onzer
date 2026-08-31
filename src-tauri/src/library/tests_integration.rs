//! Tests de bout en bout de l'import.
//!
//! Ces tests fabriquent de **vrais fichiers audio** (WAV PCM valides, tagués
//! avec lofty), les importent, puis vérifient à la fois le disque et la base.
//! C'est la seule façon de prouver que la chaîne complète tient : lecture des
//! tags, rangement, déplacement, dédoublonnage, indexation et recherche.

use std::io::Write;
use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};
use sqlx::SqlitePool;

use crate::core::PathResolver;
use crate::db::{self, repository};
use crate::library::importer::{self, FileHandling, ImportOutcome};
use crate::library::scanner;

// ════════════════════════════════════════════════════════════════════════════
//  Fabrique de fichiers de test
// ════════════════════════════════════════════════════════════════════════════

/// Écrit un WAV PCM 16 bits mono réellement décodable.
///
/// `seed` fait varier le contenu audio : deux fichiers de seeds différentes
/// ont des empreintes différentes, ce qui permet de tester le dédoublonnage.
fn write_wav(path: &Path, duration_ms: u32, seed: u8) {
    const SAMPLE_RATE: u32 = 44_100;

    let sample_count = SAMPLE_RATE * duration_ms / 1000;
    let data_bytes = sample_count * 2;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut file = std::fs::File::create(path).unwrap();

    // En-tête RIFF/WAVE canonique (44 octets).
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap(); // taille du bloc fmt
    file.write_all(&1_u16.to_le_bytes()).unwrap(); // PCM
    file.write_all(&1_u16.to_le_bytes()).unwrap(); // mono
    file.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
    file.write_all(&(SAMPLE_RATE * 2).to_le_bytes()).unwrap(); // octets/seconde
    file.write_all(&2_u16.to_le_bytes()).unwrap(); // alignement
    file.write_all(&16_u16.to_le_bytes()).unwrap(); // bits par échantillon
    file.write_all(b"data").unwrap();
    file.write_all(&data_bytes.to_le_bytes()).unwrap();

    // Une sinusoïde grossière, dont la fréquence dépend de la graine.
    let mut samples = Vec::with_capacity(data_bytes as usize);
    for index in 0..sample_count {
        let phase = (index as f32) * (seed as f32 + 1.0) * 0.01;
        let value = (phase.sin() * 8000.0) as i16;
        samples.extend_from_slice(&value.to_le_bytes());
    }
    file.write_all(&samples).unwrap();
}

#[derive(Default)]
struct Tags<'a> {
    title: Option<&'a str>,
    artist: Option<&'a str>,
    album: Option<&'a str>,
    album_artist: Option<&'a str>,
    year: Option<u32>,
    track: Option<u32>,
}

/// Crée un fichier audio tagué. Sans tags, seul le nom de fichier informera
/// l'import — ce qui est justement l'un des cas à tester.
fn make_track(path: &Path, duration_ms: u32, seed: u8, tags: Tags<'_>) {
    write_wav(path, duration_ms, seed);

    let has_tags = tags.title.is_some() || tags.artist.is_some() || tags.album.is_some();
    if !has_tags {
        return;
    }

    let mut tag = Tag::new(TagType::Id3v2);
    if let Some(title) = tags.title {
        tag.set_title(title.to_string());
    }
    if let Some(artist) = tags.artist {
        tag.set_artist(artist.to_string());
    }
    if let Some(album) = tags.album {
        tag.set_album(album.to_string());
    }
    if let Some(album_artist) = tags.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.to_string());
    }
    if let Some(year) = tags.year {
        tag.set_year(year);
    }
    if let Some(track) = tags.track {
        tag.set_track(track);
    }

    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

/// Environnement complet : base migrée, racine de bibliothèque, dossier source.
struct Bac {
    _dir: tempfile::TempDir,
    pool: SqlitePool,
    paths: PathResolver,
    source_dir: PathBuf,
    library_root: PathBuf,
}

async fn bac() -> Bac {
    let dir = tempfile::tempdir().unwrap();

    let data_dir = dir.path().join("donnees");
    let library_root = dir.path().join("Bibliotheque");
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&library_root).unwrap();
    std::fs::create_dir_all(&source_dir).unwrap();

    let mut paths = PathResolver::new(data_dir);
    paths.ensure_dirs().unwrap();
    paths.set_library_root(Some(library_root.clone()));

    let pool = db::connect(&paths.database_file()).await.unwrap();
    db::migrate(&pool).await.unwrap();

    Bac {
        _dir: dir,
        pool,
        paths,
        source_dir,
        library_root,
    }
}

async fn importer_dossier(bac: &Bac) -> scanner::ScanSummary {
    scanner::import_folder(
        &bac.pool,
        &bac.paths,
        &bac.source_dir,
        FileHandling::Organize,
        "scan",
        |_| {},
    )
    .await
    .unwrap()
}

// ════════════════════════════════════════════════════════════════════════════
//  Rangement
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn range_un_morceau_tague_selon_la_convention() {
    let bac = bac().await;

    make_track(
        &bac.source_dir.join("piste.wav"),
        2000,
        1,
        Tags {
            title: Some("Digital Love"),
            artist: Some("Daft Punk"),
            album: Some("Discovery"),
            year: Some(2001),
            track: Some(3),
            ..Default::default()
        },
    );

    let summary = importer_dossier(&bac).await;
    assert_eq!(summary.imported, 1, "erreurs : {:?}", summary.errors);

    let attendu = bac
        .library_root
        .join("Daft Punk/2001 - Discovery/03 - Digital Love.wav");

    assert!(attendu.is_file(), "fichier absent : {}", attendu.display());
    assert!(
        !bac.source_dir.join("piste.wav").exists(),
        "l'import doit DÉPLACER le fichier, pas le copier"
    );
}

#[tokio::test]
async fn deduit_les_metadonnees_du_nom_de_fichier_sans_tags() {
    let bac = bac().await;

    // Cas typique d'un titre récupéré par un script de téléchargement.
    make_track(
        &bac.source_dir.join("Air - La femme d'argent.wav"),
        1500,
        2,
        Tags::default(),
    );

    let summary = importer_dossier(&bac).await;
    assert_eq!(summary.imported, 1, "erreurs : {:?}", summary.errors);

    // Sans album, le morceau part dans le dossier des singles.
    assert!(bac
        .library_root
        .join("Air/Singles/La femme d'argent.wav")
        .is_file());
}

#[tokio::test]
async fn range_sous_lartiste_de_lalbum_et_non_sous_le_featuring() {
    let bac = bac().await;

    make_track(
        &bac.source_dir.join("x.wav"),
        1200,
        3,
        Tags {
            title: Some("Get Lucky"),
            artist: Some("Daft Punk feat. Pharrell Williams"),
            album: Some("Random Access Memories"),
            album_artist: Some("Daft Punk"),
            year: Some(2013),
            track: Some(8),
            ..Default::default()
        },
    );

    assert_eq!(importer_dossier(&bac).await.imported, 1);

    assert!(
        bac.library_root
            .join("Daft Punk/2013 - Random Access Memories/08 - Get Lucky.wav")
            .is_file(),
        "un featuring ne doit jamais créer son propre dossier d'artiste"
    );

    // Pharrell reste néanmoins crédité et donc trouvable.
    let invite: Option<String> = sqlx::query_scalar(
        "SELECT a.name FROM track_artists ta
           JOIN artists a ON a.id = ta.artist_id
          WHERE ta.role = 'featuring'",
    )
    .fetch_optional(&bac.pool)
    .await
    .unwrap();

    assert_eq!(invite.as_deref(), Some("Pharrell Williams"));
}

#[tokio::test]
async fn assainit_les_noms_interdits_par_exfat() {
    let bac = bac().await;

    make_track(
        &bac.source_dir.join("y.wav"),
        1000,
        4,
        Tags {
            title: Some("Hells Bells: intro"),
            artist: Some("AC/DC"),
            album: Some("Back In Black"),
            year: Some(1980),
            track: Some(1),
            ..Default::default()
        },
    );

    assert_eq!(importer_dossier(&bac).await.imported, 1);

    assert!(
        bac.library_root
            .join("AC DC/1980 - Back In Black/01 - Hells Bells intro.wav")
            .is_file(),
        "la barre oblique aurait créé un dossier fantôme"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  Dédoublonnage
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn refuse_deux_fois_le_meme_fichier() {
    let bac = bac().await;

    let tags = || Tags {
        title: Some("Around The World"),
        artist: Some("Daft Punk"),
        album: Some("Homework"),
        year: Some(1997),
        track: Some(7),
        ..Default::default()
    };

    make_track(&bac.source_dir.join("a.wav"), 1800, 5, tags());
    assert_eq!(importer_dossier(&bac).await.imported, 1);

    // Exactement le même fichier, présenté une seconde fois.
    make_track(&bac.source_dir.join("copie.wav"), 1800, 5, tags());
    let second = importer_dossier(&bac).await;

    assert_eq!(second.imported, 0);
    assert_eq!(second.duplicates, 1);
    assert!(
        bac.source_dir.join("copie.wav").exists(),
        "un doublon ne doit pas voir son fichier source déplacé"
    );
}

#[tokio::test]
async fn detecte_un_doublon_de_tags_malgre_un_contenu_different() {
    let bac = bac().await;

    // Même titre et même durée, mais un encodage différent : l'empreinte de
    // contenu ne peut pas les rapprocher, seuls les tags le peuvent.
    make_track(
        &bac.source_dir.join("v1.wav"),
        2000,
        6,
        Tags {
            title: Some("Veridis Quo"),
            artist: Some("Daft Punk"),
            ..Default::default()
        },
    );
    assert_eq!(importer_dossier(&bac).await.imported, 1);

    make_track(
        &bac.source_dir.join("v2.wav"),
        2000,
        99, // audio différent
        Tags {
            title: Some("Veridis Quo"),
            artist: Some("Daft Punk"),
            ..Default::default()
        },
    );

    let second = importer_dossier(&bac).await;
    assert_eq!(second.duplicates, 1, "erreurs : {:?}", second.errors);
}

#[tokio::test]
async fn ne_fusionne_pas_deux_titres_generiques_dartistes_differents() {
    let bac = bac().await;

    // « Intro » de durée voisine existe sur des milliers d'albums. Sans
    // l'artiste dans le critère, ces deux morceaux seraient confondus.
    for (nom, artiste, seed) in [("a.wav", "Premier Groupe", 12_u8), ("b.wav", "Second Groupe", 13)] {
        make_track(
            &bac.source_dir.join(nom),
            2000,
            seed,
            Tags {
                title: Some("Intro"),
                artist: Some(artiste),
                ..Default::default()
            },
        );
    }

    let summary = importer_dossier(&bac).await;

    assert_eq!(
        summary.imported, 2,
        "deux artistes différents peuvent avoir chacun leur « Intro »"
    );
    assert_eq!(summary.duplicates, 0);
}

#[tokio::test]
async fn desambigue_deux_titres_homonymes_du_meme_album() {
    let bac = bac().await;

    // exFAT est insensible à la casse : sans suffixe, le second écraserait
    // silencieusement le premier. Les durées sont volontairement éloignées
    // pour que le dédoublonnage ne les confonde pas.
    for (nom, titre, seed, duree) in [
        ("a.wav", "Intro", 10_u8, 1000_u32),
        ("b.wav", "INTRO", 11, 9000),
    ] {
        make_track(
            &bac.source_dir.join(nom),
            duree,
            seed,
            Tags {
                title: Some(titre),
                artist: Some("Un Groupe"),
                album: Some("Un Album"),
                year: Some(2020),
                ..Default::default()
            },
        );
    }

    let summary = importer_dossier(&bac).await;
    assert_eq!(summary.imported, 2, "erreurs : {:?}", summary.errors);

    let album_dir = bac.library_root.join("Un Groupe/2020 - Un Album");
    let fichiers = std::fs::read_dir(&album_dir).unwrap().count();

    assert_eq!(fichiers, 2, "aucun fichier ne doit en écraser un autre");
}

// ════════════════════════════════════════════════════════════════════════════
//  Base et recherche
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn alimente_la_base_et_la_recherche() {
    let bac = bac().await;

    make_track(
        &bac.source_dir.join("h.wav"),
        1400,
        20,
        Tags {
            title: Some("Halo"),
            artist: Some("Beyoncé"),
            album: Some("I Am… Sasha Fierce"),
            year: Some(2008),
            track: Some(11),
            ..Default::default()
        },
    );

    assert_eq!(importer_dossier(&bac).await.imported, 1);

    let counts = repository::counts(&bac.pool).await.unwrap();
    assert_eq!(counts.tracks, 1);
    assert_eq!(counts.artists, 1);
    assert_eq!(counts.albums, 1);

    // La recherche doit fonctionner sans les accents.
    let resultats = repository::search_tracks(&bac.pool, "beyonce", 10).await.unwrap();
    assert_eq!(resultats.len(), 1);
    assert_eq!(resultats[0].title, "Halo");
    assert_eq!(resultats[0].artist.as_deref(), Some("Beyoncé"));

    // Et sur un préfixe de titre.
    assert_eq!(
        repository::search_tracks(&bac.pool, "hal", 10).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn nagrege_pas_deux_graphies_du_meme_artiste() {
    let bac = bac().await;

    for (nom, artiste, titre, seed) in [
        ("1.wav", "A$AP Rocky", "Praise The Lord", 30_u8),
        ("2.wav", "ASAP Rocky", "Fashion Killa", 31),
    ] {
        make_track(
            &bac.source_dir.join(nom),
            1100,
            seed,
            Tags {
                title: Some(titre),
                artist: Some(artiste),
                ..Default::default()
            },
        );
    }

    assert_eq!(importer_dossier(&bac).await.imported, 2);

    let counts = repository::counts(&bac.pool).await.unwrap();
    assert_eq!(
        counts.artists, 1,
        "« A$AP Rocky » et « ASAP Rocky » sont le même artiste"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  Robustesse
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn un_fichier_corrompu_ninterrompt_pas_le_scan() {
    let bac = bac().await;

    std::fs::write(bac.source_dir.join("casse.wav"), b"ceci n'est pas de l'audio").unwrap();
    make_track(
        &bac.source_dir.join("bon.wav"),
        1000,
        40,
        Tags {
            title: Some("Valide"),
            artist: Some("Quelqu'un"),
            ..Default::default()
        },
    );

    let summary = importer_dossier(&bac).await;

    assert_eq!(summary.imported, 1);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.errors.len(), 1);
}

#[tokio::test]
async fn ignore_les_fichiers_appledouble_du_volume_exfat() {
    let bac = bac().await;

    make_track(
        &bac.source_dir.join("morceau.wav"),
        1000,
        50,
        Tags {
            title: Some("Vrai"),
            artist: Some("Artiste"),
            ..Default::default()
        },
    );
    // Ce que macOS sème à côté de chaque fichier sur un volume exFAT.
    std::fs::write(bac.source_dir.join("._morceau.wav"), b"attributs etendus").unwrap();

    let summary = importer_dossier(&bac).await;

    assert_eq!(summary.imported, 1);
    assert_eq!(summary.failed, 0, "l'AppleDouble ne doit même pas être tenté");
}

#[tokio::test]
async fn un_morceau_disparu_devient_indisponible_sans_etre_supprime() {
    let bac = bac().await;

    make_track(
        &bac.source_dir.join("m.wav"),
        1000,
        60,
        Tags {
            title: Some("Éphémère"),
            artist: Some("Artiste"),
            ..Default::default()
        },
    );
    assert_eq!(importer_dossier(&bac).await.imported, 1);

    // L'utilisateur supprime le fichier depuis le Finder.
    let chemin: String = sqlx::query_scalar("SELECT relative_path FROM tracks")
        .fetch_one(&bac.pool)
        .await
        .unwrap();
    std::fs::remove_file(bac.paths.resolve(&chemin).unwrap()).unwrap();

    let perdus = scanner::refresh_availability(&bac.pool, &bac.paths).await.unwrap();
    assert_eq!(perdus, 1);

    let counts = repository::counts(&bac.pool).await.unwrap();
    assert_eq!(counts.tracks, 1, "le morceau ne doit PAS être supprimé");
    assert_eq!(counts.unavailable, 1);
}

#[tokio::test]
async fn journalise_le_chemin_dorigine_pour_permettre_une_annulation() {
    let bac = bac().await;

    let origine = bac.source_dir.join("origine.wav");
    make_track(
        &origine,
        1000,
        70,
        Tags {
            title: Some("Titre"),
            artist: Some("Artiste"),
            ..Default::default()
        },
    );

    assert_eq!(importer_dossier(&bac).await.imported, 1);

    let (source, destination): (String, Option<String>) =
        sqlx::query_as("SELECT source_path, destination_path FROM import_jobs WHERE state = 'done'")
            .fetch_one(&bac.pool)
            .await
            .unwrap();

    assert_eq!(source, origine.display().to_string());
    assert_eq!(destination.as_deref(), Some("Artiste/Singles/Titre.wav"));
}

#[tokio::test]
async fn indexe_sur_place_sans_deplacer_une_bibliotheque_deja_rangee() {
    let bac = bac().await;

    let deja_range = bac.library_root.join("Air/1998 - Moon Safari/01 - Titre.wav");
    make_track(
        &deja_range,
        1000,
        80,
        Tags {
            title: Some("Titre"),
            artist: Some("Air"),
            album: Some("Moon Safari"),
            year: Some(1998),
            track: Some(1),
            ..Default::default()
        },
    );

    let outcome = importer::import_file(
        &bac.pool,
        &bac.paths,
        &deja_range,
        FileHandling::IndexInPlace,
        "scan",
    )
    .await
    .unwrap();

    assert!(matches!(outcome, ImportOutcome::Imported { .. }));
    assert!(deja_range.is_file(), "le fichier ne devait pas bouger");

    let chemin: String = sqlx::query_scalar("SELECT relative_path FROM tracks")
        .fetch_one(&bac.pool)
        .await
        .unwrap();
    assert_eq!(chemin, "Air/1998 - Moon Safari/01 - Titre.wav");
}

#[tokio::test]
async fn la_progression_est_rapportee_a_chaque_fichier() {
    let bac = bac().await;

    for index in 0..3_u8 {
        make_track(
            &bac.source_dir.join(format!("{index}.wav")),
            1000,
            index + 90,
            Tags {
                title: Some("Titre"),
                artist: Some("Artiste"),
                ..Default::default()
            },
        );
    }

    let mut etapes = Vec::new();
    scanner::import_folder(
        &bac.pool,
        &bac.paths,
        &bac.source_dir,
        FileHandling::Organize,
        "scan",
        |progress| etapes.push((progress.processed, progress.total)),
    )
    .await
    .unwrap();

    assert_eq!(etapes, vec![(1, 3), (2, 3), (3, 3)]);
}
