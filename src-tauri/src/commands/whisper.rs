//! Synchroniser à l'oreille, quand aucun catalogue ne sait le faire.
//!
//! # Ce qui reste après tout le reste
//!
//! La trame `SYLT` des fichiers, puis LRCLIB, ont couvert la grande majorité de
//! la bibliothèque. Il subsiste deux restes, de natures différentes :
//!
//! | Reste | Ce qu'on a | Ce qu'on fait |
//! |---|---|---|
//! | Paroles sans horodatage | Le **bon texte** | On le **cale** sur ce que le modèle entend |
//! | Aucune parole | Rien | On **transcrit**, et on le dit |
//!
//! Le premier cas est de loin le meilleur : les mots affichés restent ceux de
//! l'auteur, seule la minuterie vient du modèle, et la pire erreur possible est
//! un décalage. Le second devine le texte — il est donc signé, pour ne jamais
//! se faire passer pour les paroles officielles.
//!
//! # Pourquoi cela ne trahit pas la promesse « hors ligne »
//!
//! Le modèle tourne sur la machine, sur un fichier du disque. Rien ne sort :
//! ni l'audio, ni le texte, ni le fait qu'une transcription ait eu lieu. C'est
//! la seule forme d'intelligence artificielle qu'un lecteur hors ligne puisse
//! honnêtement proposer.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use serde::Serialize;
use tauri::State;

use crate::core::{OnzerError, Result};
use crate::db::settings;
use crate::library::{align, importer, lyrics, transcribe};
use crate::AppState;

/// Une transcription à la fois : le modèle sature déjà les cœurs de la machine,
/// et deux passes en parallèle iraient deux fois moins vite chacune.
static RUNNING: AtomicBool = AtomicBool::new(false);
static DONE: AtomicI64 = AtomicI64::new(0);
static TOTAL: AtomicI64 = AtomicI64::new(0);

/// État de l'outil et de la passe en cours.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperStatus {
    /// Whisper, ffmpeg et le modèle sont-ils tous là ?
    pub ready: bool,
    /// Ce qui manque, nommément, quand `ready` est faux.
    pub missing: Option<transcribe::Missing>,
    /// Morceaux qui ont le bon texte mais pas les temps.
    pub to_align: i64,
    /// Morceaux sans aucune parole.
    pub to_transcribe: i64,
    pub running: bool,
    pub done: i64,
    pub total: i64,
}

#[tauri::command]
pub async fn whisper_status(state: State<'_, AppState>) -> Result<WhisperStatus> {
    let model: Option<String> = settings::get(&state.pool, transcribe::MODEL_SETTING).await?;
    let detected = transcribe::Transcriber::detect(model.as_deref());

    let (to_align, to_transcribe): (i64, i64) = sqlx::query_as(
        "SELECT SUM(CASE WHEN lyrics IS NOT NULL AND lyrics <> '' AND lyrics NOT LIKE ?
                         THEN 1 ELSE 0 END),
                SUM(CASE WHEN lyrics IS NULL OR lyrics = '' THEN 1 ELSE 0 END)
           FROM tracks WHERE deleted_at IS NULL",
    )
    .bind(lyrics::SYNCED_LIKE)
    .fetch_one(&state.pool)
    .await?;

    Ok(WhisperStatus {
        ready: detected.is_ok(),
        missing: detected.err(),
        to_align,
        to_transcribe,
        running: RUNNING.load(Ordering::Relaxed),
        done: DONE.load(Ordering::Relaxed),
        total: TOTAL.load(Ordering::Relaxed),
    })
}

