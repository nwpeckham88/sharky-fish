use crate::actors::brain;
use crate::actors::queue;
use crate::config::{AppConfig, LibraryFolder, LlmConfig};
use crate::db;
use crate::downloads;
use crate::filesystem_audit;
use crate::internet_metadata;
use crate::library;
use crate::library::{
    LibraryListOptions, LibraryManagedStatusFilter, LibrarySortBy, LibrarySortDirection,
};
use crate::library_index;
use crate::managed_items;
use crate::messages::{IdentifiedMedia, LibraryChange, ReviewExecutionMode, SseEvent};
use crate::metadata;
use crate::organizer;
use crate::review;
use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Json,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use futures::StreamExt;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, Semaphore, broadcast};
use tokio_stream::wrappers::BroadcastStream;
use tower::ServiceExt as _;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Deserialize)]
struct CreateIntakeReviewRequest {
    path: String,
}

#[derive(Deserialize)]
struct BulkPathsRequest {
    paths: Vec<String>,
}

#[derive(Serialize)]
struct BulkInternetAutoSelectResponse {
    success_count: usize,
    failure_count: usize,
    failures: Vec<BulkFailure>,
}

#[derive(Deserialize)]
struct UpdateManagedStatusRequest {
    path: String,
    status: String,
}

#[derive(Deserialize)]
struct BulkUpdateManagedStatusRequest {
    paths: Vec<String>,
    status: String,
}

#[derive(Serialize)]
struct BulkFailure {
    path: String,
    error: String,
}

#[derive(Serialize)]
struct BulkCreateReviewResponse {
    jobs: Vec<db::JobWithAnalysis>,
    success_count: usize,
    failure_count: usize,
    failures: Vec<BulkFailure>,
}

#[derive(Serialize)]
struct BulkManagedStatusResponse {
    success_count: usize,
    failure_count: usize,
    failures: Vec<BulkFailure>,
}

#[derive(Deserialize)]
struct ApproveModeRequest {
    mode: ReviewExecutionMode,
}

struct HandlerError {
    status: StatusCode,
    message: String,
}

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
        .route("/api/backlog/summary", get(get_backlog_summary))
        .route("/api/backlog/items", get(list_backlog_items))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}/approve", axum::routing::post(approve_job))
        .route(
            "/api/jobs/{id}/approve-mode",
            axum::routing::post(approve_job_mode),
        )
        .route(
            "/api/jobs/{id}/mark-re-source",
            axum::routing::post(mark_job_re_source),
        )
        .route(
            "/api/jobs/{id}/mark-keep-original",
            axum::routing::post(mark_job_keep_original),
        )
        .route(
            "/api/jobs/{id}/approve-group",
            axum::routing::post(approve_job_group),
        )
        .route(
            "/api/jobs/{id}/approve-group-mode",
            axum::routing::post(approve_job_group_mode),
        )
        .route(
            "/api/jobs/{id}/mark-re-source-group",
            axum::routing::post(mark_job_group_re_source),
        )
        .route(
            "/api/jobs/{id}/mark-keep-original-group",
            axum::routing::post(mark_job_group_keep_original),
        )
        .route("/api/jobs/{id}/reject", axum::routing::post(reject_job))
        .route(
            "/api/jobs/{id}/reject-group",
            axum::routing::post(reject_job_group),
        )
        .route(
            "/api/intake/unprocessed",
            get(list_unprocessed_intake_items),
        )
        .route(
            "/api/intake/review",
            axum::routing::post(create_intake_review_job),
        )
        .route(
            "/api/intake/review/bulk",
            axum::routing::post(create_bulk_intake_review_jobs),
        )
        .route(
            "/api/intake/status",
            axum::routing::post(update_intake_managed_status),
        )
        .route(
            "/api/intake/status/bulk",
            axum::routing::post(update_bulk_intake_managed_status),
        )
        .route("/api/library", get(list_library))
        .route(
            "/api/library/rescan",
            axum::routing::post(trigger_library_rescan),
        )
        .route("/api/library/events", get(list_library_events))
        .route("/api/library/metadata", get(get_library_metadata))
        .route("/api/downloads/summary", get(get_downloads_summary))
        .route("/api/downloads/items", get(list_download_items))
        .route(
            "/api/downloads/linked-paths",
            get(get_download_linked_paths),
        )
        .route(
            "/api/downloads/delete",
            axum::routing::post(delete_download_item),
        )
        .route(
            "/api/library/internet",
            get(get_library_internet_metadata).post(save_selected_library_internet_metadata),
        )
        .route(
            "/api/library/internet/bulk",
            axum::routing::post(get_library_internet_metadata_bulk),
        )
        .route(
            "/api/library/internet/bulk/select",
            axum::routing::post(auto_select_library_internet_metadata_bulk),
        )
        .route(
            "/api/library/internet/related",
            get(get_related_library_internet_metadata_paths),
        )
        .route(
            "/api/library/internet/selected",
            get(get_selected_library_internet_metadata),
        )
        .route("/api/library/duplicates", get(list_library_duplicates))
        .route(
            "/api/library/organize",
            axum::routing::post(organize_library_file),
        )
        .route("/api/libraries", get(list_libraries).post(add_library))
        .route(
            "/api/libraries/{id}",
            axum::routing::put(update_library).delete(remove_library),
        )
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/events", get(sse_handler))
        .route("/api/health", get(health))
        .route(
            "/api/config/llm/test",
            axum::routing::post(test_llm_connection),
        )
        .route(
            "/api/config/prompt/improve",
            axum::routing::post(improve_system_prompt),
        )
        .route("/api/config", get(get_config).put(update_config))
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state));

    if Path::new("/srv/frontend/index.html").exists() {
        app = app.fallback(spa_fallback);
    }

    app
}

