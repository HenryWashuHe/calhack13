//! Henry's Claude: crash-anywhere pressure test of the two-organization
//! capability beat. Same harness as `two_org_inheritance`, but each org's
//! process is killed (server future aborted, AppState dropped) on a fixed
//! schedule and rebuilt from its same SQLite file, like a restart. The
//! loopback provider records each call, then waits before replying, so a
//! kill can land after the provider acted but before the client heard back.
//!
//! Safety (asserted): no duplicate model call, Actor build or B synthesis.
//! Liveness (reported, then asserted): does the beat still finish?
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
    model::{CapabilityGapStatus, OrgState},
};
use genesis_forge::{AnthropicGenerator, ModelConfig, PublicExample};
use genesis_providers::{ApifyCapabilityExecutor, ProviderSet};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    capability_gap_adoption::{GapAdoptionPolicy, GapAdoptionRule},
    capability_gap_finalizer::{GapFinalizationPolicy, GapFinalizationRule},
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    parse_publishers,
    registry::RegistryState,
    serve_org, spawn_registry,
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

const PROVIDER_DELAY_MS: u64 = 40;
const MODEL: &str = "claude-sonnet-4-6";
const KEY_A: &str = "two-org-loopback-key-a";
const KEY_B: &str = "two-org-loopback-key-b";
const PERMISSION: &str = "capability.venue_lookup";
const NEED: &str = "Read venue capacity price availability from the public portal";
const SOURCE: &str = "https://fixtures.genesis.example/venues/index.html";

fn a_suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
/// B's own suite: different case IDs and inputs; A never sees it.
fn b_suite() -> OracleSuite {
    let mut s = a_suite();
    s.suite_id = "two-org-suite-b".into();
    for (index, case) in s.cases.iter_mut().enumerate() {
        case.case_id = format!("b-case-{index}");
        case.input["attendees"] = json!(case.input["attendees"].as_u64().unwrap() + 7);
    }
    s
}
fn receipts() -> Vec<ExecutionEvidence> {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Org {
    A,
    B,
}

#[derive(Default)]
struct World {
    calls: Vec<(Org, String, String)>,
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
    fn reply(&mut self, org: Org, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.calls.push((org, method.into(), path.into()));
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "messages"]) => (
                StatusCode::OK,
                json!({"id":"msg_two_org","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":json!({"extractor_typescript":"export function extract() { throw new Error('loopback fixture, never executed locally'); }"}).to_string()}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":120,"output_tokens":80}}),
            ),
            ("POST", ["v2", "actors"]) => {
                let id = format!("actor-two-org-{}", self.actors.len());
                let mut response = body.clone();
                response["id"] = json!(id);
                self.actors.insert(id, body);
                (StatusCode::CREATED, json!({"data":response}))
            }
            ("GET", ["v2", "actors", actor, "versions", _]) => (
                StatusCode::OK,
                json!({"data":self.actors[*actor]["versions"][0]}),
            ),
            ("POST", ["v2", "actors", actor, "builds"]) => (StatusCode::CREATED, self.build(actor)),
            ("GET", ["v2", "actor-builds", build]) => (
                StatusCode::OK,
                self.build(build.strip_prefix("build-").unwrap()),
            ),
            ("POST", ["v2", "actors", actor, "runs"]) => {
                // Each organization may only execute its own suite's inputs.
                let suite = if org == Org::A { a_suite() } else { b_suite() };
                let case = suite
                    .cases
                    .iter()
                    .position(|case| case.input == body)
                    .unwrap_or_else(|| panic!("{org:?} ran an input outside its own suite"));
                let id = format!("run-two-org-{}", self.runs.len());
                self.runs.insert(
                    id.clone(),
                    ((*actor).into(), receipts()[case].output.clone()),
                );
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

struct Provider {
    world: Arc<Mutex<World>>,
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Provider {
    async fn start() -> Self {
        let world = Arc::new(Mutex::new(World::default()));
        let router = Router::new()
            .fallback(
                |State(world): State<Arc<Mutex<World>>>, request: Request| async move {
                    let method = request.method().to_string();
                    let path = request.uri().path().to_owned();
                    let key = request
                        .headers()
                        .get("x-api-key")
                        .map(|v| v.to_str().unwrap().to_owned())
                        .or_else(|| {
                            request.headers().get("authorization").map(|v| {
                                v.to_str().unwrap().trim_start_matches("Bearer ").to_owned()
                            })
                        })
                        .expect("authenticated provider call");
                    let org = match key.as_str() {
                        KEY_A => Org::A,
                        KEY_B => Org::B,
                        other => panic!("unknown provider key {other}"),
                    };
                    let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
                        .await
                        .unwrap();
                    let body = if bytes.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes).unwrap()
                    };
                    let (status, body) = world.lock().unwrap().reply(org, &method, &path, body);
                    tokio::time::sleep(Duration::from_millis(PROVIDER_DELAY_MS)).await;
                    (status, Json(body))
                },
            )
            .with_state(world.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { world, url, task }
    }
    fn count(&self, org: Org, method: &str, pred: impl Fn(&str) -> bool) -> usize {
        self.world
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(o, m, p)| *o == org && m == method && pred(p))
            .count()
    }
    fn model_calls(&self, org: Org) -> usize {
        self.count(org, "POST", |p| p == "/v1/messages")
    }
    fn actor_creations(&self, org: Org) -> usize {
        self.count(org, "POST", |p| p == "/v2/actors")
    }
    fn runs(&self, org: Org) -> usize {
        self.count(org, "POST", |p| p.ends_with("/runs"))
    }
    fn apify(&self, key: &str) -> ApifyClient {
        ApifyClient::for_local_testing(key, limits(), &self.url).unwrap()
    }
}
fn limits() -> RunLimits {
    RunLimits {
        request_timeout: Duration::from_secs(5),
        deadline: Duration::from_secs(8),
        poll_interval: Duration::from_millis(1),
        ..RunLimits::default()
    }
}

fn gap_policy(suite: &OracleSuite) -> GapAcquisitionPolicy {
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: "venue-reader".into(),
            suite_id: suite.suite_id.clone(),
            actor_name_prefix: "gap-reader".into(),
            allowed_source_urls: suite.cases.iter().map(|c| c.source.url.clone()).collect(),
            examples: vec![PublicExample {
                url: suite.cases[0].source.url.clone(),
                html: "<html>FIXED_PUBLIC_GAP_EXAMPLE</html>".into(),
            }],
            repair: false,
        }],
    }
}
/// B names the same need differently; adoption may still satisfy it.
fn b_gap_policy() -> GapAcquisitionPolicy {
    let mut p = gap_policy(&b_suite());
    p.rules[0].capability_id = "capacity-checker".into();
    p
}

