//! Every invariant of the kernel audit is pinned by a negative case.
//!
//! The audit refuses bad commits in demo mode (`GENESIS_AUDIT_COMMITS=1`), so
//! a check that silently stops firing is a real loss, and a test that only
//! asserts valid states are clean cannot notice it. Each case here starts from
//! a state built by real engine calls (which must pass both audits), breaks
//! exactly one thing by editing fields directly (the audit exists to catch
//! states the engine should never produce), and asserts the exact set of
//! invariants reported, with a detail fragment that tells apart the separate
//! checks sharing one invariant name (for example a self report from a cycle).
//!
//! `every_invariant_has_a_negative_case` fails when an invariant is added to
//! `INVARIANTS` or `TRANSITION_INVARIANTS` without a case here.

use chrono::Utc;
use genesis_core::audit::{
    audit, audit_transition, Violation, INVARIANTS, MAX_LIVE_CAPABILITY_GAPS, TRANSITION_INVARIANTS,
};
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::lifecycle::*;
use genesis_core::model::*;
use std::collections::{BTreeMap, BTreeSet};

const PORTAL: &str = "https://portal.example/org-a/venues/index.html";
const SOURCE: &str = "https://portal.example/org-a/venues/cards.html";
const PURPOSE: &str = "Read venue capacity and availability from the portal";
const ACQ: &str = "acq-1";

// --- Fixture -----------------------------------------------------------------

/// A valid organization built only through engine calls: coordinator, program
/// and a retired info role; an observation and a requirement assigned to
/// program; program's own work delegated to a recruited contractor; and one
/// open capability gap for the requirement.
struct Fx {
    state: OrgState,
    coordinator: RoleId,
    program: RoleId,
    info: RoleId,
    observation: ObservationId,
    coordinator_work: ResponsibilityId,
    program_work: ResponsibilityId,
    contract: ContractId,
    gap: CapabilityGapId,
}

/// The gap of [`Fx`] at each later engine stage.
struct Stages {
    acquiring: OrgState,
    acquired: OrgState,
    published: OrgState,
    installed: OrgState,
    refused: OrgState,
    installation: InstallationId,
}

fn role_named(state: &OrgState, name: &str) -> RoleId {
    state
        .roles
        .values()
        .find(|r| r.name == name)
        .unwrap()
        .role_id
        .clone()
}

fn work_named(state: &OrgState, purpose: &str) -> ResponsibilityId {
    state
        .responsibilities
        .values()
        .find(|r| r.purpose == purpose)
        .unwrap()
        .responsibility_id
        .clone()
}

fn bootstrap_role(key: &str, name: &str, scope: &str, budget: u64) -> BootstrapRole {
    BootstrapRole {
        key: key.into(),
        name: name.into(),
        charter: format!("{name} charter"),
        authority: AuthorityGrant::new([scope], budget),
        reports_to: (key != "coordinator").then(|| "coordinator".into()),
    }
}

fn bootstrap_work(purpose: &str, owner: &str) -> BootstrapResponsibility {
    BootstrapResponsibility {
        purpose: purpose.into(),
        owner: owner.into(),
        trigger: "events".into(),
        termination: "event ends".into(),
    }
}

