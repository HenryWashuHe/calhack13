//! Henry's Claude: gap finalization (`acquired` → `installed`) with real kernel
//! commits, SQLite, a real registry server and real Rust Apify/model clients
//! against a loopback provider. No generated source runs locally; no live
//! provider is contacted.
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    Json, Router,
};
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::{
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap},
    ids::*,
    lifecycle::InstallationStatus,
    model::{CapabilityGap, CapabilityGapStatus},
};
use genesis_forge::{AcquisitionPhase, AnthropicGenerator, ModelConfig, PublicExample};
use genesis_providers::{ApifyCapabilityExecutor, ProviderSet};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    capability_gap_driver,
    capability_gap_finalizer::{
        self, FinalizerCode, FinalizerState, GapFinalizationPolicy, GapFinalizationRule, Step,
    },
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    parse_publishers,
    registry::RegistryState,
    spawn_registry,
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
const KEY: &str = "gap-finalizer-loopback-key";
const PUBLIC_HTML: &str = "<html>FIXED_PUBLIC_GAP_EXAMPLE</html>";
const ORG: &str = "org-fin";
const PERMISSION: &str = "capability.venue_lookup";

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

#[derive(Default)]
struct World {
    unknown_actor: bool,
    ambiguous_runs: bool,
    wrong_outputs: bool,
    fail_build_read: bool,
    read_failed: bool,
    requests: Vec<(String, String)>,
    actors: BTreeMap<String, Value>,
    runs: BTreeMap<String, (String, Value)>,
}
impl World {
    fn build(&self, actor: &str) -> Value {
        let source = &self.actors[actor]["versions"][0];
        let version = source["versionNumber"].as_str().unwrap();
        json!({"data":{"id":format!("build-{actor}"),"actId":actor,"buildNumber":format!("{version}.1"),"status":"SUCCEEDED","actVersion":source}})
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
            ("POST", ["v2", "actors", _, "runs"]) if self.ambiguous_runs => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"PRIVATE_AMBIGUOUS_RUN_RESULT"}),
            ),
            ("POST", ["v2", "actors", actor, "runs"]) => {
                let suite = suite();
                let case = suite
                    .cases
                    .iter()
                    .position(|case| case.input == body)
                    .unwrap();
                let receipts: Vec<ExecutionEvidence> = if self.wrong_outputs {
                    decode_json(include_bytes!(
                        "../../genesis-verification/examples/fixtures/runs.rejected.json"
                    ))
                    .unwrap()
                } else {
                    decode_json(include_bytes!(
                        "../../genesis-verification/examples/fixtures/runs.json"
                    ))
                    .unwrap()
                };
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
    url: String,
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
            url,
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

fn finalization() -> GapFinalizationPolicy {
    GapFinalizationPolicy {
        rules: vec![GapFinalizationRule {
            capability_id: CapabilityId::new("venue-reader"),
            suite_id: suite().suite_id,
            intent: "Read venue capacity, price and availability from the public portal".into(),
            applicability_conditions: vec!["Only the configured oracle scope".into()],
            known_limitations: vec!["Lookup only; no reservation".into()],
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object"}),
            required_permissions: [PERMISSION.to_owned()].into(),
            responsibility_template: Some("Keep verified venue lookup available".into()),
            workflow_procedure: None,
            local_configuration_schema: json!({}),
        }],
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    config: OrgConfig,
    app: AppState,
    gap_id: CapabilityGapId,
    providers: Arc<Providers>,
}

fn provider_set(p: &Providers) -> ProviderSet {
    let client = ApifyClient::for_local_testing(
        KEY,
        RunLimits {
            request_timeout: Duration::from_secs(5),
            deadline: Duration::from_secs(8),
            poll_interval: Duration::from_millis(1),
            ..RunLimits::default()
        },
        &p.url,
    )
    .unwrap();
    ProviderSet::new().with_executor(Arc::new(ApifyCapabilityExecutor::new(client)))
}

impl Fixture {
    async fn new(scopes: &[&str]) -> Self {
        let providers = Arc::new(Providers::start(false).await);
        let registry = spawn_registry(
            RegistryState::open_in_memory(parse_publishers(&format!("{ORG}:fin-registry")), vec![])
                .unwrap(),
            "127.0.0.1:0",
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev(ORG, "Finalizer fixture");
        config.db = Some(dir.path().join("org.sqlite"));
        config.dev_stub = false;
        config.registry_url = Some(format!("http://{registry}"));
        config.registry_token = Some("fin-registry".into());
        config.oracle_suites = vec![suite()];
        let mut acq = acquisition();
        acq.model = Some(providers.model.clone());
        acq.apify = Some(providers.apify.clone());
        acq.automatic = true;
        config.acquisition = Some(acq);
        config.gap_acquisition_policy = Some(policy());
        config.gap_finalization_policy = Some(finalization());
        let app = build_org(&config, Some(provider_set(&providers))).unwrap();
        let spec: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Find a venue", "roles":[{"key":"lead","name":"Lead","charter":"Own the missing parser",
                "authority":{"scopes":scopes,"budget_cents":0}}],
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
            providers,
        }
    }

    fn gap(&self) -> CapabilityGap {
        self.app.current_state().unwrap().capability_gaps[&self.gap_id].clone()
    }

    fn reopen(&mut self) {
        self.app = build_org(&self.config, Some(provider_set(&self.providers))).unwrap();
    }

    /// Drive the gap driver and acquisition runtime to `acquired`.
    async fn acquire(&self) {
        capability_gap_driver::drain_once(&self.app).await.unwrap();
        let id = self.app.store.acquisition_ids().unwrap()[0].clone();
        for _ in 0..40 {
            let record = self.app.acquisition_runtime.status(&id).unwrap();
            if record.session().phase() == AcquisitionPhase::Verified {
                break;
            }
            self.app.acquisition_runtime.step(&id, false).await.unwrap();
        }
        capability_gap_driver::drain_once(&self.app).await.unwrap();
        assert!(
            matches!(self.gap().status, CapabilityGapStatus::Acquired { .. }),
            "{:?}",
            self.gap().status
        );
    }

    async fn finalize_step(&self) -> Step {
        capability_gap_finalizer::step(&self.app, &self.gap_id)
            .await
            .unwrap()
    }

    /// Run the local check jobs the finalizer queued, as the capability worker would.
    async fn run_checks(&self) {
        let record = self
            .app
            .store
            .gap_finalization(&self.gap_id)
            .unwrap()
            .unwrap();
        for job in &record.job_ids {
            for _ in 0..10 {
                let run = self.app.store.get_capability_run(job).unwrap();
                if run.phase == genesis_server::capability_runtime::CapabilityRunPhase::Collected {
                    break;
                }
                self.app.capability_runtime.step(job, false).await.unwrap();
            }
        }
    }

    /// Finalize to a terminal or attention state, running checks when asked.
    async fn finish(&self) -> Step {
        for _ in 0..20 {
            match self.finalize_step().await {
                Step::Checking | Step::Waiting => self.run_checks().await,
                s @ (Step::Installed | Step::Refused | Step::Attention(_)) => return s,
                _ => {}
            }
        }
        panic!("finalization did not settle");
    }
}