fn org_config(
    dir: &tempfile::TempDir,
    org: &str,
    registry: &str,
    token: &str,
    suite: OracleSuite,
    provider: &Provider,
    key: &str,
) -> OrgConfig {
    let mut config = OrgConfig::dev(org, org);
    config.db = Some(dir.path().join(format!("{org}.sqlite")));
    config.dev_stub = false;
    config.registry_url = Some(registry.into());
    config.registry_token = Some(token.into());
    config.oracle_suites = vec![suite];
    let mut acquisition = AcquisitionConfig::offline(AcquisitionPolicy {
        model: ModelConfig {
            model: MODEL.into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 3600,
    });
    acquisition.model = Some(Arc::new(
        AnthropicGenerator::with_test_endpoint(&format!("{}/v1/messages", provider.url), key)
            .unwrap(),
    ));
    acquisition.apify = Some(Arc::new(provider.apify(key)));
    acquisition.automatic = true;
    config.acquisition = Some(acquisition);
    config
}

fn providers(provider: &Provider, key: &str) -> ProviderSet {
    ProviderSet::new().with_executor(Arc::new(ApifyCapabilityExecutor::new(provider.apify(key))))
}

/// Bootstraps a mission whose committed change requests `capability_id`.
async fn request_capability(
    app: &AppState,
    capability_id: &str,
    sources: Vec<String>,
) -> CapabilityGapId {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Find a venue", "roles":[{"key":"lead","name":"Lead","charter":"Own venue lookup",
            "authority":{"scopes":["capability.acquire",PERMISSION],"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Coordinate","owner":"lead"}]
    }))
    .unwrap();
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
                Some(SOURCE.into()),
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
                NEED,
                std::slice::from_ref(&observation),
                "Pass this organization's own extraction checks",
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
                "capability_id":capability_id, "purpose":"Read venue listings", "observed_sources":sources}
        ]
    }))
    .unwrap();
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
    gap_id
}

async fn serve(app: &AppState) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app = app.clone();
    tokio::spawn(async move { serve_org(app, listener).await.unwrap() })
}

fn a_config(dir: &tempfile::TempDir, registry: &str, provider: &Provider) -> OrgConfig {
    let mut config = org_config(
        dir,
        "org-a",
        registry,
        "a-registry",
        a_suite(),
        provider,
        KEY_A,
    );
    config.gap_acquisition_policy = Some(gap_policy(&a_suite()));
    config.gap_finalization_policy = Some(GapFinalizationPolicy {
        rules: vec![GapFinalizationRule {
            capability_id: CapabilityId::new("venue-reader"),
            suite_id: a_suite().suite_id,
            intent: NEED.into(),
            applicability_conditions: vec!["Public venue portal pages".into()],
            known_limitations: vec!["Lookup only; no reservation".into()],
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object"}),
            required_permissions: [PERMISSION.to_owned()].into(),
            responsibility_template: Some("Keep verified venue lookup available".into()),
            workflow_procedure: None,
            local_configuration_schema: json!({}),
        }],
    });
    config
}

