//! Store tests over the same real retained records as `method_adoption`.
use super::*;
use crate::method_adoption::tests::{package, Records};
use crate::method_adoption::AcceptancePin;
use crate::store::EventMeta;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{Observation, Owner, Requirement};
use serde_json::{json, Value};
use std::path::PathBuf;

fn plan_ref(records: &Records, trial: &str) -> ExactContentRef {
    let path = records.receiver.records_root.join(trial).join("plan.json");
    let plan: genesis_core::development_trial::TrialPlan =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    plan.reference().unwrap()
}

pub(crate) struct Fixture {
    pub(crate) records: Records,
    path: PathBuf,
    pub(crate) store: Store,
    pub(crate) intent: CreateMethodAdoption,
    pub(crate) now: DateTime<Utc>,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let mut records = Records::new();
        let path = records.dir.path().join("org.sqlite");
        let now = DateTime::parse_from_rfc3339("2026-10-24T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let initial = OrgState::new(
            OrgId::new("org_example_b"),
            "Receiving organization",
            crate::demo_constitution(),
        );
        let store = Store::open(&path, initial.org_id.clone()).unwrap();
        store.init(&initial).unwrap();
        store.init_method_adoptions().unwrap();
        let bootstrap: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Transport over rough terrain",
            "roles":[{"key":"lead","name":"Lead","charter":"Design carts",
                "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}}],
            "responsibilities":[{"purpose":"Rough transport","owner":"lead"}]
        }))
        .unwrap();
        let (outcome, mission_id) = bootstrap_mission(&initial, &bootstrap, now).unwrap();
        let mut state = outcome.state;
        let role_id = state.roles.keys().next().unwrap().clone();
        let responsibility_id = state.responsibilities.keys().next().unwrap().clone();
        let requirement_id = RequirementId::new("rough-transport-method");
        let observation_id = ObservationId::new("rough-transport-gap");
        state.observations.insert(
            observation_id.clone(),
            Observation {
                observation_id: observation_id.clone(),
                mission_id: mission_id.clone(),
                source: "receiver".into(),
                summary: "needs a rough-transport design method".into(),
                evidence_ref: None,
                observed_at: now,
            },
        );
        state.requirements.insert(
            requirement_id.clone(),
            Requirement {
                requirement_id: requirement_id.clone(),
                mission_id: mission_id.clone(),
                summary: "rough-transport design method".into(),
                triggering_evidence: vec![observation_id],
                acceptance: "receiver acceptance trial".into(),
                status: RequirementStatus::Assigned,
                owner: Owner::Role(role_id.clone()),
            },
        );
        state
            .responsibilities
            .get_mut(&responsibility_id)
            .unwrap()
            .requirement_id = Some(requirement_id.clone());
        store
            .commit_state(
                0,
                &state,
                outcome.effects,
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        let intent = CreateMethodAdoption {
            client_request_id: "adopt-1".into(),
            mission_id,
            requirement_id,
            role_id,
            responsibility_id,
            publisher: OrgId::new("org_example_a"),
            reference: records.reference.clone(),
            decision: MethodDecision::Adopt {
                trial: "vs-fixed-cart".into(),
            },
        };
        // The receiver pins its fixed-cart acceptance plan for this requirement.
        let pinned: genesis_core::development_trial::TrialPlan = serde_json::from_slice(
            &std::fs::read(
                records
                    .receiver
                    .records_root
                    .join("vs-fixed-cart/plan.json"),
            )
            .unwrap(),
        )
        .unwrap();
        records.receiver.acceptance_plans.push(AcceptancePin {
            requirement_id: intent.requirement_id.clone(),
            publisher: intent.publisher.clone(),
            plan: pinned.reference().unwrap(),
        });
        Self {
            records,
            path,
            store,
            intent,
            now,
        }
    }
    pub(crate) fn create(
        &self,
        intent: &CreateMethodAdoption,
    ) -> Result<(MethodAdoptionRecord, bool)> {
        self.store
            .create_method_adoption(intent, &package(), &self.records.receiver, self.now)
    }
}