#[tokio::test]
async fn a_verified_gap_is_published_installed_accepted_and_recorded_installed() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    let runs_before = f.providers.count("POST", "/runs");
    assert_eq!(f.finish().await, Step::Installed);

    let state = f.app.current_state().unwrap();
    let CapabilityGapStatus::Installed {
        capability,
        installation_id,
        acquisition_id,
    } = f.gap().status
    else {
        panic!("{:?}", f.gap().status)
    };
    let installation = &state.installations[&installation_id];
    assert_eq!(installation.status, InstallationStatus::Accepted);
    assert_eq!(installation.capability, capability);
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert_eq!(record.state, FinalizerState::Installed);
    assert_eq!(record.acquisition_id, acquisition_id);
    // The exact acquired pin was published, installed and checked.
    let pinned = f.app.store.capability_manifest(&installation_id).unwrap();
    assert_eq!(pinned.version, record.artifact.capability_version);
    assert_eq!(pinned.artifact.actor_id, record.artifact.actor_id);
    assert_eq!(pinned.artifact.build_id, record.artifact.build_id);
    assert_eq!(pinned.publisher.as_str(), ORG);
    assert!(
        pinned.verification.passed,
        "publication carries the recomputed proof"
    );
    assert_eq!(
        record.publication_ref.as_deref(),
        Some(
            format!(
                "registry:{}@{}",
                capability.capability_id, capability.version
            )
            .as_str()
        )
    );
    // One local check run per suite case, no new Actor, build or model call.
    assert_eq!(
        f.providers.count("POST", "/runs") - runs_before,
        suite().cases.len()
    );
    assert_eq!(f.providers.count("POST", "/actors"), 1);
    assert_eq!(f.providers.count("POST", "/messages"), 1);
    // The kernel's note wakes the planner exactly once.
    let notes = state
        .observations
        .values()
        .filter(|o| o.source == engine::CAPABILITY_GAP_SOURCE)
        .count();
    assert_eq!(notes, 1);
    // Settled: further passes change nothing.
    let rev = state.revision;
    assert_eq!(f.finalize_step().await, Step::Installed);
    assert_eq!(f.app.current_state().unwrap().revision, rev);
}

