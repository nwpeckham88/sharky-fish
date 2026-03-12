use crate::config::AppConfig;
use crate::db;
use crate::internet_metadata::InternetMetadataMatch;
use crate::library_index;
use crate::messages::ProcessingDecision;
use crate::organizer;
use crate::sidecar::{self, ManagedItemSidecar};

use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct JobGroup {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntakeManagedItem {
    pub relative_path: String,
    pub file_path: String,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub library_id: Option<String>,
    pub managed_status: String,
    pub has_sidecar: bool,
    pub missing_metadata: bool,
    pub missing_sidecar: bool,
    pub organize_needed: bool,
    pub selected_metadata: Option<InternetMetadataMatch>,
    pub last_decision: Option<ProcessingDecision>,
    pub group_key: Option<String>,
    pub group_label: Option<String>,
    pub group_kind: String,
    pub group_source: String,
    pub member_paths: Vec<String>,
    pub member_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacklogSummary {
    pub total_items: u64,
    pub needs_attention_count: u64,
    pub unprocessed_count: u64,
    pub reviewed_count: u64,
    pub kept_original_count: u64,
    pub awaiting_approval_count: u64,
    pub approved_count: u64,
    pub processed_count: u64,
    pub failed_count: u64,
    pub missing_metadata_count: u64,
    pub missing_sidecar_count: u64,
    pub organize_needed_count: u64,
}

pub async fn sync_library_file(
    pool: &SqlitePool,
    library_root: &Path,
    relative_path: &str,
    file_path: &str,
    file_name: &str,
    media_type: &str,
    size_bytes: u64,
    modified_at: u64,
    library_id: Option<&str>,
) -> Result<()> {
    let existing = db::fetch_managed_item(pool, relative_path).await?;
    let sidecar_doc = sidecar::read_sidecar(library_root, relative_path).await?;
    let now = unix_now();

    let managed_status = sidecar_doc
        .as_ref()
        .map(|value| value.managed_status.clone())
        .or_else(|| existing.as_ref().map(|value| value.managed_status.clone()))
        .unwrap_or_else(|| "UNPROCESSED".into());

    let selected_metadata_json = match sidecar_doc.as_ref() {
        Some(value) => value
            .selected_metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
        None => existing
            .as_ref()
            .and_then(|value| value.selected_metadata_json.clone()),
    };

    let last_decision_json = match sidecar_doc.as_ref() {
        Some(value) => value
            .last_decision
            .as_ref()
            .map(|decision| {
                serde_json::to_string(&sidecar::processing_decision_from_sidecar(decision))
            })
            .transpose()?,
        None => existing
            .as_ref()
            .and_then(|value| value.last_decision_json.clone()),
    };

    let first_seen_at = existing
        .as_ref()
        .map(|value| value.first_seen_at.max(0) as u64)
        .or_else(|| sidecar_doc.as_ref().and_then(|value| value.first_seen_at))
        .unwrap_or(now);

    let sidecar_path = sidecar::find_metadata_sidecar_relative_path(library_root, relative_path).await?;

    db::upsert_managed_item(
        pool,
        relative_path,
        file_path,
        file_name,
        media_type,
        size_bytes,
        modified_at,
        library_id,
        &managed_status,
        selected_metadata_json.as_deref(),
        last_decision_json.as_deref(),
        sidecar_path.as_deref(),
        first_seen_at,
        now,
    )
    .await?;

    Ok(())
}

pub async fn remove_missing_item(pool: &SqlitePool, relative_path: &str) -> Result<()> {
    db::delete_managed_item(pool, relative_path).await
}

pub async fn persist_selected_metadata(
    pool: &SqlitePool,
    library_root: &Path,
    relative_path: &str,
    selected: &InternetMetadataMatch,
) -> Result<()> {
    let row = db::fetch_managed_item(pool, relative_path)
        .await?
        .with_context(|| format!("managed item not found for {}", relative_path))?;

    let managed_status = if row.managed_status == "UNPROCESSED" {
        "REVIEWED"
    } else {
        row.managed_status.as_str()
    };

    let selected_json = serde_json::to_string(selected)?;

    db::upsert_managed_item(
        pool,
        &row.relative_path,
        &row.file_path,
        &row.file_name,
        &row.media_type,
        row.size_bytes.max(0) as u64,
        row.modified_at.max(0) as u64,
        row.library_id.as_deref(),
        managed_status,
        Some(&selected_json),
        row.last_decision_json.as_deref(),
        row.sidecar_path.as_deref(),
        row.first_seen_at.max(0) as u64,
        unix_now(),
    )
    .await?;

    let updated_row = db::fetch_managed_item(pool, relative_path)
        .await?
        .with_context(|| format!("managed item disappeared for {}", relative_path))?;
    write_sidecar_from_row(library_root, &updated_row).await
}

pub async fn persist_processing_decision(
    pool: &SqlitePool,
    library_root: &Path,
    relative_path: &str,
    managed_status: &str,
    decision: &ProcessingDecision,
) -> Result<()> {
    let Some(row) = db::fetch_managed_item(pool, relative_path).await? else {
        return Ok(());
    };

    let decision_json = serde_json::to_string(decision)?;

    db::upsert_managed_item(
        pool,
        &row.relative_path,
        &row.file_path,
        &row.file_name,
        &row.media_type,
        row.size_bytes.max(0) as u64,
        row.modified_at.max(0) as u64,
        row.library_id.as_deref(),
        managed_status,
        row.selected_metadata_json.as_deref(),
        Some(&decision_json),
        row.sidecar_path.as_deref(),
        row.first_seen_at.max(0) as u64,
        unix_now(),
    )
    .await?;

    let updated_row = db::fetch_managed_item(pool, relative_path)
        .await?
        .with_context(|| format!("managed item disappeared for {}", relative_path))?;
    write_sidecar_from_row(library_root, &updated_row).await
}

pub async fn list_unprocessed(
    pool: &SqlitePool,
    config: &AppConfig,
    limit: i64,
    offset: i64,
) -> Result<Vec<IntakeManagedItem>> {
    let rows = db::list_managed_items_filtered(pool, None, false, false, false, i64::MAX, 0)
        .await?;
    let items = aggregate_backlog_items(config, rows)?;
    Ok(paginate_items(
        items
            .into_iter()
            .filter(|item| item.managed_status == "UNPROCESSED")
            .collect(),
        limit,
        offset,
    ))
}

pub async fn list_filtered(
    pool: &SqlitePool,
    config: &AppConfig,
    managed_status: Option<&str>,
    missing_metadata_only: bool,
    missing_sidecar_only: bool,
    needs_attention_only: bool,
    organize_needed_only: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<IntakeManagedItem>> {
    let rows = db::list_managed_items_filtered(pool, None, false, false, false, i64::MAX, 0)
        .await?;
    let items = aggregate_backlog_items(config, rows)?;

    Ok(paginate_items(
        items
            .into_iter()
            .filter(|item| {
                if let Some(status) = managed_status {
                    if item.managed_status != status {
                        return false;
                    }
                }
                if missing_metadata_only && !item.missing_metadata {
                    return false;
                }
                if missing_sidecar_only && !item.missing_sidecar {
                    return false;
                }
                if organize_needed_only && !item.organize_needed {
                    return false;
                }
                if needs_attention_only && !item_needs_attention(item) {
                    return false;
                }
                true
            })
            .collect(),
        limit,
        offset,
    ))
}

pub async fn summarize(pool: &SqlitePool, config: &AppConfig) -> Result<BacklogSummary> {
    let all_rows = db::list_managed_items_filtered(pool, None, false, false, false, i64::MAX, 0)
        .await?;
    let items = aggregate_backlog_items(config, all_rows)?;

    Ok(BacklogSummary {
        total_items: items.len() as u64,
        needs_attention_count: items.iter().filter(|item| item_needs_attention(item)).count() as u64,
        unprocessed_count: items.iter().filter(|item| item.managed_status == "UNPROCESSED").count() as u64,
        reviewed_count: items.iter().filter(|item| item.managed_status == "REVIEWED").count() as u64,
        kept_original_count: items.iter().filter(|item| item.managed_status == "KEPT_ORIGINAL").count() as u64,
        awaiting_approval_count: items.iter().filter(|item| item.managed_status == "AWAITING_APPROVAL").count() as u64,
        approved_count: items.iter().filter(|item| item.managed_status == "APPROVED").count() as u64,
        processed_count: items.iter().filter(|item| item.managed_status == "PROCESSED").count() as u64,
        failed_count: items.iter().filter(|item| item.managed_status == "FAILED").count() as u64,
        missing_metadata_count: items.iter().filter(|item| item.missing_metadata).count() as u64,
        missing_sidecar_count: items.iter().filter(|item| item.missing_sidecar).count() as u64,
        organize_needed_count: items.iter().filter(|item| item.organize_needed).count() as u64,
    })
}

fn item_needs_attention(item: &IntakeManagedItem) -> bool {
    item.managed_status == "UNPROCESSED"
        || item.managed_status == "FAILED"
        || item.managed_status == "AWAITING_APPROVAL"
        || item.missing_metadata
        || item.missing_sidecar
        || item.organize_needed
}

fn managed_item_needs_organize(config: &AppConfig, row: &db::ManagedItemRow) -> bool {
    let Some(selected_metadata_json) = row.selected_metadata_json.as_deref() else {
        return false;
    };
    let Ok(selected) = serde_json::from_str::<InternetMetadataMatch>(selected_metadata_json) else {
        return false;
    };
    let Ok(target_relative_path) = organizer::preview_target_relative_path(
        config,
        &row.relative_path,
        row.library_id.as_deref(),
        &selected,
    ) else {
        return false;
    };

    target_relative_path != row.relative_path
}

pub async fn update_managed_status(
    pool: &SqlitePool,
    library_root: &Path,
    relative_path: &str,
    managed_status: &str,
) -> Result<()> {
    let row = db::fetch_managed_item(pool, relative_path)
        .await?
        .with_context(|| format!("managed item not found for {}", relative_path))?;

    db::update_managed_item_status(pool, relative_path, managed_status, unix_now()).await?;

    let updated_row = db::fetch_managed_item(pool, relative_path)
        .await?
        .with_context(|| format!("managed item disappeared for {}", relative_path))?;

    if updated_row.selected_metadata_json.is_none()
        && updated_row.last_decision_json.is_none()
        && row.selected_metadata_json.is_none()
        && row.last_decision_json.is_none()
        && updated_row.sidecar_path.is_none()
    {
        return Ok(());
    }

    write_sidecar_from_row(library_root, &updated_row).await
}

pub async fn resolve_job_group(
    pool: &SqlitePool,
    config: &AppConfig,
    relative_path: &str,
) -> Result<JobGroup> {
    let normalized_path = relative_path.replace('\\', "/");
    let row = db::fetch_managed_item(pool, &normalized_path).await?;

    if let Some(group) = row
        .as_ref()
        .and_then(|item| item.selected_metadata_json.as_deref())
        .and_then(|json| serde_json::from_str::<InternetMetadataMatch>(json).ok())
        .and_then(job_group_from_selected_metadata)
    {
        return Ok(group);
    }

    if let Some(group) = row.as_ref().and_then(|item| {
        derive_tv_group_from_library(config, &normalized_path, item.library_id.as_deref())
    }) {
        return Ok(group);
    }

    let label = Path::new(&normalized_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&normalized_path)
        .to_string();

    Ok(JobGroup {
        key: format!("file:{}", normalized_path.to_ascii_lowercase()),
        label,
        kind: "file".into(),
        source: "file".into(),
    })
}

fn job_group_from_selected_metadata(selected: InternetMetadataMatch) -> Option<JobGroup> {
    let media_kind = selected.media_kind.trim().to_ascii_lowercase();
    if media_kind != "series" {
        return None;
    }

    let title = selected.title.trim();
    if title.is_empty() {
        return None;
    }

    let key = selected
        .tvdb_id
        .map(|value| format!("tvdb:{value}"))
        .or_else(|| {
            selected
                .imdb_id
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("imdb:{value}"))
        })
        .unwrap_or_else(|| format!("series:{}", title.to_ascii_lowercase()));

    Some(JobGroup {
        key,
        label: title.to_string(),
        kind: "tv_show".into(),
        source: "metadata".into(),
    })
}

fn derive_tv_group_from_library(
    config: &AppConfig,
    relative_path: &str,
    library_id: Option<&str>,
) -> Option<JobGroup> {
    let library = if let Some(id) = library_id {
        config
            .libraries
            .iter()
            .find(|candidate| candidate.id == id && candidate.media_type == "tv")
    } else {
        config
            .libraries
            .iter()
            .filter(|candidate| candidate.media_type == "tv")
            .find(|candidate| {
                let prefix = normalize_library_prefix(&candidate.path);
                relative_path == prefix || relative_path.starts_with(&format!("{prefix}/"))
            })
    };

    let library = library.filter(|candidate| candidate.media_type == "tv")?;
    let stripped = strip_library_prefix(relative_path, &library.path).unwrap_or(relative_path);
    let show = stripped
        .split('/')
        .find(|segment| !segment.trim().is_empty())?
        .trim();
    if show.is_empty() {
        return None;
    }

    Some(JobGroup {
        key: format!("tv-path:{}:{}", library.id, show.to_ascii_lowercase()),
        label: show.to_string(),
        kind: "tv_show".into(),
        source: "library".into(),
    })
}

fn strip_library_prefix<'a>(relative_path: &'a str, library_path: &str) -> Option<&'a str> {
    let normalized_library = normalize_library_prefix(library_path);
    if relative_path == normalized_library {
        return Some("");
    }

    relative_path
        .strip_prefix(&format!("{normalized_library}/"))
        .or(Some(relative_path))
}

