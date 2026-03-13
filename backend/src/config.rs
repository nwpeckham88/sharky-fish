use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Path to the managed media library root inside the container.
    pub data_path: String,
    /// Path to the download ingress directory inside the container.
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
    /// Relative path patterns to exclude from library index scans.
    #[serde(default = "default_scan_exclude_patterns")]
    pub scan_exclude_patterns: Vec<String>,
    /// Concurrent workers used for full library reindex scans.
    #[serde(default = "default_scan_concurrency")]
    pub scan_concurrency: usize,
    /// Backpressure capacity for queued scan items during full reindex.
    #[serde(default = "default_scan_queue_capacity")]
    pub scan_queue_capacity: usize,
    /// Per-request concurrency for bulk metadata fetches.
    #[serde(default = "default_bulk_metadata_concurrency")]
    pub bulk_metadata_concurrency: usize,
    /// Max simultaneous bulk metadata requests before returning backpressure.
    #[serde(default = "default_bulk_metadata_max_inflight")]
    pub bulk_metadata_max_inflight: usize,
    /// Golden Standards: encoding rules the LLM must respect.
    #[serde(default)]
    pub golden_standards: GoldenStandards,
    /// Freeform notes about playback devices and compatibility constraints.
    #[serde(default)]
    pub playback_context: String,
    /// Preferred library list density in the UI.
    #[serde(default = "default_library_view_mode")]
    pub library_view_mode: String,
    /// System prompt sent to the LLM for ffmpeg command generation.
    #[serde(default)]
    pub system_prompt: String,
    /// Whether AI-generated jobs should be auto-approved into the execution queue.
    #[serde(default = "default_true")]
    pub auto_approve_ai_jobs: bool,
    /// Named library folders within `data_path`.
    #[serde(default)]
    pub libraries: Vec<LibraryFolder>,
    /// Internet metadata providers (OMDb/TVDB) used for title enrichment.
    #[serde(default)]
    pub internet_metadata: InternetMetadataConfig,
    /// Whether to compute blake3 checksums for every library file during scanning.
    /// Enables checksum-duplicate detection in the Downloads view but can add
    /// several minutes to scans on large NAS libraries. Defaults to false.
    #[serde(default)]
    pub scan_compute_checksums: bool,
    /// Optional qBittorrent API integration used for transfer visibility.
    #[serde(default)]
    pub qbittorrent: QbittorrentConfig,
    /// Artwork image download counts, mirroring Jellyfin's per-type configuration.
    #[serde(default)]
    pub artwork_download: ArtworkDownloadConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QbittorrentConfig {
    /// Enable qBittorrent API polling.
    #[serde(default)]
    pub enabled: bool,
    /// qBittorrent WebUI base URL, e.g. http://qbittorrent:8080.
    #[serde(default = "default_qbittorrent_base_url")]
    pub base_url: String,
    /// qBittorrent username.
    pub username: Option<String>,
    /// qBittorrent password.
    pub password: Option<String>,
    /// API timeout for each request.
    #[serde(default = "default_qbittorrent_timeout_secs")]
    pub request_timeout_secs: u64,
    /// Maximum torrents returned in API responses.
    #[serde(default = "default_qbittorrent_max_torrents")]
    pub max_torrents: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetMetadataConfig {
	/// TMDb API key (Jellyfin's default movie/show metadata source).
	pub tmdb_api_key: Option<String>,
    /// OMDb API key (OMDb provides IMDb-backed metadata).
    pub omdb_api_key: Option<String>,
    /// TVDB API key.
    pub tvdb_api_key: Option<String>,
    /// Optional TVDB PIN.
    pub tvdb_pin: Option<String>,
    /// User-Agent sent to internet metadata services.
    pub user_agent: String,
    /// Default provider when multiple metadata sources are configured.
    #[serde(default = "default_metadata_provider")]
    pub default_provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider: "google", "openai" or "ollama".
    pub provider: String,
    /// Base URL (e.g. "https://generativelanguage.googleapis.com/v1beta", "https://api.openai.com/v1" or "http://localhost:11434").
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// API key (required for Google/OpenAI, ignored for Ollama).
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenStandards {
    pub video: VideoStandards,
    pub audio: AudioStandards,
    #[serde(default)]
    pub subtitle: SubtitleStandards,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoStandards {
    /// e.g. "h264", "h265", "av1", "vp9"
    pub codec: String,
    /// Maximum video bitrate in Mbps.
    pub max_bitrate_mbps: f64,
    /// e.g. "none", "4k", "1080p", "720p"
    pub resolution_ceiling: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioStandards {
    /// e.g. "aac", "opus", "ac3", "eac3", "copy"
    pub codec: String,
    /// Target integrated loudness in LUFS (e.g. -24.0).
    pub target_lufs: f64,
    /// Target true peak in dBTP (e.g. -2.0).
    pub target_true_peak: f64,
    /// e.g. "none", "7.1", "5.1", "stereo"
    pub max_channels: String,
    /// Keep more than one retained audio track when useful.
    #[serde(default = "default_true")]
    pub keep_multiple_tracks: bool,
    /// Ensure a stereo-compatible track exists by downmixing when needed.
    #[serde(default = "default_true")]
    pub create_stereo_downmix: bool,
}

/// Subtitle handling mode.
///   - "keep_all"          – copy every subtitle stream unchanged
///   - "remove_all"        – strip all subtitle streams
///   - "keep_preferred"    – keep only streams whose language is in `preferred_languages`
///   - "keep_forced_only"  – strip all except forced tracks in `preferred_languages`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleStandards {
    /// Processing mode (see doc above).
    pub mode: String,
    /// ISO 639-2/B language codes (e.g. "eng", "spa", "fra").
    #[serde(default)]
    pub preferred_languages: Vec<String>,
    /// Always keep forced subtitle tracks in preferred languages, even in
    /// modes that would otherwise strip them.
    #[serde(default = "default_true")]
    pub keep_forced: bool,
    /// Keep SDH (hearing-impaired) subtitle tracks.
    #[serde(default)]
    pub keep_sdh: bool,
}

fn default_true() -> bool {
    true
}

fn default_scan_exclude_patterns() -> Vec<String> {
    vec!["samples".into(), "trailers".into(), "extras".into()]
}

fn default_scan_concurrency() -> usize {
    4
}

fn default_scan_queue_capacity() -> usize {
    512
}

fn default_bulk_metadata_concurrency() -> usize {
    6
}

fn default_bulk_metadata_max_inflight() -> usize {
    2
}

fn default_metadata_provider() -> String {
    "tmdb".into()
}

fn default_library_view_mode() -> String {
    "compact".into()
}

// ---------------------------------------------------------------------------
// Artwork download configuration (Jellyfin-style per-type image counts)
// ---------------------------------------------------------------------------

/// How many images of each type to download per content category.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtworkDownloadConfig {
    /// Image counts for movies.
    pub movies: ArtworkTypeCounts,
    /// Image counts for TV series (top-level show).
    pub series: ArtworkTypeCounts,
    /// Image counts for TV seasons.
    pub seasons: ArtworkTypeCounts,
    /// Image counts for TV episodes.
    pub episodes: ArtworkTypeCounts,
}

/// Per-type image counts for a single content category. All fields default to 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkTypeCounts {
    /// Primary / poster image (the main showcase artwork).
    #[serde(default)]
    pub primary: u32,
    /// Backdrop / fanart images (full-width background).
    #[serde(default)]
    pub backdrop: u32,
    /// Logo images (transparent title / studio logos, clearlogo).
    #[serde(default)]
    pub logo: u32,
    /// Banner images (wide horizontal, 800×150-style).
    #[serde(default)]
    pub banner: u32,
    /// Thumb images (landscape thumbnail, 16:9).
    #[serde(default)]
    pub thumb: u32,
    /// Disc / optical disc cover art.
    #[serde(default)]
    pub disc: u32,
    /// ClearArt / transparent character or scene art.
    #[serde(default)]
    pub art: u32,
    /// Screenshot images (for episodes / bonus content).
    #[serde(default)]
    pub screenshot: u32,
    /// Box art (primarily for music/game libraries).
    #[serde(default)]
    pub box_art: u32,
    /// Rear box art.
    #[serde(default)]
    pub box_rear: u32,
    /// Menu / bonus menu art.
    #[serde(default)]
    pub menu: u32,
}

impl Default for ArtworkDownloadConfig {
    fn default() -> Self {
        Self {
            movies: ArtworkTypeCounts {
                primary: 1,
                backdrop: 3,
                logo: 1,
                banner: 0,
                thumb: 0,
                disc: 1,
                art: 0,
                screenshot: 0,
                box_art: 0,
                box_rear: 0,
                menu: 0,
            },
            series: ArtworkTypeCounts {
                primary: 1,
                backdrop: 3,
                logo: 1,
                banner: 1,
                thumb: 0,
                disc: 0,
                art: 0,
                screenshot: 0,
                box_art: 0,
                box_rear: 0,
                menu: 0,
            },
            seasons: ArtworkTypeCounts {
                primary: 1,
                backdrop: 0,
                logo: 0,
                banner: 1,
                thumb: 0,
                disc: 0,
                art: 0,
                screenshot: 0,
                box_art: 0,
                box_rear: 0,
                menu: 0,
            },
            episodes: ArtworkTypeCounts {
                primary: 1,
                backdrop: 0,
                logo: 0,
                banner: 0,
                thumb: 0,
                disc: 0,
                art: 0,
                screenshot: 0,
                box_art: 0,
                box_rear: 0,
                menu: 0,
            },
        }
    }
}



fn default_qbittorrent_base_url() -> String {
    "http://qbittorrent:8080".into()
}

fn default_qbittorrent_timeout_secs() -> u64 {
    8
}

fn default_qbittorrent_max_torrents() -> usize {
    100
}

/// A named library folder mapped to a subdirectory inside `data_path`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryFolder {
    /// Unique identifier (slug, e.g. "movies", "tv-shows").
    pub id: String,
    /// Human-readable name (e.g. "Movies", "TV Shows").
    pub name: String,
    /// Relative path within `data_path` (e.g. "movies", "tv").
    pub path: String,
    /// Content type: "movie" or "tv".
    pub media_type: String,
}