#[tokio::test]
async fn every_stage_recovers_after_restart_without_repeating_effects() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    let mut seen = vec![];
    for _ in 0..20 {
        // Reopen between every stage: only durable state carries over.
        f.reopen();
        let step = f.finalize_step().await;
        seen.push(step.clone());
        match step {
            Step::Checking | Step::Waiting => {
                f.reopen();
                f.run_checks().await;
            }
            Step::Installed => break,
            Step::Refused | Step::Attention(_) => panic!("{seen:?}"),
            _ => {}
        }
    }
    eprintln!("stages: {seen:?}");
    assert_eq!(seen.last(), Some(&Step::Installed), "{seen:?}");
    let state = f.app.current_state().unwrap();
    assert_eq!(
        state.installations.len(),
        1,
        "one installation across restarts"
    );
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert_eq!(record.job_ids.len(), suite().cases.len());
}

#[tokio::test]
async fn concurrent_steps_keep_one_record_one_installation_and_one_batch() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    for _ in 0..8 {
        let (a, b) = tokio::join!(
            capability_gap_finalizer::step(&f.app, &f.gap_id),
            capability_gap_finalizer::step(&f.app, &f.gap_id)
        );
        // A lost compare-and-swap is reported as a conflict, never a second child.
        for r in [a, b] {
            if let Err(e) = r {
                assert!(e.to_string().contains("revision conflict"), "{e}");
            }
        }
        if let Some(r) = f.app.store.gap_finalization(&f.gap_id).unwrap() {
            if !r.job_ids.is_empty() {
                f.run_checks().await;
            }
        }
    }
    assert_eq!(f.finish().await, Step::Installed);
    let state = f.app.current_state().unwrap();
    assert_eq!(state.installations.len(), 1);
    assert_eq!(
        f.providers.count("POST", "/runs"),
        3 + suite().cases.len(),
        "acquisition runs plus exactly one check batch"
    );
}

#[tokio::test]
async fn without_the_capability_permission_the_gap_never_installs() {
    // The role can acquire but cannot hold the manifest's permission.
    let f = Fixture::new(&["capability.acquire"]).await;
    f.acquire().await;
    let step = f.finish().await;
    assert_eq!(step, Step::Refused);
    let CapabilityGapStatus::Refused {
        reason,
        acquisition_id,
    } = f.gap().status
    else {
        panic!("{:?}", f.gap().status)
    };
    assert!(reason.contains("authority"), "{reason}");
    assert!(acquisition_id.is_some());
    let state = f.app.current_state().unwrap();
    assert!(state
        .installations
        .values()
        .all(|i| i.status != InstallationStatus::Accepted));
    let runs = f.providers.count("POST", "/runs");
    assert_eq!(runs, 3, "no check runs without authority");
}

