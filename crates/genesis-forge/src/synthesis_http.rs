//! One-shot Anthropic Messages adapter. No redirects, tool calls, or automatic
//! retry: a lost submission/response is an ambiguous outcome for the durable
//! session to reconcile. Provider error bodies are deliberately never retained.
use crate::{GenerationReceipt, ModelConfig, SynthesisError, SynthesisPrompt};
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_TIMEOUT: Duration = Duration::from_secs(120);

// Intentionally no Debug implementation: credentials must not appear in logs.
pub struct AnthropicGenerator {
    client: reqwest::Client,
    endpoint: url::Url,
    api_key: HeaderValue,
}

impl AnthropicGenerator {
    pub fn new(api_key: &str) -> Result<Self, SynthesisError> {
        Self::build(MESSAGES_URL, api_key, MAX_TIMEOUT, false)
    }

    /// Inject a literal loopback HTTP endpoint for contract tests only. There is
    /// no general configurable production endpoint that could receive the key.
    pub fn with_test_endpoint(endpoint: &str, api_key: &str) -> Result<Self, SynthesisError> {
        Self::build(endpoint, api_key, MAX_TIMEOUT, true)
    }

    /// Bounded timeout override for deterministic loopback transport tests.
    pub fn with_test_endpoint_and_timeout(
        endpoint: &str,
        api_key: &str,
        timeout: Duration,
    ) -> Result<Self, SynthesisError> {
        Self::build(endpoint, api_key, timeout, true)
    }

    fn build(
        endpoint: &str,
        api_key: &str,
        timeout: Duration,
        test_endpoint: bool,
    ) -> Result<Self, SynthesisError> {
        let endpoint = url::Url::parse(endpoint).map_err(|_| SynthesisError::InvalidInput)?;
        let loopback = match endpoint.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if (test_endpoint && (endpoint.scheme() != "http" || !loopback))
            || (!test_endpoint && endpoint.as_str() != MESSAGES_URL)
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || timeout.is_zero()
            || timeout > MAX_TIMEOUT
            || api_key.is_empty()
            || api_key.len() > 4096
            || !api_key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(SynthesisError::InvalidInput);
        }
        let mut api_key =
            HeaderValue::from_str(api_key).map_err(|_| SynthesisError::InvalidInput)?;
        api_key.set_sensitive(true);
        let client = reqwest::Client::builder()
            .https_only(!test_endpoint)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .build()
            .map_err(|_| SynthesisError::InvalidInput)?;
        Ok(Self {
            client,
            endpoint,
            api_key,
        })
    }

    pub async fn generate(
        &self,
        prompt: &SynthesisPrompt,
        config: &ModelConfig,
    ) -> Result<GenerationReceipt, SynthesisError> {
        prompt.validate()?;
        config.validate()?;
        let body = MessageRequest {
            model: &config.model,
            max_tokens: config.max_tokens,
            system: &prompt.system,
            messages: [UserMessage {
                role: "user",
                content: &prompt.user,
            }],
            stream: false,
        };
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", "2023-06-01")
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|_| SynthesisError::AmbiguousTransport)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(if (300..=599).contains(&status) {
                SynthesisError::Http { status }
            } else {
                SynthesisError::Protocol
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(SynthesisError::Protocol);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| SynthesisError::AmbiguousTransport)?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(SynthesisError::Protocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        parse_response(&bytes, prompt, config)
    }
}

#[derive(Serialize)]
struct MessageRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: [UserMessage<'a>; 1],
    stream: bool,
}
#[derive(Serialize)]
struct UserMessage<'a> {
    role: &'a str,
    content: &'a str,
}

// Anthropic can add response metadata without changing the content contract.
// All fields consumed below reject duplicates through typed deserialization.
#[derive(Deserialize)]
struct MessageResponse {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    role: String,
    model: String,
    content: Vec<TextBlock>,
    stop_reason: String,
    stop_sequence: Option<String>,
    usage: Usage,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    citations: Option<Vec<serde_json::Value>>,
}
#[derive(Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedSource {
    extractor_typescript: String,
}

fn parse_response(
    bytes: &[u8],
    prompt: &SynthesisPrompt,
    config: &ModelConfig,
) -> Result<GenerationReceipt, SynthesisError> {
    let response: MessageResponse =
        serde_json::from_slice(bytes).map_err(|_| SynthesisError::Protocol)?;
    if response.kind != "message"
        || response.role != "assistant"
        || response.model != config.model
        || response.stop_reason != "end_turn"
        || response.stop_sequence.is_some()
        || response.content.len() != 1
    {
        return Err(SynthesisError::Protocol);
    }
    let block = response
        .content
        .into_iter()
        .next()
        .ok_or(SynthesisError::Protocol)?;
    if block.kind != "text" || block.citations.is_some_and(|c| !c.is_empty()) {
        return Err(SynthesisError::Protocol);
    }
    let generated: GeneratedSource =
        serde_json::from_str(&block.text).map_err(|_| SynthesisError::Protocol)?;
    GenerationReceipt::new(
        response.id,
        response.model,
        response.usage.input_tokens,
        response.usage.output_tokens,
        generated.extractor_typescript,
        prompt,
        config,
    )
}
