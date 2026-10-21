//! Engine tests mapped to TECH_SPEC §12 acceptance criteria.

use chrono::Utc;
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::lifecycle::*;
use genesis_core::model::*;
use std::collections::BTreeMap;

const SCOPES: &[&str] = &[
    "mission.coordinate",
    "program.schedule",
    "attendees.inform",
    "ledger.reserve_venue",
    "contract.logistics",
    "capability.venue_lookup",
];

struct Demo {
    state: OrgState,
    mission: MissionId,
    coordinator: RoleId,
    program: RoleId,
    observation: ObservationId,
    requirement: RequirementId,
}

fn constitution() -> Constitution {
    Constitution {
        scopes: SCOPES.iter().map(|s| s.to_string()).collect(),
        budget_cents: 500_000,
        max_active_roles: 6,
        max_active_contracts: 2,
    }
}

fn role_id_by_name(state: &OrgState, name: &str) -> RoleId {
    state
        .roles
        .values()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("role {name}"))
        .role_id
        .clone()
}

/// Org A: a 120-person campus event with three starting roles, then a venue
/// closure observation and the derived requirement.
fn demo() -> Demo {
    let now = Utc::now();
    let state = OrgState::new(OrgId::new("org_a"), "Org A", constitution());
    let spec = MissionBootstrap {
        goal: "Run the campus event for 120 attendees".into(),
        constraints: vec!["budget $5,000".into()],
        deadline: None,
        success_checks: vec!["venue booked for 120".into()],
        roles: vec![
            BootstrapRole {
                key: "coordinator".into(),
                name: "Mission coordinator".into(),
                charter: "Owns the mission plan".into(),
                authority: AuthorityGrant::new(["mission.coordinate"], 100_000),
                reports_to: None,
            },
            BootstrapRole {
                key: "program".into(),
                name: "Program lead".into(),
                charter: "Owns the schedule".into(),
                authority: AuthorityGrant::new(["program.schedule"], 50_000),
                reports_to: Some("coordinator".into()),
            },
            BootstrapRole {
                key: "info".into(),
                name: "Attendee information".into(),
                charter: "Keeps attendees informed".into(),
                authority: AuthorityGrant::new(["attendees.inform"], 10_000),
                reports_to: Some("coordinator".into()),
            },
        ],
        responsibilities: vec![
            BootstrapResponsibility {
                purpose: "Coordinate the mission".into(),
                owner: "coordinator".into(),
                trigger: "mission events".into(),
                termination: "event ends".into(),
            },
            BootstrapResponsibility {
                purpose: "Maintain the program schedule".into(),
                owner: "program".into(),
                trigger: "schedule changes".into(),
                termination: "event ends".into(),
            },
        ],
    };
    let (out, mission) = bootstrap_mission(&state, &spec, now).unwrap();
    assert_eq!(out.state.revision, 1);
    let state = out.state;
    let coordinator = role_id_by_name(&state, "Mission coordinator");
    let program = role_id_by_name(&state, "Program lead");
    let (out, observation) = record_observation(
        &state,
        &mission,
        "venue-portal",
        "Main hall closed for the event date",
        Some("fixture://closure".into()),
        now,
    )
    .unwrap();
    let (out, requirement) = record_requirement(
        &out.state,
        &mission,
        "Relocate the event and coordinate transport",
        std::slice::from_ref(&observation),
        "a feasible venue for 120 is reserved in the ledger",
    )
    .unwrap();
    Demo {
        state: out.state,
        mission,
        coordinator,
        program,
        observation,
        requirement,
    }
}

fn binding(provider: Provider, id: &str) -> AgentBinding {
    AgentBinding {
        provider,
        provider_agent_id: id.into(),
        config_digest: "digest".into(),
        conversation_ref: None,
    }
}

