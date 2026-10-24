//! Durable gap-to-intent integration with real kernel commits and SQLite.
//! Provider execution is simulated on loopback; no generated source runs locally.
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    Json, Router,
};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::{
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap},
    ids::*,
    lifecycle::{AssignmentState, MissionStatus, RoleState},
    model::{CapabilityGap, CapabilityGapStatus, OrgState},
};
use genesis_forge::{AcquisitionPhase, AnthropicGenerator, ModelConfig, PublicExample};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    capability_gap_driver::drain_once,
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    store::EventMeta,
    AcquisitionConfig, OrgConfig,
};
use genesis_verification::{decode_json, ExecutionEvidence, OracleSuite};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

const MODEL: &str = "claude-sonnet-4-6";
const KEY: &str = "gap-driver-loopback-key";
const PUBLIC_HTML: &str = "<html>FIXED_PUBLIC_GAP_EXAMPLE</html>";

struct RunningServer(tokio::task::JoinHandle<()>);
impl Drop for RunningServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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
    async fn new(org: &str, providers: Option<&Providers>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev(org, "Gap driver fixture");
        config.db = Some(dir.path().join("org.sqlite"));
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        config.oracle_suites = vec![suite()];
        let mut acq = acquisition();
        if let Some(providers) = providers {
            acq.model = Some(providers.model.clone());
            acq.apify = Some(providers.apify.clone());
        }
        config.acquisition = Some(acq);
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
    fn ids(&self) -> Vec<String> {
        self.app.store.acquisition_ids().unwrap()
    }
    fn record(&self) -> genesis_server::capability_acquisition_store::AcquisitionRecord {
        assert_eq!(self.ids().len(), 1);
        self.app.acquisition_runtime.status(&self.ids()[0]).unwrap()
    }
    fn reopen(&mut self, policy: GapAcquisitionPolicy) {
        self.app = build_org(&self.config, None).unwrap();
        self.app.gap_acquisition_policy = Some(Arc::new(policy));
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.config.db.as_ref().unwrap()).unwrap()
    }
    async fn change(&self, edit: impl FnOnce(&mut OrgState, &CapabilityGap)) {
        let _guard = self.app.kernel.lock().await;
        let old = self.app.current_state().unwrap();
        let gap = old.capability_gaps[&self.gap_id].clone();
        let mut next = old.clone();
        edit(&mut next, &gap);
        next.revision += 1;
        self.app
            .store
            .commit_state(
                old.revision,
                &next,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
    async fn finish(&self) -> AcquisitionPhase {
        for _ in 0..40 {
            let record = self.record();
            let phase = record.session().phase();
            if matches!(
                phase,
                AcquisitionPhase::Verified
                    | AcquisitionPhase::Rejected
                    | AcquisitionPhase::Failed
                    | AcquisitionPhase::NeedsReconciliation
            ) {
                return phase;
            }
            self.app
                .acquisition_runtime
                .step(record.id(), false)
                .await
                .unwrap();
        }
        panic!("bounded fixture did not reach terminal acquisition state")
    }
}

#[tokio::test]
async fn disabled_driver_does_not_claim_a_genuine_committed_gap() {
    let mut f = Fixture::new("gap-disabled", None).await;
    f.app.gap_acquisition_policy = None;
    drain_once(&f.app).await.unwrap();
    assert_eq!(f.gap().status, CapabilityGapStatus::Open);
    assert!(f.ids().is_empty());
    assert!(f
        .app
        .store
        .gap_acquisition_binding(f.gap_id.as_str())
        .unwrap()
        .is_none());
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 0);
}

#[tokio::test]
async fn status_is_operator_only_and_omits_frozen_inputs() {
    let f = Fixture::new("gap-status", None).await;
    drain_once(&f.app).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let path = format!(
        "http://{}/api/v1/capability-gaps/{}/acquisition",
        listener.local_addr().unwrap(),
        f.gap_id
    );
    let router = f.app.clone().router();
    let _server = RunningServer(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .unwrap();
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(f.config.tokens.agent.as_str()), StatusCode::FORBIDDEN),
        (
            Some(f.config.tokens.adapter.as_str()),
            StatusCode::FORBIDDEN,
        ),
        (Some(f.config.tokens.operator.as_str()), StatusCode::OK),
    ] {
        let mut request = http.get(&path);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), expected);
        let bytes = response.bytes().await.unwrap();
        assert!(bytes.len() < 8192);
        let text = std::str::from_utf8(&bytes).unwrap();
        for private in [
            PUBLIC_HTML,
            MODEL,
            "Read public venue listings",
            "Pass independent extraction checks",
            "allowed_source_urls",
            "extractor_typescript",
        ] {
            assert!(
                !text.contains(private),
                "private acquisition content in status"
            );
        }
        if expected == StatusCode::OK {
            let status: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(status["gap_id"], f.gap_id.as_str());
            assert_eq!(status["acquisition_id"], f.record().id());
            assert_eq!(status["enabled"], true);
        }
    }
}

