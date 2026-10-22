//! Persistent venue assessments through real Rust Apify and CREAO clients over
//! loopback HTTP. Initial publication and monitor provisioning are explicitly
//! trusted synthetic fixtures; no live provider, agent creation, readiness,
//! source fetch, conversation memory, booking or sponsor evidence is claimed.
use axum::{
    body::to_bytes,
    extract::{Request, State},
    Json, Router,
};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_apify::{ApifyClient, RunLimits, SourceFile};
use genesis_core::{
    ids::*,
    lifecycle::{JobStatus, RoleState},
    model::{AgentBinding, OrgState, Provider},
};
use genesis_creao::{AccountLimiter, CreaoClient, Limits, QuotaLimits, Runtime, RuntimeStore};
use genesis_forge::{CandidateSpec, ForgeSession, SourcePackage};
use genesis_providers::{
    ApifyCapabilityExecutor, CreaoSpecialist, DevStub, ProviderSet, SpecialistRuntime,
};
use genesis_server::{
    app::AppState,
    build_org, parse_publishers,
    registry::{self, RegistryState},
    serve_org, OrgConfig,
};
use genesis_verification::{
    decode_json, json_sha256, ArtifactPin, ExecutionEvidence, ExtractionOutcome, OracleSuite,
    RunStatus,
};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};

const ORG: &str = "org_assessment";
const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ORACLE_CANARY: &str = "PRIVATE_ASSESSMENT_ORACLE_DO_NOT_SEND_TO_CREAO";
const FORGE_REF: &str = "synthetic-assessment-publication";
const CAPABILITY: &str = "offline-venue-extractor";
const VERSION: &str = "0.0.1";

fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.suite_id = "assessment-synthetic-suite".into();
    suite.scope = ORACLE_CANARY.into();
    // A second allowed snapshot of the same venue and event, not a claim that
    // an arbitrary future web page is covered by the original oracle.
    let mut update = suite.cases[0].clone();
    update.case_id = "capacity-cut-update".into();
    update.source.url = "https://fixtures.genesis.example/venues/capacity-cut.html".into();
    update.source.sha256 = "b".repeat(64);
    update.input["source_url"] = json!(update.source.url);
    if let ExtractionOutcome::Supported { venues } = &mut update.expected {
        for venue in venues {
            venue.evidence.source_url = update.source.url.clone();
            venue.evidence.snapshot_sha256 = update.source.sha256.clone();
            if venue.venue_id == "grand-hall" {
                venue.capacity = 100;
            }
        }
    }
    suite.cases.push(update);
    suite
}

fn checkpoint() -> ForgeSession {
    let spec = CandidateSpec {
        operation_id: FORGE_REF.into(),
        capability_id: CAPABILITY.into(),
        capability_version: VERSION.into(),
        actor_name: "synthetic-assessment-fixture".into(),
        source_version: "0.0".into(),
        source: SourcePackage::new(vec![SourceFile {
            name: "main.js".into(),
            content: "// Synthetic trusted policy; never uploaded.\n".into(),
        }])
        .unwrap(),
    };
    let pin = ArtifactPin {
        capability_id: CAPABILITY.into(),
        capability_version: VERSION.into(),
        source_sha256: spec.source.sha256().unwrap(),
        actor_id: "assessment-actor".into(),
        build_id: "assessment-build".into(),
        build_number: VERSION.into(),
    };
    let runs: Vec<_> = suite()
        .cases
        .iter()
        .enumerate()
        .map(|(i, case)| ExecutionEvidence {
            case_id: case.case_id.clone(),
            artifact: pin.clone(),
            input_sha256: json_sha256(&case.input),
            source: case.source.clone(),
            run_id: format!("trusted-fixture-run-{i}"),
            dataset_id: format!("trusted-fixture-dataset-{i}"),
            status: RunStatus::Succeeded,
            output: serde_json::to_value(&case.expected).unwrap(),
        })
        .collect();
    let definition = spec.definition().definition_sha256().unwrap();
    let mut record = serde_json::to_value(ForgeSession::new(spec, &suite()).unwrap()).unwrap();
    record["phase"] = json!("verified");
    record["actor"] = json!({"schema_version":1,"actor_id":pin.actor_id,"version_number":"0.0","definition_sha256":definition});
    record["build"] = json!({"schema_version":1,"actor_id":pin.actor_id,"build_id":pin.build_id,"build_number":pin.build_number,"version_number":"0.0","definition_sha256":definition});
    record["pin"] = json!(pin);
    record["receipts"] = json!(runs);
    serde_json::from_value(record).unwrap()
}

fn manifest() -> Value {
    let verified = checkpoint().verified(&suite()).unwrap();
    json!({"capability_id":CAPABILITY,"version":VERSION,"publisher":ORG,"source_digest":verified.artifact.source_sha256,
        "intent":"Read synthetic venue snapshots","applicability_conditions":["four fixed fixture inputs"],"known_limitations":["loopback only"],
        "input_schema":{"type":"object"},"output_schema":{"type":"object"},"required_permissions":["capability.venue_lookup"],
        "artifact":{"actor_id":verified.artifact.actor_id,"build_id":verified.artifact.build_id,"build_number":VERSION},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,"local_configuration_schema":{},"acceptance_suite_ref":suite().suite_id,
        "verification":{"passed":true,"cases":[{"name":"table","kind":"success","passed":true,"detail":"untrusted claim"},
            {"name":"cards","kind":"success","passed":true,"detail":"untrusted claim"},{"name":"missing","kind":"unsupported","passed":true,"detail":"untrusted claim"}],
            "tested_environment":"synthetic","limitations":[],"oracle_ref":"untrusted","verified_at":"2000-01-01T00:00:00Z"},
        "provenance":[],"state":"APPROVED","published_at":null})
}