/// Lance la passe d'écoute sur tout ce qui n'est pas synchronisé.
///
/// # Pourquoi elle se déroule sur un fil dédié
///
/// Le modèle occupe la machine trente à quarante secondes par morceau. Le
/// faire sur l'exécuteur asynchrone bloquerait tout le reste — la lecture
/// comprise, ce qui serait absurde pour un lecteur de musique.
///
/// # Pourquoi les paroles calées sont écrites dans le fichier
///
/// Contrairement à la trame `SYLT`, qui vient déjà du fichier, celles-ci
/// n'existaient nulle part : quarante secondes de calcul ne doivent pas être à
/// refaire au prochain import, ni perdues si la base est effacée.
#[tauri::command]
pub async fn listen_and_sync(state: State<'_, AppState>, transcribe_empty: bool) -> Result<i64> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(OnzerError::Invalid("une écoute est déjà en cours".to_string()));
    }

    let model: Option<String> = settings::get(&state.pool, transcribe::MODEL_SETTING).await?;
    let transcriber = match transcribe::Transcriber::detect(model.as_deref()) {
        Ok(transcriber) => transcriber,
        Err(missing) => {
            RUNNING.store(false, Ordering::SeqCst);
            return Err(OnzerError::Invalid(explain(&missing)));
        }
    };

    // Les morceaux au bon texte d'abord : c'est là que le résultat est le
    // meilleur, et l'utilisateur qui interrompt la passe en cours de route
    // aura gagné le plus.
    let pending: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, relative_path, lyrics FROM tracks
          WHERE deleted_at IS NULL
            AND (lyrics IS NULL OR lyrics = '' OR lyrics NOT LIKE ?)
            AND (? OR (lyrics IS NOT NULL AND lyrics <> ''))
          ORDER BY (lyrics IS NULL OR lyrics = ''), id",
    )
    .bind(lyrics::SYNCED_LIKE)
    .bind(transcribe_empty)
    .fetch_all(&state.pool)
    .await?;

    let pool = state.pool.clone();
    let paths = std::sync::Arc::clone(&state.paths);
    let total = pending.len() as i64;

    TOTAL.store(total, Ordering::SeqCst);
    DONE.store(0, Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        for (track_id, relative_path, existing) in pending {
            let resolver = paths.read().await.clone();
            let Ok(audio) = importer::absolute_path(&resolver, &relative_path) else {
                DONE.fetch_add(1, Ordering::Relaxed);
                continue;
            };

            if !audio.is_file() {
                DONE.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            let tool = transcriber.clone();
            let known = existing.filter(|text| !text.trim().is_empty());
            let outcome =
                tokio::task::spawn_blocking(move || listen(&tool, &audio, known.as_deref())).await;

            match outcome {
                Ok(Ok(Some(lrc))) => {
                    if let Err(error) = write(&pool, &resolver, track_id, &lrc).await {
                        tracing::warn!(track_id, %error, "paroles calées non enregistrées");
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => tracing::warn!(track_id, %error, "écoute impossible"),
                Err(error) => tracing::warn!(track_id, %error, "écoute interrompue"),
            }

            DONE.fetch_add(1, Ordering::Relaxed);

            if !RUNNING.load(Ordering::Relaxed) {
                // L'utilisateur a demandé l'arrêt : on rend la main entre deux
                // morceaux plutôt qu'au milieu d'un.
                break;
            }
        }

        RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(total)
}

/// Cale les paroles d'un seul morceau, à la demande.
///
/// # Pourquoi un chemin séparé de la passe complète
///
/// La passe traite des centaines de morceaux en tâche de fond, sans retour
/// immédiat. Ici l'utilisateur regarde un morceau précis, souvent celui qu'il
/// écoute : il attend un résultat, tout de suite, et veut le voir apparaître.
///
/// Le verrou reste commun : deux modèles en parallèle sur la même machine
/// iraient deux fois moins vite chacun. Demander pendant qu'une passe tourne
/// rend donc une erreur claire plutôt qu'une lenteur inexpliquée.
#[tauri::command]
pub async fn sync_track(state: State<'_, AppState>, track_id: i64) -> Result<lyrics::Lyrics> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(OnzerError::Invalid(
            "une écoute est déjà en cours".to_string(),
        ));
    }

    let outcome = sync_one(&state, track_id).await;
    RUNNING.store(false, Ordering::SeqCst);
    outcome
}

