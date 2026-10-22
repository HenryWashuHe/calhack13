//! HTTP evidence-gate integration with independent organizations and a real
//! Apify adapter pointed at a loopback simulator. No cloud execution is claimed.
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::{Json, Router};
use genesis_apify::{ApifyClient, RunLimits, SourceFile};
use genesis_forge::{CandidateSpec, ForgeSession, SourcePackage};
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::registry::{self, RegistryState};
use genesis_server::{build_org, parse_publishers, serve_org, OrgConfig};
use genesis_verification::{decode_json, ArtifactPin, ExecutionEvidence, OracleSuite};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// A dedicated runtime models process shutdown: all background workers are
    /// gone before the same SQLite database is opened by the replacement server.
    async fn start_org(app: genesis_server::app::AppState) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let listener = listener.into_std().unwrap();
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                tokio::select! {
                    result = serve_org(app, listener) => result.unwrap(),
                    _ = stopped => {}
                }
            });
        });
        Self {
            url,
            task,
            process_shutdown: Some(shutdown),
        }
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.process_shutdown.take() {
            let _ = shutdown.send(());
            (&mut self.task).await.unwrap();
        } else {
            self.task.abort();
            let _ = (&mut self.task).await;
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
                return (StatusCode::OK, Json(json!([state.outputs[*case]])));
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
    server: Server,
    id: String,
    http: reqwest::Client,
}