#[test]
fn decisions_are_durable_idempotent_and_exactly_pinned() {
    let f = Fixture::new();
    let (adopted, new) = f.create(&f.intent).unwrap();
    assert!(new);
    assert!(matches!(
        adopted.outcome,
        MethodAdoptionOutcome::Adopted { .. }
    ));
    assert_eq!(f.create(&f.intent).unwrap(), (adopted.clone(), false));
    let mut changed = f.intent.clone();
    changed.decision = MethodDecision::Adopt {
        trial: "vs-random-search".into(),
    };
    assert!(matches!(
        f.create(&changed).unwrap_err(),
        E::IdempotencyConflict
    ));
    let mut again = f.intent.clone();
    again.client_request_id = "adopt-2".into();
    assert!(matches!(f.create(&again).unwrap_err(), E::AlreadyAdopted));
    changed.client_request_id = "adopt-3".into();
    // Under its fixed-cart pin, the receiver can't substitute another trial.
    assert!(matches!(
        f.create(&changed).unwrap_err(),
        E::Acceptance(MethodAcceptanceError::Binding(_))
    ));
    // Without any pin for the requirement, nothing can be adopted.
    let mut receiver = f.records.receiver.clone();
    receiver.acceptance_plans.clear();
    assert!(matches!(
        f.store
            .create_method_adoption(&changed, &package(), &receiver, f.now)
            .unwrap_err(),
        E::Acceptance(MethodAcceptanceError::Binding(_))
    ));
    // Had the receiver pinned random search instead, that trial decides.
    let mut receiver = f.records.receiver.clone();
    receiver.acceptance_plans[0].plan = plan_ref(&f.records, "vs-random-search");
    let (rejected, _) = f
        .store
        .create_method_adoption(&changed, &package(), &receiver, f.now)
        .unwrap();
    assert!(matches!(
        rejected.outcome,
        MethodAdoptionOutcome::NotApplicable { .. }
    ));
    let mut decline = f.intent.clone();
    decline.client_request_id = "decline-1".into();
    decline.decision = MethodDecision::Decline {
        reason: "prefer our own method".into(),
    };
    let (declined, _) = f.create(&decline).unwrap();
    assert!(matches!(
        declined.outcome,
        MethodAdoptionOutcome::Declined { .. }
    ));
    let state = f.store.current_state().unwrap();
    assert_eq!(adopted.authorize(&state, f.now).unwrap(), adopted.grant);
    for record in [&rejected, &declined] {
        assert!(matches!(
            record.authorize(&state, f.now).unwrap_err(),
            E::NotAdopted
        ));
    }
    drop(f.store);
    let reopened = Store::open(&f.path, OrgId::new("org_example_b")).unwrap();
    reopened.init_method_adoptions().unwrap();
    assert_eq!(reopened.method_adoption(&adopted.id).unwrap(), adopted);
    assert!(matches!(
        reopened.method_adoption("absent").unwrap_err(),
        E::NotFound
    ));
}

#[test]
fn authorization_follows_the_current_owner_grant() {
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    let mut state = f.store.current_state().unwrap();
    assert!(adopted.authorize(&state, f.now).is_ok());
    let assignment = state
        .assignments
        .get_mut(&adopted.grant.assignment_id)
        .unwrap();
    assignment.epoch += 1;
    assert!(matches!(
        adopted.authorize(&state, f.now).unwrap_err(),
        E::Authority(AdoptionFence::AuthorityChanged)
    ));
    let mut state = f.store.current_state().unwrap();
    state
        .requirements
        .get_mut(&f.intent.requirement_id)
        .unwrap()
        .summary
        .push_str(" v2");
    assert!(matches!(
        adopted.authorize(&state, f.now).unwrap_err(),
        E::Authority(AdoptionFence::RequirementChanged)
    ));
    let mut wrong_role = f.intent.clone();
    wrong_role.client_request_id = "adopt-4".into();
    wrong_role.role_id = RoleId::new("missing-role");
    assert!(matches!(
        f.create(&wrong_role).unwrap_err(),
        E::Authority(_)
    ));
}