fn relocation_proposal(d: &Demo) -> OrgChangeProposal {
    OrgChangeProposal {
        expected_org_revision: d.state.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "Venue closure creates continuing relocation ownership".into(),
        budget_limit_cents: Some(200_000),
        operations: vec![
            OrgOp::CreateRole {
                key: "relocation".into(),
                name: "Relocation lead".into(),
                charter: "Find and secure a replacement venue and transport".into(),
                authority: AuthorityGrant::new(
                    ["ledger.reserve_venue", "contract.logistics"],
                    150_000,
                ),
                reports_to: Some(RoleRef::existing(d.coordinator.clone())),
                provider: Provider::Elastic,
                justification: RoleJustification::IndependentOwnership,
            },
            OrgOp::AssignResponsibility {
                target: ResponsibilityTarget::FromRequirement {
                    key: "relocate".into(),
                    requirement_id: d.requirement.clone(),
                    purpose: "Relocate the event".into(),
                    trigger: "venue availability changes".into(),
                    termination: "venue reserved and event held".into(),
                },
                owner: RoleRef::new_key("relocation"),
            },
            OrgOp::RecruitSpecialist {
                key: "logistics".into(),
                specialist_address: "agent1qlogistics".into(),
                protocol: "genesis-contract/1".into(),
                scope: "transport plan for 120 attendees".into(),
                deadline: None,
                supervising_role: Some(RoleRef::new_key("relocation")),
            },
            OrgOp::DelegateTask {
                responsibility: ResponsibilityRef::New {
                    key: "relocate".into(),
                },
                executor: ExecutorRef::Contractor {
                    contract: ContractRef::New {
                        key: "logistics".into(),
                    },
                },
                task: "Plan shuttle transport".into(),
            },
        ],
    }
}

fn empty_catalog() -> StaticCatalog {
    StaticCatalog::default()
}

fn commit_with_bindings(d: &Demo, p: &OrgChangeProposal, alloc: &KeyAllocation) -> Outcome {
    let mut bindings = BTreeMap::new();
    bindings.insert(
        "relocation".to_string(),
        binding(Provider::Elastic, "elastic-agent-1"),
    );
    commit(&d.state, p, alloc, &bindings, &empty_catalog(), Utc::now()).unwrap()
}

#[test]
fn requirement_causality_change_must_cite_known_evidence() {
    let d = demo();
    let mut p = relocation_proposal(&d);
    p.trigger_evidence_ids.clear();
    let alloc = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &alloc, &empty_catalog(), Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::MissingEvidence));

    p.trigger_evidence_ids = vec![ObservationId::new("obs_made_up")];
    let errs = validate(&d.state, &p, &alloc, &empty_catalog(), Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::UnknownEvidence));
}

#[test]
fn validated_preview_does_not_activate_new_roles() {
    let d = demo();
    let p = relocation_proposal(&d);
    let alloc = KeyAllocation::allocate(&p);
    let preview = validate(&d.state, &p, &alloc, &empty_catalog(), Utc::now()).unwrap();
    let role = &preview.state.roles[&alloc.roles["relocation"]];
    assert_eq!(role.state, RoleState::Provisioning);
    assert!(role.binding.is_none());
    assert_eq!(
        preview.state.revision, d.state.revision,
        "validation must not advance revision"
    );
    let grant = authorize_dispatch(
        &preview.state,
        &role.role_id,
        &alloc.responsibilities["relocate"],
        None,
    );
    assert!(matches!(grant, Err(DispatchError::RoleNotActive { .. })));
}

#[test]
fn meaningful_new_ownership_after_commit() {
    let d = demo();
    let p = relocation_proposal(&d);
    let alloc = KeyAllocation::allocate(&p);
    let out = commit_with_bindings(&d, &p, &alloc);
    let st = &out.state;
    assert_eq!(st.revision, d.state.revision + 1);

    let role_id = &alloc.roles["relocation"];
    let role = &st.roles[role_id];
    assert_eq!(role.state, RoleState::Active);
    assert_eq!(
        role.binding.as_ref().unwrap().provider_agent_id,
        "elastic-agent-1"
    );
    assert_eq!(role.reports_to.as_ref(), Some(&d.coordinator));

    let rsp = &st.responsibilities[&alloc.responsibilities["relocate"]];
    assert_eq!(rsp.owner, Owner::Role(role_id.clone()));
    assert_eq!(rsp.epoch, 1);
    // Delegation changed the executor, not the accountable owner.
    assert_eq!(
        rsp.executor,
        Some(Executor::Contractor(alloc.contracts["logistics"].clone()))
    );
    assert_eq!(
        st.requirements[&d.requirement].status,
        RequirementStatus::Assigned
    );

    // The new role can dispatch with its scope; the program lead cannot.
    let grant = authorize_dispatch(
        st,
        role_id,
        &rsp.responsibility_id,
        Some("ledger.reserve_venue"),
    )
    .unwrap();
    assert_eq!(grant.epoch, 1);
    assert!(authorize_dispatch(st, &d.program, &rsp.responsibility_id, None).is_err());
    // Existing program work continues.
    let program_rsp = st
        .responsibilities
        .values()
        .find(|r| r.purpose == "Maintain the program schedule")
        .unwrap();
    assert!(authorize_dispatch(
        st,
        &d.program,
        &program_rsp.responsibility_id,
        Some("program.schedule")
    )
    .is_ok());
}

