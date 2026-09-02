//! API HTTP locale d'import.
//!
//! Trois points d'entrée, pensés pour qu'un script de téléchargement externe
//! reste **totalement indépendant** d'Onzer :
//!
//! | Route | Rôle |
//! |---|---|
//! | `GET /api/v1/health` | Onzer tourne-t-il ? La bibliothèque est-elle en ligne ? |
//! | `GET /api/v1/exists` | **Ai-je déjà ce morceau ?** À interroger *avant* de télécharger |
//! | `POST /api/v1/import` | Voici un fichier, range-le |
//!
//! `exists` est le point le plus utile du lot : il évite de télécharger un
//! morceau déjà présent, ce qui est plus efficace que de l'importer puis de le
//! rejeter comme doublon.
//!
//! Contrat détaillé : `docs/IMPORT_API.md`.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::core::PathResolver;
use crate::db::repository;
use crate::library::importer::{self, FileHandling, ImportOutcome};
use crate::library::metadata::MetadataHint;
use crate::library::naming;

/// Port d'écoute. Choisi dans la plage éphémère haute pour ne rien bousculer.
pub const DEFAULT_PORT: u16 = 47811;

pub struct IngestState {
    pub pool: SqlitePool,
    pub paths: Arc<RwLock<PathResolver>>,
    pub token: String,
}

pub fn router(state: Arc<IngestState>) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/exists", get(exists))
        .route("/api/v1/import", post(import))
        .with_state(state)
}

/// Démarre le serveur. **Écoute exclusivement sur la boucle locale** : rien
/// n'est joignable depuis le réseau.
pub async fn serve(state: Arc<IngestState>, port: u16) -> crate::core::Result<()> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));

    let listener = tokio::net::TcpListener::bind(address).await.map_err(|error| {
        crate::core::OnzerError::Invalid(format!("port {port} indisponible : {error}"))
    })?;

    tracing::info!(%address, "API d'import à l'écoute");

    axum::serve(listener, router(state))
        .await
        .map_err(|error| crate::core::OnzerError::Invalid(format!("serveur arrêté : {error}")))
}

// ════════════════════════════════════════════════════════════════════════════
//  Authentification
// ════════════════════════════════════════════════════════════════════════════

/// Vérifie l'en-tête `Authorization: Bearer <jeton>`.
///
/// Écouter sur `127.0.0.1` ne protège de rien à soi seul : toute application
/// locale — y compris une page web ouverte dans un navigateur — peut y émettre
/// des requêtes.
fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), ApiError> {
    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();

    if super::token::constant_time_eq(provided.trim(), expected) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "jeton absent ou invalide",
        ))
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  GET /api/v1/health
// ════════════════════════════════════════════════════════════════════════════

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    status: &'static str,
    version: &'static str,
    /// `false` si le SSD est débranché : le script doit alors s'abstenir.
    library_online: bool,
    /// Dossier où déposer directement des fichiers, en alternative à l'API.
    inbox: Option<String>,
    track_count: i64,
}

async fn health(
    State(state): State<Arc<IngestState>>,
    headers: HeaderMap,
) -> Result<Json<Health>, ApiError> {
    authorize(&headers, &state.token)?;

    let paths = state.paths.read().await;
    let counts = repository::counts(&state.pool).await?;

    Ok(Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        library_online: paths.is_library_online(),
        inbox: paths
            .library_root()
            .map(|root| super::inbox::inbox_path(root).display().to_string()),
        track_count: counts.tracks,
    }))
}

// ════════════════════════════════════════════════════════════════════════════
//  GET /api/v1/exists
// ════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExistsQuery {
    title: String,
    artist: Option<String>,
    /// Durée connue, si le script la possède. Elle rend la réponse bien plus
    /// fiable : deux morceaux homonymes se distinguent surtout par leur durée.
    duration_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExistsResponse {
    exists: bool,
    track_id: Option<i64>,
}

async fn exists(
    State(state): State<Arc<IngestState>>,
    headers: HeaderMap,
    Query(query): Query<ExistsQuery>,
) -> Result<Json<ExistsResponse>, ApiError> {
    authorize(&headers, &state.token)?;

    let normalized_title = naming::normalize_key(&query.title);
    if normalized_title.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "le titre est obligatoire",
        ));
    }

    let normalized_artist = query.artist.as_deref().map(naming::normalize_key);

    let track_id = repository::find_by_tags(
        &state.pool,
        &normalized_title,
        normalized_artist.as_deref(),
        query.duration_ms,
    )
    .await?;

    Ok(Json(ExistsResponse {
        exists: track_id.is_some(),
        track_id,
    }))
}

// ════════════════════════════════════════════════════════════════════════════
//  POST /api/v1/import
// ════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportRequest {
    /// Chemin **absolu** du fichier déjà téléchargé.
    ///
    /// Un chemin plutôt qu'un envoi du contenu : le fichier est déjà sur le
    /// disque, le transférer par HTTP ne ferait que le copier deux fois.
    path: String,
    /// Indications de métadonnées. Voir `MetadataHint` pour l'arbitrage entre
    /// ces valeurs et les tags réellement présents dans le fichier.
    #[serde(default)]
    metadata: Option<MetadataHint>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportResponse {
    /// `imported` ou `duplicate`.
    outcome: &'static str,
    track_id: i64,
    /// Emplacement final dans la bibliothèque, absent en cas de doublon.
    relative_path: Option<String>,
    /// Renseigné en cas de doublon, pour que le script sache pourquoi.
    reason: Option<&'static str>,
}

async fn import(
    State(state): State<Arc<IngestState>>,
    headers: HeaderMap,
    Json(request): Json<ImportRequest>,
) -> Result<Json<ImportResponse>, ApiError> {
    authorize(&headers, &state.token)?;

    let source = PathBuf::from(&request.path);

    if !source.is_absolute() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "le chemin doit être absolu",
        ));
    }
    if !source.is_file() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "fichier introuvable"));
    }
    if !importer::is_importable(&source) {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "extension audio non reconnue",
        ));
    }

    let paths = state.paths.read().await.clone();
    if !paths.is_library_online() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "bibliothèque hors ligne — volume non monté",
        ));
    }

    let outcome = importer::import_file_with_hint(
        &state.pool,
        &paths,
        &source,
        FileHandling::Organize,
        "auto_import",
        request.metadata.as_ref(),
    )
    .await?;

    Ok(Json(match outcome {
        ImportOutcome::Imported {
            track_id,
            relative_path,
        } => ImportResponse {
            outcome: "imported",
            track_id,
            relative_path: Some(relative_path),
            reason: None,
        },
        ImportOutcome::Duplicate {
            existing_id,
            reason,
        } => ImportResponse {
            outcome: "duplicate",
            track_id: existing_id,
            relative_path: None,
            reason: Some(reason),
        },
        ImportOutcome::Restored {
            track_id,
            relative_path,
        } => ImportResponse {
            outcome: "restored",
            track_id,
            relative_path: Some(relative_path),
            reason: Some("le morceau avait perdu son fichier"),
        },
    }))
}

// ════════════════════════════════════════════════════════════════════════════
//  Erreurs
// ════════════════════════════════════════════════════════════════════════════

/// Erreur d'API, sérialisée en JSON pour rester exploitable par un script.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<crate::core::OnzerError> for ApiError {
    fn from(error: crate::core::OnzerError) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct Body {
            error: String,
        }

        (
            self.status,
            Json(Body {
                error: self.message,
            }),
        )
            .into_response()
    }
}