/// Serve static files from /srv/frontend. For any path not found there
/// (i.e. SPA client-side routes), return index.html with HTTP 200 so the
/// SvelteKit router can take over. Without this, tower_http::ServeDir sends
/// a 404 status even when falling back to index.html.
async fn spa_fallback(req: axum::extract::Request) -> axum::response::Response {
    let serve = ServeDir::new("/srv/frontend");
    match serve.oneshot(req).await {
        Ok(res) if res.status() != StatusCode::NOT_FOUND => {
            let (parts, body) = res.into_parts();
            axum::response::Response::from_parts(parts, axum::body::Body::new(body))
        }
        _ => match tokio::fs::read_to_string("/srv/frontend/index.html").await {
            Ok(html) => (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response(),
            Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
    }
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
struct BacklogItemsQuery {
    filter: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct LibraryQuery {
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    library_id: Option<String>,
    media_type: Option<String>,
    managed_status: Option<String>,
    sort_by: Option<String>,
    sort_dir: Option<String>,
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
struct DownloadsQuery {
    q: Option<String>,
    classification: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    path: Option<String>,
}

#[derive(Deserialize)]
struct DeleteDownloadRequest {
    path: String,
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
struct ImproveSystemPromptRequest {
    llm: LlmConfig,
    concept: String,
    current_prompt: String,
    playback_context: Option<String>,
    golden_standards: crate::config::GoldenStandards,
    mode: Option<String>,
}

#[derive(Serialize)]
struct ImproveSystemPromptResponse {
    prompt: String,
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
    id_mode: Option<String>,
    write_nfo: Option<bool>,
    merge_existing: Option<bool>,
    apply: Option<bool>,
}

#[derive(Serialize)]
struct SelectedInternetMetadataResponse {
    path: String,
    selected: internet_metadata::InternetMetadataMatch,
    metadata_sidecar_written: bool,
    metadata_sidecar_warning: Option<String>,
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Pagination>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    match db::list_jobs(&state.pool, limit, offset).await {
        Ok(jobs) => Json(enrich_jobs_with_filesystem(jobs).await).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_unprocessed_intake_items(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Pagination>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    let config = { state.config.read().await.clone() };
    match managed_items::list_unprocessed(&state.pool, &config, limit, offset).await {
        Ok(items) => Json(items).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_backlog_summary(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let config = { state.config.read().await.clone() };
    match managed_items::summarize(&state.pool, &config).await {
        Ok(summary) => Json(summary).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn list_backlog_items(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BacklogItemsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    let filter = params.filter.unwrap_or_else(|| "needs_attention".into());
    let config = { state.config.read().await.clone() };

    let (
        managed_status,
        missing_metadata_only,
        missing_sidecar_only,
        needs_attention_only,
        organize_needed_only,
    ) = match filter.as_str() {
        "all" => (None, false, false, false, false),
        "unprocessed" => (Some("UNPROCESSED"), false, false, false, false),
        "failed" => (Some("FAILED"), false, false, false, false),
        "awaiting_approval" => (Some("AWAITING_APPROVAL"), false, false, false, false),
        "approved" => (Some("APPROVED"), false, false, false, false),
        "reviewed" => (Some("REVIEWED"), false, false, false, false),
        "re_source" => (Some("RE_SOURCE"), false, false, false, false),
        "missing_metadata" => (None, true, false, false, false),
        "missing_sidecar" => (None, false, true, false, false),
        "organize_needed" => (None, false, false, false, true),
        "needs_attention" => (None, false, false, true, false),
        _ => return (StatusCode::BAD_REQUEST, "invalid backlog filter").into_response(),
    };

    match managed_items::list_filtered(
        &state.pool,
        &config,
        managed_items::ListFilteredOptions {
            managed_status,
            missing_metadata_only,
            missing_sidecar_only,
            needs_attention_only,
            organize_needed_only,
            limit,
            offset,
        },
    )
    .await
    {
        Ok(items) => Json(items).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
    transition_job_status(
        state,
        id,
        Some(ReviewExecutionMode::ProcessOnly),
        "APPROVED",
    )
    .await
}

async fn approve_job_mode(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(request): Json<ApproveModeRequest>,
) -> impl IntoResponse {
    let next_status = if request.mode == ReviewExecutionMode::OrganizeOnly {
        "COMPLETED"
    } else {
        "APPROVED"
    };
    transition_job_status(state, id, Some(request.mode), next_status).await
}

async fn approve_job_group(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    transition_job_group_status(
        state,
        id,
        Some(ReviewExecutionMode::ProcessOnly),
        "APPROVED",
    )
    .await
}

async fn approve_job_group_mode(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(request): Json<ApproveModeRequest>,
) -> impl IntoResponse {
    let next_status = if request.mode == ReviewExecutionMode::OrganizeOnly {
        "COMPLETED"
    } else {
        "APPROVED"
    };
    transition_job_group_status(state, id, Some(request.mode), next_status).await
}

async fn reject_job(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    transition_job_status(state, id, None, "REJECTED").await
}

async fn mark_job_re_source(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }) else {
        return (StatusCode::NOT_FOUND, format!("job {} not found", id)).into_response();
    };

    match complete_review_jobs(&state, vec![job], "RE_SOURCE", "REJECTED").await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn mark_job_keep_original(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }) else {
        return (StatusCode::NOT_FOUND, format!("job {} not found", id)).into_response();
    };

    match complete_review_jobs(&state, vec![job], "KEPT_ORIGINAL", "REJECTED").await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn reject_job_group(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    transition_job_group_status(state, id, None, "REJECTED").await
}

async fn mark_job_group_re_source(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }) else {
        return (StatusCode::NOT_FOUND, format!("job {} not found", id)).into_response();
    };

    let Some(group_key) = job.group_key.as_ref().filter(|value| !value.is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            "job is not part of a TV show group",
        )
            .into_response();
    };

    let pending_jobs = match db::list_jobs(&state.pool, 500, 0).await {
        Ok(jobs) => jobs
            .into_iter()
            .filter(|candidate| {
                candidate.group_key.as_deref() == Some(group_key.as_str())
                    && candidate.group_kind == "tv_show"
                    && candidate.status == "AWAITING_APPROVAL"
            })
            .map(|candidate| db::Job {
                id: candidate.id,
                file_path: candidate.file_path,
                status: candidate.status,
                group_key: candidate.group_key,
                group_label: candidate.group_label,
                group_kind: candidate.group_kind,
                created_at: candidate.created_at,
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };

    if pending_jobs.is_empty() {
        return (
            StatusCode::CONFLICT,
            "no awaiting-approval jobs remain in this TV show group",
        )
            .into_response();
    }

    match complete_review_jobs(&state, pending_jobs, "RE_SOURCE", "REJECTED").await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn mark_job_group_keep_original(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }) else {
        return (StatusCode::NOT_FOUND, format!("job {} not found", id)).into_response();
    };

    let Some(group_key) = job.group_key.as_ref().filter(|value| !value.is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            "job is not part of a TV show group",
        )
            .into_response();
    };

    let pending_jobs = match db::list_jobs(&state.pool, 500, 0).await {
        Ok(jobs) => jobs
            .into_iter()
            .filter(|candidate| {
                candidate.group_key.as_deref() == Some(group_key.as_str())
                    && candidate.group_kind == "tv_show"
                    && candidate.status == "AWAITING_APPROVAL"
            })
            .map(|candidate| db::Job {
                id: candidate.id,
                file_path: candidate.file_path,
                status: candidate.status,
                group_key: candidate.group_key,
                group_label: candidate.group_label,
                group_kind: candidate.group_kind,
                created_at: candidate.created_at,
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };

    if pending_jobs.is_empty() {
        return (
            StatusCode::CONFLICT,
            "no awaiting-approval jobs remain in this TV show group",
        )
            .into_response();
    }

    match complete_review_jobs(&state, pending_jobs, "KEPT_ORIGINAL", "REJECTED").await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn transition_job_status(
    state: Arc<AppState>,
    id: i64,
    mode: Option<ReviewExecutionMode>,
    next_status: &str,
) -> axum::response::Response {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(job) => job,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
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

    match transition_jobs(&state, vec![job], mode, next_status).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn transition_job_group_status(
    state: Arc<AppState>,
    id: i64,
    mode: Option<ReviewExecutionMode>,
    next_status: &str,
) -> axum::response::Response {
    let Some(job) = (match db::fetch_job(&state.pool, id).await {
        Ok(job) => job,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if job.group_kind != "tv_show" {
        return (
            StatusCode::BAD_REQUEST,
            "job is not part of a TV show group",
        )
            .into_response();
    }

    let Some(group_key) = job.group_key.as_deref() else {
        return (StatusCode::BAD_REQUEST, "job group is missing a key").into_response();
    };

    let jobs = match db::fetch_jobs_for_group(&state.pool, group_key).await {
        Ok(jobs) => jobs,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };

    let pending_jobs = jobs
        .into_iter()
        .filter(|candidate| candidate.status == "AWAITING_APPROVAL")
        .collect::<Vec<_>>();

    if pending_jobs.is_empty() {
        return (
            StatusCode::CONFLICT,
            "no awaiting-approval jobs remain in this TV show group",
        )
            .into_response();
    }

    match transition_jobs(&state, pending_jobs, mode, next_status).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn transition_jobs(
    state: &Arc<AppState>,
    jobs: Vec<db::Job>,
    mode: Option<ReviewExecutionMode>,
    next_status: &str,
) -> Result<(), (StatusCode, String)> {
    let review_updated_at = unix_now();

    for job in &jobs {
        if matches!(job.status.as_str(), "PROCESSING" | "COMPLETED") {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "job {} can no longer be changed from {}",
                    job.id, job.status
                ),
            ));
        }
    }

    for mut job in jobs {
        let execution_mode = mode.unwrap_or(ReviewExecutionMode::ProcessOnly);
        let updated_relative_path = if next_status != "REJECTED" {
            apply_review_mode(state, &job, execution_mode).await?
        } else {
            None
        };

        if let Some(relative_path) = updated_relative_path.as_deref() {
            let updated_file_path = state.library_path.join(relative_path).display().to_string();
            db::update_job_file_path(&state.pool, job.id, &updated_file_path)
                .await
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            job.file_path = updated_file_path;
        }

        db::update_job_status(&state.pool, job.id, next_status)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

        if execution_mode == ReviewExecutionMode::OrganizeOnly {
            if let Some(relative_path) = updated_relative_path.as_deref() {
                let _ = managed_items::update_managed_status(
                    &state.pool,
                    &state.library_path,
                    relative_path,
                    "REVIEWED",
                    Some(review_updated_at),
                )
                .await;
            }
        } else {
            persist_job_transition(state, &job, next_status, Some(review_updated_at)).await;
        }

        let _ = state.sse_tx.send(SseEvent::JobStatus {
            job_id: job.id,
            status: next_status.into(),
        });

        if execution_mode == ReviewExecutionMode::OrganizeOnly {
            let _ = state.sse_tx.send(SseEvent::JobCompleted {
                job_id: job.id,
                success: true,
            });
        }
    }

    Ok(())
}

async fn complete_review_jobs(
    state: &Arc<AppState>,
    jobs: Vec<db::Job>,
    managed_status: &str,
    next_job_status: &str,
) -> Result<(), (StatusCode, String)> {
    let review_updated_at = unix_now();

    for job in &jobs {
        if matches!(job.status.as_str(), "PROCESSING" | "COMPLETED") {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "job {} can no longer be changed from {}",
                    job.id, job.status
                ),
            ));
        }
    }

    for job in jobs {
        let analysis = db::fetch_job_with_analysis(&state.pool, job.id)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let relative_path =
            job_relative_path(&state.library_path, &job.file_path).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("job {} is not rooted in the managed library", job.id),
                )
            })?;

        let review_note = analysis.as_ref().and_then(|item| match managed_status {
            "RE_SOURCE" => item
                .proposal
                .as_ref()
                .and_then(|proposal| proposal.recommendation_reason.clone())
                .or_else(|| {
                    Some(
                        "Operator deferred this item until a better source is available instead of accepting the current processing plan.".into(),
                    )
                }),
            "KEPT_ORIGINAL" => Some(
                "Operator kept the original media instead of executing the reviewed plan.".into(),
            ),
            _ => None,
        });

        if let Some(decision) = analysis.and_then(|item| item.decision) {
            managed_items::persist_processing_decision(
                &state.pool,
                &state.library_path,
                &relative_path,
                managed_status,
                &decision,
                review_note.as_deref(),
                Some(review_updated_at),
            )
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        } else {
            managed_items::update_managed_status(
                &state.pool,
                &state.library_path,
                &relative_path,
                managed_status,
                Some(review_updated_at),
            )
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        }

        db::update_job_status(&state.pool, job.id, next_job_status)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

        let _ = state.sse_tx.send(SseEvent::JobStatus {
            job_id: job.id,
            status: next_job_status.into(),
        });
    }

    Ok(())
}

async fn apply_review_mode(
    state: &Arc<AppState>,
    job: &db::Job,
    mode: ReviewExecutionMode,
) -> Result<Option<String>, (StatusCode, String)> {
    if mode == ReviewExecutionMode::ProcessOnly {
        return Ok(None);
    }

    let analysis = db::fetch_job_with_analysis(&state.pool, job.id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("job {} not found", job.id)))?;

    let proposal = analysis.proposal.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            "review proposal is missing for this job".to_string(),
        )
    })?;

    if !proposal.allowed_modes.contains(&mode) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("mode {:?} is not allowed for this proposal", mode),
        ));
    }

    if !proposal.organization.organize_needed {
        return Ok(None);
    }

    let selected = load_selected_metadata_for_path(state, &proposal.relative_path).await?;
    let managed_item = db::fetch_managed_item(&state.pool, &proposal.relative_path)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("managed item missing for {}", proposal.relative_path),
            )
        })?;
    let config = state.config.read().await.clone();

    let result = organizer::preview_or_apply(
        &config,
        &state.library_path,
        organizer::OrganizeRequest {
            relative_path: proposal.relative_path.clone(),
            library_id: managed_item.library_id,
            selected,
            season: None,
            episode: None,
            scope: Some(proposal.organization.scope.clone()),
            id_mode: Some("none".into()),
            write_nfo: true,
            merge_existing: false,
        },
        true,
    )
    .await
    .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;

    if result.applied && result.changed {
        managed_items::reconcile_after_organize(
            &state.pool,
            &state.library_path,
            &proposal.relative_path,
            &result.target_relative_path,
        )
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }

    Ok(Some(result.target_relative_path))
}

