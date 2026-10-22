use super::*;
use crate::store::EventMeta;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::lifecycle::{MissionStatus, RoleState};
use genesis_core::model::{Observation, Requirement};
use serde_json::json;
use std::path::PathBuf;

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}

fn manifest() -> CapabilityManifest {
    serde_json::from_value(json!({
        "capability_id":"venue-lookup", "version":"1.0.0", "publisher":"Foreign Org 東京",
        "source_digest":"a".repeat(64), "intent":"PRIVATE_MANIFEST_INTENT",
        "input_schema":{"type":"object"}, "output_schema":{"type":"object"},
        "required_permissions":["capability.venue_lookup"],
        "artifact":{"actor_id":"actor_1","build_id":"build_1","build_number":"1.0.1"},
        "acceptance_suite_ref":"PRIVATE_ORACLE_REF", "state":"APPROVED",
        "verification": {"passed":true,"cases":[
            {"name":"PRIVATE_CASE_A","kind":"success","passed":true},
            {"name":"PRIVATE_CASE_B","kind":"success","passed":true},
            {"name":"PRIVATE_CASE_C","kind":"unsupported","passed":true}
        ],"tested_environment":"PRIVATE_ENVIRONMENT","oracle_ref":"PRIVATE_ORACLE_REF",
        "verified_at":"2026-10-22T00:00:00Z"}
    }))
    .unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    store: Store,
    state: OrgState,
    intent: CreateAdoption,
    now: DateTime<Utc>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("org.sqlite");
        let now = DateTime::parse_from_rfc3339("2026-10-22T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let initial = OrgState::new(
            OrgId::new("Org B 東京"),
            "Independent adoption",
            crate::demo_constitution(),
        );
        let store = Store::open(&path, initial.org_id.clone()).unwrap();
        store.init(&initial).unwrap();
        store.init_capability_adoptions().unwrap();
        let bootstrap: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Acquire venue capability",
            "roles":[{"key":"lead","name":"Lead","charter":"Find tools",
                "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}}],
            "responsibilities":[{"purpose":"Find venue capability","owner":"lead"}]
        }))
        .unwrap();
        let (outcome, mission_id) = bootstrap_mission(&initial, &bootstrap, now).unwrap();
        let mut state = outcome.state;
        let role_id = state.roles.keys().next().unwrap().clone();
        let responsibility_id = state.responsibilities.keys().next().unwrap().clone();
        let requirement_id = RequirementId::new("Requirement 東京 A");
        let observation_id = ObservationId::new("Evidence 東京 A");
        state.observations.insert(
            observation_id.clone(),
            Observation {
                observation_id: observation_id.clone(),
                mission_id: mission_id.clone(),
                source: "PRIVATE_EVIDENCE_SOURCE".into(),
                summary: "PRIVATE_EVIDENCE_SUMMARY".into(),
                evidence_ref: None,
                observed_at: now,
            },
        );
        state.requirements.insert(
            requirement_id.clone(),
            Requirement {
                requirement_id: requirement_id.clone(),
                mission_id: mission_id.clone(),
                summary: "PRIVATE_DISCOVERY_QUERY venues".into(),
                triggering_evidence: vec![observation_id],
                acceptance: "PRIVATE_REQUIREMENT_ACCEPTANCE".into(),
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
        let intent = CreateAdoption {
            client_request_id: "PRIVATE_CLIENT_KEY 東京".into(),
            mission_id,
            requirement_id,
            role_id,
            responsibility_id,
            suite_id: suite().suite_id,
            provider_access_acknowledged: true,
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

    fn create(&self) -> AdoptionRecord {
        self.store
            .create_adoption(&self.intent, &suite(), self.now)
            .unwrap()
            .0
    }

    fn mutate(&mut self, change: impl FnOnce(&mut OrgState)) {
        let base = self.state.revision;
        change(&mut self.state);
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

    fn selected(&self) -> AdoptionRecord {
        let initial = self.create();
        let claim = initial.begin_discovery(self.now).unwrap();
        self.store.save_adoption(&initial, &claim).unwrap();
        let selected = claim
            .select(
                manifest(),
                InstallationId::new("Installation 東京"),
                self.now,
            )
            .unwrap();
        self.store.save_adoption(&claim, &selected).unwrap()
    }

    fn checking(&self) -> AdoptionRecord {
        let selected = self.selected();
        let jobs = (0..suite().cases.len())
            .map(|n| JobId::new(format!("job_{n}")))
            .collect();
        let next = selected.checking(jobs, self.now).unwrap();
        self.store.save_adoption(&selected, &next).unwrap()
    }
}

#[test]
fn explicit_ack_exact_intent_utf8_and_active_slot_are_durable() {
    let f = Fixture::new();
    let mut missing_ack = f.intent.clone();
    missing_ack.provider_access_acknowledged = false;
    assert!(matches!(
        f.store.create_adoption(&missing_ack, &suite(), f.now),
        Err(AdoptionError::InvalidInput)
    ));
    assert!(f.store.list_adoptions(0, 32).unwrap().is_empty());
    let record = f.create();
    assert_eq!(record.binding.org_id.as_str(), "Org B 東京");
    assert_eq!(
        record.query(),
        f.state.requirements[&f.intent.requirement_id].summary
    );
    assert_eq!(record.binding.deadline, f.now + Duration::seconds(1800));
    let (same, created) = f
        .store
        .create_adoption(&f.intent, &suite(), f.now + Duration::hours(1))
        .unwrap();
    assert!(!created);
    assert_eq!(same.id(), record.id());
    let mut changed = f.intent.clone();
    changed.deadline = Some(f.now + Duration::seconds(10));
    assert!(matches!(
        f.store.create_adoption(&changed, &suite(), f.now),
        Err(AdoptionError::IdempotencyConflict)
    ));
    let mut new_key = f.intent.clone();
    new_key.client_request_id.push_str(" another");
    assert!(matches!(
        f.store.create_adoption(&new_key, &suite(), f.now),
        Err(AdoptionError::ActiveConflict)
    ));
    let reopened = Store::open(&f.path, f.state.org_id.clone()).unwrap();
    reopened.init_capability_adoptions().unwrap();
    assert_eq!(
        reopened.get_adoption(record.id()).unwrap().record_sha256,
        record.record_sha256
    );
}

#[test]
fn claims_consume_discovery_budget_before_reads_and_survive_reopen() {
    let f = Fixture::new();
    let mut record = f.create();
    assert!(record
        .select(manifest(), InstallationId::new("install"), f.now)
        .is_err());
    for attempt in 1..=MAX_DISCOVERY_ATTEMPTS {
        let now = f.now + Duration::seconds(i64::from(attempt));
        let claim = record.begin_discovery(now).unwrap();
        assert_eq!(claim.discovery_attempts(), attempt);
        record = f.store.save_adoption(&record, &claim).unwrap();
        assert!(f.store.due_adoption_ids(now, 16).unwrap().is_empty());
        assert_eq!(
            f.store
                .due_adoption_ids(now + Duration::seconds(1), 16)
                .unwrap(),
            vec![record.id()]
        );
        record = Store::open(&f.path, f.state.org_id.clone())
            .unwrap()
            .get_adoption(record.id())
            .unwrap();
    }
    assert!(matches!(
        record.begin_discovery(f.now + Duration::seconds(4)),
        Err(AdoptionError::Limit)
    ));
    let paused = record
        .attention(
            AdoptionAttention::RegistryUnavailable,
            f.now + Duration::seconds(4),
        )
        .unwrap();
    f.store.save_adoption(&record, &paused).unwrap();
    assert!(f
        .store
        .due_adoption_ids(f.now + Duration::hours(1), 16)
        .unwrap()
        .is_empty());
    assert!(paused
        .begin_discovery(f.now + Duration::seconds(5))
        .is_err());
}

#[test]
fn one_frozen_selection_and_full_batch_distinguish_acceptance_from_rejection() {
    let f = Fixture::new();
    let selected = f.selected();
    assert!(f
        .store
        .adoption_owns_installation(selected.installation_id().unwrap())
        .unwrap());
    assert!(!f
        .store
        .adoption_owns_installation(&InstallationId::new("unrelated"))
        .unwrap());
    assert!(selected
        .select(manifest(), InstallationId::new("other"), f.now)
        .is_err());
    assert!(selected.checking(vec![JobId::new("one")], f.now).is_err());
    assert!(selected
        .checking(vec![JobId::new("same"); suite().cases.len()], f.now)
        .is_err());
    let checking = selected
        .checking(
            (0..suite().cases.len())
                .map(|n| JobId::new(format!("job_{n}")))
                .collect(),
            f.now,
        )
        .unwrap();
    f.store.save_adoption(&selected, &checking).unwrap();
    assert!(checking
        .accept(format!("adoption-rejection:{}", "a".repeat(64)), f.now)
        .is_err());
    assert!(checking
        .rejected(format!("verification:{}", "b".repeat(64)), f.now)
        .is_err());
    let rejected = checking
        .rejected(format!("adoption-rejection:{}", "b".repeat(64)), f.now)
        .unwrap();
    f.store.save_adoption(&checking, &rejected).unwrap();
    assert_eq!(rejected.phase(), AdoptionPhase::Rejected);
    assert!(rejected.acceptance_ref().is_none());
    assert!(rejected.rejection_ref().is_some());
    assert!(f
        .store
        .adoption_owns_installation(rejected.installation_id().unwrap())
        .unwrap());
    assert!(rejected.wait_until(f.now, f.now).is_err());
    assert!(rejected
        .attention(AdoptionAttention::Internal, f.now)
        .is_err());
    let status = serde_json::to_string(&rejected.status()).unwrap();
    assert!(!status.contains("PRIVATE_"));
    assert!(status.contains("Foreign Org 東京"));
    assert!(status.contains("build_1"));
}

#[test]
fn current_authority_is_required_before_acceptance_but_committed_acceptance_survives_retirement() {
    let mut f = Fixture::new();
    let checking = f.checking();
    let accepted = checking
        .accept(format!("verification:{}", "c".repeat(64)), f.now)
        .unwrap();
    let role = f.intent.role_id.clone();
    f.mutate(|state| state.roles.get_mut(&role).unwrap().state = RoleState::Retired);
    assert!(matches!(
        f.store.save_adoption(&checking, &accepted),
        Err(AdoptionError::Conflict)
    ));
    let accepted = accepted.for_revision(f.state.revision).unwrap();
    assert!(matches!(
        f.store.save_adoption(&checking, &accepted),
        Err(AdoptionError::Authority(_))
    ));
    let fenced = checking
        .fence(AdoptionFence::AuthorityChanged, f.now)
        .unwrap()
        .for_revision(f.state.revision)
        .unwrap();
    f.store.save_adoption(&checking, &fenced).unwrap();

    let mut f = Fixture::new();
    let checking = f.checking();
    let accepted = checking
        .accept(format!("verification:{}", "d".repeat(64)), f.now)
        .unwrap();
    f.store.save_adoption(&checking, &accepted).unwrap();
    let role = f.intent.role_id.clone();
    f.mutate(|state| state.roles.get_mut(&role).unwrap().state = RoleState::Retired);
    let saved = f.store.get_adoption(accepted.id()).unwrap();
    assert_eq!(saved.acceptance_ref(), accepted.acceptance_ref());
    assert_eq!(saved.phase(), AdoptionPhase::Accepted);
    assert!(saved.authorize(&f.state, f.now).is_err());
    assert!(saved.fence(AdoptionFence::AuthorityChanged, f.now).is_err());
    assert!(!saved.phase().is_active());
}

#[test]
fn authority_binds_evidence_requirement_mission_scope_and_exact_lease_boundary() {
    let mut f = Fixture::new();
    let until = f.now + Duration::seconds(30);
    f.mutate(|state| {
        state
            .assignments
            .values_mut()
            .next()
            .unwrap()
            .lease_expires_at = Some(until)
    });
    let record = f.create();
    assert_eq!(record.binding.deadline, until);
    assert!(record
        .authorize(&f.state, until - Duration::nanoseconds(1))
        .is_ok());
    assert!(matches!(
        record.authorize(&f.state, until),
        Err(AdoptionError::Authority(AdoptionFence::LeaseExpired))
    ));
    let mut altered = f.state.clone();
    altered
        .observations
        .values_mut()
        .next()
        .unwrap()
        .summary
        .push_str(" altered");
    assert!(matches!(
        record.authorize(&altered, f.now),
        Err(AdoptionError::Authority(AdoptionFence::RequirementChanged))
    ));
    let mut altered = f.state.clone();
    altered
        .requirements
        .get_mut(&f.intent.requirement_id)
        .unwrap()
        .acceptance
        .push_str(" changed");
    assert!(matches!(
        record.authorize(&altered, f.now),
        Err(AdoptionError::Authority(AdoptionFence::RequirementChanged))
    ));
    let mut altered = f.state.clone();
    altered
        .missions
        .get_mut(&f.intent.mission_id)
        .unwrap()
        .status = MissionStatus::Succeeded;
    assert!(record.authorize(&altered, f.now).is_err());
    let selected = record
        .begin_discovery(f.now)
        .unwrap()
        .select(manifest(), InstallationId::new("install"), f.now)
        .unwrap();
    let mut altered = f.state.clone();
    altered
        .constitution
        .scopes
        .remove("capability.venue_lookup");
    assert!(selected.authorize(&altered, f.now).is_err());
    let mut altered = f.state.clone();
    altered
        .responsibilities
        .get_mut(&f.intent.responsibility_id)
        .unwrap()
        .epoch += 1;
    assert!(record.authorize(&altered, f.now).is_err());
    let record = Fixture::new().create();
    assert!(matches!(
        record.authorize(&Fixture::new().state, record.binding.deadline),
        Err(AdoptionError::Authority(AdoptionFence::DeadlineExpired))
    ));
}

#[test]
fn checkpoint_cas_and_parent_transaction_failure_preserve_both_sides() {
    let f = Fixture::new();
    let previous = f.create();
    let next = previous.begin_discovery(f.now).unwrap();
    let mut changed = f.state.clone();
    changed.revision += 1;
    let incorrect_revision = next.clone();
    let result = f.store.commit_state(
        f.state.revision,
        &changed,
        vec![],
        &EventMeta::default(),
        vec![],
        |tx| save_in_tx(tx, &previous, &incorrect_revision),
    );
    assert!(result.is_err());
    assert_eq!(f.store.current_state().unwrap().revision, f.state.revision);
    assert_eq!(f.store.get_adoption(previous.id()).unwrap().version(), 1);
    let committed = next.for_revision(changed.revision).unwrap();
    f.store
        .commit_state(
            f.state.revision,
            &changed,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| save_in_tx(tx, &previous, &committed),
        )
        .unwrap();
    assert_eq!(f.store.get_adoption(previous.id()).unwrap().version(), 2);
    let competing = previous
        .attention(AdoptionAttention::Internal, f.now)
        .unwrap()
        .for_revision(changed.revision)
        .unwrap();
    assert!(matches!(
        f.store.save_adoption(&previous, &competing),
        Err(AdoptionError::Conflict)
    ));
    assert_eq!(
        f.store.get_adoption(previous.id()).unwrap().phase(),
        AdoptionPhase::Discovering
    );
}

#[test]
fn load_rejects_every_index_drift_and_a_resealed_foreign_organization() {
    for sql in [
        "UPDATE capability_adoptions SET client_request_id='other'",
        "UPDATE capability_adoptions SET requirement_id='other'",
        "UPDATE capability_adoptions SET assignment_id='other'",
        "UPDATE capability_adoptions SET epoch=epoch+1",
        "UPDATE capability_adoptions SET version=version+1",
        "UPDATE capability_adoptions SET phase='checking'",
        "UPDATE capability_adoptions SET active=0",
        "UPDATE capability_adoptions SET next_attempt_at=NULL",
        "UPDATE capability_adoptions SET installation_id='foreign-install'",
        "UPDATE capability_adoptions SET record_json=record_json || ' garbage'",
    ] {
        let f = Fixture::new();
        let record = f.create();
        f.store.lock().execute(sql, []).unwrap();
        assert!(
            matches!(
                f.store.get_adoption(record.id()),
                Err(AdoptionError::InvalidRecord)
            ),
            "{sql}"
        );
    }
    let f = Fixture::new();
    let mut record = f.create();
    record.binding.org_id = OrgId::new("Foreign Org");
    record.seal().unwrap();
    record.validate().unwrap();
    f.store
        .lock()
        .execute(
            "UPDATE capability_adoptions SET record_json=?1",
            params![encoded(&record).unwrap()],
        )
        .unwrap();
    assert!(matches!(
        f.store.get_adoption(record.id()),
        Err(AdoptionError::InvalidRecord)
    ));
}

#[test]
fn retained_intent_limit_is_persistent_and_terminal_work_releases_active_slot() {
    let f = Fixture::new();
    for n in 0..MAX_ADOPTIONS {
        let mut intent = f.intent.clone();
        intent.client_request_id = format!("request-{n}");
        let record = f.store.create_adoption(&intent, &suite(), f.now).unwrap().0;
        let next = record
            .attention(AdoptionAttention::NoCandidate, f.now)
            .unwrap();
        f.store.save_adoption(&record, &next).unwrap();
    }
    assert_eq!(f.store.list_adoptions(0, 32).unwrap().len(), MAX_ADOPTIONS);
    let reopened = Store::open(&f.path, f.state.org_id.clone()).unwrap();
    reopened.init_capability_adoptions().unwrap();
    let mut intent = f.intent.clone();
    intent.client_request_id = "one-too-many".into();
    assert!(matches!(
        reopened.create_adoption(&intent, &suite(), f.now),
        Err(AdoptionError::Limit)
    ));
    assert!(reopened.due_adoption_ids(f.now, 16).unwrap().is_empty());
    assert!(reopened.list_adoptions(0, 33).is_err());
    assert!(reopened.due_adoption_ids(f.now, 17).is_err());
}

#[test]
fn active_limit_counts_independent_requirements_and_releases_after_fence() {
    let mut f = Fixture::new();
    let base_req = f.state.requirements[&f.intent.requirement_id].clone();
    let base_resp = f.state.responsibilities[&f.intent.responsibility_id].clone();
    let base_assignment = f.state.assignments.values().next().unwrap().clone();
    let mut intents = Vec::new();
    f.mutate(|state| {
        for n in 0..=MAX_ACTIVE_ADOPTIONS {
            let mut req = base_req.clone();
            req.requirement_id = RequirementId::new(format!("requirement-{n}"));
            let mut resp = base_resp.clone();
            resp.responsibility_id = ResponsibilityId::new(format!("responsibility-{n}"));
            resp.requirement_id = Some(req.requirement_id.clone());
            let mut assignment = base_assignment.clone();
            assignment.assignment_id = AssignmentId::new(format!("assignment-{n}"));
            assignment.responsibility_id = resp.responsibility_id.clone();
            intents.push((req.requirement_id.clone(), resp.responsibility_id.clone()));
            state.requirements.insert(req.requirement_id.clone(), req);
            state
                .responsibilities
                .insert(resp.responsibility_id.clone(), resp);
            state
                .assignments
                .insert(assignment.assignment_id.clone(), assignment);
        }
    });
    let intents: Vec<_> = intents
        .into_iter()
        .enumerate()
        .map(|(n, (requirement_id, responsibility_id))| CreateAdoption {
            client_request_id: format!("request-{n}"),
            requirement_id,
            responsibility_id,
            ..f.intent.clone()
        })
        .collect();
    let mut records = Vec::new();
    for intent in &intents[..MAX_ACTIVE_ADOPTIONS] {
        records.push(f.store.create_adoption(intent, &suite(), f.now).unwrap().0);
    }
    assert!(matches!(
        f.store
            .create_adoption(&intents[MAX_ACTIVE_ADOPTIONS], &suite(), f.now),
        Err(AdoptionError::Limit)
    ));
    let fenced = records[0]
        .fence(AdoptionFence::RequirementChanged, f.now)
        .unwrap();
    f.store.save_adoption(&records[0], &fenced).unwrap();
    assert!(f
        .store
        .create_adoption(&intents[MAX_ACTIVE_ADOPTIONS], &suite(), f.now)
        .is_ok());
    assert_eq!(
        f.store.due_adoption_ids(f.now, 16).unwrap().len(),
        MAX_ACTIVE_ADOPTIONS
    );
}

#[test]
fn bounds_suite_drift_and_resealed_forbidden_progress_fail_closed() {
    let f = Fixture::new();
    let previous = f.create();
    let mut different_suite = suite();
    different_suite.scope.push_str(" changed policy");
    assert!(previous.suite_matches(&different_suite).is_err());
    let mut mutated = previous.clone();
    mutated.query.push_str(" tampered");
    assert!(mutated.validate().is_err());
    let mut mutated = previous.begin_discovery(f.now).unwrap();
    mutated.discovery_attempts = 3;
    mutated.seal().unwrap();
    assert!(matches!(
        f.store.save_adoption(&previous, &mutated),
        Err(AdoptionError::InvalidRecord)
    ));
    let selected = previous
        .begin_discovery(f.now)
        .unwrap()
        .select(manifest(), InstallationId::new("install"), f.now)
        .unwrap();
    let status = serde_json::to_string(&selected.status()).unwrap();
    assert!(!status.contains("PRIVATE_"));
    let mut oversized = manifest();
    oversized.intent = "x".repeat(MAX_MANIFEST_BYTES);
    assert!(previous
        .begin_discovery(f.now)
        .unwrap()
        .select(oversized, InstallationId::new("install"), f.now)
        .is_err());
    let mut invalid_intent = f.intent.clone();
    invalid_intent.client_request_id = "é".repeat(129);
    assert!(matches!(
        f.store.create_adoption(&invalid_intent, &suite(), f.now),
        Err(AdoptionError::InvalidInput)
    ));

    // Oracle policy uses verification's existing bounds; only its digest is
    // retained, so a valid suite larger than the record cap must remain usable.
    let f = Fixture::new();
    let mut large_suite = suite();
    large_suite.cases[0].input["context"] = json!("x".repeat(MAX_RECORD_BYTES + 1));
    validate_suite(&large_suite).unwrap();
    let record = f
        .store
        .create_adoption(&f.intent, &large_suite, f.now)
        .unwrap()
        .0;
    record.suite_matches(&large_suite).unwrap();
    assert!(record.suite_matches(&suite()).is_err());
    assert!(encoded(&record).unwrap().len() < MAX_RECORD_BYTES);
}
