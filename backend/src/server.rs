use crate::config::{AppConfig, LibraryFolder, LlmConfig};
use crate::db;
use crate::internet_metadata;
use crate::library;
use crate::library_index;
use crate::managed_items;
use crate::metadata;
use crate::messages::{LibraryChange, SseEvent};
use crate::organizer;
use crate::actors::brain;
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
use tokio::sync::{broadcast, RwLock, Semaphore};
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
    pub bulk_metadata_request_limiter: Arc<Semaphore>,
}

pub fn build_router(state: AppState) -> Router {
    let mut app = Router::new()
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}/approve", axum::routing::post(approve_job))
        .route("/api/jobs/{id}/reject", axum::routing::post(reject_job))
        .route("/api/intake/unprocessed", get(list_unprocessed_intake_items))
        .route("/api/library", get(list_library))
        .route("/api/library/rescan", axum::routing::post(trigger_library_rescan))
        .route("/api/library/events", get(list_library_events))
        .route("/api/library/metadata", get(get_library_metadata))
        .route(
            "/api/library/internet",
            get(get_library_internet_metadata).post(save_selected_library_internet_metadata),
        )
        .route("/api/library/internet/bulk", axum::routing::post(get_library_internet_metadata_bulk))
        .route("/api/library/internet/related", get(get_related_library_internet_metadata_paths))
        .route("/api/library/internet/selected", get(get_selected_library_internet_metadata))
        .route("/api/library/duplicates", get(list_library_duplicates))
        .route("/api/library/organize", axum::routing::post(organize_library_file))
        .route("/api/libraries", get(list_libraries).post(add_library))
        .route(
            "/api/libraries/{id}",
            axum::routing::put(update_library).delete(remove_library),
        )
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/events", get(sse_handler))
        .route("/api/health", get(health))
        .route("/api/config/llm/test", axum::routing::post(test_llm_connection))
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
    query: Option<String>,
}

#[derive(Serialize)]
struct RelatedInternetMetadataPathsResponse {
    paths: Vec<String>,
}

#[derive(Serialize)]
struct DuplicateGroupMemberResponse {
    path: String,
    file_name: String,
    library_id: Option<String>,
}

