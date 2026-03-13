use crate::config::{AppConfig, GoldenStandards, LlmConfig};
use crate::messages::{IdentifiedMedia, ProcessingDecision, QueueMsg};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{info, warn};

pub async fn test_llm_connection(llm_config: &LlmConfig) -> Result<String> {
    let system_prompt = "You validate API connectivity and must return strict JSON.";
    let user_prompt =
        "Return JSON exactly like {\"status\":\"ok\",\"message\":\"connection verified\"}.";
    let client = reqwest::Client::new();

    let (url, body) = match llm_config.provider.as_str() {
        "google" => build_google_request(llm_config, system_prompt, user_prompt, 0.0),
        "openai" => build_openai_request(llm_config, system_prompt, user_prompt, 0.0),
        "ollama" => build_ollama_request(llm_config, system_prompt, user_prompt, 0.0),
        other => anyhow::bail!("unsupported LLM provider: {other}"),
    };

    let mut req = client.post(&url).json(&body);
    if llm_config.provider == "google" {
        let Some(key) = llm_config
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            anyhow::bail!("Google AI API key is required");
        };
        req = req.header("x-goog-api-key", key);
    } else if llm_config.provider == "openai" {
        let Some(key) = llm_config
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            anyhow::bail!("OpenAI API key is required");
        };
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
    Ok(content.to_string())
}

pub async fn create_processing_decision(
    app_config: &AppConfig,
    media: &IdentifiedMedia,
) -> Result<ProcessingDecision> {
    let client = reqwest::Client::new();
    let probe_json = serde_json::to_string_pretty(&media.probe)?;
    let standards = &app_config.golden_standards;
    let system_prompt = if app_config.system_prompt.trim().is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        app_config.system_prompt.trim().to_string()
    };

    let subtitle_summary = {
        let sub_streams: Vec<_> = media
            .probe
            .streams
            .iter()
            .filter(|s| s.codec_type == "subtitle")
            .collect();
        if sub_streams.is_empty() {
            "No subtitle streams present.".to_string()
        } else {
            let descs: Vec<String> = sub_streams
                .iter()
                .map(|s| {
                    let lang = s.language.as_deref().unwrap_or("und");
                    let mut flags = Vec::new();
                    if s.disposition.forced {
                        flags.push("forced");
                    }
                    if s.disposition.hearing_impaired {
                        flags.push("SDH");
                    }
                    if s.disposition.default {
                        flags.push("default");
                    }
                    let flag_str = if flags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", flags.join(", "))
                    };
                    format!("  #{}: {} ({}){}", s.index, lang, s.codec_name, flag_str)
                })
                .collect();
            format!("Subtitle streams:\n{}", descs.join("\n"))
        }
    };

    let user_prompt = format!(
        "Analyze the following media probe data and generate optimized FFmpeg arguments.\n\n\
         File: {}\n\n\
         Golden Standards:\n\
         - Video: codec={}, max_bitrate={}Mbps, resolution_ceiling={}\n\
         - Audio: codec={}, target_lufs={}, target_true_peak={}, max_channels={}, keep_multiple_tracks={}, create_stereo_downmix={}\n\
         - Subtitles: mode={}, preferred_languages=[{}], keep_forced={}, keep_sdh={}\n\n\
         Playback Context:\n{}\n\n\
         {}\n\n\
         Probe:\n```json\n{}\n```",
        media.path.display(),
        standards.video.codec,
        standards.video.max_bitrate_mbps,
        standards.video.resolution_ceiling,
        standards.audio.codec,
        standards.audio.target_lufs,
        standards.audio.target_true_peak,
        standards.audio.max_channels,
        standards.audio.keep_multiple_tracks,
        standards.audio.create_stereo_downmix,
        standards.subtitle.mode,
        standards.subtitle.preferred_languages.join(", "),
        standards.subtitle.keep_forced,
        standards.subtitle.keep_sdh,
        playback_context_block(&app_config.playback_context),
        subtitle_summary,
        probe_json
    );

    let mut temperature = 0.1_f64;
    for attempt in 0..3 {
        match call_llm(
            &client,
            &app_config.llm,
            &system_prompt,
            &user_prompt,
            temperature,
        )
        .await
        {
            Ok(decision) => return Ok(decision),
            Err(e) => {
                warn!(attempt, err = %e, "brain: LLM call failed, retrying");
                temperature += 0.1;
            }
        }
    }

    anyhow::bail!("LLM failed after 3 attempts")
}

