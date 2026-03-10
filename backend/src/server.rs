use crate::config::AppConfig;
use crate::db;
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
use serde::Deserialize;
use sqlx::SqlitePool;
use std::convert::Infallible;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
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
}

#[derive(Deserialize)]
struct LibraryMetadataQuery {
    path: String,
}

#[derive(Deserialize)]
struct LibraryEventsQuery {
    limit: Option<i64>,
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

    match library::scan_library(
        state.library_path.clone(),
        state.ingest_path.clone(),
        params.q,
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

/// SSE endpoint: streams real-time events to the frontend.
async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
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
