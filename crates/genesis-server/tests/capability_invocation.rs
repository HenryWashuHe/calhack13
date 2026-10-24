//! Invocation finalization through HTTP with real backend Apify tickets and
//! loopback evidence. Initial publication uses an explicitly synthetic,
//! independently reverified policy fixture; no cloud execution is claimed.
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::{Json, Router};
use genesis_apify::{ApifyClient, RunLimits, SourceFile};
use genesis_forge::{CandidateSpec, ForgeSession, SourcePackage};
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::registry::{self, RegistryState};
use genesis_server::{build_org, parse_publishers, OrgConfig};
use genesis_verification::{decode_json, ArtifactPin, ExecutionEvidence, OracleSuite};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

const ORACLE_CANARY: &str = "private-backend-oracle-canary-never-send-to-actor";
const PROOF: &str = "synthetic-trusted-forge-policy-proof";

fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.scope = ORACLE_CANARY.into();
    suite
}

fn fixture_runs() -> Vec<ExecutionEvidence> {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap()
}

/// Synthetic trusted-storage fixture, independently reverified by the policy.
/// This checkpoint does not assert that a real provider built the test source.
fn checkpoint() -> ForgeSession {
    let spec = CandidateSpec {
        operation_id: PROOF.into(),
        capability_id: "offline-venue-extractor".into(),
        capability_version: "0.0.1".into(),
        actor_name: "synthetic-policy-fixture".into(),
        source_version: "0.0".into(),
        source: SourcePackage::new(vec![SourceFile {
            name: "main.js".into(),
            content: "// Offline policy fixture; never uploaded.\n".into(),
        }])
        .unwrap(),
    };
    let pin = ArtifactPin {
        capability_id: spec.capability_id.clone(),
        capability_version: spec.capability_version.clone(),
        source_sha256: spec.source.sha256().unwrap(),
        actor_id: "synthetic-actor".into(),
        build_id: "synthetic-build".into(),
        build_number: "0.0.1".into(),
    };
    let digest = spec.definition().definition_sha256().unwrap();
    let mut runs = fixture_runs();
    for run in &mut runs {
        run.artifact = pin.clone();
    }
    let mut saved = serde_json::to_value(ForgeSession::new(spec, &suite()).unwrap()).unwrap();
    saved["phase"] = json!("verified");
    saved["actor"] = json!({"schema_version":1,"actor_id":pin.actor_id,
        "version_number":"0.0","definition_sha256":digest});
    saved["build"] = json!({"schema_version":1,"actor_id":pin.actor_id,
        "build_id":pin.build_id,"build_number":pin.build_number,
        "version_number":"0.0","definition_sha256":digest});
    saved["pin"] = json!(pin);
    saved["receipts"] = json!(runs);
    serde_json::from_value(saved).unwrap()
}

fn manifest(publisher: &str) -> Value {
    let proof = checkpoint().verified(&suite()).unwrap();
    json!({
        "capability_id":proof.artifact.capability_id,"version":proof.artifact.capability_version,
        "publisher":publisher,"source_digest":proof.artifact.source_sha256,
        "intent":"Read fixture venue data","applicability_conditions":["configured fixture inputs"],
        "known_limitations":["offline test"],"input_schema":{"type":"object"},
        "output_schema":{"type":"object"},"required_permissions":["capability.venue_lookup"],
        "artifact":{"actor_id":proof.artifact.actor_id,"build_id":proof.artifact.build_id,
            "build_number":proof.artifact.build_number},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,
        "local_configuration_schema":{},"acceptance_suite_ref":suite().suite_id,
        "verification":{"passed":true,"cases":[
            {"name":"caller-a","kind":"success","passed":true,"detail":"caller claim"},
            {"name":"caller-b","kind":"success","passed":true,"detail":"caller claim"},
            {"name":"caller-c","kind":"unsupported","passed":true,"detail":"caller claim"}],
            "tested_environment":"caller assertion","limitations":[],
            "oracle_ref":"caller-supplied-report","verified_at":"2000-01-01T00:00:00Z"},
        "provenance":[],"state":"APPROVED","published_at":null,
    })
}

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
    process_shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            url,
            task,
            process_shutdown: None,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(shutdown) = self.process_shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
    }
}

async fn registry() -> Server {
    let publishers = parse_publishers("org_a:org_a-registry,org_b:org_b-registry");
    Server::start(registry::router(
        RegistryState::open_in_memory(publishers, vec![]).unwrap(),
    ))
    .await
}

struct Captured {
    method: String,
    path: String,
    body: Value,
}

struct MockState {
    pin: ArtifactPin,
    inputs: Vec<Value>,
    outputs: Vec<Value>,
    next_run: AtomicUsize,
    wrong: AtomicBool,
    cases_by_run: Mutex<BTreeMap<usize, usize>>,
    captured: Mutex<Vec<Captured>>,
}

