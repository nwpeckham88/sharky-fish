use crate::config::AppConfig;
use crate::db;
use crate::library_index;
use crate::messages::{IngestEvent, LibraryChange, SseEvent};
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{info, warn};

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
        let (notify_tx, mut notify_rx) = mpsc::channel::<notify::Result<Event>>(256);

        // Spawn the blocking watcher on a dedicated thread since `notify` uses
        // synchronous callbacks.
        let ingest_path = self.ingest_path.clone();
        let library_path = self.library_path.clone();
        let _watcher = tokio::task::spawn_blocking(move || -> Result<RecommendedWatcher> {
            let _ = std::fs::create_dir_all(&ingest_path);
            let _ = std::fs::create_dir_all(&library_path);
            let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
                let _ = notify_tx.blocking_send(res);
            })?;
            watcher.watch(&ingest_path, RecursiveMode::Recursive)?;
            if library_path != ingest_path {
                watcher.watch(&library_path, RecursiveMode::Recursive)?;
            }
            info!(path = %ingest_path.display(), "watcher: monitoring ingest directory");
            info!(path = %library_path.display(), "watcher: monitoring library directory");
            // Keep watcher alive by blocking this thread.
            std::thread::park();
            Ok(watcher)
        });

        // Event processing loop on the async runtime.
        while let Some(event_result) = notify_rx.recv().await {
            match event_result {
                Ok(event) => self.handle_event(event).await,
                Err(e) => warn!("watcher: notification error: {e}"),
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
                let (libraries, exclude_patterns) = {
                    let cfg = self.config.read().await;
                    (cfg.libraries.clone(), cfg.scan_exclude_patterns.clone())
                };

                if let Err(error) = library_index::apply_library_path_change(
                    &self.pool,
                    &self.library_path,
                    &libraries,
                    &exclude_patterns,
                    &path,
                    change,
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

fn is_media_file(path: &PathBuf) -> bool {
    library_index::detect_media_type(path.as_path()).is_some()
}