#[tokio::test]
async fn concurrent_drains_and_reopen_keep_one_intent_and_frozen_request() {
    let mut f = Fixture::new("Org A 東京", None).await;
    let (a, b) = tokio::join!(drain_once(&f.app), drain_once(&f.app));
    a.unwrap();
    b.unwrap();
    let record = f.record();
    assert_eq!(
        f.gap().status,
        CapabilityGapStatus::Acquiring {
            acquisition_id: record.id().into()
        }
    );
    assert!(f
        .app
        .store
        .gap_acquisition_binding(f.gap_id.as_str())
        .unwrap()
        .is_some());
    assert_eq!(record.binding().org_id.as_str(), "Org A 東京");
    assert_eq!(
        record.session().plan().request.examples[0].html,
        PUBLIC_HTML
    );
    let before = serde_json::to_value(&record).unwrap();
    let counts = f.app.store.acquisition_counters().unwrap();
    assert_eq!(counts.total_intents, 1);
    assert_eq!(counts.model_calls, 0);
    f.reopen(policy());
    for _ in 0..3 {
        drain_once(&f.app).await.unwrap();
    }
    assert_eq!(serde_json::to_value(f.record()).unwrap(), before);
    assert_eq!(f.app.store.acquisition_counters().unwrap(), counts);
}

#[tokio::test]
async fn reopening_with_changed_policy_and_model_cannot_reinterpret_existing_intent() {
    let mut f = Fixture::new("gap-policy-change", None).await;
    drain_once(&f.app).await.unwrap();
    let original = f.record();
    let mut changed = policy();
    changed.rules[0].examples[0].html = "<html>NEW_PUBLIC_EXAMPLE</html>".into();
    changed.rules[0].actor_name_prefix = "another-prefix".into();
    f.config.acquisition.as_mut().unwrap().policy.model.model = "another-explicit-model".into();
    f.reopen(changed);
    drain_once(&f.app).await.unwrap();
    let current = f.record();
    assert_eq!(current.id(), original.id());
    assert_eq!(current.session().plan(), original.session().plan());
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 1);
    assert_eq!(f.app.store.acquisition_counters().unwrap().model_calls, 0);
}

#[tokio::test]
async fn failed_child_creation_keeps_the_frozen_request_across_policy_change_and_restart() {
    let mut f = Fixture::new("gap-create-failure", None).await;
    f.sql().execute_batch("CREATE TRIGGER fail_gap_child BEFORE INSERT ON acquisition_history WHEN NEW.event_kind='created' BEGIN SELECT RAISE(ABORT,'PRIVATE_GAP_CREATE_FAILURE'); END;").unwrap();
    assert!(drain_once(&f.app).await.is_err());
    assert!(f.ids().is_empty());
    assert_eq!(f.gap().status, CapabilityGapStatus::Open);
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 0);
    let frozen = f
        .app
        .store
        .gap_acquisition_binding(f.gap_id.as_str())
        .unwrap()
        .unwrap();
    assert!(frozen.acquisition_id().is_none());
    let request = frozen.request().clone();
    f.sql()
        .execute_batch("DROP TRIGGER fail_gap_child;")
        .unwrap();
    let mut changed = policy();
    changed.rules[0].examples[0].html =
        "<html>replacement must not enter original intent</html>".into();
    changed.rules[0].actor_name_prefix = "new-policy".into();
    f.config.acquisition.as_mut().unwrap().policy.model.model = "new-policy-model".into();
    f.reopen(changed);
    drain_once(&f.app).await.unwrap();
    let record = f.record();
    assert_eq!(record.session().plan().request.examples, request.examples);
    assert_eq!(record.session().plan().request.candidate, request.candidate);
    assert_eq!(record.session().plan().model.model, MODEL);
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 1);
    assert_eq!(f.app.store.acquisition_counters().unwrap().model_calls, 0);
}

