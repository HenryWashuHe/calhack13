//! Review regressions for PR70 at c1162d1. No production code changes.
use chrono::Utc;
use genesis_core::audit::{audit, audit_transition};
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::model::*;
use genesis_server::audit::audit_database;
use genesis_server::store::{insert_snapshot, EventMeta, Store};
use rusqlite::params;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn bootstrap(store: &Store, role_count: usize) -> (OrgState, MissionId) {
    let state = OrgState::new(
        OrgId::new("org_a"),
        "A",
        Constitution {
            scopes: ["a".to_owned()].into(),
            budget_cents: 1000,
            max_active_roles: role_count as u32,
            max_active_contracts: 1,
        },
    );
    store.init(&state).unwrap();
    let spec = MissionBootstrap {
        goal: "Relocate the event".into(),
        constraints: vec![],
        deadline: None,
        success_checks: vec![],
        roles: (0..role_count)
            .map(|i| BootstrapRole {
                key: format!("r{i}"),
                name: format!("Role {i}"),
                charter: "Own the plan".into(),
                authority: AuthorityGrant::new(["a"], 0),
                reports_to: (i > 0).then(|| format!("r{}", i - 1)),
            })
            .collect(),
        responsibilities: vec![BootstrapResponsibility {
            purpose: "Coordinate".into(),
            owner: "r0".into(),
            trigger: "events".into(),
            termination: "event ends".into(),
        }],
    };
    let (out, mission) = bootstrap_mission(&state, &spec, Utc::now()).unwrap();
    save(store, &state, &out);
    (out.state, mission)
}

