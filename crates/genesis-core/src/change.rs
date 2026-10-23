//! Organization-change proposals (TECH_SPEC §5).
//!
//! An agent (normally the Elastic mission agent) submits an
//! [`OrgChangeProposal`]. The kernel treats it as untrusted input: it is
//! validated by [`crate::engine::validate`], provisioned as a saga, and only
//! then committed in one transaction by [`crate::engine::commit`].

use crate::ids::*;
use crate::model::{AuthorityGrant, CapabilityRef, Provider};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Refers to an existing role or to a role created earlier in the same proposal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoleRef {
    Existing { role_id: RoleId },
    New { key: String },
}

impl RoleRef {
    pub fn existing(id: impl Into<RoleId>) -> Self {
        RoleRef::Existing { role_id: id.into() }
    }
    pub fn new_key(key: impl Into<String>) -> Self {
        RoleRef::New { key: key.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponsibilityRef {
    Existing { responsibility_id: ResponsibilityId },
    New { key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContractRef {
    Existing { contract_id: ContractId },
    New { key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutorRef {
    Role { role: RoleRef },
    Contractor { contract: ContractRef },
}

/// Why a separate role is needed instead of reusing one (TECH_SPEC §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleJustification {
    Concurrency,
    Authority,
    IndependentOwnership,
    Expertise,
    Lifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponsibilityTarget {
    /// An existing responsibility currently held by the mission controller.
    Existing { responsibility_id: ResponsibilityId },
    /// A new continuing responsibility derived from an unmet requirement.
    FromRequirement {
        key: String,
        requirement_id: RequirementId,
        purpose: String,
        trigger: String,
        termination: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Handoff {
    pub responsibility_id: ResponsibilityId,
    pub to: RoleRef,
}

/// The mutation vocabulary. Names match TECH_SPEC §5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op")]
pub enum OrgOp {
    CreateRole {
        key: String,
        name: String,
        charter: String,
        authority: AuthorityGrant,
        reports_to: Option<RoleRef>,
        provider: Provider,
        justification: RoleJustification,
    },
    RecruitSpecialist {
        key: String,
        specialist_address: String,
        protocol: String,
        scope: String,
        deadline: Option<DateTime<Utc>>,
        supervising_role: Option<RoleRef>,
    },
    AssignResponsibility {
        target: ResponsibilityTarget,
        owner: RoleRef,
    },
    DelegateTask {
        responsibility: ResponsibilityRef,
        executor: ExecutorRef,
        task: String,
    },
    ReassignResponsibility {
        responsibility_id: ResponsibilityId,
        from: RoleId,
        to: RoleRef,
    },
    AttachCapability {
        role: RoleRef,
        capability: CapabilityRef,
    },
    ChangeReportingEdge {
        role: RoleRef,
        reports_to: Option<RoleRef>,
    },
    RetireRole {
        role_id: RoleId,
        #[serde(default)]
        handoffs: Vec<Handoff>,
    },
}

impl OrgOp {
    pub fn name(&self) -> &'static str {
        match self {
            OrgOp::CreateRole { .. } => "CreateRole",
            OrgOp::RecruitSpecialist { .. } => "RecruitSpecialist",
            OrgOp::AssignResponsibility { .. } => "AssignResponsibility",
            OrgOp::DelegateTask { .. } => "DelegateTask",
            OrgOp::ReassignResponsibility { .. } => "ReassignResponsibility",
            OrgOp::AttachCapability { .. } => "AttachCapability",
            OrgOp::ChangeReportingEdge { .. } => "ChangeReportingEdge",
            OrgOp::RetireRole { .. } => "RetireRole",
        }
    }
}

/// A structured, untrusted proposal to change the organization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OrgChangeProposal {
    /// Revision the proposer reasoned about. A mismatch is a conflict.
    pub expected_org_revision: u64,
    /// Observations that justify the change. Must be non-empty and known.
    pub trigger_evidence_ids: Vec<ObservationId>,
    pub rationale: String,
    pub operations: Vec<OrgOp>,
    /// Optional cap on new budget this change may allocate.
    pub budget_limit_cents: Option<u64>,
}

/// Which keys in a proposal map to which freshly allocated IDs. Allocated once
/// at validation so that commit produces the same IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KeyAllocation {
    #[serde(default)]
    pub roles: std::collections::BTreeMap<String, RoleId>,
    #[serde(default)]
    pub responsibilities: std::collections::BTreeMap<String, ResponsibilityId>,
    #[serde(default)]
    pub contracts: std::collections::BTreeMap<String, ContractId>,
}

impl KeyAllocation {
    /// Allocate IDs for every `key` introduced by the proposal. Duplicate keys
    /// are reported by validation, so here the first occurrence wins.
    pub fn allocate(proposal: &OrgChangeProposal) -> Self {
        let mut alloc = KeyAllocation::default();
        for op in &proposal.operations {
            match op {
                OrgOp::CreateRole { key, .. } => {
                    alloc
                        .roles
                        .entry(key.clone())
                        .or_insert_with(RoleId::generate);
                }
                OrgOp::RecruitSpecialist { key, .. } => {
                    alloc
                        .contracts
                        .entry(key.clone())
                        .or_insert_with(ContractId::generate);
                }
                OrgOp::AssignResponsibility {
                    target: ResponsibilityTarget::FromRequirement { key, .. },
                    ..
                } => {
                    alloc
                        .responsibilities
                        .entry(key.clone())
                        .or_insert_with(ResponsibilityId::generate);
                }
                _ => {}
            }
        }
        alloc
    }
}

/// A stored organization change with its lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrgChangeRecord {
    pub change_id: ChangeId,
    pub status: crate::lifecycle::ChangeStatus,
    pub proposal: OrgChangeProposal,
    pub allocation: KeyAllocation,
    /// Provider bindings obtained during provisioning, keyed by role key.
    #[serde(default)]
    pub provisioned: std::collections::BTreeMap<String, crate::model::AgentBinding>,
    #[serde(default)]
    pub errors: Vec<crate::engine::ValidationError>,
    pub committed_revision: Option<u64>,
    pub submitted_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