async fn load_selected_metadata_for_path(
    state: &Arc<AppState>,
    relative_path: &str,
) -> Result<internet_metadata::InternetMetadataMatch, (StatusCode, String)> {
    let row = db::fetch_selected_internet_metadata(&state.pool, relative_path)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("selected metadata missing for {}", relative_path),
            )
        })?;

    Ok(internet_metadata_match_from_row(row))
}

async fn load_optional_selected_metadata_for_path(
    state: &Arc<AppState>,
    relative_path: &str,
) -> Result<Option<internet_metadata::InternetMetadataMatch>, String> {
    let row = db::fetch_selected_internet_metadata(&state.pool, relative_path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(row.map(internet_metadata_match_from_row))
}

fn internet_metadata_match_from_row(
    row: db::SelectedInternetMetadataRow,
) -> internet_metadata::InternetMetadataMatch {
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
}

async fn persist_job_transition(
    state: &Arc<AppState>,
    job: &db::Job,
    next_status: &str,
    review_updated_at: Option<u64>,
) {
    let Some(relative_path) = job_relative_path(&state.library_path, &job.file_path) else {
        return;
    };

    if let Ok(Some(job_with_analysis)) = db::fetch_job_with_analysis(&state.pool, job.id).await
        && let Some(decision) = job_with_analysis.decision
        && let Err(error) = managed_items::persist_processing_decision(
            &state.pool,
            &state.library_path,
            &relative_path,
            next_status,
            &decision,
            None,
            review_updated_at,
        )
        .await
    {
        tracing::warn!(err = %error, path = relative_path, "failed to persist managed status change");
    }
}

fn job_relative_path(library_path: &Path, file_path: &str) -> Option<String> {
    Path::new(file_path)
        .strip_prefix(library_path)
        .ok()
        .map(|value| value.to_string_lossy().replace('\\', "/"))
}

async fn create_intake_review_job(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateIntakeReviewRequest>,
) -> impl IntoResponse {
    match create_intake_review_job_for_path(&state, request.path.trim()).await {
        Ok(job) => Json(job).into_response(),
        Err(error) => (error.status, error.message).into_response(),
    }
}

async fn create_bulk_intake_review_jobs(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BulkPathsRequest>,
) -> impl IntoResponse {
    if request.paths.is_empty() {
        return (StatusCode::BAD_REQUEST, "paths are required").into_response();
    }
    if request.paths.len() > 500 {
        return (StatusCode::BAD_REQUEST, "too many paths").into_response();
    }

    let mut jobs = Vec::new();
    let mut failures = Vec::new();

    for path in request.paths {
        let trimmed = path.trim().to_string();
        match create_intake_review_job_for_path(&state, &trimmed).await {
            Ok(job) => jobs.push(job),
            Err(error) => failures.push(BulkFailure {
                path: trimmed,
                error: error.message,
            }),
        }
    }

    Json(BulkCreateReviewResponse {
        success_count: jobs.len(),
        failure_count: failures.len(),
        jobs,
        failures,
    })
    .into_response()
}

async fn update_intake_managed_status(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpdateManagedStatusRequest>,
) -> impl IntoResponse {
    match update_managed_status_for_path(&state, request.path.trim(), &request.status).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn update_bulk_intake_managed_status(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BulkUpdateManagedStatusRequest>,
) -> impl IntoResponse {
    if request.paths.is_empty() {
        return (StatusCode::BAD_REQUEST, "paths are required").into_response();
    }
    if request.paths.len() > 500 {
        return (StatusCode::BAD_REQUEST, "too many paths").into_response();
    }

    let mut success_count = 0;
    let mut failures = Vec::new();

    for path in request.paths {
        let trimmed = path.trim().to_string();
        match update_managed_status_for_path(&state, &trimmed, &request.status).await {
            Ok(()) => success_count += 1,
            Err(error) => failures.push(BulkFailure {
                path: trimmed,
                error,
            }),
        }
    }

    Json(BulkManagedStatusResponse {
        success_count,
        failure_count: failures.len(),
        failures,
    })
    .into_response()
}

async fn create_intake_review_job_for_path(
    state: &Arc<AppState>,
    relative_path: &str,
) -> Result<db::JobWithAnalysis, HandlerError> {
    if relative_path.is_empty() {
        return Err(HandlerError {
            status: StatusCode::BAD_REQUEST,
            message: "path is required".into(),
        });
    }

    let metadata =
        metadata::get_or_probe_library_metadata(&state.pool, &state.library_path, relative_path)
            .await
            .map_err(|error| HandlerError {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
            })?;

    match db::fetch_active_job_for_path(&state.pool, &metadata.file_path).await {
        Ok(Some(job)) => {
            return Err(HandlerError {
                status: StatusCode::CONFLICT,
                message: format!("job {} is already active for this file", job.id),
            });
        }
        Ok(None) => {}
        Err(error) => {
            return Err(HandlerError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            });
        }
    }

    let config = { state.config.read().await.clone() };
    let managed_item = db::fetch_managed_item(&state.pool, relative_path)
        .await
        .map_err(|error| HandlerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        })?;
    let selected_metadata = load_optional_selected_metadata_for_path(state, relative_path)
        .await
        .map_err(|message| HandlerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        })?;
    let group = managed_items::resolve_job_group(&state.pool, &config, relative_path)
        .await
        .map_err(|error| HandlerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        })?;

    let mut decision = brain::create_processing_decision(
        &config,
        &IdentifiedMedia {
            path: PathBuf::from(&metadata.file_path),
            probe: metadata.probe.clone(),
        },
    )
    .await
    .map_err(|error| HandlerError {
        status: StatusCode::BAD_GATEWAY,
        message: error.to_string(),
    })?;

    let media = IdentifiedMedia {
        path: PathBuf::from(&metadata.file_path),
        probe: metadata.probe.clone(),
    };
    let proposal = review::build_review_proposal(
        &config,
        relative_path,
        managed_item
            .as_ref()
            .and_then(|item| item.library_id.as_deref()),
        selected_metadata.as_ref(),
        metadata.filesystem.clone(),
        &metadata.probe,
        &decision,
    );

    let job_id = queue::enqueue_job(
        &state.pool,
        &state.sse_tx,
        &media,
        &mut decision,
        queue::EnqueueJobOptions {
            auto_approve: config.auto_approve_ai_jobs,
            proposal: Some(&proposal),
            initial_status_override: Some("AWAITING_APPROVAL"),
            group_key: Some(group.key.as_str()),
            group_label: Some(group.label.as_str()),
            group_kind: group.kind.as_str(),
        },
    )
    .await
    .map_err(|error| HandlerError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: error.to_string(),
    })?;

    if let Err(error) = managed_items::persist_processing_decision(
        &state.pool,
        &state.library_path,
        relative_path,
        "AWAITING_APPROVAL",
        &decision,
        None,
        None,
    )
    .await
    {
        tracing::warn!(err = %error, path = relative_path, "failed to persist managed review state");
    }

    if let Ok(Some(job)) = db::fetch_job_with_analysis(&state.pool, job_id).await {
        let mut jobs = enrich_jobs_with_filesystem(vec![job]).await;
        return Ok(jobs
            .pop()
            .expect("created job missing from enrichment result"));
    }

    let fallback = db::fetch_job(&state.pool, job_id)
        .await
        .map_err(|error| HandlerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        })?
        .ok_or_else(|| HandlerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("job {} created but could not be loaded", job_id),
        })?;

    Ok(db::JobWithAnalysis {
        id: fallback.id,
        file_path: fallback.file_path,
        status: fallback.status,
        group_key: fallback.group_key,
        group_label: fallback.group_label,
        group_kind: fallback.group_kind,
        created_at: fallback.created_at,
        probe: None,
        decision: None,
        proposal: None,
        filesystem: None,
    })
}

