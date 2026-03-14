use sqlx::{FromRow, SqlitePool};
use crate::filesystem_audit::FileSystemFacts;
use crate::internet_metadata::InternetMetadataMatch;
use crate::messages::{MediaProbe};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ItemPlan {
    pub id: i64,
    pub relative_path: String,
    pub status: String,
    pub current_revision_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ItemPlanRevision {
    pub id: i64,
    pub item_plan_id: i64,
    pub revision_number: i64,
    pub source: String,
    pub local_facts_json: String,
    pub ai_intake_json: Option<String>,
    pub metadata_resolution_json: Option<String>,
    pub organization_json: Option<String>,
    pub processing_json: Option<String>,
    pub audio_strategy_json: Option<String>,
    pub recommendation_json: String,
    pub followups_json: String,
    pub warnings_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ItemPlanMessage {
    pub id: i64,
    pub item_plan_id: i64,
    pub revision_id: Option<i64>,
    pub role: String,
    pub message_text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ItemPlanAcceptance {
    pub id: i64,
    pub item_plan_id: i64,
    pub accepted_revision_id: i64,
    pub accepted_metadata_json: Option<String>,
    pub accepted_processing_json: Option<String>,
    pub accepted_audio_strategy_json: Option<String>,
    pub accepted_execution_mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAudioPreference {
    pub id: i64,
    pub scope_type: String,
    pub scope_key: String,
    pub default_audio_track_policy: String,
    pub normalization_mode: String,
    pub night_listening_layout: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ItemPlanStatus {
    Draft,
    PendingOperator,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingAction {
    KeepAsIs,
    Organize,
    Process,
    OrganizeAndProcess,
    ReSource,
    NeedsOperatorInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerAudioStrategy {
   pub mode: AudioNormalizationMode,
   pub night_listening_layout: Option<NightListeningLayout>,
   pub default_track_policy: DefaultAudioTrackPolicy,
   pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioNormalizationMode {
   Disabled,
   NormalizeAll,
   NormalizePrimaryAndAlternate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NightListeningLayout {
   Stereo,
   TwoPointOne,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DefaultAudioTrackPolicy {
   PreserveOriginalDefault,
   PreferNightListeningDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerLocalFacts {
    pub relative_path: String,
    pub absolute_path: String,
    pub fs_facts: FileSystemFacts,
    pub probe: Option<MediaProbe>,
    pub selected_metadata: Option<InternetMetadataMatch>,
    pub existing_job_id: Option<i64>,
}

use anyhow::Result;

pub async fn gather_local_facts(pool: &SqlitePool, relative_path: &str) -> Result<PlannerLocalFacts> {
    // Stub implementation
    anyhow::bail!("gather_local_facts not yet implemented")
}

pub async fn run_ai_intake() -> Result<()> {
    // Stub implementation
    anyhow::bail!("run_ai_intake not yet implemented")
}

pub async fn resolve_metadata_candidates() -> Result<()> {
    // Stub implementation
    anyhow::bail!("resolve_metadata_candidates not yet implemented")
}

pub async fn rank_metadata_candidates() -> Result<()> {
    // Stub implementation
    anyhow::bail!("rank_metadata_candidates not yet implemented")
}

pub async fn build_processing_proposal() -> Result<()> {
    // Stub implementation
    anyhow::bail!("build_processing_proposal not yet implemented")
}

pub async fn apply_followup() -> Result<()> {
    // Stub implementation
    anyhow::bail!("apply_followup not yet implemented")
}



pub async fn get_plan_for_item(pool: &sqlx::SqlitePool, relative_path: &str) -> anyhow::Result<Option<ItemPlan>> {
    let plan = sqlx::query_as::<_, ItemPlan>(
        "SELECT id, relative_path, status, current_revision_id, created_at, updated_at FROM item_plans WHERE relative_path = ?"
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;
    Ok(plan)
}

pub async fn get_plan_history(pool: &sqlx::SqlitePool, plan_id: i64) -> anyhow::Result<Vec<ItemPlanRevision>> {
    let revisions = sqlx::query_as::<_, ItemPlanRevision>(
        "SELECT id, item_plan_id, revision_number, source, local_facts_json, ai_intake_json, metadata_resolution_json, organization_json, processing_json, audio_strategy_json, recommendation_json, followups_json, warnings_json, created_at FROM item_plan_revisions WHERE item_plan_id = ? ORDER BY revision_number DESC"
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(revisions)
}
