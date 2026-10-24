//! Complete HTTP scenario with two private SQLite stores and real Rust provider
//! adapters. Model output and provider execution are canned loopback fixtures;
//! this does not claim cloud execution, autonomous planning, or live gateway
//! readiness. Both synthetic provider credentials may access the same Actor.
//! Real organization servers own progression; an HTTP guard rejects all manual
//! step/verify calls and low-level discovery/installation/acceptance requests.
//! B submits one durable adoption intent; its server chooses and verifies the
//! foreign capability using B's own requirement and trusted suite.
//! The repaired success case also routes real ElasticClient traffic through a
//! recording proxy to fake Elasticsearch: shared registry retrieval and each
//! organization's evidence projection. This checks HTTP contracts and tenant
//! filtering, not cloud credentials or Elasticsearch relevance.
//! B's continuing monitor is created and run through the real CREAO adapter.
//! Readiness is an explicit fixture check of its completed creation journal,
//! not a live gateway callback, skill installation or sponsor eligibility proof.

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::capability::sha256_hex;
use genesis_core::engine::RoleToProvision;
use genesis_core::ids::{InstallationId, JobId, RoleId};
use genesis_core::lifecycle::{InstallationStatus, JobStatus, RoleState};
use genesis_core::model::{AgentBinding, Owner, Provider};
use genesis_creao::{
    AccountLimiter, CreaoClient, Limits, OperationPhase, OperationPurpose, QuotaLimits, Runtime,
    RuntimeStore,
};
use genesis_elastic::{ElasticClient, ElasticConfig};
use genesis_forge::{
    AcquisitionPhase, AnthropicGenerator, CandidateIdentity, ModelConfig, PublicExample,
};
use genesis_providers::{
    ApifyCapabilityExecutor, CreaoSpecialist, DevStub, ProviderError, ProviderSet,
    SpecialistRuntime,
};
use genesis_server::app::AppState;
use genesis_server::capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas};
use genesis_server::capability_runtime::CapabilityRunPhase;
use genesis_server::capability_scenario::{
    ScenarioDriver, ScenarioEndpoint, ScenarioError, ScenarioLimits, ScenarioProgression,
    ScenarioReport, ScenarioSpec, ScenarioVenueMonitoring,
};
use genesis_server::registry::{self, RegistryState};
use genesis_server::specialist::{Readiness, ReadinessVerifier};
use genesis_server::venue_assessment_domain::{AssessmentVerdict, VenueAssessmentPolicy};
use genesis_server::{build_org, parse_publishers, serve_org, AcquisitionConfig, OrgConfig};
use genesis_verification::{
    decode_json, json_sha256, ExecutionEvidence, ExtractionOutcome, OracleSuite, RunStatus,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
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
const ELASTIC_KEY: &str = "scenario-elastic-fixture-key";
const CREAO_KEY: &str = "cr_sk_scenario_b_fixture_key";
const CUT_URL: &str = "https://fixtures.genesis.example/venues/capacity-cut.html";

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
    body: Value,
    adoption_response: Option<Value>,
}
impl OrgRequest {
    fn forbidden_automatic_call(&self) -> bool {
        let path = self.path.trim_end_matches('/');
        self.method == "POST"
            && (matches!(path.rsplit('/').next(), Some("step" | "verify"))
                || matches!(
                    path,
                    "/api/v1/capabilities/search" | "/api/v1/capability-installations"
                )
                || (path.starts_with("/api/v1/capability-installations/")
                    && matches!(path.rsplit('/').next(), Some("checks" | "acceptance"))))
    }
}
#[derive(Clone)]
struct GuardedProxy {
    upstream: String,
    client: reqwest::Client,
    requests: Arc<Mutex<Vec<OrgRequest>>>,
}
async fn guarded_proxy(State(proxy): State<GuardedProxy>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    let body = to_bytes(body, 2 * 1024 * 1024).await.unwrap();
    let observed = OrgRequest {
        method: parts.method.to_string(),
        path: parts.uri.path().to_owned(),
        body: if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
        adoption_response: None,
    };
    let forbidden = observed.forbidden_automatic_call();
    let is_adoption_observation =
        observed.method == "GET" && observed.path.starts_with("/api/v1/capability-adoptions/");
    let request_index = {
        let mut requests = proxy.requests.lock().unwrap();
        let index = requests.len();
        requests.push(observed);
        index
    };
    if forbidden {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"error":"scenario must let the server own acquisition, adoption, and finalization"})),
        ).into_response();
    }
    parts.headers.remove("host");
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
    if is_adoption_observation && status.is_success() {
        proxy.requests.lock().unwrap()[request_index].adoption_response =
            Some(serde_json::from_slice(&bytes).unwrap());
    }
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap()
}

#[derive(Debug)]
struct ElasticSearch {
    path: String,
    body: Value,
    status: StatusCode,
    response: Value,
}

