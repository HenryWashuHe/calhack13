//! Pure organization engine: validation, commit, and the other state
//! transitions the kernel performs.
//!
//! Everything here is deterministic and side-effect free: functions take the
//! current [`OrgState`] and return a new state plus the [`EventKind`]s that
//! describe what changed. Persistence, provider calls and event sequencing
//! live in the server crate.

use crate::capability::{CapabilityCatalog, CapabilityManifest};
use crate::change::*;
use crate::events::EventKind;
use crate::ids::*;
use crate::lifecycle::*;
use crate::model::*;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCode {
    EmptyChange,
    RevisionConflict,
    MissingEvidence,
    UnknownEvidence,
    DuplicateKey,
    UnknownKey,
    UnknownRole,
    UnknownResponsibility,
    UnknownRequirement,
    UnknownContract,
    UnknownMission,
    RoleNotActive,
    AuthorityExceedsConstitution,
    BudgetExceeded,
    ChangeBudgetExceeded,
    RoleLimitExceeded,
    ContractLimitExceeded,
    AccountabilityConflict,
    ResponsibilityNotOpen,
    RequirementNotUnmet,
    CapabilityUnknown,
    CapabilityNotApproved,
    ReportingCycle,
    RetirementDropsObligations,
    NewRoleWithoutResponsibility,
    NotProvisioned,
    InvalidTransition,
    InvalidInput,
    CapabilityGapLimitExceeded,
}

/// Live (non-terminal) capability gaps an organization may hold at once.
/// Each can become an acquisition; the agent must not be able to flood it.
pub const MAX_LIVE_CAPABILITY_GAPS: usize = 16;
/// Live capability gaps one requirement may hold at once.
pub const MAX_LIVE_GAPS_PER_REQUIREMENT: usize = 3;
/// Refused capability requests after which a requirement may not request any
/// capability again. A refusal wakes the planner; without this bound a planner
/// that keeps re-requesting (a model loop, or injected portal text), under the
/// same or a fresh capability name, would spend one model call and two commits
/// per cycle forever. Changed circumstances arrive as new evidence, hence a new
/// requirement, which starts fresh.
pub const MAX_REFUSED_REQUESTS_PER_REQUIREMENT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{code:?}{}: {message}", op_index.map(|i| format!(" (operation {i})")).unwrap_or_default())]
pub struct ValidationError {
    pub code: ValidationCode,
    /// Index of the offending operation in the proposal, if any.
    pub op_index: Option<usize>,
    pub message: String,
}

impl ValidationError {
    pub fn new(code: ValidationCode, op_index: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            code,
            op_index,
            message: message.into(),
        }
    }
}

/// Result of a successful engine step.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub state: OrgState,
    pub effects: Vec<EventKind>,
}

// ---------------------------------------------------------------------------
// Organization-change protocol
// ---------------------------------------------------------------------------

/// Validate a proposal against the current state. On success the returned
/// state is a *preview*: new roles are in `PROVISIONING` and hold no binding.
/// The revision is not advanced.
pub fn validate(
    state: &OrgState,
    proposal: &OrgChangeProposal,
    alloc: &KeyAllocation,
    catalog: &dyn CapabilityCatalog,
    now: DateTime<Utc>,
) -> Result<Outcome, Vec<ValidationError>> {
    Sim::new(state, alloc, catalog, None, now).run(proposal)
}

/// Commit a provisioned proposal. Re-runs every check against the current
/// state (which must still be at `expected_org_revision`), activates new roles
/// with their provider bindings, and advances the revision by one.
pub fn commit(
    state: &OrgState,
    proposal: &OrgChangeProposal,
    alloc: &KeyAllocation,
    bindings: &BTreeMap<String, AgentBinding>,
    catalog: &dyn CapabilityCatalog,
    now: DateTime<Utc>,
) -> Result<Outcome, Vec<ValidationError>> {
    let mut outcome = Sim::new(state, alloc, catalog, Some(bindings), now).run(proposal)?;
    outcome.state.revision = state.revision + 1;
    Ok(outcome)
}

