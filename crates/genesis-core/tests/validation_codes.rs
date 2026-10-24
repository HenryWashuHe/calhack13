//! Each validation check in the engine is pinned by an assertion on its
//! `ValidationCode` (and, for proposals, the offending op index).
//!
//! Many of these checks sit in reference resolvers that return `None` after
//! reporting. Without the report, the bad operation is silently dropped and the
//! rest of the proposal commits. The planner is a model and sends exactly these
//! references (unknown ids, stale keys, retired roles), so every case asserts the
//! specific code rather than only that the proposal failed. Where it is cheap, a
//! control proposal with the reference repaired must validate.

use chrono::Utc;
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::lifecycle::*;
use genesis_core::model::*;
use std::collections::BTreeMap;

const PORTAL: &str = "https://portal.example/org-a/venues/index.html";
const SOURCE: &str = "https://portal.example/org-a/venues/cards.html";

struct Demo {
    state: OrgState,
    mission: MissionId,
    coordinator: RoleId,
    program: RoleId,
    info: RoleId,
    observation: ObservationId,
    requirement: RequirementId,
    coordinator_work: ResponsibilityId,
    program_work: ResponsibilityId,
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

/// Three active roles (coordinator and program own one responsibility each,
/// info owns none), one observation with an https evidence reference, and one
/// unmet requirement derived from it.
fn demo() -> Demo {
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
    Demo {
        coordinator: role_named(&state, "Coordinator"),
        program: role_named(&state, "Program"),
        info: role_named(&state, "Info"),
        coordinator_work: work_named(&state, "Coordinate the mission"),
        program_work: work_named(&state, "Maintain the schedule"),
        state,
        mission,
        observation,
        requirement,
    }
}

fn proposal(state: &OrgState, evidence: &ObservationId, ops: Vec<OrgOp>) -> OrgChangeProposal {
    OrgChangeProposal {
        expected_org_revision: state.revision,
        trigger_evidence_ids: vec![evidence.clone()],
        rationale: "validation code test".into(),
        budget_limit_cents: None,
        operations: ops,
    }
}

fn check(d: &Demo, state: &OrgState, ops: Vec<OrgOp>) -> Result<Outcome, Vec<ValidationError>> {
    let p = proposal(state, &d.observation, ops);
    let alloc = KeyAllocation::allocate(&p);
    validate(state, &p, &alloc, &StaticCatalog::default(), Utc::now())
}

/// Commit operations that create no roles.
fn commit_ops(d: &Demo, state: &OrgState, ops: Vec<OrgOp>, catalog: &StaticCatalog) -> OrgState {
    let p = proposal(state, &d.observation, ops);
    let alloc = KeyAllocation::allocate(&p);
    commit(state, &p, &alloc, &BTreeMap::new(), catalog, Utc::now())
        .unwrap_or_else(|e| panic!("setup commit failed: {e:?}"))
        .state
}

fn has(errs: &[ValidationError], code: ValidationCode, op: Option<usize>) -> bool {
    errs.iter().any(|e| e.code == code && e.op_index == op)
}

fn count(errs: &[ValidationError], code: ValidationCode, op: Option<usize>) -> usize {
    errs.iter()
        .filter(|e| e.code == code && e.op_index == op)
        .count()
}

/// One check: `ops` against `state` must report `code` at `op`. `control`, if
/// given, is the same intent with the bad reference repaired and must validate.
struct Case {
    check: &'static str,
    state: OrgState,
    ops: Vec<OrgOp>,
    alloc: Option<KeyAllocation>,
    code: ValidationCode,
    op: Option<usize>,
    control: Option<Vec<OrgOp>>,
}

impl Case {
    fn new(
        check: &'static str,
        state: &OrgState,
        ops: Vec<OrgOp>,
        code: ValidationCode,
        op: Option<usize>,
    ) -> Self {
        Self {
            check,
            state: state.clone(),
            ops,
            alloc: None,
            code,
            op,
            control: None,
        }
    }

    fn control(mut self, ops: Vec<OrgOp>) -> Self {
        self.control = Some(ops);
        self
    }