pub async fn improve_system_prompt(
    llm_config: &LlmConfig,
    concept: &str,
    current_prompt: &str,
    playback_context: &str,
    standards: &GoldenStandards,
    mode: &str,
) -> Result<String> {
    let client = reqwest::Client::new();
    let system_prompt = "You rewrite rough media-policy ideas into a precise system prompt for Sharky Fish. Return strict JSON exactly like {\"prompt\":\"...\"}. Do not use markdown fences, commentary, or extra keys.";
    let mode_instructions = if mode == "append_policy" {
        "Generate a standalone policy section that can be appended to an existing system prompt. Do not restate the entire prompt. Start directly with policy content that reads naturally when appended."
    } else {
        "Generate a full replacement system prompt."
    };
    let user_prompt = format!(
        "Expand the operator's rough concept into a stronger system prompt for the Sharky Fish FFmpeg planning assistant.\n\n\
         Requirements:\n\
         - Preserve the app's strict JSON-only output behavior for FFmpeg planning.\n\
         - Make the prompt explicit about codec, compatibility, and media-quality priorities.\n\
         - Treat playback device notes as compatibility guidance, not as a rigid inventory schema.\n\
         - Keep the result concise enough to be practical as a saved system prompt.\n\
         - Return only a single prompt string inside the JSON object.\n\n\
         Output mode:\n{}\n\n\
         Current prompt:\n{}\n\n\
         Operator concept:\n{}\n\n\
         Playback context:\n{}\n\n\
         Golden standards:\n\
         - Video: codec={}, max_bitrate={}Mbps, resolution_ceiling={}\n\
         - Audio: codec={}, target_lufs={}, target_true_peak={}, max_channels={}, keep_multiple_tracks={}, create_stereo_downmix={}\n\
         - Subtitles: mode={}, preferred_languages=[{}], keep_forced={}, keep_sdh={}",
        mode_instructions,
        current_prompt.trim(),
        concept.trim(),
        playback_context_block(playback_context),
        standards.video.codec,
        standards.video.max_bitrate_mbps,
        standards.video.resolution_ceiling,
        standards.audio.codec,
        standards.audio.target_lufs,
        standards.audio.target_true_peak,
        standards.audio.max_channels,
        standards.audio.keep_multiple_tracks,
        standards.audio.create_stereo_downmix,
        standards.subtitle.mode,
        standards.subtitle.preferred_languages.join(", "),
        standards.subtitle.keep_forced,
        standards.subtitle.keep_sdh,
    );

    let (url, body) = match llm_config.provider.as_str() {
        "google" => build_google_request(llm_config, system_prompt, &user_prompt, 0.3),
        "openai" => build_openai_request(llm_config, system_prompt, &user_prompt, 0.3),
        "ollama" => build_ollama_request(llm_config, system_prompt, &user_prompt, 0.3),
        other => anyhow::bail!("unsupported LLM provider: {other}"),
    };

    let mut req = client.post(&url).json(&body);
    if llm_config.provider == "google" {
        let Some(key) = llm_config
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            anyhow::bail!("Google AI API key is required");
        };
        req = req.header("x-goog-api-key", key);
    } else if llm_config.provider == "openai" {
        let Some(key) = llm_config
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            anyhow::bail!("OpenAI API key is required");
        };
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
    let parsed: PromptImprovementOutput =
        serde_json::from_str(content).context("failed to parse improved prompt JSON output")?;
    Ok(parsed.prompt.trim().to_string())
}

/// The Brain actor receives identified media from the Identifier, consults the
/// LLM to determine optimal processing parameters, and enqueues jobs via the
/// Queue actor.
pub struct BrainActor {
    rx: mpsc::Receiver<IdentifiedMedia>,
    queue_tx: mpsc::Sender<QueueMsg>,
    config: Arc<RwLock<AppConfig>>,
}

