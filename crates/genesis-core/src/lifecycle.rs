//! Lifecycle state machines.
//!
//! Each lifecycle enum lists its allowed transitions explicitly. Callers move
//! between states with [`StateMachine::transition`], which rejects anything not
//! in the table. The tables mirror TECH_SPEC sections 5, 6 and 8.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// A transition that the lifecycle table does not allow.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, JsonSchema)]
#[error("{machine}: transition {from} -> {to} is not allowed")]
pub struct TransitionError {
    pub machine: String,
    pub from: String,
    pub to: String,
}

pub trait StateMachine: Copy + Debug + PartialEq + Sized + 'static {
    const NAME: &'static str;

    /// States reachable in one step from `self`.
    fn allowed_next(self) -> &'static [Self];

    /// Whether no further transitions are possible.
    fn is_terminal(self) -> bool {
        self.allowed_next().is_empty()
    }

    fn can_transition_to(self, to: Self) -> bool {
        self.allowed_next().contains(&to)
    }

    fn transition(self, to: Self) -> Result<Self, TransitionError> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(TransitionError {
                machine: Self::NAME.to_owned(),
                from: format!("{self:?}"),
                to: format!("{to:?}"),
            })
        }
    }
}

/// Role lifecycle (TECH_SPEC §5).
///
/// `PROPOSED -> PROVISIONING -> READY -> ACTIVE -> DRAINING -> RETIRED`, with
/// `PROVISIONING_FAILED` as the failure exit of provisioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoleState {
    Proposed,
    Provisioning,
    Ready,
    Active,
    Draining,
    Retired,
    ProvisioningFailed,
}

impl StateMachine for RoleState {
    const NAME: &'static str = "role";
    fn allowed_next(self) -> &'static [Self] {
        use RoleState::*;
        match self {
            Proposed => &[Provisioning],
            Provisioning => &[Ready, ProvisioningFailed],
            Ready => &[Active],
            Active => &[Draining],
            Draining => &[Retired],
            Retired | ProvisioningFailed => &[],
        }
    }
}

impl RoleState {
    /// Only active roles may receive dispatches or new assignments.
    pub fn accepts_work(self) -> bool {
        self == RoleState::Active
    }
}

/// Organization-change lifecycle (TECH_SPEC §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeStatus {
    Proposed,
    Validated,
    Provisioning,
    Ready,
    Committed,
    Rejected,
    Failed,
}

impl StateMachine for ChangeStatus {
    const NAME: &'static str = "org_change";
    fn allowed_next(self) -> &'static [Self] {
        use ChangeStatus::*;
        match self {
            Proposed => &[Validated, Rejected],
            Validated => &[Provisioning, Rejected],
            Provisioning => &[Ready, Failed],
            // A revision conflict discovered at commit time rejects the change.
            Ready => &[Committed, Rejected, Failed],
            Committed | Rejected | Failed => &[],
        }
    }
}

/// Assignment of a role to a responsibility holder, fenced by epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssignmentState {
    Active,
    /// Superseded by a newer epoch; results from it are rejected.
    Fenced,
    Released,
}

impl StateMachine for AssignmentState {
    const NAME: &'static str = "assignment";
    fn allowed_next(self) -> &'static [Self] {
        use AssignmentState::*;
        match self {
            Active => &[Fenced, Released],
            Fenced => &[Released],
            Released => &[],
        }
    }
}

/// Normalized job status (TECH_SPEC §6). Provider-native statuses are kept
/// separately on the job record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Queued,
    Submitting,
    Running,
    Verifying,
    Succeeded,
    Failed,
    ResultRejected,
    /// The outcome is not known (e.g. a dropped stream). Never treated as success.
    Unknown,
}

impl StateMachine for JobStatus {
    const NAME: &'static str = "job";
    fn allowed_next(self) -> &'static [Self] {
        use JobStatus::*;
        match self {
            Queued => &[Submitting, Failed],
            Submitting => &[Running, Failed, Unknown],
            Running => &[Verifying, Failed, Unknown, ResultRejected],
            Verifying => &[Succeeded, ResultRejected, Failed],
            // Reconciliation can resolve an unknown outcome.
            Unknown => &[Running, Verifying, Failed, ResultRejected],
            Succeeded | Failed | ResultRejected => &[],
        }
    }
}

