use anyhow::Result;
use crate::messages::MediaProbe;
use crate::internet_metadata::InternetMetadataMatch;
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
        "INSERT INTO library_scan_state (id, status, scanned_items, total_items)
         VALUES (1, 'idle', 0, 0)
         ON CONFLICT(id) DO NOTHING",
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
    sqlx::query("DELETE FROM library_index").execute(pool).await?;
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
        "SELECT relative_path, file_name, extension, media_type, size_bytes, modified_at, library_id
         FROM library_index
         WHERE (?1 IS NULL OR library_id = ?1)
           AND (?2 IS NULL OR lower(relative_path) LIKE ?2)
         ORDER BY modified_at DESC, relative_path ASC
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

pub async fn complete_library_scan(pool: &SqlitePool, completed_at: u64, scanned_items: usize) -> Result<()> {
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
