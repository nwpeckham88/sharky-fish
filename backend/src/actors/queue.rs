use crate::db;
use crate::messages::{QueueMsg, QueuedJob};
use anyhow::Result;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// The Queue actor owns all job/task lifecycle state, backed by SQLite durable
/// execution tables.  It receives enqueue requests from the Brain and poll
/// requests from the Forge.
pub struct QueueActor {
    rx: mpsc::Receiver<QueueMsg>,
    pool: SqlitePool,
}

impl QueueActor {
    pub fn new(rx: mpsc::Receiver<QueueMsg>, pool: SqlitePool) -> Self {
        Self { rx, pool }
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
        let file_path = media.path.to_string_lossy();
        let job_id = db::insert_job(&self.pool, &file_path).await?;
        decision.job_id = job_id;

        // If two-pass normalization is required, create two tasks.
        if decision.requires_two_pass {
            db::insert_task(&self.pool, job_id, 1, "AUDIO_SCAN", None).await?;
            db::insert_task(
                &self.pool,
                job_id,
                2,
                "TRANSCODE",
                Some(&serde_json::to_string(&decision.arguments)?),
            )
            .await?;
        } else {
            db::insert_task(
                &self.pool,
                job_id,
                1,
                "TRANSCODE",
                Some(&serde_json::to_string(&decision.arguments)?),
            )
            .await?;
        }

        info!(job_id, file = %file_path, "queue: job enqueued");
        Ok(())
    }

    async fn poll_next(&self) -> Option<QueuedJob> {
        let jobs = db::fetch_pending_jobs(&self.pool, 1).await.ok()?;
        let job = jobs.into_iter().next()?;

        // Mark as processing.
        db::update_job_status(&self.pool, job.id, "PROCESSING")
            .await
            .ok()?;

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
                    // Reset to PENDING so they re-enter the pipeline.
                    let _ = db::update_job_status(&self.pool, job.id, "PENDING").await;
                }
            }
            _ => {}
        }
    }
}
