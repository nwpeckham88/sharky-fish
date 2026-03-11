use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Messages flowing between actors
// ---------------------------------------------------------------------------

/// Emitted by the Watcher when a new file appears in the ingest directory.
#[derive(Debug, Clone)]
pub struct IngestEvent {
    pub path: PathBuf,
}

/// Emitted by the Identifier after extracting probe metadata.
#[derive(Debug, Clone)]
pub struct IdentifiedMedia {
    pub path: PathBuf,
    pub probe: MediaProbe,
}

/// Compact representation of ffprobe output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaProbe {
    pub format: String,
    pub duration_secs: f64,
    pub streams: Vec<StreamInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub codec_type: String, // "video", "audio", "subtitle"
    pub codec_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_rate: Option<u64>,
    /// ISO 639-2/B language tag (e.g. "eng", "spa").
    pub language: Option<String>,
    /// Stream title/label (often describes subtitle track content).
    pub title: Option<String>,
    /// Disposition flags from the container.
    #[serde(default)]
    pub disposition: StreamDisposition,
}

/// Disposition flags reported by ffprobe for a stream.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamDisposition {
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub forced: bool,
    #[serde(default)]
    pub hearing_impaired: bool,
}

/// Decision produced by the Brain (LLM).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingDecision {
    pub job_id: i64,
    pub arguments: Vec<String>,
    pub requires_two_pass: bool,
    pub rationale: String,
}

/// Message sent to the Queue actor.
#[derive(Debug)]
pub enum QueueMsg {
    /// Schedule a new job.
    Enqueue {
        media: IdentifiedMedia,
        decision: ProcessingDecision,
    },
    /// Request the next job ready for execution. Responds via oneshot.
    PollNext {
        reply: oneshot::Sender<Option<QueuedJob>>,
    },
    /// A job has finished (success or failure).
    Complete { job_id: i64, success: bool },
}

/// A fully-resolved job ready for the Forge.
#[derive(Debug, Clone)]
pub struct QueuedJob {
    pub job_id: i64,
    pub source_path: PathBuf,
    pub arguments: Vec<String>,
    pub requires_two_pass: bool,
}

// ---------------------------------------------------------------------------
// FFmpeg progress event (parsed from stderr)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfmpegProgress {
    pub job_id: i64,
    pub frame: Option<u64>,
    pub fps: Option<f64>,
    pub speed: Option<String>,
    pub time_secs: Option<f64>,
    pub percent: Option<f64>,
}

/// Loudnorm Pass 1 results (EBU R128).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoudnormMeasurement {
    pub input_i: f64,
    pub input_lra: f64,
    pub input_tp: f64,
    pub input_thresh: f64,
    pub target_offset: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryChange {
    pub relative_path: String,
    pub path: String,
    pub change: String,
    pub occurred_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryIndexScanProgress {
    pub status: String,
    pub scanned_items: usize,
    pub total_items: usize,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub last_scan_at: Option<u64>,
    pub last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// SSE broadcast event
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    #[serde(rename = "job_created")]
    JobCreated {
        job_id: i64,
        file_path: String,
        status: String,
        group_key: Option<String>,
        group_label: Option<String>,
        group_kind: String,
    },
    #[serde(rename = "job_status")]
    JobStatus { job_id: i64, status: String },
    #[serde(rename = "library_change")]
    LibraryChange(LibraryChange),
    #[serde(rename = "library_scan_progress")]
    LibraryIndexScanProgress(LibraryIndexScanProgress),
    #[serde(rename = "progress")]
    Progress(FfmpegProgress),
    #[serde(rename = "job_completed")]
    JobCompleted { job_id: i64, success: bool },
}