fn save(store: &Store, prev: &OrgState, next: &Outcome) {
    assert!(audit(&next.state).is_empty());
    assert!(audit_transition(prev, &next.state).is_empty());
    store
        .commit_state(
            prev.revision,
            &next.state,
            next.effects.clone(),
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
}

fn gap_history(store: &Store) -> (OrgState, CapabilityGapId) {
    let (state, mission) = bootstrap(store, 1);
    let (observed, observation) = record_observation(
        &state,
        &mission,
        "venue-portal",
        "The venue closed",
        Some("https://portal.example/a".into()),
        Utc::now(),
    )
    .unwrap();
    save(store, &state, &observed);
    let (required, requirement) = record_requirement(
        &observed.state,
        &mission,
        "Find a venue",
        std::slice::from_ref(&observation),
        "A feasible venue is verified",
    )
    .unwrap();
    save(store, &observed.state, &required);
    let lead = required.state.roles.keys().next().unwrap().clone();
    let proposal = OrgChangeProposal {
        expected_org_revision: required.state.revision,
        trigger_evidence_ids: vec![observation],
        rationale: "Need a reader".into(),
        budget_limit_cents: Some(0),
        operations: vec![
            OrgOp::AssignResponsibility {
                target: ResponsibilityTarget::FromRequirement {
                    key: "relocate".into(),
                    requirement_id: requirement.clone(),
                    purpose: "Relocate".into(),
                    trigger: "closure".into(),
                    termination: "replacement found".into(),
                },
                owner: RoleRef::existing(lead.clone()),
            },
            OrgOp::RequestCapability {
                key: "reader".into(),
                requirement_id: requirement,
                role: RoleRef::existing(lead),
                responsibility: ResponsibilityRef::New {
                    key: "relocate".into(),
                },
                capability_id: CapabilityId::new("venue-reader"),
                purpose: "Read venue data".into(),
                observed_sources: vec!["https://portal.example/a".into()],
            },
        ],
    };
    let out = commit(
        &required.state,
        &proposal,
        &KeyAllocation::allocate(&proposal),
        &BTreeMap::new(),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap();
    save(store, &required.state, &out);
    let id = out.state.capability_gaps.keys().next().unwrap().clone();
    (out.state, id)
}

#[test]
fn immutable_gap_source_and_evidence_rewrites_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("gaps.db"), OrgId::new("org_a"))
        .unwrap()
        .with_commit_audit(true);
    let (state, id) = gap_history(&store);
    let other_responsibility = state
        .responsibilities
        .keys()
        .find(|r| **r != state.capability_gaps[&id].responsibility_id)
        .unwrap()
        .clone();
    let mut missed = vec![];
    for field in [
        "source",
        "evidence",
        "purpose",
        "responsibility",
        "opened_revision",
        "gap_id",
    ] {
        let mut next = state.clone();
        next.revision += 1;
        let gap = next.capability_gaps.get_mut(&id).unwrap();
        match field {
            "source" => gap.observed_sources = vec!["https://foreign.example/other".into()],
            "evidence" => gap.triggering_evidence.clear(),
            "purpose" => gap.purpose = "Different capability work".into(),
            "responsibility" => gap.responsibility_id = other_responsibility.clone(),
            "opened_revision" => gap.opened_in_revision = 0,
            "gap_id" => gap.gap_id = CapabilityGapId::new("gap_different"),
            _ => unreachable!(),
        }
        if audit(&next).is_empty() && audit_transition(&state, &next).is_empty() {
            missed.push(field);
        }
    }
    let mut rewritten = state.clone();
    rewritten.revision += 1;
    rewritten
        .capability_gaps
        .get_mut(&id)
        .unwrap()
        .observed_sources = vec!["https://foreign.example/other".into()];
    let accepted_by_commit_guard = store
        .commit_state(
            state.revision,
            &rewritten,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .is_ok();
    assert!(
        missed.is_empty() && !accepted_by_commit_guard,
        "audits accepted rewrites of frozen gap fields: {missed:?}; opt-in commit guard persisted source rewrite: {accepted_by_commit_guard}"
    );
}

#[test]
fn acquired_capability_and_publication_cannot_be_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("gaps.db"), OrgId::new("org_a"))
        .unwrap()
        .with_commit_audit(true);
    let (state, id) = gap_history(&store);
    let claiming = update_capability_gap(
        &state,
        &id,
        CapabilityGapStatus::Acquiring {
            acquisition_id: "acq1".into(),
        },
    )
    .unwrap();
    save(&store, &state, &claiming);
    let acquired = |version: &str, publication: Option<&str>| CapabilityGapStatus::Acquired {
        acquisition_id: "acq1".into(),
        capability: CapabilityRef {
            capability_id: CapabilityId::new("venue-reader"),
            version: version.into(),
        },
        publication_ref: publication.map(String::from),
    };
    let verified = update_capability_gap(&claiming.state, &id, acquired("v1", None)).unwrap();
    save(&store, &claiming.state, &verified);
    // This actual legal publication transition must remain accepted.
    let published =
        update_capability_gap(&verified.state, &id, acquired("v1", Some("pub1"))).unwrap();
    save(&store, &verified.state, &published);
    assert!(audit_transition(&state, &published.state).is_empty());
    let mut missed = vec![];
    for (label, status) in [
        ("verified version replacement", acquired("v2", Some("pub1"))),
        ("publication replacement", acquired("v1", Some("pub2"))),
        ("publication removal", acquired("v1", None)),
    ] {
        assert!(
            update_capability_gap(&published.state, &id, status.clone()).is_err(),
            "kernel must refuse {label}"
        );
        let mut next = published.state.clone();
        next.revision += 1;
        next.capability_gaps.get_mut(&id).unwrap().status = status;
        if audit(&next).is_empty() && audit_transition(&published.state, &next).is_empty() {
            missed.push(label);
        }
    }
    // Legal subsequent installation still passes both audits and commit refusal.
    let manifest: genesis_core::capability::CapabilityManifest =
        serde_json::from_value(serde_json::json!({
            "capability_id": "venue-reader", "version": "v1", "publisher": "org_a", "source_digest": "sha256:source",
            "intent": "Read venue data", "input_schema": {}, "output_schema": {},
            "artifact": {"actor_id":"actor1", "build_id":"build1", "build_number":"0.1"},
            "acceptance_suite_ref":"suite1",
            "verification": {"passed":true, "cases":[], "tested_environment":"fixture",
                "oracle_ref":"private", "verified_at":Utc::now()},
            "state":"APPROVED"
        }))
        .unwrap();
    let (installing, installation_id) = begin_installation(
        &published.state,
        &manifest,
        serde_json::Value::Null,
        Utc::now(),
    )
    .unwrap();
    save(&store, &published.state, &installing);
    let accepted = finish_installation(
        &installing.state,
        &installation_id,
        true,
        "acceptance1",
        vec![],
    )
    .unwrap();
    save(&store, &installing.state, &accepted);
    let installed = update_capability_gap(
        &accepted.state,
        &id,
        CapabilityGapStatus::Installed {
            acquisition_id: "acq1".into(),
            capability: manifest.reference(),
            installation_id,
        },
    )
    .unwrap();
    save(&store, &accepted.state, &installed);
    assert!(audit_transition(&state, &installed.state).is_empty());
    assert!(
        missed.is_empty(),
        "audits accepted illegal gap outcome rewrites: {missed:?}"
    );
}

