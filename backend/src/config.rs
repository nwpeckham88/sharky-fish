use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Path to the primary media library (MergerFS pool mount inside container).
    pub data_path: String,
    /// Path to ingest / cache directory.
    pub ingest_path: String,
    /// Path to config directory (SQLite db lives here).
    pub config_path: String,
    /// HTTP port for the Axum server.
    pub port: u16,
    /// LLM provider configuration.
    pub llm: LlmConfig,
    /// Maximum concurrent I/O operations against the storage array.
    pub max_io_concurrency: usize,
    /// Number of recent library items to prewarm metadata for on startup.
    pub metadata_prewarm_limit: usize,
    /// Golden Standards: encoding rules the LLM must respect.
    #[serde(default)]
    pub golden_standards: GoldenStandards,
    /// System prompt sent to the LLM for ffmpeg command generation.
    #[serde(default)]
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider: "openai" or "ollama".
    pub provider: String,
    /// Base URL (e.g. "https://api.openai.com/v1" or "http://localhost:11434").
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// API key (required for OpenAI, ignored for Ollama).
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenStandards {
    pub video: VideoStandards,
    pub audio: AudioStandards,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStandards {
    /// e.g. "h264", "h265", "av1", "vp9"
    pub codec: String,
    /// Maximum video bitrate in Mbps.
    pub max_bitrate_mbps: f64,
    /// e.g. "none", "4k", "1080p", "720p"
    pub resolution_ceiling: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStandards {
    /// Target integrated loudness in LUFS (e.g. -24.0).
    pub target_lufs: f64,
    /// Target true peak in dBTP (e.g. -2.0).
    pub target_true_peak: f64,
    /// e.g. "none", "7.1", "5.1", "stereo"
    pub max_channels: String,
}

impl Default for GoldenStandards {
    fn default() -> Self {
        Self {
            video: VideoStandards {
                codec: "h264".into(),
                max_bitrate_mbps: 15.0,
                resolution_ceiling: "none".into(),
            },
            audio: AudioStandards {
                target_lufs: -24.0,
                target_true_peak: -2.0,
                max_channels: "none".into(),
            },
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_path: "/data".into(),
            ingest_path: "/ingest".into(),
            config_path: "/config".into(),
            port: 3000,
            llm: LlmConfig {
                provider: "ollama".into(),
                base_url: "http://localhost:11434".into(),
                model: "llama3".into(),
                api_key: None,
            },
            max_io_concurrency: 4,
            metadata_prewarm_limit: 250,
            golden_standards: GoldenStandards::default(),
            system_prompt: String::new(),
        }
    }
}

impl AppConfig {
    /// Load config from `{config_path}/sharky.json`, falling back to defaults.
    pub fn load(config_path: &str) -> Self {
        let path = std::path::Path::new(config_path).join("sharky.json");
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => {
                let cfg = Self::default();
                // Persist defaults for user editing.
                if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                    let _ = std::fs::create_dir_all(config_path);
                    let _ = std::fs::write(&path, json);
                }
                cfg
            }
        }
    }

    /// Persist current config to `{config_path}/sharky.json`.
    pub fn save(&self) -> Result<(), String> {
        let path = std::path::Path::new(&self.config_path).join("sharky.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.config_path).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
        Ok(())
    }
}
