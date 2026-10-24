//! Real HTTP boundary tests with trusted local setup and loopback-only clients.
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::{routing::post, Json, Router};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::change::{KeyAllocation, OrgChangeProposal};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_forge::{AnthropicGenerator, ModelConfig};
use genesis_server::app::AppState;
use genesis_server::capability_acquisition_driver::drain_once;
use genesis_server::capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas};
use genesis_server::error::validation_error;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, serve_org, AcquisitionConfig, OrgConfig};
use genesis_verification::{decode_json, OracleSuite};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Notify;

const BASE: &str = "/capability-acquisitions";
const MODEL: &str = "claude-sonnet-4-6";
const EXAMPLE: &str = "ACQUISITION_PUBLIC_HTML_NOT_STATUS_9af0";
const ORACLE: &str = "ACQUISITION_PRIVATE_ORACLE_0e8c";
const SOURCE: &str = "ACQUISITION_GENERATED_SOURCE_NOT_STATUS_c41d";
const REQUIREMENT: &str = "ACQUISITION_BACKEND_REQUIREMENT_2b7f";
const OTHER_SUITE: &str = "another-trusted-acquisition-suite";

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start_org(app: AppState) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
        Self { url, task }
    }

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

fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.scope = ORACLE.into();
    suite
}
fn policy() -> AcquisitionPolicy {
    AcquisitionPolicy {
        model: ModelConfig {
            model: MODEL.into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 3600,
    }
}

struct Org {
    server: Server,
    app: AppState,
    id: String,
    body: Value,
    http: reqwest::Client,
}
impl Org {
    async fn start(id: &str, acquisition: Option<AcquisitionConfig>, authorized: bool) -> Self {
        let mut config = OrgConfig::dev(id, id);
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        let mut other_suite = suite();
        other_suite.suite_id = OTHER_SUITE.into();
        config.oracle_suites = vec![suite(), other_suite];
        config.acquisition = acquisition;
        // Publication refusal must occur before any registry request.
        config.registry_url = Some("http://127.0.0.1:1".into());
        config.registry_token = Some("local-registry-token".into());
        let app = build_org(&config, None).unwrap();
        let body = seed(&app, authorized).await;
        // Do not start the automatic worker or consume developer credentials.
        let server = Server::start(app.clone().router()).await;
        Self {
            server,
            app,
            id: id.into(),
            body,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(4))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }
    async fn offline(id: &str) -> Self {
        Self::start(id, Some(AcquisitionConfig::offline(policy())), true).await
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        principal: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .http
            .request(method, format!("{}/api/v1{path}", self.server.url));
        if let Some(principal) = principal {
            request = request.bearer_auth(format!("{}-{principal}", self.id));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        response(request.send().await.unwrap()).await
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.request(Method::POST, path, Some("operator"), Some(body))
            .await
    }
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.request(Method::GET, path, Some("operator"), None)
            .await
    }
    async fn create(&self) -> Value {
        let (status, body) = self.post(BASE, self.body.clone()).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        redact(&body);
        body
    }
    fn ids(&self) -> Vec<String> {
        self.app.store.acquisition_ids().unwrap()
    }
}

async fn response(response: reqwest::Response) -> (StatusCode, Value) {
    let status = response.status();
    let text = response.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}
fn redact(body: &Value) {
    let text = body.to_string();
    for private in [EXAMPLE, ORACLE, SOURCE, REQUIREMENT] {
        assert!(
            !text.contains(private),
            "private content in response: {text}"
        );
    }
    for forbidden in [
        "extractor_typescript",
        "checkpoint_sha256",
        "previous_extractor_typescript",
    ] {
        assert!(
            !text.contains(forbidden),
            "raw checkpoint field in response: {text}"
        );
    }
}
fn id(status: &Value) -> &str {
    status["id"].as_str().expect("acquisition status ID")
}
fn path(status: &Value) -> String {
    format!("{BASE}/{}", id(status))
}

async fn seed(app: &AppState, authorized: bool) -> Value {
    let scopes: Vec<&str> = if authorized {
        vec!["capability.acquire"]
    } else {
        vec![]
    };
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": "Acquire a missing web capability",
        "roles": [{"key": "owner", "name": "Owner", "charter": "Own acquisition",
            "authority": {"scopes": scopes, "budget_cents": 1000}}],
        "responsibilities": [{"purpose": "Coordinate acquisition", "owner": "owner"}]
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
            engine::record_observation(state, &mission, "test", "Missing parser", None, Utc::now())
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
                REQUIREMENT,
                std::slice::from_ref(&observation),
                "Independent oracle accepts output",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let role = state.roles.keys().next().unwrap().clone();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision": state.revision,
        "trigger_evidence_ids": [observation], "rationale": "Give the requirement one accountable owner",
        "budget_limit_cents": 0,
        "operations": [{"op": "AssignResponsibility", "target": {"kind": "from_requirement",
            "key": "acquisition", "requirement_id": requirement, "purpose": "Acquire the missing parser",
            "trigger": "Missing capability", "termination": "Verified capability available"},
            "owner": {"kind": "existing", "role_id": role}}]
    })).unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    let responsibility = allocation.responsibilities["acquisition"].clone();
    app.mutate(EventMeta::default(), |state| {
        let outcome = engine::commit(
            state,
            &proposal,
            &allocation,
            &Default::default(),
            &engine::local_catalog(state),
            Utc::now(),
        )
        .map_err(validation_error)?;
        Ok((outcome, ()))
    })
    .await
    .unwrap();
    let oracle = suite();
    let urls: Vec<_> = oracle
        .cases
        .iter()
        .map(|case| case.source.url.clone())
        .collect();
    json!({
        "client_request_id": "one-acquisition", "mission_id": mission, "requirement_id": requirement,
        "role_id": role, "responsibility_id": responsibility, "suite_id": oracle.suite_id,
        "candidate": {"capability_id": "http-venue-reader", "capability_version": "1.0.0",
            "actor_name": "genesis-http-acquisition", "source_version": "0.1"},
        "repair": null, "allowed_source_urls": urls,
        "examples": [{"url": urls[0], "html": format!("<html>{EXAMPLE}</html>")}],
        "deadline": (Utc::now() + ChronoDuration::minutes(20)).to_rfc3339()
    })
}

