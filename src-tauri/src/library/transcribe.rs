//! Faire écouter un morceau à un modèle local.
//!
//! # Ce qui tourne, et où
//!
//! `whisper.cpp` sur la machine de l'utilisateur, sur un modèle posé sur son
//! disque. **Rien ne part sur le réseau** : ni l'audio, ni le texte, ni même le
//! fait qu'une transcription ait eu lieu. C'est la seule forme d'intelligence
//! artificielle compatible avec un lecteur qui promet de fonctionner hors
//! ligne.
//!
//! # Pourquoi appeler un binaire plutôt que d'embarquer le modèle
//!
//! Onzer se sert de ce que l'utilisateur possède déjà — même principe que pour
//! `spotdl` et `yt-dlp` (ADR-036). Embarquer `whisper.cpp` alourdirait la
//! compilation de plusieurs minutes et l'application de plusieurs mégaoctets,
//! pour une fonction dont beaucoup n'auront jamais l'usage. Absent, il ne
//! manque rien : la transcription se propose, elle ne s'impose pas.
//!
//! # Le passage par `ffmpeg`
//!
//! `whisper.cpp` ne lit que du PCM 16 kHz mono. La conversion est confiée à
//! `ffmpeg`, qui sait décoder tous les formats de la bibliothèque là où
//! réimplémenter ce pont en Rust ajouterait un décodeur de plus à maintenir.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::core::{OnzerError, Result};
use crate::library::align::HeardWord;

/// Nom du réglage qui retient le chemin du modèle.
pub const MODEL_SETTING: &str = "whisper_model";

/// Signature apposée aux paroles que le modèle a devinées.
///
/// Le format `.lrc` prévoit ces métadonnées entre crochets ; l'analyseur
/// d'Onzer les ignore déjà, et les autres lecteurs les affichent en en-tête.
pub const SIGNATURE: &str = "[by:Onzer — transcription automatique]";

/// Ce qu'il faut pour transcrire.
#[derive(Debug, Clone)]
pub struct Transcriber {
    whisper: PathBuf,
    ffmpeg: PathBuf,
    model: PathBuf,
}

/// Pourquoi la transcription n'est pas disponible.
///
/// Un message d'erreur qui dit **quoi installer** vaut mieux qu'un bouton
/// grisé sans explication (ADR-030).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub whisper: bool,
    pub ffmpeg: bool,
    pub model: bool,
}

impl Transcriber {
    /// Rassemble les trois pièces, ou dit lesquelles manquent.
    pub fn detect(model: Option<&str>) -> std::result::Result<Self, Missing> {
        let whisper = find_binary("whisper-cli");
        let ffmpeg = find_binary("ffmpeg");
        let model = find_model(model);

        match (whisper, ffmpeg, model) {
            (Some(whisper), Some(ffmpeg), Some(model)) => Ok(Self {
                whisper,
                ffmpeg,
                model,
            }),
            (whisper, ffmpeg, model) => Err(Missing {
                whisper: whisper.is_none(),
                ffmpeg: ffmpeg.is_none(),
                model: model.is_none(),
            }),
        }
    }

    /// Écoute un morceau et rend les mots entendus, horodatés.
    ///
    /// # Pourquoi `auto` plutôt qu'une langue fixée
    ///
    /// La bibliothèque mélange le français et l'anglais. Imposer une langue
    /// ferait transcrire l'un dans l'autre — et l'alignement, qui compare des
    /// mots, s'effondrerait sur une langue mal devinée.
    pub fn hear(&self, audio: &Path) -> Result<Vec<HeardWord>> {
        let work = tempfile::tempdir()
            .map_err(|error| OnzerError::Invalid(format!("dossier de travail : {error}")))?;

        let wav = work.path().join("piste.wav");
        self.to_wav(audio, &wav)?;

        let prefix = work.path().join("sortie");
        self.run_whisper(&wav, &prefix)?;

        let json = std::fs::read_to_string(prefix.with_extension("json"))
            .map_err(|error| OnzerError::Invalid(format!("sortie de whisper : {error}")))?;

        parse_words(&json)
    }

    /// Transcrit un morceau dont on n'a aucune parole, directement en `.lrc`.
    ///
    /// # Pourquoi une autre découpe que pour l'alignement
    ///
    /// L'alignement veut des **mots** isolés, pour dater chaque vers connu au
    /// plus juste. Ici il n'y a pas de vers connu : c'est le modèle qui écrit
    /// le texte, et sa découpe naturelle en phrases est bien plus lisible
    /// qu'une avalanche d'un mot par ligne.
    ///
    /// # Pourquoi le résultat est signé
    ///
    /// Ces paroles sont **devinées**. Une balise `[by:]` — métadonnée que le
    /// format `.lrc` prévoit, que l'analyseur d'Onzer ignore déjà et que tout
    /// autre lecteur affichera — dit d'où elles viennent. Sans elle, on ferait
    /// passer une transcription approximative pour le texte de l'auteur.
    pub fn transcribe(&self, audio: &Path) -> Result<Option<String>> {
        let work = tempfile::tempdir()
            .map_err(|error| OnzerError::Invalid(format!("dossier de travail : {error}")))?;

        let wav = work.path().join("piste.wav");
        self.to_wav(audio, &wav)?;

        let prefix = work.path().join("sortie");
        self.run(&wav, &prefix, &["-olrc"])?;

        let lrc = std::fs::read_to_string(prefix.with_extension("lrc"))
            .map_err(|error| OnzerError::Invalid(format!("sortie de whisper : {error}")))?;

        let body = lrc.trim();
        if body.is_empty() {
            return Ok(None);
        }

        Ok(Some(format!("{SIGNATURE}\n{body}")))
    }