fn b_config(dir: &tempfile::TempDir, registry: &str, provider: &Provider) -> OrgConfig {
    let mut config = org_config(
        dir,
        "org-b",
        registry,
        "b-registry",
        b_suite(),
        provider,
        KEY_B,
    );
    config.gap_acquisition_policy = Some(b_gap_policy());
    config.gap_adoption_policy = Some(GapAdoptionPolicy {
        rules: vec![GapAdoptionRule {
            capability_id: "capacity-checker".into(),
            suite_id: b_suite().suite_id,
            synthesis_fallback: true,
        }],
    });
    config
}

/// Where a gap ended up after the last restart.
fn describe(app: &AppState, gap: &CapabilityGapId) -> String {
    let state = app.current_state().unwrap();
    let status = format!("{:?}", state.capability_gaps[gap].status);
    let status = status.split_whitespace().next().unwrap_or("").to_owned();
    let driver = genesis_server::capability_gap_driver::status(app, gap)
        .map(|s| format!("driver={:?}/{:?}", s.state, s.code))
        .unwrap_or_default();
    let adoption = app
        .store
        .gap_adoption(gap)
        .ok()
        .flatten()
        .map(|r| {
            format!(
                " adoption={:?}/{:?} id={:?}",
                r.state, r.code, r.adoption_id
            )
        })
        .unwrap_or_default();
    let finalizer = app
        .store
        .gap_finalization(gap)
        .ok()
        .flatten()
        .map(|r| format!(" finalizer={:?}/{:?}", r.state, r.code))
        .unwrap_or_default();
    format!("{status} {driver}{adoption}{finalizer}")
}

