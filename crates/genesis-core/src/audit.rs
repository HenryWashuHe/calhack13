//! Structural invariants of an organization state, checked independently of
//! the validator that is supposed to keep them.
//!
//! [`audit`] never mutates and never fails: it returns every violation it
//! finds. The engine fuzz runs it on every reachable state, and the server can
//! run it over a stored history (`genesis audit`). An empty result is the
//! claim "this state is well formed"; it says nothing about whether the
//! organization is doing useful work.

use crate::lifecycle::{
    AssignmentState, InstallationStatus, ResponsibilityStatus, RoleState, StateMachine,
};
use crate::model::{CapabilityGapStatus, Executor, OrgState, Owner};
use std::collections::BTreeMap;

/// Live (non-terminal) capability gaps an organization may hold.
pub use crate::engine::MAX_LIVE_CAPABILITY_GAPS;

/// One broken invariant.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Violation {
    /// Stable invariant name, one of [`INVARIANTS`].
    pub invariant: &'static str,
    pub detail: String,
}

/// Every invariant [`audit`] checks, by stable name.
pub const INVARIANTS: &[&str] = &[
    "role_limit",
    "budget_limit",
    "constitution_scopes",
    "role_revisions",
    "reporting_tree",
    "assignment_references",
    "single_active_assignment",
    "active_assignment_current",
    "live_owner",
    "executor_exists",
    "contract_limit",
    "gap_references",
    "installed_gap_has_installation",
    "live_gap_limit",
    "requirement_evidence",
];

fn dead(state: RoleState) -> bool {
    matches!(state, RoleState::Retired | RoleState::ProvisioningFailed)
}