async fn enrich_jobs_with_filesystem(
    mut jobs: Vec<db::JobWithAnalysis>,
) -> Vec<db::JobWithAnalysis> {
    for job in &mut jobs {
        job.filesystem = filesystem_audit::stat_path(Path::new(&job.file_path))
            .await
            .ok();
    }

    jobs
}

async fn update_managed_status_for_path(
    state: &Arc<AppState>,
    relative_path: &str,
    status: &str,
) -> Result<(), String> {
    if relative_path.is_empty() {
        return Err("path is required".into());
    }

    let normalized_status = status.trim().to_ascii_uppercase();
    if !matches!(normalized_status.as_str(), "REVIEWED" | "KEPT_ORIGINAL") {
        return Err("status must be REVIEWED or KEPT_ORIGINAL".into());
    }

    managed_items::update_managed_status(
        &state.pool,
        &state.library_path,
        relative_path,
        &normalized_status,
        Some(unix_now()),
    )
    .await
    .map_err(|error| error.to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

async fn list_library(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(40).clamp(1, 500);
    let offset = params.offset.unwrap_or(0);
    let config = { state.config.read().await.clone() };

    match library::list_from_index(
        &state.pool,
        &config,
        state.library_path.display().to_string(),
        state.ingest_path.display().to_string(),
        LibraryListOptions {
            query: params.q,
            library_id: params.library_id,
            media_type: params.media_type,
            managed_status: LibraryManagedStatusFilter::parse(params.managed_status.as_deref()),
            sort_by: LibrarySortBy::parse(params.sort_by.as_deref()),
            sort_direction: LibrarySortDirection::parse(params.sort_dir.as_deref()),
            limit,
            offset,
        },
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
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    }

    let (libraries, exclude_patterns, scan_concurrency, scan_queue_capacity, compute_checksums) = {
        let cfg = state.config.read().await;
        (
            cfg.libraries.clone(),
            cfg.scan_exclude_patterns.clone(),
            cfg.scan_concurrency,
            cfg.scan_queue_capacity,
            cfg.scan_compute_checksums,
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
            compute_checksums,
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
    match metadata::get_or_probe_library_metadata(&state.pool, &state.library_path, &params.path)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn get_downloads_summary(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match downloads::summarize(&state.pool, &state.ingest_path).await {
        Ok(summary) => Json(summary).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn list_download_items(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DownloadsQuery>,
) -> impl IntoResponse {
    match downloads::list_items(
        &state.pool,
        &state.ingest_path,
        downloads::DownloadsListOptions {
            query: params.q,
            classification: params.classification,
            limit: params.limit.unwrap_or(100).clamp(1, 500),
            offset: params.offset.unwrap_or(0),
        },
    )
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_download_linked_paths(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DownloadsQuery>,
) -> impl IntoResponse {
    let Some(path) = params
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (StatusCode::BAD_REQUEST, "path is required").into_response();
    };

    match downloads::linked_paths(&state.pool, &state.ingest_path, path).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn delete_download_item(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DeleteDownloadRequest>,
) -> impl IntoResponse {
    let path = request.path.trim();
    if path.is_empty() {
        return (StatusCode::BAD_REQUEST, "path is required").into_response();
    }

    match downloads::delete_item(&state.pool, &state.ingest_path, path).await {
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

    match internet_metadata::lookup_for_library_path_with_query(
        &config,
        &params.path,
        params.query.as_deref(),
    )
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn get_related_library_internet_metadata_paths(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryInternetMetadataQuery>,
) -> impl IntoResponse {
    let Some(selected) =
        (match db::fetch_selected_internet_metadata(&state.pool, &params.path).await {
            Ok(value) => value,
            Err(error) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
            }
        })
    else {
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
    let rows = match db::list_duplicate_candidates(&state.pool, params.library_id.as_deref()).await
    {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
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
                .and_then(|library_id| {
                    config
                        .libraries
                        .iter()
                        .find(|library| library.id == library_id)
                })
                .map(|library| organizer::movie_target_container(&library.path, &selected, "none"))
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
        return Json(internet_metadata::InternetMetadataBulkResponse { items: Vec::new() })
            .into_response();
    }

    if request.paths.len() > 500 {
        return (StatusCode::BAD_REQUEST, "paths length must be <= 500").into_response();
    }

    let _request_permit = match state
        .bulk_metadata_request_limiter
        .clone()
        .try_acquire_owned()
    {
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
    match save_selected_library_internet_metadata_for_path(
        &state,
        request.path.trim(),
        &request.selected,
    )
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (error.status, error.message).into_response(),
    }
}

async fn auto_select_library_internet_metadata_bulk(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LibraryInternetMetadataBulkRequest>,
) -> impl IntoResponse {
    if request.paths.is_empty() {
        return Json(BulkInternetAutoSelectResponse {
            success_count: 0,
            failure_count: 0,
            failures: Vec::new(),
        })
        .into_response();
    }

    if request.paths.len() > 500 {
        return (StatusCode::BAD_REQUEST, "paths length must be <= 500").into_response();
    }

    let _request_permit = match state
        .bulk_metadata_request_limiter
        .clone()
        .try_acquire_owned()
    {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "too many concurrent bulk metadata requests",
            )
                .into_response();
        }
    };

    let config = { state.config.read().await.clone() };
    let mut success_count = 0;
    let mut failures = Vec::new();

    for path in request.paths {
        let trimmed = path.trim().to_string();
        if trimmed.is_empty() {
            failures.push(BulkFailure {
                path,
                error: "path is required".into(),
            });
            continue;
        }

        let lookup = match internet_metadata::lookup_for_library_path(&config, &trimmed).await {
            Ok(result) => result,
            Err(error) => {
                failures.push(BulkFailure {
                    path: trimmed,
                    error: error.to_string(),
                });
                continue;
            }
        };

        let Some(first_match) = lookup.matches.first() else {
            failures.push(BulkFailure {
                path: trimmed,
                error: "no metadata matches found".into(),
            });
            continue;
        };

        match save_selected_library_internet_metadata_for_path(&state, &trimmed, first_match).await
        {
            Ok(_) => success_count += 1,
            Err(error) => failures.push(BulkFailure {
                path: trimmed,
                error: error.message,
            }),
        }
    }

    Json(BulkInternetAutoSelectResponse {
        success_count,
        failure_count: failures.len(),
        failures,
    })
    .into_response()
}

async fn save_selected_library_internet_metadata_for_path(
    state: &Arc<AppState>,
    path: &str,
    selected: &internet_metadata::InternetMetadataMatch,
) -> Result<SelectedInternetMetadataResponse, HandlerError> {
    if path.trim().is_empty() {
        return Err(HandlerError {
            status: StatusCode::BAD_REQUEST,
            message: "path is required".into(),
        });
    }

    db::upsert_selected_internet_metadata(&state.pool, path.trim(), selected)
        .await
        .map_err(|error| HandlerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        })?;

    let persist_outcome = match managed_items::persist_selected_metadata(
        &state.pool,
        &state.library_path,
        path.trim(),
        selected,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::warn!(err = %error, path = path.trim(), "failed to persist managed item sidecar");
            managed_items::SelectedMetadataPersistOutcome {
                metadata_sidecar_written: false,
                metadata_sidecar_warning: Some(
                    "Metadata was saved, but the Jellyfin .nfo could not be refreshed.".into(),
                ),
            }
        }
    };

    Ok(SelectedInternetMetadataResponse {
        path: path.trim().to_string(),
        selected: selected.clone(),
        metadata_sidecar_written: persist_outcome.metadata_sidecar_written,
        metadata_sidecar_warning: persist_outcome.metadata_sidecar_warning,
    })
}

async fn get_selected_library_internet_metadata(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LibraryInternetMetadataQuery>,
) -> impl IntoResponse {
    let row = match db::fetch_selected_internet_metadata(&state.pool, &params.path).await {
        Ok(value) => value,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
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
        metadata_sidecar_written: false,
        metadata_sidecar_warning: None,
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
        let Some(row) =
            (match db::fetch_selected_internet_metadata(&state.pool, relative_path).await {
                Ok(value) => value,
                Err(error) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
                }
            })
        else {
            return (
                StatusCode::BAD_REQUEST,
                "selected metadata missing; run metadata lookup first",
            )
                .into_response();
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
            id_mode: request.id_mode,
            write_nfo: request.write_nfo.unwrap_or(true),
            merge_existing: request.merge_existing.unwrap_or(false),
        },
        apply,
    )
    .await
    {
        Ok(result) => {
            if result.applied
                && result.changed
                && let Err(error) = managed_items::reconcile_after_organize(
                    &state.pool,
                    &state.library_path,
                    relative_path,
                    &result.target_relative_path,
                )
                .await
            {
                tracing::warn!(err = %error, from = relative_path, to = %result.target_relative_path, "failed to reconcile managed item after organize");
            }
            Json(result).into_response()
        }
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

async fn improve_system_prompt(
    Json(request): Json<ImproveSystemPromptRequest>,
) -> impl IntoResponse {
    match brain::improve_system_prompt(
        &request.llm,
        &request.concept,
        &request.current_prompt,
        request.playback_context.as_deref().unwrap_or_default(),
        &request.golden_standards,
        request.mode.as_deref().unwrap_or("replace"),
    )
    .await
    {
        Ok(prompt) => Json(ImproveSystemPromptResponse { prompt }).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
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
        return (
            StatusCode::BAD_REQUEST,
            "media_type must be \"movie\" or \"tv\"",
        )
            .into_response();
    }

    // Validate path doesn't escape data_path (no ".." traversal).
    if folder.path.contains("..") {
        return (StatusCode::BAD_REQUEST, "path must not contain '..'").into_response();
    }

    let mut cfg = state.config.write().await;

    // Check for duplicate id.
    if cfg.libraries.iter().any(|l| l.id == folder.id) {
        return (
            StatusCode::CONFLICT,
            "A library with this id already exists",
        )
            .into_response();
    }

    // Verify the target directory exists on disk.
    let full_path = PathBuf::from(&cfg.data_path).join(&folder.path);
    if !full_path.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            "Path does not exist or is not a directory",
        )
            .into_response();
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
        return (
            StatusCode::BAD_REQUEST,
            "media_type must be \"movie\" or \"tv\"",
        )
            .into_response();
    }
    if folder.path.contains("..") {
        return (StatusCode::BAD_REQUEST, "path must not contain '..'").into_response();
    }

    let mut cfg = state.config.write().await;
    if cfg
        .libraries
        .iter()
        .any(|library| library.id == folder.id && library.id != id)
    {
        return (
            StatusCode::CONFLICT,
            "A library with this id already exists",
        )
            .into_response();
    }

    let full_path = PathBuf::from(&cfg.data_path).join(&folder.path);
    if !full_path.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            "Path does not exist or is not a directory",
        )
            .into_response();
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
