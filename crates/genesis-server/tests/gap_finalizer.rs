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
    capability::CapabilityManifest,
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap},
    ids::*,
    lifecycle::InstallationStatus,
    model::{CapabilityGap, CapabilityGapStatus, OrgState},
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
    fail_next_run_read: bool,
    run_response_barrier: Option<Arc<RunResponseBarrier>>,
    requests: Vec<(String, String)>,
    actors: BTreeMap<String, Value>,
    runs: BTreeMap<String, (String, Value)>,
}
#[derive(Default)]
struct RunResponseBarrier {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
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
            ("GET", ["v2", "actor-runs", _]) if self.fail_next_run_read => {
                self.fail_next_run_read = false;
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error":"TEMPORARY_READ_FAILURE"}),
                )
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
                    let ((status, body), barrier) = {
                        let mut world = world.lock().unwrap();
                        let response = world.reply(&method, &path, body);
                        let barrier = (method == "POST" && path.ends_with("/runs"))
                            .then(|| world.run_response_barrier.clone())
                            .flatten();
                        (response, barrier)
                    };
                    if let Some(barrier) = barrier {
                        barrier.entered.notify_one();
                        barrier.release.notified().await;
                    }
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
async fn a_conflicting_manifest_at_the_same_pin_does_not_confirm_publication() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    // A legitimate earlier publication shares the exact artifact and scopes,
    // but its operator-authored promise differs from this frozen intent.
    let mut earlier = record.manifest.clone();
    earlier.intent = "An earlier, different promise for this same parser".into();
    let url = format!("{}/api/v1/capabilities/publish", serve_url(&f).await);
    let client = reqwest::Client::new();
    let verification_ref = format!("acquisition:{}", record.acquisition_id);
    let published = client
        .post(&url)
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"manifest": earlier, "verification_ref": verification_ref}))
        .send()
        .await
        .unwrap();
    assert_eq!(published.status().as_u16(), 201);
    let published: Value = published.json().await.unwrap();
    assert_eq!(published["intent"], earlier.intent);
    assert_eq!(published["artifact"], json!(record.manifest.artifact));
    assert_eq!(published["source_digest"], record.manifest.source_digest);
    assert_eq!(published["publisher"], json!(record.manifest.publisher));
    assert_eq!(
        published["required_permissions"],
        json!(record.manifest.required_permissions)
    );
    assert_eq!(published["verification"]["passed"], true);

    // Confirm this is a definite immutable-manifest conflict, not a lost reply.
    let conflict = client
        .post(&url)
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"manifest": record.manifest, "verification_ref": verification_ref}))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status().as_u16(), 409);
    // The organization API deliberately redacts the registry error body.
    // Its HTTP conflict plus the independently checked stored intent suffice.

    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Publication),
        "a same-pin read must not replace the frozen publication contract"
    );
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert!(record.publication_ref.is_none());
    assert!(record.installation_id.is_none());
    let CapabilityGapStatus::Acquired {
        publication_ref, ..
    } = f.gap().status
    else {
        panic!("{:?}", f.gap().status)
    };
    assert!(publication_ref.is_none());
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

