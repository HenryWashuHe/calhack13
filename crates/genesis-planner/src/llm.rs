//! Model-agnostic LLM interface.
//!
//! The planner talks to a model only through [`LlmClient`]. Concrete clients:
//!
//! - [`AnthropicClient`]: Anthropic Messages API.
//! - [`OpenAiCompatClient`]: any OpenAI-compatible `/chat/completions`
//!   endpoint (OpenAI, Gemini's OpenAI-compatible endpoint, local servers).
//! - [`ReplayClient`]: returns scripted responses. **Tests and offline demos
//!   only**; its model name is `replay` so evidence shows it was not a model.
//!
//! An Elastic Agent Builder planner can implement the same trait later.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("model is not configured: {0}")]
    NotConfigured(String),
    #[error("model API returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("model request failed: {0}")]
    Transport(String),
    #[error("unexpected model response: {0}")]
    Malformed(String),
    #[error("replay script exhausted")]
    ReplayExhausted,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Human-readable `provider/model`, recorded in planner decisions.
    fn model(&self) -> String;
    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError>;
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("reqwest client")
}

async fn read_json(resp: reqwest::Response) -> Result<serde_json::Value, LlmError> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| LlmError::Transport(e.to_string()))?;
    if !status.is_success() {
        // Never echo request bodies or keys; the response body is the provider's.
        let body: String = text.chars().take(600).collect();
        return Err(LlmError::Http {
            status: status.as_u16(),
            body,
        });
    }
    serde_json::from_str(&text).map_err(|e| LlmError::Malformed(e.to_string()))
}

/// Anthropic Messages API.
pub struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    http: reqwest::Client,
}

impl AnthropicClient {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".into()),
            http: http(),
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    fn model(&self) -> String {
        format!("anthropic/{}", self.model)
    }

    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": req.messages,
        });
        let resp = self
            .http
            .post(format!(
                "{}/v1/messages",
                self.base_url.trim_end_matches('/')
            ))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        let v = read_json(resp).await?;
        let text: String = v["content"]
            .as_array()
            .ok_or_else(|| LlmError::Malformed("missing content".into()))?
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            return Err(LlmError::Malformed("no text content".into()));
        }
        Ok(text)
    }
}

/// OpenAI-compatible chat completions.
pub struct OpenAiCompatClient {
    api_key: String,
    model: String,
    base_url: String,
    provider_label: String,
    http: reqwest::Client,
}

impl OpenAiCompatClient {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
        provider_label: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
            provider_label: provider_label.into(),
            http: http(),
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    fn model(&self) -> String {
        format!("{}/{}", self.provider_label, self.model)
    }

    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError> {
        let mut messages = vec![serde_json::json!({"role": "system", "content": req.system})];
        messages.extend(
            req.messages
                .iter()
                .map(|m| serde_json::json!({"role": m.role, "content": m.content})),
        );
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
            "response_format": {"type": "json_object"},
        });
        let resp = self
            .http
            .post(format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        let v = read_json(resp).await?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| LlmError::Malformed("missing choices[0].message.content".into()))
    }
}

/// Scripted responses for tests and offline demos. Records every request so
/// tests can check what the planner sent.
#[derive(Default)]
pub struct ReplayClient {
    responses: Mutex<VecDeque<String>>,
    pub seen: Mutex<Vec<LlmRequest>>,
}

impl ReplayClient {
    pub fn new<I, S>(responses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            responses: Mutex::new(responses.into_iter().map(Into::into).collect()),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<LlmRequest> {
        self.seen.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[async_trait]
impl LlmClient for ReplayClient {
    fn model(&self) -> String {
        "replay".into()
    }

    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError> {
        self.seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(req.clone());
        self.responses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front()
            .ok_or(LlmError::ReplayExhausted)
    }
}

/// Build a client from environment variables. Returns `Ok(None)` when no
/// provider is configured yet ("pending"), so callers can still preview
/// prompts.
///
/// - `GENESIS_LLM_PROVIDER`: `anthropic` | `openai` | `gemini` | `openai-compatible`
/// - `GENESIS_LLM_MODEL`: model name (required when a provider is set)
/// - `GENESIS_LLM_API_KEY`: API key
/// - `GENESIS_LLM_BASE_URL`: override endpoint (required for `openai-compatible`)
pub fn from_env() -> Result<Option<Box<dyn LlmClient>>, LlmError> {
    let provider = std::env::var("GENESIS_LLM_PROVIDER").unwrap_or_default();
    let provider = provider.trim().to_lowercase();
    if provider.is_empty() || provider == "pending" || provider == "none" {
        return Ok(None);
    }
    let model = std::env::var("GENESIS_LLM_MODEL")
        .map_err(|_| LlmError::NotConfigured("GENESIS_LLM_MODEL is required".into()))?;
    let key = std::env::var("GENESIS_LLM_API_KEY")
        .map_err(|_| LlmError::NotConfigured("GENESIS_LLM_API_KEY is required".into()))?;
    let base = std::env::var("GENESIS_LLM_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let client: Box<dyn LlmClient> = match provider.as_str() {
        "anthropic" => Box::new(AnthropicClient::new(key, model, base)),
        "openai" => Box::new(OpenAiCompatClient::new(
            key,
            model,
            base.unwrap_or_else(|| "https://api.openai.com/v1".into()),
            "openai",
        )),
        "gemini" => Box::new(OpenAiCompatClient::new(
            key,
            model,
            base.unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta/openai".into()),
            "gemini",
        )),
        "openai-compatible" => Box::new(OpenAiCompatClient::new(
            key,
            model,
            base.ok_or_else(|| {
                LlmError::NotConfigured("GENESIS_LLM_BASE_URL is required for openai-compatible".into())
            })?,
            "openai-compatible",
        )),
        other => {
            return Err(LlmError::NotConfigured(format!(
                "unknown GENESIS_LLM_PROVIDER `{other}` (use anthropic, openai, gemini or openai-compatible)"
            )))
        }
    };
    Ok(Some(client))
}
