//! Prompt construction.
//!
//! The system prompt is domain-neutral on purpose: it describes the mutation
//! vocabulary and the decision rules from TECH_SPEC §5, never a scenario.
//! The organization's role names, charters and responsibilities come only
//! from the evidence and current state. A test guards against scenario words
//! leaking into the static prompt.

use genesis_core::lifecycle::{ContractState, InstallationStatus, ResponsibilityStatus, RoleState};
use genesis_core::model::{CapabilityGapStatus, Executor, Observation, OrgState, Owner};
use std::fmt::Write;

/// An externally discovered specialist the planner may recruit. Supplied by
/// discovery (Fetch/Agentverse); the planner never invents addresses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KnownSpecialist {
    pub address: String,
    pub protocol: String,
    pub description: String,
}

pub const SYSTEM_PROMPT: &str = r#"You are the planning agent for an organization run by the GENESIS kernel.

You do not perform mission work yourself. When new evidence arrives, you decide whether the organization must change so the mission still succeeds. If it must, you propose the smallest valid change. The kernel validates every proposal against its constitution and may reject it. You cannot commit anything yourself.

Decision procedure
1. Decide whether the evidence affects the mission. If it does not, the verdict is "no_change".
2. If it does, state the new requirement: what must become true, and how anyone can check it is satisfied.
3. Choose the least disruptive change that gives that requirement exactly one accountable owner. Prefer, in this order:
   a. assign the requirement to an existing ACTIVE role whose charter already covers it;
   b. attach an installed, approved capability to an existing role;
   c. if the work needs an executable capability that is not installed and not already requested (for example reading an unfamiliar public data source), request it with RequestCapability for the accountable role, after assigning that role the responsibility in the same change;
   d. recruit a known external specialist for bounded work, supervised by an accountable role;
   e. create a new role.
   Create a new role only for a distinct continuing responsibility with a reason for separate execution: concurrency, authority, independent_ownership, expertise or lifecycle. Put that reason in "justification".
4. Every role you create must own or execute at least one responsibility in the same change.
5. Grant only scopes listed in the constitution, and only the scopes the work needs. Keep total budget within what remains.
6. Use only IDs that appear in the state below. Refer to things you create in this change by their "key". Recruit only specialists from the known-specialists list, using their exact address.
7. RequestCapability only states the need. You never choose how it is built, tested or approved; the kernel and an operator policy decide. Its observed_sources must be public URLs on the same site as the evidence_ref of the observation behind the requirement. Attach a requested capability only once it appears under "Installed capabilities"; acquired or published is not installed. Never request a capability that is already requested and not refused.
8. If you cannot produce a valid proposal, the verdict is "escalate" and the rationale says what a human must decide.

Output exactly one JSON object and nothing else:
{
  "assessment": "one or two sentences: what the evidence means for the mission",
  "verdict": "change" | "no_change" | "escalate",
  "requirement": {"summary": "...", "acceptance": "..."} | null,
  "rationale": "why this change (or none) is the least disruptive valid option",
  "budget_limit_cents": integer | null,
  "operations": [ ... ]
}
"requirement" and "operations" are required when the verdict is "change". Write "$REQUIREMENT" wherever an operation needs the ID of the requirement you stated.

Operation vocabulary (field "op" selects the operation)
- CreateRole: {"op":"CreateRole","key":str,"name":str,"charter":str,"authority":{"scopes":[str],"budget_cents":int},"reports_to":RoleRef|null,"provider":"elastic"|"creao"|"internal","justification":"concurrency"|"authority"|"independent_ownership"|"expertise"|"lifecycle"}
  provider "elastic" = an internal specialist that reasons over organization evidence; "creao" = a persistent operational role for recurring or long-running work.
- AssignResponsibility: {"op":"AssignResponsibility","target":Target,"owner":RoleRef}
  Target is {"kind":"from_requirement","key":str,"requirement_id":"$REQUIREMENT","purpose":str,"trigger":str,"termination":str}
  or {"kind":"existing","responsibility_id":str} for a responsibility currently held by the mission controller.
- RecruitSpecialist: {"op":"RecruitSpecialist","key":str,"specialist_address":str,"protocol":str,"scope":str,"deadline":null,"supervising_role":RoleRef|null}
- DelegateTask: {"op":"DelegateTask","responsibility":{"kind":"new","key":str}|{"kind":"existing","responsibility_id":str},"executor":{"kind":"role","role":RoleRef}|{"kind":"contractor","contract":{"kind":"new","key":str}|{"kind":"existing","contract_id":str}},"task":str}
  Delegation changes who executes, never who is accountable.