#[test]
fn partial_provisioning_never_activates_role() {
    let d = demo();
    let p = relocation_proposal(&d);
    let alloc = KeyAllocation::allocate(&p);
    let errs = commit(
        &d.state,
        &p,
        &alloc,
        &BTreeMap::new(),
        &empty_catalog(),
        Utc::now(),
    )
    .unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::NotProvisioned));
    // Original state is untouched: requirement still unmet and owned by the controller.
    assert_eq!(
        d.state.requirements[&d.requirement].status,
        RequirementStatus::Unmet
    );
    assert_eq!(
        d.state.requirements[&d.requirement].owner,
        Owner::MissionController
    );
}

#[test]
fn revision_conflict_rejects_second_proposal() {
    let d = demo();
    let p1 = relocation_proposal(&d);
    let a1 = KeyAllocation::allocate(&p1);
    let after = commit_with_bindings(&d, &p1, &a1);

    // A concurrent proposal reasoned about the old revision.
    let p2 = relocation_proposal(&d);
    let a2 = KeyAllocation::allocate(&p2);
    let errs = validate(&after.state, &p2, &a2, &empty_catalog(), Utc::now()).unwrap_err();
    assert_eq!(errs[0].code, ValidationCode::RevisionConflict);
}

#[test]
fn double_assignment_is_an_accountability_conflict() {
    let d = demo();
    let p = relocation_proposal(&d);
    let a = KeyAllocation::allocate(&p);
    let after = commit_with_bindings(&d, &p, &a);
    let rsp = a.responsibilities["relocate"].clone();
    let p2 = OrgChangeProposal {
        expected_org_revision: after.state.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "try to give the same work to the program lead".into(),
        budget_limit_cents: None,
        operations: vec![OrgOp::AssignResponsibility {
            target: ResponsibilityTarget::Existing {
                responsibility_id: rsp,
            },
            owner: RoleRef::existing(d.program.clone()),
        }],
    };
    let a2 = KeyAllocation::allocate(&p2);
    let errs = validate(&after.state, &p2, &a2, &empty_catalog(), Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::AccountabilityConflict));
}

#[test]
fn authority_cannot_exceed_constitution() {
    let d = demo();
    let mut p = relocation_proposal(&d);
    if let OrgOp::CreateRole { authority, .. } = &mut p.operations[0] {
        authority.scopes.insert("treasury.wire_funds".into());
    }
    let a = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &a, &empty_catalog(), Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::AuthorityExceedsConstitution));
}

#[test]
fn budget_limits_are_enforced() {
    let d = demo();
    let mut p = relocation_proposal(&d);
    if let OrgOp::CreateRole { authority, .. } = &mut p.operations[0] {
        authority.budget_cents = 450_000; // total would exceed 500_000
    }
    p.budget_limit_cents = None;
    let a = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &a, &empty_catalog(), Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::BudgetExceeded));
}

#[test]
fn new_role_without_responsibility_is_rejected() {
    let d = demo();
    let p = OrgChangeProposal {
        expected_org_revision: d.state.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "a role with nothing to own".into(),
        budget_limit_cents: None,
        operations: vec![OrgOp::CreateRole {
            key: "idle".into(),
            name: "Idle".into(),
            charter: "Nothing".into(),
            authority: AuthorityGrant::default(),
            reports_to: None,
            provider: Provider::Elastic,
            justification: RoleJustification::Expertise,
        }],
    };
    let a = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &a, &empty_catalog(), Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::NewRoleWithoutResponsibility));
}

