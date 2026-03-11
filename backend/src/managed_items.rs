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
    pub selected_metadata: Option<InternetMetadataMatch>,
    pub last_decision: Option<ProcessingDecision>,
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

    let sidecar_path = sidecar::sidecar_relative_path(relative_path);

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
    limit: i64,
    offset: i64,
) -> Result<Vec<IntakeManagedItem>> {
    let rows = db::list_unprocessed_managed_items(pool, limit, offset).await?;
    rows.into_iter().map(intake_item_from_row).collect()
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
    let rows = if needs_attention_only || organize_needed_only {
        let all_rows =
            db::list_managed_items_filtered(pool, None, false, false, false, i64::MAX, 0).await?;

        all_rows
            .into_iter()
            .filter(|row| {
                let organize_needed = managed_item_needs_organize(config, row);
                if organize_needed_only {
                    return organize_needed;
                }
                base_needs_attention(row) || organize_needed
            })
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect()
    } else {
        db::list_managed_items_filtered(
            pool,
            managed_status,
            missing_metadata_only,
            missing_sidecar_only,
            needs_attention_only,
            limit,
            offset,
        )
        .await?
    };
    rows.into_iter().map(intake_item_from_row).collect()
}

pub async fn summarize(pool: &SqlitePool, config: &AppConfig) -> Result<BacklogSummary> {
    let row = db::summarize_managed_items(pool).await?;
    let all_rows =
        db::list_managed_items_filtered(pool, None, false, false, false, i64::MAX, 0).await?;
    let organize_needed_count = all_rows
        .iter()
        .filter(|item| managed_item_needs_organize(config, item))
        .count() as u64;
    let needs_attention_count = all_rows
        .iter()
        .filter(|item| base_needs_attention(item) || managed_item_needs_organize(config, item))
        .count() as u64;

    Ok(BacklogSummary {
        total_items: row.total_items.max(0) as u64,
        needs_attention_count,
        unprocessed_count: row.unprocessed_count.max(0) as u64,
        reviewed_count: row.reviewed_count.max(0) as u64,
        kept_original_count: row.kept_original_count.max(0) as u64,
        awaiting_approval_count: row.awaiting_approval_count.max(0) as u64,
        approved_count: row.approved_count.max(0) as u64,
        processed_count: row.processed_count.max(0) as u64,
        failed_count: row.failed_count.max(0) as u64,
        missing_metadata_count: row.missing_metadata_count.max(0) as u64,
        missing_sidecar_count: row.missing_sidecar_count.max(0) as u64,
        organize_needed_count,
    })
}

fn base_needs_attention(row: &db::ManagedItemRow) -> bool {
    row.managed_status == "UNPROCESSED"
        || row.managed_status == "FAILED"
        || row.managed_status == "AWAITING_APPROVAL"
        || (row.selected_metadata_json.is_none()
            && row.managed_status != "KEPT_ORIGINAL"
            && row.managed_status != "PROCESSED")
        || row.sidecar_path.is_none()
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
    })
}

fn derive_tv_group_from_library(
    config: &AppConfig,
    relative_path: &str,
    library_id: Option<&str>,
) -> Option<JobGroup> {
    let library = if let Some(id) = library_id {
        config.libraries.iter().find(|candidate| candidate.id == id)
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
    let stripped = strip_library_prefix(relative_path, &library.path)?;
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

fn intake_item_from_row(row: db::ManagedItemRow) -> Result<IntakeManagedItem> {
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
        relative_path: row.relative_path,
        file_path: row.file_path,
        file_name: row.file_name,
        media_type: row.media_type,
        size_bytes: row.size_bytes.max(0) as u64,
        modified_at: row.modified_at.max(0) as u64,
        library_id: row.library_id,
        managed_status: row.managed_status,
        has_sidecar: row.sidecar_path.is_some(),
        selected_metadata,
        last_decision,
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}
