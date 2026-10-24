//! Regression tests for the PR #3 review findings (docs/reviews/PR-3.md).
//! Adapted from the reviewer's probes, which asserted the faulty behavior;
//! these assert the intended behavior. Run in debug and release.

use chrono::Utc;
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::lifecycle::*;
use genesis_core::model::*;
use std::collections::BTreeMap;

fn role(key: &str) -> BootstrapRole {
    BootstrapRole {
        key: key.into(),
        name: key.into(),
        charter: "owns work".into(),
        authority: AuthorityGrant::default(),
        reports_to: None,
    }
}

fn bootstrap_spec() -> MissionBootstrap {
    MissionBootstrap {
        goal: "Review mission".into(),
        constraints: vec![],
        deadline: None,
        success_checks: vec![],
        roles: vec![role("owner"), role("old_executor"), role("new_executor")],
        responsibilities: vec![BootstrapResponsibility {
            purpose: "Open work".into(),
            owner: "owner".into(),
            trigger: String::new(),
            termination: String::new(),
        }],
    }
}

fn seed() -> (OrgState, ObservationId) {
    let state = OrgState::new(OrgId::new("review"), "Review", Constitution::default());
    let (out, mission) = bootstrap_mission(&state, &bootstrap_spec(), Utc::now()).unwrap();
    let (out, obs) = record_observation(
        &out.state,
        &mission,
        "review",
        "Changed requirement",
        None,
        Utc::now(),
    )
    .unwrap();
    (out.state, obs)
}

fn id(st: &OrgState, name: &str) -> RoleId {
    st.roles
        .values()
        .find(|r| r.name == name)
        .unwrap()
        .role_id
        .clone()
}

fn change(st: &OrgState, obs: &ObservationId, operations: Vec<OrgOp>) -> OrgState {
    let p = OrgChangeProposal {
        expected_org_revision: st.revision,
        trigger_evidence_ids: vec![obs.clone()],
        rationale: "Review mutation".into(),
        operations,
        budget_limit_cents: None,
    };
    commit(
        st,
        &p,
        &KeyAllocation::allocate(&p),
        &BTreeMap::new(),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap()
    .state
}

fn delegate(st: &OrgState, obs: &ObservationId, executor: &RoleId) -> OrgState {
    let rid = st.responsibilities.keys().next().unwrap().clone();
    change(
        st,
        obs,
        vec![OrgOp::DelegateTask {
            responsibility: ResponsibilityRef::Existing {
                responsibility_id: rid,
            },
            executor: ExecutorRef::Role {
                role: RoleRef::existing(executor.clone()),
            },
            task: "Execute work".into(),
        }],
    )
}

/// Finding 1: a replaced delegated executor's in-flight result is fenced.
#[test]
fn replaced_executor_result_is_rejected() {
    let (st, obs) = seed();
    let old = id(&st, "old_executor");
    let new = id(&st, "new_executor");
    let st = delegate(&st, &obs, &old);
    let rid = st.responsibilities.keys().next().unwrap().clone();
    let grant = authorize_dispatch(&st, &old, &rid, None).unwrap();
    let st = delegate(&st, &obs, &new);
    assert!(authorize_dispatch(&st, &old, &rid, None).is_err());
    assert!(matches!(
        check_job_result(&st, &old, &rid, grant.epoch),
        Err(DispatchError::StaleEpoch { .. })
    ));
    // Newly dispatched work uses the current epoch; old owner work is fenced too.
    let current = authorize_dispatch(&st, &new, &rid, None).unwrap();
    assert!(check_job_result(&st, &new, &rid, current.epoch).is_ok());
    assert!(check_job_result(&st, &id(&st, "owner"), &rid, current.epoch).is_ok());
}

/// Finding 1: a retired executor's in-flight result is fenced.
#[test]
fn retired_executor_result_is_rejected() {
    let (st, obs) = seed();
    let old = id(&st, "old_executor");
    let st = delegate(&st, &obs, &old);
    let rid = st.responsibilities.keys().next().unwrap().clone();
    let grant = authorize_dispatch(&st, &old, &rid, None).unwrap();
    let st = change(
        &st,
        &obs,
        vec![OrgOp::RetireRole {
            role_id: old.clone(),
            handoffs: vec![],
        }],
    );
    assert_eq!(st.roles[&old].state, RoleState::Retired);
    assert!(matches!(
        check_job_result(&st, &old, &rid, grant.epoch),
        Err(DispatchError::RoleNotActive { .. })
    ));
}

/// Finding 3: after completion no second in-flight result can have effects.
#[test]
fn completed_responsibility_rejects_old_results() {
    let (st, _) = seed();
    let rid = st.responsibilities.keys().next().unwrap().clone();
    let owner = id(&st, "owner");
    let grant = authorize_dispatch(&st, &owner, &rid, None).unwrap();
    let st = complete_responsibility(&st, &rid).unwrap().state;
    assert!(matches!(
        check_result_epoch(&st, &rid, grant.epoch),
        Err(DispatchError::ResponsibilityNotOpen { .. })
    ));
    assert!(check_job_result(&st, &owner, &rid, grant.epoch).is_err());
}

/// Finding 13: bootstrap enforces the same reporting-graph invariant as changes.
#[test]
fn bootstrap_rejects_reporting_cycles() {
    let st = OrgState::new(OrgId::new("review"), "Review", Constitution::default());
    let mut two = bootstrap_spec();
    two.roles[0].reports_to = Some("old_executor".into());
    two.roles[1].reports_to = Some("owner".into());
    let errs = bootstrap_mission(&st, &two, Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::ReportingCycle));

    let mut selfloop = bootstrap_spec();
    selfloop.roles[0].reports_to = Some("owner".into());
    let errs = bootstrap_mission(&st, &selfloop, Utc::now()).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::ReportingCycle));
}

