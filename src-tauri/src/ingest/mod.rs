//! Import automatique — la porte d'entrée des scripts externes.
//!
//! Deux voies, un seul tuyau derrière :
//!
//! ```text
//!   dossier _Inbox  ──┐
//!                     ├──►  library::importer  ──►  bibliothèque rangée
//!   API 127.0.0.1   ──┘
//! ```
//!
//! Le dossier surveillé ne demande aucune intégration : on y dépose un fichier,
//! il est avalé. L'API permet en plus de fournir des métadonnées et surtout de
//! **demander à Onzer s'il possède déjà un morceau avant de le télécharger**.

pub mod inbox;
pub mod server;
pub mod token;

use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::core::PathResolver;
use crate::library::importer::{self, FileHandling, ImportOutcome};

/// Démarre les deux voies d'import.
///
/// Ne retourne jamais d'erreur fatale : si le port est occupé, l'application
/// continue de fonctionner avec le seul dossier surveillé. L'import automatique
/// est un confort, pas une condition de démarrage.
pub fn start(
    pool: SqlitePool,
    paths: Arc<RwLock<PathResolver>>,
    data_dir: &std::path::Path,
    port: u16,
) {
    let token = match token::load_or_create(data_dir) {
        Ok(token) => token,
        Err(error) => {
            tracing::error!(%error, "jeton d'API illisible, import automatique désactivé");
            return;
        }
    };

    spawn_inbox_loop(pool.clone(), Arc::clone(&paths));

    let state = Arc::new(server::IngestState { pool, paths, token });
    tauri::async_runtime::spawn(async move {
        if let Err(error) = server::serve(state, port).await {
            tracing::error!(%error, "API d'import indisponible");
        }
    });
}

/// Boucle de surveillance du dossier de dépôt.
fn spawn_inbox_loop(pool: SqlitePool, paths: Arc<RwLock<PathResolver>>) {
    tauri::async_runtime::spawn(async move {
        let mut tracker = inbox::StabilityTracker::default();
        let mut interval = tokio::time::interval(inbox::POLL_INTERVAL);

        loop {
            interval.tick().await;

            let resolver = paths.read().await.clone();

            // Sans racine configurée ou sans volume monté, il n'y a rien à
            // surveiller — et surtout nulle part où ranger.
            let Some(root) = resolver.library_root() else {
                continue;
            };
            if !resolver.is_library_online() {
                continue;
            }

            let inbox_dir = inbox::inbox_path(root);
            if !inbox_dir.is_dir() {
                // Créé paresseusement : inutile de polluer la bibliothèque tant
                // que l'utilisateur n'a pas de racine valide.
                let _ = std::fs::create_dir_all(&inbox_dir);
                continue;
            }

            for file in tracker.observe(&inbox::list_candidates(&inbox_dir)) {
                import_from_inbox(&pool, &resolver, &file).await;
            }
        }
    });
}

/// Importe un fichier déposé, en journalisant l'issue.
async fn import_from_inbox(pool: &SqlitePool, paths: &PathResolver, file: &std::path::Path) {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    match importer::import_file(pool, paths, file, FileHandling::Organize, "auto_import").await {
        Ok(ImportOutcome::Imported { relative_path, .. }) => {
            tracing::info!(fichier = %name, destination = %relative_path, "import automatique");
        }
        Ok(ImportOutcome::Duplicate { reason, .. }) => {
            // Le fichier reste dans le dépôt : à l'utilisateur de décider s'il
            // veut le supprimer. Onzer ne détruit jamais rien de lui-même.
            tracing::info!(fichier = %name, reason, "doublon laissé dans le dépôt");
        }
        Err(error) => {
            tracing::warn!(fichier = %name, %error, "import automatique échoué");
        }
    }
}