impl Org {
    async fn start(id: &str, registry: &Server, mock: &ApifyMock, publisher_proof: bool) -> Self {
        let mut config = OrgConfig::dev(id, id);
        config.registry_url = Some(registry.url.clone());
        config.registry_token = Some(format!("{id}-registry"));
        config.oracle_suites = vec![suite()];
        config.forge_checkpoints = if publisher_proof {
            vec![checkpoint()]
        } else {
            vec![]
        };
        let app = build_org(&config, Some(mock.providers())).unwrap();
        // Serve the real router without the automatic worker; each test drives
        // durable provider operations explicitly through the public step route.
        let server = Server::start(app.router()).await;
        Self {
            server,
            id: id.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }

    async fn start_with_worker(config: &OrgConfig, mock: &ApifyMock) -> Self {
        let app = build_org(config, Some(mock.providers())).unwrap();
        Self {
            server: Server::start_org(app).await,
            id: config.org_id.as_str().to_owned(),
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

    async fn wait_for_worker(&self, ids: &[String]) {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let mut complete = true;
                for id in ids {
                    let record = self.get(&format!("/capability-runs/{id}")).await;
                    let phase = record["phase"].as_str().unwrap();
                    assert!(
                        !["failed", "rejected", "needs_reconciliation"].contains(&phase),
                        "automatic worker failed: {record}"
                    );
                    complete &= phase == "collected";
                }
                if complete {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("automatic worker did not collect all cases within 8 seconds");
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

#[tokio::test]
async fn elastic_bound_role_can_use_backend_apify_checks_and_invocation() {
    let registry = registry().await;
    let mock = ApifyMock::start().await;
    let mut config = OrgConfig::dev("org_a", "org_a");
    config.registry_url = Some(registry.url.clone());
    config.registry_token = Some("org_a-registry".into());
    config.oracle_suites = vec![suite()];
    config.forge_checkpoints = vec![checkpoint()];
    let app = build_org(&config, Some(mock.providers())).unwrap();
    let org = Org {
        server: Server::start(app.clone().router()).await,
        id: "org_a".into(),
        http: reqwest::Client::new(),
    };
    org.publish().await;
    let installed = org.seed_and_install().await;
    {
        // Trusted fixture state models an active Elastic specialist; this test
        // makes no native Elastic provisioning or converse claim. Actual Actor
        // requests use the real Apify adapter against the loopback fixture.
        let _guard = app.kernel.lock().await;
        let mut state = app.current_state().unwrap();
        let revision = state.revision;
        let role = state
            .roles
            .get_mut(&genesis_core::ids::RoleId::new(&installed.role))
            .unwrap();
        role.binding = Some(genesis_core::model::AgentBinding {
            provider: genesis_core::model::Provider::Elastic,
            provider_agent_id: "genesis-review-agent".into(),
            config_digest: "c".repeat(64),
            conversation_ref: None,
        });
        state.revision += 1;
        app.store
            .commit_state(
                revision,
                &state,
                vec![],
                &Default::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
    let (status, rejected) = org
        .post(
            "/jobs",
            json!({"client_request_id":"generic-apify-override","role_id":installed.role,
                "responsibility_id":installed.responsibility,"task":"Read venue data",
                "input":{},"provider":"apify"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
    assert_eq!(rejected["error"], "provider_mismatch");
    assert_eq!(mock.submissions(), 0);

    let checks = org.checks(&installed).await;
    org.collect(&checks).await;
    org.accept(&installed, &checks).await;
    org.attach(&installed).await;
    let (status, job) = org
        .post(
            "/capability-runs",
            json!({"client_request_id":"elastic-role-actor-run","installation_id":installed.id,
                "role_id":installed.role,"responsibility_id":installed.responsibility,
                "suite_id":suite().suite_id,"input":suite().cases[0].input}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["provider"], "apify");
    org.collect(&[job["job_id"].as_str().unwrap().to_owned()])
        .await;
    assert_eq!(mock.submissions(), suite().cases.len() + 1);
    assert!(org.get("/ledger").await.as_array().unwrap().is_empty());
    mock.assert_no_oracle_or_synthesis_requests();
}

#[tokio::test]
async fn production_publication_requires_known_backend_proof_and_exact_pin() {
    let registry = registry().await;
    let mock = ApifyMock::start().await;
    let a = Org::start("org_a", &registry, &mock, true).await;
    for body in [
        manifest("org_a"),
        json!({"manifest":manifest("org_a"),"verification_ref":"invented-proof"}),
    ] {
        let (status, response) = a.post("/capabilities/publish", body).await;
        assert!(!status.is_success(), "{response}");
    }
    let mut different = manifest("org_a");
    different["artifact"]["build_id"] = json!("foreign-build");
    let (status, response) = a
        .post(
            "/capabilities/publish",
            json!({"manifest":different,"verification_ref":PROOF}),
        )
        .await;
    assert!(!status.is_success(), "{response}");
    let published = a.publish().await;
    assert_eq!(published["verification"]["oracle_ref"], suite().suite_id);
    assert_ne!(
        published["verification"]["tested_environment"],
        "caller assertion"
    );
    assert_eq!(
        published["source_digest"],
        checkpoint().artifact().unwrap().source_sha256
    );
    assert_eq!(mock.submissions(), 0);
}

#[tokio::test]
async fn independent_local_checks_require_local_receipts_and_never_regenerate_source() {
    let registry = registry().await;
    let mock = ApifyMock::start().await;
    let a = Org::start("org_a", &registry, &mock, true).await;
    let b = Org::start("org_b", &registry, &mock, false).await;
    a.publish().await;
    let ai = a.seed_and_install().await;
    let bi = b.seed_and_install().await;
    let accept_path = format!("/capability-installations/{}/acceptance", bi.id);
    let (status, forged) = b
        .post(
            &accept_path,
            json!({"passed":true,"report_ref":"caller-forgery"}),
        )
        .await;
    assert!(!status.is_success(), "{forged}");
    let a_ids = a.checks(&ai).await;
    a.collect(&a_ids).await;
    a.accept(&ai, &a_ids).await;
    let b_ids = b.checks(&bi).await;
    let (status, foreign) = b
        .post(
            &accept_path,
            json!({"suite_id":suite().suite_id,"job_ids":a_ids}),
        )
        .await;
    assert!(!status.is_success(), "{foreign}");
    let (status, premature) = b
        .post(
            &accept_path,
            json!({"suite_id":suite().suite_id,"job_ids":b_ids}),
        )
        .await;
    assert!(!status.is_success(), "{premature}");
    b.collect(&b_ids).await;
    let accepted = b.accept(&bi, &b_ids).await;
    assert_eq!(
        accepted["installation"]["source_digest"],
        checkpoint().artifact().unwrap().source_sha256
    );
    assert_eq!(mock.submissions(), 6);
    assert!(b.get("/ledger").await.as_array().unwrap().is_empty());
    mock.assert_no_oracle_or_synthesis_requests();
}

#[tokio::test]
async fn normal_run_rejects_changed_input_replay_and_generic_passed_results() {
    let registry = registry().await;
    let mock = ApifyMock::start().await;
    let a = Org::start("org_a", &registry, &mock, true).await;
    a.publish().await;
    let installed = a.seed_and_install().await;
    let ids = a.checks(&installed).await;
    a.collect(&ids).await;
    a.accept(&installed, &ids).await;
    a.attach(&installed).await;
    let mut body = json!({"client_request_id":"normal-run-1","installation_id":installed.id,
        "role_id":installed.role,"responsibility_id":installed.responsibility,
        "suite_id":suite().suite_id,"input":suite().cases[0].input});
    let (status, job) = a.post("/capability-runs", body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["job_id"].as_str().unwrap().to_owned();
    let (status, duplicate) = a.post("/capability-runs", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{duplicate}");
    assert_eq!(duplicate["job_id"], id);
    body["input"] = suite().cases[1].input.clone();
    let (status, changed) = a.post("/capability-runs", body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{changed}");
    body["client_request_id"] = json!("unconfigured-input");
    body["input"]["attendees"] = json!(999);
    let (status, unconfigured) = a.post("/capability-runs", body).await;
    assert!(!status.is_success(), "{unconfigured}");
    let (status, forged) = a
        .request(
            reqwest::Method::POST,
            &format!("/jobs/{id}/results"),
            "adapter",
            Some(json!({
                "attempt_id":job["attempt_id"],"epoch":job["epoch"],"outcome":"succeeded",
                "provider_status":"SUCCEEDED","output":{},"domain_verified":true,
                "operation_id":"forged-booking","completes_responsibility":false,
            })),
        )
        .await;
    assert!(!status.is_success(), "{forged}");
    assert_ne!(a.get(&format!("/jobs/{id}")).await["status"], "SUCCEEDED");
    a.collect(&[id]).await;
    assert_eq!(mock.submissions(), 4);
    assert!(a.get("/ledger").await.as_array().unwrap().is_empty());
    mock.assert_no_oracle_or_synthesis_requests();
}

#[tokio::test]
async fn automatic_worker_and_accepted_installation_survive_sqlite_restart() {
    let registry = registry().await;
    let mock = ApifyMock::start().await;
    let database = tempfile::tempdir().unwrap();
    let mut config = OrgConfig::dev("org_a", "Persistent organization");
    config.db = Some(database.path().join("organization.sqlite"));
    config.registry_url = Some(registry.url.clone());
    config.registry_token = Some("org_a-registry".into());
    config.oracle_suites = vec![suite()];
    config.forge_checkpoints = vec![checkpoint()];

    let a = Org::start_with_worker(&config, &mock).await;
    a.publish().await;
    let installed = a.seed_and_install().await;
    let case_jobs = a.checks(&installed).await;
    // No /step calls: serve_org's real background worker submits and collects.
    a.wait_for_worker(&case_jobs).await;
    let accepted = a.accept(&installed, &case_jobs).await;
    a.attach(&installed).await;
    let accepted_reference = accepted["installation"]["acceptance_report_ref"].clone();
    assert!(accepted_reference
        .as_str()
        .unwrap()
        .starts_with("verification:"));
    assert_eq!(mock.submissions(), 3);
    a.server.stop().await;

    // Rebuild every in-memory object from the same persisted database and
    // immutable backend catalog. No synthesis or acceptance is repeated.
    let restarted = Org::start_with_worker(&config, &mock).await;
    let state = restarted.get("/organization").await;
    let restored = &state["installations"][&installed.id];
    assert_eq!(restored["status"], "ACCEPTED");
    assert_eq!(restored["acceptance_report_ref"], accepted_reference);
    assert_eq!(
        restored["source_digest"],
        checkpoint().artifact().unwrap().source_sha256
    );
    for id in &case_jobs {
        let record = restarted.get(&format!("/capability-runs/{id}")).await;
        assert_eq!(record["phase"], "collected");
        assert_eq!(
            record["request"]["artifact"],
            json!(checkpoint().artifact().unwrap())
        );
    }
    assert_eq!(mock.submissions(), 3);

    let (status, job) = restarted
        .post(
            "/capability-runs",
            json!({
                "client_request_id":"after-process-restart","installation_id":installed.id,
                "role_id":installed.role,"responsibility_id":installed.responsibility,
                "suite_id":suite().suite_id,"input":suite().cases[0].input,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let job_id = job["job_id"].as_str().unwrap().to_owned();
    restarted
        .wait_for_worker(std::slice::from_ref(&job_id))
        .await;
    assert_eq!(
        restarted.get(&format!("/jobs/{job_id}")).await["status"],
        "VERIFYING"
    );
    assert_eq!(mock.submissions(), 4);
    assert!(restarted
        .get("/ledger")
        .await
        .as_array()
        .unwrap()
        .is_empty());
    mock.assert_no_oracle_or_synthesis_requests();
    restarted.server.stop().await;
}