/// Capability version lifecycle (TECH_SPEC §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityState {
    Requested,
    Synthesizing,
    Building,
    Testing,
    Approved,
    Published,
    Rejected,
}

impl StateMachine for CapabilityState {
    const NAME: &'static str = "capability";
    fn allowed_next(self) -> &'static [Self] {
        use CapabilityState::*;
        match self {
            Requested => &[Synthesizing, Rejected],
            Synthesizing => &[Building, Rejected],
            Building => &[Testing, Rejected],
            Testing => &[Approved, Rejected],
            Approved => &[Published],
            Published | Rejected => &[],
        }
    }
}

impl CapabilityState {
    /// Only approved or published versions may be attached or installed.
    pub fn is_usable(self) -> bool {
        matches!(self, CapabilityState::Approved | CapabilityState::Published)
    }
}

/// External contractor agreement (Fetch specialist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContractState {
    Requested,
    Offered,
    Accepted,
    Released,
    Declined,
}

impl StateMachine for ContractState {
    const NAME: &'static str = "contract";
    fn allowed_next(self) -> &'static [Self] {
        use ContractState::*;
        match self {
            Requested => &[Offered, Declined],
            Offered => &[Accepted, Declined],
            Accepted => &[Released],
            Released | Declined => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResponsibilityStatus {
    Open,
    Completed,
    Cancelled,
}

impl StateMachine for ResponsibilityStatus {
    const NAME: &'static str = "responsibility";
    fn allowed_next(self) -> &'static [Self] {
        use ResponsibilityStatus::*;
        match self {
            Open => &[Completed, Cancelled],
            Completed | Cancelled => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequirementStatus {
    /// Owned by the mission controller until assigned.
    Unmet,
    Assigned,
    Satisfied,
}

impl StateMachine for RequirementStatus {
    const NAME: &'static str = "requirement";
    fn allowed_next(self) -> &'static [Self] {
        use RequirementStatus::*;
        match self {
            Unmet => &[Assigned, Satisfied],
            Assigned => &[Satisfied],
            Satisfied => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MissionStatus {
    Active,
    Succeeded,
    Failed,
}

impl StateMachine for MissionStatus {
    const NAME: &'static str = "mission";
    fn allowed_next(self) -> &'static [Self] {
        use MissionStatus::*;
        match self {
            Active => &[Succeeded, Failed],
            Succeeded | Failed => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallationStatus {
    /// Manifest fetched; compatibility passed; local acceptance pending.
    Checking,
    Accepted,
    Rejected,
    Uninstalled,
}

impl StateMachine for InstallationStatus {
    const NAME: &'static str = "installation";
    fn allowed_next(self) -> &'static [Self] {
        use InstallationStatus::*;
        match self {
            Checking => &[Accepted, Rejected],
            Accepted => &[Uninstalled],
            Rejected | Uninstalled => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_lifecycle_happy_path() {
        let mut s = RoleState::Proposed;
        for next in [
            RoleState::Provisioning,
            RoleState::Ready,
            RoleState::Active,
            RoleState::Draining,
            RoleState::Retired,
        ] {
            s = s.transition(next).unwrap();
        }
        assert!(s.is_terminal());
    }

    #[test]
    fn role_cannot_skip_draining() {
        let err = RoleState::Active
            .transition(RoleState::Retired)
            .unwrap_err();
        assert_eq!(err.from, "Active");
        assert_eq!(err.to, "Retired");
    }

    #[test]
    fn provisioning_failure_is_terminal_and_never_active() {
        let s = RoleState::Provisioning
            .transition(RoleState::ProvisioningFailed)
            .unwrap();
        assert!(s.is_terminal());
        assert!(!s.accepts_work());
    }

    #[test]
    fn unknown_job_is_not_success() {
        assert!(!JobStatus::Unknown.is_terminal());
        assert!(!JobStatus::Unknown.can_transition_to(JobStatus::Succeeded));
    }

    #[test]
    fn only_approved_capabilities_are_usable() {
        assert!(!CapabilityState::Testing.is_usable());
        assert!(!CapabilityState::Rejected.is_usable());
        assert!(CapabilityState::Approved.is_usable());
        assert!(CapabilityState::Published.is_usable());
    }

    #[test]
    fn states_serialize_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&RoleState::ProvisioningFailed).unwrap(),
            "\"PROVISIONING_FAILED\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::ResultRejected).unwrap(),
            "\"RESULT_REJECTED\""
        );
    }
}