/// Finding 12: the requirement's owner follows its responsibility on handoff.
#[test]
fn transfer_updates_linked_requirement_owner() {
    let (st, obs) = seed();
    let mission = st.missions.keys().next().unwrap().clone();
    let (out, req) = record_requirement(
        &st,
        &mission,
        "New need",
        std::slice::from_ref(&obs),
        "Done",
    )
    .unwrap();
    let old = id(&out.state, "owner");
    let new = id(&out.state, "new_executor");
    let st = change(
        &out.state,
        &obs,
        vec![OrgOp::AssignResponsibility {
            target: ResponsibilityTarget::FromRequirement {
                key: "requirement_owner".into(),
                requirement_id: req.clone(),
                purpose: "Satisfy need".into(),
                trigger: "change".into(),
                termination: "done".into(),
            },
            owner: RoleRef::existing(old.clone()),
        }],
    );
    let rid = st
        .responsibilities
        .values()
        .find(|r| r.requirement_id.as_ref() == Some(&req))
        .unwrap()
        .responsibility_id
        .clone();
    let st = change(
        &st,
        &obs,
        vec![OrgOp::ReassignResponsibility {
            responsibility_id: rid.clone(),
            from: old,
            to: RoleRef::existing(new.clone()),
        }],
    );
    assert_eq!(st.responsibilities[&rid].owner, Owner::Role(new.clone()));
    assert_eq!(st.requirements[&req].owner, Owner::Role(new));
}

/// Finding 2: overflowing budgets are a validation error, never a panic or bypass.
#[test]
fn bootstrap_budget_overflow_is_rejected() {
    let st = OrgState::new(OrgId::new("review"), "Review", Constitution::default());
    let mut spec = bootstrap_spec();
    spec.roles[0].authority.budget_cents = u64::MAX;
    spec.roles[1].authority.budget_cents = 1;
    let result = std::panic::catch_unwind(|| bootstrap_mission(&st, &spec, Utc::now()));
    let errs = result.expect("must not panic").unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::BudgetExceeded));
}

/// Finding 2: an agent-submitted change with overflowing budgets is rejected.
#[test]
fn proposal_budget_overflow_is_rejected() {
    let (mut st, obs) = seed();
    let mission = st.missions.keys().next().unwrap().clone();
    let mut operations = Vec::new();
    for (key, amount) in [("huge", u64::MAX), ("one", 1)] {
        let (out, req) =
            record_requirement(&st, &mission, key, std::slice::from_ref(&obs), "Done").unwrap();
        st = out.state;
        operations.push(OrgOp::CreateRole {
            key: key.into(),
            name: key.into(),
            charter: "Do work".into(),
            authority: AuthorityGrant::new(Vec::<String>::new(), amount),
            reports_to: None,
            provider: Provider::Elastic,
            justification: RoleJustification::IndependentOwnership,
        });
        operations.push(OrgOp::AssignResponsibility {
            target: ResponsibilityTarget::FromRequirement {
                key: key.into(),
                requirement_id: req,
                purpose: "Do work".into(),
                trigger: "New".into(),
                termination: "Done".into(),
            },
            owner: RoleRef::new_key(key),
        });
    }
    let p = OrgChangeProposal {
        expected_org_revision: st.revision,
        trigger_evidence_ids: vec![obs],
        rationale: "Review budget".into(),
        operations,
        budget_limit_cents: Some(0),
    };
    let result = std::panic::catch_unwind(|| {
        validate(
            &st,
            &p,
            &KeyAllocation::allocate(&p),
            &StaticCatalog::default(),
            Utc::now(),
        )
    });
    let errs = result.expect("must not panic").unwrap_err();
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::BudgetExceeded));
    assert!(errs
        .iter()
        .any(|e| e.code == ValidationCode::ChangeBudgetExceeded));
}