/// Review fixture: establish genuine local acceptance through the public API,
/// before the finalizer has selected an installation. The native run receipts
/// come from the real Rust Apify client; no accepted checkpoint is fabricated.
async fn review_accept_published_artifact(f: &Fixture, suite_id: &str) -> InstallationId {
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert!(record.publication_ref.is_some());
    assert!(record.installation_id.is_none());
    let base = serve_url(f).await;
    let http = reqwest::Client::new();
    let response = http
        .post(format!("{base}/api/v1/capability-installations"))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"capability_id":record.capability.capability_id,
            "version":record.capability.version,"configuration":{}}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status.as_u16(), 201, "{body}");
    let id = InstallationId::new(body["installation_id"].as_str().unwrap());
    let response = http
        .post(format!(
            "{base}/api/v1/capability-installations/{id}/checks"
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"client_request_id":"review-preexisting-acceptance",
            "role_id":record.role_id,"responsibility_id":record.responsibility_id,
            "suite_id":suite_id}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let checks: Value = response.json().await.unwrap();
    assert_eq!(status.as_u16(), 202, "{checks}");
    let jobs: Vec<JobId> = serde_json::from_value(checks["job_ids"].clone()).unwrap();
    assert_eq!(jobs.len(), suite().cases.len());
    for job in &jobs {
        for _ in 0..10 {
            if f.app.store.get_capability_run(job).unwrap().phase
                == genesis_server::capability_runtime::CapabilityRunPhase::Collected
            {
                break;
            }
            f.app.capability_runtime.step(job, false).await.unwrap();
        }
        assert_eq!(
            f.app.store.get_capability_run(job).unwrap().phase,
            genesis_server::capability_runtime::CapabilityRunPhase::Collected
        );
    }
    let response = http
        .post(format!(
            "{base}/api/v1/capability-installations/{id}/acceptance"
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"suite_id":suite_id,"job_ids":jobs}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let accepted: Value = response.json().await.unwrap();
    assert_eq!(status.as_u16(), 200, "{accepted}");
    assert_eq!(accepted["installation"]["status"], "ACCEPTED");
    let proof = f.app.store.capability_acceptance(&id).unwrap().unwrap();
    assert_eq!(proof.suite_id, suite_id);
    assert_eq!(proof.job_ids, jobs);
    assert_eq!(
        f.app.current_state().unwrap().installations[&id]
            .acceptance_report_ref
            .as_deref(),
        Some(proof.report_ref.as_str())
    );
    assert!(f
        .app
        .store
        .gap_finalization(&f.gap_id)
        .unwrap()
        .unwrap()
        .installation_id
        .is_none());
    id
}

#[tokio::test]
async fn review_acceptance_from_a_weaker_suite_cannot_complete_the_frozen_finalizer_suite() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    // Both suites are trusted before any provider work. The finalizer requires
    // an additional exact input; the independently accepted installation covers
    // only the original three cases. The new source remains in the allowlist.
    let mut stronger = suite();
    stronger.suite_id = "review-finalizer-four-case-suite".into();
    stronger.scope = "Original three cases plus a distinct attendee input".into();
    let mut extra = stronger.cases[0].clone();
    extra.case_id = "review-additional-attendee-input".into();
    extra.input["attendees"] = json!(121);
    stronger.cases.push(extra);
    genesis_verification::validate_suite(&stronger).unwrap();
    f.config.oracle_suites.push(stronger.clone());
    f.config.gap_finalization_policy.as_mut().unwrap().rules[0].suite_id =
        stronger.suite_id.clone();
    f.reopen();
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    assert_eq!(f.finalize_step().await, Step::Published);
    let installation = review_accept_published_artifact(&f, &suite().suite_id).await;
    let accepted = f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .unwrap();
    let before = f.providers.count("POST", "/runs");
    let first = f.finalize_step().await;
    let next = if first == Step::Installing {
        f.finalize_step().await
    } else {
        first
    };
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert_eq!(record.suite_id, stronger.suite_id);
    assert_ne!(accepted.suite_id, record.suite_id);
    assert_eq!(accepted.job_ids.len(), 3);
    assert_eq!(stronger.cases.len(), 4);
    assert_eq!(f.providers.count("POST", "/runs"), before);
    assert_ne!(
        next,
        Step::Installed,
        "the finalizer must require its frozen four-case suite, not reuse a three-case acceptance of the same pin"
    );
    assert!(!matches!(
        f.gap().status,
        CapabilityGapStatus::Installed { .. }
    ));
    // Blocking at attention or preparing a fresh matching batch is safe. Any
    // new checks must explicitly belong to the frozen suite.
    for job in &record.job_ids {
        assert!(matches!(
            f.app.store.get_capability_run(job).unwrap().purpose,
            genesis_server::capability_runtime::ExecutionPurpose::LocalAcceptance {
                installation_id,
                suite_id,
            } if installation_id == installation && suite_id == stronger.suite_id
        ));
    }
}

