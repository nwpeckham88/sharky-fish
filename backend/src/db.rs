use anyhow::Result;
use crate::messages::MediaProbe;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
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
    pub created_at: String,
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

/// Insert a new job and return its id.
pub async fn insert_job(pool: &SqlitePool, file_path: &str) -> Result<i64> {
    let id = sqlx::query("INSERT INTO jobs (file_path) VALUES (?)")
        .bind(file_path)
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok(id)
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

/// Fetch pending jobs ordered by creation time.
pub async fn fetch_pending_jobs(pool: &SqlitePool, limit: i64) -> Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
        "SELECT id, file_path, status, created_at FROM jobs WHERE status = 'PENDING' ORDER BY created_at ASC LIMIT ?"
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
        "SELECT id, file_path, status, created_at FROM jobs WHERE status = 'PROCESSING' ORDER BY created_at ASC"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List jobs with pagination.
pub async fn list_jobs(pool: &SqlitePool, limit: i64, offset: i64) -> Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
        "SELECT id, file_path, status, created_at FROM jobs ORDER BY created_at DESC LIMIT ? OFFSET ?"
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn fetch_media_metadata(pool: &SqlitePool, file_path: &str) -> Result<Option<CachedMediaMetadata>> {
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
    let video_stream = probe.streams.iter().find(|stream| stream.codec_type == "video");
    let audio_stream = probe.streams.iter().find(|stream| stream.codec_type == "audio");
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
    .bind(audio_stream.and_then(|stream| stream.channels).map(i64::from))
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
