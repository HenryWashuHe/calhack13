//! Job records (TECH_SPEC §6).

use crate::ids::*;
use crate::lifecycle::{JobStatus, StateMachine, TransitionError};
use crate::model::{CapabilityRef, Provider};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Every dispatch carries this envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Job {
    pub job_id: JobId,
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub assignment_id: AssignmentId,
    /// Responsibility epoch at dispatch time. Results must carry the same epoch.
    pub epoch: u64,
    pub task: String,
    #[serde(default)]
    pub input: serde_json::Value,
    pub attempt_id: AttemptId,
    pub attempt_no: u32,
    pub correlation_id: CorrelationId,
    pub idempotency_key: String,
    pub deadline: Option<DateTime<Utc>>,
    pub capability: Option<CapabilityRef>,
    pub provider: Option<Provider>,
    pub status: JobStatus,
    /// Provider identifiers (run ID, dataset ID, conversation ID, ...).
    #[serde(default)]
    pub provider_refs: BTreeMap<String, String>,
    /// Last provider-native status string, preserved for diagnosis.
    pub raw_status: Option<String>,
    pub result: Option<serde_json::Value>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Job {
    pub fn set_status(
        &mut self,
        to: JobStatus,
        now: DateTime<Utc>,
    ) -> Result<JobStatus, TransitionError> {
        let from = self.status;
        self.status = from.transition(to)?;
        self.updated_at = now;
        Ok(from)
    }
}

/// Body of `POST /api/v1/jobs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DispatchRequest {
    /// Client request ID for deduplication.
    pub client_request_id: String,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub task: String,
    #[serde(default)]
    pub input: serde_json::Value,
    /// Scope required for this work, checked against the role's authority.
    pub required_scope: Option<String>,
    pub capability: Option<CapabilityRef>,
    pub provider: Option<Provider>,
    pub deadline: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultOutcome {
    Succeeded,
    Failed,
    Unknown,
}

/// Body of `POST /api/v1/jobs/{id}/results`, sent by provider adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobResultSubmission {
    pub attempt_id: AttemptId,
    pub epoch: u64,
    pub outcome: ResultOutcome,
    /// Provider-native status, stored verbatim.
    pub provider_status: String,
    #[serde(default)]
    pub provider_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub output: serde_json::Value,
    /// Logical ledger operation. Duplicate deliveries with the same ID produce
    /// one ledger effect.
    pub operation_id: Option<OperationId>,
    /// Domain verification of the output, done by the caller's checker.
    /// Provider success without domain verification never marks success.
    pub domain_verified: bool,
    /// Mark the responsibility complete when this result is accepted.
    #[serde(default)]
    pub completes_responsibility: bool,
}
