use crate::db;
use crate::messages::{
    FfmpegProgress, LoudnormMeasurement, QueueMsg, QueuedJob, SseEvent,
};
use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{info, warn};
use std::sync::Arc;

/// The Forge actor pulls jobs from the Queue and executes FFmpeg transcoding,
/// including two-pass EBU R128 loudnorm when required.  It owns the I/O
/// semaphore to bound concurrent disk access, and broadcasts progress events
/// over the SSE channel.
pub struct ForgeActor {
    queue_tx: mpsc::Sender<QueueMsg>,
    pool: SqlitePool,
    sse_tx: broadcast::Sender<SseEvent>,
    io_semaphore: Arc<Semaphore>,
    data_path: PathBuf,
}

impl ForgeActor {
    pub fn new(
        queue_tx: mpsc::Sender<QueueMsg>,
        pool: SqlitePool,
        sse_tx: broadcast::Sender<SseEvent>,
        io_semaphore: Arc<Semaphore>,
        data_path: PathBuf,
    ) -> Self {
        Self {
            queue_tx,
            pool,
            sse_tx,
            io_semaphore,
            data_path,
        }
    }

    pub async fn run(self) -> Result<()> {
        info!("forge: actor started");
        loop {
            // Poll the queue for the next job.
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            self.queue_tx
                .send(QueueMsg::PollNext { reply: reply_tx })
                .await
                .ok();

            let Some(job) = reply_rx.await.ok().flatten() else {
                // No work available; back off before polling again.
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            };

            let _permit = self.io_semaphore.clone().acquire_owned().await?;
            let success = match self.execute_job(&job).await {
                Ok(()) => true,
                Err(e) => {
                    warn!(job_id = job.job_id, err = %e, "forge: job failed");
                    false
                }
            };

            let _ = self
                .queue_tx
                .send(QueueMsg::Complete {
                    job_id: job.job_id,
                    success,
                })
                .await;

            let _ = self.sse_tx.send(SseEvent::JobCompleted {
                job_id: job.job_id,
                success,
            });
        }
    }

    async fn execute_job(&self, job: &QueuedJob) -> Result<()> {
        let _ = self.sse_tx.send(SseEvent::JobCreated {
            job_id: job.job_id,
            file_path: job.source_path.to_string_lossy().into(),
        });
        let _ = self.sse_tx.send(SseEvent::JobStatus {
            job_id: job.job_id,
            status: "PROCESSING".into(),
        });

        let output_name = job
            .source_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "output".into());

        let output_partial = self.data_path.join(format!("{output_name}.mp4.partial"));
        let output_final = self.data_path.join(format!("{output_name}.mp4"));

        if job.requires_two_pass {
            // Pass 1: loudnorm analysis
            let measurement = self.run_loudnorm_pass1(job).await?;

            // Persist measurement to DB for durable execution.
            let tasks = db::fetch_tasks_for_job(&self.pool, job.job_id).await?;
            if let Some(pass2_task) = tasks.iter().find(|t| t.task_type == "TRANSCODE") {
                let payload = serde_json::to_string(&measurement)?;
                db::update_task(&self.pool, pass2_task.id, "QUEUED", Some(&payload)).await?;
            }

            // Pass 2: actual transcode with injected loudnorm params.
            self.run_transcode(job, &output_partial, Some(&measurement))
                .await?;
        } else {
            self.run_transcode(job, &output_partial, None).await?;
        }

        // Atomic finalization: rename .partial → final.
        self.finalize_output(&output_partial, &output_final)
            .await?;