#[test]
fn verification_gate_unapproved_capability_cannot_attach() {
    let d = demo();
    let p = relocation_proposal(&d);
    let a = KeyAllocation::allocate(&p);
    let after = commit_with_bindings(&d, &p, &a);
    let cap = CapabilityRef::new("cap_venue", "1.0.0");
    let attach = |catalog: &StaticCatalog| {
        let p2 = OrgChangeProposal {
            expected_org_revision: after.state.revision,
            trigger_evidence_ids: vec![d.observation.clone()],
            rationale: "attach venue lookup".into(),
            budget_limit_cents: None,
            operations: vec![OrgOp::AttachCapability {
                role: RoleRef::existing(a.roles["relocation"].clone()),
                capability: cap.clone(),
            }],
        };
        let a2 = KeyAllocation::allocate(&p2);
        commit(
            &after.state,
            &p2,
            &a2,
            &BTreeMap::new(),
            catalog,
            Utc::now(),
        )
    };
    let testing = StaticCatalog::default().with(cap.clone(), CapabilityState::Testing);
    let errs = attach(&testing).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::CapabilityNotApproved));
    let errs = attach(&StaticCatalog::default()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::CapabilityUnknown));

    let approved = StaticCatalog::default().with(cap.clone(), CapabilityState::Approved);
    let ok = attach(&approved).unwrap();
    let role = &ok.state.roles[&a.roles["relocation"]];
    assert_eq!(role.capabilities, vec![cap.clone()]);
    assert!(authorize_capability(&ok.state, &role.role_id, &cap).is_ok());
    assert!(authorize_capability(&ok.state, &d.program, &cap).is_err());
}

#[test]
fn latest_capability_reference_is_rejected() {
    let d = demo();
    let p = relocation_proposal(&d);
    let a = KeyAllocation::allocate(&p);
    let after = commit_with_bindings(&d, &p, &a);
    let p2 = OrgChangeProposal {
        expected_org_revision: after.state.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "".into(),
        budget_limit_cents: None,
        operations: vec![OrgOp::AttachCapability {
            role: RoleRef::existing(a.roles["relocation"].clone()),
            capability: CapabilityRef::new("cap_venue", "latest"),
        }],
    };
    let a2 = KeyAllocation::allocate(&p2);
    let errs = validate(
        &after.state,
        &p2,
        &a2,
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap_err();
    assert_eq!(errs[0].code, ValidationCode::InvalidInput);
}

/// Hand off the relocation responsibility to a new monitor role and retire
/// the temporary relocation lead.
fn handoff_and_retire(
    d: &Demo,
    after: &OrgState,
    a: &KeyAllocation,
    handoff: bool,
) -> Result<Outcome, Vec<ValidationError>> {
    let p = OrgChangeProposal {
        expected_org_revision: after.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "Relocation done; continuing monitoring moves to a persistent role".into(),
        budget_limit_cents: None,
        operations: vec![
            OrgOp::CreateRole {
                key: "monitor".into(),
                name: "Venue monitor".into(),
                charter: "Watch the booked venue for changes".into(),
                authority: AuthorityGrant::new(["capability.venue_lookup"], 5_000),
                reports_to: Some(RoleRef::existing(d.coordinator.clone())),
                provider: Provider::Creao,
                justification: RoleJustification::Lifecycle,
            },
            OrgOp::RetireRole {
                role_id: a.roles["relocation"].clone(),
                handoffs: if handoff {
                    vec![Handoff {
                        responsibility_id: a.responsibilities["relocate"].clone(),
                        to: RoleRef::new_key("monitor"),
                    }]
                } else {
                    vec![]
                },
            },
        ],
    };
    let alloc = KeyAllocation::allocate(&p);
    let mut bindings = BTreeMap::new();
    bindings.insert(
        "monitor".to_string(),
        binding(Provider::Creao, "creao-agent-9"),
    );
    commit(
        after,
        &p,
        &alloc,
        &bindings,
        &StaticCatalog::default(),
        Utc::now(),
    )
}

#[test]
fn retirement_cannot_discard_open_obligations() {
    let d = demo();
    let p = relocation_proposal(&d);
    let a = KeyAllocation::allocate(&p);
    let after = commit_with_bindings(&d, &p, &a);
    let errs = handoff_and_retire(&d, &after.state, &a, false).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::RetirementDropsObligations));
}

