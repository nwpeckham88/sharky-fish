use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::task;
use walkdir::WalkDir;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryResponse {
    pub items: Vec<LibraryEntry>,
    pub total_items: usize,
    pub limit: usize,
    pub offset: usize,
    pub summary: LibrarySummary,
    pub roots: LibraryRoots,
}

pub async fn scan_library(
    library_root: PathBuf,
    ingest_root: PathBuf,
    query: Option<String>,
    limit: usize,
    offset: usize,
) -> Result<LibraryResponse> {
    task::spawn_blocking(move || scan_library_blocking(&library_root, &ingest_root, query, limit, offset))
        .await?
}

fn scan_library_blocking(
    library_root: &Path,
    ingest_root: &Path,
    query: Option<String>,
    limit: usize,
    offset: usize,
) -> Result<LibraryResponse> {
    let query = query
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty());

    let roots = LibraryRoots {
        library_path: library_root.display().to_string(),
        ingest_path: ingest_root.display().to_string(),
    };

    if !library_root.exists() {
        return Ok(LibraryResponse {
            items: Vec::new(),
            total_items: 0,
            limit,
            offset,
            summary: LibrarySummary::default(),
            roots,
        });
    }

    let mut summary = LibrarySummary::default();
    let mut items = Vec::new();

    for entry in WalkDir::new(library_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        let Some(media_type) = detect_media_type(path) else {
            continue;
        };

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let size_bytes = metadata.len();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_secs());

        summary.total_items += 1;
        summary.total_bytes += size_bytes;
        match media_type.as_str() {
            "video" => summary.video_items += 1,
            "audio" => summary.audio_items += 1,
            _ => summary.other_items += 1,
        }

        let relative_path = path
            .strip_prefix(library_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        if let Some(filter) = &query {
            let haystack = relative_path.to_lowercase();
            if !haystack.contains(filter) {
                continue;
            }
        }

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_lowercase();

        items.push(LibraryEntry {
            relative_path,
            file_name,
            extension,
            media_type,
            size_bytes,
            modified_at,
        });
    }

    items.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });

    let total_items = items.len();
    let paged_items = items.into_iter().skip(offset).take(limit).collect();

    Ok(LibraryResponse {
        items: paged_items,
        total_items,
        limit,
        offset,
        summary,
        roots,
    })
}

fn detect_media_type(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "mkv" | "mp4" | "avi" | "mov" | "ts" | "webm" | "m4v" => "video",
        "flac" | "mp3" | "wav" | "m4a" | "aac" | "ogg" => "audio",
        "srt" | "ass" | "vtt" => "subtitle",
        _ => return None,
    };

    Some(media_type.to_string())
}