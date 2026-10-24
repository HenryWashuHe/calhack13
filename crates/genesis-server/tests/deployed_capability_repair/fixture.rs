//! Loopback adapters exercise real journals; generated source is never executed
//! locally. Fixture provider output is not evidence of live model/Actor quality.
use axum::{
    body::to_bytes,
    extract::{Request, State},
    Json, Router,
};
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::{
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap},
};
use genesis_forge::{AnthropicGenerator, ModelConfig};
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    error::validation_error,
    parse_publishers,
    registry::{self, RegistryState},
    store::EventMeta,
    AcquisitionConfig, OrgConfig,
};
use genesis_verification::{decode_json, ExecutionEvidence, OracleSuite};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;

pub const ORACLE: &str = "PRIVATE_DEPLOYED_REPAIR_ORACLE";
pub const PUBLIC_HTML: &str = "PUBLIC_REPAIR_EXAMPLE_HTML";
pub const OLD_PARSER: &str =
    "export function extract() { throw new Error('old loopback parser, never executed locally'); }";
pub const NEW_PARSER: &str = "export function extract() { throw new Error('replacement loopback parser, never executed locally'); }";
const MODEL: &str = "claude-sonnet-4-6";
const KEY: &str = "deployed-repair-loopback-key";

pub fn suite() -> OracleSuite {
    let mut value: OracleSuite = decode_json(include_bytes!(
        "../../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    value.scope = ORACLE.into();
    value
}

pub struct Server {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, task }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub struct World {
    pub wrong_output: bool,
    pub unchanged_parser: bool,
    pub unknown_actor: bool,
    pub requests: Vec<(String, String, Value)>,
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
        self.requests
            .push((method.into(), path.into(), body.clone()));
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "messages"]) => {
                let parser = if self.actors.is_empty() || self.unchanged_parser {
                    OLD_PARSER
                } else {
                    NEW_PARSER
                };
                (
                    StatusCode::OK,
                    json!({"id":format!("msg_repair_{}", self.requests.len()),"type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":json!({"extractor_typescript":parser}).to_string()}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":120,"output_tokens":80}}),
                )
            }
            ("POST", ["v2", "actors"]) if self.unknown_actor => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"private ambiguous Actor response"}),
            ),
            ("POST", ["v2", "actors"]) => {
                let id = format!("actor-repair-{}", self.actors.len());
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
            ("GET", ["v2", "actor-builds", build]) => (
                StatusCode::OK,
                self.build(build.strip_prefix("build-").unwrap()),
            ),
            ("POST", ["v2", "actors", actor, "runs"]) => {
                let suite = suite();
                let index = suite
                    .cases
                    .iter()
                    .position(|case| case.input == body)
                    .expect("configured input");
                let receipts: Vec<ExecutionEvidence> = decode_json(include_bytes!(
                    "../../../genesis-verification/examples/fixtures/runs.json"
                ))
                .unwrap();
                let mut output = receipts[index].output.clone();
                if self.wrong_output && index == 0 {
                    output["venues"][0]["capacity"] = json!(9999);
                }
                let id = format!("run-repair-{}", self.runs.len());
                self.runs.insert(id.clone(), ((*actor).into(), output));
                (StatusCode::CREATED, self.run(&id))
            }
            ("GET", ["v2", "actor-runs", run]) => (StatusCode::OK, self.run(run)),
            ("GET", ["v2", "datasets", dataset, "items"]) => (
                StatusCode::OK,
                json!([self.runs[dataset.strip_prefix("dataset-").unwrap()].1]),
            ),
            _ => panic!("unexpected loopback request {method} {path}"),
        }
    }
}