#[tokio::test]
async fn a_changed_policy_or_suite_after_freezing_stops_at_attention() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    let mut changed = finalization();
    changed.rules[0].intent = "A different promise".into();
    f.config.gap_finalization_policy = Some(changed);
    f.reopen();
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Configuration)
    );
    // Attention is sticky; nothing was published.
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Configuration)
    );
    let CapabilityGapStatus::Acquired {
        publication_ref, ..
    } = f.gap().status
    else {
        panic!()
    };
    assert!(publication_ref.is_none());
    assert!(f.app.current_state().unwrap().installations.is_empty());

    // Operator-only status; the frozen manifest and suite are not exposed.
    let url = serve_url(&f).await;
    let http = reqwest::Client::new();
    let path = format!("{url}/api/v1/capability-gaps/{}/finalization", f.gap_id);
    let denied = http
        .get(&path)
        .bearer_auth(format!("{ORG}-agent"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 403);
    let status: Value = http
        .get(&path)
        .bearer_auth(format!("{ORG}-operator"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["state"], "attention");
    assert_eq!(status["code"], "configuration");
    let text = status.to_string();
    assert!(
        !text.contains("A different promise") && !text.contains("expected"),
        "{text}"
    );
    // Retry re-reads and stops again for the same reason; nothing is published.
    let retried: Value = http
        .post(format!("{path}/retry"))
        .bearer_auth(format!("{ORG}-operator"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(retried["state"], "active");
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Configuration)
    );
    assert!(f.app.current_state().unwrap().installations.is_empty());
}

#[tokio::test]
async fn an_unknown_check_run_outcome_stays_attention_with_ids_kept() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    // Every later run submission returns an ambiguous provider error.
    f.providers.state.lock().unwrap().ambiguous_runs = true;
    let mut last = Step::Idle;
    for _ in 0..12 {
        last = f.finalize_step().await;
        if matches!(last, Step::Checking | Step::Waiting) {
            let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
            for job in &record.job_ids {
                let _ = f.app.capability_runtime.step(job, false).await;
            }
        }
        if matches!(last, Step::Attention(_) | Step::Refused | Step::Installed) {
            break;
        }
    }
    assert_eq!(last, Step::Attention(FinalizerCode::Checks));
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert_eq!(record.job_ids.len(), suite().cases.len());
    assert!(record.installation_id.is_some());
    assert!(
        matches!(f.gap().status, CapabilityGapStatus::Acquired { .. }),
        "not refused"
    );
    let posts = f.providers.count("POST", "/runs");
    // Retrying the attention re-reads; it never resubmits an unknown run.
    let response = capability_gap_finalizer::step(&f.app, &f.gap_id)
        .await
        .unwrap();
    assert_eq!(response, Step::Attention(FinalizerCode::Checks));
    assert_eq!(f.providers.count("POST", "/runs"), posts);
}

#[tokio::test]
async fn a_semantic_rejection_refuses_the_gap_and_closes_the_installation() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    // The installed build now answers the local suite incorrectly.
    f.providers.state.lock().unwrap().wrong_outputs = true;
    assert_eq!(f.finish().await, Step::Refused);
    let CapabilityGapStatus::Refused { reason, .. } = f.gap().status else {
        panic!("{:?}", f.gap().status)
    };
    assert!(reason.contains("rejected"), "{reason}");
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    let state = f.app.current_state().unwrap();
    let installation = &state.installations[record.installation_id.as_ref().unwrap()];
    assert_eq!(installation.status, InstallationStatus::Rejected);
}

#[tokio::test]
async fn a_publication_whose_reply_was_lost_is_confirmed_not_duplicated() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    // The registry already holds this exact version (e.g. the reply was lost).
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    let body = json!({"manifest": record.manifest, "verification_ref": format!("acquisition:{}", record.acquisition_id)});
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/capabilities/publish",
            serve_url(&f).await
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status().as_u16(),
        201,
        "{}",
        response.text().await.unwrap()
    );
    assert_eq!(f.finalize_step().await, Step::Published);
    assert_eq!(f.finish().await, Step::Installed);
}

