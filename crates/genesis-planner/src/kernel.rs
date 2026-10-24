//! HTTP client for one organization kernel, using the restricted agent token.

use genesis_core::change::{OrgChangeProposal, OrgChangeRecord};
use genesis_core::events::{DomainEvent, PlannerVerdict};
use genesis_core::ids::*;
use genesis_core::model::OrgState;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("kernel unreachable: {0}")]
    Transport(String),
    /// Structured kernel rejection (`error` code, message, details).
    #[error("kernel returned {status} {code}: {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
        details: Value,
    },
    #[error("unexpected kernel response: {0}")]
    Decode(String),
}

impl KernelError {
    /// The request may or may not have taken effect (transport failure or a
    /// server error). Reconcile by reading before any new mutation.
    pub fn is_ambiguous(&self) -> bool {
        match self {
            KernelError::Transport(_) | KernelError::Decode(_) => true,
            KernelError::Api { status, .. } => *status >= 500,
        }
    }

    /// A definite rejection of the request's content (4xx with a code).
    pub fn is_rejection(&self) -> bool {
        matches!(self, KernelError::Api { status, .. } if (400..500).contains(status))
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            KernelError::Api { code, .. } => Some(code),
            _ => None,
        }
    }

    /// Human-readable list of validation errors, for feeding back to a model.
    pub fn reasons(&self) -> Vec<String> {
        match self {
            KernelError::Api {
                details, message, ..
            } => {
                let errs = details
                    .get("errors")
                    .or(Some(details))
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|e| {
                                format!(
                                    "{}{}: {}",
                                    e["code"].as_str().unwrap_or("error"),
                                    e["op_index"]
                                        .as_u64()
                                        .map(|i| format!(" (operation {i})"))
                                        .unwrap_or_default(),
                                    e["message"].as_str().unwrap_or("")
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if errs.is_empty() {
                    vec![message.clone()]
                } else {
                    errs
                }
            }
            other => vec![other.to_string()],
        }
    }
}

#[derive(Clone)]
pub struct KernelClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

/// Decision record sent to `POST /api/v1/planner/decisions`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionRecord {
    pub planner: String,
    pub model: String,
    pub observation_id: ObservationId,
    pub verdict: PlannerVerdict,
    pub summary: String,
    pub rationale: String,
    pub attempt: u32,
    pub latency_ms: u64,
    pub requirement_id: Option<RequirementId>,
    pub change_id: Option<ChangeId>,
    pub kernel_errors: Vec<String>,
}

impl KernelClient {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            token: token.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    async fn send<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, KernelError> {
        let resp = req
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| KernelError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| KernelError::Transport(e.to_string()))?;
        if status.is_success() {
            serde_json::from_str(&text).map_err(|e| KernelError::Decode(e.to_string()))
        } else {
            let v: Value = serde_json::from_str(&text).unwrap_or_default();
            Err(KernelError::Api {
                status: status.as_u16(),
                code: v["error"].as_str().unwrap_or("http_error").to_owned(),
                message: v["message"].as_str().unwrap_or(&text).to_owned(),
                details: v.get("details").cloned().unwrap_or(Value::Null),
            })
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base, path)
    }

    pub async fn state(&self) -> Result<OrgState, KernelError> {
        self.send(self.http.get(self.url("/organization"))).await
    }

    /// Capabilities the server's trusted gap policy can act on, with the
    /// public sources a request must list. Servers without the route (or
    /// without a policy) yield an empty list.
    pub async fn requestable_capabilities(
        &self,
    ) -> Result<Vec<crate::prompt::Requestable>, KernelError> {
        #[derive(serde::Deserialize)]
        struct Response {
            capabilities: Vec<crate::prompt::Requestable>,
        }
        match self
            .send::<Response>(self.http.get(self.url("/capability-gaps/requestable")))
            .await
        {
            Ok(r) => Ok(r.capabilities),
            Err(KernelError::Api { status: 404, .. }) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    pub async fn events_after(
        &self,
        after: u64,
        limit: u32,
    ) -> Result<Vec<DomainEvent>, KernelError> {
        self.send(
            self.http
                .get(self.url(&format!("/events/log?after={after}&limit={limit}"))),
        )
        .await
    }

    pub async fn record_requirement(
        &self,
        mission: &MissionId,
        summary: &str,
        evidence: &[ObservationId],
        acceptance: &str,
    ) -> Result<RequirementId, KernelError> {
        self.post_requirement(
            mission,
            json!({"summary": summary, "evidence": evidence, "acceptance": acceptance}),
        )
        .await
    }

    /// Record a requirement under an idempotency identity. Resending the same
    /// `client_request_id` and body returns the requirement it created, so an
    /// ambiguous outcome can be retried without creating a duplicate.
    pub async fn record_requirement_once(
        &self,
        client_request_id: &str,
        mission: &MissionId,
        summary: &str,
        evidence: &[ObservationId],
        acceptance: &str,
    ) -> Result<RequirementId, KernelError> {
        self.post_requirement(
            mission,
            json!({"summary": summary, "evidence": evidence, "acceptance": acceptance,
                   "client_request_id": client_request_id}),
        )
        .await
    }

    async fn post_requirement(
        &self,
        mission: &MissionId,
        body: Value,
    ) -> Result<RequirementId, KernelError> {
        let v: Value = self
            .send(
                self.http
                    .post(self.url(&format!("/missions/{mission}/requirements")))
                    .json(&body),
            )
            .await?;
        v["requirement_id"]
            .as_str()
            .map(RequirementId::new)
            .ok_or_else(|| KernelError::Decode("missing requirement_id".into()))
    }

    pub async fn submit_change(
        &self,
        client_request_id: &str,
        proposal: &OrgChangeProposal,
    ) -> Result<OrgChangeRecord, KernelError> {
        self.send(
            self.http
                .post(self.url("/org-changes"))
                .json(&json!({"client_request_id": client_request_id, "proposal": proposal})),
        )
        .await
    }

    /// The change submitted with `client_request_id`, if any.
    pub async fn change_by_request(
        &self,
        client_request_id: &str,
    ) -> Result<Option<OrgChangeRecord>, KernelError> {
        let encoded: String = client_request_id
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect();
        match self
            .send(
                self.http
                    .get(self.url(&format!("/org-changes/requests/{encoded}"))),
            )
            .await
        {
            Ok(r) => Ok(Some(r)),
            Err(KernelError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn change(&self, id: &ChangeId) -> Result<OrgChangeRecord, KernelError> {
        self.send(self.http.get(self.url(&format!("/org-changes/{id}"))))
            .await
    }

    pub async fn record_decision(&self, d: &DecisionRecord) -> Result<(), KernelError> {
        let _: Value = self
            .send(self.http.post(self.url("/planner/decisions")).json(d))
            .await?;
        Ok(())
    }
}