#[tokio::test]
async fn failed_gap_transition_recovers_the_already_committed_child_exactly_once() {
    let mut f = Fixture::new("gap-transition-failure", None).await;
    f.sql().execute_batch("CREATE TRIGGER fail_gap_projection BEFORE INSERT ON org_snapshots WHEN NEW.state_json LIKE '%\"state\":\"acquiring\"%' BEGIN SELECT RAISE(ABORT,'PRIVATE_GAP_PROJECTION_FAILURE'); END;").unwrap();
    assert!(drain_once(&f.app).await.is_err());
    let child = f.record();
    assert_eq!(f.gap().status, CapabilityGapStatus::Open);
    assert_eq!(
        f.app
            .store
            .gap_acquisition_binding(f.gap_id.as_str())
            .unwrap()
            .unwrap()
            .acquisition_id(),
        Some(child.id())
    );
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 1);
    f.sql()
        .execute_batch("DROP TRIGGER fail_gap_projection;")
        .unwrap();
    f.reopen(policy());
    let (a, b) = tokio::join!(drain_once(&f.app), drain_once(&f.app));
    a.unwrap();
    b.unwrap();
    assert_eq!(f.ids(), vec![child.id().to_owned()]);
    assert_eq!(
        f.gap().status,
        CapabilityGapStatus::Acquiring {
            acquisition_id: child.id().into()
        }
    );
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 1);
}

#[tokio::test]
async fn missing_or_changed_suite_keeps_a_frozen_binding_in_attention_without_a_child() {
    for missing in [false, true] {
        let mut f = Fixture::new(
            if missing {
                "gap-missing-suite"
            } else {
                "gap-changed-suite"
            },
            None,
        )
        .await;
        f.sql().execute_batch("CREATE TRIGGER fail_gap_child BEFORE INSERT ON acquisition_history WHEN NEW.event_kind='created' BEGIN SELECT RAISE(ABORT,'PRIVATE_SUITE_TEST_STOP'); END;").unwrap();
        assert!(drain_once(&f.app).await.is_err());
        let frozen = f
            .app
            .store
            .gap_acquisition_binding(f.gap_id.as_str())
            .unwrap()
            .unwrap();
        f.sql()
            .execute_batch("DROP TRIGGER fail_gap_child;")
            .unwrap();
        if missing {
            f.config.oracle_suites.clear();
        } else {
            f.config.oracle_suites[0]
                .scope
                .push_str(" changed backend contract");
        }
        f.reopen(policy());
        for _ in 0..2 {
            drain_once(&f.app).await.unwrap();
        }
        let status = genesis_server::capability_gap_driver::status(&f.app, &f.gap_id).unwrap();
        assert_eq!(serde_json::to_value(&status).unwrap()["state"], "attention");
        assert_eq!(
            serde_json::to_value(status).unwrap()["code"],
            "configuration"
        );
        assert!(f.ids().is_empty());
        assert_eq!(f.gap().status, CapabilityGapStatus::Open);
        assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 0);
        let restored = f
            .app
            .store
            .gap_acquisition_binding(f.gap_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(restored.request(), frozen.request());
        assert!(restored.acquisition_id().is_none());
    }
}

#[tokio::test]
async fn identical_gap_ids_in_independent_organizations_get_distinct_candidates() {
    let a = Fixture::new("gap-org-a", None).await;
    let mut b = Fixture::new("gap-org-b", None).await;
    let same_work = a.app.current_state().unwrap();
    // Trusted fixture: identical committed work identifiers in distinct stores.
    // No acquisition checkpoint or binding is copied between organizations.
    b.change(|state, _| {
        let org = state.org_id.clone();
        let revision = state.revision;
        *state = same_work;
        state.org_id = org;
        state.revision = revision;
    })
    .await;
    b.gap_id = a.gap_id.clone();
    drain_once(&a.app).await.unwrap();
    drain_once(&b.app).await.unwrap();
    let ac = a.record();
    let bc = b.record();
    let ar = &ac.session().plan().request.candidate;
    let br = &bc.session().plan().request.candidate;
    assert_ne!(ar.actor_name, br.actor_name);
    assert_ne!(ar.capability_version, br.capability_version);
    assert_ne!(ac.id(), bc.id());
    assert_eq!(a.app.store.acquisition_counters().unwrap().total_intents, 1);
    assert_eq!(b.app.store.acquisition_counters().unwrap().total_intents, 1);
}

