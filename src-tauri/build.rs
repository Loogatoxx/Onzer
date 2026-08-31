use std::path::Path;

fn main() {
    purge_fichiers_appledouble(Path::new("."));

    // Toute modification d'une migration doit déclencher une recompilation :
    // `sqlx::migrate!` les embarque dans le binaire.
    println!("cargo:rerun-if-changed=migrations");

    tauri_build::build()
}

/// Supprime récursivement les fichiers AppleDouble « ._* » du crate.
///
/// # Pourquoi ce code existe
///
/// Le projet vit sur un volume **exFAT**, sur lequel macOS matérialise les
/// attributs étendus dans des fichiers frères nommés « ._<nom> ». Ils portent
/// le même suffixe que l'original mais contiennent des données binaires.
///
/// Trois outils y sont déjà tombés :
///
/// | Outil | Fichier fatal | Symptôme |
/// |---|---|---|
/// | `sqlx::migrate!` | `migrations/._0001_initial.sql` | Sélectionne par extension `.sql`, aurait exécuté un binaire comme migration |
/// | `tauri_build` | `capabilities/._default.json` | « stream did not contain valid UTF-8 » |
/// | `tauri_build` | `target/…/._default.toml` | Panique du build script (résolu par ailleurs : `target/` est hors du volume) |
///
/// Le nettoyage est fait ici, **avant** la compilation du crate et avant
/// `tauri_build::build()`, donc avant tout parcours de fichiers.
///
/// `target/` n'est pas concerné : il vit sur un volume APFS (ADR-009).
fn purge_fichiers_appledouble(racine: &Path) {
    let Ok(entries) = std::fs::read_dir(racine) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let nom = entry.file_name();
        let nom = nom.to_string_lossy();

        if nom.starts_with("._") {
            let _ = std::fs::remove_file(&path);
        } else if path.is_dir() && nom != "target" && nom != "gen" {
            purge_fichiers_appledouble(&path);
        }
    }
}