- ReassignResponsibility: {"op":"ReassignResponsibility","responsibility_id":str,"from":role_id,"to":RoleRef}
- AttachCapability: {"op":"AttachCapability","role":RoleRef,"capability":{"capability_id":str,"version":str}}
- ChangeReportingEdge: {"op":"ChangeReportingEdge","role":RoleRef,"reports_to":RoleRef|null}
- RequestCapability: {"op":"RequestCapability","key":str,"requirement_id":"$REQUIREMENT","role":RoleRef,"responsibility":{"kind":"new","key":str}|{"kind":"existing","responsibility_id":str},"capability_id":str,"purpose":str,"observed_sources":[str]}
  capability_id is a short lowercase name (a-z, 0-9, "-", "."). The role must own the responsibility when this operation applies.
- RetireRole: {"op":"RetireRole","role_id":str,"handoffs":[{"responsibility_id":str,"to":RoleRef}]}
  A role can retire only after every open responsibility it owns is handed off.
RoleRef is {"kind":"existing","role_id":str} or {"kind":"new","key":str}. Operations apply in order; a "new" key must be created by an earlier operation in the same list.
"#;

fn money(cents: u64) -> String {
    format!("${:.2}", cents as f64 / 100.0)
}

/// The user message: current state plus the new evidence.
pub fn user_message(
    state: &OrgState,
    observation: &Observation,
    specialists: &[KnownSpecialist],
) -> String {
    user_message_with(state, observation, specialists, &[])
}

/// A capability the server's trusted gap policy can act on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Requestable {
    pub capability_id: String,
    pub observed_sources: Vec<String>,
}