#[tokio::test]
async fn closed_or_expired_mission_and_changed_requirement_never_create_intents() {
    for variant in 0..3 {
        let f = Fixture::new(&format!("gap-stale-{variant}"), None).await;
        f.change(|state, gap| match variant {
            0 => state.missions.get_mut(&gap.mission_id).unwrap().status = MissionStatus::Failed,
            1 => {
                state.missions.get_mut(&gap.mission_id).unwrap().deadline =
                    Some(Utc::now() - ChronoDuration::seconds(1))
            }
            _ => state
                .requirements
                .get_mut(&gap.requirement_id)
                .unwrap()
                .acceptance
                .push_str(" altered after gap commit"),
        })
        .await;
        drain_once(&f.app).await.unwrap();
        assert!(f.ids().is_empty());
        assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 0);
        assert!(!matches!(
            f.gap().status,
            CapabilityGapStatus::Acquiring { .. } | CapabilityGapStatus::Acquired { .. }
        ));
    }
}

#[tokio::test]
async fn a_temporarily_unprovisioned_role_waits_then_creates_once() {
    let f = Fixture::new("gap-provisioning", None).await;
    // Trusted lifecycle fixture setup; no claim of native provider provisioning.
    f.change(|state, gap| {
        state.roles.get_mut(&gap.role_id).unwrap().state = RoleState::Provisioning
    })
    .await;
    drain_once(&f.app).await.unwrap();
    assert!(f.ids().is_empty());
    assert_eq!(f.gap().status, CapabilityGapStatus::Open);
    f.change(|state, gap| state.roles.get_mut(&gap.role_id).unwrap().state = RoleState::Active)
        .await;
    drain_once(&f.app).await.unwrap();
    drain_once(&f.app).await.unwrap();
    assert_eq!(f.ids().len(), 1);
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 1);
}

#[tokio::test]
async fn reassignment_does_not_rebind_or_replace_an_existing_acquisition() {
    let f = Fixture::new("gap-reassigned", None).await;
    drain_once(&f.app).await.unwrap();
    let original = f.record();
    f.change(|state, gap| {
        let old = state
            .active_assignment(&gap.responsibility_id)
            .unwrap()
            .clone();
        state.assignments.get_mut(&old.assignment_id).unwrap().state = AssignmentState::Fenced;
        let mut replacement = old;
        replacement.assignment_id = AssignmentId::generate();
        replacement.epoch += 1;
        state
            .responsibilities
            .get_mut(&gap.responsibility_id)
            .unwrap()
            .epoch = replacement.epoch;
        state
            .assignments
            .insert(replacement.assignment_id.clone(), replacement);
    })
    .await;
    drain_once(&f.app).await.unwrap();
    f.app
        .acquisition_runtime
        .reconcile_if_current(original.id(), original.version())
        .await
        .unwrap();
    drain_once(&f.app).await.unwrap();
    assert_eq!(f.record().id(), original.id());
    assert!(f.record().is_fenced());
    assert_eq!(f.app.store.acquisition_counters().unwrap().total_intents, 1);
    assert_eq!(f.app.store.acquisition_counters().unwrap().model_calls, 0);
}