#[derive(Clone)]
struct ElasticProxy {
    upstream: String,
    client: reqwest::Client,
    searches: Arc<Mutex<Vec<ElasticSearch>>>,
}

async fn elastic_proxy(State(proxy): State<ElasticProxy>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    let body = to_bytes(body, 2 * 1024 * 1024).await.unwrap();
    let search = (parts.method == "POST" && parts.uri.path().ends_with("/_search")).then(|| {
        (
            parts.uri.path().to_owned(),
            serde_json::from_slice::<Value>(&body).unwrap(),
        )
    });
    assert_eq!(
        parts.headers["authorization"].to_str().unwrap(),
        format!("ApiKey {ELASTIC_KEY}")
    );
    parts.headers.remove("host");
    // Bulk indexing is NDJSON. Forward its bytes/content-type unchanged and
    // record only the JSON search calls that establish retrieval actually ran.
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
    if let Some((path, body)) = search {
        proxy.searches.lock().unwrap().push(ElasticSearch {
            path,
            body,
            status,
            response: serde_json::from_slice(&bytes).unwrap(),
        });
    }
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap()
}

struct ElasticFixture {
    _proxy: Server,
    client: ElasticClient,
    shared: genesis_elastic::fake::Shared,
    searches: Arc<Mutex<Vec<ElasticSearch>>>,
}

