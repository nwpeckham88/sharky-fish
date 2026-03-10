use crate::config::{AppConfig, LibraryFolder};
use crate::internet_metadata::InternetMetadataMatch;

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct OrganizeRequest {
    pub relative_path: String,
    pub library_id: Option<String>,
    pub selected: InternetMetadataMatch,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganizeResult {
    pub current_relative_path: String,
    pub target_relative_path: String,
    pub changed: bool,
    pub applied: bool,
}

pub async fn preview_or_apply(
    config: &AppConfig,
    library_root: &Path,
    request: OrganizeRequest,
    apply: bool,
) -> Result<OrganizeResult> {
    let current_relative = sanitize_relative_path(&request.relative_path)?;
    let current_relative_str = current_relative.to_string_lossy().replace('\\', "/");

    let library_folder = resolve_library_folder(config, &current_relative_str, request.library_id.as_deref())
        .context("unable to resolve library folder for path")?;

    let extension = current_relative
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default();

    let target_relative = if library_folder.media_type == "tv" {
        let (season, episode) = infer_or_validate_episode_numbers(
            &current_relative_str,
            request.season,
            request.episode,
        )?;
        build_tv_target(&library_folder.path, &request.selected, season, episode, &extension)
    } else {
        build_movie_target(&library_folder.path, &request.selected, &extension)
    };

    let target_relative = sanitize_relative_path(&target_relative)?;
    let target_relative_str = target_relative.to_string_lossy().replace('\\', "/");

    let changed = current_relative_str != target_relative_str;
    if apply && changed {
        let source_abs = library_root.join(&current_relative);
        let target_abs = library_root.join(&target_relative);

        if !source_abs.exists() {
            anyhow::bail!("source file does not exist");
        }
        if target_abs.exists() {
            anyhow::bail!("target file already exists");
        }

        if let Some(parent) = target_abs.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&source_abs, &target_abs)
            .await
            .with_context(|| {
                format!(
                    "failed to rename '{}' to '{}'",
                    source_abs.display(),
                    target_abs.display()
                )
            })?;
    }

    Ok(OrganizeResult {
        current_relative_path: current_relative_str,
        target_relative_path: target_relative_str,
        changed,
        applied: apply && changed,
    })
}

fn resolve_library_folder<'a>(
    config: &'a AppConfig,
    relative_path: &str,
    library_id: Option<&str>,
) -> Option<&'a LibraryFolder> {
    if let Some(id) = library_id {
        return config.libraries.iter().find(|lib| lib.id == id);
    }

    config.libraries.iter().find(|lib| {
        let prefix = if lib.path.ends_with('/') {
            lib.path.clone()
        } else {
            format!("{}/", lib.path)
        };
        relative_path == lib.path || relative_path.starts_with(&prefix)
    })
}

fn build_movie_target(library_prefix: &str, selected: &InternetMetadataMatch, extension: &str) -> String {
    let title = sanitize_segment(&selected.title);
    let year = selected.year.map(|v| v.to_string()).unwrap_or_else(|| "0000".into());
    let movie_dir = format!("{} ({})", title, year);
    let file_name = format!("{} ({})", title, year);

    if extension.is_empty() {
        format!("{}/{}/{}", trim_slashes(library_prefix), movie_dir, file_name)
    } else {
        format!(
            "{}/{}/{}.{}",
            trim_slashes(library_prefix),
            movie_dir,
            file_name,
            extension
        )
    }
}

fn build_tv_target(
    library_prefix: &str,
    selected: &InternetMetadataMatch,
    season: u32,
    episode: u32,
    extension: &str,
) -> String {
    let show = sanitize_segment(&selected.title);
    let season_dir = format!("Season {:02}", season);
    let episode_name = format!("{} - S{:02}E{:02}", show, season, episode);

    if extension.is_empty() {
        format!("{}/{}/{}/{}", trim_slashes(library_prefix), show, season_dir, episode_name)
    } else {
        format!(
            "{}/{}/{}/{}.{}",
            trim_slashes(library_prefix),
            show,
            season_dir,
            episode_name,
            extension
        )
    }
}

fn infer_or_validate_episode_numbers(
    relative_path: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<(u32, u32)> {
    if let (Some(s), Some(e)) = (season, episode) {
        return Ok((s, e));
    }

    if let Some((s, e)) = parse_sxxexx(relative_path) {
        return Ok((season.unwrap_or(s), episode.unwrap_or(e)));
    }

    anyhow::bail!("season/episode required for TV files when not inferable from filename")
}

fn parse_sxxexx(input: &str) -> Option<(u32, u32)> {
    let upper = input.to_ascii_uppercase();
    let bytes = upper.as_bytes();

    for i in 0..bytes.len().saturating_sub(5) {
        if bytes[i] != b'S' {
            continue;
        }
        if !bytes.get(i + 1)?.is_ascii_digit() || !bytes.get(i + 2)?.is_ascii_digit() {
            continue;
        }
        if bytes.get(i + 3)? != &b'E' {
            continue;
        }
        if !bytes.get(i + 4)?.is_ascii_digit() || !bytes.get(i + 5)?.is_ascii_digit() {
            continue;
        }

        let season = std::str::from_utf8(&bytes[i + 1..=i + 2]).ok()?.parse().ok()?;
        let episode = std::str::from_utf8(&bytes[i + 4..=i + 5]).ok()?.parse().ok()?;
        return Some((season, episode));
    }

    None
}

fn sanitize_relative_path(relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        anyhow::bail!("path must be relative");
    }

    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => sanitized.push(value),
            Component::CurDir => {}
            Component::ParentDir => anyhow::bail!("parent directory traversal is not allowed"),
            Component::RootDir | Component::Prefix(_) => anyhow::bail!("invalid path component"),
        }
    }

    Ok(sanitized)
}

fn sanitize_segment(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '(' | ')') {
            out.push(c);
        }
    }

    let compact = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "Unknown".into()
    } else {
        compact
    }
}

fn trim_slashes(value: &str) -> String {
    value.trim_matches('/').to_string()
}