/// Roles that a proposal creates, with the provider each must be provisioned on.
pub fn roles_to_provision(
    proposal: &OrgChangeProposal,
    alloc: &KeyAllocation,
) -> Vec<RoleToProvision> {
    proposal
        .operations
        .iter()
        .filter_map(|op| match op {
            OrgOp::CreateRole {
                key,
                name,
                charter,
                authority,
                provider,
                ..
            } => alloc.roles.get(key).map(|role_id| RoleToProvision {
                key: key.clone(),
                role_id: role_id.clone(),
                name: name.clone(),
                charter: charter.clone(),
                authority: authority.clone(),
                provider: *provider,
            }),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RoleToProvision {
    pub key: String,
    pub role_id: RoleId,
    pub name: String,
    pub charter: String,
    pub authority: AuthorityGrant,
    pub provider: Provider,
}

struct Sim<'a> {
    st: OrgState,
    alloc: &'a KeyAllocation,
    catalog: &'a dyn CapabilityCatalog,
    bindings: Option<&'a BTreeMap<String, AgentBinding>>,
    now: DateTime<Utc>,
    next_revision: u64,
    errors: Vec<ValidationError>,
    effects: Vec<EventKind>,
    new_roles: BTreeSet<RoleId>,
    new_contracts: BTreeSet<ContractId>,
    /// New budget allocated by this change; `None` after an overflow.
    new_budget: Option<u64>,
}

impl<'a> Sim<'a> {
    fn new(
        state: &OrgState,
        alloc: &'a KeyAllocation,
        catalog: &'a dyn CapabilityCatalog,
        bindings: Option<&'a BTreeMap<String, AgentBinding>>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            next_revision: state.revision + 1,
            st: state.clone(),
            alloc,
            catalog,
            bindings,
            now,
            errors: Vec::new(),
            effects: Vec::new(),
            new_roles: BTreeSet::new(),
            new_contracts: BTreeSet::new(),
            new_budget: Some(0),
        }
    }

    fn err(&mut self, code: ValidationCode, op: Option<usize>, msg: impl Into<String>) {
        self.errors.push(ValidationError::new(code, op, msg));
    }

    fn committing(&self) -> bool {
        self.bindings.is_some()
    }

    fn run(mut self, proposal: &OrgChangeProposal) -> Result<Outcome, Vec<ValidationError>> {
        if proposal.operations.is_empty() {
            return Err(vec![ValidationError::new(
                ValidationCode::EmptyChange,
                None,
                "proposal contains no operations",
            )]);
        }
        if proposal.expected_org_revision != self.st.revision {
            return Err(vec![ValidationError::new(
                ValidationCode::RevisionConflict,
                None,
                format!(
                    "proposal expects revision {}, organization is at {}",
                    proposal.expected_org_revision, self.st.revision
                ),
            )]);
        }
        self.check_evidence(proposal);
        self.check_duplicate_keys(proposal);
        for (i, op) in proposal.operations.iter().enumerate() {
            self.apply(i, op);
        }
        self.post_checks(proposal);
        if self.errors.is_empty() {
            Ok(Outcome {
                state: self.st,
                effects: self.effects,
            })
        } else {
            Err(self.errors)
        }
    }

    fn check_evidence(&mut self, proposal: &OrgChangeProposal) {
        if proposal.trigger_evidence_ids.is_empty() {
            self.err(
                ValidationCode::MissingEvidence,
                None,
                "an organization change must cite at least one observation",
            );
        }
        for id in &proposal.trigger_evidence_ids {
            if !self.st.observations.contains_key(id) {
                self.err(
                    ValidationCode::UnknownEvidence,
                    None,
                    format!("unknown observation {id}"),
                );
            }
        }
    }

    fn check_duplicate_keys(&mut self, proposal: &OrgChangeProposal) {
        let mut seen: BTreeSet<(&'static str, String)> = BTreeSet::new();
        for (i, op) in proposal.operations.iter().enumerate() {
            let key = match op {
                OrgOp::CreateRole { key, .. } => Some(("role", key.clone())),
                OrgOp::RecruitSpecialist { key, .. } => Some(("contract", key.clone())),
                OrgOp::AssignResponsibility {
                    target: ResponsibilityTarget::FromRequirement { key, .. },
                    ..
                } => Some(("responsibility", key.clone())),
                OrgOp::RequestCapability { key, .. } => Some(("gap", key.clone())),
                _ => None,
            };
            if let Some(k) = key {
                if k.1.trim().is_empty() {
                    self.err(
                        ValidationCode::InvalidInput,
                        Some(i),
                        "keys must be non-empty",
                    );
                } else if !seen.insert(k.clone()) {
                    self.err(
                        ValidationCode::DuplicateKey,
                        Some(i),
                        format!("duplicate {} key `{}`", k.0, k.1),
                    );
                }
            }
        }
    }

    /// Resolve a role reference to a role that can take on work: an existing
    /// active role, or a role created earlier in this proposal.
    fn working_role(&mut self, i: usize, r: &RoleRef) -> Option<RoleId> {
        match r {
            RoleRef::Existing { role_id } => match self.st.roles.get(role_id) {
                None => {
                    self.err(
                        ValidationCode::UnknownRole,
                        Some(i),
                        format!("unknown role {role_id}"),
                    );
                    None
                }
                Some(role) if !role.state.accepts_work() && !self.new_roles.contains(role_id) => {
                    self.err(
                        ValidationCode::RoleNotActive,
                        Some(i),
                        format!("role {role_id} is {:?}, not ACTIVE", role.state),
                    );
                    None
                }
                Some(_) => Some(role_id.clone()),
            },
            RoleRef::New { key } => match self.alloc.roles.get(key) {
                Some(id) if self.new_roles.contains(id) => Some(id.clone()),
                _ => {
                    self.err(
                        ValidationCode::UnknownKey,
                        Some(i),
                        format!("role key `{key}` is not created by an earlier CreateRole"),
                    );
                    None
                }
            },
        }
    }

    fn responsibility(&mut self, i: usize, r: &ResponsibilityRef) -> Option<ResponsibilityId> {
        let id = match r {
            ResponsibilityRef::Existing { responsibility_id } => responsibility_id.clone(),
            ResponsibilityRef::New { key } => match self.alloc.responsibilities.get(key) {
                Some(id) => id.clone(),
                None => {
                    self.err(
                        ValidationCode::UnknownKey,
                        Some(i),
                        format!("unknown responsibility key `{key}`"),
                    );
                    return None;
                }
            },
        };
        match self.st.responsibilities.get(&id) {
            None => {
                self.err(
                    ValidationCode::UnknownResponsibility,
                    Some(i),
                    format!("unknown responsibility {id}"),
                );
                None
            }
            Some(r) if r.status != ResponsibilityStatus::Open => {
                self.err(
                    ValidationCode::ResponsibilityNotOpen,
                    Some(i),
                    format!("responsibility {id} is {:?}", r.status),
                );
                None
            }
            Some(_) => Some(id),
        }
    }

    fn contract(&mut self, i: usize, c: &ContractRef) -> Option<ContractId> {
        let id = match c {
            ContractRef::Existing { contract_id } => contract_id.clone(),
            ContractRef::New { key } => {
                match self.alloc.contracts.get(key) {
                    Some(id) if self.new_contracts.contains(id) => id.clone(),
                    _ => {
                        self.err(
                        ValidationCode::UnknownKey,
                        Some(i),
                        format!("contract key `{key}` is not created by an earlier RecruitSpecialist"),
                    );
                        return None;
                    }
                }
            }
        };
        match self.st.contracts.get(&id) {
            None => {
                self.err(
                    ValidationCode::UnknownContract,
                    Some(i),
                    format!("unknown contract {id}"),
                );
                None
            }
            Some(c) if matches!(c.state, ContractState::Released | ContractState::Declined) => {
                self.err(
                    ValidationCode::InvalidTransition,
                    Some(i),
                    format!("contract {id} is {:?}", c.state),
                );
                None
            }
            Some(_) => Some(id),
        }
    }

    fn apply(&mut self, i: usize, op: &OrgOp) {
        match op {
            OrgOp::CreateRole {
                key,
                name,
                charter,
                authority,
                reports_to,
                provider,
                justification: _,
            } => self.create_role(
                i,
                key,
                name,
                charter,
                authority,
                reports_to.as_ref(),
                *provider,
            ),
            OrgOp::RecruitSpecialist {
                key,
                specialist_address,
                protocol,
                scope,
                deadline,
                supervising_role,
            } => self.recruit(
                i,
                key,
                specialist_address,
                protocol,
                scope,
                *deadline,
                supervising_role.as_ref(),
            ),
            OrgOp::AssignResponsibility { target, owner } => self.assign(i, target, owner),
            OrgOp::DelegateTask {
                responsibility,
                executor,
                task,
            } => self.delegate(i, responsibility, executor, task),
            OrgOp::ReassignResponsibility {
                responsibility_id,
                from,
                to,
            } => self.reassign(i, responsibility_id, from, to),
            OrgOp::AttachCapability { role, capability } => self.attach(i, role, capability),
            OrgOp::ChangeReportingEdge { role, reports_to } => {
                self.reporting_edge(i, role, reports_to.as_ref())
            }
            OrgOp::RetireRole { role_id, handoffs } => self.retire(i, role_id, handoffs),
            OrgOp::RequestCapability {
                key,
                requirement_id,
                role,
                responsibility,
                capability_id,
                purpose,
                observed_sources,
            } => self.request_capability(
                i,
                key,
                requirement_id,
                role,
                responsibility,
                capability_id,
                purpose,
                observed_sources,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_role(
        &mut self,
        i: usize,
        key: &str,
        name: &str,
        charter: &str,
        authority: &AuthorityGrant,
        reports_to: Option<&RoleRef>,
        provider: Provider,
    ) {
        let Some(role_id) = self.alloc.roles.get(key).cloned() else {
            self.err(
                ValidationCode::UnknownKey,
                Some(i),
                format!("no allocated id for role key `{key}`"),
            );
            return;
        };
        if self.new_roles.contains(&role_id) {
            return; // duplicate key, already reported
        }
        if name.trim().is_empty() || charter.trim().is_empty() {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "role name and charter are required",
            );
        }
        if matches!(provider, Provider::Apify) {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "Apify Actors are capabilities, not roles; use AttachCapability",
            );
        }
        let outside: Vec<_> = authority
            .scopes
            .iter()
            .filter(|s| !self.st.constitution.scopes.contains(*s))
            .cloned()
            .collect();
        if !outside.is_empty() {
            self.err(
                ValidationCode::AuthorityExceedsConstitution,
                Some(i),
                format!("scopes not in constitution: {}", outside.join(", ")),
            );
        }
        let parent = reports_to.and_then(|r| self.working_role(i, r));

        let (state, binding) = if self.committing() {
            match self.bindings.and_then(|b| b.get(key)) {
                Some(binding) => {
                    // Walk the lifecycle table so an illegal path cannot sneak in.
                    let s = RoleState::Proposed
                        .transition(RoleState::Provisioning)
                        .and_then(|s| s.transition(RoleState::Ready))
                        .and_then(|s| s.transition(RoleState::Active))
                        .expect("static role lifecycle path");
                    (s, Some(binding.clone()))
                }
                None => {
                    self.err(
                        ValidationCode::NotProvisioned,
                        Some(i),
                        format!("role `{key}` has no provider binding; it cannot be activated"),
                    );
                    (RoleState::Provisioning, None)
                }
            }
        } else {
            (RoleState::Provisioning, None)
        };

        self.st.roles.insert(
            role_id.clone(),
            Role {
                role_id: role_id.clone(),
                name: name.to_owned(),
                charter: charter.to_owned(),
                authority: authority.clone(),
                reports_to: parent,
                state,
                binding,
                capabilities: Vec::new(),
                created_in_revision: self.next_revision,
                retired_in_revision: None,
            },
        );
        self.new_roles.insert(role_id.clone());
        self.new_budget = self
            .new_budget
            .and_then(|b| b.checked_add(authority.budget_cents));
        self.effects.push(EventKind::RoleCreated {
            role_id: role_id.clone(),
            name: name.to_owned(),
            provider,
        });
        if state == RoleState::Active {
            self.effects.push(EventKind::RoleActivated { role_id });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn recruit(
        &mut self,
        i: usize,
        key: &str,
        address: &str,
        protocol: &str,
        scope: &str,
        deadline: Option<DateTime<Utc>>,
        supervising: Option<&RoleRef>,
    ) {
        let Some(contract_id) = self.alloc.contracts.get(key).cloned() else {
            self.err(
                ValidationCode::UnknownKey,
                Some(i),
                format!("no allocated id for contract key `{key}`"),
            );
            return;
        };
        if self.new_contracts.contains(&contract_id) {
            return;
        }
        if address.trim().is_empty() || scope.trim().is_empty() {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "specialist address and scope are required",
            );
        }
        let supervising_role = supervising.and_then(|r| self.working_role(i, r));
        self.st.contracts.insert(
            contract_id.clone(),
            ContractorAgreement {
                contract_id: contract_id.clone(),
                specialist_address: address.to_owned(),
                protocol: protocol.to_owned(),
                scope: scope.to_owned(),
                deadline,
                state: ContractState::Requested,
                supervising_role,
            },
        );
        self.new_contracts.insert(contract_id.clone());
        self.effects.push(EventKind::ContractRequested {
            contract_id,
            specialist_address: address.to_owned(),
            scope: scope.to_owned(),
        });
    }

    fn assign(&mut self, i: usize, target: &ResponsibilityTarget, owner: &RoleRef) {
        let owner_id = self.working_role(i, owner);
        let rid = match target {
            ResponsibilityTarget::Existing { responsibility_id } => {
                match self.st.responsibilities.get(responsibility_id) {
                    None => {
                        self.err(
                            ValidationCode::UnknownResponsibility,
                            Some(i),
                            format!("unknown responsibility {responsibility_id}"),
                        );
                        return;
                    }
                    Some(r) if r.status != ResponsibilityStatus::Open => {
                        self.err(
                            ValidationCode::ResponsibilityNotOpen,
                            Some(i),
                            format!("responsibility {responsibility_id} is not open"),
                        );
                        return;
                    }
                    Some(r) if r.owner != Owner::MissionController => {
                        self.err(
                            ValidationCode::AccountabilityConflict,
                            Some(i),
                            format!(
                                "responsibility {responsibility_id} already has an accountable owner; use ReassignResponsibility"
                            ),
                        );
                        return;
                    }
                    Some(_) => responsibility_id.clone(),
                }
            }
            ResponsibilityTarget::FromRequirement {
                key,
                requirement_id,
                purpose,
                trigger,
                termination,
            } => {
                let Some(rid) = self.alloc.responsibilities.get(key).cloned() else {
                    self.err(
                        ValidationCode::UnknownKey,
                        Some(i),
                        format!("no allocated id for key `{key}`"),
                    );
                    return;
                };
                if self.st.responsibilities.contains_key(&rid) {
                    return; // duplicate key, already reported
                }
                let Some(req) = self.st.requirements.get_mut(requirement_id) else {
                    self.errors.push(ValidationError::new(
                        ValidationCode::UnknownRequirement,
                        Some(i),
                        format!("unknown requirement {requirement_id}"),
                    ));
                    return;
                };
                if req.status != RequirementStatus::Unmet {
                    let msg = format!(
                        "requirement {requirement_id} is {:?}, not UNMET",
                        req.status
                    );
                    self.err(ValidationCode::RequirementNotUnmet, Some(i), msg);
                    return;
                }
                if purpose.trim().is_empty() {
                    self.errors.push(ValidationError::new(
                        ValidationCode::InvalidInput,
                        Some(i),
                        "responsibility purpose is required",
                    ));
                }
                let mission_id = req.mission_id.clone();
                req.status = RequirementStatus::Assigned;
                if let Some(o) = &owner_id {
                    req.owner = Owner::Role(o.clone());
                }
                self.st.responsibilities.insert(
                    rid.clone(),
                    Responsibility {
                        responsibility_id: rid.clone(),
                        mission_id,
                        purpose: purpose.clone(),
                        requirement_id: Some(requirement_id.clone()),
                        owner: Owner::MissionController,
                        executor: None,
                        trigger: trigger.clone(),
                        termination: termination.clone(),
                        status: ResponsibilityStatus::Open,
                        epoch: 0,
                    },
                );
                self.effects.push(EventKind::ResponsibilityCreated {
                    responsibility_id: rid.clone(),
                    purpose: purpose.clone(),
                });
                rid
            }
        };
        if let Some(owner_id) = owner_id {
            let Some(epoch) = self.set_owner(i, &rid, &owner_id) else {
                return;
            };
            self.effects.push(EventKind::ResponsibilityAssigned {
                responsibility_id: rid,
                owner: owner_id,
                epoch,
            });
        }
    }

    /// Make `owner` the accountable owner of `rid`: advance the epoch, fence the
    /// previous assignment and create a new one. Returns the new epoch.
    fn set_owner(&mut self, i: usize, rid: &ResponsibilityId, owner: &RoleId) -> Option<u64> {
        let Some(next_epoch) = self.st.responsibilities[rid].epoch.checked_add(1) else {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "responsibility epoch is exhausted",
            );
            return None;
        };
        let epoch = {
            let r = self
                .st
                .responsibilities
                .get_mut(rid)
                .expect("checked by caller");
            r.owner = Owner::Role(owner.clone());
            r.epoch = next_epoch;
            if matches!(&r.executor, Some(Executor::Role(e)) if e != owner) {
                r.executor = None;
            }
            (r.epoch, r.requirement_id.clone())
        };
        let (epoch, requirement) = epoch;
        // Keep the linked requirement's accountable owner in step.
        if let Some(req) = requirement.and_then(|id| self.st.requirements.get_mut(&id)) {
            req.owner = Owner::Role(owner.clone());
        }
        let fenced: Vec<AssignmentId> = self
            .st
            .assignments
            .values()
            .filter(|a| &a.responsibility_id == rid && a.state == AssignmentState::Active)
            .map(|a| a.assignment_id.clone())
            .collect();
        for aid in fenced {
            let a = self.st.assignments.get_mut(&aid).expect("present");
            a.state = AssignmentState::Fenced;
            self.effects.push(EventKind::AssignmentFenced {
                assignment_id: aid,
                responsibility_id: rid.clone(),
                epoch: a.epoch,
            });
        }
        let aid = AssignmentId::generate();
        self.st.assignments.insert(
            aid.clone(),
            Assignment {
                assignment_id: aid,
                role_id: owner.clone(),
                responsibility_id: rid.clone(),
                epoch,
                state: AssignmentState::Active,
                lease_expires_at: None,
            },
        );
        Some(epoch)
    }

    fn delegate(
        &mut self,
        i: usize,
        responsibility: &ResponsibilityRef,
        executor: &ExecutorRef,
        task: &str,
    ) {
        let Some(rid) = self.responsibility(i, responsibility) else {
            return;
        };
        if self.st.responsibilities[&rid].owner == Owner::MissionController {
            self.err(
                ValidationCode::AccountabilityConflict,
                Some(i),
                format!(
                    "responsibility {rid} has no accountable role; assign it before delegating"
                ),
            );
            return;
        }
        let exec = match executor {
            ExecutorRef::Role { role } => self.working_role(i, role).map(Executor::Role),
            ExecutorRef::Contractor { contract } => {
                self.contract(i, contract).map(Executor::Contractor)
            }
        };
        if let Some(exec) = exec {
            if self.st.responsibilities[&rid].executor.as_ref() != Some(&exec) {
                // A role returning after A -> B -> A must receive a new grant.
                // Fence the whole responsibility, including prior owner work;
                // the accountable owner and its lease remain unchanged.
                let Owner::Role(owner) = self.st.responsibilities[&rid].owner.clone() else {
                    return;
                };
                let lease = self
                    .st
                    .active_assignment(&rid)
                    .and_then(|a| a.lease_expires_at);
                let Some(epoch) = self.set_owner(i, &rid, &owner) else {
                    return;
                };
                let assignment = self
                    .st
                    .active_assignment(&rid)
                    .expect("just created")
                    .assignment_id
                    .clone();
                self.st
                    .assignments
                    .get_mut(&assignment)
                    .expect("just created")
                    .lease_expires_at = lease;
                self.effects.push(EventKind::ResponsibilityAssigned {
                    responsibility_id: rid.clone(),
                    owner,
                    epoch,
                });
            }
            let r = self.st.responsibilities.get_mut(&rid).expect("checked");
            r.executor = Some(exec.clone());
            self.effects.push(EventKind::TaskDelegated {
                responsibility_id: rid,
                executor: exec,
                task: task.to_owned(),
            });
        }
    }

    fn reassign(&mut self, i: usize, rid: &ResponsibilityId, from: &RoleId, to: &RoleRef) {
        let Some(id) = self.responsibility(
            i,
            &ResponsibilityRef::Existing {
                responsibility_id: rid.clone(),
            },
        ) else {
            return;
        };
        let current = self.st.responsibilities[&id].owner.clone();
        if current != Owner::Role(from.clone()) {
            self.err(
                ValidationCode::AccountabilityConflict,
                Some(i),
                format!("responsibility {rid} is owned by {current:?}, not {from}"),
            );
            return;
        }
        let Some(to_id) = self.working_role(i, to) else {
            return;
        };
        if &to_id == from {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "cannot reassign a responsibility to its current owner",
            );
            return;
        }
        let Some(epoch) = self.set_owner(i, &id, &to_id) else {
            return;
        };
        self.effects.push(EventKind::ResponsibilityTransferred {
            responsibility_id: id,
            from: from.clone(),
            to: to_id,
            epoch,
        });
    }

    fn attach(&mut self, i: usize, role: &RoleRef, capability: &CapabilityRef) {
        let Some(role_id) = self.working_role(i, role) else {
            return;
        };
        if capability.version.trim().is_empty() || capability.version.eq_ignore_ascii_case("latest")
        {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "capability bindings must name an exact version, never `latest`",
            );
            return;
        }
        match self.catalog.state_of(capability) {
            None => self.err(
                ValidationCode::CapabilityUnknown,
                Some(i),
                format!(
                    "{}@{} is not installed in this organization",
                    capability.capability_id, capability.version
                ),
            ),
            Some(s) if !s.is_usable() => self.err(
                ValidationCode::CapabilityNotApproved,
                Some(i),
                format!(
                    "{}@{} is {s:?}",
                    capability.capability_id, capability.version
                ),
            ),
            Some(_) => {
                let r = self.st.roles.get_mut(&role_id).expect("resolved");
                if !r.capabilities.contains(capability) {
                    r.capabilities.push(capability.clone());
                }
                self.effects.push(EventKind::CapabilityAttached {
                    role_id,
                    capability: capability.clone(),
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn request_capability(
        &mut self,
        i: usize,
        key: &str,
        requirement_id: &RequirementId,
        role: &RoleRef,
        responsibility: &ResponsibilityRef,
        capability_id: &CapabilityId,
        purpose: &str,
        observed_sources: &[String],
    ) {
        let Some(gap_id) = self.alloc.gaps.get(key).cloned() else {
            self.err(
                ValidationCode::UnknownKey,
                Some(i),
                format!("no allocated id for capability gap key `{key}`"),
            );
            return;
        };
        let Some(role_id) = self.working_role(i, role) else {
            return;
        };
        let Some(responsibility_id) = self.responsibility(i, responsibility) else {
            return;
        };
        let Some(requirement) = self.st.requirements.get(requirement_id).cloned() else {
            self.err(
                ValidationCode::UnknownRequirement,
                Some(i),
                format!("unknown requirement {requirement_id}"),
            );
            return;
        };
        if requirement.status == RequirementStatus::Satisfied {
            self.err(
                ValidationCode::RequirementNotUnmet,
                Some(i),
                format!("requirement {requirement_id} is already satisfied"),
            );
        }
        let (resp_mission, resp_owner, resp_requirement) = {
            let r = &self.st.responsibilities[&responsibility_id];
            (
                r.mission_id.clone(),
                r.owner.clone(),
                r.requirement_id.clone(),
            )
        };
        // The gap serves the responsibility created for this exact
        // requirement; acquisition authority checks the same link.
        if resp_requirement.as_ref() != Some(requirement_id) {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                format!("responsibility {responsibility_id} was not created for requirement {requirement_id}"),
            );
        }
        if requirement.owner != Owner::Role(role_id.clone()) {
            self.err(
                ValidationCode::AccountabilityConflict,
                Some(i),
                format!("requirement {requirement_id} is not owned by {role_id}"),
            );
        }
        if resp_mission != requirement.mission_id {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "the responsibility and the requirement belong to different missions",
            );
        }
        // The capability serves the role's own accountable work: assign the
        // responsibility to the role (earlier in this change) first.
        if resp_owner != Owner::Role(role_id.clone()) {
            self.err(
                ValidationCode::AccountabilityConflict,
                Some(i),
                format!("{role_id} does not own responsibility {responsibility_id}; assign it before requesting a capability for it"),
            );
        }
        if !valid_capability_name(capability_id.as_str()) {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "capability_id must be 3-64 characters of a-z, 0-9, '-' or '.'",
            );
        }
        if purpose.trim().is_empty() || purpose.chars().count() > 400 {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "purpose must be 1-400 characters",
            );
        }
        // Sources must be public pages on the same origin as the evidence that
        // caused the requirement; the proposing agent cannot invent targets.
        let evidence_origins: BTreeSet<String> = requirement
            .triggering_evidence
            .iter()
            .filter_map(|o| self.st.observations.get(o))
            .filter_map(|o| o.evidence_ref.as_deref().and_then(http_origin))
            .collect();
        if observed_sources.is_empty() || observed_sources.len() > 8 {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                "observed_sources must list 1-8 public URLs",
            );
        }
        for url in observed_sources {
            match acquirable_source(url) {
                None => self.err(
                    ValidationCode::InvalidInput,
                    Some(i),
                    format!(
                        "observed source {url:?} must be a canonical https URL with no credentials or fragment, as acquisition requires"
                    ),
                ),
                Some(origin) if !evidence_origins.contains(&origin) => self.err(
                    ValidationCode::UnknownEvidence,
                    Some(i),
                    format!(
                        "observed source {url} is not on the origin of the requirement's evidence"
                    ),
                ),
                Some(_) => {}
            }
        }
        if self.catalog_has_usable(capability_id) {
            self.err(
                ValidationCode::InvalidInput,
                Some(i),
                format!("{capability_id} is already installed and approved; use AttachCapability"),
            );
        }
        if self.st.capability_gaps.values().any(|g| {
            g.requirement_id == *requirement_id
                && g.capability_id == *capability_id
                && !g.status.is_terminal()
        }) {
            self.err(
                ValidationCode::DuplicateKey,
                Some(i),
                format!("{capability_id} is already requested for requirement {requirement_id}"),
            );
        }
        let refused = self
            .st
            .capability_gaps
            .values()
            .filter(|g| {
                g.requirement_id == *requirement_id
                    && matches!(g.status, CapabilityGapStatus::Refused { .. })
            })
            .count();
        if refused >= MAX_REFUSED_REQUESTS_PER_REQUIREMENT {
            self.err(
                ValidationCode::CapabilityGapLimitExceeded,
                Some(i),
                format!(
                    "requirement {requirement_id} already had {refused} capability requests refused; proceed without a new capability, or record new evidence and a new requirement if circumstances changed"
                ),
            );
        }
        let live: Vec<_> = self
            .st
            .capability_gaps
            .values()
            .filter(|g| !g.status.is_terminal())
            .collect();
        if live.len() >= MAX_LIVE_CAPABILITY_GAPS {
            self.err(
                ValidationCode::CapabilityGapLimitExceeded,
                Some(i),
                format!("the organization already has {MAX_LIVE_CAPABILITY_GAPS} live capability requests"),
            );
        } else if live
            .iter()
            .filter(|g| g.requirement_id == *requirement_id)
            .count()
            >= MAX_LIVE_GAPS_PER_REQUIREMENT
        {
            self.err(
                ValidationCode::CapabilityGapLimitExceeded,
                Some(i),
                format!("requirement {requirement_id} already has {MAX_LIVE_GAPS_PER_REQUIREMENT} live capability requests"),
            );
        }
        if !self.errors.is_empty() {
            return;
        }
        let requirement_digest = requirement_digest(&requirement);
        self.st.capability_gaps.insert(
            gap_id.clone(),
            CapabilityGap {
                gap_id: gap_id.clone(),
                mission_id: requirement.mission_id.clone(),
                requirement_id: requirement_id.clone(),
                requirement_digest,
                triggering_evidence: requirement.triggering_evidence.clone(),
                role_id: role_id.clone(),
                responsibility_id: responsibility_id.clone(),
                capability_id: capability_id.clone(),
                purpose: purpose.trim().to_owned(),
                observed_sources: observed_sources.to_vec(),
                status: CapabilityGapStatus::Open,
                opened_in_revision: self.next_revision,
            },
        );
        self.effects.push(EventKind::CapabilityGapOpened {
            gap_id,
            requirement_id: requirement_id.clone(),
            role_id,
            responsibility_id,
            capability_id: capability_id.clone(),
        });
    }

    fn catalog_has_usable(&self, capability_id: &CapabilityId) -> bool {
        self.st.installations.values().any(|inst| {
            inst.capability.capability_id == *capability_id
                && self
                    .catalog
                    .state_of(&inst.capability)
                    .is_some_and(|s| s.is_usable())
        })
    }

    fn reporting_edge(&mut self, i: usize, role: &RoleRef, reports_to: Option<&RoleRef>) {
        let Some(role_id) = self.working_role(i, role) else {
            return;
        };
        let parent = match reports_to {
            None => None,
            Some(r) => match self.working_role(i, r) {
                Some(p) => Some(p),
                None => return,
            },
        };
        if let Some(p) = &parent {
            if p == &role_id || self.st.reports_transitively_to(p, &role_id) {
                self.err(
                    ValidationCode::ReportingCycle,
                    Some(i),
                    format!("{role_id} reporting to {p} would create a cycle"),
                );
                return;
            }
        }
        self.st
            .roles
            .get_mut(&role_id)
            .expect("resolved")
            .reports_to = parent.clone();
        self.effects.push(EventKind::ReportingEdgeChanged {
            role_id,
            reports_to: parent,
        });
    }

    fn retire(&mut self, i: usize, role_id: &RoleId, handoffs: &[Handoff]) {
        let Some(role) = self.st.roles.get(role_id) else {
            self.err(
                ValidationCode::UnknownRole,
                Some(i),
                format!("unknown role {role_id}"),
            );
            return;
        };
        if role.state != RoleState::Active {
            let msg = format!(
                "role {role_id} is {:?}; only ACTIVE roles can be retired",
                role.state
            );
            self.err(ValidationCode::RoleNotActive, Some(i), msg);
            return;
        }
        let grandparent = role.reports_to.clone();
        for h in handoffs {
            let owned_by_role = self.st.responsibilities.get(&h.responsibility_id).map(|r| {
                r.owner == Owner::Role(role_id.clone()) && r.status == ResponsibilityStatus::Open
            });
            match owned_by_role {
                None => {
                    self.err(
                        ValidationCode::UnknownResponsibility,
                        Some(i),
                        format!("unknown responsibility {}", h.responsibility_id),
                    );
                    continue;
                }
                Some(false) => {
                    self.err(
                        ValidationCode::AccountabilityConflict,
                        Some(i),
                        format!(
                            "{} is not an open responsibility of {role_id}",
                            h.responsibility_id
                        ),
                    );
                    continue;
                }
                Some(true) => {}
            }
            let Some(to) = self.working_role(i, &h.to) else {
                continue;
            };
            if &to == role_id {
                self.err(
                    ValidationCode::InvalidInput,
                    Some(i),
                    "cannot hand off to the retiring role",
                );
                continue;
            }
            let Some(epoch) = self.set_owner(i, &h.responsibility_id, &to) else {
                continue;
            };
            self.effects.push(EventKind::ResponsibilityTransferred {
                responsibility_id: h.responsibility_id.clone(),
                from: role_id.clone(),
                to,
                epoch,
            });
        }
        let remaining: Vec<String> = self
            .st
            .open_responsibilities_of(role_id)
            .iter()
            .map(|r| r.responsibility_id.to_string())
            .collect();
        if !remaining.is_empty() {
            self.err(
                ValidationCode::RetirementDropsObligations,
                Some(i),
                format!(
                    "retiring {role_id} would drop open responsibilities: {}",
                    remaining.join(", ")
                ),
            );
            return;
        }
        // Clear execution handled by the retiring role on others' responsibilities.
        for r in self.st.responsibilities.values_mut() {
            if r.executor == Some(Executor::Role(role_id.clone())) {
                r.executor = None;
            }
        }
        for c in self.st.contracts.values_mut() {
            if c.supervising_role.as_ref() == Some(role_id) {
                c.supervising_role = None;
            }
        }
        for r in self.st.roles.values_mut() {
            if r.reports_to.as_ref() == Some(role_id) {
                r.reports_to = grandparent.clone();
            }
        }
        // Fence anything still assigned to the role.
        let leftover: Vec<AssignmentId> = self
            .st
            .assignments
            .values()
            .filter(|a| &a.role_id == role_id && a.state == AssignmentState::Active)
            .map(|a| a.assignment_id.clone())
            .collect();
        for aid in leftover {
            let a = self.st.assignments.get_mut(&aid).expect("present");
            a.state = AssignmentState::Fenced;
            self.effects.push(EventKind::AssignmentFenced {
                assignment_id: aid,
                responsibility_id: a.responsibility_id.clone(),
                epoch: a.epoch,
            });
        }
        let next_revision = self.next_revision;
        let role = self.st.roles.get_mut(role_id).expect("present");
        role.state = role
            .state
            .transition(RoleState::Draining)
            .and_then(|s| s.transition(RoleState::Retired))
            .expect("checked ACTIVE above");
        role.retired_in_revision = Some(next_revision);
        self.effects.push(EventKind::RoleDraining {
            role_id: role_id.clone(),
        });
        self.effects.push(EventKind::RoleRetired {
            role_id: role_id.clone(),
        });
    }

    fn post_checks(&mut self, proposal: &OrgChangeProposal) {
        for role_id in self.new_roles.clone() {
            let owns = !self.st.open_responsibilities_of(&role_id).is_empty();
            let executes = self.st.responsibilities.values().any(|r| {
                r.status == ResponsibilityStatus::Open
                    && r.executor == Some(Executor::Role(role_id.clone()))
            });
            if !owns && !executes {
                self.err(
                    ValidationCode::NewRoleWithoutResponsibility,
                    None,
                    format!(
                        "new role {role_id} has no responsibility; attach a capability to an existing role instead"
                    ),
                );
            }
        }
        let c = &self.st.constitution;
        match self.st.checked_allocated_budget_cents() {
            None => self.err(
                ValidationCode::BudgetExceeded,
                None,
                "role budgets overflow; no allocation can exceed the constitution budget",
            ),
            Some(allocated) if allocated > c.budget_cents => {
                let msg = format!(
                    "roles would hold {allocated} cents of budget, constitution allows {}",
                    c.budget_cents
                );
                self.err(ValidationCode::BudgetExceeded, None, msg);
            }
            Some(_) => {}
        }
        match (self.new_budget, proposal.budget_limit_cents) {
            (None, _) => self.err(
                ValidationCode::ChangeBudgetExceeded,
                None,
                "budgets in this change overflow",
            ),
            (Some(new), Some(limit)) if new > limit => {
                let msg = format!("change allocates {new} cents, its own limit is {limit}");
                self.err(ValidationCode::ChangeBudgetExceeded, None, msg);
            }
            _ => {}
        }
        let live = self.st.live_roles().count() as u32;
        if live > self.st.constitution.max_active_roles {
            let msg = format!(
                "{live} live roles exceed the limit of {}",
                self.st.constitution.max_active_roles
            );
            self.err(ValidationCode::RoleLimitExceeded, None, msg);
        }
        let contracts = self.st.active_contracts() as u32;
        if contracts > self.st.constitution.max_active_contracts {
            let msg = format!(
                "{contracts} open contracts exceed the limit of {}",
                self.st.constitution.max_active_contracts
            );
            self.err(ValidationCode::ContractLimitExceeded, None, msg);
        }
        // Every open responsibility owned by a role must be owned by a live role.
        let orphaned: Vec<String> = self
            .st
            .responsibilities
            .values()
            .filter(|r| r.status == ResponsibilityStatus::Open)
            .filter_map(|r| r.owner.role().map(|o| (r, o)))
            .filter(|(_, o)| {
                self.st
                    .roles
                    .get(*o)
                    .map(|role| {
                        matches!(
                            role.state,
                            RoleState::Retired | RoleState::ProvisioningFailed
                        )
                    })
                    .unwrap_or(true)
            })
            .map(|(r, _)| r.responsibility_id.to_string())
            .collect();
        if !orphaned.is_empty() {
            self.err(
                ValidationCode::RetirementDropsObligations,
                None,
                format!(
                    "responsibilities without a live owner: {}",
                    orphaned.join(", ")
                ),
            );
        }
        let _ = self.now;
    }
}

// ---------------------------------------------------------------------------
// Bootstrap and evidence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BootstrapRole {
    pub key: String,
    pub name: String,
    pub charter: String,
    #[serde(default)]
    pub authority: AuthorityGrant,
    pub reports_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BootstrapResponsibility {
    pub purpose: String,
    pub owner: String,
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub termination: String,
}

/// Body of `POST /api/v1/missions`: a mission plus its initial organization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MissionBootstrap {
    pub goal: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub success_checks: Vec<String>,
    #[serde(default)]
    pub roles: Vec<BootstrapRole>,
    #[serde(default)]
    pub responsibilities: Vec<BootstrapResponsibility>,
}

