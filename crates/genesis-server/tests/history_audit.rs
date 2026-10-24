//! Henry's Claude: `audit_history` passes a real stored history and names
//! rewritten history, foreign snapshots and orphaned events.
use chrono::Utc;
use genesis_core::engine::{
    bootstrap_mission, record_observation, record_requirement, BootstrapResponsibility,
    BootstrapRole, MissionBootstrap,
};
use genesis_core::ids::OrgId;
use genesis_core::model::*;
use genesis_server::audit::{audit_database, audit_history};
use genesis_server::store::{EventMeta, Store};
use rusqlite::params;

fn constitution() -> Constitution {
    Constitution {
        scopes: ["a"].map(String::from).into(),
        budget_cents: 100,
        max_active_roles: 3,
        max_active_contracts: 1,
    }
}

/// A store holding r0 (initialized), r1 (mission), r2 (observation), r3 (requirement).
fn history(store: &Store) -> Vec<OrgState> {
    let now = Utc::now();
    let s0 = OrgState::new(OrgId::new("org_a"), "A", constitution());
    store.init(&s0).unwrap();
    let spec = MissionBootstrap {
        goal: "g".into(),
        constraints: vec![],
        deadline: None,
        success_checks: vec![],
        roles: vec![BootstrapRole {
            key: "lead".into(),
            name: "Lead".into(),
            charter: "c".into(),
            authority: AuthorityGrant::new(["a"], 10),
            reports_to: None,
        }],
        responsibilities: vec![BootstrapResponsibility {
            purpose: "p".into(),
            owner: "lead".into(),
            trigger: "t".into(),
            termination: "e".into(),
        }],
    };
    let (o1, m) = bootstrap_mission(&s0, &spec, now).unwrap();
    let (o2, obs) = record_observation(
        &o1.state,
        &m,
        "s",
        "x",
        Some("https://p.example/a".into()),
        now,
    )
    .unwrap();
    let (o3, _) = record_requirement(&o2.state, &m, "r", &[obs], "acc").unwrap();
    let mut states = vec![s0];
    for o in [o1, o2, o3] {
        let base = states.last().unwrap().revision;
        store
            .commit_state(
                base,
                &o.state,
                o.effects,
                &EventMeta::mission(m.clone()),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        states.push(o.state);
    }
    states
}

#[test]
fn a_stored_history_passes_and_rewrites_are_named() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("org_a.db");
    let store = Store::open(&path, OrgId::new("org_a")).unwrap();
    let states = history(&store);

    let report = audit_history(&store).unwrap();
    assert!(report.ok(), "{:#?}", report.violations);
    assert_eq!(report.snapshots_checked, 4);
    assert_eq!(
        (report.first_revision, report.last_revision),
        (Some(0), Some(3))
    );
    assert!(report.events_checked >= 4);

    // Rewrite history: r3 loses its requirement and observation, and an event
    // names a revision that was never stored.
    let mut rewritten = states[3].clone();
    rewritten.requirements.clear();
    rewritten.observations.clear();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE org_snapshots SET state_json = ?1 WHERE revision = 3",
            params![serde_json::to_string(&rewritten).unwrap()],
        )
        .unwrap();
        conn.execute(
            "UPDATE events SET revision = 9 WHERE seq = (SELECT MAX(seq) FROM events)",
            [],
        )
        .unwrap();
    }
    let report = audit_database(&path, &OrgId::new("org_a")).unwrap();
    let found: Vec<_> = report.violations.iter().map(|v| v.invariant).collect();
    for expected in ["history_is_append_only", "event_revision_known"] {
        assert!(found.contains(&expected), "{expected} not in {found:?}");
    }
    assert!(report
        .violations
        .iter()
        .all(|v| v.revision == 3 || v.revision == 9));

    // The wrong organization's database is named too.
    let report = audit_database(&path, &OrgId::new("org_b")).unwrap();
    let found: Vec<_> = report.violations.iter().map(|v| v.invariant).collect();
    for expected in ["snapshot_org_matches", "event_org_matches"] {
        assert!(found.contains(&expected), "{expected} not in {found:?}");
    }
}

/// Eric's Claude: a stored snapshot that rewrites the constitution to cover a
/// widened role is named at its revision. The state checks compare against the
/// snapshot's own constitution, so only the step from the previous revision
/// can tell.
#[test]
fn a_rewritten_constitution_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("org_a.db");
    let store = Store::open(&path, OrgId::new("org_a")).unwrap();
    let states = history(&store);

    let mut rewritten = states[3].clone();
    rewritten.constitution.budget_cents = 10_000;
    rewritten.constitution.scopes.insert("payments.send".into());
    let lead = rewritten.roles.values_mut().next().unwrap();
    lead.authority.budget_cents = 5_000;
    lead.authority.scopes.insert("payments.send".into());
    // On its own the rewritten state is well formed.
    assert!(genesis_core::audit::audit(&rewritten).is_empty());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE org_snapshots SET state_json = ?1 WHERE revision = 3",
            params![serde_json::to_string(&rewritten).unwrap()],
        )
        .unwrap();

    let report = audit_database(&path, &OrgId::new("org_a")).unwrap();
    let found: Vec<_> = report
        .violations
        .iter()
        .map(|v| (v.revision, v.invariant))
        .collect();
    assert_eq!(
        found,
        [(3, "constitution_unchanged")],
        "{:#?}",
        report.violations
    );
    let detail = &report.violations[0].detail;
    assert!(detail.contains("budget_cents 100 -> 10000"), "{detail}");
    assert!(detail.contains("scopes added: payments.send"), "{detail}");
}

#[test]
fn a_gap_in_snapshot_history_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("org_a.db");
    let store = Store::open(&path, OrgId::new("org_a")).unwrap();
    history(&store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM org_snapshots WHERE revision = 2", [])
        .unwrap();
    let report = audit_database(&path, &OrgId::new("org_a")).unwrap();
    let found: Vec<_> = report.violations.iter().map(|v| v.invariant).collect();
    assert!(found.contains(&"snapshot_history_contiguous"), "{found:?}");
    assert!(found.contains(&"event_revision_known"), "{found:?}");
}

#[test]
fn commit_audit_refuses_an_ill_formed_commit_and_keeps_the_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("org_a.db");
    let store = Store::open(&path, OrgId::new("org_a"))
        .unwrap()
        .with_commit_audit(true);
    // Legal history commits normally with auditing on.
    let states = history(&store);
    let last = states.last().unwrap();

    // A commit that drops the recorded requirement is refused.
    let mut bad = last.clone();
    bad.revision += 1;
    bad.requirements.clear();
    let err = store
        .commit_state(
            last.revision,
            &bad,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(err.to_string().contains("history_is_append_only"), "{err}");
    assert_eq!(store.current_state().unwrap().revision, last.revision);
    assert!(audit_history(&store).unwrap().ok());

    // Off (the default), the same commit is stored and the audit reports it.
    let plain = Store::open(&path, OrgId::new("org_a"))
        .unwrap()
        .with_commit_audit(false);
    plain
        .commit_state(
            last.revision,
            &bad,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    assert!(!audit_history(&plain).unwrap().ok());
}
