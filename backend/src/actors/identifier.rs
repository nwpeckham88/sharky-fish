use crate::metadata::probe_media;
use crate::messages::{IdentifiedMedia, IngestEvent};
use anyhow::Result;
use tokio::sync::{mpsc, Semaphore};
use tracing::{info, warn};
use std::sync::Arc;

/// The Identifier actor receives raw ingest events from the Watcher, runs
/// `ffprobe` to extract media metadata, and forwards the result to the Brain.
pub struct IdentifierActor {
    rx: mpsc::Receiver<IngestEvent>,
    tx: mpsc::Sender<IdentifiedMedia>,
    io_semaphore: Arc<Semaphore>,
}

impl IdentifierActor {
    pub fn new(
        rx: mpsc::Receiver<IngestEvent>,
        tx: mpsc::Sender<IdentifiedMedia>,
        io_semaphore: Arc<Semaphore>,
    ) -> Self {
        Self { rx, tx, io_semaphore }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("identifier: actor started");
        while let Some(event) = self.rx.recv().await {
            let permit = self.io_semaphore.clone().acquire_owned().await?;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match probe_media(&event.path).await {
                    Ok(probe) => {
                        info!(file = %event.path.display(), "identifier: probe complete");
                        let _ = tx
                            .send(IdentifiedMedia {
                                path: event.path,
                                probe,
                            })
                            .await;
                    }
                    Err(e) => {
                        warn!(file = %event.path.display(), err = %e, "identifier: probe failed");
                    }
                }
                drop(permit);
            });
        }
        Ok(())
    }
}
