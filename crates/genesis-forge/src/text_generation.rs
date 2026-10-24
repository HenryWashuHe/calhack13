//! One bounded text completion, independent of any generated source language.
//! Provider accounting survives a rejected output. No retry or execution grant.
use crate::{AnthropicGenerator, SynthesisError};
use genesis_core::development_method::ResourceUsage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub const MAX_PROMPT_BYTES: usize = 512 * 1024;
pub const MAX_TEXT_BYTES: usize = 800 * 1024;
pub const API_VERSION: &str = crate::synthesis_http::ANTHROPIC_VERSION;

pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, SynthesisError> {
    serde_json::to_vec(value).map_err(|_| SynthesisError::InvalidInput)
}
pub(crate) fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn unknown() -> ResourceUsage {
    ResourceUsage::Unknown {
        reason: "provider usage unavailable".into(),
    }
}

/// Explicit disclosure: no sampling parameter is sent. This is not evidence of
/// fixed sampling across provider changes and cannot establish G4 comparability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampling {
    ProviderDefault,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextModelSettings {
    pub model: String,
    pub max_tokens: u32,
    pub timeout_ms: u64,
    pub sampling: Sampling,
}
impl TextModelSettings {
    pub fn validate(&self) -> Result<(), SynthesisError> {
        if !identifier(&self.model, 128)
            || !(1..=16_384).contains(&self.max_tokens)
            || !(1..=120_000).contains(&self.timeout_ms)
        {
            return Err(SynthesisError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextGenerationRequest {
    pub system: String,
    pub user: String,
    pub settings: TextModelSettings,
}
impl TextGenerationRequest {
    pub fn validate(&self) -> Result<(), SynthesisError> {
        self.settings.validate()?;
        if self.system.trim().is_empty()
            || self.user.trim().is_empty()
            || self.system.len() + self.user.len() > MAX_PROMPT_BYTES
            || self.system.contains('\0')
            || self.user.contains('\0')
        {
            return Err(SynthesisError::InvalidInput);
        }
        Ok(())
    }
    /// Exact submitted JSON bytes. API version is bound separately in identity.
    pub fn wire_bytes(&self) -> Result<Vec<u8>, SynthesisError> {
        self.validate()?;
        encode(
            &serde_json::json!({"model": self.settings.model, "max_tokens": self.settings.max_tokens,
            "system": self.system, "messages": [{"role":"user", "content": self.user}], "stream":false}),
        )
    }
    pub fn sha256(&self) -> Result<String, SynthesisError> {
        Ok(hash(&encode(&(
            "genesis-text-request/1",
            crate::synthesis_http::MESSAGES_URL,
            API_VERSION,
            self,
            hash(&self.wire_bytes()?),
        ))?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationUsage {
    pub model_calls: ResourceUsage,
    pub input_tokens: ResourceUsage,
    pub output_tokens: ResourceUsage,
    pub wall_time_ms: ResourceUsage,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextOutcome {
    Returned { text: String },
    Failed { reason: TextFailure },
    Unknown { reason: TextFailure },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFailure {
    Http,
    Protocol,
    AmbiguousTransport,
    Interrupted,
}

/// Observed endpoint class, not a signature or independently authenticated proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationTransport {
    AnthropicHttps,
    LoopbackFixture,
    Unobserved,
}

/// Stored only in trusted host storage. Hashes do not authenticate a provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextCompletion {
    pub transport: GenerationTransport,
    pub request_sha256: String,
    pub wire_sha256: String,
    pub response_sha256: Option<String>,
    pub message_id: Option<String>,
    pub actual_model: Option<String>,
    pub usage: GenerationUsage,
    pub outcome: TextOutcome,
}
impl TextCompletion {
    /// Host recovery observation for a persisted intent with no recorded outcome.
    pub fn interrupted(request: &TextGenerationRequest) -> Result<Self, SynthesisError> {
        Self::empty(
            request,
            TextOutcome::Unknown {
                reason: TextFailure::Interrupted,
            },
        )
    }
    fn empty(
        request: &TextGenerationRequest,
        outcome: TextOutcome,
    ) -> Result<Self, SynthesisError> {
        Ok(Self {
            transport: GenerationTransport::Unobserved,
            request_sha256: request.sha256()?,
            wire_sha256: hash(&request.wire_bytes()?),
            response_sha256: None,
            message_id: None,
            actual_model: None,
            usage: GenerationUsage {
                model_calls: unknown(),
                input_tokens: unknown(),
                output_tokens: unknown(),
                wall_time_ms: unknown(),
            },
            outcome,
        })
    }
    pub(crate) fn validate_for(
        &self,
        request: &TextGenerationRequest,
    ) -> Result<(), SynthesisError> {
        if self.request_sha256 != request.sha256()?
            || self.wire_sha256 != hash(&request.wire_bytes()?)
        {
            return Err(SynthesisError::Protocol);
        }
        if self
            .message_id
            .as_ref()
            .is_some_and(|s| !s.starts_with("msg_") || !identifier(s, 256))
            || self
                .actual_model
                .as_ref()
                .is_some_and(|s| !identifier(s, 128))
            || self.response_sha256.as_ref().is_some_and(|s| {
                s.len() != 64
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(SynthesisError::Protocol);
        }
        for usage in [
            &self.usage.model_calls,
            &self.usage.input_tokens,
            &self.usage.output_tokens,
            &self.usage.wall_time_ms,
        ] {
            if let ResourceUsage::Unknown { reason } = usage {
                if reason.trim().is_empty() || reason.len() > 256 {
                    return Err(SynthesisError::Protocol);
                }
            }
        }
        if let TextOutcome::Returned { text } = &self.outcome {
            if text.len() > MAX_TEXT_BYTES
                || self.actual_model.as_deref() != Some(&request.settings.model)
                || self.message_id.is_none()
                || self.response_sha256.is_none()
                || self.usage.model_calls != (ResourceUsage::Measured { value: 1 })
                || matches!(self.usage.output_tokens, ResourceUsage::Measured {value} if value>u64::from(request.settings.max_tokens))
            {
                return Err(SynthesisError::Protocol);
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Response {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    role: String,
    model: String,
    content: serde_json::Value,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    usage: Option<ProviderUsage>,
}
#[derive(Deserialize)]
struct ProviderUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    citations: Option<Vec<serde_json::Value>>,
}
fn reported(value: Option<u64>) -> ResourceUsage {
    value.map_or_else(unknown, |value| ResourceUsage::Measured { value })
}

impl AnthropicGenerator {
    /// Caller must persist its requesting state first. This performs at most one
    /// POST. Invalid configuration fails before any effect; all later failures
    /// return a bound completion so unknown or measured usage is retained.
    pub async fn generate_text(
        &self,
        request: &TextGenerationRequest,
    ) -> Result<TextCompletion, SynthesisError> {
        let bytes = request.wire_bytes()?;
        let mut completion = TextCompletion::empty(
            request,
            TextOutcome::Failed {
                reason: TextFailure::Protocol,
            },
        )?;
        completion.transport = if self.is_loopback_fixture() {
            GenerationTransport::LoopbackFixture
        } else {
            GenerationTransport::AnthropicHttps
        };
        let started = Instant::now();
        let response = tokio::time::timeout(
            Duration::from_millis(request.settings.timeout_ms),
            self.post_message(&bytes),
        )
        .await;
        completion.usage.wall_time_ms = ResourceUsage::Measured {
            value: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        };
        let bytes = match response {
            Ok(Ok(bytes)) => bytes,
            Err(_) | Ok(Err(SynthesisError::AmbiguousTransport)) => {
                completion.outcome = TextOutcome::Unknown {
                    reason: TextFailure::AmbiguousTransport,
                };
                return Ok(completion);
            }
            Ok(Err(SynthesisError::Http { .. })) => {
                completion.outcome = TextOutcome::Failed {
                    reason: TextFailure::Http,
                };
                return Ok(completion);
            }
            Ok(Err(_)) => return Ok(completion),
        };
        completion.response_sha256 = Some(hash(&bytes));
        let Ok(value) = genesis_verification::decode_json(&bytes) else {
            return Ok(completion);
        };
        let Ok(response) = serde_json::from_value::<Response>(value) else {
            return Ok(completion);
        };
        if response.kind != "message"
            || response.role != "assistant"
            || !response.id.starts_with("msg_")
            || !identifier(&response.id, 256)
            || !identifier(&response.model, 128)
        {
            return Ok(completion);
        }
        completion.message_id = Some(response.id);
        completion.actual_model = Some(response.model.clone());
        completion.usage.model_calls = ResourceUsage::Measured { value: 1 };
        if let Some(usage) = response.usage {
            completion.usage.input_tokens = reported(usage.input_tokens);
            completion.usage.output_tokens = reported(usage.output_tokens);
        }
        if response.model != request.settings.model
            || response.stop_reason.as_deref() != Some("end_turn")
            || response.stop_sequence.is_some()
            || matches!(completion.usage.output_tokens, ResourceUsage::Measured {value} if value>u64::from(request.settings.max_tokens))
        {
            return Ok(completion);
        }
        let Ok(mut blocks) = serde_json::from_value::<Vec<Block>>(response.content) else {
            return Ok(completion);
        };
        if blocks.len() != 1 {
            return Ok(completion);
        }
        let block = blocks.remove(0);
        if block.kind != "text"
            || block.text.len() > MAX_TEXT_BYTES
            || block.citations.is_some_and(|v| !v.is_empty())
        {
            return Ok(completion);
        }
        completion.outcome = TextOutcome::Returned { text: block.text };
        completion.validate_for(request)?;
        Ok(completion)
    }
}
