use crate::config::AppConfig;
use crate::db;
use crate::library_index;
use crate::messages::{IngestEvent, LibraryChange, SseEvent};
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{info, warn};

const WATCHER_EVENT_BUFFER_CAPACITY: usize = 4096;
const WATCHER_DROP_LOG_INTERVAL_SECS: u64 = 30;

/// The Watcher actor monitors the ingest directory for new media files via
/// filesystem notification APIs (inotify on Linux) and forwards events to the
/// Identifier.
pub struct WatcherActor {
    ingest_path: PathBuf,
    library_path: PathBuf,
    tx: mpsc::Sender<IngestEvent>,
    sse_tx: broadcast::Sender<SseEvent>,
    pool: SqlitePool,
    config: Arc<RwLock<AppConfig>>,
}

impl WatcherActor {
    pub fn new(
        ingest_path: PathBuf,
        library_path: PathBuf,
        tx: mpsc::Sender<IngestEvent>,
        sse_tx: broadcast::Sender<SseEvent>,
        pool: SqlitePool,
        config: Arc<RwLock<AppConfig>>,
    ) -> Self {
        Self {
            ingest_path,
            library_path,
            tx,
            sse_tx,
            pool,
            config,
        }
    }

    pub async fn run(self) -> Result<()> {
        let (notify_tx, mut notify_rx) =
            mpsc::channel::<notify::Result<Event>>(WATCHER_EVENT_BUFFER_CAPACITY);

        let ingest_path = self.ingest_path.clone();
        let library_path = self.library_path.clone();
        let dropped_events = Arc::new(AtomicU64::new(0));
        let last_drop_log_at = Arc::new(AtomicU64::new(0));
        let dropped_events_for_cb = dropped_events.clone();
        let last_drop_log_at_for_cb = last_drop_log_at.clone();
        let ingest_for_cb = ingest_path.clone();
        let library_for_cb = library_path.clone();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
            let should_enqueue = match &res {
                Ok(event) => should_enqueue_event(event, &ingest_for_cb, &library_for_cb),
                Err(_) => true,
            };

            if !should_enqueue {
                return;
            }

            if notify_tx.try_send(res).is_err() {
                note_dropped_event(
                    dropped_events_for_cb.as_ref(),
                    last_drop_log_at_for_cb.as_ref(),
                );
            }
        })?;

        let mut watched_any = false;
        watched_any |= watch_path(&mut watcher, &ingest_path, "ingest");
        if library_path != ingest_path {
            watched_any |= watch_path(&mut watcher, &library_path, "library");
        }

        if !watched_any {
            warn!(
                ingest = %ingest_path.display(),
                library = %library_path.display(),
                "watcher: no paths could be monitored; check container volume mounts and permissions"
            );
        }

        let mut drop_log_tick = tokio::time::interval(Duration::from_secs(5));
        drop_log_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Event processing loop on the async runtime.
        loop {
            tokio::select! {
                _ = drop_log_tick.tick() => {
                    flush_dropped_event_summary(
                        dropped_events.as_ref(),
                        last_drop_log_at.as_ref(),
                    );
                }
                maybe_event = notify_rx.recv() => {
                    let Some(event_result) = maybe_event else {
                        break;
                    };

                    match event_result {
                        Ok(event) => self.handle_event(event).await,
                        Err(e) => warn!("watcher: notification error: {e}"),
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_event(&self, event: Event) {
        let Some(change) = change_label(&event.kind) else {
            return;
        };

        for path in event.paths {
            if is_media_file(&path) && path.starts_with(&self.ingest_path) {
                info!(file = %path.display(), "watcher: new media detected");
                let _ = self.tx.send(IngestEvent { path }).await;
                continue;
            }

            if is_media_file(&path) && path.starts_with(&self.library_path) {
                let (libraries, exclude_patterns, compute_checksums) = {
                    let cfg = self.config.read().await;
                    (cfg.libraries.clone(), cfg.scan_exclude_patterns.clone(), cfg.scan_compute_checksums)
                };

                if let Err(error) = library_index::apply_library_path_change(
                    &self.pool,
                    &self.library_path,
                    &libraries,
                    &exclude_patterns,
                    &path,
                    change,
                    compute_checksums,
                )
                .await
                {
                    warn!(err = %error, path = %path.display(), "watcher: failed to apply library index update");
                }

                let relative_path = path
                    .strip_prefix(&self.library_path)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let occurred_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);
                let change_event = LibraryChange {
                    relative_path,
                    path: path.display().to_string(),
                    change: change.to_string(),
                    occurred_at,
                };
                let _ = db::insert_library_event(
                    &self.pool,
                    &change_event.relative_path,
                    &change_event.path,
                    &change_event.change,
                    change_event.occurred_at,
                )
                .await;
                let _ = self.sse_tx.send(SseEvent::LibraryChange(change_event));
            }
        }
    }
}

fn change_label(kind: &EventKind) -> Option<&'static str> {
    match kind {
        EventKind::Create(_) => Some("created"),
        EventKind::Remove(_) => Some("removed"),
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => Some("renamed"),
        EventKind::Modify(_) => Some("modified"),
        _ => None,
    }
}

fn is_media_file(path: &Path) -> bool {
    library_index::detect_media_type(path).is_some()
}

fn is_metadata_sidecar(path: &Path) -> bool {
    library_index::is_metadata_sidecar_path(path)
}

fn should_enqueue_event(event: &Event, ingest_path: &Path, library_path: &Path) -> bool {
    if change_label(&event.kind).is_none() {
        return false;
    }

    event.paths.iter().any(|path| {
        (path.starts_with(ingest_path) && is_media_file(path))
            || (path.starts_with(library_path) && (is_media_file(path) || is_metadata_sidecar(path)))
    })
}

fn note_dropped_event(dropped_events: &AtomicU64, last_drop_log_at: &AtomicU64) {
    dropped_events.fetch_add(1, Ordering::Relaxed);
    flush_dropped_event_summary(dropped_events, last_drop_log_at);
}

fn flush_dropped_event_summary(dropped_events: &AtomicU64, last_drop_log_at: &AtomicU64) {
    let now = unix_now();
    let last = last_drop_log_at.load(Ordering::Relaxed);
    if now.saturating_sub(last) < WATCHER_DROP_LOG_INTERVAL_SECS {
        return;
    }

    let dropped = dropped_events.swap(0, Ordering::Relaxed);
    if dropped == 0 {
        return;
    }

    last_drop_log_at.store(now, Ordering::Relaxed);
    warn!(
        dropped_events = dropped,
        window_secs = WATCHER_DROP_LOG_INTERVAL_SECS,
        "watcher: dropped filesystem events because callback buffer was full"
    );
}

fn watch_path(watcher: &mut RecommendedWatcher, path: &Path, label: &str) -> bool {
    if !path.exists() {
        if let Err(error) = std::fs::create_dir_all(path) {
            warn!(path = %path.display(), kind = label, err = %error, "watcher: cannot create path");
            return false;
        }
    }

    match watcher.watch(path, RecursiveMode::Recursive) {
        Ok(()) => {
            info!(path = %path.display(), kind = label, "watcher: monitoring path");
            true
        }
        Err(error) => {
            warn!(path = %path.display(), kind = label, err = %error, "watcher: cannot monitor path");
            false
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}
