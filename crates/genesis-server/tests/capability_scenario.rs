//! Complete HTTP scenario with two private SQLite stores and real Rust provider
//! adapters. Model output and provider execution are canned loopback fixtures;
//! this does not claim cloud execution, autonomous planning, or native role
//! activation. Both synthetic provider credentials may access the same Actor.
//! Real organization servers own progression; an HTTP guard rejects all manual
//! step/verify requests made by the scenario or these tests.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::ids::{InstallationId, JobId, RoleId};
use genesis_core::lifecycle::{InstallationStatus, JobStatus, RoleState};
use genesis_core::model::{Owner, Provider};
use genesis_forge::{
    AcquisitionPhase, AnthropicGenerator, CandidateIdentity, ModelConfig, PublicExample,
};
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::app::AppState;
use genesis_server::capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas};
use genesis_server::capability_runtime::CapabilityRunPhase;
use genesis_server::capability_scenario::{
    ScenarioDriver, ScenarioEndpoint, ScenarioError, ScenarioLimits, ScenarioProgression,
    ScenarioSpec,
};
use genesis_server::registry::{self, RegistryState};
use genesis_server::{build_org, parse_publishers, serve_org, AcquisitionConfig, OrgConfig};
use genesis_verification::{decode_json, json_sha256, ExecutionEvidence, OracleSuite, RunStatus};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

const MODEL: &str = "claude-sonnet-4-6";
const MODEL_KEY: &str = "scenario-model-fixture-key";
const A_KEY: &str = "scenario-a-apify-fixture-key";
const B_KEY: &str = "scenario-b-apify-fixture-key";
const ORACLE_CANARY: &str = "PRIVATE_SCENARIO_ORACLE_568ab";
const EXAMPLE_CANARY: &str = "PUBLIC_SCENARIO_HTML_NOT_IN_REPORT_784be";
const SOURCE_CANARY: &str = "CANNED_GENERATED_SOURCE_NOT_IN_REPORT_20b43";
const PROVIDER_CANARY: &str = "PRIVATE_FAILED_PROVIDER_BODY_a58df";

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, task }
    }
    async fn start_org(app: AppState) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
        Self { url, task }
    }
    async fn stop(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone, Debug)]
struct OrgRequest {
    method: String,
    path: String,
}
impl OrgRequest {
    fn manual_advancement(&self) -> bool {
        self.method == "POST"
            && matches!(
                self.path.trim_end_matches('/').rsplit('/').next(),
                Some("step" | "verify")
            )
    }
}
#[derive(Clone)]
struct GuardedProxy {
    upstream: String,
    client: reqwest::Client,
    requests: Arc<Mutex<Vec<OrgRequest>>>,
}
async fn guarded_proxy(State(proxy): State<GuardedProxy>, request: Request) -> Response {
    let observed = OrgRequest {
        method: request.method().to_string(),
        path: request.uri().path().to_owned(),
    };
    let forbidden = observed.manual_advancement();
    proxy.requests.lock().unwrap().push(observed);
    if forbidden {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"error":"scenario must use automatic progression"})),
        )
            .into_response();
    }
    let (mut parts, body) = request.into_parts();
    parts.headers.remove("host");
    let body = to_bytes(body, 2 * 1024 * 1024).await.unwrap();
    let response = proxy
        .client
        .request(parts.method, format!("{}{}", proxy.upstream, parts.uri))
        .headers(parts.headers)
        .body(body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    assert!(bytes.len() <= 2 * 1024 * 1024);
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap()
}

