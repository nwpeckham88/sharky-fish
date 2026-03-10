use crate::config::LlmConfig;
use crate::messages::{IdentifiedMedia, ProcessingDecision, QueueMsg};
use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// The Brain actor receives identified media from the Identifier, consults the
/// LLM to determine optimal processing parameters, and enqueues jobs via the
/// Queue actor.
pub struct BrainActor {
    rx: mpsc::Receiver<IdentifiedMedia>,
    queue_tx: mpsc::Sender<QueueMsg>,
    llm_config: LlmConfig,
    client: reqwest::Client,
}

impl BrainActor {
    pub fn new(
        rx: mpsc::Receiver<IdentifiedMedia>,
        queue_tx: mpsc::Sender<QueueMsg>,
        llm_config: LlmConfig,
    ) -> Self {
        Self {
            rx,
            queue_tx,
            llm_config,
            client: reqwest::Client::new(),
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
        let probe_json = serde_json::to_string_pretty(&media.probe)?;
        let user_prompt = format!(
            "Analyze the following media probe data and generate optimized FFmpeg arguments.\n\n\
             File: {}\n\n\
             Probe:\n```json\n{}\n```",
            media.path.display(),
            probe_json
        );

        let mut temperature = 0.1_f64;
        for attempt in 0..3 {
            match self.call_llm(&user_prompt, temperature).await {
                Ok(decision) => return Ok(decision),
                Err(e) => {
                    warn!(attempt, err = %e, "brain: LLM call failed, retrying");
                    temperature += 0.1;
                }
            }
        }
        anyhow::bail!("LLM failed after 3 attempts");
    }

    async fn call_llm(&self, user_prompt: &str, temperature: f64) -> Result<ProcessingDecision> {
        let (url, body) = match self.llm_config.provider.as_str() {
            "openai" => self.build_openai_request(user_prompt, temperature),
            "ollama" => self.build_ollama_request(user_prompt, temperature),
            other => anyhow::bail!("unsupported LLM provider: {other}"),
        };

        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.llm_config.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await.context("LLM HTTP request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API returned {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        self.parse_llm_response(&json)
    }

    fn build_openai_request(
        &self,
        user_prompt: &str,
        temperature: f64,
    ) -> (String, serde_json::Value) {
        let url = format!("{}/chat/completions", self.llm_config.base_url);
        let body = serde_json::json!({
            "model": self.llm_config.model,
            "temperature": temperature,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": user_prompt }
            ]
        });
        (url, body)
    }

    fn build_ollama_request(
        &self,
        user_prompt: &str,
        temperature: f64,
    ) -> (String, serde_json::Value) {
        let url = format!("{}/api/chat", self.llm_config.base_url);
        let body = serde_json::json!({
            "model": self.llm_config.model,
            "stream": false,
            "format": "json",
            "options": { "temperature": temperature },
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": user_prompt }
            ]
        });
        (url, body)
    }

    fn parse_llm_response(&self, json: &serde_json::Value) -> Result<ProcessingDecision> {
        // Extract the content string from the model response.
        let content = json
            .pointer("/choices/0/message/content")          // OpenAI
            .or_else(|| json.pointer("/message/content"))   // Ollama
            .and_then(|v| v.as_str())
            .context("missing content in LLM response")?;

        let parsed: LlmOutput = serde_json::from_str(content)
            .context("failed to parse LLM JSON output")?;

        Ok(ProcessingDecision {
            job_id: 0, // assigned by Queue
            arguments: parsed.arguments,
            requires_two_pass: parsed.requires_two_pass,
            rationale: parsed.rationale,
        })
    }

    /// Hard-coded CPU-based libx264 fallback when LLM is unavailable.
    fn fallback_decision(media: &IdentifiedMedia) -> ProcessingDecision {
        let has_video = media.probe.streams.iter().any(|s| s.codec_type == "video");
        let args = if has_video {
            vec![
                "-i".into(), "input.mkv".into(),
                "-c:v".into(), "libx264".into(),
                "-preset".into(), "medium".into(),
                "-crf".into(), "20".into(),
                "-c:a".into(), "aac".into(),
                "-b:a".into(), "192k".into(),
                "-movflags".into(), "+faststart".into(),
                "output.mp4".into(),
            ]
        } else {
            vec![
                "-i".into(), "input.mkv".into(),
                "-c:a".into(), "aac".into(),
                "-b:a".into(), "192k".into(),
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

#[derive(Deserialize)]
struct LlmOutput {
    arguments: Vec<String>,
    requires_two_pass: bool,
    rationale: String,
}

const SYSTEM_PROMPT: &str = "\
You are an expert systems architect and media processing engine. \
Your sole function is to generate highly optimized, syntactically valid FFmpeg \
command arguments based on user requirements and host hardware capabilities. \
The host environment utilizes a Debian Linux architecture and natively supports \
NVIDIA NVENC hardware acceleration.\n\n\
Constraints:\n\
- Do not output the ffmpeg binary name; return only the exact argument array.\n\
- Ensure all audio streams are evaluated for EBU R128 compliance. If \
  normalization is required, flag it.\n\
- Do not include hardcoded path names or file variables; use generic \
  -i input.mkv and output.mp4 placeholders.\n\
- You must strictly adhere to the provided JSON schema. Do not include \
  markdown formatting, conversational text, thinking tokens, or preambles.\n\n\
Output strictly as JSON: {\"arguments\": [...], \"requires_two_pass\": bool, \"rationale\": \"...\"}";
