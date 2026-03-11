use crate::internet_metadata::InternetMetadataMatch;
use crate::messages::{MediaProbe, ProcessingDecision};
use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

/// Initialize the SQLite connection pool with WAL mode and recommended PRAGMAs.
pub async fn init_pool(db_path: &Path) -> Result<SqlitePool> {
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let options = SqliteConnectOptions::from_str(&db_url)?
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS jobs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path   TEXT    NOT NULL,
            status      TEXT    NOT NULL DEFAULT 'PENDING',
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    ensure_column(pool, "jobs", "group_key", "TEXT").await?;
    ensure_column(pool, "jobs", "group_label", "TEXT").await?;
    ensure_column(pool, "jobs", "group_kind", "TEXT NOT NULL DEFAULT 'file'").await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tasks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id      INTEGER NOT NULL REFERENCES jobs(id),
            step_order  INTEGER NOT NULL,
            task_type   TEXT    NOT NULL,
            payload     TEXT,
            status      TEXT    NOT NULL DEFAULT 'QUEUED'
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS job_analysis (
            job_id          INTEGER PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE,
            probe_json      TEXT NOT NULL,
            decision_json   TEXT NOT NULL,
            updated_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    // Composite index for queue polling.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_jobs_status_created
         ON jobs (status, created_at)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tasks_job_status
         ON tasks (job_id, status)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS media_metadata (
            file_path       TEXT PRIMARY KEY,
            size_bytes      INTEGER NOT NULL,
            modified_at     INTEGER NOT NULL,
            format          TEXT NOT NULL,
            duration_secs   REAL NOT NULL,
            video_codec     TEXT,
            audio_codec     TEXT,
            width           INTEGER,
            height          INTEGER,
            audio_channels  INTEGER,
            stream_count    INTEGER NOT NULL,
            probe_json      TEXT NOT NULL,
            updated_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS library_events (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            relative_path   TEXT NOT NULL,
            file_path       TEXT NOT NULL,
            change_type     TEXT NOT NULL,
            occurred_at     INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_library_events_occurred_at
         ON library_events (occurred_at DESC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_media_metadata_modified
         ON media_metadata (modified_at)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS selected_internet_metadata (
            relative_path   TEXT PRIMARY KEY,
            provider        TEXT NOT NULL,
            title           TEXT NOT NULL,
            year            INTEGER,
            media_kind      TEXT NOT NULL,
            imdb_id         TEXT,
            tvdb_id         INTEGER,
            overview        TEXT,
            rating          REAL,
            genres_json     TEXT NOT NULL,
            poster_url      TEXT,
            source_url      TEXT,
            updated_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS library_index (
            relative_path   TEXT PRIMARY KEY,
            file_path       TEXT NOT NULL,
            file_name       TEXT NOT NULL,
            extension       TEXT NOT NULL,
            media_type      TEXT NOT NULL,
            size_bytes      INTEGER NOT NULL,
            modified_at     INTEGER NOT NULL,
            library_id      TEXT,
            updated_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_library_index_modified
         ON library_index (modified_at DESC, relative_path ASC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_library_index_library_id
         ON library_index (library_id, modified_at DESC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS library_scan_state (
            id              INTEGER PRIMARY KEY CHECK (id = 1),
            status          TEXT NOT NULL,
            scanned_items   INTEGER NOT NULL DEFAULT 0,
            total_items     INTEGER NOT NULL DEFAULT 0,
            started_at      INTEGER,
            completed_at    INTEGER,
            last_scan_at    INTEGER,
            last_error      TEXT
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS managed_items (
            relative_path           TEXT PRIMARY KEY,
            file_path               TEXT NOT NULL,
            file_name               TEXT NOT NULL,
            media_type              TEXT NOT NULL,
            size_bytes              INTEGER NOT NULL,
            modified_at             INTEGER NOT NULL,
            library_id              TEXT,
            managed_status          TEXT NOT NULL DEFAULT 'UNPROCESSED',
            selected_metadata_json  TEXT,
            last_decision_json      TEXT,
            sidecar_path            TEXT,
            first_seen_at           INTEGER NOT NULL,
            last_seen_at            INTEGER NOT NULL,
            updated_at              DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_managed_items_status_modified
         ON managed_items (managed_status, modified_at DESC, relative_path ASC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO library_scan_state (id, status, scanned_items, total_items)
         VALUES (1, 'idle', 0, 0)
         ON CONFLICT(id) DO NOTHING",
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn ensure_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;
    let exists = rows
        .iter()
        .any(|row| row.try_get::<String, _>("name").ok().as_deref() == Some(column));
    if exists {
        return Ok(());
    }

    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    sqlx::query(&alter).execute(pool).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Data access helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct Job {
    pub id: i64,
    pub file_path: String,
    pub status: String,
    pub group_key: Option<String>,
    pub group_label: Option<String>,
    pub group_kind: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobWithAnalysis {
    pub id: i64,
    pub file_path: String,
    pub status: String,
    pub group_key: Option<String>,
    pub group_label: Option<String>,
    pub group_kind: String,
    pub created_at: String,
    pub probe: Option<MediaProbe>,
    pub decision: Option<ProcessingDecision>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct Task {
    pub id: i64,
    pub job_id: i64,
    pub step_order: i64,
    pub task_type: String,
    pub payload: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct CachedMediaMetadata {
    pub file_path: String,
    pub size_bytes: i64,
    pub modified_at: i64,
    pub format: String,
    pub duration_secs: f64,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub audio_channels: Option<i64>,
    pub stream_count: i64,
    pub probe_json: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct LibraryEventRow {
    pub id: i64,
    pub relative_path: String,
    pub file_path: String,
    pub change_type: String,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct SelectedInternetMetadataRow {
    pub relative_path: String,
    pub provider: String,
    pub title: String,
    pub year: Option<i64>,
    pub media_kind: String,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
    pub overview: Option<String>,
    pub rating: Option<f64>,
    pub genres_json: String,
    pub poster_url: Option<String>,
    pub source_url: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct LibraryIndexRow {
    pub relative_path: String,
    pub file_name: String,
    pub extension: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub modified_at: i64,
    pub library_id: Option<String>,
    pub managed_status: Option<String>,
    pub has_sidecar: bool,
    pub has_selected_metadata: bool,
    pub selected_metadata_json: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct DuplicateCandidateRow {
    pub relative_path: String,
    pub file_name: String,
    pub library_id: Option<String>,
    pub provider: String,
    pub title: String,
    pub year: Option<i64>,
    pub media_kind: String,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct LibrarySummaryRow {
    pub total_items: i64,
    pub total_bytes: i64,
    pub video_items: i64,
    pub audio_items: i64,
    pub other_items: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct LibraryScanStateRow {
    pub status: String,
    pub scanned_items: i64,
    pub total_items: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub last_scan_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct ManagedItemsSummaryRow {
    pub total_items: i64,
    pub needs_attention_count: i64,
    pub unprocessed_count: i64,
    pub reviewed_count: i64,
    pub kept_original_count: i64,
    pub awaiting_approval_count: i64,
    pub approved_count: i64,
    pub processed_count: i64,
    pub failed_count: i64,
    pub missing_metadata_count: i64,
    pub missing_sidecar_count: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, FromRow)]
pub struct ManagedItemRow {
    pub relative_path: String,
    pub file_path: String,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub modified_at: i64,
    pub library_id: Option<String>,
    pub managed_status: String,
    pub selected_metadata_json: Option<String>,
    pub last_decision_json: Option<String>,
    pub sidecar_path: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub updated_at: String,
}

/// Insert a new job and return its id.
pub async fn insert_job(
    pool: &SqlitePool,
    file_path: &str,
    status: &str,
    group_key: Option<&str>,
    group_label: Option<&str>,
    group_kind: &str,
) -> Result<i64> {
    let id = sqlx::query(
		"INSERT INTO jobs (file_path, status, group_key, group_label, group_kind) VALUES (?, ?, ?, ?, ?)",
	)
        .bind(file_path)
        .bind(status)
        .bind(group_key)
        .bind(group_label)
        .bind(group_kind)
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok(id)
}

pub async fn upsert_job_analysis(
    pool: &SqlitePool,
    job_id: i64,
    probe: &MediaProbe,
    decision: &ProcessingDecision,
) -> Result<()> {
    let probe_json = serde_json::to_string(probe)?;
    let decision_json = serde_json::to_string(decision)?;

    sqlx::query(
        "INSERT INTO job_analysis (job_id, probe_json, decision_json, updated_at)
         VALUES (?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(job_id) DO UPDATE SET
            probe_json = excluded.probe_json,
            decision_json = excluded.decision_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(job_id)
    .bind(probe_json)
    .bind(decision_json)
    .execute(pool)
    .await?;

    Ok(())
}

/// Insert a task for a given job.
pub async fn insert_task(
    pool: &SqlitePool,
    job_id: i64,
    step_order: i64,
    task_type: &str,
    payload: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query(
        "INSERT INTO tasks (job_id, step_order, task_type, payload) VALUES (?, ?, ?, ?)",
    )
    .bind(job_id)
    .bind(step_order)
    .bind(task_type)
    .bind(payload)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

/// Update job status.
pub async fn update_job_status(pool: &SqlitePool, job_id: i64, status: &str) -> Result<()> {
    sqlx::query("UPDATE jobs SET status = ? WHERE id = ?")
        .bind(status)
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn fetch_job(pool: &SqlitePool, job_id: i64) -> Result<Option<Job>> {
    let row = sqlx::query_as::<_, Job>(
		"SELECT id, file_path, status, group_key, group_label, COALESCE(group_kind, 'file') AS group_kind, created_at FROM jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn fetch_active_job_for_path(pool: &SqlitePool, file_path: &str) -> Result<Option<Job>> {
    let row = sqlx::query_as::<_, Job>(
                "SELECT id, file_path, status, group_key, group_label, COALESCE(group_kind, 'file') AS group_kind, created_at
         FROM jobs
         WHERE file_path = ?
           AND status IN ('AWAITING_APPROVAL', 'APPROVED', 'PROCESSING')
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(file_path)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn fetch_jobs_for_group(pool: &SqlitePool, group_key: &str) -> Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
        "SELECT id, file_path, status, group_key, group_label, COALESCE(group_kind, 'file') AS group_kind, created_at
         FROM jobs
         WHERE group_key = ?
         ORDER BY created_at DESC, id DESC",
    )
    .bind(group_key)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn fetch_job_with_analysis(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Option<JobWithAnalysis>> {
    let row = sqlx::query(
        "SELECT j.id, j.file_path, j.status, j.group_key, j.group_label, COALESCE(j.group_kind, 'file') AS group_kind, j.created_at, a.probe_json, a.decision_json
         FROM jobs j
         LEFT JOIN job_analysis a ON a.job_id = j.id
         WHERE j.id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let probe = row
        .try_get::<Option<String>, _>("probe_json")?
        .map(|json| serde_json::from_str::<MediaProbe>(&json))
        .transpose()?;
    let decision = row
        .try_get::<Option<String>, _>("decision_json")?
        .map(|json| serde_json::from_str::<ProcessingDecision>(&json))
        .transpose()?;

    Ok(Some(JobWithAnalysis {
        id: row.try_get("id")?,
        file_path: row.try_get("file_path")?,
        status: row.try_get("status")?,
        group_key: row.try_get("group_key")?,
        group_label: row.try_get("group_label")?,
        group_kind: row.try_get("group_kind")?,
        created_at: row.try_get("created_at")?,
        probe,
        decision,
    }))
}

/// Update task status and optionally its payload.
pub async fn update_task(
    pool: &SqlitePool,
    task_id: i64,
    status: &str,
    payload: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE tasks SET status = ?, payload = COALESCE(?, payload) WHERE id = ?")
        .bind(status)
        .bind(payload)
        .bind(task_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch approved jobs ordered by creation time.
pub async fn fetch_ready_jobs(pool: &SqlitePool, limit: i64) -> Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
		"SELECT id, file_path, status, group_key, group_label, COALESCE(group_kind, 'file') AS group_kind, created_at FROM jobs WHERE status = 'APPROVED' ORDER BY created_at ASC LIMIT ?"
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch all tasks for a given job, ordered by step.
pub async fn fetch_tasks_for_job(pool: &SqlitePool, job_id: i64) -> Result<Vec<Task>> {
    let rows = sqlx::query_as::<_, Task>(
        "SELECT id, job_id, step_order, task_type, payload, status FROM tasks WHERE job_id = ? ORDER BY step_order ASC"
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch jobs that were interrupted mid-processing (for resumption on startup).
pub async fn fetch_resumable_jobs(pool: &SqlitePool) -> Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
		"SELECT id, file_path, status, group_key, group_label, COALESCE(group_kind, 'file') AS group_kind, created_at FROM jobs WHERE status = 'PROCESSING' ORDER BY created_at ASC"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List jobs with pagination.
pub async fn list_jobs(pool: &SqlitePool, limit: i64, offset: i64) -> Result<Vec<JobWithAnalysis>> {
    let rows = sqlx::query(
        "SELECT j.id, j.file_path, j.status, j.group_key, j.group_label, COALESCE(j.group_kind, 'file') AS group_kind, j.created_at, a.probe_json, a.decision_json
         FROM jobs j
         LEFT JOIN job_analysis a ON a.job_id = j.id
         ORDER BY j.created_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        let probe = row
            .try_get::<Option<String>, _>("probe_json")?
            .map(|json| serde_json::from_str::<MediaProbe>(&json))
            .transpose()?;
        let decision = row
            .try_get::<Option<String>, _>("decision_json")?
            .map(|json| serde_json::from_str::<ProcessingDecision>(&json))
            .transpose()?;

        jobs.push(JobWithAnalysis {
            id: row.try_get("id")?,
            file_path: row.try_get("file_path")?,
            status: row.try_get("status")?,
            group_key: row.try_get("group_key")?,
            group_label: row.try_get("group_label")?,
            group_kind: row.try_get("group_kind")?,
            created_at: row.try_get("created_at")?,
            probe,
            decision,
        });
    }

    Ok(jobs)
}

pub async fn fetch_media_metadata(
    pool: &SqlitePool,
    file_path: &str,
) -> Result<Option<CachedMediaMetadata>> {
    let row = sqlx::query_as::<_, CachedMediaMetadata>(
        "SELECT file_path, size_bytes, modified_at, format, duration_secs, video_codec, audio_codec, width, height, audio_channels, stream_count, probe_json, updated_at
         FROM media_metadata WHERE file_path = ?",
    )
    .bind(file_path)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn upsert_media_metadata(
    pool: &SqlitePool,
    file_path: &str,
    size_bytes: u64,
    modified_at: u64,
    probe: &MediaProbe,
) -> Result<()> {
    let video_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video");
    let audio_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "audio");
    let probe_json = serde_json::to_string(probe)?;

    sqlx::query(
        "INSERT INTO media_metadata (
            file_path, size_bytes, modified_at, format, duration_secs, video_codec, audio_codec,
            width, height, audio_channels, stream_count, probe_json, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(file_path) DO UPDATE SET
            size_bytes = excluded.size_bytes,
            modified_at = excluded.modified_at,
            format = excluded.format,
            duration_secs = excluded.duration_secs,
            video_codec = excluded.video_codec,
            audio_codec = excluded.audio_codec,
            width = excluded.width,
            height = excluded.height,
            audio_channels = excluded.audio_channels,
            stream_count = excluded.stream_count,
            probe_json = excluded.probe_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(file_path)
    .bind(size_bytes as i64)
    .bind(modified_at as i64)
    .bind(&probe.format)
    .bind(probe.duration_secs)
    .bind(video_stream.map(|stream| stream.codec_name.clone()))
    .bind(audio_stream.map(|stream| stream.codec_name.clone()))
    .bind(video_stream.and_then(|stream| stream.width).map(i64::from))
    .bind(video_stream.and_then(|stream| stream.height).map(i64::from))
    .bind(
        audio_stream
            .and_then(|stream| stream.channels)
            .map(i64::from),
    )
    .bind(probe.streams.len() as i64)
    .bind(probe_json)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn insert_library_event(
    pool: &SqlitePool,
    relative_path: &str,
    file_path: &str,
    change_type: &str,
    occurred_at: u64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO library_events (relative_path, file_path, change_type, occurred_at) VALUES (?, ?, ?, ?)",
    )
    .bind(relative_path)
    .bind(file_path)
    .bind(change_type)
    .bind(occurred_at as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_library_events(pool: &SqlitePool, limit: i64) -> Result<Vec<LibraryEventRow>> {
    let rows = sqlx::query_as::<_, LibraryEventRow>(
        "SELECT id, relative_path, file_path, change_type, occurred_at
         FROM library_events ORDER BY occurred_at DESC, id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn upsert_selected_internet_metadata(
    pool: &SqlitePool,
    relative_path: &str,
    selected: &InternetMetadataMatch,
) -> Result<()> {
    let genres_json = serde_json::to_string(&selected.genres)?;
    sqlx::query(
        "INSERT INTO selected_internet_metadata (
            relative_path, provider, title, year, media_kind, imdb_id, tvdb_id, overview,
            rating, genres_json, poster_url, source_url, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(relative_path) DO UPDATE SET
            provider = excluded.provider,
            title = excluded.title,
            year = excluded.year,
            media_kind = excluded.media_kind,
            imdb_id = excluded.imdb_id,
            tvdb_id = excluded.tvdb_id,
            overview = excluded.overview,
            rating = excluded.rating,
            genres_json = excluded.genres_json,
            poster_url = excluded.poster_url,
            source_url = excluded.source_url,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(relative_path)
    .bind(&selected.provider)
    .bind(&selected.title)
    .bind(selected.year.map(i64::from))
    .bind(&selected.media_kind)
    .bind(&selected.imdb_id)
    .bind(selected.tvdb_id.map(|v| v as i64))
    .bind(&selected.overview)
    .bind(selected.rating)
    .bind(genres_json)
    .bind(&selected.poster_url)
    .bind(&selected.source_url)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn fetch_selected_internet_metadata(
    pool: &SqlitePool,
    relative_path: &str,
) -> Result<Option<SelectedInternetMetadataRow>> {
    let row = sqlx::query_as::<_, SelectedInternetMetadataRow>(
        "SELECT relative_path, provider, title, year, media_kind, imdb_id, tvdb_id, overview,
                rating, genres_json, poster_url, source_url, updated_at
         FROM selected_internet_metadata
         WHERE relative_path = ?",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn find_related_selected_internet_metadata_paths(
    pool: &SqlitePool,
    relative_path: &str,
    imdb_id: Option<&str>,
    tvdb_id: Option<i64>,
) -> Result<Vec<String>> {
    if imdb_id.is_none() && tvdb_id.is_none() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_scalar::<_, String>(
        "SELECT relative_path
         FROM selected_internet_metadata
         WHERE relative_path != ?1
           AND ((?2 IS NOT NULL AND imdb_id = ?2) OR (?3 IS NOT NULL AND tvdb_id = ?3))
         ORDER BY relative_path ASC",
    )
    .bind(relative_path)
    .bind(imdb_id)
    .bind(tvdb_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn rename_selected_internet_metadata(
    pool: &SqlitePool,
    current_relative_path: &str,
    target_relative_path: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE selected_internet_metadata
         SET relative_path = ?, updated_at = CURRENT_TIMESTAMP
         WHERE relative_path = ?",
    )
    .bind(target_relative_path)
    .bind(current_relative_path)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upsert_library_index_entry(
    pool: &SqlitePool,
    relative_path: &str,
    file_path: &str,
    file_name: &str,
    extension: &str,
    media_type: &str,
    size_bytes: u64,
    modified_at: u64,
    library_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO library_index (
            relative_path, file_path, file_name, extension, media_type,
            size_bytes, modified_at, library_id, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(relative_path) DO UPDATE SET
            file_path = excluded.file_path,
            file_name = excluded.file_name,
            extension = excluded.extension,
            media_type = excluded.media_type,
            size_bytes = excluded.size_bytes,
            modified_at = excluded.modified_at,
            library_id = excluded.library_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(relative_path)
    .bind(file_path)
    .bind(file_name)
    .bind(extension)
    .bind(media_type)
    .bind(size_bytes as i64)
    .bind(modified_at as i64)
    .bind(library_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_library_index_entry(pool: &SqlitePool, relative_path: &str) -> Result<()> {
    sqlx::query("DELETE FROM library_index WHERE relative_path = ?")
        .bind(relative_path)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_library_index(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM library_index")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_library_index(
    pool: &SqlitePool,
    query_like: Option<&str>,
    library_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<LibraryIndexRow>> {
    let rows = sqlx::query_as::<_, LibraryIndexRow>(
                "SELECT l.relative_path, l.file_name, l.extension, l.media_type, l.size_bytes, l.modified_at, l.library_id,
                                m.managed_status,
                                CASE WHEN m.sidecar_path IS NULL THEN 0 ELSE 1 END AS has_sidecar,
                                CASE WHEN m.selected_metadata_json IS NULL THEN 0 ELSE 1 END AS has_selected_metadata,
                                m.selected_metadata_json
                 FROM library_index l
                 LEFT JOIN managed_items m ON m.relative_path = l.relative_path
                 WHERE (?1 IS NULL OR l.library_id = ?1)
                     AND (?2 IS NULL OR lower(l.relative_path) LIKE ?2)
                 ORDER BY l.modified_at DESC, l.relative_path ASC
                 LIMIT ?3 OFFSET ?4",
    )
    .bind(library_id)
    .bind(query_like)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn count_library_index(
    pool: &SqlitePool,
    query_like: Option<&str>,
    library_id: Option<&str>,
) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM library_index
         WHERE (?1 IS NULL OR library_id = ?1)
           AND (?2 IS NULL OR lower(relative_path) LIKE ?2)",
    )
    .bind(library_id)
    .bind(query_like)
    .fetch_one(pool)
    .await?;

    Ok(count)
}

pub async fn summarize_library_index(
    pool: &SqlitePool,
    library_id: Option<&str>,
) -> Result<LibrarySummaryRow> {
    let row = sqlx::query_as::<_, LibrarySummaryRow>(
        "SELECT
            COUNT(*) AS total_items,
            COALESCE(SUM(size_bytes), 0) AS total_bytes,
            SUM(CASE WHEN media_type = 'video' THEN 1 ELSE 0 END) AS video_items,
            SUM(CASE WHEN media_type = 'audio' THEN 1 ELSE 0 END) AS audio_items,
            SUM(CASE WHEN media_type NOT IN ('video', 'audio') THEN 1 ELSE 0 END) AS other_items
         FROM library_index
         WHERE (?1 IS NULL OR library_id = ?1)",
    )
    .bind(library_id)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn list_duplicate_candidates(
    pool: &SqlitePool,
    library_id: Option<&str>,
) -> Result<Vec<DuplicateCandidateRow>> {
    let rows = sqlx::query_as::<_, DuplicateCandidateRow>(
        "SELECT l.relative_path, l.file_name, l.library_id,
                s.provider, s.title, s.year, s.media_kind, s.imdb_id, s.tvdb_id
         FROM selected_internet_metadata s
         INNER JOIN library_index l ON l.relative_path = s.relative_path
         WHERE s.media_kind = 'movie'
           AND (?1 IS NULL OR l.library_id = ?1)
           AND ((s.imdb_id IS NOT NULL AND trim(s.imdb_id) != '') OR s.tvdb_id IS NOT NULL)
         ORDER BY COALESCE(s.imdb_id, ''), COALESCE(s.tvdb_id, 0), l.relative_path ASC",
    )
    .bind(library_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn fetch_library_scan_state(pool: &SqlitePool) -> Result<LibraryScanStateRow> {
    let row = sqlx::query_as::<_, LibraryScanStateRow>(
        "SELECT status, scanned_items, total_items, started_at, completed_at, last_scan_at, last_error
         FROM library_scan_state WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn try_begin_library_scan(pool: &SqlitePool, started_at: u64) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE library_scan_state
         SET status = 'running', scanned_items = 0, total_items = 0,
             started_at = ?, completed_at = NULL, last_error = NULL
         WHERE id = 1 AND status != 'running'",
    )
    .bind(started_at as i64)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn update_library_scan_progress(
    pool: &SqlitePool,
    scanned_items: usize,
    total_items: usize,
) -> Result<()> {
    sqlx::query(
        "UPDATE library_scan_state
         SET scanned_items = ?, total_items = ?
         WHERE id = 1",
    )
    .bind(scanned_items as i64)
    .bind(total_items as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn complete_library_scan(
    pool: &SqlitePool,
    completed_at: u64,
    scanned_items: usize,
) -> Result<()> {
    sqlx::query(
        "UPDATE library_scan_state
         SET status = 'idle', scanned_items = ?, total_items = ?,
             completed_at = ?, last_scan_at = ?, last_error = NULL
         WHERE id = 1",
    )
    .bind(scanned_items as i64)
    .bind(scanned_items as i64)
    .bind(completed_at as i64)
    .bind(completed_at as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn fail_library_scan(
    pool: &SqlitePool,
    completed_at: u64,
    scanned_items: usize,
    total_items: usize,
    error_message: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE library_scan_state
         SET status = 'error', scanned_items = ?, total_items = ?,
             completed_at = ?, last_error = ?
         WHERE id = 1",
    )
    .bind(scanned_items as i64)
    .bind(total_items as i64)
    .bind(completed_at as i64)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn fetch_managed_item(
    pool: &SqlitePool,
    relative_path: &str,
) -> Result<Option<ManagedItemRow>> {
    let row = sqlx::query_as::<_, ManagedItemRow>(
        "SELECT relative_path, file_path, file_name, media_type, size_bytes, modified_at,
                library_id, managed_status, selected_metadata_json, last_decision_json,
                sidecar_path, first_seen_at, last_seen_at, updated_at
         FROM managed_items
         WHERE relative_path = ?",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn upsert_managed_item(
    pool: &SqlitePool,
    relative_path: &str,
    file_path: &str,
    file_name: &str,
    media_type: &str,
    size_bytes: u64,
    modified_at: u64,
    library_id: Option<&str>,
    managed_status: &str,
    selected_metadata_json: Option<&str>,
    last_decision_json: Option<&str>,
    sidecar_path: Option<&str>,
    first_seen_at: u64,
    last_seen_at: u64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO managed_items (
            relative_path, file_path, file_name, media_type, size_bytes, modified_at,
            library_id, managed_status, selected_metadata_json, last_decision_json,
            sidecar_path, first_seen_at, last_seen_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(relative_path) DO UPDATE SET
            file_path = excluded.file_path,
            file_name = excluded.file_name,
            media_type = excluded.media_type,
            size_bytes = excluded.size_bytes,
            modified_at = excluded.modified_at,
            library_id = excluded.library_id,
            managed_status = excluded.managed_status,
            selected_metadata_json = excluded.selected_metadata_json,
            last_decision_json = excluded.last_decision_json,
            sidecar_path = excluded.sidecar_path,
            first_seen_at = managed_items.first_seen_at,
            last_seen_at = excluded.last_seen_at,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(relative_path)
    .bind(file_path)
    .bind(file_name)
    .bind(media_type)
    .bind(size_bytes as i64)
    .bind(modified_at as i64)
    .bind(library_id)
    .bind(managed_status)
    .bind(selected_metadata_json)
    .bind(last_decision_json)
    .bind(sidecar_path)
    .bind(first_seen_at as i64)
    .bind(last_seen_at as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_managed_item(pool: &SqlitePool, relative_path: &str) -> Result<()> {
    sqlx::query("DELETE FROM managed_items WHERE relative_path = ?")
        .bind(relative_path)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_stale_managed_items(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "DELETE FROM managed_items
         WHERE relative_path NOT IN (SELECT relative_path FROM library_index)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn rename_managed_item_path(
    pool: &SqlitePool,
    current_relative_path: &str,
    target_relative_path: &str,
    file_path: &str,
    file_name: &str,
    sidecar_path: Option<&str>,
    last_seen_at: u64,
) -> Result<()> {
    sqlx::query(
        "UPDATE managed_items
         SET relative_path = ?, file_path = ?, file_name = ?, sidecar_path = ?,
             last_seen_at = ?, updated_at = CURRENT_TIMESTAMP
         WHERE relative_path = ?",
    )
    .bind(target_relative_path)
    .bind(file_path)
    .bind(file_name)
    .bind(sidecar_path)
    .bind(last_seen_at as i64)
    .bind(current_relative_path)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_managed_item_status(
    pool: &SqlitePool,
    relative_path: &str,
    managed_status: &str,
    last_seen_at: u64,
) -> Result<()> {
    sqlx::query(
        "UPDATE managed_items
         SET managed_status = ?, last_seen_at = ?, updated_at = CURRENT_TIMESTAMP
         WHERE relative_path = ?",
    )
    .bind(managed_status)
    .bind(last_seen_at as i64)
    .bind(relative_path)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_unprocessed_managed_items(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<ManagedItemRow>> {
    let rows = sqlx::query_as::<_, ManagedItemRow>(
        "SELECT relative_path, file_path, file_name, media_type, size_bytes, modified_at,
                library_id, managed_status, selected_metadata_json, last_decision_json,
                sidecar_path, first_seen_at, last_seen_at, updated_at
         FROM managed_items
         WHERE managed_status = 'UNPROCESSED'
         ORDER BY modified_at DESC, relative_path ASC
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn list_managed_items_filtered(
    pool: &SqlitePool,
    managed_status: Option<&str>,
    missing_metadata_only: bool,
    missing_sidecar_only: bool,
    needs_attention_only: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<ManagedItemRow>> {
    let rows = sqlx::query_as::<_, ManagedItemRow>(
        "SELECT relative_path, file_path, file_name, media_type, size_bytes, modified_at,
                library_id, managed_status, selected_metadata_json, last_decision_json,
                sidecar_path, first_seen_at, last_seen_at, updated_at
         FROM managed_items
         WHERE (?1 IS NULL OR managed_status = ?1)
           AND (?2 = 0 OR (selected_metadata_json IS NULL AND managed_status NOT IN ('KEPT_ORIGINAL', 'PROCESSED')))
           AND (?3 = 0 OR sidecar_path IS NULL)
           AND (?4 = 0 OR (
               managed_status IN ('UNPROCESSED', 'FAILED', 'AWAITING_APPROVAL')
               OR (selected_metadata_json IS NULL AND managed_status NOT IN ('KEPT_ORIGINAL', 'PROCESSED'))
               OR sidecar_path IS NULL
           ))
         ORDER BY
           CASE managed_status
               WHEN 'FAILED' THEN 0
               WHEN 'UNPROCESSED' THEN 1
               WHEN 'AWAITING_APPROVAL' THEN 2
               WHEN 'APPROVED' THEN 3
               WHEN 'REVIEWED' THEN 4
               WHEN 'KEPT_ORIGINAL' THEN 5
               WHEN 'PROCESSED' THEN 6
               ELSE 7
           END,
           modified_at DESC,
           relative_path ASC
         LIMIT ?5 OFFSET ?6",
    )
    .bind(managed_status)
    .bind(if missing_metadata_only { 1 } else { 0 })
    .bind(if missing_sidecar_only { 1 } else { 0 })
    .bind(if needs_attention_only { 1 } else { 0 })
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn summarize_managed_items(pool: &SqlitePool) -> Result<ManagedItemsSummaryRow> {
    let row = sqlx::query_as::<_, ManagedItemsSummaryRow>(
        "SELECT
            COUNT(*) AS total_items,
            COALESCE(SUM(CASE
                WHEN managed_status IN ('UNPROCESSED', 'FAILED', 'AWAITING_APPROVAL')
                    OR (selected_metadata_json IS NULL AND managed_status NOT IN ('KEPT_ORIGINAL', 'PROCESSED'))
                    OR sidecar_path IS NULL
                THEN 1 ELSE 0
            END), 0) AS needs_attention_count,
            COALESCE(SUM(CASE WHEN managed_status = 'UNPROCESSED' THEN 1 ELSE 0 END), 0) AS unprocessed_count,
            COALESCE(SUM(CASE WHEN managed_status = 'REVIEWED' THEN 1 ELSE 0 END), 0) AS reviewed_count,
            COALESCE(SUM(CASE WHEN managed_status = 'KEPT_ORIGINAL' THEN 1 ELSE 0 END), 0) AS kept_original_count,
            COALESCE(SUM(CASE WHEN managed_status = 'AWAITING_APPROVAL' THEN 1 ELSE 0 END), 0) AS awaiting_approval_count,
            COALESCE(SUM(CASE WHEN managed_status = 'APPROVED' THEN 1 ELSE 0 END), 0) AS approved_count,
            COALESCE(SUM(CASE WHEN managed_status = 'PROCESSED' THEN 1 ELSE 0 END), 0) AS processed_count,
            COALESCE(SUM(CASE WHEN managed_status = 'FAILED' THEN 1 ELSE 0 END), 0) AS failed_count,
            COALESCE(SUM(CASE WHEN selected_metadata_json IS NULL AND managed_status NOT IN ('KEPT_ORIGINAL', 'PROCESSED') THEN 1 ELSE 0 END), 0) AS missing_metadata_count,
            COALESCE(SUM(CASE WHEN sidecar_path IS NULL THEN 1 ELSE 0 END), 0) AS missing_sidecar_count
         FROM managed_items",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}