#[tokio::test]
async fn review_acceptance_for_the_exact_frozen_suite_can_be_reused() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    assert_eq!(f.finalize_step().await, Step::Frozen);
    assert_eq!(f.finalize_step().await, Step::Published);
    let installation = review_accept_published_artifact(&f, &suite().suite_id).await;
    let proof = f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .unwrap();
    let before = f.providers.count("POST", "/runs");
    assert_eq!(f.finish().await, Step::Installed);
    let record = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert_eq!(record.installation_id, Some(installation.clone()));
    assert_eq!(record.suite_id, proof.suite_id);
    assert_eq!(f.providers.count("POST", "/runs"), before);
    assert_eq!(
        f.app
            .store
            .capability_acceptance(&installation)
            .unwrap()
            .unwrap()
            .report_ref,
        proof.report_ref
    );
}

#[tokio::test]
async fn review_configuration_attention_preserves_prepared_checks_for_explicit_recovery() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
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
    let before = f.app.current_state().unwrap();
    let posts = f.providers.count("POST", "/runs");
    // A restart detects changed operator configuration, while the mission,
    // requirement, assignment, epoch and scopes all remain valid and unchanged.
    f.config.gap_finalization_policy.as_mut().unwrap().rules[0].intent =
        "Temporarily changed operator configuration".into();
    f.reopen();
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Configuration)
    );
    let _ = f
        .app
        .capability_runtime
        .step(&record.job_ids[0], false)
        .await;
    let paused = f.app.store.get_capability_run(&record.job_ids[0]).unwrap();
    assert_eq!(f.providers.count("POST", "/runs"), posts);
    assert_eq!(f.app.current_state().unwrap().revision, before.revision);

    // Restore the exact frozen policy and use the public retry action. No new
    // batch or request keys may be needed to finish these untouched requests.
    f.config.gap_finalization_policy = Some(finalization());
    f.reopen();
    let base = serve_url(&f).await;
    let response = reqwest::Client::new()
        .post(format!(
            "{base}/api/v1/capability-gaps/{}/finalization/retry",
            f.gap_id
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["state"], "active");
    let outcome = f.finish().await;
    assert_eq!(
        outcome,
        Step::Installed,
        "operational attention must pause, not permanently reject a valid prepared child: {:?}",
        paused.phase
    );
    let recovered = f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap();
    assert_eq!(recovered.job_ids, record.job_ids);
    assert_eq!(
        f.providers.count("POST", "/runs") - posts,
        suite().cases.len()
    );
}

#[tokio::test]
async fn review_sibling_attention_preserves_an_inflight_unknown_submission() {
    use genesis_server::capability_runtime::CapabilityRunPhase;
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
    let before = f.app.current_state().unwrap();
    // First child owns a real native ticket, then experiences a retryable GET.
    f.app
        .capability_runtime
        .step(&record.job_ids[0], false)
        .await
        .unwrap();
    f.providers.state.lock().unwrap().fail_next_run_read = true;
    let failed_read = f
        .app
        .capability_runtime
        .step(&record.job_ids[0], false)
        .await
        .unwrap();
    assert_eq!(failed_read.phase, CapabilityRunPhase::Running);
    assert!(failed_read.retryable_read);

    // The second POST has reached the loopback provider, but its 503 reply is
    // held while the separate finalizer observes the first child's read error.
    let barrier = Arc::new(RunResponseBarrier::default());
    {
        let mut world = f.providers.state.lock().unwrap();
        world.ambiguous_runs = true;
        world.run_response_barrier = Some(barrier.clone());
    }
    let runtime = f.app.capability_runtime.clone();
    let job = record.job_ids[1].clone();
    let submitting = tokio::spawn(async move { runtime.step(&job, false).await });
    tokio::time::timeout(Duration::from_secs(2), barrier.entered.notified())
        .await
        .unwrap();
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Checks)
    );
    assert_eq!(f.app.current_state().unwrap().revision, before.revision);
    barrier.release.notify_one();
    let result = submitting.await.unwrap().unwrap();
    let posts = f.providers.count("POST", "/runs");
    // Further explicit child stepping must not issue the unknown POST a
    // second time. The original uncertainty must remain visible, not become a
    // definite authority rejection while the actual grant never changed.
    let _ = f
        .app
        .capability_runtime
        .step(&record.job_ids[1], false)
        .await;
    assert_eq!(f.providers.count("POST", "/runs"), posts);
    assert_eq!(
        result.phase,
        CapabilityRunPhase::NeedsReconciliation,
        "sibling operational attention overwrote an ambiguous native POST: {:?}",
        result.failure
    );
    assert!(matches!(
        result.failure,
        Some(genesis_providers::ProviderError::Unknown(_))
    ));
}

