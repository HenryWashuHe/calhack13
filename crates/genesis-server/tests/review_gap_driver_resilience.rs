//! Henry's Claude: pressure test for the gap driver worker (#66).
//! One transient storage failure stops `capability_gap_driver::spawn` for
//! good: after the fault clears, a committed gap is never taken in, and the
//! operator status still reports the driver as enabled and ready.
//! Loopback/offline only; no provider calls.
use chrono::Utc;
use genesis_core::{
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap},
    ids::*,
    model::{CapabilityGap, CapabilityGapStatus},
};
use genesis_forge::{ModelConfig, PublicExample};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    capability_gap_driver::{self, GapDriverState},
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    store::EventMeta,
    AcquisitionConfig, OrgConfig,
};
use genesis_verification::{decode_json, OracleSuite};
use serde_json::json;
use std::{sync::Arc, time::Duration};

const MODEL: &str = "claude-sonnet-4-6";
const PUBLIC_HTML: &str = "<html>FIXED_PUBLIC_GAP_EXAMPLE</html>";

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
fn policy() -> GapAcquisitionPolicy {
    let suite = suite();
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: "venue-reader".into(),
            suite_id: suite.suite_id.clone(),
            actor_name_prefix: "gap-reader".into(),
            allowed_source_urls: suite
                .cases
                .iter()
                .map(|case| case.source.url.clone())
                .collect(),
            examples: vec![PublicExample {
                url: suite.cases[0].source.url.clone(),
                html: PUBLIC_HTML.into(),
            }],
            repair: false,
        }],
    }
}
fn acquisition() -> AcquisitionConfig {
    AcquisitionConfig::offline(AcquisitionPolicy {
        model: ModelConfig {
            model: MODEL.into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 3600,
    })
}

struct Fixture {
    _dir: tempfile::TempDir,
    config: OrgConfig,
    app: AppState,
    gap_id: CapabilityGapId,
}
impl Fixture {
    async fn new(org: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev(org, "Gap driver fixture");
        config.db = Some(dir.path().join("org.sqlite"));
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        config.oracle_suites = vec![suite()];
        config.acquisition = Some(acquisition());
        let mut app = build_org(&config, None).unwrap();
        app.gap_acquisition_policy = Some(Arc::new(policy()));
        let spec: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Find a venue", "roles":[{"key":"lead","name":"Lead","charter":"Own the missing parser",
                "authority":{"scopes":["capability.acquire"],"budget_cents":0}}],
            "responsibilities":[{"purpose":"Coordinate","owner":"lead"}]
        })).unwrap();
        let mission = app
            .mutate(EventMeta::default(), |state| {
                engine::bootstrap_mission(state, &spec, Utc::now()).map_err(validation_error)
            })
            .await
            .unwrap()
            .value;
        let observation = app
            .mutate(EventMeta::default(), |state| {
                engine::record_observation(
                    state,
                    &mission,
                    "public portal",
                    "New venue layout",
                    Some("https://fixtures.genesis.example/venues/index.html".into()),
                    Utc::now(),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap()
            .value;
        let requirement = app
            .mutate(EventMeta::default(), |state| {
                engine::record_requirement(
                    state,
                    &mission,
                    "Read public venue listings",
                    std::slice::from_ref(&observation),
                    "Pass independent extraction checks",
                )
                .map_err(Into::into)
            })
            .await
            .unwrap()
            .value;
        let state = app.current_state().unwrap();
        let role = state.roles.keys().next().unwrap().clone();
        let proposal: OrgChangeProposal = serde_json::from_value(json!({
            "expected_org_revision":state.revision, "trigger_evidence_ids":[observation],
            "rationale":"The requirement needs a parser", "budget_limit_cents":0,
            "operations":[
                {"op":"AssignResponsibility", "target":{"kind":"from_requirement","key":"reader","requirement_id":requirement,
                    "purpose":"Read venue listings","trigger":"Layout changes","termination":"Verified parser available"},
                    "owner":{"kind":"existing","role_id":role}},
                {"op":"RequestCapability", "key":"parser", "requirement_id":requirement,
                    "role":{"kind":"existing","role_id":role}, "responsibility":{"kind":"new","key":"reader"},
                    "capability_id":"venue-reader", "purpose":"Read venue listings", "observed_sources":policy().rules[0].allowed_source_urls}
            ]
        })).unwrap();
        let allocation = KeyAllocation::allocate(&proposal);
        let gap_id = allocation.gaps["parser"].clone();
        app.mutate(EventMeta::default(), |state| {
            engine::commit(
                state,
                &proposal,
                &allocation,
                &Default::default(),
                &engine::local_catalog(state),
                Utc::now(),
            )
            .map(|outcome| (outcome, ()))
            .map_err(validation_error)
        })
        .await
        .unwrap();
        Self {
            _dir: dir,
            config,
            app,
            gap_id,
        }
    }
    fn gap(&self) -> CapabilityGap {
        self.app.current_state().unwrap().capability_gaps[&self.gap_id].clone()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.config.db.as_ref().unwrap()).unwrap()
    }
}

#[tokio::test]
async fn one_transient_storage_failure_stops_gap_intake_until_restart() {
    let f = Fixture::new("org_a").await;
    f.app.store.init_gap_driver().unwrap();
    // Transient fault: progress writes fail (as on a full disk).
    f.sql()
        .execute_batch(
            "CREATE TRIGGER transient_fault BEFORE INSERT ON capability_gap_progress
             BEGIN SELECT RAISE(ABORT, 'injected transient storage failure'); END;",
        )
        .unwrap();
    let worker = capability_gap_driver::spawn(f.app.clone());
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // The fault clears.
    f.sql()
        .execute_batch("DROP TRIGGER transient_fault")
        .unwrap();
    tokio::time::sleep(Duration::from_millis(3500)).await;

    let status = capability_gap_driver::status(&f.app, &f.gap_id).unwrap();
    eprintln!(
        "worker finished: {}, gap: {:?}, status: {status:?}",
        worker.is_finished(),
        f.gap().status
    );
    // Desired: intake resumes once storage works again (as the outbox does).
    assert!(
        matches!(f.gap().status, CapabilityGapStatus::Acquiring { .. }),
        "gap intake never resumed after the fault cleared (worker finished: {}); \
         operator status still says enabled={} state={:?}",
        worker.is_finished(),
        status.enabled,
        status.state
    );
    assert_ne!(status.state, GapDriverState::Ready);
}
