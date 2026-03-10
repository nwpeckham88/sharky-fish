use crate::db;
use crate::messages::{IngestEvent, LibraryChange, SseEvent};
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::sync::broadcast;
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
}

impl WatcherActor {
    pub fn new(
        ingest_path: PathBuf,
        library_path: PathBuf,
        tx: mpsc::Sender<IngestEvent>,
        sse_tx: broadcast::Sender<SseEvent>,
        pool: SqlitePool,
    ) -> Self {
        Self {
            ingest_path,
            library_path,
            tx,
            sse_tx,
            pool,
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
            let mut watcher =
                notify::recommended_watcher(move |res: notify::Result<Event>| {
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
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_lowercase().as_str(),
        "mkv" | "mp4" | "avi" | "mov" | "ts" | "flac" | "mp3" | "wav" | "m4a" | "webm"
    )
}