        info!(job_id = job.job_id, output = %output_final.display(), "forge: job complete");
        Ok(())
    }

    /// Pass 1: run loudnorm filter with `print_format=json` and parse the
    /// measurement block from stderr.
    async fn run_loudnorm_pass1(&self, job: &QueuedJob) -> Result<LoudnormMeasurement> {
        info!(job_id = job.job_id, "forge: loudnorm pass 1 (analysis)");
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-i"])
            .arg(&job.source_path)
            .args([
                "-af",
                "loudnorm=I=-14:TP=-1.5:LRA=11:print_format=json",
                "-f", "null",
                "-",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn ffmpeg pass 1")?;
        let stderr = child.stderr.take().expect("stderr piped");
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        let mut json_block = String::new();
        let mut in_json = false;

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim_start().starts_with('{') {
                in_json = true;
                json_block.clear();
            }
            if in_json {
                json_block.push_str(&line);
                json_block.push('\n');
                if line.trim_end().ends_with('}') {
                    in_json = false;
                }
            }
        }

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("ffmpeg pass 1 exited with {status}");
        }

        let measurement: LoudnormMeasurement = serde_json::from_str(&json_block)
            .context("failed to parse loudnorm JSON output")?;

        info!(job_id = job.job_id, input_i = measurement.input_i, "forge: pass 1 complete");
        Ok(measurement)
    }

    /// Run the actual transcode pass.  When `loudnorm` data is provided, it
    /// injects the measured parameters into the `loudnorm` filter chain.
    async fn run_transcode(
        &self,
        job: &QueuedJob,
        output_path: &Path,
        loudnorm: Option<&LoudnormMeasurement>,
    ) -> Result<()> {
        info!(job_id = job.job_id, "forge: transcoding");
        let mut args: Vec<String> = Vec::new();

        // Replace placeholders in the LLM-generated argument list.
        for arg in &job.arguments {
            match arg.as_str() {
                "input.mkv" => args.push(job.source_path.to_string_lossy().into()),
                "output.mp4" | "output.m4a" => args.push(output_path.to_string_lossy().into()),
                _ => args.push(arg.clone()),
            }
        }

        // Inject loudnorm pass 2 filter if measurements are provided.
        if let Some(m) = loudnorm {
            let filter = format!(
                "loudnorm=I=-14:TP=-1.5:LRA=11:\
                 measured_I={:.2}:measured_LRA={:.2}:measured_TP={:.2}:\
                 measured_thresh={:.2}:offset={:.2}:linear=true",
                m.input_i, m.input_lra, m.input_tp, m.input_thresh, m.target_offset
            );
            // Insert the audio filter before the output path.
            if let Some(pos) = args.iter().position(|a| a == "-af") {
                args[pos + 1] = filter;
            } else {
                let output_pos = args.len().saturating_sub(1);
                args.insert(output_pos, "-af".into());
                args.insert(output_pos + 1, filter);
            }
        }

        // Add progress pipe and override confirmation.
        let full_args = [&["-y".to_string(), "-progress".to_string(), "pipe:1".to_string()], args.as_slice()].concat();

        let mut cmd = Command::new("ffmpeg");
        cmd.args(&full_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn ffmpeg")?;

        // Drain stderr to prevent pipe buffer deadlock.
        let stderr = child.stderr.take().expect("stderr piped");
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(_)) = lines.next_line().await {}
        });

        // Parse progress from stdout.
        let stdout = child.stdout.take().expect("stdout piped");
        let job_id = job.job_id;
        let duration = self.get_duration(job).await;
        let sse_tx = self.sse_tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(progress) = parse_progress_line(&line, job_id, duration) {
                    let _ = sse_tx.send(SseEvent::Progress(progress));
                }
            }
        });

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("ffmpeg exited with {status}");
        }
        Ok(())
    }

    /// Atomic rename with EXDEV fallback (copy-fsync-rename-unlink).
    async fn finalize_output(&self, partial: &Path, final_path: &Path) -> Result<()> {
        match tokio::fs::rename(partial, final_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.raw_os_error() == Some(18) => { // EXDEV
                // Cross-device: copy → fsync → rename → unlink.
                let tmp_dest = final_path.with_extension("mp4.partial");
                tokio::fs::copy(partial, &tmp_dest).await?;

                // fsync via spawn_blocking
                let tmp_clone = tmp_dest.clone();
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let f = std::fs::File::open(&tmp_clone)?;
                    f.sync_all()?;
                    Ok(())
                })
                .await??;

                tokio::fs::rename(&tmp_dest, final_path).await?;
                tokio::fs::remove_file(partial).await?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn get_duration(&self, job: &QueuedJob) -> f64 {
        // Quick ffprobe to get duration; best-effort.
        let output = Command::new("ffprobe")
            .args(["-v", "quiet", "-show_entries", "format=duration", "-of", "csv=p=0"])
            .arg(&job.source_path)
            .output()
            .await
            .ok();

        output
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.0)
    }
}

/// Parse a single ffmpeg `-progress pipe:1` key=value line.
fn parse_progress_line(line: &str, job_id: i64, total_duration: f64) -> Option<FfmpegProgress> {
    if !line.contains('=') {
        return None;
    }

    let mut progress = FfmpegProgress {
        job_id,
        frame: None,
        fps: None,
        speed: None,
        time_secs: None,
        percent: None,
    };

    let (key, value) = line.split_once('=')?;
    match key.trim() {
        "frame" => progress.frame = value.trim().parse().ok(),
        "fps" => progress.fps = value.trim().parse().ok(),
        "speed" => {
            progress.speed = Some(value.trim().to_string());
        }
        "out_time_us" => {
            if let Ok(us) = value.trim().parse::<f64>() {
                let secs = us / 1_000_000.0;
                progress.time_secs = Some(secs);
                if total_duration > 0.0 {
                    progress.percent = Some((secs / total_duration * 100.0).min(100.0));
                }
            }
        }
        _ => return None,
    }

    Some(progress)
}