#[tokio::test]
async fn acquisition_management_requires_the_operator_for_every_route() {
    let org = Org::offline("acquisition-auth").await;
    for principal in [None, Some("unknown"), Some("agent"), Some("adapter")] {
        let (status, denied) = org
            .request(Method::POST, BASE, principal, Some(org.body.clone()))
            .await;
        assert_eq!(
            status,
            if matches!(principal, None | Some("unknown")) {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::FORBIDDEN
            },
            "{denied}"
        );
        redact(&denied);
        assert!(org.ids().is_empty());
    }
    let created = org.create().await;
    let resource = path(&created);
    for principal in [None, Some("unknown"), Some("agent"), Some("adapter")] {
        let expected = if matches!(principal, None | Some("unknown")) {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::FORBIDDEN
        };
        for (method, endpoint, body) in [
            (Method::GET, resource.clone(), None),
            (
                Method::POST,
                format!("{resource}/step"),
                Some(json!({"retry_read": false})),
            ),
        ] {
            let (status, denied) = org.request(method, &endpoint, principal, body).await;
            assert_eq!(status, expected, "{denied}");
            redact(&denied);
        }
    }
    let (status, current) = org.get(&resource).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current, created);
    let (status, unavailable) = org.post(&format!("{resource}/step"), json!({})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{unavailable}");
    assert_eq!(org.get(&resource).await.1, created);
}

#[tokio::test]
async fn http_cannot_import_or_override_trusted_generation_or_verification_state() {
    let org = Org::offline("acquisition-import").await;
    for key in [
        "model",
        "model_config",
        "suite",
        "oracle_suite",
        "checkpoint",
        "receipts",
        "plan",
        "requirement",
        "operation_id",
        "org_id",
    ] {
        let mut body = org.body.clone();
        body[key] = json!({"caller_supplied": true});
        let (status, rejected) = org.post(BASE, body).await;
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "accepted {key}: {status} {rejected}"
        );
        redact(&rejected);
        assert!(org.ids().is_empty());
    }
    let mut nested = org.body.clone();
    nested["candidate"]["checkpoint"] = json!({"phase": "verified"});
    let (status, rejected) = org.post(BASE, nested).await;
    assert!(status.is_client_error(), "{status} {rejected}");
    assert!(org.ids().is_empty());
}

