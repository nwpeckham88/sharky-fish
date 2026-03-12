use crate::config::{AppConfig, LibraryFolder};
use crate::internet_metadata::InternetMetadataMatch;
use crate::sidecar;

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
    pub scope: Option<String>,
    pub id_mode: Option<String>,
    pub write_nfo: bool,
    pub merge_existing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganizeResult {
    pub current_relative_path: String,
    pub target_relative_path: String,
    pub changed: bool,
    pub applied: bool,
    pub scope: String,
    pub target_exists: bool,
    pub conflict_path: Option<String>,
    pub metadata_sidecar_path: Option<String>,
    pub metadata_sidecar_written: bool,
}

pub async fn preview_or_apply(
    config: &AppConfig,
    library_root: &Path,
    request: OrganizeRequest,
    apply: bool,
) -> Result<OrganizeResult> {
    let current_relative = sanitize_relative_path(&request.relative_path)?;
    let current_relative_str = current_relative.to_string_lossy().replace('\\', "/");
    let scope = request.scope.as_deref().unwrap_or("file");

    let library_folder =
        resolve_library_folder(config, &current_relative_str, request.library_id.as_deref())
            .context("unable to resolve library folder for path")?;

    let extension = current_relative
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default();

    let id_mode = request.id_mode.as_deref().unwrap_or("none");

    let episode_numbers = if library_folder.media_type == "tv" {
        let (season, episode) = infer_or_validate_episode_numbers(
            &current_relative_str,
            request.season,
            request.episode,
        )?;
        Some((season, episode))
    } else {
        None
    };

    let target_relative = if library_folder.media_type == "tv" {
        let (season, episode) = episode_numbers.context("season/episode missing for tv target")?;
        build_tv_target(
            &library_folder.path,
            &request.selected,
            season,
            episode,
            &extension,
            id_mode,
        )
    } else {
        build_movie_target(&library_folder.path, &request.selected, &extension, id_mode)
    };

    let target_relative = sanitize_relative_path(&target_relative)?;
    let target_relative_str = target_relative.to_string_lossy().replace('\\', "/");

    let (changed, target_exists, conflict_path) =
        if scope == "movie_folder" && library_folder.media_type == "movie" {
            let source_container = current_relative
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(PathBuf::new);
            let source_container_str = source_container.to_string_lossy().replace('\\', "/");
            let target_container = movie_target_container(&library_folder.path, &request.selected, id_mode);
            let target_container = sanitize_relative_path(&target_container)?;
            let target_container_str = target_container.to_string_lossy().replace('\\', "/");
            let target_container_abs = library_root.join(&target_container);
            let changed = source_container_str != target_container_str;
            let target_exists = changed && target_container_abs.exists();
            (
                changed,
                target_exists,
                target_exists.then_some(target_container_str),
            )
        } else {
            let changed = current_relative_str != target_relative_str;
            let target_abs = library_root.join(&target_relative);
            let target_exists = changed && target_abs.exists();
            (
                changed,
                target_exists,
                target_exists.then_some(target_relative_str.clone()),
            )
        };

    if apply && changed {
        let source_abs = library_root.join(&current_relative);

        if scope == "movie_folder" && library_folder.media_type == "movie" {
            apply_movie_folder_organization(
                library_root,
                &current_relative,
                &target_relative,
                library_folder,
                &request.selected,
                request.merge_existing,
            )
            .await?;
        } else {
            let target_abs = library_root.join(&target_relative);

            if !source_abs.exists() {
                anyhow::bail!("source file does not exist");
            }
            if target_exists {
                anyhow::bail!("target file already exists: {}", target_relative_str);
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

            rename_sidecar_for_file(library_root, &current_relative, &target_relative).await?;
        }
    }

    let metadata_sidecar_path = if request.write_nfo {
        sidecar::metadata_sidecar_relative_path(&target_relative_str)
    } else {
        None
    };
    let mut metadata_sidecar_written = false;
    if apply && request.write_nfo {
        let (season, episode) = episode_numbers.map(|(s, e)| (Some(s), Some(e))).unwrap_or((None, None));
        sidecar::write_jellyfin_nfo(
            library_root,
            &target_relative_str,
            &request.selected,
            season,
            episode,
        )
        .await?;
        metadata_sidecar_written = true;
    }

    Ok(OrganizeResult {
        current_relative_path: current_relative_str,
        target_relative_path: target_relative_str,
        changed,
        applied: apply && changed,
        scope: scope.to_string(),
        target_exists,
        conflict_path,
        metadata_sidecar_path,
        metadata_sidecar_written,
    })
}

pub fn movie_target_container(
    library_prefix: &str,
    selected: &InternetMetadataMatch,
    id_mode: &str,
) -> String {
    let title = movie_title_segment(selected, id_mode);
    let year = selected
        .year
        .map(|value| value.to_string())
        .unwrap_or_else(|| "0000".into());
    format!("{}/{} ({})", trim_slashes(library_prefix), title, year)
}

pub fn preview_target_relative_path(
    config: &AppConfig,
    relative_path: &str,
    library_id: Option<&str>,
    selected: &InternetMetadataMatch,
) -> Result<String> {
    let current_relative = sanitize_relative_path(relative_path)?;
    let current_relative_str = current_relative.to_string_lossy().replace('\\', "/");
    let library_folder = resolve_library_folder(config, &current_relative_str, library_id)
        .context("unable to resolve library folder for path")?;

    let extension = current_relative
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default();

    let target_relative = if library_folder.media_type == "tv" {
        let (season, episode) =
            infer_or_validate_episode_numbers(&current_relative_str, None, None)?;
        build_tv_target(&library_folder.path, selected, season, episode, &extension, "none")
    } else {
        build_movie_target(&library_folder.path, selected, &extension, "none")
    };

    Ok(sanitize_relative_path(&target_relative)?
        .to_string_lossy()
        .replace('\\', "/"))
}

async fn apply_movie_folder_organization(
    library_root: &Path,
    current_relative: &Path,
    target_relative: &Path,
    library_folder: &LibraryFolder,
    selected: &InternetMetadataMatch,
    merge_existing: bool,
) -> Result<()> {
    let source_file_abs = library_root.join(current_relative);
    if !source_file_abs.exists() {
        anyhow::bail!("source file does not exist");
    }

    let source_dir_relative = current_relative.parent().unwrap_or_else(|| Path::new(""));
    let source_dir_abs = library_root.join(source_dir_relative);
    let target_dir_relative = PathBuf::from(movie_target_container(&library_folder.path, selected, "none"));
    let target_dir_abs = library_root.join(&target_dir_relative);

    let source_stem = current_relative
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("movie")
        .to_string();
    let target_stem = target_relative
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("movie")
        .to_string();

    if source_dir_abs == target_dir_abs {
        rename_matching_sidecars_in_dir(&source_dir_abs, &source_stem, &target_stem).await?;
        return Ok(());
    }

    if !target_dir_abs.exists() {
        if let Some(parent) = target_dir_abs.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&source_dir_abs, &target_dir_abs)
            .await
            .with_context(|| {
                format!(
                    "failed to rename '{}' to '{}'",
                    source_dir_abs.display(),
                    target_dir_abs.display()
                )
            })?;
        rename_matching_sidecars_in_dir(&target_dir_abs, &source_stem, &target_stem).await?;
        return Ok(());
    }

    if !merge_existing {
        anyhow::bail!(
            "target folder already exists: {}",
            target_dir_relative.display()
        );
    }

    merge_movie_folder_contents(&source_dir_abs, &target_dir_abs, &source_stem, &target_stem)
        .await?;
    Ok(())
}