struct Server {
    url: String,
    task: JoinHandle<()>,
    workers: Vec<JoinHandle<()>>,
}
impl Server {
    async fn router(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        Self {
            url,
            task: tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            }),
            workers: vec![],
        }
    }
    async fn org(app: AppState, automatic: bool) -> Self {
        if automatic {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            Self {
                url,
                task: tokio::spawn(async move {
                    serve_org(app, listener).await.unwrap();
                }),
                workers: vec![],
            }
        } else {
            // Real dispatch/polling, deliberately no semantic-finalizer worker:
            // tests can observe the native completion / domain proof boundary.
            let mut server = Self::router(app.clone().router()).await;
            server
                .workers
                .push(genesis_server::outbox::spawn(app.clone()));
            server
                .workers
                .push(genesis_server::specialist::spawn(app, Duration::from_millis(10)).unwrap());
            server
        }
    }
    async fn stop(&mut self) {
        for worker in self.workers.drain(..) {
            worker.abort();
            let _ = worker.await;
        }
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
        for worker in &self.workers {
            worker.abort();
        }
    }
}

#[derive(Clone)]
struct Seen {
    method: String,
    path: String,
    body: Value,
}
#[derive(Clone, Copy, Default)]
enum ResponseMode {
    #[default]
    Correct,
    WrongCapacity,
    ReplayFirst,
    Ambiguous,
}
#[derive(Default)]
struct World {
    seen: Vec<Seen>,
    apify_runs: BTreeMap<String, usize>,
    creao_runs: BTreeMap<String, Value>,
    mode: ResponseMode,
    first: Option<Value>,
}
struct Providers {
    server: Server,
    world: Arc<Mutex<World>>,
}
impl Providers {
    async fn start() -> Self {
        let world = Arc::new(Mutex::new(World::default()));
        let server = Server::router(
            Router::new()
                .fallback(provider_reply)
                .with_state(world.clone()),
        )
        .await;
        Self { server, world }
    }
    fn configure(
        &self,
        directory: &std::path::Path,
    ) -> (ProviderSet, Arc<CreaoSpecialist>, RuntimeStore) {
        let apify = ApifyClient::for_local_testing(
            "synthetic-apify-token",
            RunLimits {
                request_timeout: Duration::from_secs(2),
                deadline: Duration::from_secs(4),
                poll_interval: Duration::from_millis(1),
                ..Default::default()
            },
            &self.server.url,
        )
        .unwrap();
        let creao = CreaoClient::for_local_testing(
            "cr_sk_synthetic_assessment",
            Limits {
                request_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            AccountLimiter::open(
                directory.join("quota.sqlite"),
                "synthetic-assessment-account",
                QuotaLimits::default(),
            )
            .unwrap(),
            &format!("{}/v1", self.server.url),
        )
        .unwrap();
        let journal = RuntimeStore::open(directory.join("creao.sqlite"), ORG).unwrap();
        let specialist = Arc::new(CreaoSpecialist::new(
            Runtime::new(creao, journal.clone(), Duration::from_millis(1)).unwrap(),
        ));
        (
            ProviderSet::new()
                .with_dev_stub(DevStub::default())
                .with_executor(Arc::new(ApifyCapabilityExecutor::new(apify)))
                .with_specialist(specialist.clone()),
            specialist,
            journal,
        )
    }
    fn execution_posts(&self) -> Vec<Seen> {
        self.world
            .lock()
            .unwrap()
            .seen
            .iter()
            .filter(|r| r.method == "POST" && r.path == "/v1/runs")
            .cloned()
            .collect()
    }
    fn mode(&self, mode: ResponseMode) {
        self.world.lock().unwrap().mode = mode;
    }
}

fn structured_output(context: &Value, mode: ResponseMode, first: &Option<Value>) -> Value {
    if matches!(mode, ResponseMode::ReplayFirst) {
        return first.clone().unwrap();
    }
    // These are fixed responses for two declared fixture inputs, not a second
    // implementation of the production policy evaluator.
    let capacity = context["venue"]["capacity"] == 180;
    assert!(capacity || context["venue"]["capacity"] == 100);
    let capacity = if matches!(mode, ResponseMode::WrongCapacity) {
        !capacity
    } else {
        capacity
    };
    json!({"schema_version":1,"assessment_id":context["assessment_id"],"source_job_id":context["source_job_id"],
        "invocation_proof_ref":context["invocation_proof_ref"],"policy_sha256":context["policy_sha256"],
        "selected_venue_id":"grand-hall","evidence":context["venue"]["evidence"],
        "checks":{"capacity":capacity,"price":true,"window":true,"availability":true,"step_free":true},
        "verdict":if capacity {"feasible"} else {"reopen"}})
}

async fn provider_reply(
    State(world): State<Arc<Mutex<World>>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 512 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let mut world = world.lock().unwrap();
    world.seen.push(Seen {
        method: method.clone(),
        path: path.clone(),
        body: body.clone(),
    });
    let pin = checkpoint().artifact().unwrap().clone();
    if method == "GET" && path == format!("/v2/actor-builds/{}", pin.build_id) {
        return (
            StatusCode::OK,
            Json(
                json!({"data":{"id":pin.build_id,"actId":pin.actor_id,"buildNumber":pin.build_number,"status":"SUCCEEDED"}}),
            ),
        );
    }
    if method == "POST" && path == format!("/v2/actors/{}/runs", pin.actor_id) {
        let Some(case) = suite().cases.iter().position(|case| case.input == body) else {
            panic!("unexpected Apify fixture input")
        };
        let id = format!("apify-run-{}", world.apify_runs.len());
        world.apify_runs.insert(id.clone(), case);
        return (
            StatusCode::OK,
            Json(
                json!({"data":{"id":id,"actId":pin.actor_id,"buildId":pin.build_id,"status":"SUCCEEDED","defaultDatasetId":format!("dataset-{id}")}}),
            ),
        );
    }
    if let Some(id) = path.strip_prefix("/v2/actor-runs/") {
        assert!(world.apify_runs.contains_key(id));
        return (
            StatusCode::OK,
            Json(
                json!({"data":{"id":id,"actId":pin.actor_id,"buildId":pin.build_id,"status":"SUCCEEDED","defaultDatasetId":format!("dataset-{id}")}}),
            ),
        );
    }
    if let Some(id) = path
        .strip_prefix("/v2/datasets/dataset-")
        .and_then(|p| p.strip_suffix("/items"))
    {
        let case = world.apify_runs[id];
        return (StatusCode::OK, Json(json!([suite().cases[case].expected])));
    }
    if method == "POST" && path == "/v1/runs" {
        assert_eq!(body["agent_id"], AGENT);
        assert_eq!(body["should_notify"], false);
        assert!(body.get("conversation_id").is_none());
        assert!(body.get("workspace_id").is_none());
        assert!(!body.to_string().contains(ORACLE_CANARY));
        assert!(body["input"]["input"].get("expected").is_none());
        if matches!(world.mode, ResponseMode::Ambiguous) {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"PRIVATE_UPSTREAM_ERROR_CANARY"})),
            );
        }
        let context = &body["input"]["input"]["context"];
        assert!(
            context.is_object(),
            "assessment must provide the frozen domain context"
        );
        let result = structured_output(context, world.mode, &world.first);
        if world.first.is_none() {
            world.first = Some(result.clone());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let conversation = uuid::Uuid::new_v4().to_string();
        let completed = json!({"run_id":id,"agent_id":AGENT,"status":"completed","conversation_id":conversation,"model":"synthetic-model",
            "result":{"text":serde_json::to_string(&result).unwrap()}});
        world.creao_runs.insert(id.clone(), completed);
        return (
            StatusCode::ACCEPTED,
            Json(
                json!({"run_id":id,"agent_id":AGENT,"status":"pending","conversation_id":conversation,"model":"synthetic-model"}),
            ),
        );
    }
    if method == "GET" && path.starts_with("/v1/runs/") {
        return (
            StatusCode::OK,
            Json(world.creao_runs[path.trim_start_matches("/v1/runs/")].clone()),
        );
    }
    panic!("unexpected provider path {method} {path}; this fixture must never create another agent")
}