async fn sync_one(state: &State<'_, AppState>, track_id: i64) -> Result<lyrics::Lyrics> {
    let model: Option<String> = settings::get(&state.pool, transcribe::MODEL_SETTING).await?;
    let transcriber = transcribe::Transcriber::detect(model.as_deref())
        .map_err(|missing| OnzerError::Invalid(explain(&missing)))?;

    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT relative_path, lyrics FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&state.pool)
            .await?;

    let (relative_path, existing) =
        row.ok_or_else(|| OnzerError::Invalid("morceau introuvable".to_string()))?;

    let resolver = state.paths.read().await.clone();
    let audio = importer::absolute_path(&resolver, &relative_path)?;

    if !audio.is_file() {
        return Err(OnzerError::Invalid(
            "le fichier n'est pas là — le disque est-il branché ?".to_string(),
        ));
    }

    let known = existing.filter(|text| !text.trim().is_empty());
    let lrc = tokio::task::spawn_blocking(move || listen(&transcriber, &audio, known.as_deref()))
        .await
        .map_err(|error| OnzerError::Invalid(error.to_string()))??;

    let lrc = lrc.ok_or_else(|| {
        OnzerError::Invalid(
            "le modèle n'a pas reconnu assez de mots pour caler ces paroles".to_string(),
        )
    })?;

    write(&state.pool, &resolver, track_id, &lrc).await?;

    Ok(lyrics::parse(&lrc))
}

/// Interrompt la passe après le morceau en cours.
#[tauri::command]
pub async fn stop_listening() -> Result<()> {
    RUNNING.store(false, Ordering::SeqCst);
    Ok(())
}

/// Écoute un morceau, et rend des paroles synchronisées.
///
/// Deux chemins selon ce que l'on sait déjà : caler du texte connu, ou
/// transcrire ce qu'on n'a pas.
fn listen(
    tool: &transcribe::Transcriber,
    audio: &std::path::Path,
    known: Option<&str>,
) -> Result<Option<String>> {
    match known {
        Some(text) => {
            let heard = tool.hear(audio)?;
            Ok(align::align(text, &heard))
        }
        None => tool.transcribe(audio),
    }
}

/// Écrit les paroles dans le fichier puis en base.
async fn write(
    pool: &sqlx::SqlitePool,
    paths: &crate::core::PathResolver,
    track_id: i64,
    lrc: &str,
) -> Result<()> {
    let relative_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(pool)
            .await?;

    if let Some(relative_path) = relative_path {
        if let Ok(path) = importer::absolute_path(paths, &relative_path) {
            if path.is_file() {
                let text = lrc.to_string();
                tokio::task::spawn_blocking(move || lyrics::write_to_file(&path, &text))
                    .await
                    .map_err(|error| OnzerError::Invalid(error.to_string()))??;
            }
        }
    }

    sqlx::query("UPDATE tracks SET lyrics = ? WHERE id = ?")
        .bind(lrc)
        .bind(track_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Dit quoi installer, plutôt que de constater un échec.
fn explain(missing: &transcribe::Missing) -> String {
    let mut manques = Vec::new();

    if missing.whisper {
        manques.push("whisper-cli (brew install whisper-cpp)");
    }
    if missing.ffmpeg {
        manques.push("ffmpeg (brew install ffmpeg)");
    }
    if missing.model {
        manques.push("un modèle .bin de whisper.cpp");
    }

    format!("il manque : {}", manques.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_message_nomme_ce_quil_faut_installer() {
        // Un échec qui ne dit pas quoi faire est un échec qu'on subit deux
        // fois (ADR-030).
        let message = explain(&transcribe::Missing {
            whisper: true,
            ffmpeg: false,
            model: true,
        });

        assert!(message.contains("brew install whisper-cpp"), "{message}");
        assert!(message.contains("modèle"), "{message}");
        assert!(!message.contains("ffmpeg"), "{message}");
    }
}