// ---------------------------------------------------------------------------
// Guard pins. Each test below observes one finalizer fence directly: the
// returned step, provider/registry effects, and record/gap/installation state.
// Kernel edits use the same commit seam as the review tests above; gap
// refusals go through the kernel's own transition validation.
// ---------------------------------------------------------------------------

/// Refuse the gap through the kernel's transition rules. Caller holds the
/// kernel lock.
fn guard_refuse_gap_locked(app: &AppState, gap_id: &CapabilityGapId) {
    let before = app.current_state().unwrap();
    let gap = before.capability_gaps[gap_id].clone();
    let outcome = engine::update_capability_gap_at(
        &before,
        gap_id,
        CapabilityGapStatus::Refused {
            reason: "Closed by the organization while finalization was pending".into(),
            acquisition_id: gap.status.acquisition_id().map(str::to_owned),
        },
        Utc::now(),
    )
    .unwrap();
    app.store
        .commit_state(
            before.revision,
            &outcome.state,
            outcome.effects,
            &EventMeta::mission(gap.mission_id),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    assert!(matches!(
        app.current_state().unwrap().capability_gaps[gap_id].status,
        CapabilityGapStatus::Refused { .. }
    ));
}

async fn guard_refuse_gap(f: &Fixture) {
    let _guard = f.app.kernel.lock().await;
    guard_refuse_gap_locked(&f.app, &f.gap_id);
}

/// Commit an arbitrary state edit through the store seam.
async fn guard_edit_state(f: &Fixture, change: impl FnOnce(&mut OrgState)) {
    let _guard = f.app.kernel.lock().await;
    let before = f.app.current_state().unwrap();
    let mut after = before.clone();
    change(&mut after);
    after.revision += 1;
    f.app
        .store
        .commit_state(
            before.revision,
            &after,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
}

async fn guard_steps(f: &Fixture, expected: &[Step]) {
    for step in expected {
        assert_eq!(&f.finalize_step().await, step);
    }
}

fn guard_record(f: &Fixture) -> capability_gap_finalizer::FinalizationRecord {
    f.app.store.gap_finalization(&f.gap_id).unwrap().unwrap()
}

/// A registry endpoint that answers every version read with `manifest`:
/// stored registry content that no longer matches what was confirmed.
struct GuardRegistry {
    task: tokio::task::JoinHandle<()>,
}
impl Drop for GuardRegistry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Point the organization at a registry serving the confirmed manifest after
/// `change`. Returns the real registry URL so the caller can switch back.
async fn guard_serve_changed_manifest(
    f: &mut Fixture,
    change: impl FnOnce(&mut CapabilityManifest),
) -> (GuardRegistry, Option<String>) {
    let record = guard_record(f);
    let mut manifest = f
        .app
        .registry
        .as_ref()
        .unwrap()
        .manifest(&record.capability.capability_id, &record.capability.version)
        .await
        .unwrap();
    change(&mut manifest);
    let served = serde_json::to_value(&manifest).unwrap();
    let router = Router::new().fallback(move |request: Request| {
        let served = served.clone();
        async move {
            if request.method() == "GET"
                && request
                    .uri()
                    .path()
                    .starts_with("/registry/v1/capabilities/")
            {
                (StatusCode::OK, Json(served))
            } else {
                (StatusCode::NOT_FOUND, Json(json!({"error":"not served"})))
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let real = f.config.registry_url.replace(url);
    f.reopen();
    (GuardRegistry { task }, real)
}

fn guard_restore_registry(f: &mut Fixture, real: Option<String>) {
    f.config.registry_url = real;
    f.reopen();
}

/// Install, check and accept the published version through the public API
/// with the given role/responsibility, before the finalizer selects one.
async fn guard_public_acceptance(
    f: &Fixture,
    role: &RoleId,
    responsibility: &ResponsibilityId,
    accept: bool,
) -> InstallationId {
    let record = guard_record(f);
    let base = serve_url(f).await;
    let http = reqwest::Client::new();
    let response = http
        .post(format!("{base}/api/v1/capability-installations"))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"capability_id":record.capability.capability_id,
            "version":record.capability.version,"configuration":{}}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status.as_u16(), 201, "{body}");
    let id = InstallationId::new(body["installation_id"].as_str().unwrap());
    if !accept {
        return id;
    }
    let response = http
        .post(format!(
            "{base}/api/v1/capability-installations/{id}/checks"
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"client_request_id":"guard-public-acceptance",
            "role_id":role,"responsibility_id":responsibility,"suite_id":record.suite_id}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let checks: Value = response.json().await.unwrap();
    assert_eq!(status.as_u16(), 202, "{checks}");
    let jobs: Vec<JobId> = serde_json::from_value(checks["job_ids"].clone()).unwrap();
    for job in &jobs {
        for _ in 0..10 {
            if f.app.store.get_capability_run(job).unwrap().phase
                == genesis_server::capability_runtime::CapabilityRunPhase::Collected
            {
                break;
            }
            f.app.capability_runtime.step(job, false).await.unwrap();
        }
    }
    let response = http
        .post(format!(
            "{base}/api/v1/capability-installations/{id}/acceptance"
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({"suite_id":record.suite_id,"job_ids":jobs}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let accepted: Value = response.json().await.unwrap();
    assert_eq!(status.as_u16(), 200, "{accepted}");
    assert_eq!(accepted["installation"]["status"], "ACCEPTED");
    id
}

fn guard_not_installed(f: &Fixture) {
    assert!(
        matches!(f.gap().status, CapabilityGapStatus::Acquired { .. }),
        "{:?}",
        f.gap().status
    );
}

// --- Freeze ----------------------------------------------------------------

#[tokio::test]
async fn guard_freeze_rereads_the_gap_under_the_kernel_lock() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    // Freeze reads the gap, then verifies the acquisition without holding the
    // kernel lock. A refusal committed in that window must be seen.
    let guard = f.app.kernel.lock().await;
    let (app, gap) = (f.app.clone(), f.gap_id.clone());
    let step = tokio::spawn(async move { capability_gap_finalizer::step(&app, &gap).await });
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    guard_refuse_gap_locked(&f.app, &f.gap_id);
    drop(guard);
    assert_eq!(step.await.unwrap().unwrap(), Step::Idle);
    assert!(
        f.app.store.gap_finalization(&f.gap_id).unwrap().is_none(),
        "no record is frozen for a gap that is no longer acquired"
    );
}

// --- Gap still names the acquisition at every stage ------------------------

#[tokio::test]
async fn guard_publish_stops_before_the_registry_when_the_gap_moved_on() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen]).await;
    guard_refuse_gap(&f).await;
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Acquisition)
    );
    let record = guard_record(&f);
    assert!(record.publication_ref.is_none());
    assert!(
        f.app
            .registry
            .as_ref()
            .unwrap()
            .manifest(&record.capability.capability_id, &record.capability.version)
            .await
            .is_err(),
        "nothing was published for a gap that no longer names the acquisition"
    );
}

#[tokio::test]
async fn guard_install_stops_when_the_gap_moved_on() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    guard_refuse_gap(&f).await;
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Acquisition)
    );
    assert!(f.app.current_state().unwrap().installations.is_empty());
    assert!(guard_record(&f).installation_id.is_none());
}