fn suite(side: &str) -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.suite_id = format!("scenario-{side}-independent-suite");
    suite.scope = format!("{ORACLE_CANARY}-{side}: synthetic fixture scope only");
    for (index, case) in suite.cases.iter_mut().enumerate() {
        case.case_id = format!("PRIVATE_CASE_{side}_{index}");
        if side == "b" {
            case.input["attendees"] = json!(case.input["attendees"].as_u64().unwrap() + 1);
        }
    }
    suite
}
fn fixture_outputs() -> Vec<Value> {
    let evidence: Vec<ExecutionEvidence> = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap();
    evidence.into_iter().map(|receipt| receipt.output).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    A,
    B,
}
#[derive(Clone, Copy)]
enum Behavior {
    RepairOnce,
    AmbiguousActor,
    WrongInvocation,
}
#[derive(Clone)]
struct Seen {
    side: Side,
    method: String,
    path: String,
    query: String,
    body: Value,
}
struct Actor {
    definition: Value,
    attempt: usize,
}
struct Run {
    actor_id: String,
    output: Value,
    side: Side,
}
struct World {
    behavior: Behavior,
    model_requests: Vec<Value>,
    requests: Vec<Seen>,
    actors: BTreeMap<String, Actor>,
    runs: BTreeMap<String, Run>,
    inputs_a: Vec<Value>,
    inputs_b: Vec<Value>,
    outputs: Vec<Value>,
}
impl World {
    fn build(&self, actor_id: &str) -> Value {
        let actor = &self.actors[actor_id];
        json!({"data":{"id":format!("build-{actor_id}"),"actId":actor_id,"buildNumber":"0.1.1",
            "status":"SUCCEEDED","actVersion":actor.definition["versions"][0]}})
    }
    fn run(&self, run_id: &str) -> Value {
        let run = &self.runs[run_id];
        json!({"data":{"id":run_id,"actId":run.actor_id,"buildId":format!("build-{}",run.actor_id),
            "status":"SUCCEEDED","defaultDatasetId":format!("dataset-{run_id}")}})
    }
    fn reply(&mut self, seen: Seen) -> (StatusCode, Value) {
        self.requests.push(seen.clone());
        let parts: Vec<_> = seen.path.trim_start_matches('/').split('/').collect();
        match (seen.method.as_str(), parts.as_slice()) {
            ("POST", ["v2", "actors"]) => {
                assert_eq!(seen.side, Side::A, "B must never generate another Actor");
                if matches!(self.behavior, Behavior::AmbiguousActor) {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error":PROVIDER_CANARY}),
                    );
                }
                assert_eq!(seen.body["isPublic"], false);
                assert_eq!(seen.body["actorPermissionLevel"], "LIMITED_PERMISSIONS");
                let attempt = self.actors.len();
                let id = format!("actor-scenario-{attempt}");
                let mut created = seen.body.clone();
                created["id"] = json!(id);
                self.actors.insert(
                    id,
                    Actor {
                        definition: seen.body,
                        attempt,
                    },
                );
                (StatusCode::CREATED, json!({"data":created}))
            }
            ("GET", ["v2", "actors", id, "versions", version]) if self.actors.contains_key(*id) => {
                let source = &self.actors[*id].definition["versions"][0];
                assert_eq!(source["versionNumber"], *version);
                (StatusCode::OK, json!({"data":source}))
            }
            ("POST", ["v2", "actors", id, "builds"]) if self.actors.contains_key(*id) => {
                assert_eq!(seen.side, Side::A);
                assert!(seen.query.contains("useCache=false"));
                (StatusCode::CREATED, self.build(id))
            }
            ("GET", ["v2", "actor-builds", build]) => {
                let Some(id) = build
                    .strip_prefix("build-")
                    .filter(|id| self.actors.contains_key(*id))
                else {
                    return (StatusCode::NOT_FOUND, json!({"error":"unknown build"}));
                };
                (StatusCode::OK, self.build(id))
            }
            ("POST", ["v2", "actors", id, "runs"]) if self.actors.contains_key(*id) => {
                assert!(seen.query.contains("build=0.1.1"));
                assert!(seen
                    .query
                    .contains("forcePermissionLevel=LIMITED_PERMISSIONS"));
                let inputs = if seen.side == Side::A {
                    &self.inputs_a
                } else {
                    &self.inputs_b
                };
                let Some(case) = inputs.iter().position(|input| *input == seen.body) else {
                    return (
                        StatusCode::BAD_REQUEST,
                        json!({"error":"unconfigured fixture input"}),
                    );
                };
                let actor = &self.actors[*id];
                let mut output = self.outputs[case].clone();
                // One semantic error in the first candidate triggers the actual
                // verifier and bounded repair, not a forged failed phase.
                if matches!(self.behavior, Behavior::RepairOnce) && actor.attempt == 0 && case == 0
                {
                    output["venues"][0]["capacity"] = json!(9999);
                }
                // B first accepts every oracle case correctly, then receives
                // a wrong fresh answer in an otherwise exact successful run.
                if matches!(self.behavior, Behavior::WrongInvocation)
                    && seen.side == Side::B
                    && self.runs.values().filter(|run| run.side == Side::B).count()
                        >= self.inputs_b.len()
                {
                    output["venues"][0]["capacity"] = json!(9999);
                }
                let run_id = format!("run-scenario-{}", self.runs.len());
                self.runs.insert(
                    run_id.clone(),
                    Run {
                        actor_id: (*id).into(),
                        output,
                        side: seen.side,
                    },
                );
                (StatusCode::CREATED, self.run(&run_id))
            }
            ("GET", ["v2", "actor-runs", id]) if self.runs.contains_key(*id) => {
                assert_eq!(self.runs[*id].side, seen.side, "organization run ownership");
                (StatusCode::OK, self.run(id))
            }
            ("GET", ["v2", "datasets", dataset, "items"]) => {
                let Some(id) = dataset
                    .strip_prefix("dataset-")
                    .filter(|id| self.runs.contains_key(*id))
                else {
                    return (StatusCode::NOT_FOUND, json!({"error":"unknown dataset"}));
                };
                assert_eq!(self.runs[id].side, seen.side);
                (StatusCode::OK, json!([self.runs[id].output]))
            }
            _ => (
                StatusCode::NOT_FOUND,
                json!({"error":"unexpected fixture route"}),
            ),
        }
    }
}

