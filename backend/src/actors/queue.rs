use crate::db;
use crate::config::AppConfig;
use crate::managed_items;
use crate::messages::IdentifiedMedia;
use crate::messages::{QueueMsg, QueuedJob};
use anyhow::Result;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{info, warn};

use crate::messages::SseEvent;

pub async fn enqueue_job(
    pool: &SqlitePool,
    sse_tx: &broadcast::Sender<SseEvent>,
    auto_approve: bool,
    media: &IdentifiedMedia,
    decision: &mut crate::messages::ProcessingDecision,
    initial_status_override: Option<&str>,
) -> Result<i64> {
    let file_path = media.path.to_string_lossy().to_string();
    let initial_status = initial_status_override.unwrap_or(if auto_approve {
        "APPROVED"
    } else {
        "AWAITING_APPROVAL"
    });

    let job_id = db::insert_job(pool, &file_path, initial_status).await?;
    decision.job_id = job_id;
    db::upsert_job_analysis(pool, job_id, &media.probe, decision).await?;

    if decision.requires_two_pass {
        db::insert_task(pool, job_id, 1, "AUDIO_SCAN", None).await?;
        db::insert_task(
            pool,
            job_id,
            2,
            "TRANSCODE",
            Some(&serde_json::to_string(&decision.arguments)?),
        )
        .await?;
    } else {
        db::insert_task(
            pool,
            job_id,
            1,
            "TRANSCODE",
            Some(&serde_json::to_string(&decision.arguments)?),
        )
        .await?;
    }

    let _ = sse_tx.send(SseEvent::JobCreated {
        job_id,
        file_path: file_path.clone(),
        status: initial_status.into(),
    });

    info!(job_id, file = %file_path, "queue: job enqueued");
    Ok(job_id)
}

/// The Queue actor owns all job/task lifecycle state, backed by SQLite durable
/// execution tables.  It receives enqueue requests from the Brain and poll
/// requests from the Forge.
pub struct QueueActor {
    rx: mpsc::Receiver<QueueMsg>,
    pool: SqlitePool,
    sse_tx: broadcast::Sender<SseEvent>,
    config: Arc<RwLock<AppConfig>>,
}

impl QueueActor {
    pub fn new(
        rx: mpsc::Receiver<QueueMsg>,
        pool: SqlitePool,
        sse_tx: broadcast::Sender<SseEvent>,
        config: Arc<RwLock<AppConfig>>,
    ) -> Self {
        Self {
            rx,
            pool,
            sse_tx,
            config,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("queue: actor started");

        // Resume any interrupted jobs from a prior crash.
        self.resume_interrupted().await;

        while let Some(msg) = self.rx.recv().await {
            match msg {
                QueueMsg::Enqueue { media, mut decision } => {
                    if let Err(e) = self.handle_enqueue(&media, &mut decision).await {
                        warn!(err = %e, "queue: enqueue failed");
                    }
                }
                QueueMsg::PollNext { reply } => {
                    let job = self.poll_next().await;
                    let _ = reply.send(job);
                }
                QueueMsg::Complete { job_id, success } => {
                    let status = if success { "COMPLETED" } else { "FAILED" };
                    if let Err(e) = db::update_job_status(&self.pool, job_id, status).await {
                        warn!(job_id, err = %e, "queue: failed to mark job {status}");
                    } else {
                        self.update_managed_completion_status(job_id, success).await;
                        info!(job_id, status, "queue: job finished");
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_enqueue(
        &self,
        media: &crate::messages::IdentifiedMedia,
        decision: &mut crate::messages::ProcessingDecision,
    ) -> Result<()> {
        let auto_approve = {
            let cfg = self.config.read().await;
            cfg.auto_approve_ai_jobs
        };
        enqueue_job(&self.pool, &self.sse_tx, auto_approve, media, decision, None).await?;
        Ok(())
    }

    async fn update_managed_completion_status(&self, job_id: i64, success: bool) {
        let data_path = {
            let cfg = self.config.read().await;
            PathBuf::from(&cfg.data_path)
        };

        let Ok(Some(job)) = db::fetch_job_with_analysis(&self.pool, job_id).await else {
            return;
        };
        let Some(decision) = job.decision else {
            return;
        };

        let Ok(relative_path) = PathBuf::from(&job.file_path).strip_prefix(&data_path).map(|v| v.to_path_buf()) else {
            return;
        };

        let relative_path = relative_path.to_string_lossy().replace('\\', "/");
        let next_status = if success { "PROCESSED" } else { "FAILED" };
        if let Err(error) = managed_items::persist_processing_decision(
            &self.pool,
            &data_path,
            &relative_path,
            next_status,
            &decision,
        )
        .await
        {
            warn!(job_id, err = %error, "queue: failed to persist managed completion state");
        }
    }

    async fn poll_next(&self) -> Option<QueuedJob> {
        let jobs = db::fetch_ready_jobs(&self.pool, 1).await.ok()?;
        let job = jobs.into_iter().next()?;

        // Mark as processing.
        db::update_job_status(&self.pool, job.id, "PROCESSING")
            .await
            .ok()?;
        let _ = self.sse_tx.send(SseEvent::JobStatus {
            job_id: job.id,
            status: "PROCESSING".into(),
        });

        let tasks = db::fetch_tasks_for_job(&self.pool, job.id).await.ok()?;

        // Determine arguments from the first TRANSCODE task's payload.
        let transcode_task = tasks.iter().find(|t| t.task_type == "TRANSCODE")?;
        let arguments: Vec<String> = transcode_task
            .payload
            .as_ref()
            .and_then(|p| serde_json::from_str(p).ok())
            .unwrap_or_default();

        let requires_two_pass = tasks.iter().any(|t| t.task_type == "AUDIO_SCAN");

        Some(QueuedJob {
            job_id: job.id,
            source_path: job.file_path.into(),
            arguments,
            requires_two_pass,
        })
    }

    async fn resume_interrupted(&self) {
        match db::fetch_resumable_jobs(&self.pool).await {
            Ok(jobs) if !jobs.is_empty() => {
                info!(count = jobs.len(), "queue: resuming interrupted jobs");
                for job in jobs {
                    // Reset to APPROVED so they re-enter the execution queue.
                    let _ = db::update_job_status(&self.pool, job.id, "APPROVED").await;
                }
            }
            _ => {}
        }
    }
}