#[tokio::test]
async fn a_reassignment_after_installation_refuses_and_closes_it() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    assert_eq!(f.finalize_step().await, Step::Published);
    assert_eq!(f.finalize_step().await, Step::Installing);
    // The responsibility moves to a new epoch before checks are queued.
    let _guard = f.app.kernel.lock().await;
    let old = f.app.current_state().unwrap();
    let gap = old.capability_gaps[&f.gap_id].clone();
    let mut next = old.clone();
    next.responsibilities
        .get_mut(&gap.responsibility_id)
        .unwrap()
        .epoch += 1;
    for a in next.assignments.values_mut() {
        if a.responsibility_id == gap.responsibility_id {
            a.epoch += 1;
        }
    }
    next.revision += 1;
    f.app
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
    drop(_guard);
    assert_eq!(f.finalize_step().await, Step::Refused);
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    let state = f.app.current_state().unwrap();
    assert_eq!(
        state.installations[record.installation_id.as_ref().unwrap()].status,
        InstallationStatus::Rejected
    );
    assert!(record.job_ids.is_empty(), "no checks under the stale grant");
}

async fn serve_url(f: &Fixture) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = f.app.clone().router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

#[tokio::test]
async fn serve_org_takes_a_committed_gap_all_the_way_to_installed() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    assert_eq!(f.gap().status, CapabilityGapStatus::Open);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app = f.app.clone();
    let server =
        tokio::spawn(async move { genesis_server::serve_org(app, listener).await.unwrap() });
    // No manual drain, step, publish, install, check or acceptance call.
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            match f.gap().status {
                CapabilityGapStatus::Installed { .. } => break,
                CapabilityGapStatus::Refused { reason, .. } => panic!("refused: {reason}"),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await;
    server.abort();
    result.expect("serve_org workers did not install the gap");
    let state = f.app.current_state().unwrap();
    assert_eq!(state.installations.len(), 1);
    assert!(state
        .installations
        .values()
        .all(|i| i.status == InstallationStatus::Accepted));
    assert_eq!(f.providers.count("POST", "/messages"), 1);
    assert_eq!(f.providers.count("POST", "/actors"), 1);
    assert_eq!(f.providers.count("POST", "/runs"), 3 + suite().cases.len());
}

// Read-only PR review regressions. Requirement edits below deliberately use
// the existing kernel/store seam: there is currently no HTTP requirement-edit
// or satisfy endpoint. They model a committed requirement revision, not a
// caller-supplied proof or an invented public route.
async fn review_commit_requirement(
    f: &Fixture,
    change: impl FnOnce(&mut genesis_core::model::Requirement),
) {
    let _guard = f.app.kernel.lock().await;
    let before = f.app.current_state().unwrap();
    let gap = before.capability_gaps[&f.gap_id].clone();
    let mut after = before.clone();
    change(after.requirements.get_mut(&gap.requirement_id).unwrap());
    after.revision += 1;
    f.app
        .store
        .commit_state(
            before.revision,
            &after,
            vec![],
            &EventMeta::mission(gap.mission_id),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
}

#[tokio::test]
async fn review_changed_requirement_must_stop_before_local_installation() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    assert_eq!(f.finalize_step().await, Step::Published);
    let frozen = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    review_commit_requirement(&f, |requirement| {
        requirement.acceptance =
            "The old venue extraction task is superseded by a different acceptance contract".into();
    })
    .await;
    assert_eq!(f.gap().requirement_digest, frozen.requirement_digest);
    let effects_before = f.providers.count("POST", "/runs");
    let outcome = capability_gap_finalizer::step(&f.app, &f.gap_id).await;
    assert!(
        f.app.current_state().unwrap().installations.is_empty(),
        "a stale gap digest must not authorize installation after its live requirement changed: {outcome:?}"
    );
    assert_eq!(f.providers.count("POST", "/runs"), effects_before);
}

