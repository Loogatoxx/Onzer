//! Résolution centralisée des chemins (ADR-006).
//!
//! **Aucun chemin absolu ne doit jamais être écrit en base.** La bibliothèque
//! vit sur un SSD externe : son point de montage n'est pas stable. Si un autre
//! volume nommé « Lexar » est déjà monté, macOS montera le nôtre sur
//! `/Volumes/Lexar 1` et tous les chemins absolus deviendraient faux d'un coup.
//!
//! La base ne stocke donc que des chemins **relatifs** à la racine, et cette
//! racine est le seul endroit du système où l'information absolue existe.

use std::path::{Component, Path, PathBuf};

use crate::core::error::{OnzerError, Result};

#[derive(Debug, Clone)]
pub struct PathResolver {
    /// `~/Library/Application Support/Onzer` — disque interne, journalisé.
    /// Contient la base, les pochettes et les vecteurs d'analyse.
    data_dir: PathBuf,

    /// Racine de la bibliothèque audio, sur le SSD externe.
    /// `None` tant que l'utilisateur ne l'a pas choisie.
    library_root: Option<PathBuf>,
}

impl PathResolver {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            library_root: None,
        }
    }

    /// Crée l'arborescence de données si elle n'existe pas encore.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.artwork_dir())?;
        std::fs::create_dir_all(self.analysis_dir())?;
        Ok(())
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("onzer.db")
    }

    pub fn artwork_dir(&self) -> PathBuf {
        self.data_dir.join("artwork")
    }

    pub fn analysis_dir(&self) -> PathBuf {
        self.data_dir.join("analysis")
    }

    pub fn library_root(&self) -> Option<&Path> {
        self.library_root.as_deref()
    }

    pub fn set_library_root(&mut self, root: Option<PathBuf>) {
        self.library_root = root;
    }

    /// La bibliothèque est-elle réellement accessible ?
    ///
    /// Une racine configurée ne suffit pas : le SSD peut être débranché.
    /// C'est cette distinction qui permet d'afficher « Reconnecte Lexar »
    /// au lieu de croire que tous les morceaux ont disparu.
    pub fn is_library_online(&self) -> bool {
        self.library_root.as_deref().is_some_and(Path::is_dir)
    }

    /// Chemin relatif stocké en base → chemin absolu utilisable.
    ///
    /// Rejette tout chemin absolu ou remontant (`..`) : une base corrompue ou
    /// un import malveillant ne doit pas pouvoir faire lire un fichier
    /// arbitraire du disque.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf> {
        let root = self
            .library_root
            .as_ref()
            .ok_or(OnzerError::LibraryNotConfigured)?;

        let candidate = Path::new(relative);
        let is_escaping = candidate.is_absolute()
            || candidate
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)));

        if is_escaping {
            return Err(OnzerError::PathOutsideLibrary(relative.to_string()));
        }

        Ok(root.join(candidate))
    }

    /// Chemin absolu → chemin relatif stockable en base.
    ///
    /// Le séparateur est normalisé en `/` pour que la valeur stockée reste
    /// indépendante du système de fichiers.
    pub fn relativize(&self, absolute: &Path) -> Result<String> {
        let root = self
            .library_root
            .as_ref()
            .ok_or(OnzerError::LibraryNotConfigured)?;

        absolute
            .strip_prefix(root)
            .map_err(|_| OnzerError::PathOutsideLibrary(absolute.display().to_string()))
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver_with_root(root: &str) -> PathResolver {
        let mut resolver = PathResolver::new(PathBuf::from("/tmp/onzer-data"));
        resolver.set_library_root(Some(PathBuf::from(root)));
        resolver
    }

    #[test]
    fn resout_un_chemin_relatif_valide() {
        let resolver = resolver_with_root("/Volumes/Lexar/Musique");
        let resolved = resolver
            .resolve("Daft Punk/2001 - Discovery/03 - Digital Love.mp3")
            .unwrap();

        assert_eq!(
            resolved,
            PathBuf::from("/Volumes/Lexar/Musique/Daft Punk/2001 - Discovery/03 - Digital Love.mp3")
        );
    }

    #[test]
    fn refuse_une_remontee_hors_bibliotheque() {
        let resolver = resolver_with_root("/Volumes/Lexar/Musique");
        assert!(matches!(
            resolver.resolve("../../../etc/passwd"),
            Err(OnzerError::PathOutsideLibrary(_))
        ));
    }

    #[test]
    fn refuse_un_chemin_absolu() {
        let resolver = resolver_with_root("/Volumes/Lexar/Musique");
        assert!(matches!(
            resolver.resolve("/etc/passwd"),
            Err(OnzerError::PathOutsideLibrary(_))
        ));
    }

    #[test]
    fn echoue_sans_racine_configuree() {
        let resolver = PathResolver::new(PathBuf::from("/tmp/onzer-data"));
        assert!(matches!(
            resolver.resolve("a.mp3"),
            Err(OnzerError::LibraryNotConfigured)
        ));
    }

    #[test]
    fn relativise_un_chemin_de_la_bibliotheque() {
        let resolver = resolver_with_root("/Volumes/Lexar/Musique");
        let relative = resolver
            .relativize(Path::new("/Volumes/Lexar/Musique/Air/1998 - Moon Safari/01 - La femme d'argent.mp3"))
            .unwrap();

        assert_eq!(
            relative,
            "Air/1998 - Moon Safari/01 - La femme d'argent.mp3"
        );
    }

    #[test]
    fn refuse_de_relativiser_un_chemin_exterieur() {
        let resolver = resolver_with_root("/Volumes/Lexar/Musique");
        assert!(matches!(
            resolver.relativize(Path::new("/Users/carlos/Downloads/x.mp3")),
            Err(OnzerError::PathOutsideLibrary(_))
        ));
    }

    #[test]
    fn bibliotheque_hors_ligne_si_la_racine_nexiste_pas() {
        let resolver = resolver_with_root("/Volumes/VolumeQuiNExistePas/Musique");
        assert!(!resolver.is_library_online());
    }
}