/// Create a mission together with its starting roles and responsibilities.
/// Starting roles are implemented by the kernel (`Provider::Internal`).
pub fn bootstrap_mission(
    state: &OrgState,
    spec: &MissionBootstrap,
    now: DateTime<Utc>,
) -> Result<(Outcome, MissionId), Vec<ValidationError>> {
    let mut st = state.clone();
    let mut errors = Vec::new();
    let mut effects = Vec::new();
    let next_revision = st.revision + 1;
    if spec.goal.trim().is_empty() {
        errors.push(ValidationError::new(
            ValidationCode::InvalidInput,
            None,
            "goal is required",
        ));
    }
    let mission_id = MissionId::generate();
    st.missions.insert(
        mission_id.clone(),
        Mission {
            mission_id: mission_id.clone(),
            goal: spec.goal.clone(),
            constraints: spec.constraints.clone(),
            deadline: spec.deadline,
            success_checks: spec.success_checks.clone(),
            status: MissionStatus::Active,
            created_at: now,
        },
    );
    effects.push(EventKind::MissionCreated {
        mission_id: mission_id.clone(),
        goal: spec.goal.clone(),
    });

    let mut keys: BTreeMap<String, RoleId> = BTreeMap::new();
    for (i, r) in spec.roles.iter().enumerate() {
        if keys.contains_key(&r.key) {
            errors.push(ValidationError::new(
                ValidationCode::DuplicateKey,
                Some(i),
                format!("duplicate role key `{}`", r.key),
            ));
            continue;
        }
        let outside: Vec<_> = r
            .authority
            .scopes
            .iter()
            .filter(|s| !st.constitution.scopes.contains(*s))
            .cloned()
            .collect();
        if !outside.is_empty() {
            errors.push(ValidationError::new(
                ValidationCode::AuthorityExceedsConstitution,
                Some(i),
                format!(
                    "role `{}` requests scopes outside the constitution: {}",
                    r.key,
                    outside.join(", ")
                ),
            ));
        }
        keys.insert(r.key.clone(), RoleId::generate());
    }
    for (i, r) in spec.roles.iter().enumerate() {
        let Some(role_id) = keys.get(&r.key).cloned() else {
            continue;
        };
        if st.roles.contains_key(&role_id) {
            continue;
        }
        let reports_to = match &r.reports_to {
            None => None,
            Some(k) => match keys.get(k) {
                Some(id) => Some(id.clone()),
                None => {
                    errors.push(ValidationError::new(
                        ValidationCode::UnknownKey,
                        Some(i),
                        format!("unknown reports_to key `{k}`"),
                    ));
                    None
                }
            },
        };
        let config =
            serde_json::json!({"name": r.name, "charter": r.charter, "authority": r.authority});
        st.roles.insert(
            role_id.clone(),
            Role {
                role_id: role_id.clone(),
                name: r.name.clone(),
                charter: r.charter.clone(),
                authority: r.authority.clone(),
                reports_to,
                state: RoleState::Active,
                binding: Some(AgentBinding {
                    provider: Provider::Internal,
                    provider_agent_id: format!("kernel:{}", r.key),
                    config_digest: crate::digest::canonical_sha256(&config),
                    conversation_ref: None,
                }),
                capabilities: Vec::new(),
                created_in_revision: next_revision,
                retired_in_revision: None,
            },
        );
        effects.push(EventKind::RoleCreated {
            role_id: role_id.clone(),
            name: r.name.clone(),
            provider: Provider::Internal,
        });
        effects.push(EventKind::RoleActivated { role_id });
    }
    for (i, r) in spec.responsibilities.iter().enumerate() {
        let Some(owner) = keys.get(&r.owner).cloned() else {
            errors.push(ValidationError::new(
                ValidationCode::UnknownKey,
                Some(i),
                format!("unknown owner key `{}`", r.owner),
            ));
            continue;
        };
        let rid = ResponsibilityId::generate();
        st.responsibilities.insert(
            rid.clone(),
            Responsibility {
                responsibility_id: rid.clone(),
                mission_id: mission_id.clone(),
                purpose: r.purpose.clone(),
                requirement_id: None,
                owner: Owner::Role(owner.clone()),
                executor: None,
                trigger: r.trigger.clone(),
                termination: r.termination.clone(),
                status: ResponsibilityStatus::Open,
                epoch: 1,
            },
        );
        let aid = AssignmentId::generate();
        st.assignments.insert(
            aid.clone(),
            Assignment {
                assignment_id: aid,
                role_id: owner.clone(),
                responsibility_id: rid.clone(),
                epoch: 1,
                state: AssignmentState::Active,
                lease_expires_at: None,
            },
        );
        effects.push(EventKind::ResponsibilityCreated {
            responsibility_id: rid.clone(),
            purpose: r.purpose.clone(),
        });
        effects.push(EventKind::ResponsibilityAssigned {
            responsibility_id: rid,
            owner,
            epoch: 1,
        });
    }
    match st.checked_allocated_budget_cents() {
        Some(total) if total <= st.constitution.budget_cents => {}
        Some(_) => errors.push(ValidationError::new(
            ValidationCode::BudgetExceeded,
            None,
            "initial roles exceed the constitution budget",
        )),
        None => errors.push(ValidationError::new(
            ValidationCode::BudgetExceeded,
            None,
            "initial role budgets overflow",
        )),
    }
    // The same reporting-graph invariant ChangeReportingEdge enforces.
    let cyclic: Vec<String> = st
        .roles
        .values()
        .filter(|r| st.reports_transitively_to(&r.role_id, &r.role_id))
        .map(|r| r.name.clone())
        .collect();
    if !cyclic.is_empty() {
        errors.push(ValidationError::new(
            ValidationCode::ReportingCycle,
            None,
            format!(
                "reporting lines form a cycle through: {}",
                cyclic.join(", ")
            ),
        ));
    }
    if st.live_roles().count() as u32 > st.constitution.max_active_roles {
        errors.push(ValidationError::new(
            ValidationCode::RoleLimitExceeded,
            None,
            "initial roles exceed the role limit",
        ));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    st.revision = next_revision;
    Ok((Outcome { state: st, effects }, mission_id))
}

/// Record a piece of evidence. Observations never name the role to create.
pub fn record_observation(
    state: &OrgState,
    mission_id: &MissionId,
    source: &str,
    summary: &str,
    evidence_ref: Option<String>,
    now: DateTime<Utc>,
) -> Result<(Outcome, ObservationId), ValidationError> {
    if !state.missions.contains_key(mission_id) {
        return Err(ValidationError::new(
            ValidationCode::UnknownMission,
            None,
            format!("unknown mission {mission_id}"),
        ));
    }
    if summary.trim().is_empty() {
        return Err(ValidationError::new(
            ValidationCode::InvalidInput,
            None,
            "summary is required",
        ));
    }
    let mut st = state.clone();
    let id = ObservationId::generate();
    st.observations.insert(
        id.clone(),
        Observation {
            observation_id: id.clone(),
            mission_id: mission_id.clone(),
            source: source.to_owned(),
            summary: summary.to_owned(),
            evidence_ref,
            observed_at: now,
        },
    );
    st.revision += 1;
    let effects = vec![EventKind::ObservationRecorded {
        observation_id: id.clone(),
        source: source.to_owned(),
        summary: summary.to_owned(),
    }];
    Ok((Outcome { state: st, effects }, id))
}

/// Record an unmet requirement derived from evidence. It stays owned by the
/// mission controller until an organization change assigns it.
pub fn record_requirement(
    state: &OrgState,
    mission_id: &MissionId,
    summary: &str,
    evidence: &[ObservationId],
    acceptance: &str,
) -> Result<(Outcome, RequirementId), ValidationError> {
    if !state.missions.contains_key(mission_id) {
        return Err(ValidationError::new(
            ValidationCode::UnknownMission,
            None,
            format!("unknown mission {mission_id}"),
        ));
    }
    if evidence.is_empty() {
        return Err(ValidationError::new(
            ValidationCode::MissingEvidence,
            None,
            "a requirement must reference triggering evidence",
        ));
    }
    if let Some(missing) = evidence
        .iter()
        .find(|e| !state.observations.contains_key(*e))
    {
        return Err(ValidationError::new(
            ValidationCode::UnknownEvidence,
            None,
            format!("unknown observation {missing}"),
        ));
    }
    let mut st = state.clone();
    let id = RequirementId::generate();
    st.requirements.insert(
        id.clone(),
        Requirement {
            requirement_id: id.clone(),
            mission_id: mission_id.clone(),
            summary: summary.to_owned(),
            triggering_evidence: evidence.to_vec(),
            acceptance: acceptance.to_owned(),
            status: RequirementStatus::Unmet,
            owner: Owner::MissionController,
        },
    );
    st.revision += 1;
    let effects = vec![EventKind::RequirementRecorded {
        requirement_id: id.clone(),
        summary: summary.to_owned(),
        evidence: evidence.to_vec(),
    }];
    Ok((Outcome { state: st, effects }, id))
}

// ---------------------------------------------------------------------------
// Dispatch, results and fencing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DispatchError {
    #[error("unknown role {role_id}")]
    UnknownRole { role_id: RoleId },
    #[error("role {role_id} is not active")]
    RoleNotActive { role_id: RoleId },
    #[error("unknown responsibility {responsibility_id}")]
    UnknownResponsibility { responsibility_id: ResponsibilityId },
    #[error("responsibility {responsibility_id} is not open")]
    ResponsibilityNotOpen { responsibility_id: ResponsibilityId },
    #[error("role {role_id} has no active assignment for {responsibility_id}")]
    NotAssigned {
        role_id: RoleId,
        responsibility_id: ResponsibilityId,
    },
    #[error("stale epoch {got}; current epoch is {current}")]
    StaleEpoch { current: u64, got: u64 },
    #[error("scope `{scope}` is not granted to role {role_id}")]
    ScopeNotGranted { role_id: RoleId, scope: String },
    #[error("capability {capability_id}@{version} is not attached to role {role_id}")]
    CapabilityNotAttached {
        role_id: RoleId,
        capability_id: CapabilityId,
        version: String,
    },
}