struct ApifyMock {
    server: Server,
    state: Arc<MockState>,
}

impl ApifyMock {
    async fn start() -> Self {
        let state = Arc::new(MockState {
            pin: checkpoint().artifact().unwrap().clone(),
            inputs: suite().cases.into_iter().map(|case| case.input).collect(),
            outputs: fixture_runs().into_iter().map(|run| run.output).collect(),
            next_run: AtomicUsize::new(0),
            wrong: AtomicBool::new(false),
            cases_by_run: Mutex::new(BTreeMap::new()),
            captured: Mutex::new(Vec::new()),
        });
        let server = Server::start(
            Router::new()
                .fallback(apify_reply)
                .with_state(state.clone()),
        )
        .await;
        Self { server, state }
    }

    fn providers(&self) -> ProviderSet {
        let client = ApifyClient::for_local_testing(
            "synthetic-api-policy-token",
            RunLimits {
                request_timeout: Duration::from_secs(2),
                deadline: Duration::from_secs(3),
                poll_interval: Duration::from_millis(1),
                ..RunLimits::default()
            },
            &self.server.url,
        )
        .unwrap();
        // The real executor takes precedence over the explicit role fallback.
        // This also checks that a dev flag cannot enable raw proof assertions
        // while actual capability execution uses Apify.
        ProviderSet::new()
            .with_dev_stub(DevStub::default())
            .with_executor(Arc::new(ApifyCapabilityExecutor::new(client)))
    }

    fn submissions(&self) -> usize {
        self.state
            .captured
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "POST")
            .count()
    }

    fn assert_no_oracle_or_synthesis_requests(&self) {
        for request in self.state.captured.lock().unwrap().iter() {
            let body = request.body.to_string();
            assert!(!body.contains(ORACLE_CANARY));
            assert!(!body.contains("\"expected\""));
            assert!(!body.contains("\"venues\""));
            assert!(!body.contains("sourceFiles"));
            if request.method == "POST" {
                assert_eq!(request.path, "/v2/actors/synthetic-actor/runs");
            }
        }
    }
}

async fn apify_reply(
    State(state): State<Arc<MockState>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    assert_eq!(
        request.headers()["authorization"],
        "Bearer synthetic-api-policy-token"
    );
    let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    state.captured.lock().unwrap().push(Captured {
        method: method.clone(),
        path: path.clone(),
        body: body.clone(),
    });
    let pin = &state.pin;
    if method == "GET" && path == format!("/v2/actor-builds/{}", pin.build_id) {
        return (
            StatusCode::OK,
            Json(json!({"data":{"id":pin.build_id,"actId":pin.actor_id,
            "buildNumber":pin.build_number,"status":"SUCCEEDED"}})),
        );
    }
    if method == "POST" && path == format!("/v2/actors/{}/runs", pin.actor_id) {
        let Some(case) = state.inputs.iter().position(|input| *input == body) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"unexpected test input"})),
            );
        };
        let run = state.next_run.fetch_add(1, Ordering::SeqCst);
        state.cases_by_run.lock().unwrap().insert(run, case);
        return (StatusCode::OK, Json(run_response(pin, run)));
    }
    if method == "GET" {
        if let Some(run) = path
            .strip_prefix("/v2/actor-runs/synthetic-run-")
            .and_then(|s| s.parse::<usize>().ok())
        {
            if state.cases_by_run.lock().unwrap().contains_key(&run) {
                return (StatusCode::OK, Json(run_response(pin, run)));
            }
        }
        if let Some(run) = path
            .strip_prefix("/v2/datasets/synthetic-dataset-")
            .and_then(|s| s.strip_suffix("/items"))
            .and_then(|s| s.parse::<usize>().ok())
        {
            if let Some(case) = state.cases_by_run.lock().unwrap().get(&run) {
                let mut output = state.outputs[*case].clone();
                if *case == 0 && state.wrong.load(Ordering::SeqCst) {
                    output["venues"][0]["capacity"] = json!(9999);
                }
                return (StatusCode::OK, Json(json!([output])));
            }
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error":"unexpected test path"})),
    )
}

fn run_response(pin: &ArtifactPin, run: usize) -> Value {
    json!({"data":{"id":format!("synthetic-run-{run}"),"actId":pin.actor_id,
        "buildId":pin.build_id,"status":"SUCCEEDED","defaultDatasetId":format!("synthetic-dataset-{run}")}})
}

struct Org {
    app: genesis_server::app::AppState,
    server: Server,
    id: String,
    http: reqwest::Client,
}

