use crate::config::AppConfig;
use crate::filesystem_audit::FileSystemFacts;
use crate::internet_metadata::{self, InternetMetadataMatch, InternetMetadataResponse};
use crate::messages::MediaProbe;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, SqlitePool};

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
pub struct ItemPlanEnvelope {
    pub plan: ItemPlan,
    pub latest_revision: Option<ItemPlanRevision>,
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
    pub library_id: Option<String>,
    pub fs_facts: FileSystemFacts,
    pub probe: Option<MediaProbe>,
    pub selected_metadata: Option<InternetMetadataMatch>,
    pub existing_job_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct AudioPreferenceRow {
    scope_type: String,
    scope_key: String,
    default_audio_track_policy: String,
    normalization_mode: String,
    night_listening_layout: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerAiIntake {
    pub media_kind_guess: String,
    pub title_guess: Option<String>,
    pub year_guess: Option<u16>,
    pub search_queries: Vec<String>,
    pub confidence: f64,
    pub ambiguities: Vec<String>,
    pub operator_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerMetadataResolution {
    pub query_attempted: String,
    pub candidate_count: usize,
    pub selected_candidate_index: Option<usize>,
    pub selected_candidate: Option<InternetMetadataMatch>,
    pub provider_used: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerProcessingProposal {
    pub action: ProcessingAction,
    pub ffmpeg_arguments: Vec<String>,
    pub requires_two_pass: bool,
    pub rationale: String,
    pub risk_notes: Vec<String>,
    pub better_source_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerOrganizationProposal {
    pub organize_needed: bool,
    pub target_hint: Option<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerRecommendation {
    pub category: String,
    pub reason: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
struct PlannerDerivedData {
    ai_intake: PlannerAiIntake,
    metadata_resolution: PlannerMetadataResolution,
    organization: PlannerOrganizationProposal,
    processing: PlannerProcessingProposal,
    audio_strategy: PlannerAudioStrategy,
    recommendation: PlannerRecommendation,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
struct LibraryIndexFactsRow {
    file_path: String,
    library_id: Option<String>,
    device_id: i64,
    inode: i64,
    link_count: i64,
    size_bytes: i64,
    modified_at: i64,
}

#[derive(Debug, Clone, FromRow)]
struct ProbeRow {
    probe_json: String,
}

#[derive(Debug, Clone, FromRow)]
struct SelectedMetadataRow {
    provider: String,
    title: String,
    year: Option<i64>,
    media_kind: String,
    imdb_id: Option<String>,
    tvdb_id: Option<i64>,
    overview: Option<String>,
    rating: Option<f64>,
    genres_json: String,
    poster_url: Option<String>,
    backdrop_url: Option<String>,
    source_url: Option<String>,
}

pub async fn gather_local_facts(pool: &SqlitePool, relative_path: &str) -> Result<PlannerLocalFacts> {
    let index_row = sqlx::query_as::<_, LibraryIndexFactsRow>(
        "SELECT file_path, library_id, device_id, inode, link_count, size_bytes, modified_at
         FROM library_index
         WHERE relative_path = ?
         LIMIT 1",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;

    let (absolute_path, library_id, fs_facts) = if let Some(row) = index_row {
        let link_count = row.link_count.max(1) as u64;
        (
            row.file_path,
            row.library_id,
            FileSystemFacts {
                device_id: row.device_id.max(0) as u64,
                inode: row.inode.max(0) as u64,
                link_count,
                size_bytes: row.size_bytes.max(0) as u64,
                modified_at: row.modified_at.max(0) as u64,
                is_hard_linked: link_count > 1,
            },
        )
    } else {
        (relative_path.to_string(), None, FileSystemFacts::default())
    };

    let probe = sqlx::query_as::<_, ProbeRow>(
        "SELECT probe_json
         FROM media_metadata
         WHERE file_path = ?
         LIMIT 1",
    )
    .bind(&absolute_path)
    .fetch_optional(pool)
    .await?
    .and_then(|row| serde_json::from_str::<MediaProbe>(&row.probe_json).ok());

    let selected_metadata = sqlx::query_as::<_, SelectedMetadataRow>(
        "SELECT provider, title, year, media_kind, imdb_id, tvdb_id, overview,
                rating, genres_json, poster_url, backdrop_url, source_url
         FROM selected_internet_metadata
         WHERE relative_path = ?
         LIMIT 1",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?
    .map(|row| {
        let genres = serde_json::from_str::<Vec<String>>(&row.genres_json).unwrap_or_default();
        InternetMetadataMatch {
            provider: row.provider,
            title: row.title,
            year: row.year.and_then(|value| u16::try_from(value).ok()),
            media_kind: row.media_kind,
            imdb_id: row.imdb_id,
            tvdb_id: row.tvdb_id.and_then(|value| u64::try_from(value).ok()),
            overview: row.overview,
            rating: row.rating,
            genres,
            poster_url: row.poster_url,
            backdrop_url: row.backdrop_url,
            source_url: row.source_url,
        }
    });

    let existing_job_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM jobs WHERE file_path = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;

    Ok(PlannerLocalFacts {
        relative_path: relative_path.to_string(),
        absolute_path,
        library_id,
        fs_facts,
        probe,
        selected_metadata,
        existing_job_id,
    })
}

fn infer_media_kind_from_path(relative_path: &str) -> String {
    let lowered = relative_path.to_ascii_lowercase();
    if lowered.contains("s01")
        || lowered.contains("e01")
        || lowered.contains("season")
        || lowered.contains("episodes")
    {
        "series".to_string()
    } else {
        "movie".to_string()
    }
}

fn stem_title_guess(relative_path: &str) -> String {
    let stem = std::path::Path::new(relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(relative_path);
    stem
        .replace('.', " ")
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn year_guess_from_text(value: &str) -> Option<u16> {
    for token in value.split(|ch: char| !ch.is_ascii_digit()) {
        if token.len() == 4 {
            if let Ok(year) = token.parse::<u16>() {
                if (1900..=2100).contains(&year) {
                    return Some(year);
                }
            }
        }
    }
    None
}

#[derive(Debug, Deserialize)]
struct LlmPlannerIntakeOutput {
    media_kind_guess: Option<String>,
    title_guess: Option<String>,
    year_guess: Option<u16>,
    search_queries: Option<Vec<String>>,
    confidence: Option<f64>,
    ambiguities: Option<Vec<String>>,
    operator_questions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct LlmPlannerProcessingOutput {
    action: Option<String>,
    rationale: Option<String>,
    risk_notes: Option<Vec<String>>,
    better_source_recommended: Option<bool>,
    better_source_reason: Option<String>,
    audio_mode: Option<String>,
    night_listening_layout: Option<String>,
    default_track_policy: Option<String>,
    audio_rationale: Option<String>,
}

fn fallback_ai_intake(relative_path: &str, followups: &[String]) -> PlannerAiIntake {
    let media_kind_guess = infer_media_kind_from_path(relative_path);
    let title_guess = Some(stem_title_guess(relative_path));
    let year_guess = title_guess.as_deref().and_then(year_guess_from_text);
    let mut search_queries = Vec::new();
    if let Some(title) = title_guess.as_ref() {
        search_queries.push(title.clone());
        if let Some(year) = year_guess {
            search_queries.push(format!("{title} {year}"));
        }
    }

    let mut ambiguities = Vec::new();
    if followups.is_empty() {
        ambiguities.push("No operator guidance provided yet".to_string());
    }

    PlannerAiIntake {
        media_kind_guess,
        title_guess,
        year_guess,
        search_queries,
        confidence: 0.55,
        ambiguities,
        operator_questions: vec![
            "Confirm exact title/year if this is a remake or alternate cut.".to_string(),
        ],
    }
}

fn planner_system_prompt() -> &'static str {
    "You are the Sharky Fish planner assistant. Return strict JSON only and never include markdown fences or explanatory prose. Do not invent provider results as facts."
}

fn build_openai_request(
    llm_config: &crate::config::LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> (String, serde_json::Value) {
    let url = format!("{}/chat/completions", llm_config.base_url);
    let body = serde_json::json!({
        "model": llm_config.model,
        "temperature": temperature,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ]
    });
    (url, body)
}

fn build_google_request(
    llm_config: &crate::config::LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> (String, serde_json::Value) {
    let url = format!(
        "{}/models/{}:generateContent",
        llm_config.base_url.trim_end_matches('/'),
        llm_config.model
    );
    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [
                { "text": system_prompt }
            ]
        },
        "contents": [
            {
                "role": "user",
                "parts": [
                    { "text": user_prompt }
                ]
            }
        ],
        "generationConfig": {
            "temperature": temperature,
            "responseMimeType": "application/json"
        }
    });
    (url, body)
}

fn build_ollama_request(
    llm_config: &crate::config::LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> (String, serde_json::Value) {
    let url = format!("{}/api/chat", llm_config.base_url);
    let body = serde_json::json!({
        "model": llm_config.model,
        "stream": false,
        "format": "json",
        "options": { "temperature": temperature },
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ]
    });
    (url, body)
}

fn extract_llm_content(json: &serde_json::Value) -> Result<&str> {
    json.pointer("/candidates/0/content/parts/0/text")
        .or_else(|| json.pointer("/choices/0/message/content"))
        .or_else(|| json.pointer("/message/content"))
        .and_then(|value| value.as_str())
        .context("missing content in LLM response")
}

async fn call_llm_json<T: for<'de> Deserialize<'de>>(
    config: &AppConfig,
    user_prompt: &str,
    temperature: f64,
) -> Result<T> {
    let client = reqwest::Client::new();
    let llm_config = &config.llm;

    let (url, body) = match llm_config.provider.as_str() {
        "google" => build_google_request(llm_config, planner_system_prompt(), user_prompt, temperature),
        "openai" => build_openai_request(llm_config, planner_system_prompt(), user_prompt, temperature),
        "ollama" => build_ollama_request(llm_config, planner_system_prompt(), user_prompt, temperature),
        other => anyhow::bail!("unsupported LLM provider: {other}"),
    };

    let mut req = client.post(&url).json(&body);
    if llm_config.provider == "google" {
        if let Some(key) = &llm_config.api_key {
            req = req.header("x-goog-api-key", key);
        }
    } else if let Some(key) = &llm_config.api_key {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await.context("LLM HTTP request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("LLM API returned {status}: {text}");
    }

    let json: serde_json::Value = resp.json().await?;
    let content = extract_llm_content(&json)?;
    let parsed = serde_json::from_str::<T>(content)
        .context("failed to parse planner LLM JSON output")?;
    Ok(parsed)
}

fn normalize_media_kind(value: Option<String>, fallback: &str) -> String {
    match value
        .unwrap_or_else(|| fallback.to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "tv" | "show" | "series" | "episode" => "series".to_string(),
        "movie" | "film" => "movie".to_string(),
        _ => fallback.to_string(),
    }
}

pub async fn run_ai_intake(
    config: &AppConfig,
    local_facts: &PlannerLocalFacts,
    followups: &[String],
) -> Result<PlannerAiIntake> {
    let fallback = fallback_ai_intake(&local_facts.relative_path, followups);
    let probe = local_facts
        .probe
        .as_ref()
        .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "null".to_string());

    let followup_block = if followups.is_empty() {
        "None".to_string()
    } else {
        followups.join("\n- ")
    };

    let prompt = format!(
        "Create an intake interpretation for one library item.\n\
         Return strict JSON with fields: media_kind_guess, title_guess, year_guess, search_queries, confidence, ambiguities, operator_questions.\n\
         Rules:\n\
         - media_kind_guess must be movie or series.\n\
         - confidence must be 0.0 to 1.0.\n\
         - search_queries must prioritize realistic metadata lookup terms.\n\
         - Do not invent external IDs.\n\n\
         Relative path: {}\n\
         Existing selected metadata title: {}\n\
         Operator followups:\n- {}\n\
         Probe JSON:\n{}",
        local_facts.relative_path,
        local_facts
            .selected_metadata
            .as_ref()
            .map(|value| value.title.as_str())
            .unwrap_or("none"),
        followup_block,
        probe,
    );

    match call_llm_json::<LlmPlannerIntakeOutput>(config, &prompt, 0.2).await {
        Ok(parsed) => {
            let mut search_queries = parsed.search_queries.unwrap_or_default();
            search_queries.retain(|value| !value.trim().is_empty());
            if search_queries.is_empty() {
                search_queries = fallback.search_queries.clone();
            }

            Ok(PlannerAiIntake {
                media_kind_guess: normalize_media_kind(parsed.media_kind_guess, &fallback.media_kind_guess),
                title_guess: parsed.title_guess.or(fallback.title_guess),
                year_guess: parsed.year_guess.or(fallback.year_guess),
                search_queries,
                confidence: parsed.confidence.unwrap_or(fallback.confidence).clamp(0.0, 1.0),
                ambiguities: parsed.ambiguities.unwrap_or(fallback.ambiguities),
                operator_questions: parsed
                    .operator_questions
                    .unwrap_or(fallback.operator_questions),
            })
        }
        Err(error) => {
            let mut degraded = fallback;
            degraded
                .ambiguities
                .push(format!("LLM intake unavailable; fallback used: {error}"));
            Ok(degraded)
        }
    }
}

fn score_candidate(candidate: &InternetMetadataMatch, ai: &PlannerAiIntake) -> i64 {
    let mut score = 0_i64;
    if candidate.media_kind.eq_ignore_ascii_case(&ai.media_kind_guess) {
        score += 50;
    }
    if let (Some(title_guess), true) = (
        ai.title_guess.as_ref(),
        candidate
            .title
            .to_ascii_lowercase()
            .contains(&ai.title_guess.as_deref().unwrap_or_default().to_ascii_lowercase()),
    ) {
        if !title_guess.is_empty() {
            score += 40;
        }
    }
    if let (Some(candidate_year), Some(guess_year)) = (candidate.year, ai.year_guess) {
        let delta = (candidate_year as i32 - guess_year as i32).abs();
        score += i64::from(20_i32.saturating_sub(delta));
    }
    score
}

fn metadata_match_dedupe_key(candidate: &InternetMetadataMatch) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        candidate.provider,
        candidate.imdb_id.clone().unwrap_or_default(),
        candidate
            .tvdb_id
            .map(|value| value.to_string())
            .unwrap_or_default(),
        candidate.title.to_ascii_lowercase(),
        candidate
            .year
            .map(|value| value.to_string())
            .unwrap_or_default(),
    )
}

fn normalize_planner_query(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn resolve_metadata_candidates(
    config: &AppConfig,
    relative_path: &str,
    ai_intake: &PlannerAiIntake,
) -> Result<PlannerMetadataResolution> {
    let mut queries = Vec::<String>::new();
    for query in ai_intake.search_queries.iter().take(4) {
        if let Some(normalized) = normalize_planner_query(query) {
            queries.push(normalized);
        }
    }

    // Preserve path-derived candidates even when AI suggestions are present.
    let path_fallback_query = relative_path.to_string();
    if queries.is_empty() {
        queries.push(path_fallback_query.clone());
    } else {
        queries.push(path_fallback_query.clone());
    }

    let mut seen_queries = std::collections::HashSet::<String>::new();
    queries.retain(|query| seen_queries.insert(query.to_ascii_lowercase()));

    let mut aggregate_matches: Vec<InternetMetadataMatch> = Vec::new();
    let mut provider_used: Option<String> = None;
    let mut warnings: Vec<String> = Vec::new();
    let mut attempted_queries: Vec<String> = Vec::new();

    for query in &queries {
        attempted_queries.push(query.clone());

        let result: Result<InternetMetadataResponse> = if query == &path_fallback_query {
            internet_metadata::lookup_for_library_path(config, relative_path).await
        } else {
            internet_metadata::lookup_for_library_path_with_query(config, relative_path, Some(query))
                .await
        };

        match result {
            Ok(response) => {
                if provider_used.is_none() {
                    provider_used = response.provider_used.clone();
                }
                warnings.extend(
                    response
                        .warnings
                        .into_iter()
                        .map(|warning| format!("[{query}] {warning}")),
                );
                aggregate_matches.extend(response.matches);
            }
            Err(error) => {
                warnings.push(format!("[{query}] metadata lookup failed: {error}"));
            }
        }
    }

    let mut seen_matches = std::collections::HashSet::<String>::new();
    aggregate_matches.retain(|candidate| seen_matches.insert(metadata_match_dedupe_key(candidate)));

    let mut best_index: Option<usize> = None;
    let mut best_score = i64::MIN;
    for (idx, candidate) in aggregate_matches.iter().enumerate() {
        let score = score_candidate(candidate, ai_intake);
        if score > best_score {
            best_score = score;
            best_index = Some(idx);
        }
    }

    let selected_candidate = best_index.and_then(|idx| aggregate_matches.get(idx).cloned());

    Ok(PlannerMetadataResolution {
        query_attempted: attempted_queries.join(" | "),
        candidate_count: aggregate_matches.len(),
        selected_candidate_index: best_index,
        selected_candidate,
        provider_used,
        warnings,
    })
}

fn validate_ffmpeg_arguments(arguments: &[String]) -> Result<()> {
    if arguments.is_empty() {
        anyhow::bail!("FFmpeg argument list is empty")
    }

    let mut has_input_placeholder = false;
    let mut has_output_placeholder = false;
    let blocked_flags = ["-f", "-progress", "-report", "-y", "-n"];

    for (index, argument) in arguments.iter().enumerate() {
        if argument == "input.mkv" {
            has_input_placeholder = true;
        }
        if argument == "output.mp4" || argument == "output.m4a" {
            has_output_placeholder = true;
        }

        if blocked_flags.contains(&argument.as_str()) {
            anyhow::bail!("argument `{}` is managed by the backend and not allowed", argument)
        }

        // Prevent accidental absolute outputs or shell injection style payloads.
        if index == arguments.len() - 1
            && (argument.starts_with('/') || argument.contains(';') || argument.contains('|'))
        {
            anyhow::bail!("output argument must be an output placeholder")
        }
    }

    if !has_input_placeholder {
        anyhow::bail!("missing required input placeholder `input.mkv`")
    }
    if !has_output_placeholder {
        anyhow::bail!("missing required output placeholder (`output.mp4` or `output.m4a`)")
    }

    Ok(())
}

fn parse_audio_mode(value: &str) -> AudioNormalizationMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "normalize_all" | "normalize_all_tracks" => AudioNormalizationMode::NormalizeAll,
        "normalize_primary_and_alternate" => AudioNormalizationMode::NormalizePrimaryAndAlternate,
        _ => AudioNormalizationMode::Disabled,
    }
}

fn parse_night_layout(value: &str) -> Option<NightListeningLayout> {
    match value.trim().to_ascii_lowercase().as_str() {
        "two_point_one" | "2.1" => Some(NightListeningLayout::TwoPointOne),
        "stereo" => Some(NightListeningLayout::Stereo),
        _ => None,
    }
}

fn parse_default_track_policy(value: &str) -> DefaultAudioTrackPolicy {
    match value.trim().to_ascii_lowercase().as_str() {
        "prefer_night_listening_default" => DefaultAudioTrackPolicy::PreferNightListeningDefault,
        _ => DefaultAudioTrackPolicy::PreserveOriginalDefault,
    }
}

fn is_valid_execution_mode(value: &str) -> bool {
    matches!(value, "full_plan" | "organize_only" | "process_only")
}

fn movie_scope_key(candidate: &InternetMetadataMatch) -> String {
    if let Some(imdb_id) = candidate.imdb_id.as_ref().filter(|value| !value.trim().is_empty()) {
        return imdb_id.trim().to_string();
    }

    let year = candidate
        .year
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!("{}|{}", candidate.title.trim().to_ascii_lowercase(), year)
}

fn planner_audio_scope_candidates(
    local_facts: &PlannerLocalFacts,
    metadata_resolution: &PlannerMetadataResolution,
) -> Vec<(String, String)> {
    let mut scopes = Vec::<(String, String)>::new();

    scopes.push(("item".to_string(), local_facts.relative_path.clone()));

    if let Some(candidate) = metadata_resolution.selected_candidate.as_ref() {
        if candidate.media_kind.eq_ignore_ascii_case("series") {
            scopes.push((
                "series".to_string(),
                candidate.title.trim().to_ascii_lowercase(),
            ));
        } else {
            scopes.push(("movie".to_string(), movie_scope_key(candidate)));
        }
    }

    if let Some(library_id) = local_facts
        .library_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        scopes.push(("library".to_string(), library_id.trim().to_string()));
    }

    scopes.push(("library".to_string(), "default".to_string()));
    scopes
}

async fn load_audio_preference_for_item(
    pool: &SqlitePool,
    local_facts: &PlannerLocalFacts,
    metadata_resolution: &PlannerMetadataResolution,
) -> Result<Option<AudioPreferenceRow>> {
    for (scope_type, scope_key) in planner_audio_scope_candidates(local_facts, metadata_resolution) {
        let row = sqlx::query_as::<_, AudioPreferenceRow>(
            "SELECT scope_type, scope_key, default_audio_track_policy,
                    normalization_mode, night_listening_layout
             FROM item_audio_preferences
             WHERE lower(scope_type) = lower(?)
               AND lower(scope_key) = lower(?)
             ORDER BY updated_at DESC, id DESC
             LIMIT 1",
        )
        .bind(&scope_type)
        .bind(&scope_key)
        .fetch_optional(pool)
        .await?;

        if row.is_some() {
            return Ok(row);
        }
    }

    Ok(None)
}

fn re_source_reason(probe: &MediaProbe) -> Option<String> {
    let video = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video")?;
    let codec = video.codec_name.trim().to_ascii_lowercase();
    let width = video.width.unwrap_or(0);
    let height = video.height.unwrap_or(0);
    let bitrate_mbps = video.bit_rate.map(|value| value as f64 / 1_000_000.0)?;

    let is_uhd = width >= 3800 || height >= 2100;
    let is_avc = matches!(codec.as_str(), "h264" | "avc" | "avc1");

    if is_uhd && is_avc && bitrate_mbps <= 16.0 {
        return Some(format!(
            "This 4K AVC stream is about {:.1} Mbps, so re-source may be safer than another lossy transcode.",
            bitrate_mbps
        ));
    }

    None
}

pub async fn build_processing_proposal(
    pool: &SqlitePool,
    config: &AppConfig,
    relative_path: &str,
    local_facts: &PlannerLocalFacts,
    metadata_resolution: &PlannerMetadataResolution,
    followups: &[String],
) -> Result<(PlannerProcessingProposal, PlannerAudioStrategy)> {
    let mut risk_notes = Vec::new();
    if local_facts.fs_facts.is_hard_linked {
        risk_notes.push(
            "Processing will create a new file and break current hard-link sharing.".to_string(),
        );
    }

    let mut better_source_reason = local_facts.probe.as_ref().and_then(re_source_reason);

    let fallback_action = if better_source_reason.is_some() {
        ProcessingAction::ReSource
    } else if metadata_resolution.selected_candidate.is_none() {
        ProcessingAction::NeedsOperatorInput
    } else {
        ProcessingAction::KeepAsIs
    };

    let fallback_rationale = match fallback_action {
        ProcessingAction::ReSource => {
            "Current source appears low quality for the content class; recommend re-source.".to_string()
        }
        ProcessingAction::NeedsOperatorInput => {
            "Metadata match is ambiguous; hold processing until operator confirms identity.".to_string()
        }
        _ => "No immediate processing action required in this revision.".to_string(),
    };

    let probe = local_facts
        .probe
        .as_ref()
        .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "null".to_string());
    let followup_block = if followups.is_empty() {
        "None".to_string()
    } else {
        followups.join("\n- ")
    };
    let selected_title = metadata_resolution
        .selected_candidate
        .as_ref()
        .map(|value| value.title.clone())
        .unwrap_or_else(|| "unresolved".to_string());

    let processing_prompt = format!(
        "Plan processing intent for a library item and return strict JSON with fields:\n\
         action, rationale, risk_notes, better_source_recommended, better_source_reason, audio_mode, night_listening_layout, default_track_policy, audio_rationale.\n\
         Allowed action values: keep_as_is, organize_only, process, organize_and_process, re_source, needs_operator_input.\n\
         Allowed audio_mode values: disabled, normalize_all, normalize_primary_and_alternate.\n\
         Allowed night_listening_layout values: stereo, two_point_one, null.\n\
         Allowed default_track_policy values: preserve_original_default, prefer_night_listening_default.\n\n\
         Relative path: {}\n\
         Selected metadata title: {}\n\
         Hard linked: {}\n\
         Operator followups:\n- {}\n\
         Probe JSON:\n{}",
        relative_path,
        selected_title,
        local_facts.fs_facts.is_hard_linked,
        followup_block,
        probe,
    );

    let mut action = fallback_action;
    let mut rationale = fallback_rationale;
    let mut llm_risk_notes = Vec::<String>::new();
    let mut audio_strategy = PlannerAudioStrategy {
        mode: AudioNormalizationMode::Disabled,
        night_listening_layout: Some(NightListeningLayout::Stereo),
        default_track_policy: DefaultAudioTrackPolicy::PreserveOriginalDefault,
        rationale: "Preserve original mix by default until title-specific guidance is provided."
            .to_string(),
    };

    match call_llm_json::<LlmPlannerProcessingOutput>(config, &processing_prompt, 0.2).await {
        Ok(parsed) => {
            action = match parsed
                .action
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "keep_as_is" | "keep" => ProcessingAction::KeepAsIs,
                "organize_only" | "organize" => ProcessingAction::Organize,
                "process" => ProcessingAction::Process,
                "organize_and_process" | "full_plan" => ProcessingAction::OrganizeAndProcess,
                "re_source" | "resource" => ProcessingAction::ReSource,
                "needs_operator_input" => ProcessingAction::NeedsOperatorInput,
                _ => action,
            };
            if let Some(value) = parsed.rationale.filter(|value| !value.trim().is_empty()) {
                rationale = value;
            }
            llm_risk_notes = parsed.risk_notes.unwrap_or_default();
            let mode = parse_audio_mode(&parsed.audio_mode.unwrap_or_default());
            let layout = parse_night_layout(&parsed.night_listening_layout.unwrap_or_default());
            let policy = parse_default_track_policy(&parsed.default_track_policy.unwrap_or_default());
            audio_strategy = PlannerAudioStrategy {
                mode,
                night_listening_layout: layout,
                default_track_policy: policy,
                rationale: parsed
                    .audio_rationale
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| {
                        "Preserve original mix by default until title-specific guidance is provided."
                            .to_string()
                    }),
            };

            if parsed.better_source_recommended.unwrap_or(false)
                && better_source_reason.is_none()
            {
                better_source_reason = parsed.better_source_reason;
            }
        }
        Err(error) => {
            risk_notes.push(format!(
                "Processing-stage LLM unavailable; fallback recommendation used: {error}"
            ));
        }
    }

    if let Some(preference) = load_audio_preference_for_item(pool, local_facts, metadata_resolution).await? {
        audio_strategy.mode = parse_audio_mode(&preference.normalization_mode);
        audio_strategy.night_listening_layout = parse_night_layout(&preference.night_listening_layout);
        audio_strategy.default_track_policy =
            parse_default_track_policy(&preference.default_audio_track_policy);
        audio_strategy.rationale = format!(
            "Applied saved audio preference ({}:{}). {}",
            preference.scope_type,
            preference.scope_key,
            audio_strategy.rationale
        );
    }

    let mut ffmpeg_arguments = Vec::new();
    let mut requires_two_pass = false;
    if matches!(action, ProcessingAction::Process | ProcessingAction::OrganizeAndProcess)
        && local_facts.probe.is_some()
    {
        if let Some(probe) = local_facts.probe.clone() {
            let identified = crate::messages::IdentifiedMedia {
                path: std::path::PathBuf::from(&local_facts.absolute_path),
                probe,
            };
            match crate::actors::brain::create_processing_decision(config, &identified).await {
                Ok(decision) => {
                    match validate_ffmpeg_arguments(&decision.arguments) {
                        Ok(()) => {
                            ffmpeg_arguments = decision.arguments;
                            requires_two_pass = decision.requires_two_pass;
                            if !decision.rationale.trim().is_empty() {
                                rationale = format!("{} | {}", rationale, decision.rationale);
                            }
                        }
                        Err(error) => {
                            risk_notes.push(format!(
                                "FFmpeg planning returned invalid arguments; downgraded to non-processing recommendation: {error}"
                            ));
                            action = if metadata_resolution.selected_candidate.is_none() {
                                ProcessingAction::NeedsOperatorInput
                            } else {
                                ProcessingAction::KeepAsIs
                            };
                        }
                    }
                }
                Err(error) => {
                    risk_notes.push(format!(
                        "FFmpeg planning call failed; kept non-processing plan: {error}"
                    ));
                    action = if metadata_resolution.selected_candidate.is_none() {
                        ProcessingAction::NeedsOperatorInput
                    } else {
                        ProcessingAction::KeepAsIs
                    };
                }
            }
        }
    }

    risk_notes.extend(llm_risk_notes);

    let processing = PlannerProcessingProposal {
        action,
        ffmpeg_arguments,
        requires_two_pass,
        rationale,
        risk_notes,
        better_source_reason,
    };

    Ok((processing, audio_strategy))
}

fn build_organization_proposal(
    relative_path: &str,
    metadata_resolution: &PlannerMetadataResolution,
) -> PlannerOrganizationProposal {
    let Some(selected) = metadata_resolution.selected_candidate.as_ref() else {
        return PlannerOrganizationProposal {
            organize_needed: false,
            target_hint: None,
            rationale: "Metadata is unresolved, so no deterministic organize target is suggested yet."
                .to_string(),
        };
    };

    let normalized_path = relative_path.to_ascii_lowercase();
    let normalized_title = selected.title.to_ascii_lowercase();
    let in_place = normalized_path.contains(&normalized_title);

    if in_place {
        PlannerOrganizationProposal {
            organize_needed: false,
            target_hint: None,
            rationale: "Path already appears aligned with selected metadata naming.".to_string(),
        }
    } else {
        let year_suffix = selected
            .year
            .map(|year| format!(" ({year})"))
            .unwrap_or_default();
        PlannerOrganizationProposal {
            organize_needed: true,
            target_hint: Some(format!("{}/{}{}", selected.title, selected.title, year_suffix)),
            rationale: "Selected metadata title does not appear in current path; organize is recommended."
                .to_string(),
        }
    }
}

fn build_recommendation(
    metadata_resolution: &PlannerMetadataResolution,
    processing: &PlannerProcessingProposal,
) -> PlannerRecommendation {
    if matches!(processing.action, ProcessingAction::ReSource) {
        return PlannerRecommendation {
            category: "re_source".to_string(),
            reason: processing
                .better_source_reason
                .clone()
                .unwrap_or_else(|| "Planner recommends better source".to_string()),
            confidence: 0.8,
        };
    }
    if metadata_resolution.selected_candidate.is_none() {
        return PlannerRecommendation {
            category: "needs_operator_input".to_string(),
            reason: "Metadata match is ambiguous.".to_string(),
            confidence: 0.45,
        };
    }
    PlannerRecommendation {
        category: "keep".to_string(),
        reason: "Metadata resolved and no mandatory processing flags were triggered.".to_string(),
        confidence: 0.7,
    }
}

async fn derive_plan_data(
    pool: &SqlitePool,
    config: &AppConfig,
    relative_path: &str,
    local_facts: &PlannerLocalFacts,
    followups: &[String],
) -> Result<PlannerDerivedData> {
    let ai_intake = run_ai_intake(config, local_facts, followups).await?;
    let metadata_resolution = resolve_metadata_candidates(config, relative_path, &ai_intake).await?;
    let organization = build_organization_proposal(relative_path, &metadata_resolution);
    let (processing, audio_strategy) = build_processing_proposal(
        pool,
        config,
        relative_path,
        local_facts,
        &metadata_resolution,
        followups,
    )
    .await?;
    let recommendation = build_recommendation(&metadata_resolution, &processing);

    let mut warnings = metadata_resolution.warnings.clone();
    warnings.extend(processing.risk_notes.iter().cloned());

    Ok(PlannerDerivedData {
        ai_intake,
        metadata_resolution,
        organization,
        processing,
        audio_strategy,
        recommendation,
        warnings,
    })
}

pub async fn get_plan_for_item(pool: &SqlitePool, relative_path: &str) -> Result<Option<ItemPlan>> {
    let plan = sqlx::query_as::<_, ItemPlan>(
        "SELECT id, relative_path, status, current_revision_id, created_at, updated_at
         FROM item_plans
         WHERE relative_path = ?
         ORDER BY updated_at DESC, id DESC
         LIMIT 1",
    )
    .bind(relative_path)
    .fetch_optional(pool)
    .await?;
    Ok(plan)
}

pub async fn get_plan_history(pool: &SqlitePool, plan_id: i64) -> Result<Vec<ItemPlanRevision>> {
    let revisions = sqlx::query_as::<_, ItemPlanRevision>(
        "SELECT id, item_plan_id, revision_number, source, local_facts_json,
                ai_intake_json, metadata_resolution_json, organization_json,
                processing_json, audio_strategy_json, recommendation_json,
                followups_json, warnings_json, created_at
         FROM item_plan_revisions
         WHERE item_plan_id = ?
         ORDER BY revision_number DESC",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(revisions)
}

pub async fn get_plan_messages(pool: &SqlitePool, plan_id: i64) -> Result<Vec<ItemPlanMessage>> {
    let messages = sqlx::query_as::<_, ItemPlanMessage>(
        "SELECT id, item_plan_id, revision_id, role, message_text, created_at
         FROM item_plan_messages
         WHERE item_plan_id = ?
         ORDER BY id ASC",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(messages)
}

pub async fn get_latest_revision_for_plan(
    pool: &SqlitePool,
    plan_id: i64,
) -> Result<Option<ItemPlanRevision>> {
    let revision = sqlx::query_as::<_, ItemPlanRevision>(
        "SELECT id, item_plan_id, revision_number, source, local_facts_json,
                ai_intake_json, metadata_resolution_json, organization_json,
                processing_json, audio_strategy_json, recommendation_json,
                followups_json, warnings_json, created_at
         FROM item_plan_revisions
         WHERE item_plan_id = ?
         ORDER BY revision_number DESC, id DESC
         LIMIT 1",
    )
    .bind(plan_id)
    .fetch_optional(pool)
    .await?;
    Ok(revision)
}

pub async fn get_plan_envelope_for_item(
    pool: &SqlitePool,
    relative_path: &str,
) -> Result<Option<ItemPlanEnvelope>> {
    let Some(plan) = get_plan_for_item(pool, relative_path).await? else {
        return Ok(None);
    };
    let latest_revision = get_latest_revision_for_plan(pool, plan.id).await?;
    Ok(Some(ItemPlanEnvelope {
        plan,
        latest_revision,
    }))
}

pub async fn create_or_refresh_plan(
    pool: &SqlitePool,
    config: &AppConfig,
    relative_path: &str,
    source: &str,
) -> Result<ItemPlanEnvelope> {
    let local_facts = gather_local_facts(pool, relative_path).await?;
    let derived = derive_plan_data(pool, config, relative_path, &local_facts, &[]).await?;

    let local_facts_json = serde_json::to_string(&local_facts)?;
    let ai_intake_json = serde_json::to_string(&derived.ai_intake)?;
    let metadata_resolution_json = serde_json::to_string(&derived.metadata_resolution)?;
    let organization_json = serde_json::to_string(&derived.organization)?;
    let processing_json = serde_json::to_string(&derived.processing)?;
    let audio_strategy_json = serde_json::to_string(&derived.audio_strategy)?;
    let recommendation_json = serde_json::to_string(&derived.recommendation)?;
    let empty_followups = serde_json::to_string(&Vec::<String>::new())?;
    let warnings_json = serde_json::to_string(&derived.warnings)?;

    let mut tx = pool.begin().await?;

    let existing_plan = sqlx::query_as::<_, ItemPlan>(
        "SELECT id, relative_path, status, current_revision_id, created_at, updated_at
         FROM item_plans
         WHERE relative_path = ?
         ORDER BY updated_at DESC, id DESC
         LIMIT 1",
    )
    .bind(relative_path)
    .fetch_optional(&mut *tx)
    .await?;

    let plan_id = if let Some(plan) = existing_plan {
        plan.id
    } else {
        sqlx::query(
            "INSERT INTO item_plans (relative_path, status, current_revision_id)
             VALUES (?, 'Draft', NULL)",
        )
        .bind(relative_path)
        .execute(&mut *tx)
        .await?;
        sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
            .fetch_one(&mut *tx)
            .await?
    };

    let next_revision_number = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(revision_number), 0) + 1
         FROM item_plan_revisions
         WHERE item_plan_id = ?",
    )
    .bind(plan_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO item_plan_revisions (
            item_plan_id,
            revision_number,
            source,
            local_facts_json,
            ai_intake_json,
            metadata_resolution_json,
                organization_json,
            processing_json,
            audio_strategy_json,
            recommendation_json,
            followups_json,
            warnings_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(plan_id)
    .bind(next_revision_number)
    .bind(source)
    .bind(local_facts_json)
    .bind(ai_intake_json)
    .bind(metadata_resolution_json)
    .bind(organization_json)
    .bind(processing_json)
    .bind(audio_strategy_json)
    .bind(recommendation_json)
    .bind(empty_followups)
    .bind(warnings_json)
    .execute(&mut *tx)
    .await?;

    let revision_id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&mut *tx)
        .await?;

    sqlx::query(
        "UPDATE item_plans
         SET status = 'Draft',
             current_revision_id = ?,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )
    .bind(revision_id)
    .bind(plan_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let envelope = get_plan_envelope_for_item(pool, relative_path).await?;
    Ok(envelope.expect("plan exists after refresh"))
}

pub async fn apply_followup_for_item(
    pool: &SqlitePool,
    config: &AppConfig,
    relative_path: &str,
    message: &str,
) -> Result<ItemPlanEnvelope> {
    let envelope = if let Some(envelope) = get_plan_envelope_for_item(pool, relative_path).await? {
        envelope
    } else {
        create_or_refresh_plan(pool, config, relative_path, "followup-bootstrap").await?
    };

    let base_followups: Vec<String> = envelope
        .latest_revision
        .as_ref()
        .and_then(|revision| serde_json::from_str::<Vec<String>>(&revision.followups_json).ok())
        .unwrap_or_default();

    let mut followups = base_followups;
    followups.push(message.to_string());

    let local_facts = gather_local_facts(pool, relative_path).await?;
    let derived = derive_plan_data(pool, config, relative_path, &local_facts, &followups).await?;

    let local_facts_json = serde_json::to_string(&local_facts)?;
    let ai_intake_json = serde_json::to_string(&derived.ai_intake)?;
    let metadata_resolution_json = serde_json::to_string(&derived.metadata_resolution)?;
    let organization_json = serde_json::to_string(&derived.organization)?;
    let processing_json = serde_json::to_string(&derived.processing)?;
    let audio_strategy_json = serde_json::to_string(&derived.audio_strategy)?;
    let followups_json = serde_json::to_string(&followups)?;
    let recommendation_json = serde_json::to_string(&derived.recommendation)?;
    let warnings_json = serde_json::to_string(&derived.warnings)?;

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO item_plan_messages (item_plan_id, revision_id, role, message_text)
         VALUES (?, ?, 'operator', ?)",
    )
    .bind(envelope.plan.id)
    .bind(envelope.latest_revision.as_ref().map(|revision| revision.id))
    .bind(message)
    .execute(&mut *tx)
    .await?;

    let next_revision_number = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(revision_number), 0) + 1
         FROM item_plan_revisions
         WHERE item_plan_id = ?",
    )
    .bind(envelope.plan.id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO item_plan_revisions (
            item_plan_id,
            revision_number,
            source,
            local_facts_json,
            ai_intake_json,
            metadata_resolution_json,
                organization_json,
            processing_json,
            audio_strategy_json,
            recommendation_json,
            followups_json,
            warnings_json
            ) VALUES (?, ?, 'followup', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(envelope.plan.id)
    .bind(next_revision_number)
    .bind(local_facts_json)
    .bind(ai_intake_json)
    .bind(metadata_resolution_json)
    .bind(organization_json)
    .bind(processing_json)
    .bind(audio_strategy_json)
    .bind(recommendation_json)
    .bind(followups_json)
    .bind(warnings_json)
    .execute(&mut *tx)
    .await?;

    let revision_id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&mut *tx)
        .await?;

    sqlx::query(
        "UPDATE item_plans
         SET current_revision_id = ?,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )
    .bind(revision_id)
    .bind(envelope.plan.id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO item_plan_messages (item_plan_id, revision_id, role, message_text)
         VALUES (?, ?, 'planner', ?)",
    )
    .bind(envelope.plan.id)
    .bind(revision_id)
    .bind("Follow-up received; created a new planner revision.")
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let updated = get_plan_envelope_for_item(pool, relative_path).await?;
    Ok(updated.expect("plan exists after followup"))
}

pub async fn accept_plan_for_item(
    pool: &SqlitePool,
    relative_path: &str,
    accepted_metadata_json: Option<Value>,
    accepted_processing_json: Option<Value>,
    accepted_audio_strategy_json: Option<Value>,
    execution_mode: &str,
) -> Result<ItemPlanEnvelope> {
    if !is_valid_execution_mode(execution_mode) {
        anyhow::bail!("invalid execution mode: {execution_mode}");
    }

    let Some(envelope) = get_plan_envelope_for_item(pool, relative_path).await? else {
        anyhow::bail!("no plan exists for path: {relative_path}");
    };
    let Some(latest_revision) = envelope.latest_revision.as_ref() else {
        anyhow::bail!("plan has no revision to accept");
    };

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO item_plan_acceptance (
            item_plan_id,
            accepted_revision_id,
            accepted_metadata_json,
            accepted_processing_json,
            accepted_audio_strategy_json,
            accepted_execution_mode
         ) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(envelope.plan.id)
    .bind(latest_revision.id)
    .bind(accepted_metadata_json.map(|value| value.to_string()))
    .bind(accepted_processing_json.map(|value| value.to_string()))
    .bind(accepted_audio_strategy_json.map(|value| value.to_string()))
    .bind(execution_mode)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE item_plans
         SET status = 'Approved',
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )
    .bind(envelope.plan.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let updated = get_plan_envelope_for_item(pool, relative_path).await?;
    Ok(updated.expect("plan exists after acceptance"))
}

pub async fn save_audio_preference(
    pool: &SqlitePool,
    scope_type: &str,
    scope_key: &str,
    default_audio_track_policy: &str,
    normalization_mode: &str,
    night_listening_layout: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM item_audio_preferences
         WHERE lower(scope_type) = lower(?)
           AND lower(scope_key) = lower(?)",
    )
    .bind(scope_type)
    .bind(scope_key)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO item_audio_preferences (
            scope_type,
            scope_key,
            default_audio_track_policy,
            normalization_mode,
            night_listening_layout,
            updated_at
        ) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
    )
    .bind(scope_type)
    .bind(scope_key)
    .bind(default_audio_track_policy)
    .bind(normalization_mode)
    .bind(night_listening_layout)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    fn sample_local_facts(library_id: Option<&str>) -> PlannerLocalFacts {
        PlannerLocalFacts {
            relative_path: "movies/Example.Movie.2024.mkv".to_string(),
            absolute_path: "/data/movies/Example.Movie.2024.mkv".to_string(),
            library_id: library_id.map(|value| value.to_string()),
            fs_facts: FileSystemFacts::default(),
            probe: None,
            selected_metadata: None,
            existing_job_id: None,
        }
    }

    fn sample_candidate(media_kind: &str, imdb_id: Option<&str>) -> InternetMetadataMatch {
        InternetMetadataMatch {
            provider: "tmdb".to_string(),
            title: "Example Movie".to_string(),
            year: Some(2024),
            media_kind: media_kind.to_string(),
            imdb_id: imdb_id.map(|value| value.to_string()),
            tvdb_id: None,
            overview: None,
            rating: None,
            genres: Vec::new(),
            poster_url: None,
            backdrop_url: None,
            source_url: None,
        }
    }

    #[test]
    fn validate_ffmpeg_arguments_accepts_safe_placeholders() {
        let arguments = vec![
            "-i".to_string(),
            "input.mkv".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "output.mp4".to_string(),
        ];
        assert!(validate_ffmpeg_arguments(&arguments).is_ok());
    }

    #[test]
    fn validate_ffmpeg_arguments_rejects_managed_or_incomplete_flags() {
        let blocked = vec!["-i".to_string(), "input.mkv".to_string(), "-y".to_string(), "output.mp4".to_string()];
        assert!(validate_ffmpeg_arguments(&blocked).is_err());

        let missing_input = vec!["-c:a".to_string(), "aac".to_string(), "output.m4a".to_string()];
        assert!(validate_ffmpeg_arguments(&missing_input).is_err());

        let missing_output = vec!["-i".to_string(), "input.mkv".to_string(), "-c:a".to_string(), "aac".to_string()];
        assert!(validate_ffmpeg_arguments(&missing_output).is_err());
    }

    #[test]
    fn audio_parsers_map_expected_values() {
        assert!(matches!(parse_audio_mode("normalize_all"), AudioNormalizationMode::NormalizeAll));
        assert!(matches!(parse_audio_mode("normalize_primary_and_alternate"), AudioNormalizationMode::NormalizePrimaryAndAlternate));
        assert!(matches!(parse_audio_mode("unknown"), AudioNormalizationMode::Disabled));

        assert!(matches!(parse_night_layout("stereo"), Some(NightListeningLayout::Stereo)));
        assert!(matches!(parse_night_layout("2.1"), Some(NightListeningLayout::TwoPointOne)));
        assert!(parse_night_layout("invalid").is_none());

        assert!(matches!(
            parse_default_track_policy("prefer_night_listening_default"),
            DefaultAudioTrackPolicy::PreferNightListeningDefault
        ));
        assert!(matches!(
            parse_default_track_policy("anything_else"),
            DefaultAudioTrackPolicy::PreserveOriginalDefault
        ));
    }

    #[test]
    fn movie_scope_key_prefers_imdb_when_available() {
        let with_imdb = sample_candidate("movie", Some("tt1234567"));
        assert_eq!(movie_scope_key(&with_imdb), "tt1234567");

        let no_imdb = sample_candidate("movie", None);
        assert_eq!(movie_scope_key(&no_imdb), "example movie|2024");
    }

    #[test]
    fn planner_audio_scope_candidates_orders_specific_to_global() {
        let local_facts = sample_local_facts(Some("library-main"));
        let metadata_resolution = PlannerMetadataResolution {
            query_attempted: "example".to_string(),
            candidate_count: 1,
            selected_candidate_index: Some(0),
            selected_candidate: Some(sample_candidate("series", None)),
            provider_used: Some("tmdb".to_string()),
            warnings: Vec::new(),
        };

        let scopes = planner_audio_scope_candidates(&local_facts, &metadata_resolution);
        assert_eq!(
            scopes,
            vec![
                ("item".to_string(), "movies/Example.Movie.2024.mkv".to_string()),
                ("series".to_string(), "example movie".to_string()),
                ("library".to_string(), "library-main".to_string()),
                ("library".to_string(), "default".to_string()),
            ]
        );
    }

    #[test]
    fn execution_mode_validation_allows_only_supported_modes() {
        assert!(is_valid_execution_mode("full_plan"));
        assert!(is_valid_execution_mode("organize_only"));
        assert!(is_valid_execution_mode("process_only"));
        assert!(!is_valid_execution_mode("dry_run"));
        assert!(!is_valid_execution_mode(""));
    }

    async fn setup_acceptance_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should initialize");

        sqlx::query(
            "CREATE TABLE item_plans (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                relative_path       TEXT NOT NULL,
                status              TEXT NOT NULL,
                current_revision_id INTEGER,
                created_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .expect("item_plans table should be created");

        sqlx::query(
            "CREATE TABLE item_plan_revisions (
                id                          INTEGER PRIMARY KEY AUTOINCREMENT,
                item_plan_id                INTEGER NOT NULL,
                revision_number             INTEGER NOT NULL,
                source                      TEXT NOT NULL,
                local_facts_json            TEXT NOT NULL,
                ai_intake_json              TEXT,
                metadata_resolution_json    TEXT,
                organization_json           TEXT,
                processing_json             TEXT,
                audio_strategy_json         TEXT,
                recommendation_json         TEXT NOT NULL,
                followups_json              TEXT NOT NULL,
                warnings_json               TEXT NOT NULL,
                created_at                  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .expect("item_plan_revisions table should be created");

        sqlx::query(
            "CREATE TABLE item_plan_acceptance (
                id                              INTEGER PRIMARY KEY AUTOINCREMENT,
                item_plan_id                    INTEGER NOT NULL,
                accepted_revision_id            INTEGER NOT NULL,
                accepted_metadata_json          TEXT,
                accepted_processing_json        TEXT,
                accepted_audio_strategy_json    TEXT,
                accepted_execution_mode         TEXT NOT NULL,
                created_at                      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .expect("item_plan_acceptance table should be created");

        pool
    }

    async fn seed_plan_with_revision(pool: &SqlitePool, relative_path: &str) {
        sqlx::query(
            "INSERT INTO item_plans (relative_path, status, current_revision_id)
             VALUES (?, 'Draft', NULL)",
        )
        .bind(relative_path)
        .execute(pool)
        .await
        .expect("plan insert should succeed");

        let plan_id: i64 = sqlx::query_scalar("SELECT id FROM item_plans WHERE relative_path = ?")
            .bind(relative_path)
            .fetch_one(pool)
            .await
            .expect("plan id should be available");

        sqlx::query(
            "INSERT INTO item_plan_revisions (
                item_plan_id,
                revision_number,
                source,
                local_facts_json,
                recommendation_json,
                followups_json,
                warnings_json
             ) VALUES (?, 1, 'test', '{}', '{\"category\":\"keep\",\"reason\":\"ok\",\"confidence\":0.9}', '[]', '[]')",
        )
        .bind(plan_id)
        .execute(pool)
        .await
        .expect("revision insert should succeed");

        let revision_id: i64 = sqlx::query_scalar("SELECT id FROM item_plan_revisions WHERE item_plan_id = ?")
            .bind(plan_id)
            .fetch_one(pool)
            .await
            .expect("revision id should be available");

        sqlx::query("UPDATE item_plans SET current_revision_id = ? WHERE id = ?")
            .bind(revision_id)
            .bind(plan_id)
            .execute(pool)
            .await
            .expect("plan current revision should be set");
    }

    #[tokio::test]
    async fn accept_plan_rejects_invalid_execution_mode_without_insert() {
        let pool = setup_acceptance_pool().await;
        seed_plan_with_revision(&pool, "movies/sample.mkv").await;

        let error = accept_plan_for_item(
            &pool,
            "movies/sample.mkv",
            None,
            None,
            None,
            "dry_run",
        )
        .await
        .expect_err("invalid execution mode must fail");
        assert!(error.to_string().contains("invalid execution mode"));

        let acceptance_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item_plan_acceptance")
            .fetch_one(&pool)
            .await
            .expect("count query should succeed");
        assert_eq!(acceptance_rows, 0);
    }

    #[tokio::test]
    async fn accept_plan_inserts_acceptance_and_marks_plan_approved() {
        let pool = setup_acceptance_pool().await;
        seed_plan_with_revision(&pool, "movies/sample.mkv").await;

        let envelope = accept_plan_for_item(
            &pool,
            "movies/sample.mkv",
            None,
            None,
            None,
            "full_plan",
        )
        .await
        .expect("valid execution mode should succeed");
        assert_eq!(envelope.plan.status, "Approved");

        let acceptance_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item_plan_acceptance")
            .fetch_one(&pool)
            .await
            .expect("count query should succeed");
        assert_eq!(acceptance_rows, 1);
    }
}