/// Bootstrap configuration digests do not depend on JSON key order.
#[test]
fn bootstrap_digest_is_canonical() {
    let st = OrgState::new(OrgId::new("review"), "Review", Constitution::default());
    let (out, _) = bootstrap_mission(&st, &bootstrap_spec(), Utc::now()).unwrap();
    let role = out
        .state
        .roles
        .values()
        .find(|r| r.name == "owner")
        .unwrap();
    let expected = genesis_core::digest::canonical_sha256(&serde_json::json!({
        "name": "owner", "charter": "owns work", "authority": AuthorityGrant::default()
    }));
    assert_eq!(role.binding.as_ref().unwrap().config_digest, expected);
}

#[test]
fn reappointing_executor_never_revives_pre_revocation_grant() {
    let (st, obs) = seed();
    let a = id(&st, "old_executor");
    let b = id(&st, "new_executor");
    let owner = id(&st, "owner");
    let mut st = delegate(&st, &obs, &a);
    let rid = st.responsibilities.keys().next().unwrap().clone();
    let old = authorize_dispatch(&st, &a, &rid, None).unwrap();
    let lease = Utc::now() + chrono::Duration::minutes(10);
    st.assignments
        .get_mut(&old.assignment_id)
        .unwrap()
        .lease_expires_at = Some(lease);
    let st = delegate(&delegate(&st, &obs, &b), &obs, &a);
    let fresh = authorize_dispatch(&st, &a, &rid, None).unwrap();
    assert!(fresh.epoch > old.epoch);
    assert_ne!(fresh.assignment_id, old.assignment_id);
    assert_eq!(
        st.assignments[&old.assignment_id].state,
        AssignmentState::Fenced
    );
    assert_eq!(
        st.active_assignment(&rid).unwrap().lease_expires_at,
        Some(lease)
    );
    assert_eq!(st.responsibilities[&rid].owner, Owner::Role(owner.clone()));
    assert!(check_job_result(&st, &a, &rid, old.epoch).is_err());
    assert!(check_job_result(&st, &owner, &rid, old.epoch).is_err());
    assert!(check_job_result(&st, &a, &rid, fresh.epoch).is_ok());
    assert!(check_job_result(&st, &owner, &rid, fresh.epoch).is_ok());
    // Repeating an unchanged delegation does not revoke its current grant.
    let same = delegate(&st, &obs, &a);
    assert_eq!(authorize_dispatch(&same, &a, &rid, None).unwrap(), fresh);
}

#[test]
fn exhausted_epoch_rejects_delegation_without_wrapping_or_panicking() {
    let (mut st, obs) = seed();
    let a = id(&st, "old_executor");
    let rid = st.responsibilities.keys().next().unwrap().clone();
    st.responsibilities.get_mut(&rid).unwrap().epoch = u64::MAX;
    let aid = st.active_assignment(&rid).unwrap().assignment_id.clone();
    st.assignments.get_mut(&aid).unwrap().epoch = u64::MAX;
    let p = OrgChangeProposal {
        expected_org_revision: st.revision,
        trigger_evidence_ids: vec![obs],
        rationale: "Rotate executor".into(),
        operations: vec![OrgOp::DelegateTask {
            responsibility: ResponsibilityRef::Existing {
                responsibility_id: rid.clone(),
            },
            executor: ExecutorRef::Role {
                role: RoleRef::existing(a),
            },
            task: "Work".into(),
        }],
        budget_limit_cents: None,
    };
    let errors = validate(
        &st,
        &p,
        &KeyAllocation::allocate(&p),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap_err();
    assert!(errors
        .iter()
        .any(|e| e.code == ValidationCode::InvalidInput));
    assert_eq!(st.responsibilities[&rid].epoch, u64::MAX);
    assert_eq!(st.assignments[&aid].state, AssignmentState::Active);
}