fn commit_ops(state: &OrgState, evidence: &ObservationId, ops: Vec<OrgOp>) -> OrgState {
    let p = OrgChangeProposal {
        expected_org_revision: state.revision,
        trigger_evidence_ids: vec![evidence.clone()],
        rationale: "audit invariant fixture".into(),
        budget_limit_cents: None,
        operations: ops,
    };
    let alloc = KeyAllocation::allocate(&p);
    commit(
        state,
        &p,
        &alloc,
        &BTreeMap::new(),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap_or_else(|e| panic!("fixture commit failed: {e:?}"))
    .state
}

fn fx() -> Fx {
    let now = Utc::now();
    let state = OrgState::new(
        OrgId::new("org_a"),
        "Org A",
        Constitution {
            scopes: [
                "mission.coordinate",
                "program.schedule",
                "attendees.inform",
                "ledger.reserve_venue",
                "contract.logistics",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            budget_cents: 500_000,
            max_active_roles: 6,
            max_active_contracts: 2,
        },
    );
    let spec = MissionBootstrap {
        goal: "Run the campus event for 120 attendees".into(),
        constraints: vec![],
        deadline: None,
        success_checks: vec![],
        roles: vec![
            bootstrap_role("coordinator", "Coordinator", "mission.coordinate", 100_000),
            bootstrap_role("program", "Program", "program.schedule", 50_000),
            bootstrap_role("info", "Info", "attendees.inform", 10_000),
        ],
        responsibilities: vec![
            bootstrap_work("Coordinate the mission", "coordinator"),
            bootstrap_work("Maintain the schedule", "program"),
        ],
    };
    let (out, mission) = bootstrap_mission(&state, &spec, now).unwrap();
    let (out, observation) = record_observation(
        &out.state,
        &mission,
        "venue-portal",
        "Main hall closed for the event date",
        Some(PORTAL.into()),
        now,
    )
    .unwrap();
    let (out, requirement) = record_requirement(
        &out.state,
        &mission,
        "Relocate the event",
        std::slice::from_ref(&observation),
        "a feasible venue is reserved",
    )
    .unwrap();
    let state = out.state;
    let coordinator = role_named(&state, "Coordinator");
    let program = role_named(&state, "Program");
    let info = role_named(&state, "Info");
    let coordinator_work = work_named(&state, "Coordinate the mission");
    let program_work = work_named(&state, "Maintain the schedule");

    // Info owns nothing and retires early, so later commits follow it.
    let state = commit_ops(
        &state,
        &observation,
        vec![OrgOp::RetireRole {
            role_id: info.clone(),
            handoffs: vec![],
        }],
    );
    let state = commit_ops(
        &state,
        &observation,
        vec![OrgOp::AssignResponsibility {
            target: ResponsibilityTarget::FromRequirement {
                key: "relocate".into(),
                requirement_id: requirement.clone(),
                purpose: "Relocate the event".into(),
                trigger: "venue changes".into(),
                termination: "venue reserved".into(),
            },
            owner: RoleRef::existing(program.clone()),
        }],
    );
    let linked = state
        .responsibilities
        .values()
        .find(|r| r.requirement_id.as_ref() == Some(&requirement))
        .unwrap()
        .responsibility_id
        .clone();
    let state = commit_ops(
        &state,
        &observation,
        vec![
            OrgOp::RecruitSpecialist {
                key: "logistics".into(),
                specialist_address: "agent1qlogistics".into(),
                protocol: "genesis-contract/1".into(),
                scope: "transport plan".into(),
                deadline: None,
                supervising_role: None,
            },
            OrgOp::DelegateTask {
                responsibility: ResponsibilityRef::Existing {
                    responsibility_id: program_work.clone(),
                },
                executor: ExecutorRef::Contractor {
                    contract: ContractRef::New {
                        key: "logistics".into(),
                    },
                },
                task: "do the work".into(),
            },
        ],
    );
    let contract = state.contracts.keys().next().unwrap().clone();
    let state = commit_ops(
        &state,
        &observation,
        vec![OrgOp::RequestCapability {
            key: "reader".into(),
            requirement_id: requirement.clone(),
            role: RoleRef::existing(program.clone()),
            responsibility: ResponsibilityRef::Existing {
                responsibility_id: linked.clone(),
            },
            capability_id: CapabilityId::new("venue-availability"),
            purpose: PURPOSE.into(),
            observed_sources: vec![SOURCE.into()],
        }],
    );
    let gap = state.capability_gaps.keys().next().unwrap().clone();

    // The shape the cases rely on.
    assert_eq!(state.roles[&info].state, RoleState::Retired);
    assert!(state.revision > state.roles[&info].retired_in_revision.unwrap());
    assert_eq!(
        state.responsibilities[&program_work].executor,
        Some(Executor::Contractor(contract.clone()))
    );
    assert_eq!(state.active_contracts(), 1);
    assert_eq!(
        state.capability_gaps[&gap].status,
        CapabilityGapStatus::Open
    );
    Fx {
        state,
        coordinator,
        program,
        info,
        observation,
        coordinator_work,
        program_work,
        contract,
        gap,
    }
}

fn cap(version: &str) -> CapabilityRef {
    CapabilityRef::new("venue-availability", version)
}

fn acquired(version: &str, publication_ref: Option<&str>) -> CapabilityGapStatus {
    CapabilityGapStatus::Acquired {
        acquisition_id: ACQ.into(),
        capability: cap(version),
        publication_ref: publication_ref.map(String::from),
    }
}

fn step(state: &OrgState, gap: &CapabilityGapId, to: CapabilityGapStatus) -> OrgState {
    update_capability_gap(state, gap, to)
        .unwrap_or_else(|e| panic!("fixture gap step failed: {e:?}"))
        .state
}

fn stages(f: &Fx) -> Stages {
    use CapabilityGapStatus as S;
    let acquiring = step(
        &f.state,
        &f.gap,
        S::Acquiring {
            acquisition_id: ACQ.into(),
        },
    );
    let acquired_st = step(&acquiring, &f.gap, acquired("1.0.0", None));
    let published = step(
        &acquired_st,
        &f.gap,
        acquired("1.0.0", Some("registry:venue-availability@1.0.0")),
    );
    // The one non-engine seam: an accepted local installation is inserted
    // directly (as tests/capability_gap.rs does) instead of building a full
    // manifest for begin_installation/finish_installation.
    let installation = InstallationId::new("inst_1");
    let mut with_installation = published.clone();
    with_installation.installations.insert(
        installation.clone(),
        Installation {
            installation_id: installation.clone(),
            capability: cap("1.0.0"),
            status: InstallationStatus::Accepted,
            configuration: serde_json::Value::Null,
            acceptance_report_ref: Some("report:1".into()),
            source_digest: "sha256:x".into(),
            installed_at: Utc::now(),
        },
    );
    let installed = step(
        &with_installation,
        &f.gap,
        S::Installed {
            acquisition_id: ACQ.into(),
            capability: cap("1.0.0"),
            installation_id: installation.clone(),
        },
    );
    let refused = step(
        &f.state,
        &f.gap,
        S::Refused {
            reason: "policy refused the request".into(),
            acquisition_id: None,
        },
    );
    Stages {
        acquiring,
        acquired: acquired_st,
        published,
        installed,
        refused,
        installation,
    }
}

/// The same state one revision later: a no-op transition the audit accepts.
fn later(st: &OrgState) -> OrgState {
    let mut next = st.clone();
    next.revision += 1;
    next
}

// --- Cases -------------------------------------------------------------------

/// One break of a valid state. The audit must report a violation of
/// `invariant` whose detail contains `detail`, and the set of reported
/// invariant names must be exactly `invariant` plus `also`.
struct Case {
    label: String,
    invariant: &'static str,
    detail: &'static str,
    also: &'static [&'static str],
    prev: Option<OrgState>,
    state: OrgState,
}

fn case(
    label: &str,
    invariant: &'static str,
    detail: &'static str,
    base: &OrgState,
    f: impl FnOnce(&mut OrgState),
) -> Case {
    let mut state = base.clone();
    f(&mut state);
    Case {
        label: label.into(),
        invariant,
        detail,
        also: &[],
        prev: None,
        state,
    }
}

/// A transition case: `prev` to `later(prev)` with `f` applied to the later state.
fn tcase(
    label: &str,
    invariant: &'static str,
    detail: &'static str,
    prev: &OrgState,
    f: impl FnOnce(&mut OrgState),
) -> Case {
    let mut c = case(label, invariant, detail, &later(prev), f);
    c.prev = Some(prev.clone());
    c
}

impl Case {
    fn also(mut self, also: &'static [&'static str]) -> Self {
        self.also = also;
        self
    }

    fn violations(&self) -> Vec<Violation> {
        match &self.prev {
            None => audit(&self.state),
            Some(prev) => audit_transition(prev, &self.state),
        }
    }
}

