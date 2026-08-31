fn main() {
    purge_fichiers_appledouble();

    // Toute modification d'une migration doit déclencher une recompilation :
    // `sqlx::migrate!` les embarque dans le binaire.
    println!("cargo:rerun-if-changed=migrations");

    tauri_build::build()
}

/// Supprime les fichiers AppleDouble « ._* » du dossier des migrations.
///
/// Le projet vit sur un volume **exFAT**, sur lequel macOS matérialise les
/// attributs étendus dans des fichiers frères nommés « ._<nom> ». Un
/// « ._0001_initial.sql » se retrouve donc à côté de la vraie migration.
///
/// Or `sqlx::migrate!` sélectionne les fichiers par leur extension `.sql` :
/// il ramasserait ce binaire en croyant lire une migration. Le nettoyage est
/// fait ici, avant la compilation du crate, donc avant l'expansion de la macro.
fn purge_fichiers_appledouble() {
    let Ok(entries) = std::fs::read_dir("migrations") else {
        return;
    };

    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("._") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