/// Check `st` against every invariant in [`INVARIANTS`].
pub fn audit(st: &OrgState) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut v = |invariant: &'static str, detail: String| out.push(Violation { invariant, detail });
    let c = &st.constitution;

    let live = st.live_roles().count();
    if live as u64 > u64::from(c.max_active_roles) {
        v(
            "role_limit",
            format!("{live} live roles, limit {}", c.max_active_roles),
        );
    }
    match st.checked_allocated_budget_cents() {
        Some(b) if b <= c.budget_cents => {}
        Some(b) => v(
            "budget_limit",
            format!("live roles hold {b} cents, constitution {}", c.budget_cents),
        ),
        None => v("budget_limit", "live role budgets overflow".into()),
    }

    for r in st.roles.values() {
        if !r.authority.scopes.is_subset(&c.scopes) {
            v(
                "constitution_scopes",
                format!("{} holds scopes outside the constitution", r.role_id),
            );
        }
        if r.created_in_revision > st.revision {
            v(
                "role_revisions",
                format!("{} created after the current revision", r.role_id),
            );
        }
        if r.state == RoleState::Retired
            && !r
                .retired_in_revision
                .is_some_and(|x| x >= r.created_in_revision && x <= st.revision)
        {
            v(
                "role_revisions",
                format!("{} retired without a valid revision", r.role_id),
            );
        }
        if let Some(p) = &r.reports_to {
            if p == &r.role_id {
                v("reporting_tree", format!("{} reports to itself", r.role_id));
            } else if !st.roles.contains_key(p) {
                v(
                    "reporting_tree",
                    format!("{} reports to unknown {p}", r.role_id),
                );
            }
        }
        let mut cursor = r.reports_to.clone();
        let mut hops = 0;
        while let Some(p) = cursor {
            hops += 1;
            if hops > st.roles.len() {
                v(
                    "reporting_tree",
                    format!("reporting cycle through {}", r.role_id),
                );
                break;
            }
            cursor = st.roles.get(&p).and_then(|x| x.reports_to.clone());
        }
    }

    let mut active_per_responsibility = BTreeMap::new();
    for a in st.assignments.values() {
        let resp = st.responsibilities.get(&a.responsibility_id);
        let role = st.roles.get(&a.role_id);
        if resp.is_none() || role.is_none() {
            v(
                "assignment_references",
                format!(
                    "{} names an unknown role or responsibility",
                    a.assignment_id
                ),
            );
            continue;
        }
        if a.state != AssignmentState::Active {
            continue;
        }
        *active_per_responsibility
            .entry(&a.responsibility_id)
            .or_insert(0usize) += 1;
        let (resp, role) = (resp.expect("checked"), role.expect("checked"));
        if resp.status != ResponsibilityStatus::Open || a.epoch != resp.epoch || dead(role.state) {
            v(
                "active_assignment_current",
                format!(
                    "{} is active but its responsibility is {:?} at epoch {} (assignment epoch {}) and its role is {:?}",
                    a.assignment_id, resp.status, resp.epoch, a.epoch, role.state
                ),
            );
        }
    }
    for (id, n) in active_per_responsibility {
        if n > 1 {
            v(
                "single_active_assignment",
                format!("{id} has {n} active assignments"),
            );
        }
    }

    for r in st.responsibilities.values() {
        if r.status != ResponsibilityStatus::Open {
            continue;
        }
        if let Owner::Role(o) = &r.owner {
            if !st.roles.get(o).is_some_and(|x| !dead(x.state)) {
                v(
                    "live_owner",
                    format!(
                        "open {} is owned by dead or unknown {o}",
                        r.responsibility_id
                    ),
                );
            }
        }
        let executor_known = match &r.executor {
            None => true,
            Some(Executor::Role(e)) => st.roles.contains_key(e),
            Some(Executor::Contractor(k)) => st.contracts.contains_key(k),
        };
        if !executor_known {
            v(
                "executor_exists",
                format!("{} has an unknown executor", r.responsibility_id),
            );
        }
    }

    let contracts = st.active_contracts();
    if contracts as u64 > u64::from(c.max_active_contracts) {
        v(
            "contract_limit",
            format!(
                "{contracts} open contracts, limit {}",
                c.max_active_contracts
            ),
        );
    }

    let mut live_gaps = 0;
    for g in st.capability_gaps.values() {
        if !st.requirements.contains_key(&g.requirement_id)
            || !st.roles.contains_key(&g.role_id)
            || !st.responsibilities.contains_key(&g.responsibility_id)
            || !st.missions.contains_key(&g.mission_id)
            || g.opened_in_revision > st.revision
        {
            v(
                "gap_references",
                format!(
                    "{} names an unknown mission, requirement, role or responsibility, or a future revision",
                    g.gap_id
                ),
            );
        }
        if let CapabilityGapStatus::Installed {
            capability,
            installation_id,
            ..
        } = &g.status
        {
            let accepted = st.installations.get(installation_id).is_some_and(|i| {
                i.status == InstallationStatus::Accepted && &i.capability == capability
            });
            if !accepted {
                v(
                    "installed_gap_has_installation",
                    format!(
                        "{} is installed, but {installation_id} is not an accepted installation of {}@{}",
                        g.gap_id, capability.capability_id, capability.version
                    ),
                );
            }
        }
        if !g.status.is_terminal() {
            live_gaps += 1;
        }
    }
    if live_gaps > MAX_LIVE_CAPABILITY_GAPS {
        v(
            "live_gap_limit",
            format!("{live_gaps} live gaps, limit {MAX_LIVE_CAPABILITY_GAPS}"),
        );
    }

    for rq in st.requirements.values() {
        if let Some(o) = rq
            .triggering_evidence
            .iter()
            .find(|o| !st.observations.contains_key(*o))
        {
            v(
                "requirement_evidence",
                format!("{} cites unknown {o}", rq.requirement_id),
            );
        }
    }
    out
}

/// Every invariant [`audit_transition`] checks between two states.
pub const TRANSITION_INVARIANTS: &[&str] = &[
    "revision_advances",
    "history_is_append_only",
    "observations_immutable",
    "lifecycle_reachable",
    "epoch_monotonic",
    "gap_binding_stable",
];

