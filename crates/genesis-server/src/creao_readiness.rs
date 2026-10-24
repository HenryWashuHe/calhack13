//! Submit a challenge-bound CREAO run; only the gateway receipt proves readiness.
//!
//! The private CREAO journal retains the exact request (including its nonce) for
//! immutable replay checks. Neither a saved ticket nor native completion is an
//! authority or readiness proof. This adapter never polls or retries a run.

use crate::gateway_readiness::{ProbeDispatch, ProbeRequest, ReadinessProbe, RECEIPT_PATH};
use async_trait::async_trait;
use chrono::Utc;
use genesis_creao::{
    OperationPhase, OperationPurpose, OperationRecord, OperationRequest, OperationSpec, RunRequest,
    Runtime, RuntimeError, RuntimeFailure,
};
use genesis_providers::ProviderError;
use genesis_verification::json_sha256;
use serde_json::json;

const INSTRUCTIONS: &str = "Perform only GENESIS gateway readiness: use your installed gateway skill to make one HTTP POST to receipt_url with receipt as its JSON body. The nonce authorizes only that receipt, not a mission job or any other action. Do not create or modify agents, perform mission work, or contact other endpoints. Do not include the nonce in logs, messages, or output. Report only the HTTP outcome; GENESIS determines readiness from the callback it receives, never from your report.";

#[derive(Clone)]
pub struct CreaoReadinessProbe {
    runtime: Runtime,
    receipt_url: String,
}

fn rejected() -> ProviderError {
    ProviderError::Rejected(
        "CREAO readiness request is invalid or conflicts with its saved intent".into(),
    )
}

fn unknown() -> ProviderError {
    ProviderError::Unknown("CREAO readiness dispatch may have run; retain the challenge and inspect the private checkpoint".into())
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

impl CreaoReadinessProbe {
    /// Share a clone of the recovered specialist runtime so quota and submission
    /// gates remain common. The configured URL is not taken from agent output.
    pub fn new(runtime: Runtime, receipt_url: String) -> Result<Self, ProviderError> {
        let url = reqwest::Url::parse(&receipt_url).map_err(|_| rejected())?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !bounded(&receipt_url, 2048)
            || !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.path().ends_with(RECEIPT_PATH)
        {
            return Err(rejected());
        }
        Ok(Self {
            runtime,
            receipt_url,
        })
    }

    fn spec(&self, request: &ProbeRequest) -> Result<OperationSpec, ProviderError> {
        if request.org_id.as_str() != self.runtime.org_id()
            || request.receipt_url != self.receipt_url
            || !bounded(&request.nonce, 512)
            || !bounded(&request.role_key, 256)
            || request.expires_at <= Utc::now()
        {
            return Err(rejected());
        }
        let spec = OperationSpec {
            schema_version: 1,
            api_version: self.runtime.api_version(),
            operation_id: format!(
                "creao-readiness-{}",
                json_sha256(&json!({"org_id":request.org_id,"challenge_id":request.challenge_id}))
            ),
            org_id: request.org_id.to_string(),
            role_id: request.role_id.clone(),
            account_namespace: self.runtime.account_namespace().to_owned(),
            purpose: OperationPurpose::Readiness {
                challenge_id: request.challenge_id.clone(),
                config_digest: request.config_digest.clone(),
                expires_at: request.expires_at,
            },
            request: OperationRequest::CreateRun(RunRequest {
                agent_id: request.provider_agent_id.clone(),
                input: json!({
                    "schema_version":1,
                    "task":"genesis_gateway_readiness",
                    "instructions":INSTRUCTIONS,
                    "receipt_url":request.receipt_url,
                    "receipt":request.expected_receipt(),
                    "context":{
                        "org_id":request.org_id,"role_id":request.role_id,
                        "role_key":request.role_key,"config_digest":request.config_digest,
                        "expires_at":request.expires_at,
                    },
                }),
                // Creation and execution conversations never carry over into
                // readiness. The client also forces should_notify=false.
                model: None,
                conversation_id: None,
                workspace_id: None,
            }),
        };
        spec.validate().map_err(|_| rejected())?;
        Ok(spec)
    }
}

fn dispatched(record: OperationRecord) -> Result<ProbeDispatch, ProviderError> {
    if let Some(ticket) = &record.ticket {
        // Even a completed/failed native run is only proof of dispatch. The
        // gateway independently checks the receipt, nonce and lifetime.
        return Ok(ProbeDispatch {
            probe_ref: Some(ticket.run_id().to_owned()),
        });
    }
    if record.phase == OperationPhase::Failed
        && matches!(record.failure, Some(RuntimeFailure::Provider(ref error)) if !error.post_may_have_run)
    {
        return Err(rejected());
    }
    Err(unknown())
}

#[async_trait]
impl ReadinessProbe for CreaoReadinessProbe {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        let spec = self.spec(request)?;
        let record = self.runtime.prepare(spec).map_err(|error| match error {
            RuntimeError::Conflict | RuntimeError::ContextMismatch => rejected(),
            // Validation already happened above. A failed read/save here may
            // concern an earlier dispatch: never infer that no POST happened.
            _ => unknown(),
        })?;
        if record.phase != OperationPhase::Prepared {
            return dispatched(record);
        }
        let remaining = (request.expires_at - Utc::now())
            .to_std()
            .map_err(|_| rejected())?;
        // The runtime checks Prepared and expiry again under its shared gate.
        // Concurrent callers cannot turn a second dispatch into a polling GET.
        let result = tokio::time::timeout(
            remaining,
            self.runtime.submit_prepared(&record.spec.operation_id),
        )
        .await
        .map_err(|_| unknown())?;
        let record = result.map_err(|error| match error {
            RuntimeError::Expired => rejected(),
            // This includes successful POST followed by checkpoint-save failure.
            // Preserve uncertainty even when the returned error is local storage.
            _ => unknown(),
        })?;
        dispatched(record)
    }
}