impl Org {
    async fn open(config: &OrgConfig, mock: &ApifyMock) -> Self {
        let app = build_org(config, Some(mock.providers())).unwrap();
        let server = Server::start(app.clone().router()).await;
        Self {
            app,
            server,
            id: config.org_id.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        principal: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .http
            .request(method, format!("{}/api/v1{path}", self.server.url))
            .bearer_auth(format!("{}-{principal}", self.id));
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
        self.request(reqwest::Method::POST, path, "operator", Some(body))
            .await
    }

    async fn get(&self, path: &str) -> Value {
        let (status, body) = self
            .request(reqwest::Method::GET, path, "operator", None)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn publish(&self) -> Value {
        let (status, body) = self
            .post(
                "/capabilities/publish",
                json!({"manifest":manifest(&self.id),"verification_ref":PROOF}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    async fn seed_and_install(&self) -> Installed {
        let (status, mission) = self.post("/missions", json!({
            "goal":"Check fixture venue availability","constraints":[],"success_checks":["oracle accepted"],
            "roles":[{"key":"runner","name":"Runner","charter":"Own venue checks",
                "authority":{"scopes":["capability.venue_lookup"],"budget_cents":10000}}],
            "responsibilities":[{"purpose":"Read venue availability","owner":"runner"}],
        })).await;
        assert_eq!(status, StatusCode::CREATED, "{mission}");
        let state = self.get("/organization").await;
        let role = state["roles"].as_object().unwrap().values().next().unwrap()["role_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let responsibility = state["responsibilities"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["responsibility_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, installation) = self
            .post(
                "/capability-installations",
                json!({"capability_id":"offline-venue-extractor","version":"0.0.1"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{installation}");
        assert_eq!(installation["synthesis_calls"], 0);
        assert_eq!(
            installation["manifest_source_digest"],
            checkpoint().artifact().unwrap().source_sha256
        );
        Installed {
            role,
            responsibility,
            mission: mission["mission_id"].as_str().unwrap().into(),
            id: installation["installation_id"].as_str().unwrap().into(),
        }
    }

    async fn checks(&self, installed: &Installed) -> Vec<String> {
        let path = format!("/capability-installations/{}/checks", installed.id);
        let body = json!({"client_request_id":"local-checks","role_id":installed.role,
            "responsibility_id":installed.responsibility,"suite_id":suite().suite_id});
        let (status, denied) = self
            .request(reqwest::Method::POST, &path, "agent", Some(body.clone()))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
        let (status, response) = self.post(&path, body).await;
        assert!(status.is_success(), "{status} {response}");
        let ids: Vec<String> = response["job_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().into())
            .collect();
        assert_eq!(ids.len(), 3);
        ids
    }

    async fn collect(&self, ids: &[String]) {
        for id in ids {
            for _ in 0..2 {
                let (status, response) = self
                    .post(
                        &format!("/capability-runs/{id}/step"),
                        json!({"retry_read":false}),
                    )
                    .await;
                assert_eq!(status, StatusCode::OK, "{response}");
            }
            let response = self.get(&format!("/capability-runs/{id}")).await;
            assert_eq!(response["phase"], "collected", "{response}");
        }
    }

    async fn accept(&self, installed: &Installed, ids: &[String]) -> Value {
        let (status, response) = self
            .post(
                &format!("/capability-installations/{}/acceptance", installed.id),
                json!({"suite_id":suite().suite_id,"job_ids":ids}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{response}");
        assert_eq!(response["installation"]["status"], "ACCEPTED");
        response
    }

    async fn attach(&self, installed: &Installed) {
        let (status, observation) = self.post(&format!("/missions/{}/observations", installed.mission), json!({"source":"operator","summary":"Accepted venue capability is required","evidence_ref":"fixture://request"})).await;
        assert_eq!(status, StatusCode::CREATED, "{observation}");
        let revision = self.get("/organization").await["revision"]
            .as_u64()
            .unwrap();
        let (status, change) = self.post("/org-changes", json!({"client_request_id":"attach-local-capability","proposal":{
            "expected_org_revision":revision,"trigger_evidence_ids":[observation["observation_id"]],
            "rationale":"Use the locally accepted fixture capability",
            "operations":[{"op":"AttachCapability","role":{"kind":"existing","role_id":installed.role},
                "capability":{"capability_id":"offline-venue-extractor","version":"0.0.1"}}]}})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{change}");
        let id = change["change_id"].as_str().unwrap();
        for _ in 0..100 {
            let result = self.get(&format!("/org-changes/{id}")).await;
            if result["status"] == "COMMITTED" {
                return;
            }
            assert!(
                result["status"] != "FAILED" && result["status"] != "REJECTED",
                "{result}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("attachment did not commit");
    }
}

struct Installed {
    id: String,
    role: String,
    responsibility: String,
    mission: String,
}

struct Fixture {
    _registry: Server,
    mock: ApifyMock,
    _database: tempfile::TempDir,
    config: OrgConfig,
    org: Org,
    installed: Installed,
    acceptance_jobs: Vec<String>,
    acceptance_ref: Value,
}

impl Fixture {
    async fn start() -> Self {
        let registry = registry().await;
        let mock = ApifyMock::start().await;
        let database = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev("org_a", "Invocation fixture");
        config.db = Some(database.path().join("organization.sqlite"));
        config.registry_url = Some(registry.url.clone());
        config.registry_token = Some("org_a-registry".into());
        config.oracle_suites = vec![suite()];
        config.forge_checkpoints = vec![checkpoint()];
        let org = Org::open(&config, &mock).await;
        org.publish().await;
        let installed = org.seed_and_install().await;
        let acceptance_jobs = org.checks(&installed).await;
        org.collect(&acceptance_jobs).await;
        let accepted = org.accept(&installed, &acceptance_jobs).await;
        org.attach(&installed).await;
        let acceptance_ref = accepted["installation"]["acceptance_report_ref"].clone();
        Self {
            _registry: registry,
            mock,
            _database: database,
            config,
            org,
            installed,
            acceptance_jobs,
            acceptance_ref,
        }
    }

    async fn prepare(&self, key: &str, case: usize) -> String {
        let (status, job) = self
            .org
            .post(
                "/capability-runs",
                json!({
                    "client_request_id":key,"installation_id":self.installed.id,
                    "role_id":self.installed.role,"responsibility_id":self.installed.responsibility,
                    "suite_id":suite().suite_id,"input":suite().cases[case].input
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{job}");
        job["job_id"].as_str().unwrap().to_owned()
    }

    async fn collected(&self, key: &str, case: usize) -> String {
        let id = self.prepare(key, case).await;
        self.org.collect(std::slice::from_ref(&id)).await;
        assert_eq!(
            self.org.get(&format!("/jobs/{id}")).await["status"],
            "VERIFYING"
        );
        id
    }

    async fn verify(&self, id: &str) -> Value {
        let (status, proof) = self
            .org
            .post(&format!("/capability-runs/{id}/verify"), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK, "{proof}");
        proof
    }

    async fn events(&self) -> Value {
        self.org.get("/events/log?after=0&limit=5000").await
    }

    async fn assert_no_commitment(&self) {
        assert!(self.org.get("/ledger").await.as_array().unwrap().is_empty());
        let state = self.org.get("/organization").await;
        assert_eq!(
            state["responsibilities"][&self.installed.responsibility]["status"],
            "OPEN"
        );
        assert_eq!(
            state["installations"][&self.installed.id]["acceptance_report_ref"],
            self.acceptance_ref
        );
        self.mock.assert_no_oracle_or_synthesis_requests();
    }

    async fn retire_verifier(&mut self) {
        let (status, obs) = self
            .org
            .post(
                &format!("/missions/{}/observations", self.installed.mission),
                json!({"source":"operator","summary":"Transfer continuing lookup responsibility"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{obs}");
        let state = self.org.get("/organization").await;
        let (status, change) = self.org.post("/org-changes",json!({
            "client_request_id":"retire-verifier", "proposal":{
                "expected_org_revision":state["revision"],"trigger_evidence_ids":[obs["observation_id"]],
                "rationale":"A successor inherits the accepted capability and continuing work",
                "operations":[
                    {"op":"CreateRole","key":"successor","name":"Successor","charter":"Continue lookup",
                        "authority":{"scopes":["capability.venue_lookup"],"budget_cents":10000},
                        "provider":"internal","justification":"lifecycle"},
                    {"op":"RetireRole","role_id":self.installed.role,"handoffs":[
                        {"responsibility_id":self.installed.responsibility,"to":{"kind":"new","key":"successor"}}]},
                    {"op":"AttachCapability","role":{"kind":"new","key":"successor"},
                        "capability":{"capability_id":"offline-venue-extractor","version":"0.0.1"}}
                ]
            }
        })).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{change}");
        for _ in 0..100 {
            let saved = self
                .org
                .get(&format!(
                    "/org-changes/{}",
                    change["change_id"].as_str().unwrap()
                ))
                .await;
            if saved["status"] == "COMMITTED" {
                let state = self.org.get("/organization").await;
                assert_eq!(state["roles"][&self.installed.role]["state"], "RETIRED");
                self.installed.role = state["roles"]
                    .as_object()
                    .unwrap()
                    .values()
                    .find(|r| r["name"] == "Successor")
                    .unwrap()["role_id"]
                    .as_str()
                    .unwrap()
                    .into();
                return;
            }
            assert!(
                !["FAILED", "REJECTED"].contains(&saved["status"].as_str().unwrap()),
                "{saved}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("retirement did not commit");
    }
}

fn assert_redacted(proof: &Value) {
    let encoded = proof.to_string();
    for secret in [
        ORACLE_CANARY,
        "synthetic-api-policy-token",
        "\"expected\"",
        "\"venues\"",
        "main.js",
        "sourceFiles",
    ] {
        assert!(!encoded.contains(secret), "proof leaked {secret}: {proof}");
    }
}

#[tokio::test]
async fn matching_invocation_finalizes_once_and_survives_retirement_and_restart() {
    let mut f = Fixture::start().await;
    let id = f.collected("matched", 0).await;
    let before = f.events().await.as_array().unwrap().len();
    let requests = f.mock.state.captured.lock().unwrap().len();
    let proof = f.verify(&id).await;
    assert_redacted(&proof);
    assert_proof(&proof, &id, "verified", Some("supported"));
    let collected = f.org.get(&format!("/capability-runs/{id}")).await;
    assert_eq!(proof["installation_id"], f.installed.id);
    assert_eq!(proof["acceptance_report_ref"], f.acceptance_ref);
    assert_eq!(proof["request_sha256"], collected["request_sha256"]);
    assert_eq!(
        proof["case_report"]["run_id"],
        collected["evidence"]["run_id"]
    );
    assert_eq!(
        proof["case_report"]["dataset_id"],
        collected["evidence"]["dataset_id"]
    );
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "SUCCEEDED"
    );
    let events = f.events().await;
    assert!(events.as_array().unwrap().len() > before);
    assert_eq!(
        events
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["type"] == "job_status_changed"
                && event["payload"]["job_id"] == id
                && event["payload"]["to"] == "SUCCEEDED")
            .count(),
        1
    );
    assert_eq!(f.verify(&id).await, proof);
    assert_eq!(
        f.org
            .get(&format!("/capability-runs/{id}/verification"))
            .await,
        proof
    );
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    f.assert_no_commitment().await;
    f.retire_verifier().await;
    let retired_events = f.events().await;
    assert_eq!(
        f.verify(&id).await,
        proof,
        "immutable proof must survive original executor retirement"
    );
    assert_eq!(f.events().await, retired_events);
    f.org.server.task.abort();
    (&mut f.org.server.task).await.unwrap_err();
    f.config.forge_checkpoints.clear();
    f.org = Org::open(&f.config, &f.mock).await;
    assert_eq!(f.verify(&id).await, proof);
    assert_eq!(
        f.org
            .get(&format!("/capability-runs/{id}/verification"))
            .await,
        proof
    );
    assert_eq!(f.events().await, retired_events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    let successor = f.collected("successor-after-restart", 0).await;
    let second = f.verify(&successor).await;
    assert_redacted(&second);
    assert_proof(&second, &successor, "verified", Some("supported"));
    assert_ne!(
        second, proof,
        "a fresh invocation needs a distinct receipt-bound proof"
    );
    assert_eq!(
        f.org.get(&format!("/jobs/{successor}")).await["status"],
        "SUCCEEDED"
    );
    assert_eq!(f.mock.submissions(), 5);
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn wrong_semantic_result_is_rejected_despite_successful_provider_status() {
    let f = Fixture::start().await;
    f.mock.state.wrong.store(true, Ordering::SeqCst);
    let id = f.collected("wrong-capacity", 0).await;
    let count = f.mock.state.captured.lock().unwrap().len();
    let proof = f.verify(&id).await;
    assert_redacted(&proof);
    assert_proof(&proof, &id, "semantic_rejected", Some("supported"));
    assert!(proof["case_report"]["issues"]
        .as_array()
        .unwrap()
        .contains(&json!("value_mismatch")));
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "RESULT_REJECTED"
    );
    let events = f.events().await;
    assert_eq!(f.verify(&id).await, proof);
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), count);
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn correctly_unsupported_case_matches_without_claiming_a_supported_result() {
    let f = Fixture::start().await;
    let id = f.collected("unsupported", 2).await;
    let proof = f.verify(&id).await;
    assert_redacted(&proof);
    assert_proof(&proof, &id, "verified", Some("unsupported"));
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "SUCCEEDED"
    );
    assert_eq!(f.mock.submissions(), 4);
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn verify_is_operator_only_requires_collection_and_never_imports_a_result() {
    let f = Fixture::start().await;
    let id = f.prepare("boundary", 0).await;
    let verify = format!("/capability-runs/{id}/verify");
    let read = format!("/capability-runs/{id}/verification");
    let count = f.mock.state.captured.lock().unwrap().len();
    let (status, _) = f
        .org
        .request(reqwest::Method::GET, &read, "operator", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    for principal in ["agent", "adapter"] {
        let (status, body) = f
            .org
            .request(reqwest::Method::POST, &verify, principal, Some(json!({})))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, body) = f
            .org
            .request(reqwest::Method::GET, &read, principal, None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    let (status, body) = f.org.post(&verify, json!({})).await;
    assert!(
        status.is_client_error(),
        "uncollected job was finalized: {body}"
    );
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), count);
    f.org.collect(std::slice::from_ref(&id)).await;
    let count = f.mock.state.captured.lock().unwrap().len();
    for forged in [
        json!([]),
        Value::Null,
        json!({"passed":true}),
        json!({"evidence":fixture_runs()[0]}),
        json!({"suite":suite()}),
        json!({"domain_verified":true}),
    ] {
        let (status, body) = f.org.post(&verify, forged).await;
        assert!(status.is_client_error(), "caller data accepted: {body}");
        assert_eq!(
            f.org.get(&format!("/jobs/{id}")).await["status"],
            "VERIFYING"
        );
    }
    for acceptance in &f.acceptance_jobs {
        let (status, body) = f
            .org
            .post(&format!("/capability-runs/{acceptance}/verify"), json!({}))
            .await;
        assert!(
            status.is_client_error(),
            "acceptance job finalized as invocation: {body}"
        );
        assert_eq!(
            f.org.get(&format!("/jobs/{acceptance}")).await["status"],
            "VERIFYING"
        );
    }
    let (status, _) = f
        .org
        .request(
            reqwest::Method::GET,
            "/capability-runs/job_other_org/verification",
            "operator",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), count);
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn first_finalization_after_executor_retirement_is_denied() {
    let mut f = Fixture::start().await;
    let id = f.collected("revoked-before-verify", 0).await;
    f.retire_verifier().await;
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    let proof = f.verify(&id).await;
    assert_proof(&proof, &id, "authority_rejected", None);
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "RESULT_REJECTED"
    );
    let rejected_events = f.events().await;
    assert!(rejected_events.as_array().unwrap().len() > events.as_array().unwrap().len());
    assert_eq!(f.verify(&id).await, proof);
    assert_eq!(f.events().await, rejected_events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    let successor = f.collected("new-authority", 0).await;
    f.verify(&successor).await;
    assert_eq!(
        f.org.get(&format!("/jobs/{successor}")).await["status"],
        "SUCCEEDED"
    );
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn changed_backend_catalog_cannot_reinterpret_a_prepared_invocation() {
    let mut f = Fixture::start().await;
    let id = f.collected("catalog-bound", 0).await;
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    f.org.server.task.abort();
    (&mut f.org.server.task).await.unwrap_err();
    f.config.forge_checkpoints.clear();
    f.config.oracle_suites[0].scope = "A changed full-suite policy".into();
    f.org = Org::open(&f.config, &f.mock).await;
    let (status, body) = f
        .org
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert!(
        status.is_client_error(),
        "changed catalog finalized old invocation: {body}"
    );
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "VERIFYING"
    );
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    f.assert_no_commitment().await;
}

fn assert_proof(proof: &Value, job: &str, decision: &str, extraction: Option<&str>) {
    assert_eq!(proof["schema_version"], 1);
    assert_eq!(proof["job_id"], job);
    assert_eq!(proof["decision"], decision, "{proof}");
    assert_eq!(proof["suite_id"], suite().suite_id);
    assert_eq!(
        proof["suite_sha256"],
        genesis_verification::json_sha256(&json!(suite()))
    );
    let hash = proof["proof_ref"]
        .as_str()
        .unwrap()
        .strip_prefix("invocation:")
        .unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    if let Some(extraction) = extraction {
        assert_eq!(proof["case_report"]["purpose"], "invocation_case");
        assert_eq!(proof["case_report"]["extraction"], extraction);
        assert_eq!(
            proof["case_report"]["decision"],
            if decision == "verified" {
                "accepted"
            } else {
                "rejected"
            }
        );
        assert_eq!(
            proof["case_report"]["artifact"],
            json!(checkpoint().artifact().unwrap())
        );
    } else {
        assert!(proof["case_report"].is_null());
    }
}

#[tokio::test]
async fn first_finalization_rechecks_scopes_attachment_lease_and_mission() {
    for lost in ["scope", "attachment", "lease", "mission"] {
        let f = Fixture::start().await;
        let id = f.collected(lost, 0).await;
        let state = f.org.app.current_state().unwrap();
        let mut changed = state.clone();
        let role = genesis_core::ids::RoleId::new(&f.installed.role);
        match lost {
            "scope" => changed
                .roles
                .get_mut(&role)
                .unwrap()
                .authority
                .scopes
                .clear(),
            "attachment" => changed.roles.get_mut(&role).unwrap().capabilities.clear(),
            "lease" => {
                let responsibility =
                    genesis_core::ids::ResponsibilityId::new(&f.installed.responsibility);
                let assignment = changed
                    .active_assignment(&responsibility)
                    .unwrap()
                    .assignment_id
                    .clone();
                changed
                    .assignments
                    .get_mut(&assignment)
                    .unwrap()
                    .lease_expires_at = Some(chrono::Utc::now());
            }
            "mission" => {
                changed
                    .missions
                    .get_mut(&genesis_core::ids::MissionId::new(&f.installed.mission))
                    .unwrap()
                    .status = genesis_core::lifecycle::MissionStatus::Failed
            }
            _ => unreachable!(),
        }
        changed.revision += 1;
        // Trusted state change models a policy/lease transition after collection.
        f.org
            .app
            .store
            .commit_state(
                state.revision,
                &changed,
                vec![],
                &Default::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        let requests = f.mock.state.captured.lock().unwrap().len();
        let proof = f.verify(&id).await;
        assert_proof(&proof, &id, "authority_rejected", None);
        assert_eq!(
            f.org.get(&format!("/jobs/{id}")).await["status"],
            "RESULT_REJECTED",
            "{lost}"
        );
        assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
        f.assert_no_commitment().await;
    }
}

#[tokio::test]
async fn failed_verification_transaction_leaves_no_proof_status_or_event() {
    let f = Fixture::start().await;
    let id = f.collected("atomic-finalization", 0).await;
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    let connection = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_verification_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT, 'fixture commit failure'); END;").unwrap();
    let (status, body) = f
        .org
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert!(status.is_server_error(), "{status} {body}");
    let (status, _) = f
        .org
        .request(
            reqwest::Method::GET,
            &format!("/capability-runs/{id}/verification"),
            "operator",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "VERIFYING"
    );
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    connection
        .execute_batch("DROP TRIGGER fail_verification_event;")
        .unwrap();
    let proof = f.verify(&id).await;
    assert_proof(&proof, &id, "verified", Some("supported"));
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn legacy_unbound_invocation_is_not_upgraded_by_request_replay() {
    let f = Fixture::start().await;
    let id = f.collected("legacy-unbound", 0).await;
    let connection = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    assert_eq!(
        connection
            .execute(
                "DELETE FROM capability_invocation_bindings WHERE job_id=?1",
                [&id]
            )
            .unwrap(),
        1
    );
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    let (status, job) = f
        .org
        .post(
            "/capability-runs",
            json!({
                "client_request_id":"legacy-unbound","installation_id":f.installed.id,
                "role_id":f.installed.role,"responsibility_id":f.installed.responsibility,
                "suite_id":suite().suite_id,"input":suite().cases[0].input
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["job_id"], id);
    let count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM capability_invocation_bindings WHERE job_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "request replay must not invent missing historical policy"
    );
    let (status, body) = f
        .org
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert!(status.is_client_error(), "{status} {body}");
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "VERIFYING"
    );
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
}

#[tokio::test]
async fn restored_binding_pin_and_proof_tampering_fail_without_new_effects() {
    let f = Fixture::start().await;
    let id = f.collected("storage-binding", 0).await;
    let connection = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    let binding: String = connection
        .query_row(
            "SELECT binding_json FROM capability_invocation_bindings WHERE job_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    for (field, replacement) in [
        ("job_id", "foreign-job"),
        ("installation_id", "foreign-installation"),
        ("request_sha256", "different-request"),
    ] {
        let mut changed: Value = serde_json::from_str(&binding).unwrap();
        changed[field] = json!(replacement);
        connection
            .execute(
                "UPDATE capability_invocation_bindings SET binding_json=?2 WHERE job_id=?1",
                rusqlite::params![id, changed.to_string()],
            )
            .unwrap();
        let (status, body) = f
            .org
            .post(&format!("/capability-runs/{id}/verify"), json!({}))
            .await;
        assert!(
            status.is_client_error(),
            "accepted changed {field}: {status} {body}"
        );
        assert_eq!(f.events().await, events);
    }
    connection
        .execute(
            "UPDATE capability_invocation_bindings SET binding_json=?2 WHERE job_id=?1",
            rusqlite::params![id, binding],
        )
        .unwrap();
    let manifest: String = connection
        .query_row(
            "SELECT manifest_json FROM capability_pins WHERE installation_id=?1",
            [&f.installed.id],
            |row| row.get(0),
        )
        .unwrap();
    let mut changed: Value = serde_json::from_str(&manifest).unwrap();
    changed["artifact"]["build_id"] = json!("another-build");
    connection
        .execute(
            "UPDATE capability_pins SET manifest_json=?2 WHERE installation_id=?1",
            rusqlite::params![f.installed.id, changed.to_string()],
        )
        .unwrap();
    let (status, body) = f
        .org
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert!(
        status.is_client_error(),
        "accepted changed pin: {status} {body}"
    );
    assert_eq!(f.events().await, events);
    connection
        .execute(
            "UPDATE capability_pins SET manifest_json=?2 WHERE installation_id=?1",
            rusqlite::params![f.installed.id, manifest],
        )
        .unwrap();
    let proof = f.verify(&id).await;
    let terminal_events = f.events().await;
    let saved: String = connection
        .query_row(
            "SELECT proof_json FROM capability_invocation_verifications WHERE job_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    let mut changed: Value = serde_json::from_str(&saved).unwrap();
    changed["proof"]["decision"] = json!("semantic_rejected");
    connection
        .execute(
            "UPDATE capability_invocation_verifications SET proof_json=?2 WHERE job_id=?1",
            rusqlite::params![id, changed.to_string()],
        )
        .unwrap();
    let (status, body) = f
        .org
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert!(
        status.is_client_error(),
        "accepted mutated saved proof: {status} {body}"
    );
    let (status, body) = f
        .org
        .request(
            reqwest::Method::GET,
            &format!("/capability-runs/{id}/verification"),
            "operator",
            None,
        )
        .await;
    assert!(
        status.is_client_error(),
        "returned mutated saved proof: {status} {body}"
    );
    assert_eq!(f.events().await, terminal_events);
    connection
        .execute(
            "UPDATE capability_invocation_verifications SET proof_json=?2 WHERE job_id=?1",
            rusqlite::params![id, saved],
        )
        .unwrap();
    assert_eq!(f.verify(&id).await, proof);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    f.assert_no_commitment().await;
}

#[tokio::test]
async fn failed_binding_insert_rolls_back_invocation_intent_before_provider_work() {
    let f = Fixture::start().await;
    let connection = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    connection.execute_batch("CREATE TRIGGER fail_invocation_binding BEFORE INSERT ON capability_invocation_bindings BEGIN SELECT RAISE(ABORT, 'fixture binding failure'); END;").unwrap();
    let body = json!({
        "client_request_id":"atomic-binding","installation_id":f.installed.id,
        "role_id":f.installed.role,"responsibility_id":f.installed.responsibility,
        "suite_id":suite().suite_id,"input":suite().cases[0].input
    });
    let (status, response) = f.org.post("/capability-runs", body.clone()).await;
    assert!(status.is_server_error(), "{status} {response}");
    let count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM jobs WHERE client_request_id='atomic-binding'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM capability_runs WHERE client_request_id='atomic-binding'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    connection
        .execute_batch("DROP TRIGGER fail_invocation_binding;")
        .unwrap();
    let (status, response) = f.org.post("/capability-runs", body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{response}");
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
}

#[tokio::test]
async fn authority_rejected_proof_still_pins_the_collected_evidence() {
    let mut f = Fixture::start().await;
    let id = f.collected("authority-rejected-evidence", 0).await;
    f.retire_verifier().await;
    let proof = f.verify(&id).await;
    assert_proof(&proof, &id, "authority_rejected", None);
    let events = f.events().await;
    let requests = f.mock.state.captured.lock().unwrap().len();
    let mut connection = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    let record_json: String = connection
        .query_row(
            "SELECT record_json FROM capability_runs WHERE job_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    let job_json: String = connection
        .query_row("SELECT job_json FROM jobs WHERE job_id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    let mut record: genesis_server::capability_runtime::CapabilityRunRecord =
        serde_json::from_str(&record_json).unwrap();
    let mut job: genesis_core::jobs::Job = serde_json::from_str(&job_json).unwrap();
    let evidence = record.evidence.as_mut().unwrap();
    evidence.output["venues"][0]["capacity"] = json!(8888);
    job.result = Some(evidence.output.clone());
    // Consistent transport envelopes must not bypass the immutable proof's
    // evidence binding merely because authority rejection skipped semantics.
    record.validate(&job).unwrap();
    let tx = connection.transaction().unwrap();
    tx.execute(
        "UPDATE capability_runs SET record_json=?2 WHERE job_id=?1",
        rusqlite::params![id, serde_json::to_string(&record).unwrap()],
    )
    .unwrap();
    tx.execute(
        "UPDATE jobs SET job_json=?2 WHERE job_id=?1",
        rusqlite::params![id, serde_json::to_string(&job).unwrap()],
    )
    .unwrap();
    tx.commit().unwrap();
    let (get_status, get_body) = f
        .org
        .request(
            reqwest::Method::GET,
            &format!("/capability-runs/{id}/verification"),
            "operator",
            None,
        )
        .await;
    let (post_status, post_body) = f
        .org
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert!(
        get_status.is_client_error(),
        "GET returned a proof for changed evidence: {get_status} {get_body}"
    );
    assert!(
        post_status.is_client_error(),
        "POST returned a proof for changed evidence: {post_status} {post_body}"
    );
    assert_eq!(f.events().await, events);
    assert_eq!(f.mock.state.captured.lock().unwrap().len(), requests);
    assert_eq!(
        f.org.get(&format!("/jobs/{id}")).await["status"],
        "RESULT_REJECTED"
    );
    let tx = connection.transaction().unwrap();
    tx.execute(
        "UPDATE capability_runs SET record_json=?2 WHERE job_id=?1",
        rusqlite::params![id, record_json],
    )
    .unwrap();
    tx.execute(
        "UPDATE jobs SET job_json=?2 WHERE job_id=?1",
        rusqlite::params![id, job_json],
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(f.verify(&id).await, proof);
}
