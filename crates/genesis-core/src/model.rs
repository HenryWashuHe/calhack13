//! Domain entities (TECH_SPEC §3) and the organization aggregate.

use crate::ids::*;
use crate::lifecycle::*;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Who implements a role or runs a job.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Elastic Agent Builder on Serverless: internal specialists.
    Elastic,
    /// Fetch.ai uAgents: independent external specialists.
    Fetch,
    /// CREAO: persistent operational roles.
    Creao,
    /// Apify: executable capabilities (Actors). Never an employee.
    Apify,
    /// Implemented by the kernel itself (e.g. the mission controller).
    Internal,
    /// Development-only stand-in. The UI must label it as not a live integration.
    DevStub,
}

impl Provider {
    pub fn is_dev_stub(self) -> bool {
        self == Provider::DevStub
    }
}

/// Scopes and budget a role may exercise. Scopes are free-form strings such as
/// `"ledger.reserve_venue"`; the constitution lists which scopes exist.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuthorityGrant {
    #[serde(default)]
    pub scopes: BTreeSet<String>,
    #[serde(default)]
    pub budget_cents: u64,
}

impl AuthorityGrant {
    pub fn new<I, S>(scopes: I, budget_cents: u64) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            scopes: scopes.into_iter().map(Into::into).collect(),
            budget_cents,
        }
    }

    pub fn allows(&self, scope: &str) -> bool {
        self.scopes.contains(scope)
    }
}

/// Organization-wide limits. A role can never exceed these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Constitution {
    /// Every scope that may be granted inside this organization.
    pub scopes: BTreeSet<String>,
    /// Total budget that can be allocated across non-retired roles.
    pub budget_cents: u64,
    pub max_active_roles: u32,
    pub max_active_contracts: u32,
}

