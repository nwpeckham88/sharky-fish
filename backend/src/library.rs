use anyhow::Result;
use serde::Serialize;

use crate::config::AppConfig;
use crate::db;
use crate::internet_metadata::InternetMetadataMatch;
use crate::organizer;

#[derive(Debug, Clone, Serialize)]
pub struct LibraryRoots {
    pub library_path: String,
    pub ingest_path: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LibrarySummary {
    pub total_items: usize,
    pub total_bytes: u64,
    pub video_items: usize,
    pub audio_items: usize,
    pub other_items: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    pub relative_path: String,
    pub file_name: String,
    pub extension: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub modified_at: Option<u64>,
    pub library_id: Option<String>,
    pub managed_status: Option<String>,
    pub has_sidecar: bool,
    pub has_selected_metadata: bool,
    pub organize_target_path: Option<String>,
    pub organize_needed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryScanStatus {
    pub status: String,
    pub scanned_items: usize,
    pub total_items: usize,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub last_scan_at: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryResponse {
    pub items: Vec<LibraryEntry>,
    pub total_items: usize,
    pub limit: usize,
    pub offset: usize,
    pub summary: LibrarySummary,
    pub roots: LibraryRoots,
    pub scan: LibraryScanStatus,
}

pub async fn list_from_index(
    pool: &sqlx::SqlitePool,
    config: &AppConfig,
    library_root: String,
    ingest_root: String,
    query: Option<String>,
    library_id: Option<String>,
    limit: usize,
    offset: usize,
) -> Result<LibraryResponse> {
    let query_like = query
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .map(|value| format!("%{}%", value));

    let rows = db::list_library_index(
        pool,
        query_like.as_deref(),
        library_id.as_deref(),
        limit as i64,
        offset as i64,
    )
    .await?;

    let total_items =
        db::count_library_index(pool, query_like.as_deref(), library_id.as_deref()).await?;
    let summary_row = db::summarize_library_index(pool, library_id.as_deref()).await?;
    let scan_state = db::fetch_library_scan_state(pool).await?;

    Ok(LibraryResponse {
        items: rows
            .into_iter()
            .map(|row| {
                let organize_target_path = row
                    .selected_metadata_json
                    .as_deref()
                    .and_then(|value| serde_json::from_str::<InternetMetadataMatch>(value).ok())
                    .and_then(|selected| {
                        organizer::preview_target_relative_path(
                            config,
                            &row.relative_path,
                            row.library_id.as_deref(),
                            &selected,
                        )
                        .ok()
                    });
                let organize_needed = organize_target_path
                    .as_deref()
                    .map(|target| target != row.relative_path)
                    .unwrap_or(false);

                LibraryEntry {
                    relative_path: row.relative_path,
                    file_name: row.file_name,
                    extension: row.extension,
                    media_type: row.media_type,
                    size_bytes: row.size_bytes.max(0) as u64,
                    modified_at: Some(row.modified_at.max(0) as u64),
                    library_id: row.library_id,
                    managed_status: row.managed_status,
                    has_sidecar: row.has_sidecar,
                    has_selected_metadata: row.has_selected_metadata,
                    organize_target_path,
                    organize_needed,
                }
            })
            .collect(),
        total_items: total_items.max(0) as usize,
        limit,
        offset,
        summary: LibrarySummary {
            total_items: summary_row.total_items.max(0) as usize,
            total_bytes: summary_row.total_bytes.max(0) as u64,
            video_items: summary_row.video_items.max(0) as usize,
            audio_items: summary_row.audio_items.max(0) as usize,
            other_items: summary_row.other_items.max(0) as usize,
        },
        roots: LibraryRoots {
            library_path: library_root,
            ingest_path: ingest_root,
        },
        scan: LibraryScanStatus {
            status: scan_state.status,
            scanned_items: scan_state.scanned_items.max(0) as usize,
            total_items: scan_state.total_items.max(0) as usize,
            started_at: scan_state.started_at.map(|v| v.max(0) as u64),
            completed_at: scan_state.completed_at.map(|v| v.max(0) as u64),
            last_scan_at: scan_state.last_scan_at.map(|v| v.max(0) as u64),
            last_error: scan_state.last_error,
        },
    })
}