async fn provider_reply(
    State(world): State<Arc<Mutex<World>>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    let model = path == "/v1/messages";
    let side = if model {
        assert_eq!(request.headers()["x-api-key"], MODEL_KEY);
        Side::A
    } else {
        match request.headers()["authorization"].to_str().unwrap() {
            value if value == format!("Bearer {A_KEY}") => Side::A,
            value if value == format!("Bearer {B_KEY}") => Side::B,
            _ => panic!("fixture requires a separately configured organization credential"),
        }
    };
    let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let mut world = world.lock().unwrap();
    if model {
        assert_eq!(method, "POST");
        assert_eq!(body["model"], MODEL);
        let attempt = world.model_requests.len();
        world.model_requests.push(body);
        assert!(attempt < 2, "one generation plus at most one repair");
        return (
            StatusCode::OK,
            Json(json!({
                "id":format!("msg_scenario_{attempt}"),"type":"message","role":"assistant","model":MODEL,
                "content":[{"type":"text","text":json!({"extractor_typescript":format!("// {SOURCE_CANARY}\nexport function extract() {{ throw new Error('canned provider attempt {attempt}, never executed locally'); }}\n")}).to_string()}],
                "stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":120,"output_tokens":80}
            })),
        );
    }
    let (status, reply) = world.reply(Seen {
        side,
        method,
        path,
        query,
        body,
    });
    (status, Json(reply))
}

struct Providers {
    server: Server,
    world: Arc<Mutex<World>>,
}
impl Providers {
    async fn start(behavior: Behavior) -> Self {
        let world = Arc::new(Mutex::new(World {
            behavior,
            model_requests: vec![],
            requests: vec![],
            actors: BTreeMap::new(),
            runs: BTreeMap::new(),
            inputs_a: suite("a")
                .cases
                .into_iter()
                .map(|case| case.input)
                .collect(),
            inputs_b: suite("b")
                .cases
                .into_iter()
                .map(|case| case.input)
                .collect(),
            outputs: fixture_outputs(),
        }));
        let server = Server::start(
            Router::new()
                .fallback(provider_reply)
                .with_state(world.clone()),
        )
        .await;
        Self { server, world }
    }
    fn apify(&self, side: Side) -> ApifyClient {
        ApifyClient::for_local_testing(
            if side == Side::A { A_KEY } else { B_KEY },
            RunLimits {
                request_timeout: Duration::from_secs(3),
                deadline: Duration::from_secs(5),
                poll_interval: Duration::from_millis(1),
                ..RunLimits::default()
            },
            &self.server.url,
        )
        .unwrap()
    }
    fn set(&self, side: Side) -> ProviderSet {
        ProviderSet::new()
            .with_dev_stub(DevStub::default())
            .with_executor(Arc::new(ApifyCapabilityExecutor::new(self.apify(side))))
    }
}