impl Default for SubtitleStandards {
    fn default() -> Self {
        Self {
            mode: "keep_preferred".into(),
            preferred_languages: vec!["eng".into()],
            keep_forced: true,
            keep_sdh: false,
        }
    }
}

impl Default for GoldenStandards {
    fn default() -> Self {
        Self {
            video: VideoStandards {
                codec: "h265".into(),
                max_bitrate_mbps: 15.0,
                resolution_ceiling: "none".into(),
            },
            audio: AudioStandards {
                codec: "opus".into(),
                target_lufs: -24.0,
                target_true_peak: -2.0,
                max_channels: "5.1".into(),
                keep_multiple_tracks: true,
                create_stereo_downmix: true,
            },
            subtitle: SubtitleStandards::default(),
        }
    }
}

impl Default for VideoStandards {
    fn default() -> Self {
        Self {
            codec: "h265".into(),
            max_bitrate_mbps: 15.0,
            resolution_ceiling: "none".into(),
        }
    }
}

impl Default for AudioStandards {
    fn default() -> Self {
        Self {
            codec: "opus".into(),
            target_lufs: -24.0,
            target_true_peak: -2.0,
            max_channels: "5.1".into(),
            keep_multiple_tracks: true,
            create_stereo_downmix: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_path: "/data/media".into(),
            ingest_path: "/data/downloads".into(),
            config_path: "/config".into(),
            port: 3000,
            llm: LlmConfig {
                provider: "google".into(),
                base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
                model: "gemini-3.1-flash-lite-preview".into(),
                api_key: None,
            },
            max_io_concurrency: 4,
            metadata_prewarm_limit: 250,
            scan_exclude_patterns: default_scan_exclude_patterns(),
            scan_concurrency: default_scan_concurrency(),
            scan_queue_capacity: default_scan_queue_capacity(),
            bulk_metadata_concurrency: default_bulk_metadata_concurrency(),
            bulk_metadata_max_inflight: default_bulk_metadata_max_inflight(),
            golden_standards: GoldenStandards::default(),
            playback_context: String::new(),
            library_view_mode: default_library_view_mode(),
            system_prompt: SYSTEM_PROMPT_DEFAULT.into(),
            auto_approve_ai_jobs: true,
            libraries: Vec::new(),
            internet_metadata: InternetMetadataConfig::default(),
            scan_compute_checksums: false,
            qbittorrent: QbittorrentConfig::default(),
            artwork_download: ArtworkDownloadConfig::default(),
        }
    }
}

