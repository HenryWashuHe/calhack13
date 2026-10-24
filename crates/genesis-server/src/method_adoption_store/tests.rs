//! Store tests over the same real retained records as `method_adoption`.
use super::*;
use crate::method_adoption::tests::{package, Records};
use crate::store::EventMeta;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{Observation, Owner, Requirement};
use serde_json::json;
use std::path::PathBuf;

struct Fixture {
    records: Records,
    path: PathBuf,
    store: Store,
    intent: CreateMethodAdoption,
    now: DateTime<Utc>,
}

impl Fixture {
    fn new() -> Self {
        let records = Records::new();
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
        Self {
            records,
            path,
            store,
            intent,
            now,
        }
    }
    fn create(&self, intent: &CreateMethodAdoption) -> Result<(MethodAdoptionRecord, bool)> {
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
    let (rejected, _) = f.create(&changed).unwrap();
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