#[tokio::test]
async fn guard_checks_are_not_queued_when_the_gap_moved_on() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published, Step::Installing]).await;
    guard_refuse_gap(&f).await;
    let posts = f.providers.count("POST", "/runs");
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Acquisition)
    );
    let record = guard_record(&f);
    assert!(record.job_ids.is_empty(), "no check batch was queued");
    assert!(f
        .app
        .store
        .capability_check_batch(&record.client_request_id())
        .unwrap()
        .is_none());
    assert_eq!(f.providers.count("POST", "/runs"), posts);
}

#[tokio::test]
async fn guard_acceptance_stops_when_the_gap_moved_on() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(
        &f,
        &[
            Step::Frozen,
            Step::Published,
            Step::Installing,
            Step::Checking,
        ],
    )
    .await;
    f.run_checks().await;
    guard_refuse_gap(&f).await;
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Acquisition)
    );
    let record = guard_record(&f);
    assert_eq!(
        f.app.current_state().unwrap().installations[record.installation_id.as_ref().unwrap()]
            .status,
        InstallationStatus::Checking,
        "the installation is neither accepted nor closed by the finalizer"
    );
    assert!(f
        .app
        .store
        .capability_acceptance(record.installation_id.as_ref().unwrap())
        .unwrap()
        .is_none());
}