impl ElasticFixture {
    async fn start() -> Self {
        let (upstream, shared) = genesis_elastic::fake::start().await;
        let searches = Arc::default();
        let proxy = Server::start(
            Router::new()
                .fallback(elastic_proxy)
                .with_state(ElasticProxy {
                    upstream: upstream.elasticsearch_url().to_owned(),
                    client: reqwest::Client::builder()
                        .timeout(Duration::from_secs(5))
                        .redirect(reqwest::redirect::Policy::none())
                        .no_proxy()
                        .build()
                        .unwrap(),
                    searches: Arc::clone(&searches),
                }),
        )
        .await;
        let client = ElasticClient::new(ElasticConfig {
            elasticsearch_url: proxy.url.clone(),
            api_key: ELASTIC_KEY.into(),
            index_prefix: "scenario".into(),
            timeout: Duration::from_secs(5),
        })
        .unwrap();
        Self {
            _proxy: proxy,
            client,
            shared,
            searches,
        }
    }
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

fn monitoring_suite() -> OracleSuite {
    let mut suite = suite("b");
    let baseline = include_str!("../../genesis-verification/examples/fixtures/portal/table.html");
    let updated = baseline.replacen("<td>180</td>", "<td>100</td>", 1);
    assert_ne!(baseline, updated);
    let mut cut = suite.cases[0].clone();
    cut.case_id = "PRIVATE_CASE_b_capacity_cut".into();
    cut.input["source_url"] = json!(CUT_URL);
    cut.source.url = CUT_URL.into();
    cut.source.sha256 = sha256_hex(updated.as_bytes());
    if let ExtractionOutcome::Supported { venues } = &mut cut.expected {
        for venue in venues {
            venue.evidence.source_url = CUT_URL.into();
            venue.evidence.snapshot_sha256 = cut.source.sha256.clone();
            if venue.venue_id == "grand-hall" {
                venue.capacity = 100;
            }
        }
    } else {
        panic!("the second snapshot must retain the supported baseline layout");
    }
    suite.cases.push(cut);
    suite
}

fn monitoring_policy() -> VenueAssessmentPolicy {
    serde_json::from_value(json!({"selected_venue_id":"grand-hall","attendees":121,
        "budget":{"minor_units":80000,"currency":"USD"},
        "window":{"start":"2026-11-14T18:00:00-08:00","end":"2026-11-14T22:00:00-08:00"},
        "step_free_required":true}))
    .unwrap()
}

struct RecordedCreationReadiness(RuntimeStore);
#[async_trait]
impl ReadinessVerifier for RecordedCreationReadiness {
    async fn verify(
        &self,
        role: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        let record = self
            .0
            .list()
            .unwrap()
            .into_iter()
            .find(|record| {
                record.spec.role_id == role.role_id.as_str()
                    && record.phase == OperationPhase::NativeCompleted
                    && matches!(&record.spec.purpose, OperationPurpose::Provision {config_digest,..}
                    if *config_digest == binding.config_digest)
                    && record
                        .observation
                        .as_ref()
                        .and_then(|o| o.agent_id.as_deref())
                        == Some(binding.provider_agent_id.as_str())
            })
            .expect("fixture readiness requires the actual completed creation journal");
        Ok(Readiness::Ready {
            evidence_ref: format!("fixture-created-agent:{}", record.spec.operation_id),
        })
    }
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
    creao_requests: Vec<(String, String, Value)>,
    creao_runs: BTreeMap<String, Value>,
    creao_agents: BTreeSet<String>,
}
impl World {
    fn creao_reply(&mut self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.creao_requests
            .push((method.into(), path.into(), body.clone()));
        assert!(!body.to_string().contains(ORACLE_CANARY));
        assert!(!body.to_string().contains("PRIVATE_CASE_"));
        if method == "POST" && path == "/v1/agents" {
            let agent = uuid::Uuid::new_v4().to_string();
            let run = uuid::Uuid::new_v4().to_string();
            let conversation = uuid::Uuid::new_v4().to_string();
            assert!(body["input"]
                .as_str()
                .unwrap()
                .contains("Persistent venue monitor"));
            self.creao_agents.insert(agent.clone());
            self.creao_runs.insert(run.clone(), json!({"run_id":run,"status":"completed","agent_id":null,"model":null,
                "conversation_id":conversation,"result":{"agent_id":agent,"agent_name":"Loopback monitor","text":"Created"}}));
            return (
                StatusCode::ACCEPTED,
                json!({"run_id":run,"status":"pending","agent_id":null,"model":null,"conversation_id":conversation}),
            );
        }
        if method == "POST" && path == "/v1/runs" {
            let agent = body["agent_id"].as_str().unwrap();
            assert!(
                self.creao_agents.contains(agent),
                "execution must use a previously created native agent"
            );
            assert_eq!(body["should_notify"], false);
            assert!(body.get("conversation_id").is_none());
            let context = &body["input"]["input"]["context"];
            assert!(context.is_object());
            assert_eq!(context["policy"], json!(monitoring_policy()));
            let capacity = context["venue"]["capacity"].as_u64().unwrap();
            assert!(matches!(capacity, 180 | 100));
            // Two canned fixture answers, not another implementation of the verifier.
            let output = json!({"schema_version":1,"assessment_id":context["assessment_id"],
                "source_job_id":context["source_job_id"],"invocation_proof_ref":context["invocation_proof_ref"],
                "policy_sha256":context["policy_sha256"],"selected_venue_id":"grand-hall",
                "evidence":context["venue"]["evidence"],
                "checks":{"capacity":capacity==180,"price":true,"window":true,"availability":true,"step_free":true},
                "verdict":if capacity==180 {"feasible"}else{"reopen"}});
            let run = uuid::Uuid::new_v4().to_string();
            let conversation = uuid::Uuid::new_v4().to_string();
            self.creao_runs.insert(
                run.clone(),
                json!({"run_id":run,"status":"completed","agent_id":agent,"model":null,
                "conversation_id":conversation,"result":{"text":output.to_string()}}),
            );
            return (
                StatusCode::ACCEPTED,
                json!({"run_id":run,"status":"pending","agent_id":agent,"model":null,"conversation_id":conversation}),
            );
        }
        if let ("GET", Some(run)) = (method, path.strip_prefix("/v1/runs/")) {
            return (
                StatusCode::OK,
                self.creao_runs.get(run).expect("known native run").clone(),
            );
        }
        panic!("unexpected loopback CREAO request {method} {path}");
    }
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
    let creao = path.starts_with("/v1/") && !model;
    let side = if model {
        assert_eq!(request.headers()["x-api-key"], MODEL_KEY);
        Side::A
    } else if creao {
        assert_eq!(
            request.headers()["authorization"],
            format!("Bearer {CREAO_KEY}")
        );
        Side::B
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
    if creao {
        let (status, reply) = world.creao_reply(&method, &path, body);
        return (status, Json(reply));
    }
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
    creao_directory: Option<PathBuf>,
}
impl Providers {
    async fn start(behavior: Behavior, monitoring: bool, directory: &Path) -> Self {
        let b = if monitoring {
            monitoring_suite()
        } else {
            suite("b")
        };
        let mut outputs = fixture_outputs();
        if monitoring {
            outputs.push(serde_json::to_value(&b.cases[3].expected).unwrap());
        }
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
            inputs_b: b.cases.into_iter().map(|case| case.input).collect(),
            outputs,
            creao_requests: vec![],
            creao_runs: BTreeMap::new(),
            creao_agents: BTreeSet::new(),
        }));
        let server = Server::start(
            Router::new()
                .fallback(provider_reply)
                .with_state(world.clone()),
        )
        .await;
        Self {
            server,
            world,
            creao_directory: monitoring.then(|| directory.to_owned()),
        }
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
        let set = ProviderSet::new()
            .with_dev_stub(DevStub::default())
            .with_executor(Arc::new(ApifyCapabilityExecutor::new(self.apify(side))));
        if side == Side::B && self.creao_directory.is_some() {
            set.with_specialist(Arc::new(self.specialist()))
        } else {
            set
        }
    }
    fn journal(&self) -> RuntimeStore {
        RuntimeStore::open(
            self.creao_directory
                .as_ref()
                .unwrap()
                .join("scenario_b.creao.db"),
            "scenario_b",
        )
        .unwrap()
    }
    fn specialist(&self) -> CreaoSpecialist {
        let client = CreaoClient::for_local_testing(
            CREAO_KEY,
            Limits {
                request_timeout: Duration::from_secs(3),
                ..Default::default()
            },
            AccountLimiter::open(
                self.creao_directory
                    .as_ref()
                    .unwrap()
                    .join("creao-quota.db"),
                "scenario-b-creao-account",
                QuotaLimits::default(),
            )
            .unwrap(),
            &format!("{}/v1", self.server.url),
        )
        .unwrap();
        CreaoSpecialist::new(
            Runtime::new(client, self.journal(), Duration::from_millis(1)).unwrap(),
        )
    }
}