pub struct Providers {
    pub world: Arc<Mutex<World>>,
    server: Server,
}
impl Providers {
    async fn start() -> Self {
        let world = Arc::new(Mutex::new(World::default()));
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
            .with_state(world.clone());
        Self {
            world,
            server: Server::start(router).await,
        }
    }
    fn apify(&self) -> ApifyClient {
        ApifyClient::for_local_testing(
            KEY,
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
    fn executor(&self) -> ProviderSet {
        ProviderSet::new()
            .with_dev_stub(DevStub::default())
            .with_executor(Arc::new(ApifyCapabilityExecutor::new(self.apify())))
    }
    fn acquisition(&self) -> AcquisitionConfig {
        AcquisitionConfig {
            policy: AcquisitionPolicy {
                model: ModelConfig {
                    model: MODEL.into(),
                    max_tokens: 1024,
                },
                quotas: AcquisitionQuotas::default(),
                max_duration_seconds: 3600,
            },
            automatic: false,
            model: Some(Arc::new(
                AnthropicGenerator::with_test_endpoint(
                    &format!("{}/v1/messages", self.server.url),
                    KEY,
                )
                .unwrap(),
            )),
            apify: Some(Arc::new(self.apify())),
        }
    }
    pub fn count(&self, method: &str, suffix: &str) -> usize {
        self.world
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(m, p, _)| m == method && p.ends_with(suffix))
            .count()
    }
    pub fn requests(&self) -> usize {
        self.world.lock().unwrap().requests.len()
    }
}

pub struct Org {
    pub app: AppState,
    pub config: OrgConfig,
    server: Server,
    http: reqwest::Client,
}
impl Org {
    async fn open(config: OrgConfig, providers: &Providers) -> Self {
        let app = build_org(&config, Some(providers.executor())).unwrap();
        let server = Server::start(app.clone().router()).await;
        Self {
            app,
            config,
            server,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }
    pub async fn restart(&mut self, providers: &Providers) {
        self.server.task.abort();
        let _ = (&mut self.server.task).await;
        *self = Self::open(self.config.clone(), providers).await;
    }
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        principal: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .http
            .request(method, format!("{}/api/v1{path}", self.server.url))
            .bearer_auth(format!("{}-{principal}", self.config.org_id));
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
    pub async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.request(Method::POST, path, "operator", Some(body))
            .await
    }
    pub async fn get(&self, path: &str) -> Value {
        let (status, body) = self.request(Method::GET, path, "operator", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
    pub async fn drive(&self, id: &str) -> Value {
        for _ in 0..32 {
            let value = self.get(&format!("/capability-acquisitions/{id}")).await;
            if [
                "verified",
                "rejected",
                "failed",
                "needs_reconciliation",
                "fenced",
            ]
            .contains(&value["phase"].as_str().unwrap())
            {
                return value;
            }
            let (status, body) = self
                .post(
                    &format!("/capability-acquisitions/{id}/step"),
                    json!({"retry_read":false}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        panic!("acquisition exceeded bounded fixture transitions")
    }
    pub async fn publish(&self, id: &str) -> Value {
        let candidate = self.app.acquisition_runtime.verified(id).await.unwrap();
        let pin = candidate.artifact;
        let manifest = json!({"capability_id":pin.capability_id,"version":pin.capability_version,"publisher":self.config.org_id,"source_digest":pin.source_sha256,
            "intent":"Read public fixture venues","input_schema":{"type":"object"},"output_schema":{"type":"object"},"required_permissions":["capability.venue_lookup"],
            "artifact":{"actor_id":pin.actor_id,"build_id":pin.build_id,"build_number":pin.build_number},"role_template":null,"responsibility_template":null,"workflow_procedure":null,
            "acceptance_suite_ref":"untrusted-placeholder","verification":{"passed":false,"cases":[],"tested_environment":"caller assertion","oracle_ref":"untrusted","verified_at":Utc::now()},"state":"APPROVED","published_at":null});
        let (status, body) = self
            .post(
                "/capabilities/publish",
                json!({"manifest":manifest,"verification_ref":format!("acquisition:{id}")}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["verification"]["passed"], true);
        body
    }
    pub async fn collect(&self, job: &str) {
        for _ in 0..2 {
            let (status, body) = self
                .post(
                    &format!("/capability-runs/{job}/step"),
                    json!({"retry_read":false}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        assert_eq!(
            self.get(&format!("/capability-runs/{job}")).await["phase"],
            "collected"
        );
    }
    pub async fn install_accept(&self, intent: &Value, version: &str) -> String {
        let (status, installed) = self
            .post(
                "/capability-installations",
                json!({"capability_id":"deployed-venue-reader","version":version}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{installed}");
        assert_eq!(installed["synthesis_calls"], 0);
        let id = installed["installation_id"].as_str().unwrap().to_owned();
        let (status, checks) = self.post(&format!("/capability-installations/{id}/checks"), json!({"client_request_id":format!("checks-{version}"),"role_id":intent["role_id"],"responsibility_id":intent["responsibility_id"],"suite_id":suite().suite_id})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{checks}");
        let jobs = checks["job_ids"].as_array().unwrap();
        for job in jobs {
            self.collect(job.as_str().unwrap()).await;
        }
        let (status, accepted) = self
            .post(
                &format!("/capability-installations/{id}/acceptance"),
                json!({"suite_id":suite().suite_id,"job_ids":jobs}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{accepted}");
        assert_eq!(accepted["installation"]["status"], "ACCEPTED");
        id
    }
    pub async fn attach(&self, intent: &Value, version: &str) {
        let state = self.app.current_state().unwrap();
        let evidence = state.requirements
            [&genesis_core::ids::RequirementId::new(intent["requirement_id"].as_str().unwrap())]
            .triggering_evidence
            .clone();
        let proposal: OrgChangeProposal = serde_json::from_value(json!({"expected_org_revision":state.revision,"trigger_evidence_ids":evidence,"rationale":"Use independently accepted version", "operations":[{"op":"AttachCapability","role":{"kind":"existing","role_id":intent["role_id"]},"capability":{"capability_id":"deployed-venue-reader","version":version}}]})).unwrap();
        let allocation = KeyAllocation::allocate(&proposal);
        self.app
            .mutate(EventMeta::default(), |state| {
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
    }
}

pub async fn seed(app: &AppState) -> Value {
    let spec: MissionBootstrap = serde_json::from_value(json!({"goal":"Maintain verified venue extraction", "roles":[{"key":"owner","name":"Owner","charter":"Own parser development and operation","authority":{"scopes":["capability.acquire","capability.venue_lookup"],"budget_cents":10000}}],"responsibilities":[{"purpose":"Coordinate parser work","owner":"owner"}]})).unwrap();
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
                "fixture",
                "Need reliable venue extraction",
                None,
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
                "Extract venue listings",
                std::slice::from_ref(&observation),
                "Pass independent venue oracle",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let role = state.roles.keys().next().unwrap().clone();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({"expected_org_revision":state.revision,"trigger_evidence_ids":[observation],"rationale":"Assign acquisition responsibility","operations":[{"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"parser","requirement_id":requirement,"purpose":"Develop and maintain the parser","trigger":"Extraction need","termination":"No continuing parser need"},"owner":{"kind":"existing","role_id":role}}]})).unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    let responsibility = allocation.responsibilities["parser"].clone();
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
    let suite = suite();
    let urls: Vec<_> = suite
        .cases
        .iter()
        .map(|case| case.source.url.clone())
        .collect();
    json!({"client_request_id":"parent-acquisition","mission_id":mission,"requirement_id":requirement,"role_id":role,"responsibility_id":responsibility,"suite_id":suite.suite_id,
        "candidate":{"capability_id":"deployed-venue-reader","capability_version":"1.0.0","actor_name":"deployed-reader-original","source_version":"0.1"},"repair":null,"allowed_source_urls":urls,"examples":[{"url":urls[0],"html":format!("<html>{PUBLIC_HTML}</html>")}],"deadline":(Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()})
}

pub struct Fixture {
    _registry: Server,
    directory: tempfile::TempDir,
    pub providers: Providers,
    pub org: Org,
    pub intent: Value,
    pub parent_id: String,
    pub parent_manifest: Value,
    pub installation_id: String,
}
impl Fixture {
    pub async fn start() -> Self {
        let registry = Server::start(registry::router(
            RegistryState::open_in_memory(
                parse_publishers("org_a:org_a-registry,org_b:org_b-registry"),
                vec![],
            )
            .unwrap(),
        ))
        .await;
        let directory = tempfile::tempdir().unwrap();
        let providers = Providers::start().await;
        let mut config = OrgConfig::dev("org_a", "Deployed repair fixture A");
        config.db = Some(directory.path().join("a.sqlite"));
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        config.registry_url = Some(registry.url.clone());
        config.registry_token = Some("org_a-registry".into());
        config.oracle_suites = vec![suite()];
        config.acquisition = Some(providers.acquisition());
        let org = Org::open(config, &providers).await;
        let intent = seed(&org.app).await;
        let (status, parent) = org.post("/capability-acquisitions", intent.clone()).await;
        assert_eq!(status, StatusCode::CREATED, "{parent}");
        let parent_id = parent["id"].as_str().unwrap().to_owned();
        let parent_status = org.drive(&parent_id).await;
        assert_eq!(parent_status["phase"], "verified", "{parent_status}");
        let parent_manifest = org.publish(&parent_id).await;
        let installation_id = org.install_accept(&intent, "1.0.0").await;
        org.attach(&intent, "1.0.0").await;
        Self {
            _registry: registry,
            directory,
            providers,
            org,
            intent,
            parent_id,
            parent_manifest,
            installation_id,
        }
    }
    pub async fn invocation(&self, key: &str, wrong: bool) -> (String, Value) {
        self.providers.world.lock().unwrap().wrong_output = wrong;
        let (status,job) = self.org.post("/capability-runs",json!({"client_request_id":key,"installation_id":self.installation_id,"role_id":self.intent["role_id"],"responsibility_id":self.intent["responsibility_id"],"suite_id":suite().suite_id,"input":suite().cases[0].input})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{job}");
        let id = job["job_id"].as_str().unwrap().to_owned();
        self.org.collect(&id).await;
        self.providers.world.lock().unwrap().wrong_output = false;
        let (status, proof) = self
            .org
            .post(&format!("/capability-runs/{id}/verify"), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK, "{proof}");
        assert_eq!(
            proof["decision"],
            if wrong {
                "semantic_rejected"
            } else {
                "verified"
            }
        );
        (id, proof)
    }
    pub fn repair_request(&self, job: &str) -> Value {
        let mut acquisition = self.intent.clone();
        acquisition["client_request_id"] = json!("deployed-repair-once");
        acquisition["candidate"]["capability_version"] = json!("1.0.1");
        acquisition["candidate"]["actor_name"] = json!("deployed-reader-replacement");
        json!({"parent_acquisition_id":self.parent_id,"failure_job_id":job,"acquisition":acquisition})
    }
    pub async fn new_org_b(&self) -> (Org, Value) {
        let mut config = OrgConfig::dev("org_b", "Independent receiving organization B");
        config.db = Some(self.directory.path().join("b.sqlite"));
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        config.registry_url = self.org.config.registry_url.clone();
        config.registry_token = Some("org_b-registry".into());
        config.oracle_suites = vec![suite()];
        let org = Org::open(config, &self.providers).await;
        let intent = seed(&org.app).await;
        (org, intent)
    }
    pub async fn remove_acquire_scope(&self) {
        let _guard = self.org.app.kernel.lock().await;
        let state = self.org.app.current_state().unwrap();
        let mut next = state.clone();
        let role = genesis_core::ids::RoleId::new(self.intent["role_id"].as_str().unwrap());
        next.roles
            .get_mut(&role)
            .unwrap()
            .authority
            .scopes
            .remove("capability.acquire");
        next.revision += 1;
        self.org
            .app
            .store
            .commit_state(
                state.revision,
                &next,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
}

pub fn assert_redacted(value: &Value) {
    let text = value.to_string();
    for private in [
        ORACLE,
        PUBLIC_HTML,
        OLD_PARSER,
        NEW_PARSER,
        "previous_extractor_typescript",
        "extractor_typescript",
    ] {
        assert!(!text.contains(private), "private repair payload: {text}");
    }
}
