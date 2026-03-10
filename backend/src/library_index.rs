use crate::config::LibraryFolder;
use crate::db;
use crate::messages::{LibraryIndexScanProgress, SseEvent};

use anyhow::Result;
use futures::stream::StreamExt;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use tokio::task;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Clone)]
pub struct IndexCandidate {
    pub relative_path: String,
    pub file_path: String,
    pub file_name: String,
    pub extension: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub library_id: Option<String>,
}

pub async fn run_full_rescan(
    pool: SqlitePool,
    library_root: PathBuf,
    libraries: Vec<LibraryFolder>,
    exclude_patterns: Vec<String>,
    scan_concurrency: usize,
    scan_queue_capacity: usize,
    sse_tx: broadcast::Sender<SseEvent>,
) -> Result<bool> {
    let started_at = unix_now();
    if !db::try_begin_library_scan(&pool, started_at).await? {
        return Ok(false);
    }

    let run_result = async {
        db::update_library_scan_progress(&pool, 0, 0).await?;
        let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(LibraryIndexScanProgress {
            status: "running".into(),
            scanned_items: 0,
            total_items: 0,
            started_at: Some(started_at),
            completed_at: None,
            last_scan_at: None,
            last_error: None,
        }));

        db::clear_library_index(&pool).await?;

        let concurrency = scan_concurrency.max(1);
        let queue_capacity = scan_queue_capacity.max(1);
        let mut scanned_items = 0usize;

        let (tx, rx) = mpsc::channel::<IndexCandidate>(queue_capacity);
        let discovered_total = Arc::new(AtomicUsize::new(0));
        let producer_total = discovered_total.clone();
        let producer_root = library_root.clone();
        let producer_libraries = libraries;
        let producer_excludes = exclude_patterns;

        let producer = task::spawn_blocking(move || -> Result<()> {
            if !producer_root.exists() {
                return Ok(());
            }

            for entry in walkdir::WalkDir::new(&producer_root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| {
                    let rel = entry
                        .path()
                        .strip_prefix(&producer_root)
                        .unwrap_or(entry.path())
                        .to_string_lossy()
                        .replace('\\', "/");
                    !is_excluded_relative_path(&rel, &producer_excludes)
                })
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_file())
            {
                let path = entry.path();
                let Some(media_type) = detect_media_type(path) else {
                    continue;
                };

                let metadata = match entry.metadata() {
                    Ok(value) => value,
                    Err(_) => continue,
                };

                let modified_at = metadata
                    .modified()
                    .ok()
                    .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                    .map(|value| value.as_secs())
                    .unwrap_or(0);
                let relative_path = relative_path_string(&producer_root, path);
                let file_name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string();
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();

                producer_total.fetch_add(1, Ordering::Relaxed);
                let candidate = IndexCandidate {
                    relative_path: relative_path.clone(),
                    file_path: path.display().to_string(),
                    file_name,
                    extension,
                    media_type: media_type.to_string(),
                    size_bytes: metadata.len(),
                    modified_at,
                    library_id: match_library_id(&relative_path, &producer_libraries),
                };

                if tx.blocking_send(candidate).is_err() {
                    break;
                }
            }

            Ok(())
        });

        let mut stream = ReceiverStream::new(rx)
            .map(|candidate| {
                let pool = pool.clone();
                async move {
                    db::upsert_library_index_entry(
                        &pool,
                        &candidate.relative_path,
                        &candidate.file_path,
                        &candidate.file_name,
                        &candidate.extension,
                        &candidate.media_type,
                        candidate.size_bytes,
                        candidate.modified_at,
                        candidate.library_id.as_deref(),
                    )
                    .await
                }
            })
            .buffer_unordered(concurrency);

        while let Some(result) = stream.next().await {
            result?;
            scanned_items += 1;
            let total = discovered_total.load(Ordering::Relaxed).max(scanned_items);

            if scanned_items % 200 == 0 {
                db::update_library_scan_progress(&pool, scanned_items, total).await?;
                let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(LibraryIndexScanProgress {
                    status: "running".into(),
                    scanned_items,
                    total_items: total,
                    started_at: Some(started_at),
                    completed_at: None,
                    last_scan_at: None,
                    last_error: None,
                }));
            }
        }

        producer.await??;

        let total = discovered_total.load(Ordering::Relaxed).max(scanned_items);
        db::update_library_scan_progress(&pool, scanned_items, total).await?;

        let completed_at = unix_now();
        db::complete_library_scan(&pool, completed_at, scanned_items).await?;
        let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(LibraryIndexScanProgress {
            status: "idle".into(),
            scanned_items,
            total_items: total,
            started_at: Some(started_at),
            completed_at: Some(completed_at),
            last_scan_at: Some(completed_at),
            last_error: None,
        }));

        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(error) = run_result {
        let completed_at = unix_now();
        let scan_state = db::fetch_library_scan_state(&pool).await?;
        db::fail_library_scan(
            &pool,
            completed_at,
            scan_state.scanned_items.max(0) as usize,
            scan_state.total_items.max(0) as usize,
            &error.to_string(),
        )
        .await?;
        let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(LibraryIndexScanProgress {
            status: "error".into(),
            scanned_items: scan_state.scanned_items.max(0) as usize,
            total_items: scan_state.total_items.max(0) as usize,
            started_at: scan_state.started_at.map(|v| v.max(0) as u64),
            completed_at: Some(completed_at),
            last_scan_at: scan_state.last_scan_at.map(|v| v.max(0) as u64),
            last_error: Some(error.to_string()),
        }));
        return Err(error);
    }

    Ok(true)
}