/// Run every case and report all failures at once.
fn run(cases: Vec<Case>) {
    let mut failures = Vec::new();
    for c in &cases {
        let names = if c.prev.is_some() {
            TRANSITION_INVARIANTS
        } else {
            INVARIANTS
        };
        assert!(names.contains(&c.invariant), "{}: unknown name", c.label);
        let found = c.violations();
        let hit = found
            .iter()
            .any(|v| v.invariant == c.invariant && v.detail.contains(c.detail));
        let got: BTreeSet<&str> = found.iter().map(|v| v.invariant).collect();
        let want: BTreeSet<&str> = std::iter::once(c.invariant)
            .chain(c.also.iter().copied())
            .collect();
        if !hit || got != want {
            failures.push(format!(
                "{}: expected {} ({:?}) plus {:?}, got {found:?}",
                c.label, c.invariant, c.detail, c.also
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A direct edit of one capability gap (given the state's revision).
type GapEditAt = fn(&mut CapabilityGap, u64);
type GapEdit = fn(&mut CapabilityGap);

fn active_assignment_of(st: &OrgState, r: &ResponsibilityId) -> AssignmentId {
    st.active_assignment(r).unwrap().assignment_id.clone()
}

fn role_cases(f: &Fx) -> Vec<Case> {
    let s = &f.state;
    let (coordinator, program, info) = (&f.coordinator, &f.program, &f.info);
    vec![
        case("role_limit", "role_limit", "live roles, limit", s, |st| {
            st.constitution.max_active_roles = 1
        }),
        case(
            "budget over",
            "budget_limit",
            "cents, constitution",
            s,
            |st| st.constitution.budget_cents = 1,
        ),
        case("budget overflow", "budget_limit", "overflow", s, |st| {
            st.roles
                .get_mut(coordinator)
                .unwrap()
                .authority
                .budget_cents = u64::MAX
        }),
        case(
            "scope outside constitution",
            "constitution_scopes",
            "outside the constitution",
            s,
            |st| {
                let r = st.roles.get_mut(program).unwrap();
                r.authority.scopes.insert("root".into());
            },
        ),
        // L81
        case(
            "role created in a future revision",
            "role_revisions",
            "created after the current revision",
            s,
            |st| st.roles.get_mut(coordinator).unwrap().created_in_revision = st.revision + 1,
        ),
        // L91, each way `retired_in_revision` can be invalid.
        case(
            "retired with no revision",
            "role_revisions",
            "retired without a valid revision",
            s,
            |st| st.roles.get_mut(info).unwrap().retired_in_revision = None,
        ),
        case(
            "retired in a future revision",
            "role_revisions",
            "retired without a valid revision",
            s,
            |st| st.roles.get_mut(info).unwrap().retired_in_revision = Some(st.revision + 1),
        ),
        case(
            "retired before it was created",
            "role_revisions",
            "retired without a valid revision",
            s,
            |st| {
                let r = st.roles.get_mut(info).unwrap();
                r.created_in_revision = r.retired_in_revision.unwrap() + 1;
            },
        ),
        // L98. A self report is also a one-role cycle (L111).
        case(
            "reports to itself",
            "reporting_tree",
            "reports to itself",
            s,
            |st| st.roles.get_mut(program).unwrap().reports_to = Some(program.clone()),
        ),
        // L100
        case(
            "reports to an unknown role",
            "reporting_tree",
            "reports to unknown role_ghost",
            s,
            |st| st.roles.get_mut(program).unwrap().reports_to = Some(RoleId::new("role_ghost")),
        ),
        // L111: program already reports to the coordinator.
        case(
            "two-role reporting cycle",
            "reporting_tree",
            "reporting cycle through",
            s,
            |st| st.roles.get_mut(coordinator).unwrap().reports_to = Some(program.clone()),
        ),
    ]
}

fn work_cases(f: &Fx) -> Vec<Case> {
    let s = &f.state;
    let coord_asg = active_assignment_of(s, &f.coordinator_work);
    let (coordinator, coordinator_work, program_work) =
        (&f.coordinator, &f.coordinator_work, &f.program_work);
    vec![
        // L126, both halves of the condition.
        case(
            "assignment names an unknown role",
            "assignment_references",
            "unknown role or responsibility",
            s,
            |st| st.assignments.get_mut(&coord_asg).unwrap().role_id = RoleId::new("role_ghost"),
        ),
        case(
            "assignment names an unknown responsibility",
            "assignment_references",
            "unknown role or responsibility",
            s,
            |st| {
                st.assignments
                    .get_mut(&coord_asg)
                    .unwrap()
                    .responsibility_id = ResponsibilityId::new("rsp_ghost")
            },
        ),
        // L143, each part of the condition.
        case(
            "active assignment at a stale epoch",
            "active_assignment_current",
            "is active but",
            s,
            |st| st.assignments.get_mut(&coord_asg).unwrap().epoch += 1,
        ),
        case(
            "active assignment of completed work",
            "active_assignment_current",
            "is active but",
            s,
            |st| {
                st.responsibilities
                    .get_mut(coordinator_work)
                    .unwrap()
                    .status = ResponsibilityStatus::Completed
            },
        ),
        case(
            "active assignment of a dead role",
            "active_assignment_current",
            "is active but",
            s,
            |st| st.roles.get_mut(coordinator).unwrap().state = RoleState::ProvisioningFailed,
        )
        // The dead coordinator still owns its open work.
        .also(&["live_owner"]),
        case(
            "two active assignments",
            "single_active_assignment",
            "active assignments",
            s,
            |st| {
                let mut a = st.assignments[&coord_asg].clone();
                a.assignment_id = AssignmentId::new("asg_dup");
                st.assignments.insert(a.assignment_id.clone(), a);
            },
        ),
        case(
            "open work owned by an unknown role",
            "live_owner",
            "owned by dead or unknown",
            s,
            |st| {
                st.responsibilities.get_mut(coordinator_work).unwrap().owner =
                    Owner::Role(RoleId::new("role_ghost"))
            },
        ),
        // L182, both executor kinds.
        case(
            "executor is an unknown contract",
            "executor_exists",
            "unknown executor",
            s,
            |st| {
                st.responsibilities.get_mut(program_work).unwrap().executor =
                    Some(Executor::Contractor(ContractId::new("ctr_ghost")))
            },
        ),
        case(
            "executor is an unknown role",
            "executor_exists",
            "unknown executor",
            s,
            |st| {
                st.responsibilities
                    .get_mut(coordinator_work)
                    .unwrap()
                    .executor = Some(Executor::Role(RoleId::new("role_ghost")))
            },
        ),
        // L191: the recruited contract is still active.
        case(
            "contract over the limit",
            "contract_limit",
            "open contracts, limit",
            s,
            |st| st.constitution.max_active_contracts = 0,
        ),
    ]
}

fn gap_cases(f: &Fx, g: &Stages) -> Vec<Case> {
    let s = &f.state;
    let gap = &f.gap;
    let set = |st: &mut OrgState, apply: &dyn Fn(&mut CapabilityGap)| {
        apply(st.capability_gaps.get_mut(gap).unwrap())
    };
    let ghost_cases: Vec<(&str, GapEditAt)> = vec![
        ("unknown requirement", |g, _| {
            g.requirement_id = RequirementId::new("req_ghost")
        }),
        ("unknown role", |g, _| g.role_id = RoleId::new("role_ghost")),
        ("unknown responsibility", |g, _| {
            g.responsibility_id = ResponsibilityId::new("rsp_ghost")
        }),
        ("unknown mission", |g, _| {
            g.mission_id = MissionId::new("mission_ghost")
        }),
        ("future revision", |g, rev| g.opened_in_revision = rev + 1),
    ];
    // L230, each part of the condition.
    let mut cases: Vec<Case> = ghost_cases
        .into_iter()
        .map(|(label, apply)| {
            case(
                &format!("gap names an {label}"),
                "gap_references",
                "names an unknown mission",
                s,
                |st| {
                    let rev = st.revision;
                    set(st, &|g| apply(g, rev));
                },
            )
        })
        .collect();
    let inst = g.installation.clone();
    // L248, each way the installation can fail to back the gap.
    cases.extend([
        case(
            "installed gap, rejected installation",
            "installed_gap_has_installation",
            "is not an accepted installation",
            &g.installed,
            |st| st.installations.get_mut(&inst).unwrap().status = InstallationStatus::Rejected,
        ),
        case(
            "installed gap, installation of another version",
            "installed_gap_has_installation",
            "is not an accepted installation",
            &g.installed,
            |st| st.installations.get_mut(&inst).unwrap().capability = cap("2.0.0"),
        ),
        case(
            "installed gap, installation missing",
            "installed_gap_has_installation",
            "is not an accepted installation",
            &g.installed,
            |st| {
                st.installations.remove(&inst);
            },
        ),
    ]);
    // L262: one live gap more than the limit (the open fixture gap plus clones).
    cases.push(case(
        "one live gap over the limit",
        "live_gap_limit",
        "live gaps, limit",
        s,
        |st| {
            let open = st.capability_gaps[gap].clone();
            for i in 1..=MAX_LIVE_CAPABILITY_GAPS {
                let mut extra = open.clone();
                extra.gap_id = CapabilityGapId::new(format!("gap_extra_{i}"));
                st.capability_gaps.insert(extra.gap_id.clone(), extra);
            }
        },
    ));
    cases
}

fn evidence_cases(f: &Fx) -> Vec<Case> {
    let s = &f.state;
    let (observation, contract) = (&f.observation, &f.contract);
    vec![
        case(
            "requirement cites an unknown observation",
            "requirement_evidence",
            "cites unknown",
            s,
            |st| {
                st.observations.remove(observation);
            },
        ),
        case(
            "record stored under another key",
            "map_key_identity",
            "contract stored under",
            s,
            |st| st.contracts.get_mut(contract).unwrap().contract_id = ContractId::new("ctr_other"),
        ),
    ]
}

fn record_transition_cases(f: &Fx) -> Vec<Case> {
    let s = &f.state;
    // A real engine step that advances the coordinator's work to a new epoch.
    let moved = commit_ops(
        s,
        &f.observation,
        vec![OrgOp::ReassignResponsibility {
            responsibility_id: f.coordinator_work.clone(),
            from: f.coordinator.clone(),
            to: RoleRef::existing(f.program.clone()),
        }],
    );
    let work = f.coordinator_work.clone();
    assert!(moved.responsibilities[&work].epoch > s.responsibilities[&work].epoch);
    let asg = active_assignment_of(s, &f.coordinator_work);
    let (info, observation) = (&f.info, &f.observation);
    vec![
        Case {
            prev: Some(s.clone()),
            ..case(
                "revision does not advance",
                "revision_advances",
                "follows",
                s,
                |_| {},
            )
        },
        tcase(
            "role disappears",
            "history_is_append_only",
            "role ",
            s,
            |st| {
                st.roles.remove(info);
            },
        ),
        tcase(
            "observation disappears",
            "history_is_append_only",
            "observation ",
            s,
            |st| {
                st.observations.remove(observation);
            },
        ),
        tcase(
            "observation edited",
            "observations_immutable",
            "changed",
            s,
            |st| st.observations.get_mut(observation).unwrap().summary = "edited".into(),
        ),
        tcase(
            "retired role revived",
            "lifecycle_reachable",
            "role ",
            s,
            |st| st.roles.get_mut(info).unwrap().state = RoleState::Active,
        ),
        // L377
        tcase(
            "responsibility epoch goes back",
            "epoch_monotonic",
            " epoch ",
            &moved,
            move |st| st.responsibilities.get_mut(&work).unwrap().epoch -= 1,
        ),
        tcase(
            "assignment rebound",
            "epoch_monotonic",
            "was rebound",
            s,
            |st| st.assignments.get_mut(&asg).unwrap().epoch += 1,
        ),
        tcase(
            "constitution budget raised",
            "constitution_unchanged",
            "budget_cents",
            s,
            |st| st.constitution.budget_cents += 1,
        ),
        tcase(
            "constitution role limit raised",
            "constitution_unchanged",
            "max_active_roles",
            s,
            |st| st.constitution.max_active_roles += 1,
        ),
        tcase(
            "constitution contract limit raised",
            "constitution_unchanged",
            "max_active_contracts",
            s,
            |st| st.constitution.max_active_contracts += 1,
        ),
        tcase(
            "constitution scope added",
            "constitution_unchanged",
            "scopes added: payments.send",
            s,
            |st| {
                st.constitution.scopes.insert("payments.send".into());
            },
        ),
        tcase(
            "constitution scope removed",
            "constitution_unchanged",
            "scopes removed: ",
            s,
            |st| {
                let first = st.constitution.scopes.iter().next().unwrap().clone();
                st.constitution.scopes.remove(&first);
            },
        ),
    ]
}

fn gap_transition_cases(f: &Fx, g: &Stages) -> Vec<Case> {
    use CapabilityGapStatus as S;
    let gap = f.gap.clone();
    let status =
        move |st: &mut OrgState, to: S| st.capability_gaps.get_mut(&gap).unwrap().status = to;
    let gap = f.gap.clone();
    let mut cases = vec![
        // L396
        tcase(
            "capability gap disappears",
            "history_is_append_only",
            "capability gap",
            &g.acquired,
            |st| {
                st.capability_gaps.remove(&gap);
            },
        ),
        // L432: regression in rank, with the acquisition kept.
        tcase(
            "acquired gap back to acquiring",
            "lifecycle_reachable",
            "capability gap",
            &g.acquired,
            |st| {
                status(
                    st,
                    S::Acquiring {
                        acquisition_id: ACQ.into(),
                    },
                )
            },
        ),
        tcase(
            "installed gap back to acquired",
            "lifecycle_reachable",
            "capability gap",
            &g.installed,
            |st| {
                status(
                    st,
                    acquired("1.0.0", Some("registry:venue-availability@1.0.0")),
                )
            },
        ),
        // L432: a terminal outcome changes at the same rank.
        tcase(
            "refusal reason rewritten",
            "lifecycle_reachable",
            "capability gap",
            &g.refused,
            |st| {
                status(
                    st,
                    S::Refused {
                        reason: "a different reason".into(),
                        acquisition_id: None,
                    },
                )
            },
        ),
        tcase(
            "installed gap becomes refused",
            "lifecycle_reachable",
            "capability gap",
            &g.installed,
            |st| {
                status(
                    st,
                    S::Refused {
                        reason: "changed its mind".into(),
                        acquisition_id: Some(ACQ.into()),
                    },
                )
            },
        ),
        // L432: the verified capability or confirmed publication changes.
        tcase(
            "verified version changes",
            "lifecycle_reachable",
            "capability gap",
            &g.acquired,
            |st| status(st, acquired("2.0.0", None)),
        ),
        tcase(
            "confirmed publication dropped",
            "lifecycle_reachable",
            "capability gap",
            &g.published,
            |st| status(st, acquired("1.0.0", None)),
        ),
        tcase(
            "confirmed publication replaced",
            "lifecycle_reachable",
            "capability gap",
            &g.published,
            |st| status(st, acquired("1.0.0", Some("registry:other@1.0.0"))),
        ),
        tcase(
            "installed version differs from verified",
            "lifecycle_reachable",
            "capability gap",
            &g.published,
            |st| {
                status(
                    st,
                    S::Installed {
                        acquisition_id: ACQ.into(),
                        capability: cap("2.0.0"),
                        installation_id: g.installation.clone(),
                    },
                )
            },
        ),
        // L454: the acquisition is rebound or dropped.
        tcase(
            "acquisition rebound",
            "gap_binding_stable",
            "changed its binding",
            &g.acquiring,
            |st| {
                status(
                    st,
                    S::Acquiring {
                        acquisition_id: "acq-2".into(),
                    },
                )
            },
        ),
        tcase(
            "refusal drops the acquisition",
            "gap_binding_stable",
            "changed its binding",
            &g.acquired,
            |st| {
                status(
                    st,
                    S::Refused {
                        reason: "gave up".into(),
                        acquisition_id: None,
                    },
                )
            },
        ),
    ];
    // L454: every frozen binding field.
    let fields: Vec<(&str, GapEdit)> = vec![
        ("gap_id", |g| g.gap_id = CapabilityGapId::new("gap_other")),
        ("mission_id", |g| {
            g.mission_id = MissionId::new("mission_other")
        }),
        ("requirement_id", |g| {
            g.requirement_id = RequirementId::new("req_other")
        }),
        ("requirement_digest", |g| g.requirement_digest.push('x')),
        ("triggering_evidence", |g| g.triggering_evidence.clear()),
        ("role_id", |g| g.role_id = RoleId::new("role_other")),
        ("responsibility_id", |g| {
            g.responsibility_id = ResponsibilityId::new("rsp_other")
        }),
        ("capability_id", |g| {
            g.capability_id = CapabilityId::new("other-capability")
        }),
        ("purpose", |g| g.purpose.push('!')),
        ("observed_sources", |g| {
            g.observed_sources.push(PORTAL.into())
        }),
        ("opened_in_revision", |g| g.opened_in_revision -= 1),
    ];
    for (field, apply) in fields {
        let gap = f.gap.clone();
        cases.push(tcase(
            &format!("gap {field} changes"),
            "gap_binding_stable",
            "changed its binding",
            &g.acquiring,
            move |st| apply(st.capability_gaps.get_mut(&gap).unwrap()),
        ));
    }
    cases
}

// --- Tests -------------------------------------------------------------------

#[test]
fn engine_states_pass_both_audits() {
    let f = fx();
    let g = stages(&f);
    let chain = [
        &f.state,
        &g.acquiring,
        &g.acquired,
        &g.published,
        &g.installed,
    ];
    for st in chain.iter().copied().chain([&g.refused]) {
        assert_eq!(audit(st), vec![], "r{}", st.revision);
    }
    for pair in chain.windows(2) {
        assert_eq!(audit_transition(pair[0], pair[1]), vec![]);
    }
    // Several commits apart, and the other terminal branch.
    assert_eq!(audit_transition(&f.state, &g.installed), vec![]);
    assert_eq!(audit_transition(&f.state, &g.refused), vec![]);
    assert_eq!(audit_transition(&f.state, &later(&f.state)), vec![]);
    // The live gap limit is inclusive: exactly the limit is still valid.
    let mut full = f.state.clone();
    let open = full.capability_gaps[&f.gap].clone();
    for i in 1..MAX_LIVE_CAPABILITY_GAPS {
        let mut extra = open.clone();
        extra.gap_id = CapabilityGapId::new(format!("gap_extra_{i}"));
        full.capability_gaps.insert(extra.gap_id.clone(), extra);
    }
    assert_eq!(full.capability_gaps.len(), MAX_LIVE_CAPABILITY_GAPS);
    assert_eq!(audit(&full), vec![]);
}

#[test]
fn role_invariants_fire() {
    run(role_cases(&fx()));
}

#[test]
fn assignment_and_responsibility_invariants_fire() {
    run(work_cases(&fx()));
}

#[test]
fn capability_gap_invariants_fire() {
    let f = fx();
    let g = stages(&f);
    run(gap_cases(&f, &g));
}

#[test]
fn evidence_and_identity_invariants_fire() {
    run(evidence_cases(&fx()));
}

#[test]
fn record_transition_invariants_fire() {
    run(record_transition_cases(&fx()));
}

#[test]
fn capability_gap_transition_invariants_fire() {
    let f = fx();
    let g = stages(&f);
    run(gap_transition_cases(&f, &g));
}

/// Every name in `INVARIANTS` and `TRANSITION_INVARIANTS` has a negative case
/// above, so a new invariant cannot be added without one.
#[test]
fn every_invariant_has_a_negative_case() {
    let f = fx();
    let g = stages(&f);
    let mut state = BTreeSet::new();
    let mut transition = BTreeSet::new();
    let all = [
        role_cases(&f),
        work_cases(&f),
        gap_cases(&f, &g),
        evidence_cases(&f),
        record_transition_cases(&f),
        gap_transition_cases(&f, &g),
    ];
    for c in all.into_iter().flatten() {
        if c.prev.is_some() {
            transition.insert(c.invariant);
        } else {
            state.insert(c.invariant);
        }
    }
    let missing: Vec<_> = INVARIANTS.iter().filter(|i| !state.contains(*i)).collect();
    assert!(
        missing.is_empty(),
        "audit invariants without a case: {missing:?}"
    );
    let missing: Vec<_> = TRANSITION_INVARIANTS
        .iter()
        .filter(|i| !transition.contains(*i))
        .collect();
    assert!(
        missing.is_empty(),
        "transition invariants without a case: {missing:?}"
    );
}