// --- Authority -------------------------------------------------------------

#[tokio::test]
async fn guard_lost_authority_refuses_while_checks_are_still_pending() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(
        &f,
        &[
            Step::Frozen,
            Step::Published,
            Step::Installing,
            Step::Checking,
        ],
    )
    .await;
    // No run has been collected yet; the live requirement is satisfied.
    review_commit_requirement(&f, |requirement| {
        requirement.status = genesis_core::lifecycle::RequirementStatus::Satisfied;
    })
    .await;
    let posts = f.providers.count("POST", "/runs");
    assert_eq!(f.finalize_step().await, Step::Refused);
    let record = guard_record(&f);
    assert_eq!(record.state, FinalizerState::Refused);
    assert_eq!(record.code, Some(FinalizerCode::Authority));
    let CapabilityGapStatus::Refused { reason, .. } = f.gap().status else {
        panic!("{:?}", f.gap().status)
    };
    assert!(reason.contains("authority"), "{reason}");
    assert_eq!(
        f.app.current_state().unwrap().installations[record.installation_id.as_ref().unwrap()]
            .status,
        InstallationStatus::Rejected
    );
    assert_eq!(f.providers.count("POST", "/runs"), posts);
}

#[tokio::test]
async fn guard_accepted_installation_checked_under_another_responsibility_is_not_reused() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    let record = guard_record(&f);
    let state = f.app.current_state().unwrap();
    // The same role's other responsibility, which it legitimately holds.
    let other = state
        .responsibilities
        .values()
        .find(|r| r.responsibility_id != record.responsibility_id && r.purpose == "Coordinate")
        .unwrap()
        .responsibility_id
        .clone();
    let installation = guard_public_acceptance(&f, &record.role_id, &other, true).await;
    let posts = f.providers.count("POST", "/runs");
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Installation)
    );
    assert!(guard_record(&f).installation_id.is_none());
    guard_not_installed(&f);
    assert_eq!(
        f.app.current_state().unwrap().installations[&installation].status,
        InstallationStatus::Accepted
    );
    assert_eq!(f.providers.count("POST", "/runs"), posts);
}