#[tokio::test]
async fn review_satisfied_requirement_must_fence_prepared_native_checks() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    for expected in [
        Step::Frozen,
        Step::Published,
        Step::Installing,
        Step::Checking,
    ] {
        assert_eq!(f.finalize_step().await, expected);
    }
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    review_commit_requirement(&f, |requirement| {
        requirement.status = genesis_core::lifecycle::RequirementStatus::Satisfied;
    })
    .await;
    // The separate capability worker can run before the finalizer's next tick.
    // Its native dispatch must still honor the finalizer's frozen work identity.
    let posts_before = f.providers.count("POST", "/runs");
    let result = f
        .app
        .capability_runtime
        .step(&record.job_ids[0], false)
        .await;
    assert_eq!(
        f.providers.count("POST", "/runs"),
        posts_before,
        "a prepared finalizer check dispatched after its requirement was satisfied: {result:?}"
    );
    assert!(!matches!(
        f.gap().status,
        CapabilityGapStatus::Installed { .. }
    ));
}

#[tokio::test]
async fn review_public_acceptance_then_delegation_must_not_finalize_a_revoked_grant() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    for expected in [
        Step::Frozen,
        Step::Published,
        Step::Installing,
        Step::Checking,
    ] {
        assert_eq!(f.finalize_step().await, expected);
    }
    f.run_checks().await;
    let frozen = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    let installation = frozen.installation_id.as_ref().unwrap();
    let url = serve_url(&f).await;
    let http = reqwest::Client::new();
    // This is the existing public acceptance endpoint, with genuine collected
    // evidence. It is currently not fenced as finalizer-owned installation.
    let accepted = http
        .post(format!(
            "{url}/api/v1/capability-installations/{installation}/acceptance"
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"suite_id":frozen.suite_id,"job_ids":frozen.job_ids}))
        .send()
        .await
        .unwrap();
    let accepted_status = accepted.status();
    let accepted_body = accepted.text().await.unwrap();
    // An ownership guard is also a safe fix: acceptance then must remain the
    // finalizer's atomic decision, not this ordinary endpoint's decision.
    if accepted_status == StatusCode::CONFLICT {
        assert_eq!(
            f.app.current_state().unwrap().installations[installation].status,
            InstallationStatus::Checking
        );
        return;
    }
    assert_eq!(accepted_status, StatusCode::OK, "{accepted_body}");
    assert_eq!(
        f.app.current_state().unwrap().installations[installation].status,
        InstallationStatus::Accepted
    );
    // Explicit delegation to the same role is a genuine public kernel change:
    // None -> Role advances the epoch and fences the old owner's assignment.
    let state = f.app.current_state().unwrap();
    let response = http
        .post(format!("{url}/api/v1/org-changes"))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({
            "client_request_id":"review-finalizer-public-delegation",
            "proposal":{
                "expected_org_revision":state.revision,
                "trigger_evidence_ids":f.gap().triggering_evidence,
                "rationale":"Make the accountable role the explicit executor under a fresh grant",
                "budget_limit_cents":0,
                "operations":[{"op":"DelegateTask",
                    "responsibility":{"kind":"existing","responsibility_id":frozen.responsibility_id},
                    "executor":{"kind":"role","role":{"kind":"existing","role_id":frozen.role_id}},
                    "task":"Continue work under the new explicit executor grant"}]
            }
        }))
        .send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
    let current = f.app.current_state().unwrap();
    let grant = current
        .active_assignment(&frozen.responsibility_id)
        .unwrap();
    assert_ne!(grant.assignment_id, frozen.assignment_id, "{body}");
    assert!(grant.epoch > frozen.epoch);
    let outcome = capability_gap_finalizer::step(&f.app, &f.gap_id).await;
    assert!(
        !matches!(f.gap().status, CapabilityGapStatus::Installed { .. }),
        "the Accepted shortcut finalized an old grant after real public delegation: {outcome:?}"
    );
}