struct Org {
    server: Server,
    backend: Server,
    app: AppState,
    config: OrgConfig,
    requests: Arc<Mutex<Vec<OrgRequest>>>,
    elastic: Option<ElasticClient>,
}
impl Org {
    async fn start(
        config: OrgConfig,
        providers: ProviderSet,
        requests: Arc<Mutex<Vec<OrgRequest>>>,
        elastic: Option<ElasticClient>,
    ) -> Self {
        let native_creao = providers
            .configured_specialists()
            .contains(&Provider::Creao);
        let mut app = build_org(&config, Some(providers)).unwrap();
        if native_creao {
            let journal = RuntimeStore::open(
                config.db.as_ref().unwrap().with_extension("creao.db"),
                config.org_id.as_str(),
            )
            .unwrap();
            app = app.with_readiness_verifier(
                Provider::Creao,
                Arc::new(RecordedCreationReadiness(journal)),
            );
        }
        if let Some(client) = &elastic {
            app = app.with_elastic(client.clone());
        }
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
            elastic,
        }
    }
    async fn restart(&mut self, providers: ProviderSet) {
        self.server.stop().await;
        self.backend.stop().await;
        *self = Self::start(
            self.config.clone(),
            providers,
            self.requests.clone(),
            self.elastic.clone(),
        )
        .await;
    }
    fn adoption_request(&self) -> Value {
        let requests = self.requests.lock().unwrap();
        let adoption: Vec<_> = requests
            .iter()
            .filter(|request| {
                request.method == "POST" && request.path == "/api/v1/capability-adoptions"
            })
            .collect();
        assert_eq!(
            adoption.len(),
            1,
            "B should submit one durable adoption intent"
        );
        let body = adoption[0].body.clone();
        let keys: BTreeSet<_> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let required: BTreeSet<_> = [
            "client_request_id",
            "mission_id",
            "requirement_id",
            "role_id",
            "responsibility_id",
            "suite_id",
            "provider_access_acknowledged",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            keys, required,
            "the scenario must not supply A's identity, artifact, or search query"
        );
        assert_eq!(body["provider_access_acknowledged"], true);
        assert_eq!(body["suite_id"], suite("b").suite_id);
        assert!(body["client_request_id"]
            .as_str()
            .is_some_and(|key| !key.is_empty()));
        body
    }
    fn assert_server_owned_progression(&self) {
        let requests = self.requests.lock().unwrap();
        assert!(
            !requests.is_empty(),
            "the guard must observe actual organization traffic"
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.forbidden_automatic_call()),
            "client attempted an operation owned by the automatic server: {:?}",
            requests
                .iter()
                .filter(|request| request.forbidden_automatic_call())
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
    elastic: Option<ElasticFixture>,
}
impl Fixture {
    async fn start(behavior: Behavior) -> Self {
        Self::start_with_automation(behavior, true).await
    }
    async fn start_with_automation(behavior: Behavior, automatic: bool) -> Self {
        Self::start_configured(behavior, automatic, false).await
    }
    async fn start_with_elastic(behavior: Behavior) -> Self {
        Self::start_configured(behavior, true, true).await
    }
    async fn start_configured(behavior: Behavior, automatic: bool, with_elastic: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let elastic = if with_elastic {
            Some(ElasticFixture::start().await)
        } else {
            None
        };
        let mut registry_state = RegistryState::open_in_memory(
            parse_publishers("scenario_a:scenario_a-registry,scenario_b:scenario_b-registry"),
            vec![],
        )
        .unwrap();
        if let Some(elastic) = &elastic {
            registry_state = registry_state.with_elastic(elastic.client.clone());
        }
        let registry = Server::start(registry::router(registry_state)).await;
        // Only the repaired full-composition fixture enables native monitoring;
        // the other four scenarios keep their original three-case contracts.
        let providers = Providers::start(behavior, with_elastic, directory.path()).await;
        let mut orgs = Vec::new();
        for side in [Side::A, Side::B] {
            let letter = if side == Side::A { "a" } else { "b" };
            let id = format!("scenario_{letter}");
            let mut config = OrgConfig::dev(&id, &id);
            config.db = Some(directory.path().join(format!("{id}.db")));
            config.oracle_suites = vec![if with_elastic && side == Side::B {
                monitoring_suite()
            } else {
                suite(letter)
            }];
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
            orgs.push(
                Org::start(
                    config,
                    providers.set(side),
                    Arc::default(),
                    elastic.as_ref().map(|elastic| elastic.client.clone()),
                )
                .await,
            );
        }
        let b = orgs.pop().unwrap();
        let a = orgs.pop().unwrap();
        Self {
            _directory: directory,
            _registry: registry,
            providers,
            a,
            b,
            elastic,
        }
    }
    fn spec(&self) -> ScenarioSpec {
        let a = suite("a");
        let b = self.b.config.oracle_suites[0].clone();
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
            allowed_source_urls: a
                .cases
                .iter()
                .chain(&b.cases)
                .map(|case| case.source.url.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            examples: vec![PublicExample {
                url: a.cases[0].source.url.clone(),
                html: format!("<html>{EXAMPLE_CANARY}</html>"),
            }],
            invocation_input: b.cases[0].input.clone(),
            venue_monitoring: self.providers.creao_directory.as_ref().map(|_| {
                ScenarioVenueMonitoring {
                    policy: monitoring_policy(),
                    updated_invocation_input: b.cases[3].input.clone(),
                }
            }),
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

async fn assert_own_evidence_is_searchable(org: &Org, requirement_id: &str, foreign_mission: &str) {
    let state = org.app.current_state().unwrap();
    let doc_id = format!("{}:requirement:{requirement_id}", state.org_id);
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (status, projection) = get(org, "/projection").await;
            assert_eq!(status, StatusCode::OK, "{projection}");
            assert_eq!(projection["configured"], true);
            if projection["projected_revision"] == state.revision {
                assert!(projection["last_error"].is_null(), "{projection}");
                assert!(projection["documents"]
                    .as_u64()
                    .is_some_and(|count| count > 0));
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("the actual organization worker must project its committed revision");
    let (status, result) = get(org, "/search/evidence?q=venue&kinds=requirement&limit=50").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["projected_revision"], state.revision);
    let hits = result["hits"].as_array().unwrap();
    assert!(
        hits.iter().any(|hit| hit["doc"]["doc_id"] == doc_id),
        "{result}"
    );
    assert!(
        hits.iter()
            .all(|hit| hit["doc"]["org_id"] == state.org_id.as_str()),
        "{result}"
    );
    let (status, foreign) = get(
        org,
        &format!("/search/evidence?q=venue&kinds=requirement&mission_id={foreign_mission}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{foreign}");
    assert!(
        foreign["hits"].as_array().unwrap().is_empty(),
        "an org token must not search the other organization's projected mission: {foreign}"
    );
}

async fn assert_native_monitoring(fixture: &Fixture, report: &ScenarioReport) -> Vec<Value> {
    let monitoring = json!(report
        .venue_monitoring
        .as_ref()
        .expect("two assessments are required"));
    assert_eq!(monitoring["monitor_role_id"], report.monitor_role_id);
    assert_eq!(
        monitoring["policy_sha256"],
        json_sha256(&json!(monitoring_policy()))
    );
    let state = fixture.b.app.current_state().unwrap();
    let role = &state.roles[&RoleId::new(&report.monitor_role_id)];
    let binding = role.binding.as_ref().unwrap();
    assert_eq!(binding.provider, Provider::Creao);
    assert_eq!(monitoring["provider_agent_id"], binding.provider_agent_id);
    let specialist = fixture.providers.specialist();
    let mut records = Vec::new();
    let mut assignment = None;
    let mut native_runs = BTreeSet::new();
    for (index, label, verdict) in [
        (0, "baseline", AssessmentVerdict::Feasible),
        (3, "updated", AssessmentVerdict::Reopen),
    ] {
        let summary = &monitoring[label];
        let id = summary["assessment_id"].as_str().unwrap();
        let (status, record) = get(&fixture.b, &format!("/venue-assessments/{id}")).await;
        assert_eq!(status, StatusCode::OK, "{record}");
        assert_eq!(record["status"], "SUCCEEDED");
        assert_eq!(record["policy"], json!(monitoring_policy()));
        assert_eq!(record["role_id"], report.monitor_role_id);
        assert_eq!(record["responsibility_id"], report.responsibility_b_id);
        assert_eq!(record["job_id"], summary["job_id"]);
        assert_eq!(record["source_job_id"], summary["source_job_id"]);
        let proof = &record["proof"];
        assert_eq!(proof["decision"], "accepted");
        assert_eq!(proof["output"]["verdict"], json!(verdict));
        assert_eq!(summary["verdict"], json!(verdict));
        for (public, full) in [
            ("proof_ref", "proof_ref"),
            ("invocation_proof_ref", "invocation_proof_ref"),
            ("native_run_id", "run_id"),
            ("receipt_sha256", "execution_receipt_sha256"),
            ("observation_id", "observation_id"),
            ("requirement_id", "requirement_id"),
        ] {
            assert_eq!(summary[public], proof[full]);
        }
        let job = fixture
            .b
            .app
            .store
            .get_job(&JobId::new(summary["job_id"].as_str().unwrap()))
            .unwrap();
        assert_eq!(job.role_id, role.role_id);
        let current = (job.assignment_id.clone(), job.epoch);
        if let Some(previous) = &assignment {
            assert_eq!(previous, &current)
        } else {
            assignment = Some(current)
        }
        let receipt = specialist.execution_receipt(&job, binding).unwrap();
        assert_eq!(
            proof["execution_receipt_sha256"],
            json_sha256(&json!(receipt))
        );
        assert_eq!(proof["run_id"], receipt.run_id);
        assert_eq!(receipt.provider_agent_id, binding.provider_agent_id);
        assert!(native_runs.insert(receipt.run_id));
        let source_id = summary["source_job_id"].as_str().unwrap();
        let source = fixture.b.app.store.get_job(&JobId::new(source_id)).unwrap();
        assert_eq!(source.input, monitoring_suite().cases[index].input);
        assert_eq!(source.role_id, role.role_id);
        assert_eq!(source.assignment_id, job.assignment_id);
        assert_eq!(source.epoch, job.epoch);
        let (status, source_proof) = get(
            &fixture.b,
            &format!("/capability-runs/{source_id}/verification"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(source_proof["decision"], "verified");
        assert_eq!(source_proof["proof_ref"], summary["invocation_proof_ref"]);
        assert_eq!(
            source_proof["case_report"]["artifact"],
            json!(report.artifact)
        );
        let observation = &state.observations[&proof["observation_id"].as_str().unwrap().into()];
        assert_eq!(
            observation.evidence_ref.as_deref(),
            proof["proof_ref"].as_str()
        );
        if index == 0 {
            assert_eq!(source_id, report.continuation_run_b_id);
            assert!(proof["requirement_id"].is_null());
        } else {
            let requirement =
                &state.requirements[&proof["requirement_id"].as_str().unwrap().into()];
            assert_eq!(requirement.owner, Owner::MissionController);
            assert_eq!(
                requirement.status,
                genesis_core::lifecycle::RequirementStatus::Unmet
            );
            assert_eq!(
                requirement.triggering_evidence,
                vec![observation.observation_id.clone()]
            );
            assert!(observation.summary.contains("capacity 100 is below 121"));
            assert!(requirement.acceptance.contains("121 attendees"));
            assert!(requirement.acceptance.contains("80000 USD"));
        }
        records.push(record);
    }
    assert_ne!(
        monitoring["baseline"]["source_job_id"],
        monitoring["updated"]["source_job_id"]
    );
    assert_ne!(
        monitoring["baseline"]["job_id"],
        monitoring["updated"]["job_id"]
    );
    assert_eq!(
        state.roles.len(),
        2,
        "neither assessment may create another role"
    );
    assert_eq!(
        state.observations.len(),
        3,
        "one scenario observation plus two verified assessments"
    );
    assert_eq!(
        state.requirements.len(),
        2,
        "one original requirement plus one verified replacement"
    );
    assert!(fixture.b.app.store.commitments().unwrap().is_empty());
    let journal = fixture.providers.journal().list().unwrap();
    assert_eq!(journal.len(), 3, "one creation and two native executions");
    assert!(journal
        .iter()
        .all(|record| record.phase == OperationPhase::NativeCompleted));
    let world = fixture.providers.world.lock().unwrap();
    assert_eq!(world.creao_agents.len(), 1);
    assert_eq!(
        world
            .creao_requests
            .iter()
            .filter(|(method, path, _)| method == "POST" && path == "/v1/agents")
            .count(),
        1
    );
    let runs: Vec<_> = world
        .creao_requests
        .iter()
        .filter(|(method, path, _)| method == "POST" && path == "/v1/runs")
        .collect();
    assert_eq!(runs.len(), 2);
    assert!(runs
        .iter()
        .all(|(_, _, body)| body["agent_id"] == binding.provider_agent_id));
    assert_ne!(runs[0].2["input"], runs[1].2["input"]);
    records
}

#[tokio::test]
async fn repaired_acquisition_survives_independent_adoption_and_both_role_handoffs() {
    let mut fixture = Fixture::start_with_elastic(Behavior::RepairOnce).await;
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
    let monitoring_records = assert_native_monitoring(&fixture, &report).await;
    assert_eq!(report.progression, ScenarioProgression::Automatic);
    let adoption_id = report
        .adoption_b_id
        .as_deref()
        .expect("automatic scenario reports B's durable adoption");
    assert_eq!(
        report.adoption_b_publisher.as_deref(),
        Some(report.org_a_id.as_str())
    );
    let adoption_request = fixture.b.adoption_request();
    assert_eq!(adoption_request["mission_id"], report.mission_b_id);
    assert_eq!(adoption_request["requirement_id"], report.requirement_b_id);
    assert_eq!(adoption_request["role_id"], report.verifier_role_id);
    assert_eq!(
        adoption_request["responsibility_id"],
        report.responsibility_b_id
    );
    assert!(fixture
        .a
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method != "POST" || request.path != "/api/v1/capability-adoptions"));
    let (status, adoption) = get(&fixture.b, &format!("/capability-adoptions/{adoption_id}")).await;
    assert_eq!(status, StatusCode::OK, "{adoption}");
    assert_eq!(adoption["phase"], "accepted");
    assert_eq!(adoption["selection"]["publisher"], report.org_a_id);
    assert_eq!(
        adoption["selection"]["capability_id"],
        report.artifact.capability_id
    );
    assert_eq!(
        adoption["selection"]["version"],
        report.artifact.capability_version
    );
    assert_eq!(
        adoption["artifact"],
        serde_json::to_value(&report.artifact).unwrap()
    );
    assert_eq!(adoption["installation_id"], report.installation_b_id);
    assert_eq!(adoption["job_ids"], json!(report.acceptance_b_jobs));
    assert_eq!(adoption["acceptance_ref"], report.acceptance_b_ref);
    assert_eq!(adoption["suite_id"], suite("b").suite_id);
    assert!(adoption["rejection_ref"].is_null());
    // The driver observed this accepted status before it could request the
    // verifier handoff. The terminal record remains identical afterward.
    {
        let requests = fixture.b.requests.lock().unwrap();
        let (observed_index, observed) = requests
            .iter()
            .enumerate()
            .find(|(_, request)| {
                request
                    .adoption_response
                    .as_ref()
                    .is_some_and(|body| body["phase"] == "accepted")
            })
            .unwrap();
        assert_eq!(observed.adoption_response.as_ref().unwrap(), &adoption);
        assert!(requests
            .iter()
            .enumerate()
            .any(|(index, request)| index > observed_index
                && request.method == "POST"
                && request.path == "/api/v1/org-changes"
                && request.body["proposal"]["operations"]
                    .as_array()
                    .is_some_and(|operations| operations
                        .iter()
                        .any(|operation| operation["op"] == "RetireRole"
                            && operation["role_id"] == report.verifier_role_id))));
    }
    fixture.a.assert_server_owned_progression();
    fixture.b.assert_server_owned_progression();
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
    // Registry retrieval is part of B's actual adoption, not a separate direct
    // client search in this test. A successful recorded response proves the
    // integration did not silently use the registry's keyword fallback.
    let elastic = fixture.elastic.as_ref().unwrap();
    let b_query = &b.requirements[&report.requirement_b_id.clone().into()].summary;
    let cap_index = elastic.client.capability_index();
    let canonical_response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!(
            "{}/registry/v1/capabilities/{}/versions/{}",
            fixture._registry.url,
            report.artifact.capability_id,
            report.artifact.capability_version
        ))
        .bearer_auth(fixture.b.config.registry_token.as_ref().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(canonical_response.status(), StatusCode::OK);
    let canonical: genesis_core::capability::CapabilityManifest =
        canonical_response.json().await.unwrap();
    assert_eq!(canonical.publisher.as_str(), report.org_a_id);
    assert_eq!(canonical.source_digest, report.artifact.source_sha256);
    assert_eq!(canonical.artifact.actor_id, report.artifact.actor_id);
    assert_eq!(canonical.artifact.build_id, report.artifact.build_id);
    assert_eq!(
        canonical.artifact.build_number,
        report.artifact.build_number
    );
    let projected = serde_json::to_value(genesis_elastic::capability_doc(&canonical)).unwrap();
    {
        let shared = elastic.shared.lock().unwrap();
        let published = shared.docs[&cap_index]
            .values()
            .find(|doc| {
                doc["capability_id"] == report.artifact.capability_id
                    && doc["version"] == report.artifact.capability_version
            })
            .expect("the canonical repaired version must exist in the shared capability index");
        assert_eq!(published, &projected);
        assert_eq!(published["state"], "PUBLISHED");
    }
    {
        let searches = elastic.searches.lock().unwrap();
        let retrieval: Vec<_> = searches
            .iter()
            .filter(|search| search.path == format!("/{cap_index}/_search"))
            .collect();
        assert!(
            !retrieval.is_empty(),
            "B adoption must execute Elastic retrieval"
        );
        assert!(
            retrieval
                .iter()
                .all(|search| search.status == StatusCode::OK
                    && search.body["query"]["bool"]["must"][0]["multi_match"]["query"] == *b_query),
            "{retrieval:?}"
        );
        assert!(
            retrieval
                .iter()
                .any(|search| search.response["hits"]["hits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|hit| hit["_source"] == projected
                        && hit["_score"].as_f64().is_some_and(|score| score > 0.0))),
            "successful Elastic search must return A's canonical repaired version: {retrieval:?}"
        );
    }
    assert_own_evidence_is_searchable(&fixture.a, &report.requirement_a_id, &report.mission_b_id)
        .await;
    assert_own_evidence_is_searchable(&fixture.b, &report.requirement_b_id, &report.mission_a_id)
        .await;
    {
        let index_a = elastic.client.evidence_index(&report.org_a_id);
        let index_b = elastic.client.evidence_index(&report.org_b_id);
        assert_ne!(index_a, index_b);
        assert_ne!(index_a, cap_index);
        assert_ne!(index_b, cap_index);
        let shared = elastic.shared.lock().unwrap();
        for (index, org_id, requirement_id) in [
            (&index_a, &report.org_a_id, &report.requirement_a_id),
            (&index_b, &report.org_b_id, &report.requirement_b_id),
        ] {
            let documents = &shared.docs[index];
            assert!(documents.contains_key(&format!("{org_id}:requirement:{requirement_id}")));
            assert!(documents.values().all(|doc| doc["org_id"] == *org_id));
        }
    }
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
    for role_id in [&report.producer_role_id, &report.producer_successor_id] {
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
    assert_eq!(local.job_ids.len(), 4);
    let mut native_ids = BTreeSet::new();
    for id in report.acceptance_b_jobs.iter().chain([
        &report.initial_run_b_id,
        &report.continuation_run_b_id,
        &report
            .venue_monitoring
            .as_ref()
            .unwrap()
            .updated
            .source_job_id,
    ]) {
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
            json_sha256(&serde_json::to_value(monitoring_suite()).unwrap())
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
        ELASTIC_KEY,
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
            7
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
    let native_request_count = fixture.providers.world.lock().unwrap().creao_requests.len();
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
    assert_eq!(
        assert_native_monitoring(&fixture, &report).await,
        monitoring_records
    );
    assert_eq!(
        fixture.providers.world.lock().unwrap().creao_requests.len(),
        native_request_count,
        "historical proof reconstruction and restart must not invoke CREAO"
    );
    let restarted = &fixture.b;
    assert!(
        restarted.app.projection.client.is_some(),
        "restart must retain Elastic projection configuration"
    );
    assert_own_evidence_is_searchable(restarted, &report.requirement_b_id, &report.mission_a_id)
        .await;
    let (status, restored_adoption) =
        get(restarted, &format!("/capability-adoptions/{adoption_id}")).await;
    assert_eq!(status, StatusCode::OK, "{restored_adoption}");
    assert_eq!(restored_adoption, adoption, "accepted adoption survives verifier retirement and process restart with the same pin, jobs and proof");
    assert_eq!(
        restarted.adoption_request(),
        adoption_request,
        "recovery observes the existing intent without creating another one"
    );
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
    fixture.a.assert_server_owned_progression();
    fixture.b.assert_server_owned_progression();
    let world = fixture.providers.world.lock().unwrap();
    assert_eq!(world.model_requests.len(), 2);
    assert_eq!(world.actors.len(), 2);
    assert_eq!(
        world
            .runs
            .values()
            .filter(|run| run.side == Side::B)
            .count(),
        8
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
    fixture.a.assert_server_owned_progression();
    fixture.b.assert_server_owned_progression();
    let adoption_request = fixture.b.adoption_request();
    let adopted = fixture.b.app.store.list_adoptions(0, 32).unwrap();
    assert_eq!(adopted.len(), 1);
    let status = serde_json::to_value(adopted[0].status()).unwrap();
    assert_eq!(status["phase"], "accepted");
    assert_eq!(status["requirement_id"], adoption_request["requirement_id"]);
    assert_eq!(status["selection"]["publisher"], "scenario_a");
    assert!(status["acceptance_ref"].as_str().is_some());
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
    fixture.a.assert_server_owned_progression();
    fixture.b.assert_server_owned_progression();
    assert!(fixture
        .b
        .app
        .store
        .list_adoptions(0, 32)
        .unwrap()
        .is_empty());
    assert!(fixture
        .b
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method != "POST" || request.path != "/api/v1/capability-adoptions"));
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
        org.assert_server_owned_progression();
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
    fixture.a.assert_server_owned_progression();
    fixture.b.assert_server_owned_progression();
    let world = fixture.providers.world.lock().unwrap();
    assert!(world.model_requests.is_empty());
    assert!(world.requests.is_empty());
}
