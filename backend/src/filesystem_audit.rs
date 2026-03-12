use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileSystemFacts {
    pub device_id: u64,
    pub inode: u64,
    pub link_count: u64,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub is_hard_linked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditedFile {
    pub path: String,
    pub relative_path: String,
    pub file_name: String,
    pub path_root_kind: String,
    pub filesystem: FileSystemFacts,
}

pub fn file_system_facts(metadata: &Metadata) -> FileSystemFacts {
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);

    #[cfg(unix)]
    {
        let link_count = metadata.nlink();
        return FileSystemFacts {
            device_id: metadata.dev(),
            inode: metadata.ino(),
            link_count,
            size_bytes: metadata.len(),
            modified_at,
            is_hard_linked: link_count > 1,
        };
    }

    #[allow(unreachable_code)]
    FileSystemFacts {
        device_id: 0,
        inode: 0,
        link_count: 1,
        size_bytes: metadata.len(),
        modified_at,
        is_hard_linked: false,
    }
}

pub async fn stat_path(path: &Path) -> Result<FileSystemFacts> {
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    Ok(file_system_facts(&metadata))
}

pub fn collect_media_files(root: &Path, root_kind: &str) -> Result<Vec<AuditedFile>> {
    let mut items = Vec::new();
    if !root.exists() {
        return Ok(items);
    }

    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        if !is_media_file(path) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(value) => value,
            Err(_) => continue,
        };

        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();

        items.push(AuditedFile {
            path: path.display().to_string(),
            relative_path,
            file_name,
            path_root_kind: root_kind.to_string(),
            filesystem: file_system_facts(&metadata),
        });
    }

    Ok(items)
}

pub fn joined_path(root: &Path, relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        anyhow::bail!("path must be relative");
    }

    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => sanitized.push(value),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => anyhow::bail!("parent directory traversal is not allowed"),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                anyhow::bail!("invalid path component")
            }
        }
    }

    Ok(root.join(sanitized))
}

pub fn normalized_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

pub fn is_media_file(path: &Path) -> bool {
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
            | "ssa"
            | "sub"
    )
}