#[test]
fn handoff_fences_stale_epoch_and_retires_role() {
    let d = demo();
    let p = relocation_proposal(&d);
    let a = KeyAllocation::allocate(&p);
    let after = commit_with_bindings(&d, &p, &a);
    let rsp = a.responsibilities["relocate"].clone();
    let old_role = a.roles["relocation"].clone();
    let old_grant = authorize_dispatch(&after.state, &old_role, &rsp, None).unwrap();

    let retired = handoff_and_retire(&d, &after.state, &a, true).unwrap();
    let st = &retired.state;
    assert_eq!(st.roles[&old_role].state, RoleState::Retired);
    assert_eq!(st.roles[&old_role].retired_in_revision, Some(st.revision));
    let r = &st.responsibilities[&rsp];
    assert_eq!(r.epoch, 2);
    let monitor = st
        .roles
        .values()
        .find(|r| r.name == "Venue monitor")
        .unwrap();
    assert_eq!(r.owner, Owner::Role(monitor.role_id.clone()));
    // The contractor delegation survives the handoff; the contract is kept.
    assert!(matches!(r.executor, Some(Executor::Contractor(_))));

    // Late result from the old epoch is fenced.
    assert_eq!(
        check_result_epoch(st, &rsp, old_grant.epoch),
        Err(DispatchError::StaleEpoch { current: 2, got: 1 })
    );
    assert!(check_result_epoch(st, &rsp, 2).is_ok());
    // The retired role can no longer dispatch.
    assert!(matches!(
        authorize_dispatch(st, &old_role, &rsp, None),
        Err(DispatchError::RoleNotActive { .. })
    ));
    // Exactly one active assignment remains for the responsibility.
    let active: Vec<_> = st
        .assignments
        .values()
        .filter(|x| x.responsibility_id == rsp && x.state == AssignmentState::Active)
        .collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].epoch, 2);
    // Budget is released by retirement.
    assert!(st.allocated_budget_cents() < after.state.allocated_budget_cents());
}

#[test]
fn reporting_cycles_are_rejected() {
    let d = demo();
    let p = OrgChangeProposal {
        expected_org_revision: d.state.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "".into(),
        budget_limit_cents: None,
        operations: vec![OrgOp::ChangeReportingEdge {
            role: RoleRef::existing(d.coordinator.clone()),
            reports_to: Some(RoleRef::existing(d.program.clone())),
        }],
    };
    let a = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &a, &StaticCatalog::default(), Utc::now()).unwrap_err();
    assert_eq!(errs[0].code, ValidationCode::ReportingCycle);
}

#[test]
fn referencing_a_role_key_before_creation_fails() {
    let d = demo();
    let mut p = relocation_proposal(&d);
    p.operations.swap(0, 1);
    let a = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &a, &StaticCatalog::default(), Utc::now()).unwrap_err();
    assert!(errs.iter().any(|e| e.code == ValidationCode::UnknownKey));
}

#[test]
fn duplicate_keys_are_rejected() {
    let d = demo();
    let mut p = relocation_proposal(&d);
    let dup = p.operations[0].clone();
    p.operations.insert(1, dup);
    let a = KeyAllocation::allocate(&p);
    let errs = validate(&d.state, &p, &a, &StaticCatalog::default(), Utc::now()).unwrap_err();
    assert!(errs.iter().any(|e| e.code == ValidationCode::DuplicateKey));
}

#[test]
fn proposal_json_round_trips_with_spec_operation_names() {
    let d = demo();
    let p = relocation_proposal(&d);
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["operations"][0]["op"], "CreateRole");
    assert_eq!(v["operations"][1]["owner"]["kind"], "new");
    let back: OrgChangeProposal = serde_json::from_value(v).unwrap();
    assert_eq!(back, p);
    let _ = d.mission;
}

#[test]
fn completing_a_responsibility_releases_its_assignment() {
    let d = demo();
    let p = relocation_proposal(&d);
    let a = KeyAllocation::allocate(&p);
    let after = commit_with_bindings(&d, &p, &a);
    let rsp = a.responsibilities["relocate"].clone();
    let done = complete_responsibility(&after.state, &rsp).unwrap();
    assert_eq!(
        done.state.responsibilities[&rsp].status,
        ResponsibilityStatus::Completed
    );
    assert!(done.state.active_assignment(&rsp).is_none());
    assert!(complete_responsibility(&done.state, &rsp).is_err());
}