pub async fn apply_library_path_change(
    pool: &SqlitePool,
    library_root: &Path,
    libraries: &[LibraryFolder],
    exclude_patterns: &[String],
    path: &Path,
    change: &str,
) -> Result<()> {
    let relative_path = relative_path_string(library_root, path);

    if is_excluded_relative_path(&relative_path, exclude_patterns) {
        return Ok(());
    }

    // Deletions and missing files should evict stale rows.
    if change == "removed" || !path.exists() {
        db::delete_library_index_entry(pool, &relative_path).await?;
        return Ok(());
    }

    let Some(media_type) = detect_media_type(path) else {
        db::delete_library_index_entry(pool, &relative_path).await?;
        return Ok(());
    };

    let metadata = match tokio::fs::metadata(path).await {
        Ok(value) => value,
        Err(_) => {
            db::delete_library_index_entry(pool, &relative_path).await?;
            return Ok(());
        }
    };

    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let library_id = match_library_id(&relative_path, libraries);
    db::upsert_library_index_entry(
        pool,
        &relative_path,
        &path.display().to_string(),
        &file_name,
        &extension,
        media_type,
        metadata.len(),
        modified_at,
        library_id.as_deref(),
    )
    .await?;

    Ok(())
}

pub fn detect_media_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "mkv" | "mp4" | "avi" | "mov" | "ts" | "webm" | "m4v" => Some("video"),
        "flac" | "mp3" | "wav" | "m4a" | "aac" | "ogg" => Some("audio"),
        "srt" | "ass" | "vtt" => Some("subtitle"),
        _ => None,
    }
}

pub fn is_excluded_relative_path(relative_path: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }

    let normalized = relative_path.to_ascii_lowercase();
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    patterns.iter().any(|pattern| {
        let p = pattern.trim().to_ascii_lowercase();
        if p.is_empty() {
            return false;
        }
        segments.iter().any(|segment| segment == &p) || normalized.contains(&format!("/{}/", p))
    })
}

pub fn match_library_id(relative_path: &str, libraries: &[LibraryFolder]) -> Option<String> {
    for lib in libraries {
        let prefix = if lib.path.ends_with('/') {
            lib.path.clone()
        } else {
            format!("{}/", lib.path)
        };
        if relative_path.starts_with(&prefix) || relative_path == lib.path {
            return Some(lib.id.clone());
        }
    }
    None
}

fn relative_path_string(library_root: &Path, path: &Path) -> String {
    path.strip_prefix(library_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}
