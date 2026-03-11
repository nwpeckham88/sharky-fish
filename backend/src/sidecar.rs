use crate::internet_metadata::InternetMetadataMatch;
use crate::messages::ProcessingDecision;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SIDECAR_SUFFIX: &str = ".sharky.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarDecision {
    pub arguments: Vec<String>,
    pub requires_two_pass: bool,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedItemSidecar {
    pub version: u32,
    pub relative_path: String,
    pub media_type: String,
    pub library_id: Option<String>,
    pub managed_status: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub first_seen_at: Option<u64>,
    pub last_updated_at: u64,
    pub selected_metadata: Option<InternetMetadataMatch>,
    pub last_decision: Option<SidecarDecision>,
}

pub fn sidecar_relative_path(relative_path: &str) -> Option<String> {
    let path = Path::new(relative_path);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path.file_stem()?.to_str()?;
    let file_name = format!("{}{}", stem, SIDECAR_SUFFIX);
    let sidecar = if parent.as_os_str().is_empty() {
        PathBuf::from(file_name)
    } else {
        parent.join(file_name)
    };
    Some(sidecar.to_string_lossy().replace('\\', "/"))
}

pub fn sidecar_absolute_path(library_root: &Path, relative_path: &str) -> Option<PathBuf> {
    Some(library_root.join(sidecar_relative_path(relative_path)?))
}

pub async fn read_sidecar(
    library_root: &Path,
    relative_path: &str,
) -> Result<Option<ManagedItemSidecar>> {
    let Some(sidecar_path) = sidecar_absolute_path(library_root, relative_path) else {
        return Ok(None);
    };

    if !tokio::fs::try_exists(&sidecar_path).await? {
        return Ok(None);
    }

    let raw = tokio::fs::read_to_string(&sidecar_path).await?;
    let parsed = serde_json::from_str::<ManagedItemSidecar>(&raw)?;
    Ok(Some(parsed))
}

pub async fn write_sidecar(library_root: &Path, sidecar: &ManagedItemSidecar) -> Result<()> {
    let Some(sidecar_path) = sidecar_absolute_path(library_root, &sidecar.relative_path) else {
        anyhow::bail!("unable to build sidecar path");
    };

    if let Some(parent) = sidecar_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let raw = serde_json::to_vec_pretty(sidecar)?;
    tokio::fs::write(sidecar_path, raw).await?;
    Ok(())
}

pub fn sidecar_decision_from_processing(decision: &ProcessingDecision) -> SidecarDecision {
    SidecarDecision {
        arguments: decision.arguments.clone(),
        requires_two_pass: decision.requires_two_pass,
        rationale: decision.rationale.clone(),
    }
}

pub fn processing_decision_from_sidecar(sidecar: &SidecarDecision) -> ProcessingDecision {
    ProcessingDecision {
        job_id: 0,
        arguments: sidecar.arguments.clone(),
        requires_two_pass: sidecar.requires_two_pass,
        rationale: sidecar.rationale.clone(),
    }
}
