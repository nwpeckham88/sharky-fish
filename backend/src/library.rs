use anyhow::Result;
use serde::Serialize;
use std::cmp::Ordering;

use crate::config::AppConfig;
use crate::db;
use crate::filesystem_audit::FileSystemFacts;
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
    pub review_note: Option<String>,
    pub review_updated_at: Option<u64>,
    pub has_sidecar: bool,
    pub has_selected_metadata: bool,
    pub organize_target_path: Option<String>,
    pub organize_needed: bool,
    pub filesystem: FileSystemFacts,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibrarySortBy {
    #[default]
    ModifiedAt,
    SizeBytes,
    FileName,
    RelativePath,
    MediaType,
    ManagedStatus,
}

impl LibrarySortBy {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or_default() {
            "size_bytes" => Self::SizeBytes,
            "file_name" => Self::FileName,
            "relative_path" => Self::RelativePath,
            "media_type" => Self::MediaType,
            "managed_status" => Self::ManagedStatus,
            _ => Self::ModifiedAt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibrarySortDirection {
    Asc,
    #[default]
    Desc,
}

impl LibrarySortDirection {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or_default() {
            "asc" => Self::Asc,
            _ => Self::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryManagedStatusFilter {
    Exact(String),
    MissingMetadata,
    NoSidecar,
    OrganizeNeeded,
}

impl LibraryManagedStatusFilter {
    pub fn parse(value: Option<&str>) -> Option<Self> {
        let normalized = value?.trim();
        if normalized.is_empty() || normalized.eq_ignore_ascii_case("all") {
            return None;
        }

        Some(match normalized {
            "MISSING_METADATA" => Self::MissingMetadata,
            "NO_SIDECAR" => Self::NoSidecar,
            "ORGANIZE_NEEDED" => Self::OrganizeNeeded,
            other => Self::Exact(other.to_ascii_uppercase()),
        })
    }

    pub fn as_db_value(&self) -> Option<&str> {
        match self {
            Self::Exact(value) => Some(value.as_str()),
            Self::MissingMetadata => Some("MISSING_METADATA"),
            Self::NoSidecar => Some("NO_SIDECAR"),
            Self::OrganizeNeeded => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LibraryListOptions {
    pub query: Option<String>,
    pub library_id: Option<String>,
    pub media_type: Option<String>,
    pub managed_status: Option<LibraryManagedStatusFilter>,
    pub sort_by: LibrarySortBy,
    pub sort_direction: LibrarySortDirection,
    pub limit: usize,
    pub offset: usize,
}

pub async fn list_from_index(
    pool: &sqlx::SqlitePool,
    config: &AppConfig,
    library_root: String,
    ingest_root: String,
    options: LibraryListOptions,
) -> Result<LibraryResponse> {
    let query_like = options
        .query
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .map(|value| format!("%{}%", value));

    let media_type = options
        .media_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"))
        .map(str::to_ascii_lowercase);

    let managed_status = options.managed_status.clone();

    let (items, total_items) = if matches!(
        managed_status,
        Some(LibraryManagedStatusFilter::OrganizeNeeded)
    ) {
        let mut entries = db::list_library_index_candidates(
            pool,
            query_like.as_deref(),
            options.library_id.as_deref(),
            media_type.as_deref(),
            true,
        )
        .await?
        .into_iter()
        .filter_map(|row| map_library_entry(config, row).ok())
        .filter(|entry| entry.organize_needed)
        .collect::<Vec<_>>();

        sort_library_entries(&mut entries, options.sort_by, options.sort_direction);

        let total_items = entries.len();
        let start = options.offset.min(total_items);
        let end = (start + options.limit).min(total_items);
        (entries[start..end].to_vec(), total_items)
    } else {
        let rows = db::list_library_index(
            pool,
            query_like.as_deref(),
            options.library_id.as_deref(),
            media_type.as_deref(),
            managed_status
                .as_ref()
                .and_then(LibraryManagedStatusFilter::as_db_value),
            options.sort_by,
            options.sort_direction,
            options.limit as i64,
            options.offset as i64,
        )
        .await?;

        let total_items = db::count_library_index(
            pool,
            query_like.as_deref(),
            options.library_id.as_deref(),
            media_type.as_deref(),
            managed_status
                .as_ref()
                .and_then(LibraryManagedStatusFilter::as_db_value),
        )
        .await?;

        (
            rows.into_iter()
                .filter_map(|row| map_library_entry(config, row).ok())
                .collect::<Vec<_>>(),
            total_items.max(0) as usize,
        )
    };

    let summary_row = db::summarize_library_index(pool, options.library_id.as_deref()).await?;
    let scan_state = db::fetch_library_scan_state(pool).await?;

    Ok(LibraryResponse {
        items,
        total_items,
        limit: options.limit,
        offset: options.offset,
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

fn map_library_entry(config: &AppConfig, row: db::LibraryIndexRow) -> Result<LibraryEntry> {
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

    Ok(LibraryEntry {
        relative_path: row.relative_path,
        file_name: row.file_name,
        extension: row.extension,
        media_type: row.media_type,
        size_bytes: row.size_bytes.max(0) as u64,
        modified_at: Some(row.modified_at.max(0) as u64),
        library_id: row.library_id,
        managed_status: row.managed_status,
        review_note: row.review_note,
        review_updated_at: row.review_updated_at.map(|value| value.max(0) as u64),
        has_sidecar: row.has_sidecar,
        has_selected_metadata: row.has_selected_metadata,
        organize_target_path,
        organize_needed,
        filesystem: FileSystemFacts {
            device_id: row.device_id.max(0) as u64,
            inode: row.inode.max(0) as u64,
            link_count: row.link_count.max(0) as u64,
            size_bytes: row.size_bytes.max(0) as u64,
            modified_at: row.modified_at.max(0) as u64,
            is_hard_linked: row.link_count > 1,
        },
    })
}

fn sort_library_entries(
    entries: &mut [LibraryEntry],
    sort_by: LibrarySortBy,
    sort_direction: LibrarySortDirection,
) {
    entries.sort_by(|left, right| {
        let ordering = match sort_by {
            LibrarySortBy::ModifiedAt => left
                .modified_at
                .cmp(&right.modified_at)
                .then_with(|| left.relative_path.cmp(&right.relative_path)),
            LibrarySortBy::SizeBytes => left
                .size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path)),
            LibrarySortBy::FileName => left
                .file_name
                .cmp(&right.file_name)
                .then_with(|| left.relative_path.cmp(&right.relative_path)),
            LibrarySortBy::RelativePath => left.relative_path.cmp(&right.relative_path),
            LibrarySortBy::MediaType => left
                .media_type
                .cmp(&right.media_type)
                .then_with(|| left.relative_path.cmp(&right.relative_path)),
            LibrarySortBy::ManagedStatus => normalized_status(left)
                .cmp(normalized_status(right))
                .then_with(|| left.relative_path.cmp(&right.relative_path)),
        };

        match sort_direction {
            LibrarySortDirection::Asc => ordering,
            LibrarySortDirection::Desc => match ordering {
                Ordering::Less => Ordering::Greater,
                Ordering::Equal => Ordering::Equal,
                Ordering::Greater => Ordering::Less,
            },
        }
    });
}

fn normalized_status(entry: &LibraryEntry) -> &str {
    entry.managed_status.as_deref().unwrap_or("UNPROCESSED")
}