#[derive(Default)]
struct World {
    unknown_actor: bool,
    fail_build_read: bool,
    read_failed: bool,
    requests: Vec<(String, String)>,
    actors: BTreeMap<String, Value>,
    runs: BTreeMap<String, (String, Value)>,
}
impl World {
    fn build(&self, actor: &str) -> Value {
        json!({"data":{"id":format!("build-{actor}"),"actId":actor,"buildNumber":"0.1.1","status":"SUCCEEDED","actVersion":self.actors[actor]["versions"][0]}})
    }
    fn run(&self, run: &str) -> Value {
        let actor = &self.runs[run].0;
        json!({"data":{"id":run,"actId":actor,"buildId":format!("build-{actor}"),"status":"SUCCEEDED","defaultDatasetId":format!("dataset-{run}")}})
    }
    fn reply(&mut self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.requests.push((method.into(), path.into()));
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "messages"]) => (
                StatusCode::OK,
                json!({"id":"msg_gap_fixture","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":json!({"extractor_typescript":"export function extract() { throw new Error('loopback fixture, never executed locally'); }"}).to_string()}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":120,"output_tokens":80}}),
            ),
            ("POST", ["v2", "actors"]) if self.unknown_actor => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"PRIVATE_AMBIGUOUS_ACTOR_RESULT"}),
            ),
            ("POST", ["v2", "actors"]) => {
                let id = format!("actor-gap-{}", self.actors.len());
                let mut response = body.clone();
                response["id"] = json!(id);
                self.actors.insert(id, body);
                (StatusCode::CREATED, json!({"data":response}))
            }
            ("GET", ["v2", "actors", actor, "versions", version]) => {
                let source = &self.actors[*actor]["versions"][0];
                assert_eq!(source["versionNumber"], *version);
                (StatusCode::OK, json!({"data":source}))
            }
            ("POST", ["v2", "actors", actor, "builds"]) => (StatusCode::CREATED, self.build(actor)),
            ("GET", ["v2", "actor-builds", _]) if self.fail_build_read && !self.read_failed => {
                self.read_failed = true;
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error":"PRIVATE_FAILED_READ"}),
                )
            }
            ("GET", ["v2", "actor-builds", build]) => (
                StatusCode::OK,
                self.build(build.strip_prefix("build-").unwrap()),
            ),
            ("POST", ["v2", "actors", actor, "runs"]) => {
                let suite = suite();
                let case = suite
                    .cases
                    .iter()
                    .position(|case| case.input == body)
                    .unwrap();
                let receipts: Vec<ExecutionEvidence> = decode_json(include_bytes!(
                    "../../genesis-verification/examples/fixtures/runs.json"
                ))
                .unwrap();
                let id = format!("run-gap-{}", self.runs.len());
                self.runs
                    .insert(id.clone(), ((*actor).into(), receipts[case].output.clone()));
                (StatusCode::CREATED, self.run(&id))
            }
            ("GET", ["v2", "actor-runs", run]) => (StatusCode::OK, self.run(run)),
            ("GET", ["v2", "datasets", dataset, "items"]) => (
                StatusCode::OK,
                json!([self.runs[dataset.strip_prefix("dataset-").unwrap()].1]),
            ),
            _ => panic!("unexpected loopback route {method} {path}"),
        }
    }
}
struct Providers {
    state: Arc<Mutex<World>>,
    model: Arc<AnthropicGenerator>,
    apify: Arc<ApifyClient>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Providers {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Providers {
    async fn start(unknown_actor: bool) -> Self {
        let state = Arc::new(Mutex::new(World {
            unknown_actor,
            ..World::default()
        }));
        let router = Router::new()
            .fallback(
                |State(world): State<Arc<Mutex<World>>>, request: Request| async move {
                    let method = request.method().to_string();
                    let path = request.uri().path().to_owned();
                    if path == "/v1/messages" {
                        assert_eq!(request.headers()["x-api-key"], KEY);
                    } else {
                        assert_eq!(request.headers()["authorization"], format!("Bearer {KEY}"));
                    }
                    let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
                        .await
                        .unwrap();
                    let body = if bytes.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes).unwrap()
                    };
                    let (status, body) = world.lock().unwrap().reply(&method, &path, body);
                    (status, Json(body))
                },
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let model = Arc::new(
            AnthropicGenerator::with_test_endpoint(&format!("{url}/v1/messages"), KEY).unwrap(),
        );
        let apify = Arc::new(
            ApifyClient::for_local_testing(
                KEY,
                RunLimits {
                    request_timeout: Duration::from_secs(5),
                    deadline: Duration::from_secs(8),
                    poll_interval: Duration::from_millis(1),
                    ..RunLimits::default()
                },
                &url,
            )
            .unwrap(),
        );
        Self {
            state,
            model,
            apify,
            task,
        }
    }
    fn count(&self, method: &str, suffix: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(m, p)| m == method && p.ends_with(suffix))
            .count()
    }
}

