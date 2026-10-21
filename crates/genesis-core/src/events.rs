//! Domain events. The kernel appends these to a durable log; the UI and other
//! consumers read them in sequence order.

use crate::ids::*;
use crate::lifecycle::{ChangeStatus, ContractState, JobStatus};
use crate::model::{CapabilityRef, Executor, Provider};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Envelope stored in the event log (TEAM_WORKSTREAMS "Domain event").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DomainEvent {
    pub schema_version: u32,
    /// Durable sequence assigned by the store. Used as the SSE cursor.
    pub seq: u64,
    pub event_id: EventId,
    pub org_id: OrgId,
    pub mission_id: Option<MissionId>,
    pub correlation_id: Option<CorrelationId>,
    /// Event or request that caused this event.
    pub causation_id: Option<String>,
    /// Organization revision after this event was applied.
    pub revision: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EventKind {
    OrganizationInitialized {
        name: String,
    },
    MissionCreated {
        mission_id: MissionId,
        goal: String,
    },
    ObservationRecorded {
        observation_id: ObservationId,
        source: String,
        summary: String,
    },
    RequirementRecorded {
        requirement_id: RequirementId,
        summary: String,
        evidence: Vec<ObservationId>,
    },
    ChangeProposed {
        change_id: ChangeId,
        expected_revision: u64,
        operations: Vec<String>,
        rationale: String,
    },
    ChangeStatusChanged {
        change_id: ChangeId,
        from: ChangeStatus,
        to: ChangeStatus,
        #[serde(default)]
        reasons: Vec<String>,
    },
    RoleProvisioned {
        change_id: ChangeId,
        role_key: String,
        role_id: RoleId,
        provider: Provider,
        provider_agent_id: String,
        /// True when a development stub stood in for the real provider.
        dev_stub: bool,
    },
    ProvisioningFailed {
        change_id: ChangeId,
        role_key: String,
        error: String,
    },
    RoleCreated {
        role_id: RoleId,
        name: String,
        provider: Provider,
    },
    RoleActivated {
        role_id: RoleId,
    },
    ResponsibilityCreated {
        responsibility_id: ResponsibilityId,
        purpose: String,
    },
    ResponsibilityAssigned {
        responsibility_id: ResponsibilityId,
        owner: RoleId,
        epoch: u64,
    },
    ResponsibilityTransferred {
        responsibility_id: ResponsibilityId,
        from: RoleId,
        to: RoleId,
        epoch: u64,
    },
    ResponsibilityCompleted {
        responsibility_id: ResponsibilityId,
    },
    TaskDelegated {
        responsibility_id: ResponsibilityId,
        executor: Executor,
        task: String,
    },
    ContractRequested {
        contract_id: ContractId,
        specialist_address: String,
        scope: String,
    },
    ContractStateChanged {
        contract_id: ContractId,
        from: ContractState,
        to: ContractState,
    },
    CapabilityAttached {
        role_id: RoleId,
        capability: CapabilityRef,
    },
    ReportingEdgeChanged {
        role_id: RoleId,
        reports_to: Option<RoleId>,
    },
    RoleDraining {
        role_id: RoleId,
    },
    RoleRetired {
        role_id: RoleId,
    },
    AssignmentFenced {
        assignment_id: AssignmentId,
        responsibility_id: ResponsibilityId,
        epoch: u64,
    },
    JobDispatched {
        job_id: JobId,
        role_id: RoleId,
        responsibility_id: ResponsibilityId,
        epoch: u64,
        task: String,
    },
    JobStatusChanged {
        job_id: JobId,
        from: JobStatus,
        to: JobStatus,
    },
    JobResultRejected {
        job_id: JobId,
        reason: String,
    },
    CommitmentRecorded {
        operation_id: OperationId,
        job_id: JobId,
    },
    CapabilityInstalled {
        installation_id: InstallationId,
        capability: CapabilityRef,
        source_digest: String,
        /// Number of source-generation calls made to install. Must be 0 for
        /// compatible adoption.
        synthesis_calls: u32,
    },
    InstallationAccepted {
        installation_id: InstallationId,
        report_ref: String,
    },
    InstallationRejected {
        installation_id: InstallationId,
        reasons: Vec<String>,
    },
    CapabilityRunRequested {
        installation_id: InstallationId,
        capability: CapabilityRef,
        role_id: RoleId,
        job_id: JobId,
    },
}

impl EventKind {
    /// The snake_case type tag, as stored in the `event_type` column.
    pub fn type_name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_flattens_type_and_payload() {
        let ev = DomainEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            seq: 7,
            event_id: EventId::new("evt_1"),
            org_id: OrgId::new("org_a"),
            mission_id: None,
            correlation_id: None,
            causation_id: None,
            revision: 3,
            timestamp: Utc::now(),
            kind: EventKind::RoleActivated {
                role_id: RoleId::new("role_x"),
            },
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "role_activated");
        assert_eq!(v["payload"]["role_id"], "role_x");
        let back: DomainEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, ev);
        assert_eq!(ev.kind.type_name(), "role_activated");
    }
}
