use chrono::{DateTime, Duration, Utc};
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::*;
use genesis_core::lifecycle::{AssignmentState, MissionStatus, RequirementStatus, RoleState};
use genesis_core::model::{Observation, OrgState, Owner, Requirement};
use genesis_forge::{
    CandidateIdentity, ModelConfig, PublicExample, SynthesisError, SynthesisPhase,
};
use genesis_server::capability_acquisition_store::{
    AcquisitionDebit, AcquisitionFence, AcquisitionPolicy, AcquisitionQuotas,
    AcquisitionStoreError, CreateAcquisition,
};
use genesis_server::store::{EventMeta, Store};
use genesis_verification::{decode_json, OracleSuite};
use serde_json::json;
use std::path::PathBuf;

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
fn policy() -> AcquisitionPolicy {
    AcquisitionPolicy {
        model: ModelConfig {
            model: "claude-sonnet-4-6".into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 600,
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    store: Store,
    state: OrgState,
    intent: CreateAcquisition,
    now: DateTime<Utc>,
}
impl Fixture {
    fn new(policy: &AcquisitionPolicy) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("org.sqlite");
        let now = Utc::now();
        let store = Store::open(&path, OrgId::new("org-acquisition")).unwrap();
        let mut constitution = genesis_server::demo_constitution();
        constitution.scopes.insert("capability.acquire".into());
        let initial = OrgState::new(
            OrgId::new("org-acquisition"),
            "Acquisition org",
            constitution,
        );
        store.init(&initial).unwrap();
        store.init_acquisitions(policy).unwrap();
        let bootstrap:MissionBootstrap=serde_json::from_value(json!({"goal":"Acquire required extraction capability","roles":[{"key":"lead","name":"Lead","charter":"Acquire tools","authority":{"scopes":["capability.acquire"],"budget_cents":100}}],"responsibilities":[{"purpose":"Acquire parser","owner":"lead"}]})).unwrap();
        let (outcome, mission_id) = bootstrap_mission(&initial, &bootstrap, now).unwrap();
        let mut state = outcome.state;
        let role_id = state.roles.keys().next().unwrap().clone();
        let responsibility_id = state.responsibilities.keys().next().unwrap().clone();
        let requirement_id = RequirementId::new("req-extraction");
        let observation_id = ObservationId::new("obs-public");
        state.observations.insert(
            observation_id.clone(),
            Observation {
                observation_id: observation_id.clone(),
                mission_id: mission_id.clone(),
                source: "public-web".into(),
                summary: "Venue format changed".into(),
                evidence_ref: None,
                observed_at: now,
            },
        );
        state.requirements.insert(
            requirement_id.clone(),
            Requirement {
                requirement_id: requirement_id.clone(),
                mission_id: mission_id.clone(),
                summary: "Extract public venues".into(),
                triggering_evidence: vec![observation_id],
                acceptance: "Return validated venue facts".into(),
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
        let oracle = suite();
        let urls: Vec<String> = oracle.cases.iter().map(|c| c.source.url.clone()).collect();
        let intent = CreateAcquisition {
            client_request_id: "acquire-1".into(),
            mission_id,
            requirement_id,
            role_id,
            responsibility_id,
            suite_id: oracle.suite_id.clone(),
            candidate: CandidateIdentity {
                capability_id: "venue-extraction".into(),
                capability_version: "1".into(),
                actor_name: "acquisition-venue".into(),
                source_version: "1.0".into(),
            },
            repair: None,
            examples: vec![PublicExample {
                url: urls[0].clone(),
                html: "<h1>Public venue structure</h1>".into(),
            }],
            allowed_source_urls: urls,
            deadline: None,
        };
        Self {
            _dir: dir,
            path,
            store,
            state,
            intent,
            now,
        }
    }
    fn mutate(&mut self, f: impl FnOnce(&mut OrgState)) {
        let base = self.state.revision;
        f(&mut self.state);
        self.state.revision += 1;
        self.store
            .commit_state(
                base,
                &self.state,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
}

#[test]
fn creates_bound_request_once_and_rejects_conflicting_request_or_active_grant() {
    let policy = policy();
    let f = Fixture::new(&policy);
    let suite = suite();
    let (record, created) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    assert!(created);
    assert_eq!(record.binding().org_id, f.state.org_id);
    assert_eq!(
        record.binding().grant.epoch,
        f.state.responsibilities[&f.intent.responsibility_id].epoch
    );
    assert_eq!(
        record.session().plan().request.requirement,
        "Requirement: Extract public venues\nAcceptance: Return validated venue facts"
    );
    assert_eq!(record.session().plan().model, policy.model);
    assert_eq!(record.binding().deadline, f.now + Duration::seconds(600));
    let (same, created) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now + Duration::seconds(1))
        .unwrap();
    assert!(!created);
    assert_eq!(same.id(), record.id());
    let mut conflicting = f.intent.clone();
    conflicting.examples[0].html.push_str(" altered");
    assert!(matches!(
        f.store
            .create_acquisition(&conflicting, &policy, &suite, f.now),
        Err(AcquisitionStoreError::IdempotencyConflict)
    ));
    let mut other_suite = suite.clone();
    other_suite.suite_id = "another-configured-suite".into();
    conflicting = f.intent.clone();
    conflicting.suite_id = other_suite.suite_id.clone();
    assert!(matches!(
        f.store
            .create_acquisition(&conflicting, &policy, &other_suite, f.now),
        Err(AcquisitionStoreError::IdempotencyConflict)
    ));
    conflicting = f.intent.clone();
    conflicting.client_request_id = "another-key".into();
    assert!(matches!(
        f.store
            .create_acquisition(&conflicting, &policy, &suite, f.now),
        Err(AcquisitionStoreError::ActiveConflict)
    ));
    let counts = f.store.acquisition_counters().unwrap();
    assert_eq!(counts.total_intents, 1);
    assert_eq!(counts.active, 1);
    assert_eq!(counts.model_calls, 0);
    assert_eq!(
        f.store.acquisition_suite_id(record.id()).unwrap(),
        suite.suite_id
    );
    assert_eq!(
        f.store.acquisition_ids().unwrap(),
        vec![record.id().to_owned()]
    );
}

#[test]
fn authority_snapshot_detects_requirement_mission_evidence_and_scope_changes() {
    let policy = policy();
    let f = Fixture::new(&policy);
    let suite = suite();
    let (record, _) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    record.authorize(&f.state, f.now).unwrap();
    let edits: Vec<fn(&mut OrgState, &CreateAcquisition)> = vec![
        |s, r| {
            s.requirements
                .get_mut(&r.requirement_id)
                .unwrap()
                .summary
                .push_str(" changed")
        },
        |s, r| {
            s.requirements
                .get_mut(&r.requirement_id)
                .unwrap()
                .acceptance
                .push_str(" changed")
        },
        |s, r| {
            s.requirements.get_mut(&r.requirement_id).unwrap().status = RequirementStatus::Satisfied
        },
        |s, r| s.requirements.get_mut(&r.requirement_id).unwrap().owner = Owner::MissionController,
        |s, r| {
            s.observations
                .get_mut(&s.requirements[&r.requirement_id].triggering_evidence[0])
                .unwrap()
                .summary
                .push_str(" changed")
        },
        |s, r| {
            s.roles
                .get_mut(&r.role_id)
                .unwrap()
                .authority
                .scopes
                .clear()
        },
        |s, r| s.roles.get_mut(&r.role_id).unwrap().state = RoleState::Retired,
        |s, r| s.roles.get_mut(&r.role_id).unwrap().authority.budget_cents += 1,
        |s, r| {
            s.missions
                .get_mut(&r.mission_id)
                .unwrap()
                .constraints
                .push("changed".into())
        },
        |s, r| s.missions.get_mut(&r.mission_id).unwrap().status = MissionStatus::Succeeded,
        |s, r| {
            s.responsibilities
                .get_mut(&r.responsibility_id)
                .unwrap()
                .requirement_id = None
        },
        |s, _| s.org_id = OrgId::new("foreign-org"),
    ];
    for (index, edit) in edits.into_iter().enumerate() {
        let mut changed = f.state.clone();
        edit(&mut changed, &f.intent);
        assert!(
            matches!(
                record.authorize(&changed, f.now),
                Err(AcquisitionStoreError::Authority(_))
            ),
            "case {index}"
        );
    }
    assert!(matches!(
        record.authorize(&f.state, f.now + Duration::seconds(600)),
        Err(AcquisitionStoreError::Authority(
            AcquisitionFence::DeadlineExpired
        ))
    ));
}

#[test]
fn lease_expiry_and_owner_reappointment_cannot_revive_the_saved_grant() {
    let policy = policy();
    let mut f = Fixture::new(&policy);
    let suite = suite();
    let now = f.now;
    f.mutate(|s| {
        s.assignments.values_mut().next().unwrap().lease_expires_at =
            Some(now + Duration::seconds(30))
    });
    let (record, _) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    assert_eq!(record.binding().deadline, f.now + Duration::seconds(30));
    assert!(matches!(
        record.authorize(&f.state, f.now + Duration::seconds(30)),
        Err(AcquisitionStoreError::Authority(
            AcquisitionFence::LeaseExpired
        ))
    ));
    let rid = f.intent.responsibility_id.clone();
    f.mutate(|state| {
        let old = state.active_assignment(&rid).unwrap().clone();
        state.assignments.get_mut(&old.assignment_id).unwrap().state = AssignmentState::Fenced;
        let mut new = old;
        new.assignment_id = AssignmentId::generate();
        new.epoch += 2;
        state.responsibilities.get_mut(&rid).unwrap().epoch = new.epoch;
        state.assignments.insert(new.assignment_id.clone(), new);
    });
    assert!(matches!(
        record.authorize(&f.state, f.now),
        Err(AcquisitionStoreError::Authority(
            AcquisitionFence::AuthorityChanged
        ))
    ));
}

#[test]
fn pending_intent_debit_and_cas_are_atomic_and_survive_reopening() {
    let policy = policy();
    let f = Fixture::new(&policy);
    let suite = suite();
    let (record, _) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    let mut session = record.session().clone();
    session.begin_generation(&suite).unwrap();
    let pending = record.with_session(session).unwrap();
    assert!(matches!(
        f.store.save_acquisition(
            record.version(),
            f.state.revision,
            &pending,
            &suite,
            None,
            f.now
        ),
        Err(AcquisitionStoreError::InvalidRecord)
    ));
    assert_eq!(
        f.store
            .get_acquisition(record.id(), &suite)
            .unwrap()
            .version(),
        1
    );
    assert_eq!(f.store.acquisition_counters().unwrap().model_calls, 0);
    let saved = f
        .store
        .save_acquisition(
            record.version(),
            f.state.revision,
            &pending,
            &suite,
            Some(AcquisitionDebit::ModelCall),
            f.now,
        )
        .unwrap();
    assert_eq!(saved.version(), 2);
    let other = Store::open(&f.path, f.state.org_id.clone()).unwrap();
    other.init_acquisitions(&policy).unwrap();
    assert_eq!(
        other
            .get_acquisition(record.id(), &suite)
            .unwrap()
            .session()
            .generation()
            .phase(),
        SynthesisPhase::Requesting
    );
    assert_eq!(other.acquisition_counters().unwrap().model_calls, 1);
    assert!(matches!(
        other.save_acquisition(
            record.version(),
            f.state.revision,
            &pending,
            &suite,
            Some(AcquisitionDebit::ModelCall),
            f.now
        ),
        Err(AcquisitionStoreError::Conflict)
    ));
    assert!(matches!(
        other.save_acquisition(
            saved.version(),
            f.state.revision + 1,
            &saved,
            &suite,
            None,
            f.now
        ),
        Err(AcquisitionStoreError::Conflict)
    ));
    assert_eq!(other.acquisition_counters().unwrap().model_calls, 1);
    let mut interrupted = saved.session().clone();
    assert!(interrupted.recover_interrupted(&suite).unwrap());
    let next = saved.with_session(interrupted).unwrap();
    other
        .save_acquisition(
            saved.version(),
            f.state.revision,
            &next,
            &suite,
            None,
            f.now,
        )
        .unwrap();
    let counts = other.acquisition_counters().unwrap();
    assert_eq!(counts.model_calls, 1);
    assert_eq!(counts.total_intents, 1);
    assert_eq!(counts.active, 0);
}

#[test]
fn quota_exhaustion_never_persists_unfunded_intent_and_quota_changes_do_not_reset_counts() {
    let mut policy = policy();
    policy.quotas.model_calls = 1;
    let f = Fixture::new(&policy);
    let suite = suite();
    let (record, _) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    let mut session = record.session().clone();
    session.begin_generation(&suite).unwrap();
    let pending = record.with_session(session).unwrap();
    let saved = f
        .store
        .save_acquisition(
            1,
            f.state.revision,
            &pending,
            &suite,
            Some(AcquisitionDebit::ModelCall),
            f.now,
        )
        .unwrap();
    let mut session = saved.session().clone();
    session
        .complete_generation(Err(SynthesisError::Protocol), &suite)
        .unwrap();
    let ended = f
        .store
        .save_acquisition(
            saved.version(),
            f.state.revision,
            &saved.with_session(session).unwrap(),
            &suite,
            None,
            f.now,
        )
        .unwrap();
    assert!(!ended.is_fenced());
    let mut second = f.intent.clone();
    second.client_request_id = "another-acquisition".into();
    let (second, _) = f
        .store
        .create_acquisition(&second, &policy, &suite, f.now)
        .unwrap();
    let mut session = second.session().clone();
    session.begin_generation(&suite).unwrap();
    let pending = second.with_session(session).unwrap();
    assert!(matches!(
        f.store.save_acquisition(
            second.version(),
            f.state.revision,
            &pending,
            &suite,
            Some(AcquisitionDebit::ModelCall),
            f.now
        ),
        Err(AcquisitionStoreError::QuotaExceeded("model_calls"))
    ));
    let unchanged = f.store.get_acquisition(second.id(), &suite).unwrap();
    assert_eq!(unchanged.version(), 1);
    assert_eq!(
        unchanged.session().generation().phase(),
        SynthesisPhase::Prepared
    );
    let mut changed = policy.clone();
    changed.quotas.model_calls = 2;
    let reopened = Store::open(&f.path, f.state.org_id.clone()).unwrap();
    assert!(matches!(
        reopened.init_acquisitions(&changed),
        Err(AcquisitionStoreError::PolicyChanged)
    ));
    reopened.init_acquisitions(&policy).unwrap();
    assert_eq!(reopened.acquisition_counters().unwrap().model_calls, 1);
    assert_eq!(reopened.acquisition_counters().unwrap().active, 1);
    let mut changed_model = policy.clone();
    changed_model.model.model = "other-concrete-model".into();
    reopened.init_acquisitions(&changed_model).unwrap();
    assert_eq!(
        reopened
            .get_acquisition(record.id(), &suite)
            .unwrap()
            .session()
            .plan()
            .model,
        policy.model
    );
}

#[test]
fn stored_record_cannot_be_loaded_or_replayed_under_another_organization() {
    let policy = policy();
    let f = Fixture::new(&policy);
    let suite = suite();
    let (record, _) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    // Simulate a copied record retained under a different organization snapshot.
    // Its own digest remains valid; the owning database must also match.
    let mut foreign = f.state.clone();
    foreign.org_id = OrgId::new("another-organization");
    let connection = rusqlite::Connection::open(&f.path).unwrap();
    connection
        .execute(
            "UPDATE org_snapshots SET state_json=?1 WHERE revision=?2",
            rusqlite::params![serde_json::to_string(&foreign).unwrap(), foreign.revision],
        )
        .unwrap();
    assert!(matches!(
        f.store.get_acquisition(record.id(), &suite),
        Err(AcquisitionStoreError::InvalidRecord)
    ));
    assert!(matches!(
        f.store
            .create_acquisition(&f.intent, &policy, &suite, f.now),
        Err(AcquisitionStoreError::InvalidRecord)
    ));
    assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 1);
}

#[test]
fn authority_loss_blocks_debit_but_preserves_completion_with_a_permanent_fence() {
    let policy = policy();
    let mut f = Fixture::new(&policy);
    let suite = suite();
    let (record, _) = f
        .store
        .create_acquisition(&f.intent, &policy, &suite, f.now)
        .unwrap();
    let mut session = record.session().clone();
    session.begin_generation(&suite).unwrap();
    let pending = record.with_session(session).unwrap();
    let saved = f
        .store
        .save_acquisition(
            1,
            f.state.revision,
            &pending,
            &suite,
            Some(AcquisitionDebit::ModelCall),
            f.now,
        )
        .unwrap();
    let role = f.intent.role_id.clone();
    f.mutate(|s| s.roles.get_mut(&role).unwrap().state = RoleState::Retired);
    let mut session = saved.session().clone();
    session
        .complete_generation(Err(SynthesisError::Http { status: 503 }), &suite)
        .unwrap();
    let fenced = f
        .store
        .save_acquisition(
            saved.version(),
            f.state.revision,
            &saved.with_session(session).unwrap(),
            &suite,
            None,
            f.now,
        )
        .unwrap();
    assert!(fenced.is_fenced());
    assert_eq!(
        fenced.session().generation().failure(),
        Some(&SynthesisError::Http { status: 503 })
    );
    f.mutate(|s| s.roles.get_mut(&role).unwrap().state = RoleState::Active);
    assert!(matches!(
        fenced.authorize(&f.state, f.now),
        Err(AcquisitionStoreError::Authority(_))
    ));
    assert_eq!(f.store.acquisition_counters().unwrap().model_calls, 1);
    assert_eq!(f.store.acquisition_counters().unwrap().active, 0);
    let mut fresh = f.intent.clone();
    fresh.client_request_id = "revoked-before-intent".into();
    let (fresh, _) = f
        .store
        .create_acquisition(&fresh, &policy, &suite, f.now)
        .unwrap();
    let mut session = fresh.session().clone();
    session.begin_generation(&suite).unwrap();
    let pending = fresh.with_session(session).unwrap();
    f.mutate(|s| s.roles.get_mut(&role).unwrap().authority.scopes.clear());
    assert!(matches!(
        f.store.save_acquisition(
            fresh.version(),
            f.state.revision,
            &pending,
            &suite,
            Some(AcquisitionDebit::ModelCall),
            f.now
        ),
        Err(AcquisitionStoreError::Authority(_))
    ));
    assert_eq!(f.store.acquisition_counters().unwrap().model_calls, 1);
    assert_eq!(
        f.store
            .get_acquisition(fresh.id(), &suite)
            .unwrap()
            .version(),
        1
    );
}

#[test]
fn invalid_policy_or_missing_scope_leaves_no_acquisition_and_no_count_debits() {
    let policy = policy();
    let mut f = Fixture::new(&policy);
    let suite = suite();
    let role = f.intent.role_id.clone();
    f.mutate(|s| s.roles.get_mut(&role).unwrap().authority.scopes.clear());
    assert!(matches!(
        f.store
            .create_acquisition(&f.intent, &policy, &suite, f.now),
        Err(AcquisitionStoreError::Authority(_))
    ));
    assert!(f.store.acquisition_ids().unwrap().is_empty());
    assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 0);
    let mut invalid = policy;
    invalid.quotas.active = 0;
    assert!(matches!(
        invalid.validate(),
        Err(AcquisitionStoreError::InvalidPolicy)
    ));
}