#[tokio::test]
async fn idempotency_replays_exact_requests_and_rejects_changed_semantics() {
    let org = Org::offline("acquisition-replay").await;
    let created = org.create().await;
    let (status, repeated) = org.post(BASE, org.body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(repeated, created);
    for (pointer, value) in [
        ("/suite_id", json!(OTHER_SUITE)),
        ("/candidate/capability_version", json!("2.0.0")),
        ("/candidate/actor_name", json!("genesis-http-another-actor")),
        (
            "/examples/0/html",
            json!("<html>changed public input</html>"),
        ),
    ] {
        let mut changed = org.body.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        let (status, conflict) = org.post(BASE, changed).await;
        assert_eq!(status, StatusCode::CONFLICT, "{pointer}: {conflict}");
        redact(&conflict);
        assert_eq!(org.ids(), vec![id(&created).to_string()]);
        assert_eq!(org.get(&path(&created)).await.1, created);
    }
}

#[tokio::test]
async fn concurrent_identical_creates_store_one_intent() {
    let org = Org::offline("acquisition-race").await;
    let (a, b) = tokio::join!(
        org.post(BASE, org.body.clone()),
        org.post(BASE, org.body.clone())
    );
    assert!(matches!(a.0, StatusCode::CREATED | StatusCode::OK), "{a:?}");
    assert!(matches!(b.0, StatusCode::CREATED | StatusCode::OK), "{b:?}");
    assert_ne!(a.0, b.0, "exactly one create and one replay");
    assert_eq!(id(&a.1), id(&b.1));
    assert_eq!(org.ids().len(), 1);
}

#[tokio::test]
async fn invalid_suite_expired_deadline_and_missing_authority_create_nothing() {
    let org = Org::offline("acquisition-invalid").await;
    for (pointer, value) in [
        ("/suite_id", json!("unknown-backend-suite")),
        (
            "/deadline",
            json!((Utc::now() - ChronoDuration::seconds(1)).to_rfc3339()),
        ),
        ("/requirement_id", json!("unknown-requirement")),
    ] {
        let mut body = org.body.clone();
        *body.pointer_mut(pointer).unwrap() = value;
        let (status, denied) = org.post(BASE, body).await;
        assert!(status.is_client_error(), "{pointer}: {status} {denied}");
        redact(&denied);
        assert!(org.ids().is_empty());
    }
    let no_scope = Org::start(
        "acquisition-no-scope",
        Some(AcquisitionConfig::offline(policy())),
        false,
    )
    .await;
    let (status, denied) = no_scope.post(BASE, no_scope.body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert!(no_scope.ids().is_empty());
}

#[tokio::test]
async fn step_rejects_checkpoint_import_and_unknown_fields_without_changing_status() {
    let org = Org::offline("acquisition-step-schema").await;
    let created = org.create().await;
    let endpoint = format!("{}/step", path(&created));
    for body in [
        json!({"retry_read": false, "checkpoint": {"phase": "verified"}}),
        json!({"model": MODEL}),
        json!({"retry_read": "yes"}),
    ] {
        let (status, rejected) = org.post(&endpoint, body).await;
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "{status} {rejected}"
        );
        redact(&rejected);
        assert_eq!(org.get(&path(&created)).await.1, created);
    }
    let response = org
        .http
        .post(format!("{}/api/v1{endpoint}", org.server.url))
        .bearer_auth(format!("{}-operator", org.id))
        .header("content-type", "application/json")
        .body("{\"retry_read\":false,\"retry_read\":true}")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_client_error());
    assert_eq!(org.get(&path(&created)).await.1, created);
}