/// Grant that accompanies a dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DispatchGrant {
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub assignment_id: AssignmentId,
    pub epoch: u64,
}

/// Check that `role` may dispatch privileged work for `responsibility` now.
/// The role must be ACTIVE and hold the current-epoch assignment; it may be the
/// accountable owner or the delegated executor.
pub fn authorize_dispatch(
    state: &OrgState,
    role_id: &RoleId,
    responsibility_id: &ResponsibilityId,
    scope: Option<&str>,
) -> Result<DispatchGrant, DispatchError> {
    let role = state
        .roles
        .get(role_id)
        .ok_or_else(|| DispatchError::UnknownRole {
            role_id: role_id.clone(),
        })?;
    if !role.state.accepts_work() {
        return Err(DispatchError::RoleNotActive {
            role_id: role_id.clone(),
        });
    }
    let resp = state
        .responsibilities
        .get(responsibility_id)
        .ok_or_else(|| DispatchError::UnknownResponsibility {
            responsibility_id: responsibility_id.clone(),
        })?;
    if resp.status != ResponsibilityStatus::Open {
        return Err(DispatchError::ResponsibilityNotOpen {
            responsibility_id: responsibility_id.clone(),
        });
    }
    let assignment =
        state
            .active_assignment(responsibility_id)
            .ok_or_else(|| DispatchError::NotAssigned {
                role_id: role_id.clone(),
                responsibility_id: responsibility_id.clone(),
            })?;
    let is_owner = &assignment.role_id == role_id;
    let is_executor = resp.executor == Some(Executor::Role(role_id.clone()));
    if !is_owner && !is_executor {
        return Err(DispatchError::NotAssigned {
            role_id: role_id.clone(),
            responsibility_id: responsibility_id.clone(),
        });
    }
    if assignment.epoch != resp.epoch {
        return Err(DispatchError::StaleEpoch {
            current: resp.epoch,
            got: assignment.epoch,
        });
    }
    if let Some(scope) = scope {
        if !role.authority.allows(scope) {
            return Err(DispatchError::ScopeNotGranted {
                role_id: role_id.clone(),
                scope: scope.to_owned(),
            });
        }
    }
    Ok(DispatchGrant {
        role_id: role_id.clone(),
        responsibility_id: responsibility_id.clone(),
        assignment_id: assignment.assignment_id.clone(),
        epoch: resp.epoch,
    })
}