impl BrainActor {
    pub fn new(
        rx: mpsc::Receiver<IdentifiedMedia>,
        queue_tx: mpsc::Sender<QueueMsg>,
        config: Arc<RwLock<AppConfig>>,
    ) -> Self {
        Self {
            rx,
            queue_tx,
            config,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("brain: actor started");
        while let Some(media) = self.rx.recv().await {
            match self.decide(&media).await {
                Ok(decision) => {
                    info!(job_id = decision.job_id, "brain: decision rendered");
                    let _ = self
                        .queue_tx
                        .send(QueueMsg::Enqueue { media, decision })
                        .await;
                }
                Err(e) => {
                    warn!(file = %media.path.display(), err = %e, "brain: LLM decision failed, using fallback");
                    let fallback = Self::fallback_decision(&media);
                    let _ = self
                        .queue_tx
                        .send(QueueMsg::Enqueue {
                            media,
                            decision: fallback,
                        })
                        .await;
                }
            }
        }
        Ok(())
    }

    async fn decide(&self, media: &IdentifiedMedia) -> Result<ProcessingDecision> {
        let cfg = self.config.read().await.clone();
        create_processing_decision(&cfg, media).await
    }

    /// Hard-coded CPU-based libx264 fallback when LLM is unavailable.
    fn fallback_decision(media: &IdentifiedMedia) -> ProcessingDecision {
        let has_video = media.probe.streams.iter().any(|s| s.codec_type == "video");
        let args = if has_video {
            vec![
                "-i".into(),
                "input.mkv".into(),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "medium".into(),
                "-crf".into(),
                "20".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
                "-movflags".into(),
                "+faststart".into(),
                "output.mp4".into(),
            ]
        } else {
            vec![
                "-i".into(),
                "input.mkv".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
                "output.m4a".into(),
            ]
        };

        ProcessingDecision {
            job_id: 0,
            arguments: args,
            requires_two_pass: true,
            rationale: "Fallback: CPU-based libx264/aac transcoding".into(),
        }
    }
}

async fn call_llm(
    client: &reqwest::Client,
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> Result<ProcessingDecision> {
    let (url, body) = match llm_config.provider.as_str() {
        "google" => build_google_request(llm_config, system_prompt, user_prompt, temperature),
        "openai" => build_openai_request(llm_config, system_prompt, user_prompt, temperature),
        "ollama" => build_ollama_request(llm_config, system_prompt, user_prompt, temperature),
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
    parse_llm_response(&json)
}

fn parse_llm_response(json: &serde_json::Value) -> Result<ProcessingDecision> {
    let content = extract_llm_content(json)?;

    let parsed: LlmOutput =
        serde_json::from_str(content).context("failed to parse LLM JSON output")?;

    Ok(ProcessingDecision {
        job_id: 0,
        arguments: parsed.arguments,
        requires_two_pass: parsed.requires_two_pass,
        rationale: parsed.rationale,
    })
}

fn build_openai_request(
    llm_config: &LlmConfig,
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
    llm_config: &LlmConfig,
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
    llm_config: &LlmConfig,
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

#[derive(Deserialize)]
struct LlmOutput {
    arguments: Vec<String>,
    requires_two_pass: bool,
    rationale: String,
}

#[derive(Deserialize)]
struct PromptImprovementOutput {
    prompt: String,
}

fn playback_context_block(playback_context: &str) -> &str {
    let trimmed = playback_context.trim();
    if trimmed.is_empty() {
        "No playback device notes were provided."
    } else {
        trimmed
    }
}

const SYSTEM_PROMPT: &str = "\
You are an expert systems architect and media processing engine. \
Your sole function is to generate highly optimized, syntactically valid FFmpeg \
command arguments based on user requirements and host hardware capabilities. \
The host environment utilizes a Debian Linux architecture and natively supports \
NVIDIA NVENC hardware acceleration.\n\n\
Constraints:\n\
- Do not output the ffmpeg binary name; return only the exact argument array.\n\
- When playback device notes are supplied, optimize for the stated client capabilities and compatibility limits.\n\
- Ensure all audio streams are evaluated for EBU R128 compliance. If \
  normalization is required, flag it.\n\
- Respect the audio policy provided in the request, including preferred codec, multitrack retention, and stereo downmix requirements.\n\
- Handle subtitle streams according to the provided subtitle standards:\n\
  * mode=keep_all: copy all subtitle streams with -c:s copy.\n\
  * mode=remove_all: map out all subtitle streams entirely (-sn).\n\
  * mode=keep_preferred: use explicit -map 0:s:N for subtitle streams \
    matching the preferred_languages list. If keep_forced is true, also \
    include forced tracks in preferred languages even if they wouldn't \
    otherwise match. If keep_sdh is true, include hearing-impaired tracks.\n\
  * mode=keep_forced_only: strip all subtitle streams except those marked \
    forced in the preferred languages.\n\
- Copy subtitle codec unless the output container cannot hold the source \
  format (e.g., ass/ssa going to mp4 should be converted to mov_text).\n\
- Do not include hardcoded path names or file variables; use generic \
  -i input.mkv and output.mp4 placeholders.\n\
- You must strictly adhere to the provided JSON schema. Do not include \
  markdown formatting, conversational text, thinking tokens, or preambles.\n\n\
Output strictly as JSON: {\"arguments\": [...], \"requires_two_pass\": bool, \"rationale\": \"...\"}";