async fn rename_sidecar_for_file(
    library_root: &Path,
    current_relative: &Path,
    target_relative: &Path,
) -> Result<()> {
    let current_relative = current_relative.to_string_lossy().replace('\\', "/");
    let target_relative = target_relative.to_string_lossy().replace('\\', "/");
    rename_one_sidecar(
        sidecar::sidecar_absolute_path(library_root, &current_relative),
        sidecar::sidecar_absolute_path(library_root, &target_relative),
    )
    .await?;
    rename_one_sidecar(
        sidecar::metadata_sidecar_absolute_path(library_root, &current_relative),
        sidecar::metadata_sidecar_absolute_path(library_root, &target_relative),
    )
    .await?;

    Ok(())
}

async fn rename_one_sidecar(current: Option<PathBuf>, target: Option<PathBuf>) -> Result<()> {
    let (Some(current_sidecar), Some(target_sidecar)) = (current, target) else {
        return Ok(());
    };

    if !tokio::fs::try_exists(&current_sidecar).await? || current_sidecar == target_sidecar {
        return Ok(());
    }
    if tokio::fs::try_exists(&target_sidecar).await? {
        return Ok(());
    }

    if let Some(parent) = target_sidecar.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::rename(&current_sidecar, &target_sidecar)
        .await
        .with_context(|| {
            format!(
                "failed to rename '{}' to '{}'",
                current_sidecar.display(),
                target_sidecar.display()
            )
        })?;

    Ok(())
}

