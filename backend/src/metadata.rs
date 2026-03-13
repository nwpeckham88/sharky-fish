use crate::db::{self, CachedMediaMetadata};
use crate::filesystem_audit::{self, FileSystemFacts};
use crate::messages::{MediaProbe, StreamDisposition, StreamInfo};
use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::process::Command;
use tokio::task;

#[derive(Debug, Clone, Serialize)]
pub struct LibraryMetadataResponse {
    pub file_path: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub format: String,
    pub duration_secs: f64,
    pub stream_count: usize,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio_channels: Option<u32>,
    pub subtitle_count: usize,
    pub subtitle_languages: Vec<String>,
    pub probe: MediaProbe,
    pub cached: bool,
    pub filesystem: FileSystemFacts,
}

pub async fn probe_media(path: &Path) -> Result<MediaProbe> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .await
        .context("failed to spawn ffprobe")?;

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("ffprobe produced invalid JSON")?;

    let format = json["format"]["format_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    let duration_secs = json["format"]["duration"]
        .as_str()
        .and_then(|duration| duration.parse::<f64>().ok())
        .unwrap_or(0.0);

    let streams = json["streams"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|stream| {
                    let disposition = &stream["disposition"];
                    StreamInfo {
                        index: stream["index"].as_u64().unwrap_or(0) as u32,
                        codec_type: stream["codec_type"].as_str().unwrap_or("").into(),
                        codec_name: stream["codec_name"].as_str().unwrap_or("").into(),
                        width: stream["width"].as_u64().map(|value| value as u32),
                        height: stream["height"].as_u64().map(|value| value as u32),
                        channels: stream["channels"].as_u64().map(|value| value as u32),
                        sample_rate: stream["sample_rate"]
                            .as_str()
                            .and_then(|value| value.parse::<u32>().ok()),
                        bit_rate: stream["bit_rate"]
                            .as_str()
                            .and_then(|value| value.parse::<u64>().ok()),
                        language: stream["tags"]["language"].as_str().map(|v| v.to_string()),
                        title: stream["tags"]["title"].as_str().map(|v| v.to_string()),
                        disposition: StreamDisposition {
                            default: disposition["default"].as_i64() == Some(1),
                            forced: disposition["forced"].as_i64() == Some(1),
                            hearing_impaired: disposition["hearing_impaired"].as_i64() == Some(1),
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(MediaProbe {
        format,
        duration_secs,
        streams,
    })
}

pub async fn get_or_probe_library_metadata(
    pool: &SqlitePool,
    library_root: &Path,
    relative_path: &str,
) -> Result<LibraryMetadataResponse> {
    let sanitized = sanitize_relative_path(relative_path)?;
    let full_path = library_root.join(&sanitized);
    let fs_metadata = tokio::fs::metadata(&full_path)
        .await
        .with_context(|| format!("failed to read metadata for {}", full_path.display()))?;

    let size_bytes = fs_metadata.len();
    let modified_at = fs_metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);

    let file_path = full_path.display().to_string();
    let filesystem = filesystem_audit::file_system_facts(&fs_metadata);

    if let Some(cached) = db::fetch_media_metadata(pool, &file_path).await?
        && cached.size_bytes as u64 == size_bytes
        && cached.modified_at as u64 == modified_at
        && cached.device_id as u64 == filesystem.device_id
        && cached.inode as u64 == filesystem.inode
        && cached.link_count as u64 == filesystem.link_count
    {
        return from_cached(
            &sanitized,
            size_bytes,
            modified_at,
            cached,
            true,
            filesystem,
        );
    }

    let probe = probe_media(&full_path).await?;
    db::upsert_media_metadata(pool, &file_path, &filesystem, &probe).await?;

    Ok(from_probe(
        sanitized.to_string_lossy().replace('\\', "/"),
        file_path,
        size_bytes,
        modified_at,
        probe,
        false,
        filesystem,
    ))
}

pub async fn prewarm_recent_library_metadata(
    pool: SqlitePool,
    library_root: PathBuf,
    limit: usize,
    concurrency: usize,
) -> Result<usize> {
    let candidates = collect_recent_media_paths(library_root.clone(), limit).await?;
    let total = candidates.len();

    stream::iter(candidates)
        .for_each_concurrent(concurrency.max(1), |relative_path| {
            let pool = pool.clone();
            let library_root = library_root.clone();
            async move {
                let _ = get_or_probe_library_metadata(&pool, &library_root, &relative_path).await;
            }
        })
        .await;

    Ok(total)
}

async fn collect_recent_media_paths(library_root: PathBuf, limit: usize) -> Result<Vec<String>> {
    task::spawn_blocking(move || -> Result<Vec<String>> {
        let mut items: Vec<(String, u64)> = Vec::new();

        if !library_root.exists() {
            return Ok(Vec::new());
        }

        for entry in walkdir::WalkDir::new(&library_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.path();
            if !is_media_file(path) {
                continue;
            }

            let modified_at = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_secs())
                .unwrap_or(0);

            let relative_path = path
                .strip_prefix(&library_root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            items.push((relative_path, modified_at));
        }

        items.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        items.truncate(limit);

        Ok(items.into_iter().map(|(path, _)| path).collect())
    })
    .await?
}

fn sanitize_relative_path(relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        anyhow::bail!("path must be relative to the library root");
    }

    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => sanitized.push(value),
            Component::CurDir => {}
            Component::ParentDir => anyhow::bail!("parent directory traversal is not allowed"),
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("invalid path component")
            }
        }
    }

    Ok(sanitized)
}

fn from_cached(
    relative_path: &Path,
    size_bytes: u64,
    modified_at: u64,
    cached: CachedMediaMetadata,
    is_cached: bool,
    filesystem: FileSystemFacts,
) -> Result<LibraryMetadataResponse> {
    let probe: MediaProbe = serde_json::from_str(&cached.probe_json)
        .context("failed to decode cached probe metadata")?;
    Ok(from_probe(
        relative_path.to_string_lossy().replace('\\', "/"),
        cached.file_path,
        size_bytes,
        modified_at,
        probe,
        is_cached,
        filesystem,
    ))
}

fn from_probe(
    relative_path: String,
    file_path: String,
    size_bytes: u64,
    modified_at: u64,
    probe: MediaProbe,
    cached: bool,
    filesystem: FileSystemFacts,
) -> LibraryMetadataResponse {
    let video_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video");
    let audio_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "audio");
    let subtitle_streams: Vec<&StreamInfo> = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type == "subtitle")
        .collect();
    let subtitle_languages: Vec<String> = subtitle_streams
        .iter()
        .filter_map(|s| s.language.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    LibraryMetadataResponse {
        file_path,
        relative_path,
        size_bytes,
        modified_at,
        format: probe.format.clone(),
        duration_secs: probe.duration_secs,
        stream_count: probe.streams.len(),
        video_codec: video_stream.map(|stream| stream.codec_name.clone()),
        audio_codec: audio_stream.map(|stream| stream.codec_name.clone()),
        width: video_stream.and_then(|stream| stream.width),
        height: video_stream.and_then(|stream| stream.height),
        audio_channels: audio_stream.and_then(|stream| stream.channels),
        subtitle_count: subtitle_streams.len(),
        subtitle_languages,
        probe,
        cached,
        filesystem,
    }
}

fn is_media_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mkv"
            | "mp4"
            | "avi"
            | "mov"
            | "ts"
            | "webm"
            | "m4v"
            | "flac"
            | "mp3"
            | "wav"
            | "m4a"
            | "aac"
            | "ogg"
            | "srt"
            | "ass"
            | "vtt"
    )
}