#[test]
fn intent_bounds_count_limit_and_organization_are_enforced() {
    let f = Fixture::new();
    let decline = |key: String, reason: String| CreateMethodAdoption {
        client_request_id: key,
        decision: MethodDecision::Decline { reason },
        ..f.intent.clone()
    };
    for (key, reason, ok) in [
        ("k".repeat(256), "r".repeat(1024), true),
        ("k".repeat(257), "no".into(), false),
        ("".into(), "no".into(), false),
        ("k2".into(), "r".repeat(1025), false),
        ("k3".into(), " padded".into(), false),
    ] {
        assert_eq!(f.create(&decline(key, reason)).is_ok(), ok);
    }
    let mut bad_ref = f.intent.clone();
    bad_ref.reference.sha256 = "short".into();
    assert!(matches!(f.create(&bad_ref).unwrap_err(), E::InvalidInput));
    for n in 1..MAX_METHOD_ADOPTIONS {
        f.create(&decline(format!("fill-{n}"), "no".into()))
            .unwrap();
    }
    assert!(matches!(
        f.create(&decline("one-more".into(), "no".into()))
            .unwrap_err(),
        E::Limit
    ));
    let g = Fixture::new();
    let (adopted, _) = g.create(&g.intent).unwrap();
    let mut state = g.store.current_state().unwrap();
    state.org_id = OrgId::new("org_example_c");
    assert!(matches!(
        adopted.authorize(&state, g.now).unwrap_err(),
        E::Authority(AdoptionFence::AuthorityChanged)
    ));
}

#[path = "review_selected_acceptance.rs"]
mod review_selected_acceptance;

#[test]
fn a_stored_record_that_breaks_its_invariants_is_refused_on_load() {
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    let other = json!("1".repeat(64));
    let edits: [&[(&str, Value)]; 5] = [
        &[(
            "$.outcome.evidence.acceptance",
            json!({"acceptance":"rejected","reason":"no"}),
        )],
        &[("$.reference.version", json!("next"))],
        &[("$.outcome.evidence.publisher", json!("org_example_c"))],
        // Consistent JSON edits that disagree with the indexed columns.
        &[
            ("$.reference.sha256", other.clone()),
            ("$.outcome.evidence.method.sha256", other),
        ],
        &[
            ("$.publisher", json!("org_example_c")),
            ("$.outcome.evidence.publisher", json!("org_example_c")),
        ],
    ];
    for edit in edits {
        let conn = f.store.lock();
        let original: String = conn
            .query_row(
                "SELECT record_json FROM method_adoptions WHERE id=?1",
                [&adopted.id],
                |r| r.get(0),
            )
            .unwrap();
        for (path, value) in edit {
            conn.execute(
                "UPDATE method_adoptions SET record_json=json_set(record_json,?1,json(?2)) WHERE id=?3",
                rusqlite::params![path, value.to_string(), adopted.id],
            )
            .unwrap();
        }
        drop(conn);
        let refused = f.store.method_adoption(&adopted.id).unwrap_err();
        assert!(matches!(refused, E::InvalidRecord), "{edit:?}");
        f.store
            .lock()
            .execute(
                "UPDATE method_adoptions SET record_json=?1 WHERE id=?2",
                rusqlite::params![original, adopted.id],
            )
            .unwrap();
    }
    assert_eq!(f.store.method_adoption(&adopted.id).unwrap(), adopted);
}

/// Applies one direct state change as the next revision.
fn commit(f: &Fixture, change: impl FnOnce(&mut OrgState)) {
    let mut state = f.store.current_state().unwrap();
    let base = state.revision;
    state.revision += 1;
    change(&mut state);
    f.store
        .commit_state(base, &state, vec![], &EventMeta::default(), vec![], |_| {
            Ok(())
        })
        .unwrap();
}