    /// Décode en PCM 16 kHz mono, le seul format que `whisper.cpp` accepte.
    fn to_wav(&self, audio: &Path, destination: &Path) -> Result<()> {
        let output = Command::new(&self.ffmpeg)
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(audio)
            .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
            .arg(destination)
            .output()
            .map_err(|error| OnzerError::Invalid(format!("ffmpeg introuvable : {error}")))?;

        if !output.status.success() {
            return Err(OnzerError::Invalid(format!(
                "ffmpeg a échoué : {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        Ok(())
    }

    fn run_whisper(&self, wav: &Path, prefix: &Path) -> Result<()> {
        // Un mot par segment : c'est la granularité dont l'alignement a
        // besoin. À l'échelle de la phrase, une ligne de paroles ne peut pas
        // être datée mieux qu'à trois secondes près.
        self.run(wav, prefix, &["-ml", "1", "-sow", "-oj"])
    }

    fn run(&self, wav: &Path, prefix: &Path, extra: &[&str]) -> Result<()> {
        let output = Command::new(&self.whisper)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(wav)
            // `auto` plutôt qu'une langue fixée : la bibliothèque mélange le
            // français et l'anglais, et transcrire l'un dans l'autre ferait
            // s'effondrer l'alignement, qui compare des mots.
            .args(["-l", "auto", "--no-prints"])
            .args(extra)
            .arg("-of")
            .arg(prefix)
            .output()
            .map_err(|error| OnzerError::Invalid(format!("whisper introuvable : {error}")))?;

        if !output.status.success() {
            return Err(OnzerError::Invalid(format!(
                "whisper a échoué : {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        Ok(())
    }
}

/// Extrait les mots horodatés de la sortie JSON de `whisper.cpp`.
pub fn parse_words(json: &str) -> Result<Vec<HeardWord>> {
    let output: WhisperOutput = serde_json::from_str(json)
        .map_err(|error| OnzerError::Invalid(format!("sortie de whisper illisible : {error}")))?;

    Ok(output
        .transcription
        .into_iter()
        .filter_map(|segment| {
            let text = segment.text.trim().to_string();
            (!text.is_empty()).then_some(HeardWord {
                at_ms: segment.offsets.from,
                text,
            })
        })
        .collect())
}

/// Cherche un exécutable là où Homebrew et les installations manuelles le
/// posent, puis dans le `PATH`.
///
/// L'application lancée depuis le Finder n'hérite pas du `PATH` du terminal :
/// se fier à lui seul ferait échouer la détection alors que l'outil est bien
/// installé.
fn find_binary(name: &str) -> Option<PathBuf> {
    const PREFIXES: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

    for prefix in PREFIXES {
        let candidate = Path::new(prefix).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// Cherche le modèle : le réglage d'abord, les emplacements usuels ensuite.
fn find_model(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.map(PathBuf::from).filter(|path| path.is_file()) {
        return Some(path);
    }

    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let usual = [
        home.join("Library/Application Support/com.loogatoxx.onzer/models"),
        home.join(".cache/whisper"),
        PathBuf::from("/opt/homebrew/share/whisper-cpp"),
    ];

    for directory in usual {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };

        // Le plus gros fichier `.bin` du dossier : entre plusieurs modèles,
        // c'est le plus complet, et c'est celui qu'on veut.
        let biggest = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
            .filter_map(|path| {
                let size = path.metadata().ok()?.len();
                Some((size, path))
            })
            .max_by_key(|(size, _)| *size);

        if let Some((_, path)) = biggest {
            return Some(path);
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct WhisperOutput {
    #[serde(default)]
    transcription: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
struct Segment {
    #[serde(default)]
    text: String,
    offsets: Offsets,
}

#[derive(Debug, Deserialize)]
struct Offsets {
    /// Début du segment, en millisecondes.
    from: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_mots_horodates() {
        let json = r#"{
            "transcription": [
                {"offsets": {"from": 20, "to": 450}, "text": " Tu"},
                {"offsets": {"from": 450, "to": 1360}, "text": " sais"},
                {"offsets": {"from": 0, "to": 20}, "text": ""}
            ]
        }"#;

        let mots = parse_words(json).unwrap();

        assert_eq!(mots.len(), 2, "les segments vides sont écartés");
        assert_eq!(mots[0].at_ms, 20);
        assert_eq!(mots[0].text, "Tu");
        assert_eq!(mots[1].text, "sais");
    }

    #[test]
    fn une_sortie_sans_transcription_ne_casse_rien() {
        assert!(parse_words(r#"{"result": {}}"#).unwrap().is_empty());
    }

    #[test]
    fn une_sortie_illisible_est_une_erreur_explicite() {
        let erreur = parse_words("ceci n'est pas du JSON").unwrap_err();
        assert!(format!("{erreur}").contains("whisper"), "{erreur}");
    }

    #[test]
    fn le_detecteur_nomme_ce_qui_manque() {
        // Un bouton grisé sans explication ne vaut rien : l'utilisateur doit
        // savoir quoi installer.
        let manque = Missing {
            whisper: true,
            ffmpeg: false,
            model: true,
        };
        assert!(manque.whisper && manque.model && !manque.ffmpeg);
    }
}