fn normalize_library_prefix(value: &str) -> String {
    value.trim_matches('/').replace('\\', "/")
}

fn aggregate_backlog_items(
    config: &AppConfig,
    rows: Vec<db::ManagedItemRow>,
) -> Result<Vec<IntakeManagedItem>> {
    let mut items = Vec::new();
    let mut grouped_rows = std::collections::HashMap::<String, (JobGroup, Vec<db::ManagedItemRow>)>::new();

    for row in rows {
        if let Some(group) = backlog_group_for_row(config, &row) {
            grouped_rows
                .entry(group.key.clone())
                .or_insert_with(|| (group.clone(), Vec::new()))
                .1
                .push(row);
            continue;
        }

        items.push(intake_item_from_row(config, row)?);
    }

    for (_, (group, group_rows)) in grouped_rows {
        items.push(intake_item_from_group(config, group, group_rows)?);
    }

    items.sort_by(compare_backlog_items);
    Ok(items)
}

fn backlog_group_for_row(config: &AppConfig, row: &db::ManagedItemRow) -> Option<JobGroup> {
    row.selected_metadata_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<InternetMetadataMatch>(json).ok())
        .and_then(job_group_from_selected_metadata)
        .or_else(|| derive_tv_group_from_library(config, &row.relative_path, row.library_id.as_deref()))
        .or_else(|| derive_tv_group_from_path_heuristic(&row.relative_path))
}