/// A result carrying `epoch` may change commitments only while the
/// responsibility is still open and the epoch is current. Late results from
/// fenced assignments or completed work are archived, not applied.
///
/// This checks the responsibility only. Job results must use
/// [`check_job_result`], which also checks that the submitting role still
/// holds the work.
pub fn check_result_epoch(
    state: &OrgState,
    responsibility_id: &ResponsibilityId,
    epoch: u64,
) -> Result<(), DispatchError> {
    let resp = state
        .responsibilities
        .get(responsibility_id)
        .ok_or_else(|| DispatchError::UnknownResponsibility {
            responsibility_id: responsibility_id.clone(),
        })?;
    if resp.epoch != epoch {
        return Err(DispatchError::StaleEpoch {
            current: resp.epoch,
            got: epoch,
        });
    }
    if resp.status != ResponsibilityStatus::Open {
        return Err(DispatchError::ResponsibilityNotOpen {
            responsibility_id: responsibility_id.clone(),
        });
    }
    Ok(())
}

/// Whether a result from `role` for work dispatched at `epoch` may still have
/// effects (ledger writes, completion). Requires an open responsibility at the
/// same epoch, a role that is still ACTIVE, and that the role still holds the
/// work: either the owner of the current active assignment or the current
/// delegated executor. A replaced, retired or otherwise revoked executor's
/// late result is rejected. Changed delegation rotates the responsibility epoch
/// and assignment, so reappointing the same executor cannot revive its old grant.
pub fn check_job_result(
    state: &OrgState,
    role_id: &RoleId,
    responsibility_id: &ResponsibilityId,
    epoch: u64,
) -> Result<(), DispatchError> {
    check_result_epoch(state, responsibility_id, epoch)?;
    let role = state
        .roles
        .get(role_id)
        .ok_or_else(|| DispatchError::UnknownRole {
            role_id: role_id.clone(),
        })?;
    if !role.state.accepts_work() {
        return Err(DispatchError::RoleNotActive {
            role_id: role_id.clone(),
        });
    }
    let resp = &state.responsibilities[responsibility_id];
    let owns = state
        .active_assignment(responsibility_id)
        .map(|a| &a.role_id == role_id && a.epoch == epoch)
        .unwrap_or(false);
    let executes = resp.executor == Some(Executor::Role(role_id.clone()));
    if !owns && !executes {
        return Err(DispatchError::NotAssigned {
            role_id: role_id.clone(),
            responsibility_id: responsibility_id.clone(),
        });
    }
    Ok(())
}