#[tokio::test]
async fn generated_source_and_oracle_never_appear_in_http_status() {
    let requests: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let model = Server::start(Router::new().route("/v1/messages", post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            captured.lock().unwrap().push(body);
            Json(json!({
                "id": "msg_acquisition_api", "type": "message", "role": "assistant", "model": MODEL,
                "content": [{"type": "text", "text": json!({"extractor_typescript": format!("// {SOURCE}\nexport function extract() {{ throw new Error('not executed on host'); }}")}).to_string()}],
                "stop_reason": "end_turn", "stop_sequence": null, "usage": {"input_tokens": 200, "output_tokens": 80}
            }))
        }
    }))).await;
    let mut config = AcquisitionConfig::offline(policy());
    config.model = Some(Arc::new(
        AnthropicGenerator::with_test_endpoint(
            &format!("{}/v1/messages", model.url),
            "local-model-test-key",
        )
        .unwrap(),
    ));
    let org = Org::start("acquisition-generated", Some(config), true).await;
    let created = org.create().await;
    let (status, progress) = org
        .post(
            &format!("{}/step", path(&created)),
            json!({"retry_read": false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{progress}");
    redact(&progress);
    let (status, current) = org.get(&path(&created)).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    redact(&current);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let request = requests[0].to_string();
    assert!(request.contains(EXAMPLE));
    assert!(request.contains(REQUIREMENT));
    assert!(!request.contains(ORACLE));
    assert_eq!(requests[0]["model"], MODEL);
    assert!(
        current["artifact"].is_null(),
        "generation is not verification"
    );
}

fn manifest(publisher: &str) -> Value {
    json!({
        "capability_id": "http-venue-reader", "version": "1.0.0", "publisher": publisher,
        "source_digest": "a".repeat(64), "intent": "Read public venue fixtures",
        "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
        "required_permissions": ["capability.acquire"],
        "artifact": {"actor_id": "unverified-actor", "build_id": "unverified-build", "build_number": "0.1.1"},
        "role_template": null, "responsibility_template": null, "workflow_procedure": null,
        "acceptance_suite_ref": suite().suite_id,
        "verification": {"passed": true, "cases": [
            {"name": "a", "kind": "success", "passed": true}, {"name": "b", "kind": "success", "passed": true},
            {"name": "c", "kind": "unsupported", "passed": true}],
            "tested_environment": "caller assertion must not establish proof", "oracle_ref": "untrusted", "verified_at": "2026-10-21T00:00:00Z"},
        "state": "APPROVED", "published_at": null
    })
}

#[tokio::test]
async fn independent_organization_cannot_read_step_or_publish_foreign_acquisition_proof() {
    let a = Org::offline("acquisition-org-a").await;
    let b = Org::offline("acquisition-org-b").await;
    let created = a.create().await;
    let resource = path(&created);
    let (status, missing) = b.get(&resource).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, missing) = b.post(&format!("{resource}/step"), json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    for org in [&a, &b] {
        let (status, denied) = org.post("/capabilities/publish", json!({
            "manifest": manifest(&org.id), "verification_ref": format!("acquisition:{}", id(&created))
        })).await;
        assert!(
            status.is_client_error(),
            "unverified or foreign proof accepted: {status} {denied}"
        );
        redact(&denied);
    }
    assert!(b.ids().is_empty());
    assert_eq!(a.get(&resource).await.1, created);
}

#[tokio::test]
async fn acquisition_must_be_enabled_by_backend_configuration() {
    let org = Org::start("acquisition-disabled", None, true).await;
    let (status, denied) = org.post(BASE, org.body.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{denied}");
    redact(&denied);
}

const AUTO_KEY: &str = "local-automatic-acquisition-provider-key";
const PROVIDER_ERROR: &str = "PRIVATE_PROVIDER_AUTOMATION_ERROR_aebf";
#[derive(Clone, Copy)]
enum AutoBehavior {
    Good,
    RepairOnce,
    FailedRead,
    UnknownActor,
    PausedModel,
}
#[derive(Clone)]
struct AutoRequest {
    method: String,
    path: String,
    body: Value,
}
struct AutoWorld {
    behavior: AutoBehavior,
    requests: Vec<AutoRequest>,
    models: usize,
    actors: BTreeMap<String, Value>,
    runs: BTreeMap<String, (String, Value)>,
    failed_read: bool,
}
impl AutoWorld {
    fn build(&self, actor: &str) -> Value {
        json!({"data":{"id":format!("build-{actor}"),"actId":actor,"buildNumber":"0.1.1",
            "status":"SUCCEEDED","actVersion":self.actors[actor]["versions"][0]}})
    }
    fn run(&self, run: &str) -> Value {
        let actor = &self.runs[run].0;
        json!({"data":{"id":run,"actId":actor,"buildId":format!("build-{actor}"),
            "status":"SUCCEEDED","defaultDatasetId":format!("dataset-{run}")}})
    }
    fn reply(&mut self, request: AutoRequest) -> (StatusCode, Value) {
        self.requests.push(request.clone());
        let parts: Vec<_> = request.path.trim_start_matches('/').split('/').collect();
        match (request.method.as_str(), parts.as_slice()) {
            ("POST", ["v1", "messages"]) => {
                let attempt = self.models;
                self.models += 1;
                assert!(
                    attempt < 2,
                    "automatic acquisition exceeded initial generation plus one repair"
                );
                assert_eq!(request.body["model"], MODEL);
                (
                    StatusCode::OK,
                    json!({"id":format!("msg_auto_{attempt}"),"type":"message","role":"assistant","model":MODEL,
                    "content":[{"type":"text","text":json!({"extractor_typescript":format!("// {SOURCE} attempt {attempt}\nexport function extract() {{ throw new Error('canned provider source, never executed locally'); }}\n")}).to_string()}],
                    "stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":120,"output_tokens":80}}),
                )
            }
            ("POST", ["v2", "actors"]) => {
                if matches!(self.behavior, AutoBehavior::UnknownActor) {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error":PROVIDER_ERROR}),
                    );
                }
                assert_eq!(request.body["isPublic"], false);
                let id = format!("actor-auto-{}", self.actors.len());
                let mut definition = request.body.clone();
                definition["id"] = json!(id);
                self.actors.insert(id, request.body);
                (StatusCode::CREATED, json!({"data":definition}))
            }
            ("GET", ["v2", "actors", actor, "versions", version]) => {
                let source = &self.actors[*actor]["versions"][0];
                assert_eq!(source["versionNumber"], *version);
                (StatusCode::OK, json!({"data":source}))
            }
            ("POST", ["v2", "actors", actor, "builds"]) => (StatusCode::CREATED, self.build(actor)),
            ("GET", ["v2", "actor-builds", build]) => {
                if matches!(self.behavior, AutoBehavior::FailedRead) && !self.failed_read {
                    self.failed_read = true;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error":PROVIDER_ERROR}),
                    );
                }
                let actor = build.strip_prefix("build-").unwrap();
                (StatusCode::OK, self.build(actor))
            }
            ("POST", ["v2", "actors", actor, "runs"]) => {
                let oracle = suite();
                let case = oracle
                    .cases
                    .iter()
                    .position(|case| case.input == request.body)
                    .unwrap();
                let receipts: Vec<genesis_verification::ExecutionEvidence> = decode_json(
                    include_bytes!("../../genesis-verification/examples/fixtures/runs.json"),
                )
                .unwrap();
                let mut output = receipts[case].output.clone();
                if matches!(self.behavior, AutoBehavior::RepairOnce)
                    && *actor == "actor-auto-0"
                    && case == 0
                {
                    output["venues"][0]["capacity"] = json!(9999);
                }
                let id = format!("run-auto-{}", self.runs.len());
                self.runs.insert(id.clone(), ((*actor).into(), output));
                (StatusCode::CREATED, self.run(&id))
            }
            ("GET", ["v2", "actor-runs", run]) => (StatusCode::OK, self.run(run)),
            ("GET", ["v2", "datasets", dataset, "items"]) => {
                let run = dataset.strip_prefix("dataset-").unwrap();
                (StatusCode::OK, json!([self.runs[run].1]))
            }
            _ => (
                StatusCode::NOT_FOUND,
                json!({"error":"unexpected automatic acquisition fixture path"}),
            ),
        }
    }
}
#[derive(Clone)]
struct AutoProviderState {
    world: Arc<Mutex<AutoWorld>>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
async fn automatic_provider(
    State(state): State<AutoProviderState>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    if path == "/v1/messages" {
        assert_eq!(request.headers()["x-api-key"], AUTO_KEY);
    } else {
        assert_eq!(
            request.headers()["authorization"],
            format!("Bearer {AUTO_KEY}")
        );
    }
    let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let (pause, (status, body)) = {
        let mut world = state.world.lock().unwrap();
        let pause = path == "/v1/messages" && matches!(world.behavior, AutoBehavior::PausedModel);
        (pause, world.reply(AutoRequest { method, path, body }))
    };
    if pause {
        state.entered.notify_one();
        state.release.notified().await;
    }
    (status, Json(body))
}
struct AutoProviders {
    _server: Server,
    state: AutoProviderState,
    model: Arc<AnthropicGenerator>,
    apify: Arc<ApifyClient>,
}
impl AutoProviders {
    async fn start(behavior: AutoBehavior) -> Self {
        let state = AutoProviderState {
            world: Arc::new(Mutex::new(AutoWorld {
                behavior,
                requests: vec![],
                models: 0,
                actors: BTreeMap::new(),
                runs: BTreeMap::new(),
                failed_read: false,
            })),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        let server = Server::start(
            Router::new()
                .fallback(automatic_provider)
                .with_state(state.clone()),
        )
        .await;
        let model = Arc::new(
            AnthropicGenerator::with_test_endpoint(
                &format!("{}/v1/messages", server.url),
                AUTO_KEY,
            )
            .unwrap(),
        );
        let apify = Arc::new(
            ApifyClient::for_local_testing(
                AUTO_KEY,
                RunLimits {
                    request_timeout: Duration::from_secs(5),
                    deadline: Duration::from_secs(8),
                    poll_interval: Duration::from_millis(1),
                    ..RunLimits::default()
                },
                &server.url,
            )
            .unwrap(),
        );
        Self {
            _server: server,
            state,
            model,
            apify,
        }
    }
    fn count(&self, method: &str, suffix: &str) -> usize {
        self.state
            .world
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r.method == method && r.path.ends_with(suffix))
            .count()
    }
    fn total(&self) -> usize {
        self.state.world.lock().unwrap().requests.len()
    }
    fn assert_private(&self) {
        let world = self.state.world.lock().unwrap();
        for request in &world.requests {
            let encoded = request.body.to_string();
            assert!(!encoded.contains(ORACLE));
            if request.path == "/v1/messages" {
                assert!(encoded.contains(EXAMPLE));
                assert!(!encoded.contains("\"expected\""));
            }
        }
    }
}
struct AutomaticFixture {
    _directory: tempfile::TempDir,
    config: OrgConfig,
    org: Org,
    providers: AutoProviders,
}
impl AutomaticFixture {
    async fn start(
        behavior: AutoBehavior,
        automatic: bool,
        repair: bool,
        model_quota: u64,
    ) -> Self {
        let providers = AutoProviders::start(behavior).await;
        let directory = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev("acquisition-auto", "Automatic acquisition");
        config.db = Some(directory.path().join("organization.sqlite"));
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        config.oracle_suites = vec![suite()];
        let mut acquisition = AcquisitionConfig::offline(policy());
        acquisition.automatic = automatic;
        acquisition.policy.quotas.model_calls = model_quota;
        acquisition.model = Some(providers.model.clone());
        acquisition.apify = Some(providers.apify.clone());
        config.acquisition = Some(acquisition);
        let app = build_org(&config, None).unwrap();
        let mut body = seed(&app, true).await;
        if repair {
            body["repair"] = json!({"capability_id":"http-venue-reader","capability_version":"1.0.1","actor_name":"genesis-http-acquisition-repaired","source_version":"0.1"});
        }
        let server = Server::start_org(app.clone()).await;
        let org = Org {
            server,
            app,
            id: config.org_id.to_string(),
            body,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .no_proxy()
                .build()
                .unwrap(),
        };
        Self {
            _directory: directory,
            config,
            org,
            providers,
        }
    }
    async fn restart(&mut self) {
        self.org.server.task.abort();
        (&mut self.org.server.task).await.unwrap_err();
        let app = build_org(&self.config, None).unwrap();
        self.org.server = Server::start_org(app.clone()).await;
        self.org.app = app;
    }
    async fn automation(&self, resource: &str) -> Value {
        let (status, body) = self.org.get(&format!("{resource}/automation")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        redact(&body);
        assert!(!body.to_string().contains(PROVIDER_ERROR));
        assert!(!body.to_string().contains(AUTO_KEY));
        body
    }
    async fn wait(&self, resource: &str, field: &str, value: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let (status, current) = self.org.get(resource).await;
                assert_eq!(status, StatusCode::OK, "{current}");
                redact(&current);
                if current[field] == value {
                    return current;
                }
                assert!(
                    !["failed", "needs_reconciliation", "fenced"]
                        .contains(&current["phase"].as_str().unwrap_or("")),
                    "unexpected terminal phase: {current}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("automatic acquisition did not reach expected state")
    }
    async fn wait_automation(&self, resource: &str, state: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let current = self.automation(resource).await;
                if current["state"] == state {
                    return current;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("automatic acquisition scheduler did not reach expected state")
    }
    async fn revoke(&self) {
        let before = self.org.app.current_state().unwrap();
        let mut after = before.clone();
        let role = genesis_core::ids::RoleId::new(self.org.body["role_id"].as_str().unwrap());
        after.roles.get_mut(&role).unwrap().state = genesis_core::lifecycle::RoleState::Draining;
        after.revision += 1;
        self.org
            .app
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
}

#[tokio::test]
async fn automatic_acquisition_is_default_off_and_creating_an_intent_has_no_provider_effects() {
    assert!(!AcquisitionConfig::offline(policy()).automatic);
    let f = AutomaticFixture::start(AutoBehavior::Good, false, false, 10).await;
    let created = f.org.create().await;
    let resource = path(&created);
    let before = f.org.app.store.acquisition_counters().unwrap();
    drain_once(&f.org.app).await.unwrap();
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.providers.total(), 0);
    assert_eq!(f.org.get(&resource).await.1, created);
    assert_eq!(f.org.app.store.acquisition_counters().unwrap(), before);
    let automation = f.automation(&resource).await;
    assert_eq!(automation["enabled"], false);
    assert_eq!(automation["state"], "disabled");
}

async fn assert_automatic_verified(repair: bool) {
    let f = AutomaticFixture::start(
        if repair {
            AutoBehavior::RepairOnce
        } else {
            AutoBehavior::Good
        },
        true,
        repair,
        10,
    )
    .await;
    let created = f.org.create().await;
    let resource = path(&created);
    let current = f.wait(&resource, "phase", "verified").await;
    assert_eq!(current["attempt"], if repair { 1 } else { 0 });
    let attempts = if repair { 2 } else { 1 };
    assert_eq!(current["organization_usage"]["model_calls"], attempts);
    assert_eq!(current["organization_usage"]["actor_creates"], attempts);
    assert_eq!(current["organization_usage"]["builds"], attempts);
    assert_eq!(current["organization_usage"]["test_submits"], attempts * 3);
    let verified = f
        .org
        .app
        .acquisition_runtime
        .verified(id(&created))
        .await
        .unwrap();
    assert_eq!(json!(verified.artifact), current["artifact"]);
    assert_eq!(verified.verification.cases.len(), 3);
    assert_eq!(f.providers.count("POST", "/messages"), attempts as usize);
    assert_eq!(f.providers.count("POST", "/actors"), attempts as usize);
    assert_eq!(f.providers.count("POST", "/runs"), (attempts * 3) as usize);
    let automation = f.wait_automation(&resource, "inactive").await;
    assert_eq!(automation["enabled"], true);
    for principal in [None, Some("agent"), Some("adapter")] {
        let (status, body) = f
            .org
            .request(
                Method::GET,
                &format!("{resource}/automation"),
                principal,
                None,
            )
            .await;
        assert_eq!(
            status,
            if principal.is_none() {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::FORBIDDEN
            },
            "{body}"
        );
        redact(&body);
    }
    f.providers.assert_private();
    if repair {
        let world = f.providers.state.world.lock().unwrap();
        let prompts: Vec<_> = world
            .requests
            .iter()
            .filter(|r| r.path == "/v1/messages")
            .collect();
        assert!(prompts[1].body.to_string().contains("value_mismatch"));
    }
}
#[tokio::test]
async fn create_only_automatic_acquisition_reaches_backend_verified_without_steps() {
    assert_automatic_verified(false).await;
}
#[tokio::test]
async fn create_only_automatic_acquisition_performs_one_actual_semantic_repair() {
    assert_automatic_verified(true).await;
}

#[tokio::test]
async fn unknown_actor_post_is_never_replayed_or_refunded_after_automatic_restart() {
    let mut f = AutomaticFixture::start(AutoBehavior::UnknownActor, true, false, 10).await;
    let created = f.org.create().await;
    let resource = path(&created);
    let stopped = f.wait(&resource, "phase", "needs_reconciliation").await;
    assert_eq!(stopped["organization_usage"]["model_calls"], 1);
    assert_eq!(stopped["organization_usage"]["actor_creates"], 1);
    let counts = f.org.app.store.acquisition_counters().unwrap();
    let requests = f.providers.total();
    f.restart().await;
    drain_once(&f.org.app).await.unwrap();
    let current = f.org.get(&resource).await.1;
    assert_eq!(current["phase"], "needs_reconciliation");
    assert_eq!(f.org.app.store.acquisition_counters().unwrap(), counts);
    assert_eq!(f.providers.total(), requests);
    assert_eq!(f.providers.count("POST", "/actors"), 1);
    assert_eq!(f.automation(&resource).await["state"], "inactive");
}

#[tokio::test]
async fn automatic_quota_attention_is_durable_redacted_and_does_not_spend_again() {
    let mut f = AutomaticFixture::start(AutoBehavior::Good, true, false, 1).await;
    let connection = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    // Persist previously consumed quota without making unrelated model calls.
    connection
        .execute(
            "UPDATE acquisition_counters SET model_calls=1 WHERE singleton=1",
            [],
        )
        .unwrap();
    let created = f.org.create().await;
    let resource = path(&created);
    let attention = f.wait_automation(&resource, "attention").await;
    assert!(attention["error_code"].is_string());
    let current = f.org.get(&resource).await.1;
    assert_eq!(current["phase"], "generating");
    assert_eq!(f.providers.total(), 0);
    let counts = f.org.app.store.acquisition_counters().unwrap();
    f.restart().await;
    drain_once(&f.org.app).await.unwrap();
    let restored = f.automation(&resource).await;
    for field in ["state", "error_code", "current_version", "observed_version"] {
        assert_eq!(restored[field], attention[field]);
    }
    assert_eq!(f.org.app.store.acquisition_counters().unwrap(), counts);
    assert_eq!(f.providers.total(), 0);
    assert!(restored["next_attempt_at"].is_null());
    assert!(restored["next_reconcile_at"].is_string());
    // Attention pauses effects, not the grant fence. A due local reconciliation
    // must release active capacity after revocation without another debit.
    f.revoke().await;
    connection.execute(
        "UPDATE acquisition_automation SET next_check_at='2000-01-01T00:00:00.000000000Z' WHERE acquisition_id=?1",
        [id(&created)],
    ).unwrap();
    drain_once(&f.org.app).await.unwrap();
    let fenced = f.org.get(&resource).await.1;
    assert_eq!(fenced["phase"], "fenced");
    let mut released = counts;
    released.active = 0;
    assert_eq!(f.org.app.store.acquisition_counters().unwrap(), released);
    assert_eq!(f.providers.total(), 0);
    assert_eq!(f.automation(&resource).await["state"], "inactive");
}

#[tokio::test]
async fn revocation_during_automatic_model_await_preserves_receipt_but_stops_progress() {
    let f = AutomaticFixture::start(AutoBehavior::PausedModel, true, false, 10).await;
    let created = f.org.create().await;
    let resource = path(&created);
    tokio::time::timeout(
        Duration::from_secs(10),
        f.providers.state.entered.notified(),
    )
    .await
    .unwrap();
    f.revoke().await;
    f.providers.state.release.notify_one();
    let fenced = f.wait(&resource, "phase", "fenced").await;
    assert_eq!(fenced["fenced"], true);
    assert_eq!(fenced["organization_usage"]["model_calls"], 1);
    assert_eq!(fenced["organization_usage"]["actor_creates"], 0);
    assert_eq!(f.providers.count("POST", "/messages"), 1);
    assert_eq!(f.providers.count("POST", "/actors"), 0);
    assert!(f
        .org
        .app
        .acquisition_runtime
        .status(id(&created))
        .unwrap()
        .session()
        .generation()
        .receipt()
        .is_some());
    drain_once(&f.org.app).await.unwrap();
    assert_eq!(f.providers.total(), 1);
}

#[tokio::test]
async fn dropping_server_during_model_post_recovers_without_automatic_replay() {
    let mut f = AutomaticFixture::start(AutoBehavior::PausedModel, true, false, 10).await;
    let created = f.org.create().await;
    let resource = path(&created);
    tokio::time::timeout(
        Duration::from_secs(10),
        f.providers.state.entered.notified(),
    )
    .await
    .unwrap();
    f.org.server.task.abort();
    (&mut f.org.server.task).await.unwrap_err();
    f.providers.state.release.notify_one();
    let app = build_org(&f.config, None).unwrap();
    f.org.server = Server::start_org(app.clone()).await;
    f.org.app = app;
    let stopped = f.wait(&resource, "phase", "needs_reconciliation").await;
    assert_eq!(stopped["organization_usage"]["model_calls"], 1);
    assert_eq!(stopped["organization_usage"]["actor_creates"], 0);
    drain_once(&f.org.app).await.unwrap();
    assert_eq!(f.providers.count("POST", "/messages"), 1);
    assert_eq!(f.providers.count("POST", "/actors"), 0);
}

#[tokio::test]
async fn failed_automatic_get_waits_through_restart_for_explicit_read_retry() {
    let mut f = AutomaticFixture::start(AutoBehavior::FailedRead, true, false, 10).await;
    let created = f.org.create().await;
    let resource = path(&created);
    let attention = f.wait_automation(&resource, "attention").await;
    assert_eq!(attention["error_code"], "retry_read");
    assert!(attention["next_attempt_at"].is_null());
    let before = f.org.get(&resource).await.1;
    assert_eq!(before["forge_failure"]["retryable"], true);
    let requests = f.providers.total();
    let counts = f.org.app.store.acquisition_counters().unwrap();
    f.restart().await;
    drain_once(&f.org.app).await.unwrap();
    assert_eq!(f.providers.total(), requests);
    assert_eq!(f.org.app.store.acquisition_counters().unwrap(), counts);
    assert_eq!(f.automation(&resource).await["error_code"], "retry_read");
    let (status, body) = f
        .org
        .post(&format!("{resource}/step"), json!({"retry_read":false}))
        .await;
    assert!(status.is_client_error(), "{body}");
    assert_eq!(f.providers.total(), requests);
    let (status, advanced) = f
        .org
        .post(&format!("{resource}/step"), json!({"retry_read":true}))
        .await;
    assert_eq!(status, StatusCode::OK, "{advanced}");
    assert!(advanced["version"].as_u64().unwrap() > before["version"].as_u64().unwrap());
    assert!(advanced["forge_failure"].is_null());
    // The changed durable version releases the old attention condition and
    // automation progresses to the first test submission without another step.
    tokio::time::timeout(Duration::from_secs(20), async {
        while f.providers.count("POST", "/runs") == 0 {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("automatic driver did not resume after explicit read retry");
    f.org.server.task.abort();
    (&mut f.org.server.task).await.unwrap_err();
    assert_eq!(f.providers.count("POST", "/messages"), 1);
    assert_eq!(f.providers.count("POST", "/actors"), 1);
}

#[tokio::test]
async fn stale_automatic_scan_cannot_recover_or_advance_an_inflight_manual_step() {
    let f = AutomaticFixture::start(AutoBehavior::PausedModel, false, false, 10).await;
    let created = f.org.create().await;
    let resource = path(&created);
    let http = f.org.http.clone();
    let endpoint = format!("{}/api/v1{resource}/step", f.org.server.url);
    let token = format!("{}-operator", f.org.id);
    let manual = tokio::spawn(async move {
        response(
            http.post(endpoint)
                .bearer_auth(token)
                .json(&json!({"retry_read":false}))
                .send()
                .await
                .unwrap(),
        )
        .await
    });
    tokio::time::timeout(
        Duration::from_secs(10),
        f.providers.state.entered.notified(),
    )
    .await
    .unwrap();
    let during = f.org.get(&resource).await.1;
    let mut automatic = f.org.app.clone();
    automatic.automatic_acquisition = true;
    let drain = tokio::spawn(async move { drain_once(&automatic).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if f.automation(&resource).await["observed_version"] == during["version"] {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("automatic scan did not record its in-flight observed version");
    f.providers.state.release.notify_one();
    let (status, completed) = manual.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{completed}");
    let summary = drain.await.unwrap().unwrap();
    assert_eq!(summary.stale, 1);
    let current = f.org.get(&resource).await.1;
    assert_eq!(current, completed);
    assert!(current["version"].as_u64().unwrap() > during["version"].as_u64().unwrap());
    assert_ne!(current["phase"], "needs_reconciliation");
    assert_eq!(f.providers.count("POST", "/messages"), 1);
    assert_eq!(f.providers.count("POST", "/actors"), 0);
}