    fn alloc(mut self, alloc: KeyAllocation) -> Self {
        self.alloc = Some(alloc);
        self
    }
}

fn run_cases(d: &Demo, cases: Vec<Case>) {
    for c in cases {
        let p = proposal(&c.state, &d.observation, c.ops);
        let alloc = c.alloc.unwrap_or_else(|| KeyAllocation::allocate(&p));
        let result = validate(&c.state, &p, &alloc, &StaticCatalog::default(), Utc::now());
        let errs = match result {
            Ok(_) => panic!("{}: proposal was accepted", c.check),
            Err(errs) => errs,
        };
        assert!(
            has(&errs, c.code, c.op),
            "{}: expected {:?} at op {:?}, got {errs:?}",
            c.check,
            c.code,
            c.op
        );
        if let Some(ops) = c.control {
            if let Err(errs) = check(d, &c.state, ops) {
                panic!("{}: repaired control was rejected: {errs:?}", c.check);
            }
        }
    }
}

// --- Operation builders ------------------------------------------------------

fn from_requirement(key: &str, requirement: &RequirementId, owner: RoleRef) -> OrgOp {
    OrgOp::AssignResponsibility {
        target: ResponsibilityTarget::FromRequirement {
            key: key.into(),
            requirement_id: requirement.clone(),
            purpose: "Relocate the event".into(),
            trigger: "venue changes".into(),
            termination: "venue reserved".into(),
        },
        owner,
    }
}

fn assign_existing(responsibility: &ResponsibilityId, owner: &RoleId) -> OrgOp {
    OrgOp::AssignResponsibility {
        target: ResponsibilityTarget::Existing {
            responsibility_id: responsibility.clone(),
        },
        owner: RoleRef::existing(owner.clone()),
    }
}

fn delegate(responsibility: ResponsibilityRef, executor: ExecutorRef) -> OrgOp {
    OrgOp::DelegateTask {
        responsibility,
        executor,
        task: "do the work".into(),
    }
}

fn existing_work(id: &ResponsibilityId) -> ResponsibilityRef {
    ResponsibilityRef::Existing {
        responsibility_id: id.clone(),
    }
}

fn to_role(id: &RoleId) -> ExecutorRef {
    ExecutorRef::Role {
        role: RoleRef::existing(id.clone()),
    }
}

fn to_contract(contract: ContractRef) -> ExecutorRef {
    ExecutorRef::Contractor { contract }
}

fn retire(role: &RoleId, handoffs: &[(&ResponsibilityId, &RoleId)]) -> OrgOp {
    OrgOp::RetireRole {
        role_id: role.clone(),
        handoffs: handoffs
            .iter()
            .map(|(r, to)| Handoff {
                responsibility_id: (*r).clone(),
                to: RoleRef::existing((*to).clone()),
            })
            .collect(),
    }
}

fn reassign(responsibility: &ResponsibilityId, from: &RoleId, to: &RoleId) -> OrgOp {
    OrgOp::ReassignResponsibility {
        responsibility_id: responsibility.clone(),
        from: from.clone(),
        to: RoleRef::existing(to.clone()),
    }
}

fn recruit(key: &str, scope: &str) -> OrgOp {
    OrgOp::RecruitSpecialist {
        key: key.into(),
        specialist_address: "agent1qlogistics".into(),
        protocol: "genesis-contract/1".into(),
        scope: scope.into(),
        deadline: None,
        supervising_role: None,
    }
}

fn request(
    requirement: &RequirementId,
    role: &RoleId,
    responsibility: &ResponsibilityId,
    purpose: &str,
) -> OrgOp {
    OrgOp::RequestCapability {
        key: "reader".into(),
        requirement_id: requirement.clone(),
        role: RoleRef::existing(role.clone()),
        responsibility: existing_work(responsibility),
        capability_id: CapabilityId::new("venue-availability"),
        purpose: purpose.into(),
        observed_sources: vec![SOURCE.into()],
    }
}

const PURPOSE: &str = "Read venue capacity and availability from the portal";

/// A new role that owns the requirement's responsibility and recruits a
/// contractor for it: the shape the planner sends for a venue closure.
fn relocation(d: &Demo) -> Vec<OrgOp> {
    vec![
        OrgOp::CreateRole {
            key: "relocation".into(),
            name: "Relocation lead".into(),
            charter: "Secure a replacement venue".into(),
            authority: AuthorityGrant::new(["ledger.reserve_venue", "contract.logistics"], 150_000),
            reports_to: Some(RoleRef::existing(d.coordinator.clone())),
            provider: Provider::Elastic,
            justification: RoleJustification::IndependentOwnership,
        },
        from_requirement("relocate", &d.requirement, RoleRef::new_key("relocation")),
        OrgOp::RecruitSpecialist {
            key: "logistics".into(),
            specialist_address: "agent1qlogistics".into(),
            protocol: "genesis-contract/1".into(),
            scope: "transport plan".into(),
            deadline: None,
            supervising_role: Some(RoleRef::new_key("relocation")),
        },
    ]
}

/// The requirement assigned to program: program owns a responsibility linked
/// to it, and the requirement is ASSIGNED with program as its owner.
fn assigned_to_program(d: &Demo) -> (OrgState, ResponsibilityId) {
    let ops = vec![from_requirement(
        "relocate",
        &d.requirement,
        RoleRef::existing(d.program.clone()),
    )];
    let state = commit_ops(d, &d.state, ops, &StaticCatalog::default());
    let rid = state
        .responsibilities
        .values()
        .find(|r| r.requirement_id.as_ref() == Some(&d.requirement))
        .unwrap()
        .responsibility_id
        .clone();
    (state, rid)
}

/// Program retired after handing its schedule work to the coordinator.
fn program_retired(d: &Demo, state: &OrgState) -> OrgState {
    let ops = vec![retire(&d.program, &[(&d.program_work, &d.coordinator)])];
    let out = commit_ops(d, state, ops, &StaticCatalog::default());
    assert_eq!(out.roles[&d.program].state, RoleState::Retired);
    out
}

fn program_work_completed(d: &Demo) -> OrgState {
    complete_responsibility(&d.state, &d.program_work)
        .unwrap()
        .state
}

// --- Tests ------------------------------------------------------------------

#[test]
fn proposal_shape_and_stale_key_allocation_are_reported() {
    let d = demo();
    let s = &d.state;
    use ValidationCode as V;

    // L212: an empty proposal is not a no-op success.
    let p = proposal(s, &d.observation, vec![]);
    let errs = validate(
        s,
        &p,
        &KeyAllocation::default(),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, V::EmptyChange);

    // An allocation that does not cover the proposal's keys (a stale or
    // mismatched persisted allocation) must not drop the keyed operation.
    let stale = KeyAllocation::default;
    let cases = vec![
        // L279
        Case::new(
            "blank key",
            s,
            vec![from_requirement(
                "  ",
                &d.requirement,
                RoleRef::existing(d.program.clone()),
            )],
            V::InvalidInput,
            Some(0),
        )
        .control(vec![from_requirement(
            "relocate",
            &d.requirement,
            RoleRef::existing(d.program.clone()),
        )]),
        // L490
        Case::new(
            "CreateRole without allocated id",
            s,
            vec![relocation(&d).remove(0)],
            V::UnknownKey,
            Some(0),
        )
        .alloc(stale()),
        // L594
        Case::new(
            "RecruitSpecialist without allocated id",
            s,
            vec![recruit("logistics", "transport plan")],
            V::UnknownKey,
            Some(0),
        )
        .alloc(stale())
        .control(vec![recruit("logistics", "transport plan")]),
        // L674
        Case::new(
            "FromRequirement without allocated id",
            s,
            vec![from_requirement(
                "relocate",
                &d.requirement,
                RoleRef::existing(d.program.clone()),
            )],
            V::UnknownKey,
            Some(0),
        )
        .alloc(stale()),
        // L970
        Case::new(
            "RequestCapability without allocated id",
            s,
            vec![request(
                &d.requirement,
                &d.program,
                &d.program_work,
                PURPOSE,
            )],
            V::UnknownKey,
            Some(0),
        )
        .alloc(stale()),
    ];
    run_cases(&d, cases);
}

#[test]
fn unresolvable_references_are_reported_not_dropped() {
    let d = demo();
    let s = &d.state;
    use ValidationCode as V;
    let ghost_role = RoleId::new("role_ghost");
    let ghost_work = ResponsibilityId::new("rsp_ghost");
    let retired = program_retired(&d, s);
    let completed = program_work_completed(&d);

    // A contract that existed and was declined.
    let recruited = commit_ops(
        &d,
        s,
        vec![recruit("logistics", "transport plan")],
        &StaticCatalog::default(),
    );
    let contract = recruited.contracts.keys().next().unwrap().clone();
    let declined = update_contract(&recruited, &contract, ContractState::Declined)
        .unwrap()
        .state;
    let use_contract = |id: &ContractId| {
        vec![delegate(
            existing_work(&d.program_work),
            to_contract(ContractRef::Existing {
                contract_id: id.clone(),
            }),
        )]
    };
    let to_coordinator = vec![delegate(
        existing_work(&d.program_work),
        to_role(&d.coordinator),
    )];

    let cases = vec![
        // L301: owner resolution in working_role.
        Case::new(
            "unknown existing role",
            s,
            vec![from_requirement(
                "relocate",
                &d.requirement,
                RoleRef::existing(ghost_role.clone()),
            )],
            V::UnknownRole,
            Some(0),
        )
        .control(vec![from_requirement(
            "relocate",
            &d.requirement,
            RoleRef::existing(d.coordinator.clone()),
        )]),
        // L309
        Case::new(
            "retired role as owner",
            &retired,
            vec![from_requirement(
                "relocate",
                &d.requirement,
                RoleRef::existing(d.program.clone()),
            )],
            V::RoleNotActive,
            Some(0),
        )
        .control(vec![from_requirement(
            "relocate",
            &d.requirement,
            RoleRef::existing(d.coordinator.clone()),
        )]),
        // L349
        Case::new(
            "delegate unknown responsibility",
            s,
            vec![delegate(
                existing_work(&ghost_work),
                to_role(&d.coordinator),
            )],
            V::UnknownResponsibility,
            Some(0),
        )
        .control(to_coordinator.clone()),
        // L357
        Case::new(
            "delegate completed responsibility",
            &completed,
            to_coordinator.clone(),
            V::ResponsibilityNotOpen,
            Some(0),
        ),
        // L375
        Case::new(
            "delegate to uncreated contract key",
            s,
            vec![delegate(
                existing_work(&d.program_work),
                to_contract(ContractRef::New {
                    key: "ghost".into(),
                }),
            )],
            V::UnknownKey,
            Some(0),
        ),
        // L387
        Case::new(
            "delegate to unknown contract",
            s,
            use_contract(&ContractId::new("ctr_ghost")),
            V::UnknownContract,
            Some(0),
        ),
        // L395
        Case::new(
            "delegate to declined contract",
            &declined,
            use_contract(&contract),
            V::InvalidTransition,
            Some(0),
        ),
        // L638
        Case::new(
            "assign unknown existing responsibility",
            s,
            vec![assign_existing(&ghost_work, &d.program)],
            V::UnknownResponsibility,
            Some(0),
        ),
        // L646
        Case::new(
            "assign completed responsibility",
            &completed,
            vec![assign_existing(&d.program_work, &d.coordinator)],
            V::ResponsibilityNotOpen,
            Some(0),
        ),
        // L1226
        Case::new(
            "retire unknown role",
            s,
            vec![retire(&ghost_role, &[])],
            V::UnknownRole,
            Some(0),
        )
        .control(vec![retire(&d.info, &[])]),
        // L1248
        Case::new(
            "hand off unknown responsibility",
            s,
            vec![retire(
                &d.program,
                &[
                    (&ghost_work, &d.coordinator),
                    (&d.program_work, &d.coordinator),
                ],
            )],
            V::UnknownResponsibility,
            Some(0),
        )
        .control(vec![retire(
            &d.program,
            &[(&d.program_work, &d.coordinator)],
        )]),
    ];
    run_cases(&d, cases);

    // L395 control: the same contract before it was declined is usable.
    assert!(check(&d, &recruited, use_contract(&contract)).is_ok());
}

#[test]
fn role_and_specialist_inputs_are_checked() {
    let d = demo();
    let s = &d.state;
    use ValidationCode as V;
    let with = |f: &dyn Fn(&mut OrgOp)| {
        let mut ops = relocation(&d);
        for op in &mut ops {
            f(op);
        }
        ops
    };
    let cases = vec![
        // L501
        Case::new(
            "blank role name",
            s,
            with(&|op| {
                if let OrgOp::CreateRole { name, .. } = op {
                    *name = " ".into();
                }
            }),
            V::InvalidInput,
            Some(0),
        )
        .control(relocation(&d)),
        // L501, the other half of the condition.
        Case::new(
            "blank role charter",
            s,
            with(&|op| {
                if let OrgOp::CreateRole { charter, .. } = op {
                    *charter = String::new();
                }
            }),
            V::InvalidInput,
            Some(0),
        ),
        // L508
        Case::new(
            "Apify role",
            s,
            with(&|op| {
                if let OrgOp::CreateRole { provider, .. } = op {
                    *provider = Provider::Apify;
                }
            }),
            V::InvalidInput,
            Some(0),
        ),
        // L605
        Case::new(
            "blank specialist scope",
            s,
            with(&|op| {
                if let OrgOp::RecruitSpecialist { scope, .. } = op {
                    *scope = "  ".into();
                }
            }),
            V::InvalidInput,
            Some(2),
        ),
        // L605, the other half of the condition.
        Case::new(
            "blank specialist address",
            s,
            with(&|op| {
                if let OrgOp::RecruitSpecialist {
                    specialist_address, ..
                } = op
                {
                    *specialist_address = String::new();
                }
            }),
            V::InvalidInput,
            Some(2),
        ),
    ];
    run_cases(&d, cases);
}

#[test]
fn accountability_and_retirement_checks() {
    let d = demo();
    let s = &d.state;
    use ValidationCode as V;
    let (assigned, _) = assigned_to_program(&d);
    let retired = program_retired(&d, s);
    let cases = vec![
        // L697: the requirement already has a responsibility.
        Case::new(
            "assign an already assigned requirement",
            &assigned,
            vec![from_requirement(
                "again",
                &d.requirement,
                RoleRef::existing(d.coordinator.clone()),
            )],
            V::RequirementNotUnmet,
            Some(0),
        ),
        // L817: a responsibility with no accountable role cannot be delegated.
        // Op 0 fails, so its new responsibility stays with the controller.
        Case::new(
            "delegate controller-owned responsibility",
            s,
            vec![
                from_requirement(
                    "relocate",
                    &d.requirement,
                    RoleRef::existing(RoleId::new("role_ghost")),
                ),
                delegate(
                    ResponsibilityRef::New {
                        key: "relocate".into(),
                    },
                    to_role(&d.coordinator),
                ),
            ],
            V::AccountabilityConflict,
            Some(1),
        )
        .control(vec![
            from_requirement(
                "relocate",
                &d.requirement,
                RoleRef::existing(d.program.clone()),
            ),
            delegate(
                ResponsibilityRef::New {
                    key: "relocate".into(),
                },
                to_role(&d.coordinator),
            ),
        ]),
        // L885
        Case::new(
            "reassign from a role that is not the owner",
            s,
            vec![reassign(&d.program_work, &d.coordinator, &d.info)],
            V::AccountabilityConflict,
            Some(0),
        )
        .control(vec![reassign(&d.program_work, &d.program, &d.info)]),
        // L896
        Case::new(
            "reassign to the current owner",
            s,
            vec![reassign(&d.program_work, &d.program, &d.program)],
            V::InvalidInput,
            Some(0),
        ),
        // L1238
        Case::new(
            "retire a retired role",
            &retired,
            vec![retire(&d.program, &[])],
            V::RoleNotActive,
            Some(0),
        ),
        // L1256
        Case::new(
            "hand off another role's responsibility",
            s,
            vec![retire(
                &d.program,
                &[
                    (&d.coordinator_work, &d.info),
                    (&d.program_work, &d.coordinator),
                ],
            )],
            V::AccountabilityConflict,
            Some(0),
        ),
        // L1272: the self-handoff is skipped; without the report the second
        // handoff alone would let the retirement through.
        Case::new(
            "hand off to the retiring role",
            s,
            vec![retire(
                &d.program,
                &[
                    (&d.program_work, &d.program),
                    (&d.program_work, &d.coordinator),
                ],
            )],
            V::InvalidInput,
            Some(0),
        ),
    ];
    run_cases(&d, cases);
}

#[test]
fn capability_requests_check_requirement_ownership_and_purpose() {
    let d = demo();
    use ValidationCode as V;
    let (assigned, linked) = assigned_to_program(&d);
    let mut satisfied = assigned.clone();
    satisfied
        .requirements
        .get_mut(&d.requirement)
        .unwrap()
        .status = RequirementStatus::Satisfied;

    let cases = vec![
        // L984
        Case::new(
            "unknown requirement",
            &assigned,
            vec![request(
                &RequirementId::new("req_ghost"),
                &d.program,
                &linked,
                PURPOSE,
            )],
            V::UnknownRequirement,
            Some(0),
        )
        .control(vec![request(&d.requirement, &d.program, &linked, PURPOSE)]),
        // L992
        Case::new(
            "satisfied requirement",
            &satisfied,
            vec![request(&d.requirement, &d.program, &linked, PURPOSE)],
            V::RequirementNotUnmet,
            Some(0),
        ),
        // L1016: program's own (unlinked) work, for a requirement still owned
        // by the mission controller.
        Case::new(
            "requirement not owned by the role",
            &d.state,
            vec![request(
                &d.requirement,
                &d.program,
                &d.program_work,
                PURPOSE,
            )],
            V::AccountabilityConflict,
            Some(0),
        ),
        // L1032: program owns the requirement, but names the coordinator's work.
        Case::new(
            "responsibility not owned by the role",
            &assigned,
            vec![request(
                &d.requirement,
                &d.program,
                &d.coordinator_work,
                PURPOSE,
            )],
            V::AccountabilityConflict,
            Some(0),
        ),
        // L1046
        Case::new(
            "blank purpose",
            &assigned,
            vec![request(&d.requirement, &d.program, &linked, "   ")],
            V::InvalidInput,
            Some(0),
        ),
        Case::new(
            "overlong purpose",
            &assigned,
            vec![request(
                &d.requirement,
                &d.program,
                &linked,
                &"p".repeat(401),
            )],
            V::InvalidInput,
            Some(0),
        ),
    ];
    run_cases(&d, cases);

    // The L1016 and L1032 cases each raise exactly one ownership conflict, so
    // the code above comes from the check under test, not the other one.
    let errs = check(
        &d,
        &d.state,
        vec![request(
            &d.requirement,
            &d.program,
            &d.program_work,
            PURPOSE,
        )],
    )
    .unwrap_err();
    assert_eq!(count(&errs, V::AccountabilityConflict, Some(0)), 1);
    let errs = check(
        &d,
        &assigned,
        vec![request(
            &d.requirement,
            &d.program,
            &d.coordinator_work,
            PURPOSE,
        )],
    )
    .unwrap_err();
    assert_eq!(count(&errs, V::AccountabilityConflict, Some(0)), 1);

    // L1023: program owns work for a second mission's requirement and cites
    // it for the first mission's requirement. Unlinked work is also reported
    // (one InvalidInput); the cross-mission check adds a second.
    let now = Utc::now();
    let (out, second) = bootstrap_mission(
        &assigned,
        &MissionBootstrap {
            goal: "A second event".into(),
            constraints: vec![],
            deadline: None,
            success_checks: vec![],
            roles: vec![],
            responsibilities: vec![],
        },
        now,
    )
    .unwrap();
    let (out, obs2) =
        record_observation(&out.state, &second, "portal", "x", Some(PORTAL.into()), now).unwrap();
    let (out, req2) = record_requirement(&out.state, &second, "y", &[obs2], "z").unwrap();
    let two = commit_ops(
        &d,
        &out.state,
        vec![from_requirement(
            "second",
            &req2,
            RoleRef::existing(d.program.clone()),
        )],
        &StaticCatalog::default(),
    );
    let other_mission_work = two
        .responsibilities
        .values()
        .find(|r| r.requirement_id.as_ref() == Some(&req2))
        .unwrap()
        .responsibility_id
        .clone();
    assert_eq!(two.responsibilities[&other_mission_work].mission_id, second);
    let errs = check(
        &d,
        &two,
        vec![request(
            &d.requirement,
            &d.program,
            &other_mission_work,
            PURPOSE,
        )],
    )
    .unwrap_err();
    assert_eq!(
        count(&errs, V::InvalidInput, Some(0)),
        2,
        "unlinked and cross-mission: {errs:?}"
    );
    // Same-mission unlinked work: only the link error.
    let errs = check(
        &d,
        &assigned,
        vec![request(
            &d.requirement,
            &d.program,
            &d.program_work,
            PURPOSE,
        )],
    )
    .unwrap_err();
    assert_eq!(count(&errs, V::InvalidInput, Some(0)), 1);
    let _ = d.mission;
}

#[test]
fn post_checks_report_limits_and_orphaned_work() {
    let d = demo();
    use ValidationCode as V;
    let bindings: BTreeMap<String, AgentBinding> = [(
        "relocation".to_string(),
        AgentBinding {
            provider: Provider::Elastic,
            provider_agent_id: "elastic-1".into(),
            config_digest: "digest".into(),
            conversation_ref: None,
        },
    )]
    .into();
    let run = |state: &OrgState, limit: Option<u64>| {
        let mut p = proposal(state, &d.observation, relocation(&d));
        p.budget_limit_cents = limit;
        let alloc = KeyAllocation::allocate(&p);
        commit(
            state,
            &p,
            &alloc,
            &bindings,
            &StaticCatalog::default(),
            Utc::now(),
        )
    };
    assert!(run(&d.state, Some(150_000)).is_ok());

    // L1396: the change allocates 150_000 against its own 100_000 limit.
    let errs = run(&d.state, Some(100_000)).unwrap_err();
    assert!(has(&errs, V::ChangeBudgetExceeded, None), "{errs:?}");

    // L1406: a fourth live role against a limit of three.
    let mut tight = d.state.clone();
    tight.constitution.max_active_roles = 3;
    let errs = run(&tight, None).unwrap_err();
    assert!(has(&errs, V::RoleLimitExceeded, None), "{errs:?}");

    // L1438: a role created and retired in the same commit can still be named
    // by its key, and take ownership after its retirement. Only the global
    // orphan check catches the open responsibility owned by a retired role.
    let ops = |retire_id: RoleId| {
        vec![
            OrgOp::CreateRole {
                key: "relocation".into(),
                name: "Relocation lead".into(),
                charter: "Secure a replacement venue".into(),
                authority: AuthorityGrant::default(),
                reports_to: None,
                provider: Provider::Elastic,
                justification: RoleJustification::IndependentOwnership,
            },
            retire(&retire_id, &[]),
            from_requirement("relocate", &d.requirement, RoleRef::new_key("relocation")),
        ]
    };
    let mut p = proposal(&d.state, &d.observation, ops(RoleId::new("placeholder")));
    let alloc = KeyAllocation::allocate(&p);
    p.operations = ops(alloc.roles["relocation"].clone());
    assert_eq!(KeyAllocation::allocate(&p).roles.len(), alloc.roles.len());
    let errs = commit(
        &d.state,
        &p,
        &alloc,
        &bindings,
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap_err();
    assert!(has(&errs, V::RetirementDropsObligations, None), "{errs:?}");
}

#[test]
fn evidence_entry_points_check_mission_and_inputs() {
    let d = demo();
    use ValidationCode as V;
    let now = Utc::now();
    let ghost = MissionId::new("mission_ghost");

    // L1706, L1713
    let obs = |m: &MissionId, summary: &str| {
        record_observation(&d.state, m, "portal", summary, None, now).map(|_| ())
    };
    assert_eq!(obs(&ghost, "closed").unwrap_err().code, V::UnknownMission);
    assert_eq!(obs(&d.mission, "  ").unwrap_err().code, V::InvalidInput);
    assert!(obs(&d.mission, "closed").is_ok());

    // L1751, L1758
    let req = |m: &MissionId, evidence: &[ObservationId]| {
        record_requirement(&d.state, m, "relocate", evidence, "done").map(|_| ())
    };
    let evidence = std::slice::from_ref(&d.observation);
    assert_eq!(req(&ghost, evidence).unwrap_err().code, V::UnknownMission);
    assert_eq!(req(&d.mission, &[]).unwrap_err().code, V::MissingEvidence);
    assert!(req(&d.mission, evidence).is_ok());
}

#[test]
fn dispatch_and_result_authority_are_checked() {
    let d = demo();
    let s = &d.state;
    let epoch = |st: &OrgState, r: &ResponsibilityId| st.responsibilities[r].epoch;

    // L1865: completed work cannot be dispatched (not merely "unassigned").
    let completed = program_work_completed(&d);
    assert!(matches!(
        authorize_dispatch(&completed, &d.program, &d.program_work, None),
        Err(DispatchError::ResponsibilityNotOpen { .. })
    ));

    // L1885: an active assignment that lags the responsibility's epoch. No
    // engine transition produces this (set_owner moves both together); it
    // models a persisted state from outside the engine, as the server's
    // venue-monitoring tests construct.
    let mut lagging = s.clone();
    let assignment = lagging
        .active_assignment(&d.program_work)
        .unwrap()
        .assignment_id
        .clone();
    lagging.assignments.get_mut(&assignment).unwrap().epoch -= 1;
    assert!(matches!(
        authorize_dispatch(&lagging, &d.program, &d.program_work, None),
        Err(DispatchError::StaleEpoch { .. })
    ));

    // L1892
    assert!(matches!(
        authorize_dispatch(s, &d.program, &d.program_work, Some("ledger.reserve_venue")),
        Err(DispatchError::ScopeNotGranted { .. })
    ));
    assert!(authorize_dispatch(s, &d.program, &d.program_work, Some("program.schedule")).is_ok());

    // L1970: an active role that neither owns nor executes the work.
    let e = epoch(s, &d.coordinator_work);
    assert!(matches!(
        check_job_result(s, &d.program, &d.coordinator_work, e),
        Err(DispatchError::NotAssigned { .. })
    ));
    assert!(check_job_result(s, &d.coordinator, &d.coordinator_work, e).is_ok());

    // L1991: a retired role keeps its attached capabilities but cannot use them.
    let cap = CapabilityRef::new("venue-availability", "1.0.0");
    let catalog = StaticCatalog::default().with(cap.clone(), CapabilityState::Approved);
    let attached = commit_ops(
        &d,
        s,
        vec![OrgOp::AttachCapability {
            role: RoleRef::existing(d.program.clone()),
            capability: cap.clone(),
        }],
        &catalog,
    );
    assert!(authorize_capability(&attached, &d.program, &cap).is_ok());
    let retired = program_retired(&d, &attached);
    assert!(retired.roles[&d.program].capabilities.contains(&cap));
    assert!(matches!(
        authorize_capability(&retired, &d.program, &cap),
        Err(DispatchError::RoleNotActive { .. })
    ));
}

#[test]
fn gap_publication_ref_must_be_bounded() {
    use CapabilityGapStatus as S;
    let d = demo();
    let (assigned, linked) = assigned_to_program(&d);
    let state = commit_ops(
        &d,
        &assigned,
        vec![request(&d.requirement, &d.program, &linked, PURPOSE)],
        &StaticCatalog::default(),
    );
    let id = state.capability_gaps.keys().next().unwrap().clone();
    let state = update_capability_gap(
        &state,
        &id,
        S::Acquiring {
            acquisition_id: "acq-1".into(),
        },
    )
    .unwrap()
    .state;
    let acquired = |publication_ref: Option<String>| S::Acquired {
        acquisition_id: "acq-1".into(),
        capability: CapabilityRef::new("venue-availability", "1.0.0"),
        publication_ref,
    };
    // L2211, from Acquiring and as a later enrichment of Acquired.
    for bad in [" ".to_string(), "r".repeat(401)] {
        let err = update_capability_gap(&state, &id, acquired(Some(bad.clone()))).unwrap_err();
        assert_eq!(err.code, ValidationCode::InvalidInput);
    }
    let verified = update_capability_gap(&state, &id, acquired(None))
        .unwrap()
        .state;
    let err = update_capability_gap(&verified, &id, acquired(Some(String::new()))).unwrap_err();
    assert_eq!(err.code, ValidationCode::InvalidInput);
    assert!(update_capability_gap(&verified, &id, acquired(Some("registry:v@1".into()))).is_ok());
}