fn derive_tv_group_from_path_heuristic(relative_path: &str) -> Option<JobGroup> {
    let normalized = relative_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter_map(|segment| {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect::<Vec<_>>();

    if segments.len() < 2 {
        return None;
    }

    let has_episode_pattern = segments.iter().any(|segment| contains_episode_marker(segment));
    let has_season_folder = segments.iter().any(|segment| is_season_segment(segment));
    if !has_episode_pattern && !has_season_folder {
        return None;
    }

    let root_tokens = ["tv", "television", "shows", "series", "tv-shows", "tvshows"];
    let show_index = if root_tokens.contains(&segments[0].to_ascii_lowercase().as_str()) {
        1
    } else {
        0
    };

    let show = segments.get(show_index)?.trim();
    if show.is_empty() || is_season_segment(show) {
        return None;
    }

    Some(JobGroup {
        key: format!("tv-heuristic:{}", show.to_ascii_lowercase()),
        label: show.to_string(),
        kind: "tv_show".into(),
        source: "path".into(),
    })
}

fn contains_episode_marker(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 6 {
        return false;
    }

    for window in bytes.windows(6) {
        let first = window[0].to_ascii_lowercase();
        if first != b's' {
            continue;
        }
        if !window[1].is_ascii_digit() || !window[2].is_ascii_digit() {
            continue;
        }
        let fourth = window[3].to_ascii_lowercase();
        if fourth != b'e' {
            continue;
        }
        if window[4].is_ascii_digit() && window[5].is_ascii_digit() {
            return true;
        }
    }

    false
}

fn is_season_segment(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    if !normalized.starts_with("season") {
        return false;
    }

    normalized[6..]
        .chars()
        .all(|ch| ch.is_ascii_whitespace() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn intake_item_from_group(
    config: &AppConfig,
    group: JobGroup,
    mut rows: Vec<db::ManagedItemRow>,
) -> Result<IntakeManagedItem> {
    rows.sort_by(compare_managed_item_rows);
    let lead = rows
        .first()
        .cloned()
        .context("backlog group missing representative row")?;

    let member_paths = rows
        .iter()
        .map(|row| row.relative_path.clone())
        .collect::<Vec<_>>();
    let missing_metadata = rows.iter().any(row_missing_metadata);
    let missing_sidecar = rows.iter().any(row_missing_sidecar);
    let organize_needed = rows.iter().any(|row| managed_item_needs_organize(config, row));
    let selected_metadata = if missing_metadata {
        None
    } else {
        rows.iter()
            .filter_map(|row| row.selected_metadata_json.as_deref())
            .find_map(|json| serde_json::from_str::<InternetMetadataMatch>(json).ok())
    };
    let last_decision = lead
        .last_decision_json
        .as_deref()
        .map(serde_json::from_str::<ProcessingDecision>)
        .transpose()?;

    Ok(IntakeManagedItem {
        relative_path: lead.relative_path,
        file_path: lead.file_path,
        file_name: group.label.clone(),
        media_type: lead.media_type,
        size_bytes: rows.iter().map(|row| row.size_bytes.max(0) as u64).sum(),
        modified_at: rows
            .iter()
            .map(|row| row.modified_at.max(0) as u64)
            .max()
            .unwrap_or(0),
        library_id: lead.library_id,
        managed_status: lead.managed_status,
        has_sidecar: !missing_sidecar,
        missing_metadata,
        missing_sidecar,
        organize_needed,
        selected_metadata,
        last_decision,
        group_key: Some(group.key),
        group_label: Some(group.label),
        group_kind: group.kind,
        group_source: group.source,
        member_count: member_paths.len() as u64,
        member_paths,
    })
}

fn compare_managed_item_rows(left: &db::ManagedItemRow, right: &db::ManagedItemRow) -> std::cmp::Ordering {
    status_priority(&left.managed_status)
        .cmp(&status_priority(&right.managed_status))
        .then_with(|| right.modified_at.cmp(&left.modified_at))
        .then_with(|| left.relative_path.cmp(&right.relative_path))
}

fn compare_backlog_items(left: &IntakeManagedItem, right: &IntakeManagedItem) -> std::cmp::Ordering {
    status_priority(&left.managed_status)
        .cmp(&status_priority(&right.managed_status))
        .then_with(|| right.modified_at.cmp(&left.modified_at))
        .then_with(|| left.file_name.cmp(&right.file_name))
        .then_with(|| left.relative_path.cmp(&right.relative_path))
}

fn status_priority(status: &str) -> u8 {
    match status {
        "FAILED" => 0,
        "UNPROCESSED" => 1,
        "AWAITING_APPROVAL" => 2,
        "APPROVED" => 3,
        "REVIEWED" => 4,
        "KEPT_ORIGINAL" => 5,
        "PROCESSED" => 6,
        _ => 7,
    }
}

fn row_missing_metadata(row: &db::ManagedItemRow) -> bool {
    row.selected_metadata_json.is_none()
        && row.managed_status != "KEPT_ORIGINAL"
        && row.managed_status != "PROCESSED"
}

fn row_missing_sidecar(row: &db::ManagedItemRow) -> bool {
    row.sidecar_path.is_none()
}

fn paginate_items(items: Vec<IntakeManagedItem>, limit: i64, offset: i64) -> Vec<IntakeManagedItem> {
    items
        .into_iter()
        .skip(offset.max(0) as usize)
        .take(limit.max(0) as usize)
        .collect()
}

pub async fn reconcile_after_organize(
    pool: &SqlitePool,
    library_root: &Path,
    current_relative_path: &str,
    target_relative_path: &str,
) -> Result<()> {
    db::rename_selected_internet_metadata(pool, current_relative_path, target_relative_path)
        .await?;

    let target_abs = library_root.join(target_relative_path);
    let file_name = target_abs
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let sidecar_path = sidecar::sidecar_relative_path(target_relative_path);
    let last_seen_at = unix_now();

    db::rename_managed_item_path(
        pool,
        current_relative_path,
        target_relative_path,
        &target_abs.display().to_string(),
        &file_name,
        sidecar_path.as_deref(),
        last_seen_at,
    )
    .await?;

    let metadata = tokio::fs::metadata(&target_abs).await?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let media_type = library_index::detect_media_type(&target_abs)
        .context("organized target is not a supported media file")?;
    let existing = db::fetch_managed_item(pool, target_relative_path)
        .await?
        .context("managed item missing after organize")?;

    sync_library_file(
        pool,
        library_root,
        target_relative_path,
        &target_abs.display().to_string(),
        &file_name,
        media_type,
        metadata.len(),
        modified_at,
        existing.library_id.as_deref(),
    )
    .await?;

    let updated_row = db::fetch_managed_item(pool, target_relative_path)
        .await?
        .context("managed item missing after organize sync")?;
    write_sidecar_from_row(library_root, &updated_row).await
}

async fn write_sidecar_from_row(library_root: &Path, row: &db::ManagedItemRow) -> Result<()> {
    let selected_metadata = row
        .selected_metadata_json
        .as_deref()
        .map(serde_json::from_str::<InternetMetadataMatch>)
        .transpose()?;
    let last_decision = row
        .last_decision_json
        .as_deref()
        .map(serde_json::from_str::<ProcessingDecision>)
        .transpose()?
        .as_ref()
        .map(sidecar::sidecar_decision_from_processing);

    let doc = ManagedItemSidecar {
        version: 1,
        relative_path: row.relative_path.clone(),
        media_type: row.media_type.clone(),
        library_id: row.library_id.clone(),
        managed_status: row.managed_status.clone(),
        size_bytes: row.size_bytes.max(0) as u64,
        modified_at: row.modified_at.max(0) as u64,
        first_seen_at: Some(row.first_seen_at.max(0) as u64),
        last_updated_at: row.last_seen_at.max(0) as u64,
        selected_metadata,
        last_decision,
    };

    sidecar::write_sidecar(library_root, &doc).await
}

fn intake_item_from_row(config: &AppConfig, row: db::ManagedItemRow) -> Result<IntakeManagedItem> {
    let missing_metadata = row_missing_metadata(&row);
    let missing_sidecar = row_missing_sidecar(&row);
    let organize_needed = managed_item_needs_organize(config, &row);
    let selected_metadata = row
        .selected_metadata_json
        .as_deref()
        .map(serde_json::from_str::<InternetMetadataMatch>)
        .transpose()?;
    let last_decision = row
        .last_decision_json
        .as_deref()
        .map(serde_json::from_str::<ProcessingDecision>)
        .transpose()?;

    Ok(IntakeManagedItem {
        relative_path: row.relative_path.clone(),
        file_path: row.file_path,
        file_name: row.file_name,
        media_type: row.media_type,
        size_bytes: row.size_bytes.max(0) as u64,
        modified_at: row.modified_at.max(0) as u64,
        library_id: row.library_id,
        managed_status: row.managed_status.clone(),
        has_sidecar: row.sidecar_path.is_some(),
        missing_metadata,
        missing_sidecar,
        organize_needed,
        selected_metadata,
        last_decision,
        group_key: None,
        group_label: None,
        group_kind: "file".into(),
        group_source: "file".into(),
        member_paths: vec![row.relative_path],
        member_count: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LibraryFolder;

    fn row(relative_path: &str, status: &str, selected_metadata_json: Option<&str>) -> db::ManagedItemRow {
        row_with_library(relative_path, "tv", status, selected_metadata_json)
    }

    fn row_with_library(
        relative_path: &str,
        library_id: &str,
        status: &str,
        selected_metadata_json: Option<&str>,
    ) -> db::ManagedItemRow {
        db::ManagedItemRow {
            relative_path: relative_path.into(),
            file_path: format!("/data/{relative_path}"),
            file_name: relative_path.rsplit('/').next().unwrap_or(relative_path).into(),
            media_type: "video".into(),
            size_bytes: 100,
            modified_at: 10,
            library_id: Some(library_id.into()),
            managed_status: status.into(),
            selected_metadata_json: selected_metadata_json.map(str::to_string),
            last_decision_json: None,
            sidecar_path: Some(format!("{relative_path}.json")),
            first_seen_at: 1,
            last_seen_at: 10,
            updated_at: "2026-03-11T00:00:00Z".into(),
        }
    }

    fn series_metadata(title: &str) -> String {
        serde_json::json!({
            "provider": "tvdb",
            "title": title,
            "year": 2024,
            "media_kind": "series",
            "imdb_id": null,
            "tvdb_id": 12345,
            "overview": null,
            "rating": null,
            "genres": [],
            "poster_url": null,
            "source_url": null
        })
        .to_string()
    }

    fn config() -> AppConfig {
        let mut config = AppConfig::default();
        config.libraries = vec![LibraryFolder {
            id: "tv".into(),
            name: "TV".into(),
            path: "tv".into(),
            media_type: "tv".into(),
        }];
        config
    }

    #[test]
    fn aggregates_tv_rows_into_one_backlog_item() {
        let config = config();
        let items = aggregate_backlog_items(
            &config,
            vec![
                row("tv/Fallout/Season 01/Fallout - S01E01.mkv", "UNPROCESSED", Some(&series_metadata("Fallout"))),
                row("tv/Fallout/Season 01/Fallout - S01E02.mkv", "REVIEWED", Some(&series_metadata("Fallout"))),
            ],
        )
        .expect("grouped backlog items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].group_kind, "tv_show");
        assert_eq!(items[0].group_label.as_deref(), Some("Fallout"));
        assert_eq!(items[0].member_count, 2);
        assert_eq!(items[0].managed_status, "UNPROCESSED");
    }

    #[test]
    fn keeps_movies_as_individual_items() {
        let config = config();
        let items = aggregate_backlog_items(
            &config,
            vec![row_with_library("movies/Dune (2021).mkv", "movies", "UNPROCESSED", None)],
        )
        .expect("backlog items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].group_kind, "file");
        assert_eq!(items[0].member_count, 1);
    }

    #[test]
    fn groups_tv_rows_by_library_id_even_without_path_prefix() {
        let config = config();
        let items = aggregate_backlog_items(
            &config,
            vec![
                row("Fallout/Season 01/Fallout - S01E01.mkv", "UNPROCESSED", None),
                row("Fallout/Season 01/Fallout - S01E02.mkv", "UNPROCESSED", None),
            ],
        )
        .expect("grouped backlog items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].group_kind, "tv_show");
        assert_eq!(items[0].group_label.as_deref(), Some("Fallout"));
        assert_eq!(items[0].member_count, 2);
    }

    #[test]
    fn groups_tv_rows_by_path_heuristic_without_library_config() {
        let config = AppConfig::default();
        let items = aggregate_backlog_items(
            &config,
            vec![
                row_with_library("tv/Will Trent/Season 04/Will Trent - S04E02.mkv", "unknown", "UNPROCESSED", None),
                row_with_library("tv/Will Trent/Season 04/Will Trent - S04E03.mkv", "unknown", "UNPROCESSED", None),
            ],
        )
        .expect("heuristic grouped backlog items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].group_kind, "tv_show");
        assert_eq!(items[0].group_label.as_deref(), Some("Will Trent"));
        assert_eq!(items[0].member_count, 2);
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}
