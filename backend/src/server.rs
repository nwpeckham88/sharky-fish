use crate::config::{AppConfig, LibraryFolder};
use crate::db;
use crate::internet_metadata;
use crate::library;
use crate::metadata;
use crate::messages::{LibraryChange, SseEvent};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::get,
    Router,
};
use futures::stream::Stream;
use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;
use std::convert::Infallible;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// Shared application state injected into handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub sse_tx: broadcast::Sender<SseEvent>,
    pub library_path: PathBuf,
    pub ingest_path: PathBuf,
    pub config: Arc<RwLock<AppConfig>>,
}

pub fn build_router(state: AppState) -> Router {
    let mut app = Router::new()
        .route("/api/jobs", get(list_jobs))
        .route("/api/library", get(list_library))
        .route("/api/library/events", get(list_library_events))
        .route("/api/library/metadata", get(get_library_metadata))
        .route(
            "/api/library/internet",
            get(get_library_internet_metadata).post(save_selected_library_internet_metadata),
        )
        .route("/api/library/internet/bulk", axum::routing::post(get_library_internet_metadata_bulk))
        .route("/api/library/internet/selected", get(get_selected_library_internet_metadata))
        .route("/api/libraries", get(list_libraries).post(add_library))
        .route("/api/libraries/{id}", axum::routing::delete(remove_library))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/events", get(sse_handler))
        .route("/api/health", get(health))
        .route("/api/config", get(get_config).put(update_config))
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state));

    if Path::new("/srv/frontend/index.html").exists() {
        app = app.fallback_service(
            ServeDir::new("/srv/frontend")
                .not_found_service(ServeFile::new("/srv/frontend/index.html")),
        );
    }

    app
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Pagination {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct LibraryQuery {
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    library_id: Option<String>,
}

#[derive(Deserialize)]
struct LibraryMetadataQuery {
    path: String,
}

#[derive(Deserialize)]
struct LibraryEventsQuery {
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct LibraryInternetMetadataQuery {
    path: String,
}

#[derive(Deserialize)]
struct LibraryInternetMetadataBulkRequest {
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct SaveSelectedInternetMetadataRequest {
    path: String,
    selected: internet_metadata::InternetMetadataMatch,
}

#[derive(Serialize)]
struct SelectedInternetMetadataResponse {
    path: String,
    selected: internet_metadata::InternetMetadataMatch,
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Pagination>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    match db::list_jobs(&state.pool, limit, offset).await {
        Ok(jobs) => Json(jobs).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_job(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    match db::fetch_tasks_for_job(&state.pool, id).await {
        Ok(tasks) => Json(serde_json::json!({ "job_id": id, "tasks": tasks })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_library(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(40).clamp(1, 200);
    let offset = params.offset.unwrap_or(0);

    let libraries = {
        let cfg = state.config.read().await;
        cfg.libraries.clone()
    };

    match library::scan_library(
        state.library_path.clone(),
        state.ingest_path.clone(),
        libraries,
        params.q,
        params.library_id,
        limit,
        offset,
    )
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_library_metadata(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryMetadataQuery>,
) -> impl IntoResponse {
    match metadata::get_or_probe_library_metadata(&state.pool, &state.library_path, &params.path).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn list_library_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryEventsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(24).clamp(1, 200);
    match db::list_library_events(&state.pool, limit).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| LibraryChange {
                    relative_path: row.relative_path,
                    path: row.file_path,
                    change: row.change_type,
                    occurred_at: row.occurred_at as u64,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_library_internet_metadata(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryInternetMetadataQuery>,
) -> impl IntoResponse {
    let config = {
        let cfg = state.config.read().await;
        cfg.clone()
    };

    match internet_metadata::lookup_for_library_path(&config, &params.path).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn get_library_internet_metadata_bulk(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LibraryInternetMetadataBulkRequest>,
) -> impl IntoResponse {
    if request.paths.is_empty() {
        return Json(internet_metadata::InternetMetadataBulkResponse { items: Vec::new() }).into_response();
    }

    if request.paths.len() > 500 {
        return (
            StatusCode::BAD_REQUEST,
            "paths length must be <= 500",
        )
            .into_response();
    }

    let config = {
        let cfg = state.config.read().await;
        cfg.clone()
    };

    let items = futures::stream::iter(request.paths.into_iter())
        .map(|path| {
            let cfg = config.clone();
            async move {
                let result = internet_metadata::lookup_for_library_path(&cfg, &path)
                    .await
                    .unwrap_or_else(|error| internet_metadata::InternetMetadataResponse {
                        query: path.clone(),
                        parsed_year: None,
                        media_hint: None,
                        matches: Vec::new(),
                        warnings: vec![error.to_string()],
                    });
                internet_metadata::InternetMetadataBulkItem { path, result }
            }
        })
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;

    Json(internet_metadata::InternetMetadataBulkResponse { items }).into_response()
}

async fn save_selected_library_internet_metadata(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SaveSelectedInternetMetadataRequest>,
) -> impl IntoResponse {
    if request.path.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "path is required").into_response();
    }

    match db::upsert_selected_internet_metadata(&state.pool, request.path.trim(), &request.selected).await {
        Ok(()) => Json(SelectedInternetMetadataResponse {
            path: request.path.trim().to_string(),
            selected: request.selected,
        })
        .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_selected_library_internet_metadata(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryInternetMetadataQuery>,
) -> impl IntoResponse {
    let row = match db::fetch_selected_internet_metadata(&state.pool, &params.path).await {
        Ok(value) => value,
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    };

    let Some(row) = row else {
        return StatusCode::NO_CONTENT.into_response();
    };

    let genres = serde_json::from_str::<Vec<String>>(&row.genres_json).unwrap_or_default();
    let selected = internet_metadata::InternetMetadataMatch {
        provider: row.provider,
        title: row.title,
        year: row.year.map(|v| v as u16),
        media_kind: row.media_kind,
        imdb_id: row.imdb_id,
        tvdb_id: row.tvdb_id.map(|v| v as u64),
        overview: row.overview,
        rating: row.rating,
        genres,
        poster_url: row.poster_url,
        source_url: row.source_url,
    };

    Json(SelectedInternetMetadataResponse {
        path: row.relative_path,
        selected,
    })
    .into_response()
}

/// SSE endpoint: streams real-time events to the frontend.
async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| async move {
        result.ok().map(|event| {
            let json = serde_json::to_string(&event).unwrap_or_default();
            Ok(Event::default().data(json))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn health() -> &'static str {
    "ok"
}

async fn get_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    Json(cfg.clone()).into_response()
}

async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(incoming): Json<AppConfig>,
) -> impl IntoResponse {
    let mut cfg = state.config.write().await;
    // Preserve read-only fields that require a restart to change.
    let mut updated = incoming;
    updated.data_path = cfg.data_path.clone();
    updated.ingest_path = cfg.ingest_path.clone();
    updated.config_path = cfg.config_path.clone();
    updated.port = cfg.port;

    if let Err(e) = updated.save() {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    *cfg = updated.clone();
    Json(updated).into_response()
}

// ---------------------------------------------------------------------------
// Library folder CRUD
// ---------------------------------------------------------------------------

async fn list_libraries(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    Json(cfg.libraries.clone()).into_response()
}

async fn add_library(
    State(state): State<Arc<AppState>>,
    Json(folder): Json<LibraryFolder>,
) -> impl IntoResponse {
    // Validate required fields.
    if folder.id.is_empty() || folder.name.is_empty() || folder.path.is_empty() {
        return (StatusCode::BAD_REQUEST, "id, name, and path are required").into_response();
    }
    if !matches!(folder.media_type.as_str(), "movie" | "tv") {
        return (StatusCode::BAD_REQUEST, "media_type must be \"movie\" or \"tv\"").into_response();
    }

    // Validate path doesn't escape data_path (no ".." traversal).
    if folder.path.contains("..") {
        return (StatusCode::BAD_REQUEST, "path must not contain '..'").into_response();
    }

    let mut cfg = state.config.write().await;

    // Check for duplicate id.
    if cfg.libraries.iter().any(|l| l.id == folder.id) {
        return (StatusCode::CONFLICT, "A library with this id already exists").into_response();
    }

    // Verify the target directory exists on disk.
    let full_path = PathBuf::from(&cfg.data_path).join(&folder.path);
    if !full_path.is_dir() {
        return (StatusCode::BAD_REQUEST, "Path does not exist or is not a directory").into_response();
    }

    cfg.libraries.push(folder.clone());
    if let Err(e) = cfg.save() {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    (StatusCode::CREATED, Json(folder)).into_response()
}

async fn remove_library(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let mut cfg = state.config.write().await;
    let before = cfg.libraries.len();
    cfg.libraries.retain(|l| l.id != id);
    if cfg.libraries.len() == before {
        return (StatusCode::NOT_FOUND, "Library not found").into_response();
    }
    if let Err(e) = cfg.save() {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}