impl Default for InternetMetadataConfig {
    fn default() -> Self {
        Self {
			tmdb_api_key: None,
            omdb_api_key: None,
            tvdb_api_key: None,
            tvdb_pin: None,
            user_agent: "sharky-fish/0.1".into(),
            default_provider: default_metadata_provider(),
        }
    }
}

impl Default for QbittorrentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_qbittorrent_base_url(),
            username: None,
            password: None,
            request_timeout_secs: default_qbittorrent_timeout_secs(),
            max_torrents: default_qbittorrent_max_torrents(),
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

const SYSTEM_PROMPT_DEFAULT: &str = "\
You are an expert systems architect and media processing engine. \
Your sole function is to generate highly optimized, syntactically valid FFmpeg \
command arguments based on user requirements and host hardware capabilities. \
The host environment utilizes a Debian Linux architecture and may expose \
hardware acceleration depending on the configured host.\n\n\
Constraints:\n\
- Do not output the ffmpeg binary name; return only the exact argument array.\n\
- When playback device notes are supplied, optimize for the stated client capabilities and compatibility limits.\n\
- Ensure all audio streams are evaluated for EBU R128 compliance. If \
    normalization is required, flag it.\n\
- Respect the audio policy provided in the request, including preferred codec, multitrack retention, and stereo downmix requirements.\n\
- Handle subtitle streams according to the provided subtitle standards:\n\
    * mode=keep_all: copy all subtitle streams with -c:s copy.\n\
    * mode=remove_all: map out all subtitle streams entirely (-sn).\n\
    * mode=keep_preferred: use explicit -map 0:s:N for subtitle streams \
        matching the preferred_languages list. If keep_forced is true, also \
        include forced tracks in preferred languages even if they wouldn't \
        otherwise match. If keep_sdh is true, include hearing-impaired tracks.\n\
    * mode=keep_forced_only: strip all subtitle streams except those marked \
        forced in the preferred languages.\n\
- Copy subtitle codec unless the output container cannot hold the source \
    format (e.g., ass/ssa going to mp4 should be converted to mov_text).\n\
- Do not include hardcoded path names or file variables; use generic \
    -i input.mkv and output.mp4 placeholders.\n\
- You must strictly adhere to the provided JSON schema. Do not include \
    markdown formatting, conversational text, thinking tokens, or preambles.\n\n\
Output strictly as JSON: {\"arguments\": [...], \"requires_two_pass\": bool, \"rationale\": \"...\"}";