#[tokio::test]
async fn guard_accepted_installation_with_unauthorized_proof_jobs_is_not_reused() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    let record = guard_record(&f);
    // An installation of the same pin whose proof jobs needed one more scope.
    let (registry, real) = guard_serve_changed_manifest(&mut f, |m| {
        m.required_permissions.insert("capability.acquire".into());
    })
    .await;
    let installation =
        guard_public_acceptance(&f, &record.role_id, &record.responsibility_id, true).await;
    drop(registry);
    guard_restore_registry(&mut f, real);
    // The role keeps every scope the frozen manifest needs, but not that one.
    let role = record.role_id.clone();
    guard_edit_state(&f, |state| {
        state
            .roles
            .get_mut(&role)
            .unwrap()
            .authority
            .scopes
            .remove("capability.acquire");
    })
    .await;
    let step = f.finalize_step().await;
    assert!(
        matches!(
            step,
            Step::Attention(FinalizerCode::Authority | FinalizerCode::Installation)
        ),
        "{step:?}"
    );
    assert!(guard_record(&f).installation_id.is_none());
    guard_not_installed(&f);
    assert_eq!(
        f.app.current_state().unwrap().installations[&installation].status,
        InstallationStatus::Accepted
    );
}

#[tokio::test]
async fn guard_acceptance_rechecks_each_check_job_grant() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    let record = guard_record(&f);
    // A still-checking installation of the same pin whose check jobs need one
    // more scope than the frozen manifest.
    let (registry, real) = guard_serve_changed_manifest(&mut f, |m| {
        m.required_permissions.insert("capability.acquire".into());
    })
    .await;
    guard_public_acceptance(&f, &record.role_id, &record.responsibility_id, false).await;
    drop(registry);
    guard_restore_registry(&mut f, real);
    let mut step = f.finalize_step().await;
    if step == Step::Installing {
        step = f.finalize_step().await;
    }
    if step == Step::Checking {
        f.run_checks().await;
        let role = record.role_id.clone();
        guard_edit_state(&f, |state| {
            state
                .roles
                .get_mut(&role)
                .unwrap()
                .authority
                .scopes
                .remove("capability.acquire");
        })
        .await;
        step = f.finalize_step().await;
    }
    assert!(
        matches!(step, Step::Refused | Step::Attention(_)),
        "a check job that is no longer authorized must not be accepted: {step:?}"
    );
    guard_not_installed_or_refused(&f);
    assert!(f
        .app
        .current_state()
        .unwrap()
        .installations
        .values()
        .all(|i| i.status != InstallationStatus::Accepted));
}

fn guard_not_installed_or_refused(f: &Fixture) {
    assert!(
        !matches!(f.gap().status, CapabilityGapStatus::Installed { .. }),
        "{:?}",
        f.gap().status
    );
}

// --- Installation integrity ------------------------------------------------

#[tokio::test]
async fn guard_install_requires_the_frozen_pin_and_this_publisher() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    // The registry now serves a different build at the confirmed version.
    let (registry, real) = guard_serve_changed_manifest(&mut f, |m| {
        m.artifact.build_id = "build-substituted".into();
    })
    .await;
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Installation)
    );
    assert!(f.app.current_state().unwrap().installations.is_empty());
    drop(registry);
    guard_restore_registry(&mut f, real);

    // After an operator retry, the same pin from another publisher also stops.
    let (_registry, _) = guard_serve_changed_manifest(&mut f, |m| {
        m.publisher = OrgId::new("org-elsewhere");
    })
    .await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/capability-gaps/{}/finalization/retry",
            serve_url(&f).await,
            f.gap_id
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Installation)
    );
    assert!(f.app.current_state().unwrap().installations.is_empty());
    assert!(guard_record(&f).installation_id.is_none());
}