async fn rename_matching_sidecars_in_dir(
    dir_abs: &Path,
    source_stem: &str,
    target_stem: &str,
) -> Result<()> {
    let mut entries = tokio::fs::read_dir(dir_abs).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let Some(mapped) = remap_movie_entry_name(&file_name, source_stem, target_stem) else {
            continue;
        };
        let dest_path = dir_abs.join(&mapped);
        if dest_path == path {
            continue;
        }
        if dest_path.exists() {
            continue;
        }
        tokio::fs::rename(&path, &dest_path)
            .await
            .with_context(|| {
                format!(
                    "failed to rename '{}' to '{}'",
                    path.display(),
                    dest_path.display()
                )
            })?;
    }
    Ok(())
}

async fn merge_movie_folder_contents(
    source_dir_abs: &Path,
    target_dir_abs: &Path,
    source_stem: &str,
    target_stem: &str,
) -> Result<()> {
    let mut entries = tokio::fs::read_dir(source_dir_abs).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let mapped_name = remap_movie_entry_name(&file_name, source_stem, target_stem)
            .unwrap_or_else(|| file_name.to_string());
        let dest_path = target_dir_abs.join(mapped_name);
        if dest_path.exists() {
            continue;
        }
        tokio::fs::rename(&path, &dest_path)
            .await
            .with_context(|| {
                format!(
                    "failed to merge '{}' into '{}'",
                    path.display(),
                    dest_path.display()
                )
            })?;
    }

    if tokio::fs::read_dir(source_dir_abs)
        .await?
        .next_entry()
        .await?
        .is_none()
    {
        let _ = tokio::fs::remove_dir(source_dir_abs).await;
    }
    Ok(())
}

fn remap_movie_entry_name(file_name: &str, source_stem: &str, target_stem: &str) -> Option<String> {
    if file_name == source_stem {
        return Some(target_stem.to_string());
    }
    let prefix = format!("{}.", source_stem);
    if let Some(rest) = file_name.strip_prefix(&prefix) {
        return Some(format!("{}.{}", target_stem, rest));
    }
    None
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

fn build_movie_target(
    library_prefix: &str,
    selected: &InternetMetadataMatch,
    extension: &str,
    id_mode: &str,
) -> String {
    let title = movie_title_segment(selected, id_mode);
    let year = selected
        .year
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0000".into());
    let movie_dir = format!("{} ({})", title, year);
    let file_name = format!("{} ({})", title, year);

    if extension.is_empty() {
        format!(
            "{}/{}/{}",
            trim_slashes(library_prefix),
            movie_dir,
            file_name
        )
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
    id_mode: &str,
) -> String {
    let show = tv_show_segment(selected, id_mode);
    let season_dir = format!("Season {:02}", season);
    let episode_name = format!("{} - S{:02}E{:02}", show, season, episode);

    if extension.is_empty() {
        format!(
            "{}/{}/{}/{}",
            trim_slashes(library_prefix),
            show,
            season_dir,
            episode_name
        )
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

fn movie_title_segment(selected: &InternetMetadataMatch, id_mode: &str) -> String {
    with_external_id(&selected.title, metadata_id_suffix(selected, id_mode))
}

fn tv_show_segment(selected: &InternetMetadataMatch, id_mode: &str) -> String {
    with_external_id(&selected.title, metadata_id_suffix(selected, id_mode))
}

fn with_external_id(title: &str, suffix: Option<String>) -> String {
    let base = sanitize_segment(title);
    match suffix {
        Some(suffix) => format!("{} {}", base, suffix),
        None => base,
    }
}

fn metadata_id_suffix(selected: &InternetMetadataMatch, id_mode: &str) -> Option<String> {
    match id_mode {
        "imdb" => selected
            .imdb_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("[imdbid-{}]", sanitize_segment(value))),
        "tvdb" => selected.tvdb_id.map(|value| format!("[tvdbid-{}]", value)),
        _ => None,
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

        let season = std::str::from_utf8(&bytes[i + 1..=i + 2])
            .ok()?
            .parse()
            .ok()?;
        let episode = std::str::from_utf8(&bytes[i + 4..=i + 5])
            .ok()?
            .parse()
            .ok()?;
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