struct Org {
    server: Server,
    backend: Server,
    app: AppState,
    config: OrgConfig,
    requests: Arc<Mutex<Vec<OrgRequest>>>,
}
impl Org {
    async fn start(
        config: OrgConfig,
        providers: ProviderSet,
        requests: Arc<Mutex<Vec<OrgRequest>>>,
    ) -> Self {
        let app = build_org(&config, Some(providers)).unwrap();
        let backend = Server::start_org(app.clone()).await;
        let server = Server::start(
            Router::new()
                .fallback(guarded_proxy)
                .with_state(GuardedProxy {
                    upstream: backend.url.clone(),
                    client: reqwest::Client::builder()
                        .timeout(Duration::from_secs(35))
                        .redirect(reqwest::redirect::Policy::none())
                        .no_proxy()
                        .build()
                        .unwrap(),
                    requests: requests.clone(),
                }),
        )
        .await;
        Self {
            server,
            backend,
            app,
            config,
            requests,
        }
    }
    async fn restart(&mut self, providers: ProviderSet) {
        self.server.stop().await;
        self.backend.stop().await;
        *self = Self::start(self.config.clone(), providers, self.requests.clone()).await;
    }
    fn assert_no_manual_advancement(&self) {
        let requests = self.requests.lock().unwrap();
        assert!(
            !requests.is_empty(),
            "the guard must observe actual organization traffic"
        );
        assert!(
            requests.iter().all(|request| !request.manual_advancement()),
            "manual progression was attempted: {:?}",
            requests
                .iter()
                .filter(|request| request.manual_advancement())
                .collect::<Vec<_>>()
        );
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    _registry: Server,
    providers: Providers,
    a: Org,
    b: Org,
}
impl Fixture {
    async fn start(behavior: Behavior) -> Self {
        Self::start_with_automation(behavior, true).await
    }
    async fn start_with_automation(behavior: Behavior, automatic: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let registry = Server::start(registry::router(
            RegistryState::open_in_memory(
                parse_publishers("scenario_a:scenario_a-registry,scenario_b:scenario_b-registry"),
                vec![],
            )
            .unwrap(),
        ))
        .await;
        let providers = Providers::start(behavior).await;
        let mut orgs = Vec::new();
        for side in [Side::A, Side::B] {
            let letter = if side == Side::A { "a" } else { "b" };
            let id = format!("scenario_{letter}");
            let mut config = OrgConfig::dev(&id, &id);
            config.db = Some(directory.path().join(format!("{id}.db")));
            config.oracle_suites = vec![suite(letter)];
            config.registry_url = Some(registry.url.clone());
            config.registry_token = Some(format!("{id}-registry"));
            if side == Side::A {
                config.acquisition = Some(AcquisitionConfig {
                    automatic,
                    policy: AcquisitionPolicy {
                        model: ModelConfig {
                            model: MODEL.into(),
                            max_tokens: 1024,
                        },
                        quotas: AcquisitionQuotas::default(),
                        max_duration_seconds: 3600,
                    },
                    model: Some(Arc::new(
                        AnthropicGenerator::with_test_endpoint(
                            &format!("{}/v1/messages", providers.server.url),
                            MODEL_KEY,
                        )
                        .unwrap(),
                    )),
                    apify: Some(Arc::new(providers.apify(side))),
                });
            }
            orgs.push(Org::start(config, providers.set(side), Arc::default()).await);
        }
        let b = orgs.pop().unwrap();
        let a = orgs.pop().unwrap();
        Self {
            _directory: directory,
            _registry: registry,
            providers,
            a,
            b,
        }
    }
    fn spec(&self) -> ScenarioSpec {
        let a = suite("a");
        let b = suite("b");
        ScenarioSpec {
            progression: ScenarioProgression::Automatic,
            run_id: "inherited-mission-fixture".into(),
            suite_a_id: a.suite_id,
            suite_b_id: b.suite_id,
            candidate: CandidateIdentity {
                capability_id: "scenario-venue-reader".into(),
                capability_version: "1.0.0".into(),
                actor_name: "scenario-initial-actor".into(),
                source_version: "0.1".into(),
            },
            repair: Some(CandidateIdentity {
                capability_id: "scenario-venue-reader".into(),
                capability_version: "1.0.1".into(),
                actor_name: "scenario-repaired-actor".into(),
                source_version: "0.1".into(),
            }),
            allowed_source_urls: a.cases.iter().map(|case| case.source.url.clone()).collect(),
            examples: vec![PublicExample {
                url: a.cases[0].source.url.clone(),
                html: format!("<html>{EXAMPLE_CANARY}</html>"),
            }],
            invocation_input: b.cases[0].input.clone(),
            allow_development_roles: true,
            provider_access_acknowledged: true,
        }
    }
    fn driver(&self) -> ScenarioDriver {
        ScenarioDriver::new(
            ScenarioEndpoint::new(&self.a.server.url, &self.a.config.tokens.operator).unwrap(),
            ScenarioEndpoint::new(&self.b.server.url, &self.b.config.tokens.operator).unwrap(),
            ScenarioLimits {
                request_timeout: Duration::from_secs(30),
                max_response_bytes: 2 * 1024 * 1024,
                poll_interval: Duration::from_millis(250),
                max_polls: 1024,
                // Debug builds rehash the complete generated package on every
                // transition; the repaired scenario validates two full passes.
                total_timeout: Duration::from_secs(240),
            },
        )
        .unwrap()
    }
}

async fn post(org: &Org, path: &str, body: Value) -> (StatusCode, Value) {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}/api/v1{path}", org.server.url))
        .bearer_auth(&org.config.tokens.operator)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

async fn get(org: &Org, path: &str) -> (StatusCode, Value) {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{}/api/v1{path}", org.server.url))
        .bearer_auth(&org.config.tokens.operator)
        .send()
        .await
        .unwrap();
    (response.status(), response.json().await.unwrap())
}

async fn wait_for_proof(org: &Org, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (status, proof) = get(org, &format!("/capability-runs/{id}/verification")).await;
            if status == StatusCode::OK {
                return proof;
            }
            assert_eq!(status, StatusCode::NOT_FOUND, "{proof}");
            let (status, finalization) =
                get(org, &format!("/capability-runs/{id}/finalization")).await;
            assert_eq!(status, StatusCode::OK, "{finalization}");
            assert_ne!(finalization["state"], "attention", "{finalization}");
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the actual server must collect and finalize the invocation without a manual request")
}

#[tokio::test]
async fn repaired_acquisition_survives_independent_adoption_and_both_role_handoffs() {
    let mut fixture = Fixture::start(Behavior::RepairOnce).await;
    let report = fixture
        .driver()
        .run(&fixture.spec())
        .await
        .unwrap_or_else(|error| {
            let world = fixture.providers.world.lock().unwrap();
            let tail: Vec<_> = world
                .requests
                .iter()
                .rev()
                .take(5)
                .map(|request| (&request.method, &request.path))
                .collect();
            panic!(
                "{error}; model calls={}, actors={}, runs={}, latest provider requests={tail:?}",
                world.model_requests.len(),
                world.actors.len(),
                world.runs.len()
            );
        });
    assert_eq!(report.progression, ScenarioProgression::Automatic);
    fixture.a.assert_no_manual_advancement();
    fixture.b.assert_no_manual_advancement();
    assert_eq!(report.acquisition_attempt, 1);
    assert_eq!(report.artifact.capability_version, "1.0.1");
    assert_eq!(report.acquisition_usage.model_calls, 2);
    assert_eq!(report.acquisition_usage.actor_creates, 2);
    assert_eq!(report.acquisition_usage.builds, 2);
    assert_eq!(report.acquisition_usage.test_submits, 6);
    assert_eq!(report.acquisition_usage.active, 0);
    assert_eq!(report.synthesis_calls_b, 0);
    assert!(report.acquisition_disabled_b);
    assert!(fixture.b.config.acquisition.is_none());
    assert!(fixture.b.config.forge_checkpoints.is_empty());
    assert_ne!(fixture.a.config.db, fixture.b.config.db);
    assert_ne!(
        fixture.a.config.tokens.operator,
        fixture.b.config.tokens.operator
    );
    let a = fixture.a.app.current_state().unwrap();
    let b = fixture.b.app.current_state().unwrap();
    assert!(fixture.a.app.store.commitments().unwrap().is_empty());
    assert!(fixture.b.app.store.commitments().unwrap().is_empty());
    assert_eq!(a.roles.len(), 3);
    assert_eq!(b.roles.len(), 2);
    for (state, old, new, responsibility) in [
        (
            &a,
            &report.producer_role_id,
            &report.producer_successor_id,
            &report.responsibility_a_id,
        ),
        (
            &b,
            &report.verifier_role_id,
            &report.monitor_role_id,
            &report.responsibility_b_id,
        ),
    ] {
        assert_eq!(state.roles[&RoleId::new(old)].state, RoleState::Retired);
        assert_eq!(state.roles[&RoleId::new(new)].state, RoleState::Active);
        assert_eq!(
            state.responsibilities[&responsibility.clone().into()].owner,
            Owner::Role(RoleId::new(new))
        );
    }
    for role_id in [
        &report.producer_role_id,
        &report.producer_successor_id,
        &report.monitor_role_id,
    ] {
        let binding = report
            .bindings
            .iter()
            .find(|binding| &binding.role_id == role_id)
            .unwrap();
        assert_eq!(binding.provider, Some(Provider::DevStub));
        assert!(binding
            .provider_agent_id
            .as_ref()
            .unwrap()
            .starts_with("devstub-"));
    }
    assert_eq!(
        report
            .bindings
            .iter()
            .filter(|binding| binding.provider == Some(Provider::Internal))
            .count(),
        2,
        "bootstrap roles retain their honest kernel binding"
    );
    let installation = InstallationId::new(&report.installation_b_id);
    assert_eq!(
        b.installations[&installation].status,
        InstallationStatus::Accepted
    );
    let local = fixture
        .b
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .unwrap();
    assert_eq!(local.suite_id, suite("b").suite_id);
    assert_eq!(local.report_ref, report.acceptance_b_ref);
    assert_eq!(local.job_ids.len(), 3);
    let mut native_ids = BTreeSet::new();
    for id in report
        .acceptance_b_jobs
        .iter()
        .chain([&report.initial_run_b_id, &report.continuation_run_b_id])
    {
        let record = fixture
            .b
            .app
            .store
            .get_capability_run(&JobId::new(id))
            .unwrap();
        assert_eq!(record.phase, CapabilityRunPhase::Collected);
        let evidence = record.evidence.unwrap();
        assert_eq!(evidence.artifact, report.artifact);
        assert!(native_ids.insert(evidence.run_id));
        let job = fixture.b.app.store.get_job(&JobId::new(id)).unwrap();
        assert_eq!(
            job.status,
            if report.acceptance_b_jobs.contains(id) {
                JobStatus::Verifying
            } else {
                JobStatus::Succeeded
            },
            "only fresh invocation proof finalizes an individual lookup"
        );
        assert!(fixture.a.app.store.get_job(&JobId::new(id)).is_err());
    }
    let before = fixture
        .b
        .app
        .store
        .get_job(&JobId::new(&report.initial_run_b_id))
        .unwrap();
    let after = fixture
        .b
        .app
        .store
        .get_job(&JobId::new(&report.continuation_run_b_id))
        .unwrap();
    assert_ne!(before.assignment_id, after.assignment_id);
    assert!(after.epoch > before.epoch);
    assert_eq!(after.role_id.as_str(), report.monitor_role_id);
    assert_eq!(before.input, suite("b").cases[0].input);
    assert_ne!(before.input, suite("a").cases[0].input);
    for (id, reference) in [
        (
            &report.initial_run_b_id,
            &report.initial_invocation_proof_ref,
        ),
        (
            &report.continuation_run_b_id,
            &report.continuation_invocation_proof_ref,
        ),
    ] {
        let (status, proof) = get(&fixture.b, &format!("/capability-runs/{id}/verification")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(proof["job_id"], *id);
        assert_eq!(proof["proof_ref"], *reference);
        assert_eq!(proof["decision"], "verified");
        assert_eq!(proof["acceptance_report_ref"], report.acceptance_b_ref);
        assert_eq!(proof["suite_id"], suite("b").suite_id);
        assert_eq!(
            proof["suite_sha256"],
            json_sha256(&serde_json::to_value(suite("b")).unwrap())
        );
        assert_eq!(proof["case_report"]["decision"], "accepted");
        assert_eq!(proof["case_report"]["extraction"], "supported");
        assert_eq!(
            proof["case_report"]["artifact"],
            serde_json::to_value(&report.artifact).unwrap()
        );
        let (status, replayed) =
            get(&fixture.b, &format!("/capability-runs/{id}/verification")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            replayed, proof,
            "completed proofs survive verifier retirement"
        );
    }
    assert_ne!(
        report.initial_invocation_proof_ref,
        report.continuation_invocation_proof_ref
    );

    let saved = fixture
        .a
        .app
        .acquisition_runtime
        .status(&report.acquisition_id)
        .unwrap();
    assert_eq!(saved.session().phase(), AcquisitionPhase::Verified);
    assert!(
        saved.authorize(&a, chrono::Utc::now()).is_err(),
        "retired producer cannot reacquire authority"
    );
    let serialized = serde_json::to_string(&report).unwrap();
    for secret in [
        MODEL_KEY,
        A_KEY,
        B_KEY,
        ORACLE_CANARY,
        EXAMPLE_CANARY,
        SOURCE_CANARY,
        "PRIVATE_CASE_",
        "extractor_typescript",
    ] {
        assert!(!serialized.contains(secret), "report leaked {secret}");
    }
    {
        let world = fixture.providers.world.lock().unwrap();
        assert_eq!(world.model_requests.len(), 2);
        assert_eq!(world.actors.len(), 2);
        assert_eq!(
            world
                .runs
                .values()
                .filter(|run| run.side == Side::A)
                .count(),
            6
        );
        assert_eq!(
            world
                .runs
                .values()
                .filter(|run| run.side == Side::B)
                .count(),
            5
        );
        assert!(world
            .runs
            .values()
            .filter(|run| run.side == Side::B)
            .all(|run| run.actor_id == report.artifact.actor_id));
        for request in &world.model_requests {
            let prompt = request.to_string();
            assert!(prompt.contains(EXAMPLE_CANARY));
            assert!(!prompt.contains(ORACLE_CANARY));
            assert!(!prompt.contains("PRIVATE_CASE_"));
        }
        assert!(world.model_requests[1]
            .to_string()
            .contains("value_mismatch"));
        for request in &world.requests {
            assert!(!request.body.to_string().contains(ORACLE_CANARY));
            assert!(!request.body.to_string().contains("PRIVATE_CASE_"));
            if request.side == Side::B && request.method == "POST" {
                assert_eq!(
                    request.path,
                    format!("/v2/actors/{}/runs", report.artifact.actor_id)
                );
                assert!(!request.body.to_string().contains("sourceFiles"));
            }
        }
    }
    // Retrying this operator workflow must refuse the populated organizations,
    // rather than create a second role, Actor or provider request.
    let request_count = fixture.providers.world.lock().unwrap().requests.len();
    assert!(matches!(
        fixture.driver().run(&fixture.spec()).await,
        Err(ScenarioError::Preflight(_))
    ));
    assert_eq!(
        fixture.providers.world.lock().unwrap().requests.len(),
        request_count
    );
    // Existing backend acceptance remains reusable after the verifier retires;
    // the retired verifier itself may not start a new invocation.
    let (status, _) = post(&fixture.b, "/capability-runs", json!({
        "client_request_id":"retired-verifier-replay","installation_id":report.installation_b_id,
        "role_id":report.verifier_role_id,"responsibility_id":report.responsibility_b_id,
        "suite_id":suite("b").suite_id,"input":suite("b").cases[0].input
    })).await;
    assert!(status.is_client_error());
    assert_eq!(
        fixture.providers.world.lock().unwrap().requests.len(),
        request_count
    );
    // Replace the HTTP server and open a fresh store connection using only B's
    // existing DB/config. No imported A checkpoint or acquisition client exists.
    fixture.b.restart(fixture.providers.set(Side::B)).await;
    let restarted = &fixture.b;
    let accepted = restarted
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .unwrap();
    assert_eq!(accepted.suite_id, local.suite_id);
    assert_eq!(accepted.report_ref, local.report_ref);
    assert_eq!(accepted.job_ids, local.job_ids);
    let (status, restored_proof) = get(
        restarted,
        &format!(
            "/capability-runs/{}/verification",
            report.continuation_run_b_id
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        restored_proof["proof_ref"],
        report.continuation_invocation_proof_ref
    );
    let (status, job) = post(restarted, "/capability-runs", json!({
        "client_request_id":"successor-after-restart","installation_id":report.installation_b_id,
        "role_id":report.monitor_role_id,"responsibility_id":report.responsibility_b_id,
        "suite_id":suite("b").suite_id,"input":suite("b").cases[0].input
    })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["assignment_id"], after.assignment_id.as_str());
    assert_eq!(job["epoch"], after.epoch);
    let id = job["job_id"].as_str().unwrap();
    let proof = wait_for_proof(restarted, id).await;
    let collected = restarted
        .app
        .store
        .get_capability_run(&JobId::new(id))
        .unwrap();
    assert_eq!(collected.phase, CapabilityRunPhase::Collected);
    let evidence = collected.evidence.unwrap();
    assert_eq!(evidence.artifact, report.artifact);
    assert!(native_ids.insert(evidence.run_id));
    assert_eq!(proof["decision"], "verified");
    assert_eq!(proof["case_report"]["extraction"], "supported");
    assert_ne!(proof["proof_ref"], report.continuation_invocation_proof_ref);
    let (status, persisted) = get(restarted, &format!("/capability-runs/{id}/verification")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(persisted, proof);
    assert_eq!(
        restarted.app.store.get_job(&JobId::new(id)).unwrap().status,
        JobStatus::Succeeded
    );
    assert!(restarted.app.store.commitments().unwrap().is_empty());
    fixture.a.assert_no_manual_advancement();
    fixture.b.assert_no_manual_advancement();
    let world = fixture.providers.world.lock().unwrap();
    assert_eq!(world.model_requests.len(), 2);
    assert_eq!(world.actors.len(), 2);
    assert_eq!(
        world
            .runs
            .values()
            .filter(|run| run.side == Side::B)
            .count(),
        6
    );
}

#[tokio::test]
async fn wrong_fresh_output_stops_after_transport_success_without_role_handoffs() {
    let fixture = Fixture::start(Behavior::WrongInvocation).await;
    let error = fixture.driver().run(&fixture.spec()).await.unwrap_err();
    assert!(
        matches!(
            error,
            ScenarioError::Stopped("fresh invocation did not verify")
        ),
        "{error}"
    );
    fixture.a.assert_no_manual_advancement();
    fixture.b.assert_no_manual_advancement();
    let job = fixture
        .b
        .app
        .store
        .find_capability_job("inherited-mission-fixture-initial-run")
        .unwrap()
        .unwrap();
    assert_eq!(job.status, JobStatus::ResultRejected);
    let record = fixture.b.app.store.get_capability_run(&job.job_id).unwrap();
    assert_eq!(record.phase, CapabilityRunPhase::Collected);
    let evidence = record.evidence.unwrap();
    assert_eq!(evidence.status, RunStatus::Succeeded);
    assert_eq!(evidence.artifact, record.request.artifact);
    assert_eq!(evidence.output["venues"][0]["capacity"], 9999);
    let (status, proof) = get(
        &fixture.b,
        &format!("/capability-runs/{}/verification", job.job_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(proof["decision"], "semantic_rejected");
    assert_eq!(proof["case_report"]["decision"], "rejected");
    assert_eq!(proof["case_report"]["issues"], json!(["value_mismatch"]));
    assert!(fixture
        .b
        .app
        .store
        .find_capability_job("inherited-mission-fixture-continuation")
        .unwrap()
        .is_none());
    for org in [&fixture.a, &fixture.b] {
        assert!(org.app.store.commitments().unwrap().is_empty());
        assert!(org
            .app
            .current_state()
            .unwrap()
            .roles
            .values()
            .all(|role| role.state == RoleState::Active));
    }
    assert!(fixture
        .b
        .app
        .current_state()
        .unwrap()
        .installations
        .values()
        .all(|installation| installation.status == InstallationStatus::Accepted));
    let jobs = fixture.b.app.store.list_jobs().unwrap();
    assert_eq!(jobs.len(), 4);
    assert_eq!(
        jobs.iter()
            .filter(|job| job.status == JobStatus::Verifying)
            .count(),
        3
    );
    let world = fixture.providers.world.lock().unwrap();
    assert_eq!(world.model_requests.len(), 1);
    assert_eq!(world.actors.len(), 1);
    assert_eq!(
        world
            .runs
            .values()
            .filter(|run| run.side == Side::B)
            .count(),
        4
    );
}

#[tokio::test]
async fn ambiguous_actor_creation_stops_without_retry_or_false_inheritance() {
    let mut fixture = Fixture::start(Behavior::AmbiguousActor).await;
    let error = fixture.driver().run(&fixture.spec()).await.unwrap_err();
    assert!(
        matches!(
            &error,
            ScenarioError::Observation {
                kind: "acquisition",
                ..
            }
        ),
        "{error}"
    );
    assert!(!error.to_string().contains(PROVIDER_CANARY));
    let ids = fixture.a.app.store.acquisition_ids().unwrap();
    assert_eq!(ids.len(), 1);
    if let ScenarioError::Observation { id, .. } = &error {
        assert_eq!(id, &ids[0]);
    }
    let record = fixture.a.app.acquisition_runtime.status(&ids[0]).unwrap();
    assert_eq!(
        record.session().phase(),
        AcquisitionPhase::NeedsReconciliation
    );
    let counters = fixture.a.app.store.acquisition_counters().unwrap();
    assert_eq!(counters.actor_creates, 1);
    assert_eq!(counters.model_calls, 1);
    let counts = serde_json::to_value(&counters).unwrap();
    let request_count = fixture.providers.world.lock().unwrap().requests.len();
    fixture.a.restart(fixture.providers.set(Side::A)).await;
    let (status, automation) = get(
        &fixture.a,
        &format!("/capability-acquisitions/{}/automation", ids[0]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{automation}");
    assert_eq!(automation["enabled"], true);
    assert_eq!(automation["state"], "inactive");
    // Let the restarted server scan repeatedly; an uncertain native POST must
    // remain inert even though automatic acquisition is still enabled.
    tokio::time::sleep(Duration::from_millis(1250)).await;
    let restored = fixture.a.app.acquisition_runtime.status(&ids[0]).unwrap();
    assert_eq!(
        restored.session().phase(),
        AcquisitionPhase::NeedsReconciliation
    );
    assert_eq!(
        serde_json::to_value(fixture.a.app.store.acquisition_counters().unwrap()).unwrap(),
        counts
    );
    fixture.a.assert_no_manual_advancement();
    fixture.b.assert_no_manual_advancement();
    let world = fixture.providers.world.lock().unwrap();
    assert_eq!(world.requests.len(), request_count);
    assert_eq!(world.model_requests.len(), 1);
    assert_eq!(
        world
            .requests
            .iter()
            .filter(|request| request.method == "POST" && request.path == "/v2/actors")
            .count(),
        1
    );
    assert!(world.requests.iter().all(|request| request.side == Side::A));
    assert!(fixture
        .b
        .app
        .current_state()
        .unwrap()
        .installations
        .is_empty());
}

#[tokio::test]
async fn automatic_scenario_refuses_disabled_automation_before_creating_missions() {
    let fixture = Fixture::start_with_automation(Behavior::RepairOnce, false).await;
    let (status, health) = get(&fixture.a, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["acquisition_enabled"], true);
    assert_eq!(health["automatic_acquisition_enabled"], false);
    let error = fixture.driver().run(&fixture.spec()).await.unwrap_err();
    assert!(matches!(error, ScenarioError::Preflight(_)), "{error}");
    for org in [&fixture.a, &fixture.b] {
        let state = org.app.current_state().unwrap();
        assert_eq!(state.revision, 0);
        assert!(state.missions.is_empty());
        assert!(state.roles.is_empty());
        org.assert_no_manual_advancement();
        assert!(org
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.method == "GET"));
    }
    assert!(fixture.a.app.store.acquisition_ids().unwrap().is_empty());
    let world = fixture.providers.world.lock().unwrap();
    assert!(world.model_requests.is_empty());
    assert!(world.requests.is_empty());
}

#[tokio::test]
async fn role_stub_and_private_actor_access_require_explicit_scenario_opt_in() {
    let fixture = Fixture::start(Behavior::RepairOnce).await;
    let mut spec = fixture.spec();
    spec.allow_development_roles = false;
    assert!(matches!(
        fixture.driver().run(&spec).await,
        Err(ScenarioError::Preflight(_))
    ));
    spec.allow_development_roles = true;
    spec.provider_access_acknowledged = false;
    assert!(fixture.driver().run(&spec).await.is_err());
    assert!(fixture.a.app.current_state().unwrap().missions.is_empty());
    assert!(fixture.b.app.current_state().unwrap().missions.is_empty());
    fixture.a.assert_no_manual_advancement();
    fixture.b.assert_no_manual_advancement();
    let world = fixture.providers.world.lock().unwrap();
    assert!(world.model_requests.is_empty());
    assert!(world.requests.is_empty());
}