#[tokio::test]
async fn guard_install_rechecks_the_registry_manifest_is_installable() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    // Same pin and publisher, but its report now records a failed case.
    let (_registry, _) = guard_serve_changed_manifest(&mut f, |m| {
        assert!(!m.verification.cases.is_empty());
        m.verification.cases[0].passed = false;
    })
    .await;
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Installation)
    );
    assert!(f.app.current_state().unwrap().installations.is_empty());
}

#[tokio::test]
async fn guard_an_existing_installation_of_another_build_is_not_reused() {
    let mut f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published]).await;
    let record = guard_record(&f);
    // Someone installed the same capability version from a different build.
    let (registry, real) = guard_serve_changed_manifest(&mut f, |m| {
        m.artifact.build_id = "build-substituted".into();
    })
    .await;
    let foreign =
        guard_public_acceptance(&f, &record.role_id, &record.responsibility_id, false).await;
    drop(registry);
    guard_restore_registry(&mut f, real);
    assert_eq!(
        f.finalize_step().await,
        Step::Attention(FinalizerCode::Installation)
    );
    assert!(guard_record(&f).installation_id.is_none());
    let state = f.app.current_state().unwrap();
    assert_eq!(state.installations.len(), 1);
    assert_eq!(
        state.installations[&foreign].status,
        InstallationStatus::Checking
    );
}

// --- Record compare-and-swap -----------------------------------------------

#[tokio::test]
async fn guard_a_stale_record_writer_conflicts_and_commits_nothing() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen, Step::Published, Step::Installing]).await;
    let before = f.app.current_state().unwrap();
    // Writer A reads the active record, then waits for the kernel lock.
    let guard = f.app.kernel.lock().await;
    let (app, gap) = (f.app.clone(), f.gap_id.clone());
    let stale = tokio::spawn(async move { capability_gap_finalizer::step(&app, &gap).await });
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    // Writer B (another process on the same database, with a changed rule)
    // moves the record to attention meanwhile.
    let mut changed = f.config.clone();
    changed.gap_finalization_policy.as_mut().unwrap().rules[0].intent =
        "A concurrently changed promise".into();
    let other = build_org(&changed, Some(provider_set(&f.providers))).unwrap();
    assert_eq!(
        capability_gap_finalizer::step(&other, &f.gap_id)
            .await
            .unwrap(),
        Step::Attention(FinalizerCode::Configuration)
    );
    drop(guard);
    let result = stale.await.unwrap();
    assert!(
        matches!(&result, Err(e) if e.to_string().contains("revision conflict")),
        "{result:?}"
    );
    // Nothing from A's transaction was committed.
    let record = guard_record(&f);
    assert_eq!(record.state, FinalizerState::Attention);
    assert_eq!(record.code, Some(FinalizerCode::Configuration));
    assert!(record.job_ids.is_empty());
    assert!(f
        .app
        .store
        .capability_check_batch(&record.client_request_id())
        .unwrap()
        .is_none());
    assert_eq!(f.app.current_state().unwrap().revision, before.revision);
}

// --- Low priority ----------------------------------------------------------

#[tokio::test]
async fn guard_a_record_stored_under_another_gap_key_is_rejected() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    f.acquire().await;
    guard_steps(&f, &[Step::Frozen]).await;
    let record = guard_record(&f);
    let db = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    db.execute(
        "INSERT INTO capability_gap_finalizations(gap_id, record_json) VALUES (?1, ?2)",
        rusqlite::params!["gap-misfiled-copy", serde_json::to_string(&record).unwrap()],
    )
    .unwrap();
    assert!(f
        .app
        .store
        .gap_finalization(&CapabilityGapId::new("gap-misfiled-copy"))
        .is_err());
    assert_eq!(guard_record(&f), record);
}

#[tokio::test]
async fn guard_status_of_an_unknown_gap_is_not_found() {
    let f = Fixture::new(&["capability.acquire", PERMISSION]).await;
    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/capability-gaps/gap-that-does-not-exist/finalization",
            serve_url(&f).await
        ))
        .bearer_auth(format!("{ORG}-operator"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