impl Default for Constitution {
    fn default() -> Self {
        Self {
            scopes: BTreeSet::new(),
            budget_cents: 0,
            max_active_roles: 12,
            max_active_contracts: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Mission {
    pub mission_id: MissionId,
    pub goal: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub success_checks: Vec<String>,
    pub status: MissionStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Observation {
    pub observation_id: ObservationId,
    pub mission_id: MissionId,
    /// Where the evidence came from, e.g. `"venue-portal"` or `"operator"`.
    pub source: String,
    pub summary: String,
    /// Pointer to raw evidence (URL, dataset ID, ...).
    pub evidence_ref: Option<String>,
    pub observed_at: DateTime<Utc>,
}

/// Accountable owner of a requirement or responsibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Owner {
    /// Unassigned work stays with the mission controller.
    MissionController,
    Role(RoleId),
}

impl Owner {
    pub fn role(&self) -> Option<&RoleId> {
        match self {
            Owner::Role(id) => Some(id),
            Owner::MissionController => None,
        }
    }
}

/// Who currently executes a responsibility. Delegation changes the executor,
/// never the accountable owner.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Executor {
    Role(RoleId),
    Contractor(ContractId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Requirement {
    pub requirement_id: RequirementId,
    pub mission_id: MissionId,
    pub summary: String,
    /// Observations that caused this requirement. Must be non-empty.
    pub triggering_evidence: Vec<ObservationId>,
    /// Predicate that tells when the requirement is satisfied.
    pub acceptance: String,
    pub status: RequirementStatus,
    pub owner: Owner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Responsibility {
    pub responsibility_id: ResponsibilityId,
    pub mission_id: MissionId,
    pub purpose: String,
    pub requirement_id: Option<RequirementId>,
    /// Exactly one accountable owner.
    pub owner: Owner,
    pub executor: Option<Executor>,
    /// What wakes this responsibility up (event, schedule, ...).
    pub trigger: String,
    /// When the responsibility ends.
    pub termination: String,
    pub status: ResponsibilityStatus,
    /// Ownership epoch. Advanced on every transfer; stale results are fenced.
    pub epoch: u64,
}

/// Provider-side implementation of a role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentBinding {
    pub provider: Provider,
    /// Provider's own identifier (Elastic agent ID, Fetch address, CREAO agent ID).
    pub provider_agent_id: String,
    /// Digest of the effective configuration sent to the provider.
    pub config_digest: String,
    pub conversation_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Role {
    pub role_id: RoleId,
    pub name: String,
    pub charter: String,
    pub authority: AuthorityGrant,
    pub reports_to: Option<RoleId>,
    pub state: RoleState,
    pub binding: Option<AgentBinding>,
    /// Exact capability versions attached to this role.
    #[serde(default)]
    pub capabilities: Vec<CapabilityRef>,
    pub created_in_revision: u64,
    pub retired_in_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Assignment {
    pub assignment_id: AssignmentId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    /// Copy of the responsibility epoch at assignment time.
    pub epoch: u64,
    pub state: AssignmentState,
    pub lease_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContractorAgreement {
    pub contract_id: ContractId,
    /// Agentverse address of the independent specialist.
    pub specialist_address: String,
    pub protocol: String,
    pub scope: String,
    pub deadline: Option<DateTime<Utc>>,
    pub state: ContractState,
    /// Role that supervises the contractor, if any.
    pub supervising_role: Option<RoleId>,
}

/// Exact reference to an approved capability version. There is no `latest`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct CapabilityRef {
    pub capability_id: CapabilityId,
    pub version: String,
}

impl CapabilityRef {
    pub fn new(capability_id: impl Into<CapabilityId>, version: impl Into<String>) -> Self {
        Self {
            capability_id: capability_id.into(),
            version: version.into(),
        }
    }
}

/// Org-local installation of a capability version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Installation {
    pub installation_id: InstallationId,
    pub capability: CapabilityRef,
    pub status: InstallationStatus,
    /// Local configuration supplied by this organization.
    #[serde(default)]
    pub configuration: serde_json::Value,
    /// Reference to the local acceptance report, once run.
    pub acceptance_report_ref: Option<String>,
    pub source_digest: String,
    pub installed_at: DateTime<Utc>,
}

/// The organization aggregate. One serialized copy is stored per revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrgState {
    pub org_id: OrgId,
    pub name: String,
    pub revision: u64,
    pub constitution: Constitution,
    #[serde(default)]
    pub missions: BTreeMap<MissionId, Mission>,
    #[serde(default)]
    pub observations: BTreeMap<ObservationId, Observation>,
    #[serde(default)]
    pub requirements: BTreeMap<RequirementId, Requirement>,
    #[serde(default)]
    pub responsibilities: BTreeMap<ResponsibilityId, Responsibility>,
    #[serde(default)]
    pub roles: BTreeMap<RoleId, Role>,
    #[serde(default)]
    pub assignments: BTreeMap<AssignmentId, Assignment>,
    #[serde(default)]
    pub contracts: BTreeMap<ContractId, ContractorAgreement>,
    #[serde(default)]
    pub installations: BTreeMap<InstallationId, Installation>,
}

impl OrgState {
    pub fn new(org_id: OrgId, name: impl Into<String>, constitution: Constitution) -> Self {
        Self {
            org_id,
            name: name.into(),
            revision: 0,
            constitution,
            missions: BTreeMap::new(),
            observations: BTreeMap::new(),
            requirements: BTreeMap::new(),
            responsibilities: BTreeMap::new(),
            roles: BTreeMap::new(),
            assignments: BTreeMap::new(),
            contracts: BTreeMap::new(),
            installations: BTreeMap::new(),
        }
    }

    /// Roles that count against limits (everything not retired or failed).
    pub fn live_roles(&self) -> impl Iterator<Item = &Role> {
        self.roles
            .values()
            .filter(|r| !matches!(r.state, RoleState::Retired | RoleState::ProvisioningFailed))
    }

    pub fn allocated_budget_cents(&self) -> u64 {
        self.live_roles().map(|r| r.authority.budget_cents).sum()
    }

    pub fn active_contracts(&self) -> usize {
        self.contracts
            .values()
            .filter(|c| {
                matches!(
                    c.state,
                    ContractState::Offered | ContractState::Accepted | ContractState::Requested
                )
            })
            .count()
    }

    /// Open responsibilities whose accountable owner is `role`.
    pub fn open_responsibilities_of(&self, role: &RoleId) -> Vec<&Responsibility> {
        self.responsibilities
            .values()
            .filter(|r| r.status == ResponsibilityStatus::Open && r.owner.role() == Some(role))
            .collect()
    }

    /// The single active assignment for a responsibility, if any.
    pub fn active_assignment(&self, responsibility: &ResponsibilityId) -> Option<&Assignment> {
        self.assignments
            .values()
            .find(|a| &a.responsibility_id == responsibility && a.state == AssignmentState::Active)
    }

    /// Whether `role` is an ancestor of `of` in the reporting tree.
    pub fn reports_transitively_to(&self, of: &RoleId, role: &RoleId) -> bool {
        let mut cursor = self.roles.get(of).and_then(|r| r.reports_to.clone());
        let mut hops = 0;
        while let Some(current) = cursor {
            if &current == role {
                return true;
            }
            hops += 1;
            if hops > self.roles.len() {
                return true; // existing cycle: treat as reaching to stay safe
            }
            cursor = self.roles.get(&current).and_then(|r| r.reports_to.clone());
        }
        false
    }
}