/// Check that a role may invoke an exact capability version.
pub fn authorize_capability(
    state: &OrgState,
    role_id: &RoleId,
    capability: &CapabilityRef,
) -> Result<(), DispatchError> {
    let role = state
        .roles
        .get(role_id)
        .ok_or_else(|| DispatchError::UnknownRole {
            role_id: role_id.clone(),
        })?;
    if !role.state.accepts_work() {
        return Err(DispatchError::RoleNotActive {
            role_id: role_id.clone(),
        });
    }
    if !role.capabilities.contains(capability) {
        return Err(DispatchError::CapabilityNotAttached {
            role_id: role_id.clone(),
            capability_id: capability.capability_id.clone(),
            version: capability.version.clone(),
        });
    }
    Ok(())
}

pub fn complete_responsibility(
    state: &OrgState,
    id: &ResponsibilityId,
) -> Result<Outcome, ValidationError> {
    let mut st = state.clone();
    let r = st.responsibilities.get_mut(id).ok_or_else(|| {
        ValidationError::new(
            ValidationCode::UnknownResponsibility,
            None,
            format!("unknown responsibility {id}"),
        )
    })?;
    r.status = r
        .status
        .transition(ResponsibilityStatus::Completed)
        .map_err(|e| {
            ValidationError::new(ValidationCode::InvalidTransition, None, e.to_string())
        })?;
    let aid = st.active_assignment(id).map(|a| a.assignment_id.clone());
    if let Some(aid) = aid {
        let a = st.assignments.get_mut(&aid).expect("present");
        a.state = AssignmentState::Released;
    }
    st.revision += 1;
    Ok(Outcome {
        state: st,
        effects: vec![EventKind::ResponsibilityCompleted {
            responsibility_id: id.clone(),
        }],
    })
}