/// Whether `to` is `from` or reachable from it through the transition table.
fn reachable<S: StateMachine>(from: S, to: S) -> bool {
    let mut seen = vec![from];
    let mut frontier = vec![from];
    while let Some(s) = frontier.pop() {
        if s == to {
            return true;
        }
        for &n in s.allowed_next() {
            if !seen.contains(&n) {
                seen.push(n);
                frontier.push(n);
            }
        }
    }
    false
}

fn gap_rank(s: &CapabilityGapStatus) -> u8 {
    match s {
        CapabilityGapStatus::Open => 0,
        CapabilityGapStatus::Acquiring { .. } => 1,
        CapabilityGapStatus::Acquired { .. } => 2,
        CapabilityGapStatus::Installed { .. } | CapabilityGapStatus::Refused { .. } => 3,
    }
}

/// Check that `next` is a legal later state of `prev`, possibly several
/// commits later: nothing recorded disappears, observations never change,
/// every lifecycle only moves along its transition table, epochs never go
/// back, and a gap keeps its acquisition and final outcome.
pub fn audit_transition(prev: &OrgState, next: &OrgState) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut v = |invariant: &'static str, detail: String| out.push(Violation { invariant, detail });
    if next.revision <= prev.revision {
        v(
            "revision_advances",
            format!("r{} follows r{}", next.revision, prev.revision),
        );
    }
    macro_rules! lifecycle {
        ($map:ident, $field:ident, $what:literal) => {
            for (id, before) in &prev.$map {
                match next.$map.get(id) {
                    None => v(
                        "history_is_append_only",
                        format!("{} {id} disappeared", $what),
                    ),
                    Some(after) if !reachable(before.$field, after.$field) => v(
                        "lifecycle_reachable",
                        format!(
                            "{} {id} moved {:?} -> {:?}",
                            $what, before.$field, after.$field
                        ),
                    ),
                    Some(_) => {}
                }
            }
        };
    }
    lifecycle!(missions, status, "mission");
    lifecycle!(roles, state, "role");
    lifecycle!(assignments, state, "assignment");
    lifecycle!(responsibilities, status, "responsibility");
    lifecycle!(requirements, status, "requirement");
    lifecycle!(contracts, state, "contract");
    lifecycle!(installations, status, "installation");
    for (id, before) in &prev.observations {
        match next.observations.get(id) {
            None => v(
                "history_is_append_only",
                format!("observation {id} disappeared"),
            ),
            Some(after) if after != before => v(
                "observations_immutable",
                format!("observation {id} changed"),
            ),
            Some(_) => {}
        }
    }
    for (id, before) in &prev.responsibilities {
        if let Some(after) = next.responsibilities.get(id) {
            if after.epoch < before.epoch {
                v(
                    "epoch_monotonic",
                    format!("{id} epoch {} -> {}", before.epoch, after.epoch),
                );
            }
        }
    }
    for (id, before) in &prev.assignments {
        if let Some(after) = next.assignments.get(id) {
            if after.epoch != before.epoch
                || after.role_id != before.role_id
                || after.responsibility_id != before.responsibility_id
            {
                v("epoch_monotonic", format!("assignment {id} was rebound"));
            }
        }
    }
    for (id, before) in &prev.capability_gaps {
        let Some(after) = next.capability_gaps.get(id) else {
            v(
                "history_is_append_only",
                format!("capability gap {id} disappeared"),
            );
            continue;
        };
        let rebound = before
            .status
            .acquisition_id()
            .is_some_and(|a| after.status.acquisition_id() != Some(a));
        let terminal_changed = before.status.is_terminal() && after.status != before.status;
        if gap_rank(&after.status) < gap_rank(&before.status) || terminal_changed {
            v(
                "lifecycle_reachable",
                format!(
                    "capability gap {id} moved {:?} -> {:?}",
                    before.status, after.status
                ),
            );
        }
        if rebound
            || after.requirement_id != before.requirement_id
            || after.requirement_digest != before.requirement_digest
            || after.capability_id != before.capability_id
            || after.role_id != before.role_id
        {
            v(
                "gap_binding_stable",
                format!("capability gap {id} changed its binding"),
            );
        }
    }
    out
}