fn reporting_change(state: &OrgState, evidence: &ObservationId) -> Outcome {
    let root = state
        .roles
        .values()
        .find(|r| r.reports_to.is_none())
        .unwrap();
    let proposal = OrgChangeProposal {
        expected_org_revision: state.revision,
        trigger_evidence_ids: vec![evidence.clone()],
        rationale: "Confirm reporting ownership".into(),
        budget_limit_cents: Some(0),
        operations: vec![OrgOp::ChangeReportingEdge {
            role: RoleRef::existing(root.role_id.clone()),
            reports_to: None,
        }],
    };
    commit(
        state,
        &proposal,
        &KeyAllocation::allocate(&proposal),
        &BTreeMap::new(),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap()
}

#[test]
fn online_audit_uses_one_database_snapshot_for_state_and_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let store = Store::open(&path, OrgId::new("org_a"))
        .unwrap()
        .with_commit_audit(true);
    let (state, mission) = bootstrap(&store, 128);
    let (observed, observation) = record_observation(
        &state,
        &mission,
        "operator",
        "Review structure",
        None,
        Utc::now(),
    )
    .unwrap();
    save(&store, &state, &observed);
    let mut state = observed.state;
    for _ in 0..250 {
        let next = reporting_change(&state, &observation);
        save(&store, &state, &next);
        state = next.state;
    }
    let pending = reporting_change(&state, &observation);
    drop(store);
    let mut writer = rusqlite::Connection::open(&path).unwrap();
    // Rollback-journal locking makes the interleaving observable: after detecting
    // the auditor's shared read lock, a pending writer commits before its next
    // independent SELECT can start. No production hooks or sleeps in audit code.
    writer
        .pragma_update(None, "journal_mode", "DELETE")
        .unwrap();
    writer.busy_timeout(Duration::ZERO).unwrap();
    let audit_path = path.clone();
    let audit = std::thread::spawn(move || audit_database(&audit_path, &OrgId::new("org_a")));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match writer.execute_batch("BEGIN EXCLUSIVE; ROLLBACK;") {
            Ok(()) => {
                assert!(
                    !audit.is_finished(),
                    "auditor finished before shared-lock synchronization"
                );
                assert!(Instant::now() < deadline, "no auditor read lock observed");
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy =>
            {
                break
            }
            Err(e) => panic!("unexpected synchronization error: {e}"),
        }
    }
    writer.busy_timeout(Duration::from_secs(10)).unwrap();
    let tx = writer.transaction().unwrap();
    insert_snapshot(&tx, &pending.state).unwrap();
    for event in &pending.effects {
        let value = serde_json::to_value(event).unwrap();
        tx.execute("INSERT INTO events (event_id, schema_version, org_id, revision, event_type, payload_json, created_at) VALUES (?1, 1, 'org_a', ?2, ?3, ?4, ?5)",
            params![EventId::generate().as_str(), pending.state.revision as i64,
                event.type_name(), serde_json::to_string(&value["payload"]).unwrap(), Utc::now().to_rfc3339()]).unwrap();
    }
    tx.commit().unwrap();
    let concurrent = audit.join().unwrap().unwrap();
    let quiescent = audit_database(&path, &OrgId::new("org_a")).unwrap();
    assert!(
        quiescent.ok(),
        "actual fully committed history is valid: {:?}",
        quiescent.violations
    );
    assert!(
        concurrent.ok(),
        "online audit mixed states and events from different committed snapshots: {:?}",
        concurrent.violations
    );
}