#[tokio::test]
async fn verified_acquisition_updates_the_gap_without_publishing_or_installing() {
    let providers = Providers::start(false).await;
    let mut f = Fixture::new("gap-verified", Some(&providers)).await;
    drain_once(&f.app).await.unwrap();
    assert_eq!(f.finish().await, AcquisitionPhase::Verified);
    let verified = f
        .app
        .acquisition_runtime
        .verified(f.record().id())
        .await
        .unwrap();
    drain_once(&f.app).await.unwrap();
    let expected = CapabilityGapStatus::Acquired {
        acquisition_id: f.record().id().into(),
        capability: genesis_core::model::CapabilityRef {
            capability_id: CapabilityId::new(verified.artifact.capability_id.clone()),
            version: verified.artifact.capability_version.clone(),
        },
        publication_ref: None,
    };
    assert_eq!(f.gap().status, expected);
    assert!(f.app.current_state().unwrap().installations.is_empty());
    assert_eq!(providers.count("POST", "/actors"), 1);
    assert_eq!(providers.count("POST", "/runs"), 3);
    let calls = providers.state.lock().unwrap().requests.len();
    f.reopen(policy());
    drain_once(&f.app).await.unwrap();
    assert_eq!(f.gap().status, expected);
    assert_eq!(providers.state.lock().unwrap().requests.len(), calls);
}