/// As [`user_message`], plus the capabilities the server's trusted gap policy
/// can act on. A request under another name, or without the policy's
/// sources, is refused, so when they are known the model is told to use them.
pub fn user_message_with(
    state: &OrgState,
    observation: &Observation,
    specialists: &[KnownSpecialist],
    requestable: &[Requestable],
) -> String {
    let mut s = String::new();
    let mission = state.missions.get(&observation.mission_id);
    let _ = writeln!(s, "## Mission");
    match mission {
        Some(m) => {
            let _ = writeln!(s, "Goal: {}", m.goal);
            if !m.constraints.is_empty() {
                let _ = writeln!(s, "Constraints: {}", m.constraints.join("; "));
            }
            if !m.success_checks.is_empty() {
                let _ = writeln!(s, "Success checks: {}", m.success_checks.join("; "));
            }
            if let Some(d) = m.deadline {
                let _ = writeln!(s, "Deadline: {d}");
            }
        }
        None => {
            let _ = writeln!(s, "(mission {} not found)", observation.mission_id);
        }
    }

    let c = &state.constitution;
    let allocated = state.allocated_budget_cents();
    let _ = writeln!(s, "\n## Constitution (revision {})", state.revision);
    let _ = writeln!(
        s,
        "Grantable scopes: {}",
        c.scopes.iter().cloned().collect::<Vec<_>>().join(", ")
    );
    let _ = writeln!(
        s,
        "Budget: {} total, {} allocated, {} remaining",
        money(c.budget_cents),
        money(allocated),
        money(c.budget_cents.saturating_sub(allocated))
    );
    let live = state.live_roles().count();
    let _ = writeln!(s, "Roles: {live} of at most {}", c.max_active_roles);
    let _ = writeln!(
        s,
        "Open contracts: {} of at most {}",
        state.active_contracts(),
        c.max_active_contracts
    );

    let _ = writeln!(s, "\n## Roles");
    for r in state
        .roles
        .values()
        .filter(|r| r.state != RoleState::Retired)
    {
        let _ = writeln!(
            s,
            "- {} \"{}\" [{:?}] provider={} reports_to={} scopes=[{}] budget={}",
            r.role_id,
            r.name,
            r.state,
            r.binding
                .as_ref()
                .map(|b| format!("{:?}", b.provider).to_lowercase())
                .unwrap_or_else(|| "none".into()),
            r.reports_to
                .as_ref()
                .map(|x| x.to_string())
                .unwrap_or_else(|| "none".into()),
            r.authority
                .scopes
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            money(r.authority.budget_cents),
        );
        let _ = writeln!(s, "  charter: {}", r.charter);
        if !r.capabilities.is_empty() {
            let caps: Vec<_> = r
                .capabilities
                .iter()
                .map(|c| format!("{}@{}", c.capability_id, c.version))
                .collect();
            let _ = writeln!(s, "  capabilities: {}", caps.join(", "));
        }
    }

    let _ = writeln!(s, "\n## Open responsibilities");
    let open: Vec<_> = state
        .responsibilities
        .values()
        .filter(|r| r.status == ResponsibilityStatus::Open)
        .collect();
    if open.is_empty() {
        let _ = writeln!(s, "(none)");
    }
    for r in open {
        let owner = match &r.owner {
            Owner::MissionController => "mission controller".to_string(),
            Owner::Role(id) => id.to_string(),
        };
        let exec = match &r.executor {
            None => String::new(),
            Some(Executor::Role(id)) => format!(" executor={id}"),
            Some(Executor::Contractor(id)) => format!(" executor=contract {id}"),
        };
        let _ = writeln!(
            s,
            "- {} \"{}\" owner={}{} epoch={}",
            r.responsibility_id, r.purpose, owner, exec, r.epoch
        );
    }

    let unmet: Vec<_> = state
        .requirements
        .values()
        .filter(|r| r.mission_id == observation.mission_id)
        .collect();
    if !unmet.is_empty() {
        let _ = writeln!(s, "\n## Requirements already recorded");
        for r in unmet {
            let _ = writeln!(
                s,
                "- {} [{:?}] {} (done when: {})",
                r.requirement_id, r.status, r.summary, r.acceptance
            );
        }
    }

    let contracts: Vec<_> = state
        .contracts
        .values()
        .filter(|c| !matches!(c.state, ContractState::Released | ContractState::Declined))
        .collect();
    if !contracts.is_empty() {
        let _ = writeln!(s, "\n## Contracts");
        for c in contracts {
            let _ = writeln!(
                s,
                "- {} {} [{:?}] scope: {}",
                c.contract_id, c.specialist_address, c.state, c.scope
            );
        }
    }

    let _ = writeln!(s, "\n## Installed capabilities");
    let installed: Vec<_> = state
        .installations
        .values()
        .filter(|i| i.status == InstallationStatus::Accepted)
        .collect();
    if installed.is_empty() {
        let _ = writeln!(s, "(none)");
    }
    for i in installed {
        let _ = writeln!(
            s,
            "- {}@{} (approved)",
            i.capability.capability_id, i.capability.version
        );
    }

    let gaps: Vec<_> = state.capability_gaps.values().collect();
    if !gaps.is_empty() {
        let _ = writeln!(s, "\n## Requested capabilities");
        for g in gaps {
            let status = match &g.status {
                CapabilityGapStatus::Open => "requested, not yet acquiring".to_owned(),
                CapabilityGapStatus::Acquiring { .. } => "acquiring".to_owned(),
                CapabilityGapStatus::Acquired { capability, .. } => format!(
                    "verified as {}@{}, not installed yet",
                    capability.capability_id, capability.version
                ),
                CapabilityGapStatus::Installed { capability, .. } => format!(
                    "installed as {}@{}; attach it",
                    capability.capability_id, capability.version
                ),
                CapabilityGapStatus::Refused { reason, .. } => format!("refused: {reason}"),
            };
            let _ = writeln!(
                s,
                "- {} for role {} / responsibility {} (requirement {}): {status}",
                g.capability_id, g.role_id, g.responsibility_id, g.requirement_id
            );
        }
    }

    if !requestable.is_empty() {
        let _ = writeln!(s, "\n## Requestable capabilities (operator policy)");
        let _ = writeln!(
            s,
            "Only these can be acquired or adopted here. If you use RequestCapability, use one of these capability_id values with exactly its observed_sources; if none fits the need, do not request a capability."
        );
        for r in requestable {
            let _ = writeln!(
                s,
                "- {}: observed_sources {}",
                r.capability_id,
                r.observed_sources.join(" ")
            );
        }
    }

    let _ = writeln!(s, "\n## Known external specialists");
    if specialists.is_empty() {
        let _ = writeln!(s, "(none discovered; do not use RecruitSpecialist)");
    }
    for k in specialists {
        let _ = writeln!(
            s,
            "- {} protocol={} — {}",
            k.address, k.protocol, k.description
        );
    }

    let mut earlier: Vec<_> = state
        .observations
        .values()
        .filter(|o| {
            o.mission_id == observation.mission_id && o.observation_id != observation.observation_id
        })
        .collect();
    earlier.sort_by_key(|o| o.observed_at);
    if !earlier.is_empty() {
        let _ = writeln!(s, "\n## Earlier evidence");
        for o in earlier.iter().rev().take(10).rev() {
            let _ = writeln!(s, "- {} [{}] {}", o.observation_id, o.source, o.summary);
        }
    }

    let _ = writeln!(s, "\n## New evidence");
    let _ = writeln!(s, "- id: {}", observation.observation_id);
    let _ = writeln!(s, "- source: {}", observation.source);
    let _ = writeln!(s, "- observed_at: {}", observation.observed_at);
    let _ = writeln!(s, "- summary: {}", observation.summary);
    if let Some(r) = &observation.evidence_ref {
        let _ = writeln!(s, "- evidence_ref: {r}");
    }
    let _ = writeln!(s, "\nDecide. Respond with the JSON object only.");
    s
}

#[cfg(test)]
mod tests {
    use super::SYSTEM_PROMPT;

    /// The static prompt must not hint at any particular scenario, so that
    /// role names and charters are derived from evidence.
    #[test]
    fn system_prompt_has_no_scenario_hints() {
        let lower = SYSTEM_PROMPT.to_lowercase();
        for word in [
            "venue",
            "relocat",
            "event",
            "attendee",
            "shuttle",
            "logistic",
            "monitor",
            "campus",
            "conference",
        ] {
            assert!(!lower.contains(word), "system prompt mentions `{word}`");
        }
    }
}