/// Kill and rebuild `config`'s org after each delay in `kills`, then let a
/// final process run. Returns the final app and whether the gap installed.
async fn run_with_crashes(
    config: &OrgConfig,
    provider: &Provider,
    key: &str,
    capability: &str,
    sources: Vec<String>,
    kills: &[u64],
) -> (AppState, CapabilityGapId, Result<OrgState, String>) {
    let mut app = build_org(config, Some(providers(provider, key))).unwrap();
    let gap = request_capability(&app, capability, sources).await;
    for ms in kills {
        let server = serve(&app).await;
        tokio::time::sleep(Duration::from_millis(*ms)).await;
        server.abort();
        let _ = server.await;
        drop(app);
        app = build_org(config, Some(providers(provider, key))).unwrap();
    }
    let _server = serve(&app).await;
    let secs = std::env::var("CHAOS_TIMEOUT_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let outcome = tokio::time::timeout(Duration::from_secs(secs), async {
        loop {
            let state = app.current_state().unwrap();
            match &state.capability_gaps[&gap].status {
                CapabilityGapStatus::Installed { .. } => return Ok(state),
                CapabilityGapStatus::Refused { reason, .. } => {
                    return Err(format!("refused: {reason}"))
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err(format!("stalled: {}", describe(&app, &gap))));
    _server.abort();
    (app, gap, outcome)
}

async fn registry() -> String {
    let addr = spawn_registry(
        RegistryState::open_in_memory(
            parse_publishers("org-a:a-registry,org-b:b-registry"),
            vec![],
        )
        .unwrap(),
        "127.0.0.1:0",
    )
    .await
    .unwrap();
    format!("http://{addr}")
}

fn schedules() -> Vec<Vec<u64>> {
    let mut out = vec![vec![]];
    for step in [30, 70, 120, 190, 280, 400, 600] {
        out.push(vec![step; 5]);
    }
    out.push(vec![15, 45, 90, 150, 240, 380, 560, 800]);
    // CHAOS_ONLY=i runs one schedule (for debugging).
    match std::env::var("CHAOS_ONLY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        Some(i) => vec![out.swap_remove(i)],
        None => out,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn org_a_survives_restarts_at_any_point_of_synthesis_and_finalization() {
    let mut report = Vec::new();
    for kills in schedules() {
        let provider = Provider::start().await;
        let registry = registry().await;
        let dir = tempfile::tempdir().unwrap();
        let config = a_config(&dir, &registry, &provider);
        let (app, _gap, outcome) = run_with_crashes(
            &config,
            &provider,
            KEY_A,
            "venue-reader",
            gap_policy(&a_suite()).rules[0].allowed_source_urls.clone(),
            &kills,
        )
        .await;
        // Safety, whatever the outcome.
        let models = provider.model_calls(Org::A);
        let actors = provider.actor_creations(Org::A);
        assert!(models <= 1, "{kills:?}: {models} model calls");
        assert!(actors <= 1, "{kills:?}: {actors} Actor creations");
        let state = app.current_state().unwrap();
        assert!(genesis_core::audit::audit(&state).is_empty(), "{kills:?}");
        report.push(format!(
            "A kills={kills:?} models={models} actors={actors} runs={} -> {}",
            provider.runs(Org::A),
            match &outcome {
                Ok(_) => "installed".to_owned(),
                Err(e) => e.clone(),
            }
        ));
    }
    let report = report.join("\n");
    eprintln!("{report}");
    assert!(
        report.lines().all(|l| l.ends_with("-> installed")),
        "the beat did not finish after some restarts:\n{report}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn org_b_survives_restarts_at_any_point_of_adoption() {
    let mut report = Vec::new();
    for kills in schedules() {
        let provider = Provider::start().await;
        let registry = registry().await;
        let dir = tempfile::tempdir().unwrap();
        let a = a_config(&dir, &registry, &provider);
        let (_a, _a_gap, a_outcome) = run_with_crashes(
            &a,
            &provider,
            KEY_A,
            "venue-reader",
            gap_policy(&a_suite()).rules[0].allowed_source_urls.clone(),
            &[],
        )
        .await;
        a_outcome.expect("A installs without restarts");
        let a_runs = provider.runs(Org::A);
        let b = b_config(&dir, &registry, &provider);
        let (app, gap, outcome) = run_with_crashes(
            &b,
            &provider,
            KEY_B,
            "capacity-checker",
            b_gap_policy().rules[0].allowed_source_urls.clone(),
            &kills,
        )
        .await;
        let runs = provider.runs(Org::B);
        assert_eq!(provider.model_calls(Org::B), 0, "{kills:?}: B synthesized");
        assert_eq!(provider.actor_creations(Org::B), 0, "{kills:?}");
        assert!(runs <= b_suite().cases.len(), "{kills:?}: {runs} B runs");
        assert_eq!(provider.runs(Org::A), a_runs, "{kills:?}");
        assert!(app
            .store
            .gap_acquisition_binding(gap.as_str())
            .unwrap()
            .is_none());
        let state = app.current_state().unwrap();
        assert!(genesis_core::audit::audit(&state).is_empty(), "{kills:?}");
        report.push(format!(
            "B kills={kills:?} runs={runs} -> {}",
            match &outcome {
                Ok(_) => "installed".to_owned(),
                Err(e) => e.clone(),
            }
        ));
    }
    let report = report.join("\n");
    eprintln!("{report}");
    assert!(
        report.lines().all(|l| l.ends_with("-> installed")),
        "the beat did not finish after some restarts:\n{report}"
    );
}

/// A restart during B's adoption checks stops the gap in attention; one
/// operator retry must still get it installed, with no synthesis.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn org_b_operator_retry_recovers_a_restart_during_checks() {
    let provider = Provider::start().await;
    let registry = registry().await;
    let dir = tempfile::tempdir().unwrap();
    let a = a_config(&dir, &registry, &provider);
    let (_a, _, a_outcome) = run_with_crashes(
        &a,
        &provider,
        KEY_A,
        "venue-reader",
        gap_policy(&a_suite()).rules[0].allowed_source_urls.clone(),
        &[],
    )
    .await;
    a_outcome.expect("A installs");
    let b = b_config(&dir, &registry, &provider);
    std::env::set_var("CHAOS_TIMEOUT_S", "30");
    let (app, gap, outcome) = run_with_crashes(
        &b,
        &provider,
        KEY_B,
        "capacity-checker",
        b_gap_policy().rules[0].allowed_source_urls.clone(),
        &[600; 5],
    )
    .await;
    let stalled = match outcome {
        Ok(_) => return eprintln!("schedule did not stall this time; nothing to retry"),
        Err(e) => e,
    };
    assert!(
        stalled.contains("adoption=Attention/Some(Checks)"),
        "{stalled}"
    );
    let retried = genesis_server::capability_gap_adoption::retry_route(
        axum::extract::State(app.clone()),
        genesis_server::auth::Principal::Operator,
        axum::extract::Path(gap.to_string()),
    )
    .await;
    assert!(
        retried.is_ok(),
        "operator retry refused: {:?}",
        retried.err().map(|e| e.message)
    );
    let _server = serve(&app).await;
    let installed = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if matches!(
                app.current_state().unwrap().capability_gaps[&gap].status,
                CapabilityGapStatus::Installed { .. }
            ) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    assert!(installed.is_ok(), "after retry: {}", describe(&app, &gap));
    assert_eq!(provider.model_calls(Org::B), 0);
    assert_eq!(provider.actor_creations(Org::B), 0);
    assert!(genesis_core::audit::audit(&app.current_state().unwrap()).is_empty());
}