struct Fixture {
    directory: tempfile::TempDir,
    _registry: Server,
    providers: Providers,
    config: OrgConfig,
    app: AppState,
    server: Server,
    http: reqwest::Client,
    specialist: Arc<CreaoSpecialist>,
    journal: RuntimeStore,
    mission: String,
    monitor: String,
    backup: String,
    responsibility: String,
    installation: String,
}
impl Fixture {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let registry = Server::router(registry::router(
            RegistryState::open_in_memory(
                parse_publishers(&format!("{ORG}:{ORG}-registry")),
                vec![],
            )
            .unwrap(),
        ))
        .await;
        let providers = Providers::start().await;
        let (set, specialist, journal) = providers.configure(directory.path());
        let mut config = OrgConfig::dev(ORG, "Venue continuity fixture");
        config.db = Some(directory.path().join("organization.sqlite"));
        config.registry_url = Some(registry.url.clone());
        config.registry_token = Some(format!("{ORG}-registry"));
        config.oracle_suites = vec![suite()];
        config.forge_checkpoints = vec![checkpoint()];
        let app = build_org(&config, Some(set)).unwrap();
        let server = Server::org(app.clone(), false).await;
        let mut f = Self {
            directory,
            _registry: registry,
            providers,
            config,
            app,
            server,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .no_proxy()
                .build()
                .unwrap(),
            specialist,
            journal,
            mission: String::new(),
            monitor: String::new(),
            backup: String::new(),
            responsibility: String::new(),
            installation: String::new(),
        };
        let (status, published) = f
            .post(
                "/capabilities/publish",
                json!({"manifest":manifest(),"verification_ref":FORGE_REF}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{published}");
        let (status, mission) = f.post("/missions", json!({"goal":"Keep the same event feasible after venue updates","roles":[
            {"key":"producer","name":"Producer","charter":"Establish local capability","authority":{"scopes":["mission.coordinate","capability.venue_lookup"],"budget_cents":10000}},
            {"key":"monitor","name":"Monitor","charter":"Own continuing venue assessment","authority":{"scopes":["mission.coordinate","capability.venue_lookup","monitor.venue"],"budget_cents":10000}},
            {"key":"backup","name":"Backup","charter":"Receive later handoff","authority":{"scopes":["mission.coordinate","capability.venue_lookup"],"budget_cents":10000}}],
            "responsibilities":[{"purpose":"Keep venue operation feasible","owner":"producer"}]})).await;
        assert_eq!(status, StatusCode::CREATED, "{mission}");
        f.mission = mission["mission_id"].as_str().unwrap().into();
        let state = f.app.current_state().unwrap();
        let role = |name: &str| {
            state
                .roles
                .values()
                .find(|r| r.name == name)
                .unwrap()
                .role_id
                .to_string()
        };
        let producer = role("Producer");
        f.monitor = role("Monitor");
        f.backup = role("Backup");
        f.responsibility = state
            .responsibilities
            .values()
            .next()
            .unwrap()
            .responsibility_id
            .to_string();
        // Already provisioned/readiness-checked monitor modeled as trusted setup.
        // The genuine work below executes its fixed native ID; never /agents.
        f.mutate(|state| {
            state
                .roles
                .get_mut(&RoleId::new(&f.monitor))
                .unwrap()
                .binding = Some(binding())
        })
        .await;
        let (status, installation) = f
            .post(
                "/capability-installations",
                json!({"capability_id":CAPABILITY,"version":VERSION}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{installation}");
        f.installation = installation["installation_id"].as_str().unwrap().into();
        let (status, checks) = f.post(&format!("/capability-installations/{}/checks", f.installation), json!({"client_request_id":"local-acceptance","role_id":producer,"responsibility_id":f.responsibility,"suite_id":suite().suite_id})).await;
        assert!(status.is_success(), "{checks}");
        for id in checks["job_ids"].as_array().unwrap() {
            f.collect(id.as_str().unwrap()).await;
        }
        let (status, accepted) = f
            .post(
                &format!("/capability-installations/{}/acceptance", f.installation),
                json!({"suite_id":suite().suite_id,"job_ids":checks["job_ids"]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{accepted}");
        f.change("handoff-to-persistent-monitor", json!([
            {"op":"RetireRole","role_id":producer,"handoffs":[{"responsibility_id":f.responsibility,"to":{"kind":"existing","role_id":f.monitor}}]},
            {"op":"AttachCapability","role":{"kind":"existing","role_id":f.monitor},"capability":{"capability_id":CAPABILITY,"version":VERSION}}
        ])).await;
        let state = f.app.current_state().unwrap();
        assert_eq!(
            state.roles[&RoleId::new(producer)].state,
            RoleState::Retired
        );
        assert_eq!(
            state.roles[&RoleId::new(&f.monitor)].binding,
            Some(binding())
        );
        f
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        principal: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .http
            .request(method, format!("{}/api/v1{path}", self.server.url))
            .bearer_auth(format!("{ORG}-{principal}"));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.request(Method::POST, path, "operator", Some(body))
            .await
    }
    async fn get(&self, path: &str) -> Value {
        let (status, body) = self.request(Method::GET, path, "operator", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
    async fn wait(&self, path: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let mut last = Value::Null;
        for _ in 0..400 {
            last = self.get(path).await;
            if predicate(&last) {
                return last;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("bounded wait failed at {path}: {last}")
    }
    async fn mutate(&self, mutate: impl FnOnce(&mut OrgState)) {
        let _guard = self.app.kernel.lock().await;
        let old = self.app.current_state().unwrap();
        let mut next = old.clone();
        mutate(&mut next);
        next.revision += 1;
        self.app
            .store
            .commit_state(
                old.revision,
                &next,
                vec![],
                &Default::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
    async fn change(&self, key: &str, operations: Value) {
        let (status, observation) = self
            .post(
                &format!("/missions/{}/observations", self.mission),
                json!({"source":"fixture","summary":"Transfer continuing venue responsibility"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{observation}");
        let (status, change) = self.post("/org-changes", json!({"client_request_id":key,"proposal":{
            "expected_org_revision":self.app.current_state().unwrap().revision,"trigger_evidence_ids":[observation["observation_id"]],
            "rationale":"Keep one continuing owner and retain accepted capability","operations":operations}})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{change}");
        self.wait(
            &format!("/org-changes/{}", change["change_id"].as_str().unwrap()),
            |v| v["status"] == "COMMITTED",
        )
        .await;
    }
    async fn collect(&self, id: &str) {
        for _ in 0..2 {
            let (status, record) = self
                .post(
                    &format!("/capability-runs/{id}/step"),
                    json!({"retry_read":false}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{record}");
        }
        assert_eq!(
            self.get(&format!("/capability-runs/{id}")).await["phase"],
            "collected"
        );
    }
    async fn source(&self, key: &str, case: usize) -> String {
        let (status, job) = self.post("/capability-runs", json!({"client_request_id":key,"installation_id":self.installation,
            "role_id":self.monitor,"responsibility_id":self.responsibility,"suite_id":suite().suite_id,"input":suite().cases[case].input})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{job}");
        let id = job["job_id"].as_str().unwrap().to_owned();
        self.collect(&id).await;
        let (status, proof) = self
            .post(&format!("/capability-runs/{id}/verify"), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK, "{proof}");
        assert_eq!(proof["decision"], "verified");
        id
    }
    fn request_body(&self, key: &str, source: &str) -> Value {
        json!({"client_request_id":key,"role_id":self.monitor,"responsibility_id":self.responsibility,"source_job_id":source,
            "policy":{"selected_venue_id":"grand-hall","attendees":120,"budget":{"minor_units":80000,"currency":"USD"},
                "window":{"start":"2026-11-14T18:00:00-08:00","end":"2026-11-14T22:00:00-08:00"},"step_free_required":true},
            "deadline":Utc::now()+ChronoDuration::minutes(10)})
    }
    async fn create(&self, body: Value) -> Value {
        let (status, record) = self.post("/venue-assessments", body).await;
        assert_eq!(status, StatusCode::CREATED, "{record}");
        record
    }
    async fn collected(&self, record: &Value) {
        self.wait(
            &format!("/jobs/{}", record["job_id"].as_str().unwrap()),
            |job| job["status"] == "VERIFYING",
        )
        .await;
    }
    async fn verify(&self, record: &Value) -> Value {
        let (status, verified) = self
            .post(
                &format!(
                    "/venue-assessments/{}/verify",
                    record["assessment_id"].as_str().unwrap()
                ),
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{verified}");
        verified
    }
    async fn restart(&mut self, automatic: bool) {
        self.server.stop().await;
        let (providers, specialist, journal) = self.providers.configure(self.directory.path());
        specialist.recover_interrupted().await.unwrap();
        self.specialist = specialist;
        self.journal = journal;
        self.config.forge_checkpoints.clear();
        self.app = build_org(&self.config, Some(providers)).unwrap();
        self.server = Server::org(self.app.clone(), automatic).await;
    }
    fn side_effect_counts(&self) -> (usize, usize) {
        let state = self.app.current_state().unwrap();
        (state.observations.len(), state.requirements.len())
    }
}
fn binding() -> AgentBinding {
    AgentBinding {
        provider: Provider::Creao,
        provider_agent_id: AGENT.into(),
        config_digest: "a".repeat(64),
        conversation_ref: None,
    }
}
fn proof(record: &Value) -> &Value {
    &record["proof"]
}

#[tokio::test]
async fn same_monitor_assesses_two_verified_updates_across_restart_without_new_agent() {
    let mut f = Fixture::start().await;
    let role = f.app.current_state().unwrap().roles[&RoleId::new(&f.monitor)].clone();
    let source_a = f.source("update-a", 0).await;
    let request_a = f.request_body("assessment-a", &source_a);
    let a = f.create(request_a.clone()).await;
    f.collected(&a).await;
    let pending = f
        .get(&format!(
            "/venue-assessments/{}",
            a["assessment_id"].as_str().unwrap()
        ))
        .await;
    assert!(
        proof(&pending).is_null(),
        "native completion is not a domain decision"
    );
    let job_a = f
        .app
        .store
        .get_job(&JobId::new(a["job_id"].as_str().unwrap()))
        .unwrap();
    assert_eq!(job_a.status, JobStatus::Verifying);
    assert_eq!(job_a.result.as_ref().unwrap()["domain_verified"], false);
    let receipt_a = f.specialist.execution_receipt(&job_a, &binding()).unwrap();
    let accepted_a = f.verify(&a).await;
    assert_eq!(proof(&accepted_a)["decision"], "accepted");
    assert_eq!(proof(&accepted_a)["output"]["verdict"], "feasible");
    assert!(proof(&accepted_a)["observation_id"].is_string());
    assert!(proof(&accepted_a)["requirement_id"].is_null());
    let effects_a = f.side_effect_counts();
    assert_eq!(f.verify(&a).await, accepted_a);
    let (status, replay) = f.post("/venue-assessments", request_a).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["assessment_id"], a["assessment_id"]);
    assert_eq!(f.side_effect_counts(), effects_a);
    assert_eq!(f.providers.execution_posts().len(), 1);
    f.restart(true).await;
    assert_eq!(
        f.get(&format!(
            "/venue-assessments/{}",
            a["assessment_id"].as_str().unwrap()
        ))
        .await,
        accepted_a
    );
    assert_eq!(
        f.specialist
            .execution_receipt(&job_a, &binding())
            .unwrap()
            .output_sha256,
        receipt_a.output_sha256
    );
    let source_b = f.source("update-b-capacity-cut", 3).await;
    let b = f.create(f.request_body("assessment-b", &source_b)).await;
    let accepted_b = f
        .wait(
            &format!(
                "/venue-assessments/{}",
                b["assessment_id"].as_str().unwrap()
            ),
            |v| !proof(v).is_null(),
        )
        .await;
    assert_eq!(proof(&accepted_b)["decision"], "accepted");
    assert_eq!(proof(&accepted_b)["output"]["verdict"], "reopen");
    assert!(proof(&accepted_b)["requirement_id"].is_string());
    assert_eq!(f.side_effect_counts(), (effects_a.0 + 1, effects_a.1 + 1));
    let state = f.app.current_state().unwrap();
    assert_eq!(state.roles[&RoleId::new(&f.monitor)].binding, role.binding);
    assert_eq!(
        state.roles[&RoleId::new(&f.monitor)].state,
        RoleState::Active
    );
    let job_b = f
        .app
        .store
        .get_job(&JobId::new(b["job_id"].as_str().unwrap()))
        .unwrap();
    let receipt_b = f.specialist.execution_receipt(&job_b, &binding()).unwrap();
    assert_ne!(source_a, source_b);
    assert_ne!(job_a.job_id, job_b.job_id);
    assert_ne!(job_a.attempt_id, job_b.attempt_id);
    assert_ne!(receipt_a.run_id, receipt_b.run_id);
    assert_ne!(receipt_a.request_sha256, receipt_b.request_sha256);
    assert_eq!(receipt_a.provider_agent_id, receipt_b.provider_agent_id);
    assert_eq!(
        receipt_a.binding_config_digest,
        receipt_b.binding_config_digest
    );
    assert_eq!(job_a.assignment_id, job_b.assignment_id);
    assert_eq!(job_a.epoch, job_b.epoch);
    assert_eq!(f.providers.execution_posts().len(), 2);
    assert_eq!(f.verify(&b).await, accepted_b);
    assert!(
        f.app.store.commitments().unwrap().is_empty(),
        "an assessment cannot reserve a venue"
    );
    let posts = f.providers.execution_posts();
    assert_ne!(
        posts[0].body["input"]["input"],
        posts[1].body["input"]["input"]
    );
    assert!(f
        .providers
        .world
        .lock()
        .unwrap()
        .seen
        .iter()
        .all(|r| !r.path.contains("agents")));
}

#[tokio::test]
async fn semantic_mismatch_and_previous_cycle_output_never_create_mission_evidence() {
    for mode in [ResponseMode::WrongCapacity, ResponseMode::ReplayFirst] {
        let f = Fixture::start().await;
        if matches!(mode, ResponseMode::ReplayFirst) {
            let source = f.source("prior-source", 0).await;
            let prior = f.create(f.request_body("prior-assessment", &source)).await;
            f.collected(&prior).await;
            assert_eq!(proof(&f.verify(&prior).await)["decision"], "accepted");
        }
        f.providers.mode(mode);
        let source = f.source("wrong-source", 3).await;
        let record = f.create(f.request_body("wrong-assessment", &source)).await;
        f.collected(&record).await;
        let before = f.side_effect_counts();
        let rejected = f.verify(&record).await;
        assert_eq!(proof(&rejected)["decision"], "semantic_rejected");
        assert!(proof(&rejected)["observation_id"].is_null());
        assert!(proof(&rejected)["requirement_id"].is_null());
        assert_eq!(f.side_effect_counts(), before);
        assert_eq!(f.verify(&record).await, rejected);
        let job = f
            .app
            .store
            .get_job(&JobId::new(record["job_id"].as_str().unwrap()))
            .unwrap();
        assert_eq!(job.status, JobStatus::ResultRejected);
        assert!(
            f.specialist.execution_receipt(&job, &binding()).is_ok(),
            "native provenance remains available despite semantic rejection"
        );
    }
}

#[tokio::test]
async fn operator_acl_idempotency_and_one_active_cycle_reject_before_extra_dispatch() {
    let f = Fixture::start().await;
    let source_a = f.source("identity-a", 0).await;
    let source_b = f.source("identity-b", 3).await;
    let request = f.request_body("identity", &source_a);
    let jobs = f.app.store.list_jobs().unwrap().len();
    for principal in ["agent", "adapter"] {
        let (status, _) = f
            .request(
                Method::POST,
                "/venue-assessments",
                principal,
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(f.app.store.list_jobs().unwrap().len(), jobs);
    let record = f.create(request.clone()).await;
    let mut changed = request.clone();
    changed["policy"]["budget"]["minor_units"] = json!(90000);
    assert_eq!(
        f.post("/venue-assessments", changed).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.post("/venue-assessments", f.request_body("parallel", &source_b))
            .await
            .0,
        StatusCode::CONFLICT
    );
    for principal in ["agent", "adapter"] {
        let path = format!(
            "/venue-assessments/{}",
            record["assessment_id"].as_str().unwrap()
        );
        assert_eq!(
            f.request(Method::GET, &path, principal, None).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.request(
                Method::POST,
                &format!("{path}/verify"),
                principal,
                Some(json!({}))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    f.collected(&record).await;
    f.verify(&record).await;
    assert_eq!(
        f.post(
            "/venue-assessments",
            f.request_body("reuse-source", &source_a)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(f.providers.execution_posts().len(), 1);
    let job = f
        .app
        .store
        .get_job(&JobId::new(record["job_id"].as_str().unwrap()))
        .unwrap();
    let (status, error) = f
        .post(
            &format!("/jobs/{}/results", job.job_id),
            json!({"attempt_id":job.attempt_id,"epoch":job.epoch,"outcome":"succeeded",
                "provider_status":"completed","provider_refs":{},"operation_id":null,
                "domain_verified":true,"output":{"verdict":"feasible"}}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"], "managed_specialist_result");
}

#[tokio::test]
async fn current_scope_lease_and_mission_are_rechecked_before_acceptance() {
    for mutation in ["scope", "lease", "mission"] {
        let f = Fixture::start().await;
        let source = f.source("authority-source", 0).await;
        let record = f
            .create(f.request_body("authority-assessment", &source))
            .await;
        f.collected(&record).await;
        let job = f
            .app
            .store
            .get_job(&JobId::new(record["job_id"].as_str().unwrap()))
            .unwrap();
        f.mutate(|state| match mutation {
            "scope" => {
                state
                    .roles
                    .get_mut(&job.role_id)
                    .unwrap()
                    .authority
                    .scopes
                    .remove("monitor.venue");
            }
            "lease" => {
                state
                    .assignments
                    .get_mut(&job.assignment_id)
                    .unwrap()
                    .lease_expires_at = Some(Utc::now());
            }
            "mission" => {
                state.missions.get_mut(&job.mission_id).unwrap().deadline = Some(Utc::now());
            }
            _ => unreachable!(),
        })
        .await;
        let before = f.side_effect_counts();
        let rejected = f.verify(&record).await;
        assert_eq!(
            proof(&rejected)["decision"],
            "authority_rejected",
            "{mutation}: {rejected}"
        );
        assert_eq!(f.side_effect_counts(), before);
        assert_eq!(f.providers.execution_posts().len(), 1);
        assert!(f.specialist.execution_receipt(&job, &binding()).is_ok());
    }
}

#[tokio::test]
async fn handoff_cannot_accept_old_work_or_recreate_an_expired_cycle() {
    let f = Fixture::start().await;
    let source = f.source("handoff-source", 0).await;
    let record = f
        .create(f.request_body("handoff-assessment", &source))
        .await;
    f.collected(&record).await;
    f.change("handoff-monitor-to-backup", json!([{"op":"ReassignResponsibility","responsibility_id":f.responsibility,"from":f.monitor,"to":{"kind":"existing","role_id":f.backup}}])).await;
    let before = f.side_effect_counts();
    assert_eq!(
        proof(&f.verify(&record).await)["decision"],
        "authority_rejected"
    );
    assert_eq!(f.side_effect_counts(), before);
    assert!(!f
        .post(
            "/venue-assessments",
            f.request_body("old-role-new-key", &source)
        )
        .await
        .0
        .is_success());
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn ambiguous_creao_submission_remains_unknown_after_restart_without_replay() {
    let mut f = Fixture::start().await;
    let source = f.source("unknown-source", 0).await;
    f.providers.mode(ResponseMode::Ambiguous);
    let request = f.request_body("unknown-assessment", &source);
    let record = f.create(request.clone()).await;
    f.wait(
        &format!("/jobs/{}", record["job_id"].as_str().unwrap()),
        |v| v["status"] == "UNKNOWN",
    )
    .await;
    assert_eq!(f.providers.execution_posts().len(), 1);
    let before = f.side_effect_counts();
    f.restart(true).await;
    let (status, replay) = f.post("/venue-assessments", request).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["assessment_id"], record["assessment_id"]);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(f.side_effect_counts(), before);
    assert!(proof(
        &f.get(&format!(
            "/venue-assessments/{}",
            record["assessment_id"].as_str().unwrap()
        ))
        .await
    )
    .is_null());
    assert!(!f
        .post(
            &format!(
                "/venue-assessments/{}/verify",
                record["assessment_id"].as_str().unwrap()
            ),
            json!({})
        )
        .await
        .0
        .is_success());
}

fn sqlite(f: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap()
}

fn durable_counts(db: &rusqlite::Connection) -> Vec<i64> {
    [
        "jobs",
        "outbox",
        "events",
        "org_snapshots",
        "venue_assessments",
    ]
    .iter()
    .map(|table| {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    })
    .collect()
}

#[tokio::test]
async fn creation_and_finalization_failures_roll_back_every_kernel_effect_then_reuse_one_run() {
    let f = Fixture::start().await;
    let source = f.source("rollback-source", 3).await;
    let request = f.request_body("rollback-assessment", &source);
    let db = sqlite(&f);
    let initial_counts = durable_counts(&db);
    let initial_state = serde_json::to_value(f.app.current_state().unwrap()).unwrap();
    db.execute_batch("CREATE TRIGGER reject_assessment_creation BEFORE INSERT ON venue_assessments BEGIN SELECT RAISE(ABORT, 'synthetic create storage failure'); END;").unwrap();
    let (status, _) = f.post("/venue-assessments", request.clone()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        durable_counts(&db),
        initial_counts,
        "no job, outbox, event, snapshot or domain row may escape the failed create transaction"
    );
    assert_eq!(
        serde_json::to_value(f.app.current_state().unwrap()).unwrap(),
        initial_state
    );
    assert!(f.providers.execution_posts().is_empty());
    db.execute_batch("DROP TRIGGER reject_assessment_creation;")
        .unwrap();
    let record = f.create(request).await;
    f.collected(&record).await;
    let job_id = JobId::new(record["job_id"].as_str().unwrap());
    let job = f.app.store.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Verifying);
    let native = f.specialist.execution_receipt(&job, &binding()).unwrap();
    let before_counts = durable_counts(&db);
    let before_state = serde_json::to_value(f.app.current_state().unwrap()).unwrap();
    let before_requests = f.providers.world.lock().unwrap().seen.len();
    db.execute_batch("CREATE TRIGGER reject_assessment_proof BEFORE UPDATE OF proof_json ON venue_assessments WHEN NEW.proof_json IS NOT NULL BEGIN SELECT RAISE(ABORT, 'synthetic proof storage failure'); END;").unwrap();
    let verify_path = format!(
        "/venue-assessments/{}/verify",
        record["assessment_id"].as_str().unwrap()
    );
    let (status, _) = f.post(&verify_path, json!({})).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(durable_counts(&db), before_counts);
    assert_eq!(
        serde_json::to_value(f.app.current_state().unwrap()).unwrap(),
        before_state
    );
    assert_eq!(f.app.store.get_job(&job_id).unwrap(), job);
    let saved = f
        .get(&format!(
            "/venue-assessments/{}",
            record["assessment_id"].as_str().unwrap()
        ))
        .await;
    assert!(proof(&saved).is_null());
    assert_eq!(saved["status"], "VERIFYING");
    assert_eq!(
        f.providers.world.lock().unwrap().seen.len(),
        before_requests
    );
    db.execute_batch("DROP TRIGGER reject_assessment_proof;")
        .unwrap();
    let accepted = f.verify(&record).await;
    assert_eq!(proof(&accepted)["decision"], "accepted");
    assert_eq!(proof(&accepted)["output"]["verdict"], "reopen");
    assert!(proof(&accepted)["observation_id"].is_string());
    assert!(proof(&accepted)["requirement_id"].is_string());
    assert_eq!(proof(&accepted)["run_id"], native.run_id);
    let after_counts = durable_counts(&db);
    let after_state = serde_json::to_value(f.app.current_state().unwrap()).unwrap();
    assert_eq!(f.verify(&record).await, accepted);
    assert_eq!(durable_counts(&db), after_counts);
    assert_eq!(
        serde_json::to_value(f.app.current_state().unwrap()).unwrap(),
        after_state
    );
    assert_eq!(
        f.providers.world.lock().unwrap().seen.len(),
        before_requests,
        "local verification and retries must never call the provider"
    );
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn altered_index_columns_are_rejected_even_when_the_sealed_binding_is_untouched() {
    let f = Fixture::start().await;
    let source = f.source("index-source", 0).await;
    let request = f.request_body("index-assessment", &source);
    let record = f.create(request.clone()).await;
    f.collected(&record).await;
    let id = record["assessment_id"].as_str().unwrap();
    let path = format!("/venue-assessments/{id}");
    let db = sqlite(&f);
    let binding_before: String = db
        .query_row(
            "SELECT binding_json FROM venue_assessments WHERE assessment_id=?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    let requests = f.providers.world.lock().unwrap().seen.len();
    let counts = f.side_effect_counts();
    for (column, original) in [
        ("source_job_id", source.as_str()),
        ("client_request_id", "index-assessment"),
    ] {
        db.execute(
            &format!("UPDATE venue_assessments SET {column}=?2 WHERE assessment_id=?1"),
            rusqlite::params![id, "unrelated-index-value"],
        )
        .unwrap();
        let (status, _) = f.request(Method::GET, &path, "operator", None).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "indexed {column} must be validated against the private record"
        );
        assert_eq!(
            f.post(&format!("{path}/verify"), json!({})).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(f.side_effect_counts(), counts);
        assert_eq!(f.providers.world.lock().unwrap().seen.len(), requests);
        let binding_after: String = db
            .query_row(
                "SELECT binding_json FROM venue_assessments WHERE assessment_id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            binding_before, binding_after,
            "this test changes an index only, not the envelope checksum"
        );
        db.execute(
            &format!("UPDATE venue_assessments SET {column}=?2 WHERE assessment_id=?1"),
            rusqlite::params![id, original],
        )
        .unwrap();
    }
    assert_eq!(
        proof(&f.verify(&record).await)["decision"],
        "accepted",
        "restoring the indexes must reveal the original valid evidence"
    );
}

#[tokio::test]
async fn deleted_domain_binding_before_dispatch_fails_closed_without_native_preparation() {
    let mut f = Fixture::start().await;
    let source = f.source("missing-binding-source", 0).await;
    // Leave the HTTP router active while stopping dispatch/polling deterministically.
    for worker in f.server.workers.drain(..) {
        worker.abort();
        let _ = worker.await;
    }
    let record = f
        .create(f.request_body("missing-binding-assessment", &source))
        .await;
    let job_id = JobId::new(record["job_id"].as_str().unwrap());
    assert_eq!(
        f.app.store.get_job(&job_id).unwrap().status,
        JobStatus::Queued
    );
    assert!(f.journal.list().unwrap().is_empty());
    let db = sqlite(&f);
    assert_eq!(
        db.execute(
            "DELETE FROM venue_assessments WHERE assessment_id=?1",
            [record["assessment_id"].as_str().unwrap()]
        )
        .unwrap(),
        1
    );
    genesis_server::outbox::drain_once(&f.app).await.unwrap();
    let refused = f.app.store.get_job(&job_id).unwrap();
    assert_eq!(refused.status, JobStatus::Failed);
    assert!(refused.rejection_reason.is_some());
    assert!(
        f.journal.list().unwrap().is_empty(),
        "a missing domain authority record must not become a generic specialist job"
    );
    assert!(f.providers.execution_posts().is_empty());
}

#[tokio::test]
async fn fabricated_generic_result_cannot_replace_the_durable_native_execution_receipt() {
    let f = Fixture::start().await;
    let source = f.source("result-tamper-source", 3).await;
    f.providers.mode(ResponseMode::WrongCapacity);
    let record = f
        .create(f.request_body("result-tamper-assessment", &source))
        .await;
    f.collected(&record).await;
    let job_id = JobId::new(record["job_id"].as_str().unwrap());
    let mut job = f.app.store.get_job(&job_id).unwrap();
    let native = f.specialist.execution_receipt(&job, &binding()).unwrap();
    let forged_correct = structured_output(&job.input["context"], ResponseMode::Correct, &None);
    assert_ne!(
        native.output.as_ref().unwrap()["text"],
        serde_json::to_string(&forged_correct).unwrap()
    );
    job.result = Some(
        json!({"kind":"specialist_run_output","provider":"creao","run_id":native.run_id,
        "agent_id":AGENT,"domain_verified":true,"output":{"text":serde_json::to_string(&forged_correct).unwrap()}}),
    );
    let db = sqlite(&f);
    assert_eq!(
        db.execute(
            "UPDATE jobs SET job_json=?2 WHERE job_id=?1",
            rusqlite::params![job_id.as_str(), serde_json::to_string(&job).unwrap()]
        )
        .unwrap(),
        1
    );
    assert_eq!(
        f.app.store.get_job(&job_id).unwrap().result,
        job.result,
        "the forged generic result must actually be installed"
    );
    let requests = f.providers.world.lock().unwrap().seen.len();
    let counts = f.side_effect_counts();
    let rejected = f.verify(&record).await;
    assert_eq!(proof(&rejected)["decision"], "semantic_rejected");
    assert_eq!(proof(&rejected)["run_id"], native.run_id);
    assert_eq!(f.side_effect_counts(), counts);
    assert_eq!(f.providers.world.lock().unwrap().seen.len(), requests);
    assert_eq!(
        f.specialist
            .execution_receipt(&job, &binding())
            .unwrap()
            .output_sha256,
        native.output_sha256
    );
}
