use crate::config::LibraryFolder;
use crate::db;
use crate::filesystem_audit;
use crate::managed_items;
use crate::messages::{LibraryIndexScanProgress, SseEvent};

use anyhow::Result;
use futures::stream::StreamExt;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use tokio::task;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, info, warn};

const STALE_SCAN_TIMEOUT_SECS: u64 = 10 * 60;

#[derive(Debug, Clone)]
pub struct IndexCandidate {
    pub relative_path: String,
    pub file_path: String,
    pub file_name: String,
    pub extension: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub device_id: u64,
    pub inode: u64,
    pub link_count: u64,
    pub library_id: Option<String>,
}

pub async fn run_full_rescan(
    pool: SqlitePool,
    library_root: PathBuf,
    libraries: Vec<LibraryFolder>,
    exclude_patterns: Vec<String>,
    scan_concurrency: usize,
    scan_queue_capacity: usize,
    compute_checksums: bool,
    sse_tx: broadcast::Sender<SseEvent>,
) -> Result<bool> {
    let started_at = unix_now();
    info!(
        root = %library_root.display(),
        libraries = libraries.len(),
        concurrency = scan_concurrency,
        queue_capacity = scan_queue_capacity,
        compute_checksums,
        "library scan: attempting to start"
    );
    if !db::try_begin_library_scan(&pool, started_at).await? {
        let scan_state = db::fetch_library_scan_state(&pool).await?;
        let prior_started_at = scan_state.started_at.map(|v| v.max(0) as u64).unwrap_or(0);
        let running_age_secs = started_at.saturating_sub(prior_started_at);

        // Recovery: a stale "running" flag can remain after an unclean shutdown.
        // If it has been running too long, mark it failed and retry once.
        if scan_state.status == "running" && running_age_secs >= STALE_SCAN_TIMEOUT_SECS {
            warn!(
                started_at = prior_started_at,
                age_secs = running_age_secs,
                scanned = scan_state.scanned_items,
                total = scan_state.total_items,
                "library scan: stale running state detected; resetting before retry"
            );

            db::fail_library_scan(
                &pool,
                started_at,
                scan_state.scanned_items.max(0) as usize,
                scan_state.total_items.max(0) as usize,
                "stale running scan state reset automatically",
            )
            .await?;

            if !db::try_begin_library_scan(&pool, started_at).await? {
                warn!("library scan: retry failed because a scan is still marked running");
                return Ok(false);
            }
        } else {
            warn!(
                status = %scan_state.status,
                age_secs = running_age_secs,
                scanned = scan_state.scanned_items,
                total = scan_state.total_items,
                "library scan: another scan is already running — skipping"
            );
            return Ok(false);
        }
    }

    let run_result = async {
        db::update_library_scan_progress(&pool, 0, 0).await?;
        let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(
            LibraryIndexScanProgress {
                status: "running".into(),
                scanned_items: 0,
                total_items: 0,
                started_at: Some(started_at),
                completed_at: None,
                last_scan_at: None,
                last_error: None,
            },
        ));

        info!("library scan: clearing existing index");
        db::clear_library_index(&pool).await?;
        info!("library scan: index cleared, starting file discovery");

        let concurrency = scan_concurrency.max(1);
        let queue_capacity = scan_queue_capacity.max(1);
        let mut scanned_items = 0usize;
        let scan_start = Instant::now();
        let mut last_progress_at = Instant::now();

        let (tx, rx) = mpsc::channel::<IndexCandidate>(queue_capacity);
        let discovered_total = Arc::new(AtomicUsize::new(0));
        let scan_library_root = library_root.clone();
        let producer_total = discovered_total.clone();
        let producer_root = library_root.clone();
        let producer_libraries = libraries;
        let producer_excludes = exclude_patterns;

        let producer = task::spawn_blocking(move || -> Result<()> {
            if !producer_root.exists() {
                warn!(path = %producer_root.display(), "library scan: library root does not exist — no files will be discovered");
                return Ok(());
            }
            debug!(path = %producer_root.display(), "library scan: walking filesystem");

            if producer_libraries.is_empty() {
                warn!("library scan: no configured libraries; skipping file discovery");
                return Ok(());
            }

            let mut seen_relative_paths = HashSet::<String>::new();
            for library in &producer_libraries {
                let library_root = producer_root.join(&library.path);
                if !library_root.exists() {
                    warn!(
                        library_id = %library.id,
                        path = %library_root.display(),
                        "library scan: configured library path does not exist; skipping"
                    );
                    continue;
                }

                for entry in walkdir::WalkDir::new(&library_root)
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
                    if !seen_relative_paths.insert(relative_path.clone()) {
                        continue;
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
                        .to_ascii_lowercase();
                    let facts = filesystem_audit::file_system_facts(&metadata);

                    let discovered = producer_total.fetch_add(1, Ordering::Relaxed) + 1;
                    if discovered.is_multiple_of(500) {
                        debug!(discovered, "library scan: discovery progress");
                    }
                    let candidate = IndexCandidate {
                        relative_path: relative_path.clone(),
                        file_path: path.display().to_string(),
                        file_name,
                        extension,
                        media_type: media_type.to_string(),
                        size_bytes: metadata.len(),
                        modified_at,
                        device_id: facts.device_id,
                        inode: facts.inode,
                        link_count: facts.link_count,
                        library_id: Some(library.id.clone()),
                    };

                    if tx.blocking_send(candidate).is_err() {
                        break;
                    }
                }
            }

            Ok(())
        });

        let mut stream = ReceiverStream::new(rx)
            .map(|candidate| {
                let pool = pool.clone();
                let library_root = scan_library_root.clone();
                async move {
                    let checksum_blake3 = if compute_checksums {
                        let checksum_start = Instant::now();
                        let hash =
                            filesystem_audit::blake3_checksum(Path::new(&candidate.file_path)).await?;
                        let checksum_elapsed = checksum_start.elapsed();
                        if checksum_elapsed.as_secs() >= 2 {
                            warn!(
                                path = %candidate.file_path,
                                secs = checksum_elapsed.as_secs(),
                                "library scan: slow checksum (large file or slow storage?)"
                            );
                        }
                        Some(hash)
                    } else {
                        None
                    };

                    db::upsert_library_index_entry(
                        &pool,
                        &candidate.relative_path,
                        &candidate.file_path,
                        &candidate.file_name,
                        &candidate.extension,
                        &candidate.media_type,
                        candidate.size_bytes,
                        candidate.modified_at,
                        candidate.device_id,
                        candidate.inode,
                        candidate.link_count,
                        checksum_blake3.as_deref(),
                        candidate.library_id.as_deref(),
                    )
                    .await?;

                    managed_items::sync_library_file(
                        &pool,
                        &library_root,
                        managed_items::SyncLibraryFileInput {
                            relative_path: &candidate.relative_path,
                            file_path: &candidate.file_path,
                            file_name: &candidate.file_name,
                            media_type: &candidate.media_type,
                            size_bytes: candidate.size_bytes,
                            modified_at: candidate.modified_at,
                            library_id: candidate.library_id.as_deref(),
                        },
                    )
                    .await
                }
            })
            .buffer_unordered(concurrency);

        while let Some(result) = stream.next().await {
            result?;
            scanned_items += 1;
            let total = discovered_total.load(Ordering::Relaxed).max(scanned_items);

            if scanned_items.is_multiple_of(50) {
                let elapsed = scan_start.elapsed();
                let rate = if elapsed.as_secs() > 0 {
                    scanned_items as u64 / elapsed.as_secs()
                } else {
                    scanned_items as u64
                };
                let since_last = last_progress_at.elapsed();
                last_progress_at = Instant::now();
                info!(
                    scanned = scanned_items,
                    total,
                    elapsed_secs = elapsed.as_secs(),
                    files_per_sec = rate,
                    last_batch_ms = since_last.as_millis(),
                    "library scan: progress"
                );
            }

            if scanned_items.is_multiple_of(200) {
                db::update_library_scan_progress(&pool, scanned_items, total).await?;
                let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(
                    LibraryIndexScanProgress {
                        status: "running".into(),
                        scanned_items,
                        total_items: total,
                        started_at: Some(started_at),
                        completed_at: None,
                        last_scan_at: None,
                        last_error: None,
                    },
                ));
            }
        }

        producer.await??;

        let total = discovered_total.load(Ordering::Relaxed).max(scanned_items);
        info!(
            scanned = scanned_items,
            total,
            elapsed_secs = scan_start.elapsed().as_secs(),
            "library scan: all files processed, finalizing"
        );
        db::update_library_scan_progress(&pool, scanned_items, total).await?;
        db::delete_stale_managed_items(&pool).await?;

        let completed_at = unix_now();
        db::complete_library_scan(&pool, completed_at, scanned_items).await?;
        info!(
            scanned = scanned_items,
            total,
            elapsed_secs = scan_start.elapsed().as_secs(),
            "library scan: complete"
        );
        let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(
            LibraryIndexScanProgress {
                status: "idle".into(),
                scanned_items,
                total_items: total,
                started_at: Some(started_at),
                completed_at: Some(completed_at),
                last_scan_at: Some(completed_at),
                last_error: None,
            },
        ));

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
        let _ = sse_tx.send(SseEvent::LibraryIndexScanProgress(
            LibraryIndexScanProgress {
                status: "error".into(),
                scanned_items: scan_state.scanned_items.max(0) as usize,
                total_items: scan_state.total_items.max(0) as usize,
                started_at: scan_state.started_at.map(|v| v.max(0) as u64),
                completed_at: Some(completed_at),
                last_scan_at: scan_state.last_scan_at.map(|v| v.max(0) as u64),
                last_error: Some(error.to_string()),
            },
        ));
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
    compute_checksums: bool,
) -> Result<()> {
    let relative_path = relative_path_string(library_root, path);
    debug!(path = %path.display(), change, "library index: applying path change");

    if is_excluded_relative_path(&relative_path, exclude_patterns) {
        return Ok(());
    }

    if is_metadata_sidecar_path(path) {
        refresh_media_for_metadata_sidecar(pool, library_root, libraries, path, compute_checksums).await?;
        return Ok(());
    }

    // Deletions and missing files should evict stale rows.
    if change == "removed" || !path.exists() {
        db::delete_library_index_entry(pool, &relative_path).await?;
        managed_items::remove_missing_item(pool, &relative_path).await?;
        return Ok(());
    }

    let Some(media_type) = detect_media_type(path) else {
        db::delete_library_index_entry(pool, &relative_path).await?;
        managed_items::remove_missing_item(pool, &relative_path).await?;
        return Ok(());
    };

    let metadata = match tokio::fs::metadata(path).await {
        Ok(value) => value,
        Err(_) => {
            db::delete_library_index_entry(pool, &relative_path).await?;
            managed_items::remove_missing_item(pool, &relative_path).await?;
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
    let facts = filesystem_audit::file_system_facts(&metadata);
    let checksum_blake3 = if compute_checksums {
        Some(filesystem_audit::blake3_checksum(path).await?)
    } else {
        None
    };

    let library_id = match_library_id(&relative_path, libraries);
    if library_id.is_none() {
        db::delete_library_index_entry(pool, &relative_path).await?;
        managed_items::remove_missing_item(pool, &relative_path).await?;
        return Ok(());
    }

    db::upsert_library_index_entry(
        pool,
        &relative_path,
        &path.display().to_string(),
        &file_name,
        &extension,
        media_type,
        metadata.len(),
        modified_at,
        facts.device_id,
        facts.inode,
        facts.link_count,
        checksum_blake3.as_deref(),
        library_id.as_deref(),
    )
    .await?;

    managed_items::sync_library_file(
        pool,
        library_root,
        managed_items::SyncLibraryFileInput {
            relative_path: &relative_path,
            file_path: &path.display().to_string(),
            file_name: &file_name,
            media_type,
            size_bytes: metadata.len(),
            modified_at,
            library_id: library_id.as_deref(),
        },
    )
    .await?;

    Ok(())
}

async fn refresh_media_for_metadata_sidecar(
    pool: &SqlitePool,
    library_root: &Path,
    libraries: &[LibraryFolder],
    sidecar_path: &Path,
    compute_checksums: bool,
) -> Result<()> {
    let Some(parent) = sidecar_path.parent() else {
        return Ok(());
    };
    let Some(stem) = sidecar_path.file_stem().and_then(|value| value.to_str()) else {
        return Ok(());
    };

    let mut entries = match tokio::fs::read_dir(parent).await {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };

    while let Some(entry) = entries.next_entry().await? {
        let candidate = entry.path();
        if !candidate.is_file() {
            continue;
        }
        let candidate_stem = candidate.file_stem().and_then(|value| value.to_str());
        if candidate_stem != Some(stem) {
            continue;
        }
        let Some(media_type) = detect_media_type(&candidate) else {
            continue;
        };
        let metadata = match tokio::fs::metadata(&candidate).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let relative_path = relative_path_string(library_root, &candidate);
        let file_name = candidate
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let extension = candidate
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_secs())
            .unwrap_or(0);
        let facts = filesystem_audit::file_system_facts(&metadata);
        let checksum_blake3 = if compute_checksums {
            Some(filesystem_audit::blake3_checksum(&candidate).await?)
        } else {
            None
        };
        let library_id = match_library_id(&relative_path, libraries);
        if library_id.is_none() {
            continue;
        }

        db::upsert_library_index_entry(
            pool,
            &relative_path,
            &candidate.display().to_string(),
            &file_name,
            &extension,
            media_type,
            metadata.len(),
            modified_at,
            facts.device_id,
            facts.inode,
            facts.link_count,
            checksum_blake3.as_deref(),
            library_id.as_deref(),
        )
        .await?;

        managed_items::sync_library_file(
            pool,
            library_root,
            managed_items::SyncLibraryFileInput {
                relative_path: &relative_path,
                file_path: &candidate.display().to_string(),
                file_name: &file_name,
                media_type,
                size_bytes: metadata.len(),
                modified_at,
                library_id: library_id.as_deref(),
            },
        )
        .await?;
    }

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

pub fn is_metadata_sidecar_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("nfo"))
        .unwrap_or(false)
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