/// Move a contractor agreement through its lifecycle (offer, accept, release).
pub fn update_contract(
    state: &OrgState,
    id: &ContractId,
    to: ContractState,
) -> Result<Outcome, ValidationError> {
    let mut st = state.clone();
    let c = st.contracts.get_mut(id).ok_or_else(|| {
        ValidationError::new(
            ValidationCode::UnknownContract,
            None,
            format!("unknown contract {id}"),
        )
    })?;
    let from = c.state;
    c.state = c.state.transition(to).map_err(|e| {
        ValidationError::new(ValidationCode::InvalidTransition, None, e.to_string())
    })?;
    st.revision += 1;
    Ok(Outcome {
        state: st,
        effects: vec![EventKind::ContractStateChanged {
            contract_id: id.clone(),
            from,
            to,
        }],
    })
}

// ---------------------------------------------------------------------------
// Capability gaps
// ---------------------------------------------------------------------------

/// `scheme://host[:port]` of an http(s) URL, lowercased; `None` otherwise.
fn http_origin(url: &str) -> Option<String> {
    if url.len() > 512 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    Some(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// Origin of a source an acquisition can actually use. The same rules as the
/// forge's synthesis input (`genesis_forge::SynthesisRequest::validate`):
/// https, a host, no credentials, no fragment, at most 1024 bytes, and already
/// in canonical form. Anything else would commit a gap no trusted policy can
/// ever materialize, so it is rejected here, where the planner can repair it.
fn acquirable_source(source: &str) -> Option<String> {
    let url = url::Url::parse(source).ok()?;
    if source.len() > 1024
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str() != source
    {
        return None;
    }
    http_origin(source)
}

/// Digest a capability gap freezes for its requirement: identity, mission,
/// summary, acceptance and triggering evidence (not status). Recompute it on
/// the live requirement to tell whether the gap still describes current work.
pub fn requirement_digest(requirement: &Requirement) -> String {
    crate::digest::canonical_sha256(&serde_json::json!({
        "requirement_id": requirement.requirement_id,
        "mission_id": requirement.mission_id,
        "summary": requirement.summary,
        "acceptance": requirement.acceptance,
        "triggering_evidence": requirement.triggering_evidence,
    }))
}

fn valid_capability_name(name: &str) -> bool {
    (3..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && name.as_bytes()[0].is_ascii_alphanumeric()
}

/// Move a capability gap forward. Allowed (backend drivers only):
///
/// - `Open -> Acquiring | Refused`
/// - `Acquiring -> Acquired | Refused` (same acquisition)
/// - `Acquired -> Installed | Refused` (same acquisition and capability;
///   the installation must exist here, for that capability, and be Accepted)
///
/// `Installed` and `Refused` are final.
pub fn update_capability_gap(
    state: &OrgState,
    gap_id: &CapabilityGapId,
    to: CapabilityGapStatus,
) -> Result<Outcome, ValidationError> {
    update_capability_gap_at(state, gap_id, to, Utc::now())
}

/// Source of the observation a final gap transition records.
pub const CAPABILITY_GAP_SOURCE: &str = "kernel.capability_gap";

/// [`update_capability_gap`] at a given time. A final transition (`installed`
/// or `refused`) also records one observation on the gap's mission, in the
/// same outcome, so the planner gets its next pass: attach the installed
/// capability, or decide what to do without it. Only while the mission is
/// active and before its deadline: a closed mission gets the audit event and
/// no new planning input. The observation carries no
/// public URL, so it cannot itself justify another `RequestCapability`.
pub fn update_capability_gap_at(
    state: &OrgState,
    gap_id: &CapabilityGapId,
    to: CapabilityGapStatus,
    now: DateTime<Utc>,
) -> Result<Outcome, ValidationError> {
    use CapabilityGapStatus as S;
    let invalid = |msg: String| ValidationError::new(ValidationCode::InvalidInput, None, msg);
    let st = state.clone();
    let gap = st
        .capability_gaps
        .get(gap_id)
        .ok_or_else(|| invalid(format!("unknown capability gap {gap_id}")))?;
    let allowed = matches!(
        (&gap.status, &to),
        (S::Open, S::Acquiring { .. })
            | (S::Open, S::Refused { .. })
            | (S::Acquiring { .. }, S::Acquired { .. })
            | (S::Acquiring { .. }, S::Refused { .. })
            | (S::Acquired { .. }, S::Installed { .. })
            | (S::Acquired { .. }, S::Refused { .. })
            // Publication is a separate effect: a verified gap may gain its
            // confirmed reference once, without rebinding (checked below).
            | (S::Acquired { publication_ref: None, .. }, S::Acquired { publication_ref: Some(_), .. })
    );
    if !allowed {
        return Err(ValidationError::new(
            ValidationCode::InvalidTransition,
            None,
            format!(
                "capability gap {gap_id} cannot move from {:?} to {to:?}",
                gap.status
            ),
        ));
    }
    let bounded = |s: &str| !s.trim().is_empty() && s.chars().count() <= 400;
    let exact = |c: &CapabilityRef| {
        c.capability_id == gap.capability_id
            && !c.version.trim().is_empty()
            && !c.version.eq_ignore_ascii_case("latest")
    };
    // Every later state stays bound to the acquisition that was claimed.
    let same_acquisition = |id: &str| gap.status.acquisition_id().is_none_or(|bound| bound == id);
    match &to {
        S::Open => return Err(invalid("a gap cannot reopen".into())),
        S::Acquiring { acquisition_id } if !bounded(acquisition_id) => {
            return Err(invalid("acquisition_id must be 1-400 characters".into()))
        }
        S::Acquired {
            acquisition_id,
            capability,
            publication_ref,
        } => {
            if let S::Acquired {
                capability: verified,
                ..
            } = &gap.status
            {
                if capability != verified {
                    return Err(invalid(
                        "recording publication cannot change the verified capability".into(),
                    ));
                }
            }
            if !same_acquisition(acquisition_id) || !exact(capability) {
                return Err(invalid(
                    "acquired needs the claimed acquisition and this gap's capability at an exact version".into(),
                ));
            }
            if publication_ref.as_deref().is_some_and(|p| !bounded(p)) {
                return Err(invalid("publication_ref must be 1-400 characters".into()));
            }
        }
        S::Installed {
            acquisition_id,
            capability,
            installation_id,
        } => {
            let S::Acquired {
                capability: verified,
                ..
            } = &gap.status
            else {
                unreachable!("transition table");
            };
            if !same_acquisition(acquisition_id) || capability != verified {
                return Err(invalid(
                    "installed must name the acquired capability version and acquisition".into(),
                ));
            }
            let accepted = st.installations.get(installation_id).is_some_and(|i| {
                i.capability == *capability && i.status == InstallationStatus::Accepted
            });
            if !accepted {
                return Err(invalid(format!(
                    "installation {installation_id} is not an accepted local installation of {}@{}",
                    capability.capability_id, capability.version
                )));
            }
        }
        S::Refused {
            reason,
            acquisition_id,
        } => {
            if !bounded(reason) {
                return Err(invalid("refusal needs a 1-400 character reason".into()));
            }
            if let Some(id) = acquisition_id {
                if !same_acquisition(id) {
                    return Err(invalid("refusal names a different acquisition".into()));
                }
            } else if gap.status.acquisition_id().is_some() {
                return Err(invalid("refusal must keep the bound acquisition_id".into()));
            }
        }
        S::Acquiring { .. } => {}
    }
    Ok(apply_gap_status(st, gap_id, to, now))
}

/// Backend-verified adoption of an installed capability for a gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionInstall {
    /// The adoption intent that selected, checked and accepted it.
    pub adoption_id: String,
    /// The exact capability version adopted; its ID may differ from the gap's
    /// own name, because adoption discovers by requirement, not by name.
    pub capability: CapabilityRef,
    pub installation_id: InstallationId,
}

/// Record a gap `installed` from an accepted adoption (backend only).
///
/// This is the one way a gap moves straight from `open` to `installed`, and
/// the one way it may name a capability other than its own ID. The caller
/// (the server's gap adoption coordinator) must hold a backend adoption
/// record that selected this exact version and accepted this installation;
/// the kernel checks the parts it can see: the gap is open, the version is
/// exact, and the installation is an accepted local installation of exactly
/// that capability. [`update_capability_gap_at`] still refuses
/// `open → installed` for any acquisition ID, whatever its prefix.
pub fn install_capability_gap_by_adoption_at(
    state: &OrgState,
    gap_id: &CapabilityGapId,
    adoption: &AdoptionInstall,
    now: DateTime<Utc>,
) -> Result<Outcome, ValidationError> {
    let invalid = |msg: String| ValidationError::new(ValidationCode::InvalidInput, None, msg);
    let gap = state
        .capability_gaps
        .get(gap_id)
        .ok_or_else(|| invalid(format!("unknown capability gap {gap_id}")))?;
    if gap.status != CapabilityGapStatus::Open {
        return Err(ValidationError::new(
            ValidationCode::InvalidTransition,
            None,
            format!(
                "capability gap {gap_id} can only be installed by adoption while open, not {:?}",
                gap.status
            ),
        ));
    }
    let id = adoption.adoption_id.trim();
    if id.is_empty() || adoption.adoption_id.chars().count() > 400 {
        return Err(invalid("adoption_id must be 1-400 characters".into()));
    }
    let c = &adoption.capability;
    if !valid_capability_name(c.capability_id.as_str())
        || c.version.trim().is_empty()
        || c.version.eq_ignore_ascii_case("latest")
    {
        return Err(invalid(
            "adoption must name an exact capability version".into(),
        ));
    }
    let accepted = state
        .installations
        .get(&adoption.installation_id)
        .is_some_and(|i| i.capability == *c && i.status == InstallationStatus::Accepted);
    if !accepted {
        return Err(invalid(format!(
            "installation {} is not an accepted local installation of {}@{}",
            adoption.installation_id, c.capability_id, c.version
        )));
    }
    let to = CapabilityGapStatus::Installed {
        acquisition_id: adoption.adoption_id.clone(),
        capability: c.clone(),
        installation_id: adoption.installation_id.clone(),
    };
    Ok(apply_gap_status(state.clone(), gap_id, to, now))
}

/// Apply an already validated gap status change: the audit event and, for a
/// final status on a runnable mission, the planner's wake-up observation.
fn apply_gap_status(
    mut st: OrgState,
    gap_id: &CapabilityGapId,
    to: CapabilityGapStatus,
    now: DateTime<Utc>,
) -> Outcome {
    use CapabilityGapStatus as S;
    let gap = st.capability_gaps.get_mut(gap_id).expect("checked");
    let from = std::mem::replace(&mut gap.status, to.clone());
    let note = match &to {
        S::Installed {
            capability,
            installation_id,
            ..
        } => Some(format!(
            "Requested capability {}@{} is installed and approved ({installation_id}); role {} may attach it for requirement {}",
            capability.capability_id, capability.version, gap.role_id, gap.requirement_id
        )),
        S::Refused { reason, .. } => Some(format!(
            "Requested capability {} for role {} was refused: {reason}",
            gap.capability_id, gap.role_id
        )),
        _ => None,
    };
    let mission_id = gap.mission_id.clone();
    // Wake the planner only for a mission that can still act on it. A gap of a
    // finished, failed or expired mission still records its final status
    // (the event above is the audit), but no fresh planning input.
    let runnable = st
        .missions
        .get(&mission_id)
        .is_some_and(|m| m.status == MissionStatus::Active && m.deadline.is_none_or(|d| d > now));
    let note = note.filter(|_| runnable);
    let mut effects = vec![EventKind::CapabilityGapStatusChanged {
        gap_id: gap_id.clone(),
        from,
        to,
    }];
    if let Some(summary) = note {
        let observation_id = ObservationId::generate();
        st.observations.insert(
            observation_id.clone(),
            Observation {
                observation_id: observation_id.clone(),
                mission_id,
                source: CAPABILITY_GAP_SOURCE.to_owned(),
                summary: summary.clone(),
                evidence_ref: Some(format!("genesis://capability-gaps/{gap_id}")),
                observed_at: now,
            },
        );
        effects.push(EventKind::ObservationRecorded {
            observation_id,
            source: CAPABILITY_GAP_SOURCE.to_owned(),
            summary,
        });
    }
    st.revision += 1;
    Outcome { state: st, effects }
}

// ---------------------------------------------------------------------------
// Capability installation
// ---------------------------------------------------------------------------

/// Org-local catalog: installed versions that passed local acceptance count as
/// approved; others are visible but unusable.
pub fn local_catalog(state: &OrgState) -> crate::capability::StaticCatalog {
    let mut cat = crate::capability::StaticCatalog::default();
    for inst in state.installations.values() {
        let s = match inst.status {
            InstallationStatus::Accepted => CapabilityState::Approved,
            InstallationStatus::Checking => CapabilityState::Testing,
            InstallationStatus::Rejected | InstallationStatus::Uninstalled => {
                CapabilityState::Rejected
            }
        };
        cat.0.insert(inst.capability.clone(), s);
    }
    cat
}

/// Start installing a published manifest. Compatibility is checked against
/// this organization's constitution; local acceptance follows separately.
pub fn begin_installation(
    state: &OrgState,
    manifest: &CapabilityManifest,
    configuration: serde_json::Value,
    now: DateTime<Utc>,
) -> Result<(Outcome, InstallationId), Vec<ValidationError>> {
    if let Err(problems) = manifest.check_compatibility(&state.constitution) {
        return Err(problems
            .into_iter()
            .map(|p| ValidationError::new(ValidationCode::CapabilityNotApproved, None, p))
            .collect());
    }
    let reference = manifest.reference();
    if state.installations.values().any(|i| {
        i.capability == reference
            && matches!(
                i.status,
                InstallationStatus::Checking | InstallationStatus::Accepted
            )
    }) {
        return Err(vec![ValidationError::new(
            ValidationCode::InvalidInput,
            None,
            format!(
                "{}@{} is already installed",
                reference.capability_id, reference.version
            ),
        )]);
    }
    let mut st = state.clone();
    let id = InstallationId::generate();
    st.installations.insert(
        id.clone(),
        Installation {
            installation_id: id.clone(),
            capability: reference.clone(),
            status: InstallationStatus::Checking,
            configuration,
            acceptance_report_ref: None,
            source_digest: manifest.source_digest.clone(),
            installed_at: now,
        },
    );
    st.revision += 1;
    Ok((
        Outcome {
            state: st,
            effects: vec![EventKind::CapabilityInstalled {
                installation_id: id.clone(),
                capability: reference,
                source_digest: manifest.source_digest.clone(),
                synthesis_calls: 0,
            }],
        },
        id,
    ))
}

/// Record the result of local acceptance for an installation.
pub fn finish_installation(
    state: &OrgState,
    id: &InstallationId,
    passed: bool,
    report_ref: &str,
    reasons: Vec<String>,
) -> Result<Outcome, ValidationError> {
    let mut st = state.clone();
    let inst = st.installations.get_mut(id).ok_or_else(|| {
        ValidationError::new(
            ValidationCode::InvalidInput,
            None,
            format!("unknown installation {id}"),
        )
    })?;
    let to = if passed {
        InstallationStatus::Accepted
    } else {
        InstallationStatus::Rejected
    };
    inst.status = inst.status.transition(to).map_err(|e| {
        ValidationError::new(ValidationCode::InvalidTransition, None, e.to_string())
    })?;
    inst.acceptance_report_ref = Some(report_ref.to_owned());
    st.revision += 1;
    let effect = if passed {
        EventKind::InstallationAccepted {
            installation_id: id.clone(),
            report_ref: report_ref.to_owned(),
        }
    } else {
        EventKind::InstallationRejected {
            installation_id: id.clone(),
            reasons,
        }
    };
    Ok(Outcome {
        state: st,
        effects: vec![effect],
    })
}