// Independent review of `db0deec` (three confirmed findings, regression first).

#[test]
fn a_record_that_no_longer_authorizes_does_not_block_readopting() {
    // Requirement, mission or authority changes that keep the assignment and
    // epoch fence the old record; a fresh decision under the still-current
    // grant must be possible.
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    let requirement = f.intent.requirement_id.clone();
    commit(&f, |s| {
        s.requirements
            .get_mut(&requirement)
            .unwrap()
            .summary
            .push_str(" v2")
    });
    let state = f.store.current_state().unwrap();
    assert!(adopted.authorize(&state, f.now).is_err());
    let mut again = f.intent.clone();
    again.client_request_id = "adopt-after-edit".into();
    let (readopted, new) = f.create(&again).unwrap();
    assert!(new);
    assert!(readopted.authorize(&state, f.now).is_ok());
    // While a record still authorizes, a second one for the same binding is refused.
    let mut third = f.intent.clone();
    third.client_request_id = "adopt-third".into();
    assert!(matches!(f.create(&third).unwrap_err(), E::AlreadyAdopted));
}

#[test]
fn the_pin_names_the_publisher_the_receiver_evaluated() {
    // The trial records carry no publisher, so identical bytes republished by
    // another organization must not get an adopted record from this trial.
    let f = Fixture::new();
    let mut copycat = f.intent.clone();
    copycat.client_request_id = "adopt-copycat".into();
    copycat.publisher = OrgId::new("org_example_c");
    assert!(matches!(
        f.create(&copycat).unwrap_err(),
        E::Acceptance(MethodAcceptanceError::Binding(_))
    ));
    assert!(f.create(&f.intent).is_ok());
}

#[test]
fn only_the_requirement_owner_adopts() {
    // A delegated executor runs work for the owner; the adoption decision and
    // its grant stay with the owner.
    let f = Fixture::new();
    let owner = f.intent.role_id.clone();
    let exec = RoleId::new("executor-role");
    let responsibility = f.intent.responsibility_id.clone();
    commit(&f, |s| {
        let mut role = s.roles[&owner].clone();
        role.role_id = exec.clone();
        role.name = "Executor".into();
        s.roles.insert(exec.clone(), role);
        s.responsibilities
            .get_mut(&responsibility)
            .unwrap()
            .executor = Some(genesis_core::model::Executor::Role(exec.clone()));
    });
    let mut by_executor = f.intent.clone();
    by_executor.client_request_id = "adopt-by-executor".into();
    by_executor.role_id = exec;
    assert!(matches!(
        f.create(&by_executor).unwrap_err(),
        E::Authority(_)
    ));
    let (adopted, _) = f.create(&f.intent).unwrap();
    assert!(adopted
        .authorize(&f.store.current_state().unwrap(), f.now)
        .is_ok());
}

#[test]
fn indexed_binding_columns_must_match_the_stored_record() {
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    for (column, value) in [
        ("requirement_id", "other-requirement".to_string()),
        ("assignment_id", "other-assignment".to_string()),
        ("epoch", "99".to_string()),
    ] {
        let conn = f.store.lock();
        let original: String = conn
            .query_row(
                &format!("SELECT CAST({column} AS TEXT) FROM method_adoptions WHERE id=?1"),
                [&adopted.id],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            &format!("UPDATE method_adoptions SET {column}=?1 WHERE id=?2"),
            rusqlite::params![value, adopted.id],
        )
        .unwrap();
        drop(conn);
        assert!(
            matches!(
                f.store.method_adoption(&adopted.id).unwrap_err(),
                E::InvalidRecord
            ),
            "{column}"
        );
        f.store
            .lock()
            .execute(
                &format!("UPDATE method_adoptions SET {column}=?1 WHERE id=?2"),
                rusqlite::params![original, adopted.id],
            )
            .unwrap();
    }
    assert_eq!(f.store.method_adoption(&adopted.id).unwrap(), adopted);
}