#[tokio::test]
async fn serve_org_drives_a_committed_gap_to_acquired_without_manual_steps() {
    let providers = Providers::start(false).await;
    let mut f = Fixture::new("gap-startup", Some(&providers)).await;
    assert_eq!(f.gap().status, CapabilityGapStatus::Open);
    assert!(f.ids().is_empty());
    f.config.gap_acquisition_policy = Some(policy());
    f.config.acquisition.as_mut().unwrap().automatic = true;
    f.app = build_org(&f.config, None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app = f.app.clone();
    let mut server = RunningServer(tokio::spawn(async move {
        genesis_server::serve_org(app, listener).await.unwrap();
    }));
    // No gap drain, acquisition step or manual check endpoint is called here.
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if matches!(f.gap().status, CapabilityGapStatus::Acquired { .. }) {
                break;
            }
            assert!(
                !matches!(f.gap().status, CapabilityGapStatus::Refused { .. }),
                "automatic gap was refused"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("configured serve_org workers did not acquire the gap");
    assert_eq!(f.record().session().phase(), AcquisitionPhase::Verified);
    let CapabilityGapStatus::Acquired {
        publication_ref, ..
    } = f.gap().status
    else {
        unreachable!()
    };
    assert!(publication_ref.is_none());
    assert!(f.app.current_state().unwrap().installations.is_empty());
    assert_eq!(f.ids().len(), 1);
    assert_eq!(providers.count("POST", "/messages"), 1);
    assert_eq!(providers.count("POST", "/actors"), 1);
    assert_eq!(providers.count("POST", "/runs"), 3);
    server.0.abort();
    assert!((&mut server.0).await.unwrap_err().is_cancelled());
    let calls = providers.state.lock().unwrap().requests.clone();
    let counts = f.app.store.acquisition_counters().unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(providers.state.lock().unwrap().requests, calls);
    assert_eq!(f.app.store.acquisition_counters().unwrap(), counts);
}

#[tokio::test]
async fn unknown_actor_outcome_survives_restart_without_a_replacement_intent() {
    let providers = Providers::start(true).await;
    let mut f = Fixture::new("gap-unknown", Some(&providers)).await;
    drain_once(&f.app).await.unwrap();
    assert_eq!(f.finish().await, AcquisitionPhase::NeedsReconciliation);
    let id = f.record().id().to_owned();
    let counts = f.app.store.acquisition_counters().unwrap();
    let calls = providers.state.lock().unwrap().requests.len();
    drain_once(&f.app).await.unwrap();
    f.reopen(policy());
    f.app
        .acquisition_runtime
        .recover_interrupted()
        .await
        .unwrap();
    for _ in 0..3 {
        drain_once(&f.app).await.unwrap();
    }
    assert_eq!(f.ids(), vec![id.clone()]);
    assert_eq!(
        f.gap().status,
        CapabilityGapStatus::Acquiring { acquisition_id: id }
    );
    assert_eq!(
        f.record().session().phase(),
        AcquisitionPhase::NeedsReconciliation
    );
    assert_eq!(f.app.store.acquisition_counters().unwrap(), counts);
    assert_eq!(counts.actor_creates, 1);
    assert_eq!(providers.count("POST", "/actors"), 1);
    assert_eq!(providers.state.lock().unwrap().requests.len(), calls);
}

#[tokio::test]
async fn failed_provider_read_is_gap_attention_without_implicit_retry_after_restart() {
    let providers = Providers::start(false).await;
    providers.state.lock().unwrap().fail_build_read = true;
    let mut f = Fixture::new("gap-read-attention", Some(&providers)).await;
    drain_once(&f.app).await.unwrap();
    for _ in 0..12 {
        let record = f.record();
        f.app
            .acquisition_runtime
            .step(record.id(), false)
            .await
            .unwrap();
        if providers.state.lock().unwrap().read_failed {
            break;
        }
    }
    assert!(providers.state.lock().unwrap().read_failed);
    let original = f.record();
    assert!(matches!(
        f.app.acquisition_runtime.step(original.id(), false).await,
        Err(genesis_server::capability_acquisition_runtime::AcquisitionRuntimeError::RetryRequired)
    ));
    let calls = providers.state.lock().unwrap().requests.clone();
    let counts = f.app.store.acquisition_counters().unwrap();
    for _ in 0..3 {
        drain_once(&f.app).await.unwrap();
    }
    let status = genesis_server::capability_gap_driver::status(&f.app, &f.gap_id).unwrap();
    assert_eq!(serde_json::to_value(&status).unwrap()["state"], "attention");
    assert_eq!(serde_json::to_value(&status).unwrap()["code"], "retry_read");
    assert_eq!(status.acquisition_id.as_deref(), Some(original.id()));
    f.reopen(policy());
    drain_once(&f.app).await.unwrap();
    let restored = genesis_server::capability_gap_driver::status(&f.app, &f.gap_id).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap()["code"],
        "retry_read"
    );
    assert_eq!(f.record().id(), original.id());
    assert_eq!(f.record().version(), original.version());
    assert_eq!(f.app.store.acquisition_counters().unwrap(), counts);
    assert_eq!(providers.state.lock().unwrap().requests, calls);
    assert_eq!(
        f.gap().status,
        CapabilityGapStatus::Acquiring {
            acquisition_id: original.id().into()
        }
    );
}

#[tokio::test]
async fn paused_quota_and_configuration_schedules_are_visible_without_new_effects() {
    for quota in [false, true] {
        let providers = Providers::start(false).await;
        let mut f = Fixture::new(
            if quota {
                "gap-quota"
            } else {
                "gap-unconfigured"
            },
            quota.then_some(&providers),
        )
        .await;
        drain_once(&f.app).await.unwrap();
        let original = f.record();
        if quota {
            let limit = f
                .config
                .acquisition
                .as_ref()
                .unwrap()
                .policy
                .quotas
                .model_calls;
            f.sql()
                .execute(
                    "UPDATE acquisition_counters SET model_calls=?1 WHERE singleton=1",
                    [limit],
                )
                .unwrap();
        }
        // Exercise the actual scheduler's persistent failure classification.
        // No serve_org worker is spawned, so this is one deterministic pass.
        f.app.automatic_acquisition = true;
        genesis_server::capability_acquisition_driver::drain_once(&f.app)
            .await
            .unwrap();
        let scheduler =
            genesis_server::capability_acquisition_driver::status(&f.app, original.id()).unwrap();
        let expected = if quota { "quota" } else { "configuration" };
        assert_eq!(
            serde_json::to_value(scheduler).unwrap()["error_code"],
            expected
        );
        let counts = f.app.store.acquisition_counters().unwrap();
        drain_once(&f.app).await.unwrap();
        let status = genesis_server::capability_gap_driver::status(&f.app, &f.gap_id).unwrap();
        assert_eq!(serde_json::to_value(&status).unwrap()["state"], "attention");
        assert_eq!(serde_json::to_value(status).unwrap()["code"], expected);
        f.reopen(policy());
        f.app.automatic_acquisition = true;
        for _ in 0..3 {
            drain_once(&f.app).await.unwrap();
        }
        let restored = genesis_server::capability_gap_driver::status(&f.app, &f.gap_id).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap()["code"], expected);
        assert_eq!(f.record().id(), original.id());
        assert_eq!(f.record().version(), original.version());
        assert_eq!(f.app.store.acquisition_counters().unwrap(), counts);
        assert!(providers.state.lock().unwrap().requests.is_empty());
    }
}
