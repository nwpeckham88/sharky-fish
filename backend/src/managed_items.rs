use crate::db;
use crate::internet_metadata::InternetMetadataMatch;
use crate::messages::ProcessingDecision;
use crate::sidecar::{self, ManagedItemSidecar};

use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
        None => existing.as_ref().and_then(|value| value.selected_metadata_json.clone()),
    };

    let last_decision_json = match sidecar_doc.as_ref() {
        Some(value) => value
            .last_decision
            .as_ref()
            .map(|decision| serde_json::to_string(&sidecar::processing_decision_from_sidecar(decision)))
            .transpose()?,
        None => existing.as_ref().and_then(|value| value.last_decision_json.clone()),
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

pub async fn list_unprocessed(pool: &SqlitePool, limit: i64, offset: i64) -> Result<Vec<IntakeManagedItem>> {
    let rows = db::list_unprocessed_managed_items(pool, limit, offset).await?;
    rows.into_iter().map(intake_item_from_row).collect()
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