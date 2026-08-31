//! Jeton d'authentification de l'API locale.
//!
//! L'API n'écoute que sur `127.0.0.1`, mais cela ne suffit pas : **toute
//! application installée sur la machine peut joindre `127.0.0.1`**, y compris
//! une page web via `fetch`. Sans jeton, n'importe quel site visité pourrait
//! interroger la bibliothèque ou y injecter des fichiers.
//!
//! Le jeton est généré au premier démarrage, stocké dans un fichier lisible par
//! le seul utilisateur, et attendu dans un en-tête `Authorization`.

use std::path::{Path, PathBuf};

use crate::core::Result;

pub const TOKEN_FILE: &str = "onzer.token";

/// Lit le jeton, ou le crée s'il n'existe pas encore.
pub fn load_or_create(data_dir: &Path) -> Result<String> {
    let path = token_path(data_dir);

    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim().to_string();
        if trimmed.len() >= 32 {
            return Ok(trimmed);
        }
        // Fichier tronqué ou corrompu : on en régénère un plutôt que de
        // démarrer avec un secret faible.
        tracing::warn!("jeton d'API illisible, régénération");
    }

    let token = generate();
    std::fs::write(&path, &token)?;
    restrict_permissions(&path)?;

    tracing::info!(fichier = %path.display(), "jeton d'API généré");
    Ok(token)
}

pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE)
}

/// 256 bits d'aléa, en hexadécimal.
///
/// Deux UUID v4 concaténés : leur générateur puise déjà dans l'entropie du
/// système, ce qui évite une dépendance supplémentaire pour un besoin ponctuel.
fn generate() -> String {
    let first = uuid::Uuid::new_v4().simple().to_string();
    let second = uuid::Uuid::new_v4().simple().to_string();
    format!("{first}{second}")
}

/// Restreint le fichier à son propriétaire (`chmod 600`).
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Compare deux jetons en **temps constant**.
///
/// Une comparaison ordinaire s'arrête au premier octet différent : le temps de
/// réponse renseigne alors sur le nombre de caractères corrects, ce qui permet
/// de reconstituer le secret octet par octet. Le surcoût ici est nul.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();

    if left.len() != right.len() {
        return false;
    }

    let mut difference = 0_u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }

    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genere_un_jeton_suffisamment_long() {
        let token = generate();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn deux_generations_different() {
        assert_ne!(generate(), generate());
    }

    #[test]
    fn cree_le_jeton_puis_le_relit_a_lidentique() {
        let dir = tempfile::tempdir().unwrap();

        let premier = load_or_create(dir.path()).unwrap();
        let second = load_or_create(dir.path()).unwrap();

        assert_eq!(premier, second, "le jeton doit être stable entre deux démarrages");
    }

    #[test]
    fn regenere_un_jeton_tronque() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(token_path(dir.path()), "trop-court").unwrap();

        let token = load_or_create(dir.path()).unwrap();
        assert_eq!(token.len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn le_fichier_nest_lisible_que_par_son_proprietaire() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        load_or_create(dir.path()).unwrap();

        let mode = std::fs::metadata(token_path(dir.path()))
            .unwrap()
            .permissions()
            .mode();

        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn la_comparaison_reste_correcte() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }
}