/// Rewrites a stored record as the schema 1 shape #155/#159 wrote (no
/// compatibility), on the pre-fence table and index those PRs created.
fn downgrade_to_schema_one(f: &Fixture, id: &str) {
    let conn = f.store.lock();
    let json: String = conn
        .query_row(
            "SELECT record_json FROM method_adoptions WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .unwrap();
    let mut record: Value = serde_json::from_str(&json).unwrap();
    record["schema_version"] = json!(1);
    record.as_object_mut().unwrap().remove("compatibility");
    conn.execute(
        "UPDATE method_adoptions SET record_json=?1 WHERE id=?2",
        params![record.to_string(), id],
    )
    .unwrap();
    conn.execute_batch(
        "DROP INDEX method_adoption_adopted;
         ALTER TABLE method_adoptions DROP COLUMN fence_sha256;
         CREATE UNIQUE INDEX method_adoption_adopted ON method_adoptions(requirement_id,assignment_id,epoch,publisher,method_sha256) WHERE outcome='adopted';",
    )
    .unwrap();
}

fn ask(f: &Fixture) -> crate::method_invocation::MethodInvocationRequest {
    crate::method_invocation::MethodInvocationRequest {
        role_id: f.intent.role_id.clone(),
        responsibility_id: f.intent.responsibility_id.clone(),
        publisher: f.intent.publisher.clone(),
        reference: f.intent.reference.clone(),
    }
}

#[test]
fn a_migrated_schema_one_adoption_is_kept_but_never_authorizes() {
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    downgrade_to_schema_one(&f, &adopted.id);
    f.store.init_method_adoptions().unwrap();
    // Migrating twice is a no-op.
    f.store.init_method_adoptions().unwrap();
    let old = f.store.method_adoption(&adopted.id).unwrap();
    assert!(old.compatibility.is_none());
    assert_eq!(old.id, adopted.id);
    // Replay still returns the original decision under its key.
    assert_eq!(f.create(&f.intent).unwrap().0.id, adopted.id);
    // It can't show current runtime/protocol compatibility, so it fences.
    let state = f.store.current_state().unwrap();
    let ask = ask(&f);
    assert!(crate::method_invocation::authorize_method_invocation(
        &f.store,
        &state,
        &f.records.receiver,
        &ask,
        f.now
    )
    .is_err());
    // A fresh decision under the same policy is not blocked by it, and that
    // one authorizes.
    let fresh = CreateMethodAdoption {
        client_request_id: "after-migration".into(),
        ..f.intent.clone()
    };
    let (new, created) = f.create(&fresh).unwrap();
    assert!(created && new.id != adopted.id);
    let grant = crate::method_invocation::authorize_method_invocation(
        &f.store,
        &state,
        &f.records.receiver,
        &ask,
        f.now,
    )
    .unwrap();
    assert_eq!(grant.adoption_id, new.id);
    // ...and the same policy can't adopt the same method twice.
    let again = CreateMethodAdoption {
        client_request_id: "after-migration-again".into(),
        ..f.intent.clone()
    };
    assert!(matches!(f.create(&again), Err(E::AlreadyAdopted)));
}

#[test]
fn a_migration_that_meets_a_bad_record_changes_nothing() {
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    downgrade_to_schema_one(&f, &adopted.id);
    f.store
        .lock()
        .execute(
            "UPDATE method_adoptions SET record_json='{}' WHERE id=?1",
            params![adopted.id],
        )
        .unwrap();
    assert!(f.store.init_method_adoptions().is_err());
    let columns: i64 = f
        .store
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('method_adoptions') WHERE name='fence_sha256'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(columns, 0, "the failed migration must roll back");
}

// Independent review probes: retained fixtures and in-memory/SQLite contracts only.
// No method source, evaluator, provider or runtime is executed.
#[path = "review_policy.rs"]
mod review_policy;