#[derive(Serialize)]
struct DuplicateGroupResponse {
    key: String,
    provider: String,
    title: String,
    year: Option<u16>,
    imdb_id: Option<String>,
    tvdb_id: Option<u64>,
    canonical_path: Option<String>,
    selected: internet_metadata::InternetMetadataMatch,
    members: Vec<DuplicateGroupMemberResponse>,
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

#[derive(Deserialize)]
struct OrganizeLibraryFileRequest {
    path: String,
    library_id: Option<String>,
    selected: Option<internet_metadata::InternetMetadataMatch>,
    season: Option<u32>,
    episode: Option<u32>,
    scope: Option<String>,
    merge_existing: Option<bool>,
    apply: Option<bool>,
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

async fn approve_job(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    transition_job_status(state, id, "APPROVED").await
}

async fn reject_job(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    transition_job_status(state, id, "REJECTED").await
}

async fn transition_job_status(state: Arc<AppState>, id: i64, next_status: &str) -> axum::response::Response {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(job) => job,
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if matches!(job.status.as_str(), "PROCESSING" | "COMPLETED") {
        return (
            StatusCode::CONFLICT,
            format!("job {} can no longer be changed from {}", id, job.status),
        )
            .into_response();
    }

    match db::update_job_status(&state.pool, id, next_status).await {
        Ok(()) => {
            if let Ok(relative_path) = Path::new(&job.file_path).strip_prefix(&state.library_path) {
                if let Ok(Some(job_with_analysis)) = db::fetch_job_with_analysis(&state.pool, id).await {
                    if let Some(decision) = job_with_analysis.decision {
                        let relative_path = relative_path.to_string_lossy().replace('\\', "/");
                        if let Err(error) = managed_items::persist_processing_decision(
                            &state.pool,
                            &state.library_path,
                            &relative_path,
                            next_status,
                            &decision,
                        )
                        .await
                        {
                            tracing::warn!(err = %error, job_id = id, "failed to persist managed item decision sidecar");
                        }
                    }
                }
            }

            let _ = state.sse_tx.send(SseEvent::JobStatus {
                job_id: id,
                status: next_status.to_string(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn list_unprocessed_intake_items(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Pagination>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    match managed_items::list_unprocessed(&state.pool, limit, offset).await {
        Ok(items) => Json(items).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn list_library(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(40).clamp(1, 200);
    let offset = params.offset.unwrap_or(0);

    match library::list_from_index(
        &state.pool,
        state.library_path.display().to_string(),
        state.ingest_path.display().to_string(),
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

async fn trigger_library_rescan(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::fetch_library_scan_state(&state.pool).await {
        Ok(scan) if scan.status == "running" => {
            return (StatusCode::CONFLICT, "library rescan already running").into_response();
        }
        Ok(_) => {}
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }

    let (libraries, exclude_patterns, scan_concurrency, scan_queue_capacity) = {
        let cfg = state.config.read().await;
        (
            cfg.libraries.clone(),
            cfg.scan_exclude_patterns.clone(),
            cfg.scan_concurrency,
            cfg.scan_queue_capacity,
        )
    };

    let pool = state.pool.clone();
    let library_path = state.library_path.clone();
    let sse_tx = state.sse_tx.clone();

    tokio::spawn(async move {
        if let Err(error) = library_index::run_full_rescan(
            pool,
            library_path,
            libraries,
            exclude_patterns,
            scan_concurrency,
            scan_queue_capacity,
            sse_tx,
        )
        .await
        {
            tracing::error!(err = %error, "manual library rescan failed");
        }
    });

    StatusCode::ACCEPTED.into_response()
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

    match internet_metadata::lookup_for_library_path_with_query(&config, &params.path, params.query.as_deref()).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn get_related_library_internet_metadata_paths(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryInternetMetadataQuery>,
) -> impl IntoResponse {
    let Some(selected) = (match db::fetch_selected_internet_metadata(&state.pool, &params.path).await {
        Ok(value) => value,
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }) else {
        return Json(RelatedInternetMetadataPathsResponse { paths: Vec::new() }).into_response();
    };

    match db::find_related_selected_internet_metadata_paths(
        &state.pool,
        &params.path,
        selected.imdb_id.as_deref(),
        selected.tvdb_id,
    )
    .await
    {
        Ok(paths) => Json(RelatedInternetMetadataPathsResponse { paths }).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn list_library_duplicates(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryQuery>,
) -> impl IntoResponse {
    let rows = match db::list_duplicate_candidates(&state.pool, params.library_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    };

    let config = {
        let cfg = state.config.read().await;
        cfg.clone()
    };

    let mut grouped = std::collections::BTreeMap::<String, Vec<db::DuplicateCandidateRow>>::new();
    for row in rows {
        let key = row
            .imdb_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("imdb:{value}"))
            .or_else(|| row.tvdb_id.map(|value| format!("tvdb:{value}")));
        if let Some(key) = key {
            grouped.entry(key).or_default().push(row);
        }
    }

    let groups = grouped
        .into_iter()
        .filter_map(|(key, mut members)| {
            if members.len() < 2 {
                return None;
            }
            members.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
            let first = members.first()?;
            let selected = internet_metadata::InternetMetadataMatch {
                provider: first.provider.clone(),
                title: first.title.clone(),
                year: first.year.map(|value| value as u16),
                media_kind: first.media_kind.clone(),
                imdb_id: first.imdb_id.clone(),
                tvdb_id: first.tvdb_id.map(|value| value as u64),
                overview: None,
                rating: None,
                genres: Vec::new(),
                poster_url: None,
                source_url: None,
            };

            let canonical_path = first
                .library_id
                .as_deref()
                .and_then(|library_id| config.libraries.iter().find(|library| library.id == library_id))
                .map(|library| organizer::movie_target_container(&library.path, &selected))
                .and_then(|target_dir| {
                    let prefix = format!("{target_dir}/");
                    members
                        .iter()
                        .find(|member| member.relative_path.starts_with(&prefix))
                        .map(|member| member.relative_path.clone())
                });

            Some(DuplicateGroupResponse {
                key,
                provider: first.provider.clone(),
                title: first.title.clone(),
                year: first.year.map(|value| value as u16),
                imdb_id: first.imdb_id.clone(),
                tvdb_id: first.tvdb_id.map(|value| value as u64),
                canonical_path,
                selected,
                members: members
                    .into_iter()
                    .map(|member| DuplicateGroupMemberResponse {
                        path: member.relative_path,
                        file_name: member.file_name,
                        library_id: member.library_id,
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();

    Json(groups).into_response()
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

    let _request_permit = match state.bulk_metadata_request_limiter.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "too many concurrent bulk metadata requests",
            )
                .into_response();
        }
    };

    let config = {
        let cfg = state.config.read().await;
        cfg.clone()
    };

    let concurrency = config.bulk_metadata_concurrency.max(1);

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
                        provider_used: None,
                        search_candidates: vec![path.clone()],
                        providers: Vec::new(),
                        matches: Vec::new(),
                        warnings: vec![error.to_string()],
                    });
                internet_metadata::InternetMetadataBulkItem { path, result }
            }
        })
        .buffer_unordered(concurrency)
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
        Ok(()) => {
            if let Err(error) = managed_items::persist_selected_metadata(
                &state.pool,
                &state.library_path,
                request.path.trim(),
                &request.selected,
            )
            .await
            {
                tracing::warn!(err = %error, path = request.path.trim(), "failed to persist managed item sidecar");
            }

            Json(SelectedInternetMetadataResponse {
                path: request.path.trim().to_string(),
                selected: request.selected,
            })
            .into_response()
        }
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

async fn organize_library_file(
    State(state): State<Arc<AppState>>,
    Json(request): Json<OrganizeLibraryFileRequest>,
) -> impl IntoResponse {
    let relative_path = request.path.trim();
    if relative_path.is_empty() {
        return (StatusCode::BAD_REQUEST, "path is required").into_response();
    }

    let selected = if let Some(selected) = request.selected {
        selected
    } else {
        let Some(row) = (match db::fetch_selected_internet_metadata(&state.pool, relative_path).await {
            Ok(value) => value,
            Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }) else {
            return (StatusCode::BAD_REQUEST, "selected metadata missing; run metadata lookup first").into_response();
        };

        let genres = serde_json::from_str::<Vec<String>>(&row.genres_json).unwrap_or_default();
        internet_metadata::InternetMetadataMatch {
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
        }
    };

    let config = {
        let cfg = state.config.read().await;
        cfg.clone()
    };

    let apply = request.apply.unwrap_or(false);
    match organizer::preview_or_apply(
        &config,
        &state.library_path,
        organizer::OrganizeRequest {
            relative_path: relative_path.to_string(),
            library_id: request.library_id,
            selected,
            season: request.season,
            episode: request.episode,
            scope: request.scope,
            merge_existing: request.merge_existing.unwrap_or(false),
        },
        apply,
    )
    .await
    {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
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

#[derive(Serialize)]
struct LlmTestResponse {
    ok: bool,
    provider: String,
    model: String,
    message: String,
}

async fn test_llm_connection(Json(llm): Json<LlmConfig>) -> impl IntoResponse {
    match brain::test_llm_connection(&llm).await {
        Ok(message) => Json(LlmTestResponse {
            ok: true,
            provider: llm.provider,
            model: llm.model,
            message,
        })
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(LlmTestResponse {
                ok: false,
                provider: llm.provider,
                model: llm.model,
                message: error.to_string(),
            }),
        )
            .into_response(),
    }
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

async fn update_library(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(folder): Json<LibraryFolder>,
) -> impl IntoResponse {
    if folder.id.is_empty() || folder.name.is_empty() || folder.path.is_empty() {
        return (StatusCode::BAD_REQUEST, "id, name, and path are required").into_response();
    }
    if !matches!(folder.media_type.as_str(), "movie" | "tv") {
        return (StatusCode::BAD_REQUEST, "media_type must be \"movie\" or \"tv\"").into_response();
    }
    if folder.path.contains("..") {
        return (StatusCode::BAD_REQUEST, "path must not contain '..'").into_response();
    }

    let mut cfg = state.config.write().await;
    if cfg.libraries.iter().any(|library| library.id == folder.id && library.id != id) {
        return (StatusCode::CONFLICT, "A library with this id already exists").into_response();
    }

    let full_path = PathBuf::from(&cfg.data_path).join(&folder.path);
    if !full_path.is_dir() {
        return (StatusCode::BAD_REQUEST, "Path does not exist or is not a directory").into_response();
    }

    let Some(existing) = cfg.libraries.iter_mut().find(|library| library.id == id) else {
        return (StatusCode::NOT_FOUND, "Library not found").into_response();
    };
    *existing = folder.clone();

    if let Err(error) = cfg.save() {
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    Json(folder).into_response()
}
