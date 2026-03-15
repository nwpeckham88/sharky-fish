use crate::config::AppConfig;
use crate::messages::{IdentifiedMedia, IngestEvent};
use crate::metadata::probe_media;
use crate::qbittorrent;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tokio::sync::RwLock;
use tracing::{info, warn};

const IDENTIFIER_MAX_PROBE_RETRIES: usize = 10;
const IDENTIFIER_RETRY_DELAY_SECS: u64 = 8;

/// The Identifier actor receives raw ingest events from the Watcher, runs
/// `ffprobe` to extract media metadata, and forwards the result to the Brain.
pub struct IdentifierActor {
    rx: mpsc::Receiver<IngestEvent>,
    tx: mpsc::Sender<IdentifiedMedia>,
    io_semaphore: Arc<Semaphore>,
    config: Arc<RwLock<AppConfig>>,
}

impl IdentifierActor {
    pub fn new(
        rx: mpsc::Receiver<IngestEvent>,
        tx: mpsc::Sender<IdentifiedMedia>,
        io_semaphore: Arc<Semaphore>,
        config: Arc<RwLock<AppConfig>>,
    ) -> Self {
        Self {
            rx,
            tx,
            io_semaphore,
            config,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("identifier: actor started");
        while let Some(event) = self.rx.recv().await {
            if !path_in_configured_library(&event.path, &self.config).await {
                info!(
                    file = %event.path.display(),
                    "identifier: skipping event outside configured media libraries"
                );
                continue;
            }

            let tx = self.tx.clone();
            let io_semaphore = self.io_semaphore.clone();
            let config = self.config.clone();
            tokio::spawn(async move {
                let mut attempt = 0usize;
                loop {
                    attempt += 1;

                    let qb_cfg = {
                        let cfg = config.read().await;
                        cfg.qbittorrent.clone()
                    };

                    if qb_cfg.enabled {
                        match qbittorrent::path_is_actively_downloading(&qb_cfg, &event.path).await {
                            Ok(true) => {
                                info!(
                                    file = %event.path.display(),
                                    attempt,
                                    "identifier: delaying probe while file is still downloading"
                                );
                                if attempt >= IDENTIFIER_MAX_PROBE_RETRIES {
                                    warn!(
                                        file = %event.path.display(),
                                        "identifier: max retries reached while waiting for download completion"
                                    );
                                    break;
                                }
                                tokio::time::sleep(Duration::from_secs(IDENTIFIER_RETRY_DELAY_SECS)).await;
                                continue;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                warn!(
                                    file = %event.path.display(),
                                    err = %error,
                                    "identifier: qBittorrent check failed, probing anyway"
                                );
                            }
                        }
                    }

                    let probe_result = async {
                        let permit = io_semaphore.clone().acquire_owned().await?;
                        let result = probe_media(&event.path).await;
                        drop(permit);
                        Result::<_, anyhow::Error>::Ok(result)
                    }
                    .await;

                    match probe_result {
                        Ok(Ok(probe)) => {
                            info!(file = %event.path.display(), "identifier: probe complete");
                            let _ = tx
                                .send(IdentifiedMedia {
                                    path: event.path,
                                    probe,
                                })
                                .await;
                            break;
                        }
                        Ok(Err(error)) => {
                            if attempt < IDENTIFIER_MAX_PROBE_RETRIES
                                && should_retry_probe_failure(&error, &event.path)
                            {
                                info!(
                                    file = %event.path.display(),
                                    attempt,
                                    err = %error,
                                    "identifier: probe failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_secs(IDENTIFIER_RETRY_DELAY_SECS)).await;
                                continue;
                            }

                            warn!(file = %event.path.display(), err = %error, "identifier: probe failed");
                            break;
                        }
                        Err(error) => {
                            warn!(file = %event.path.display(), err = %error, "identifier: semaphore acquire failed");
                            break;
                        }
                    }
                }
            });
        }
        Ok(())
    }
}

async fn path_in_configured_library(path: &Path, config: &Arc<RwLock<AppConfig>>) -> bool {
    let cfg = config.read().await;
    let media_root = Path::new(&cfg.data_path);
    let Ok(relative) = path.strip_prefix(media_root) else {
        return false;
    };

    let relative = relative.to_string_lossy().replace('\\', "/");
    cfg.libraries.iter().any(|library| {
        let prefix = if library.path.ends_with('/') {
            library.path.clone()
        } else {
            format!("{}/", library.path)
        };
        relative == library.path || relative.starts_with(&prefix)
    })
}

fn should_retry_probe_failure(error: &anyhow::Error, path: &Path) -> bool {
    if !path.exists() {
        return true;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains("exit status: 1")
        || message.contains("invalid data")
        || message.contains("moov atom not found")
        || message.contains("resource temporarily unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LibraryFolder;

    fn test_config() -> Arc<RwLock<AppConfig>> {
        let mut cfg = AppConfig::default();
        cfg.data_path = "/data/media".to_string();
        cfg.libraries = vec![
            LibraryFolder {
                id: "movies".to_string(),
                name: "Movies".to_string(),
                path: "movies".to_string(),
                media_type: "movie".to_string(),
            },
            LibraryFolder {
                id: "tv".to_string(),
                name: "TV".to_string(),
                path: "tv".to_string(),
                media_type: "tv".to_string(),
            },
        ];
        Arc::new(RwLock::new(cfg))
    }

    #[tokio::test]
    async fn path_in_configured_library_accepts_known_library_paths() {
        let cfg = test_config();
        assert!(
            path_in_configured_library(Path::new("/data/media/movies/Big.Fish.2003.mkv"), &cfg)
                .await
        );
        assert!(path_in_configured_library(Path::new("/data/media/tv/Show/S01E01.mkv"), &cfg).await);
    }

    #[tokio::test]
    async fn path_in_configured_library_rejects_outside_or_unmapped_paths() {
        let cfg = test_config();
        assert!(!path_in_configured_library(Path::new("/data/downloads/radarr/file.mkv"), &cfg).await);
        assert!(!path_in_configured_library(Path::new("/data/media/music/track.flac"), &cfg).await);
        assert!(!path_in_configured_library(Path::new("/otherroot/movies/file.mkv"), &cfg).await);
    }
}
