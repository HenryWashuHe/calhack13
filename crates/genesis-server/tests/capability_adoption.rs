//! Independent adoption through real organization/registry HTTP services and the
//! real Apify client against canned loopback replies. Publisher summaries are
//! advisory fixtures computed by the verifier; no Forge checkpoint is imported,
//! no source is executed, and no cloud or cross-account access is claimed.
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::{Json, Router};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::change::{KeyAllocation, OrgChangeProposal};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::ids::{InstallationId, RoleId};
use genesis_core::lifecycle::{InstallationStatus, RoleState};
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::app::AppState;
use genesis_server::capability_evidence::EvidencePolicy;
use genesis_server::capability_runtime::{CapabilityRunPhase, ExecutionPurpose};
use genesis_server::error::validation_error;
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, parse_publishers, serve_org, OrgConfig};
use genesis_verification::{decode_json, ArtifactPin, ExecutionEvidence, OracleSuite};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

const BASE: &str = "/capability-adoptions";
const ORACLE: &str = "PRIVATE_ADOPTION_ORACLE_891ce";
const PROVIDER_ERROR: &str = "PRIVATE_ADOPTION_PROVIDER_ERROR_d2a54";
const KEY: &str = "PRIVATE_ADOPTION_PROVIDER_KEY_bf814";
const REQUIREMENT: &str = "Read venue capacity price availability for the independent organization";
const CAPABILITY: &str = "shared-venue-reader";

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
    async fn org(app: AppState) -> Self {
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

fn suite(independent: bool) -> OracleSuite {
    let mut s: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    if independent {
        s.suite_id = "independent-adoption-suite-b".into();
        s.scope = ORACLE.into();
        for (index, case) in s.cases.iter_mut().enumerate() {
            case.case_id = format!("b-case-{index}");
            case.input["attendees"] = json!(case.input["attendees"].as_u64().unwrap() + 7);
        }
    }
    s
}
fn receipts() -> Vec<ExecutionEvidence> {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap()
}
fn pin() -> ArtifactPin {
    ArtifactPin {
        capability_id: CAPABILITY.into(),
        capability_version: "1.0.0".into(),
        source_sha256: "a".repeat(64),
        actor_id: "fixture-shared-actor".into(),
        build_id: "fixture-shared-build".into(),
        build_number: "1.0.1".into(),
    }
}
fn manifest(capability: &str, publisher: &str, permission: &str) -> Value {
    let artifact = pin();
    let mut evidence = receipts();
    for item in &mut evidence {
        item.artifact = artifact.clone();
    }
    let policy = EvidencePolicy::new(vec![suite(false)], vec![]).unwrap();
    let summary = policy
        .accept_suite(&suite(false).suite_id, &artifact, &evidence)
        .unwrap();
    json!({"capability_id":capability,"version":"1.0.0","publisher":publisher,
        "source_digest":artifact.source_sha256,"intent":REQUIREMENT,
        "applicability_conditions":["configured synthetic venue fixture inputs"],
        "known_limitations":["canned upstream fixture; fresh B evidence required"],
        "input_schema":{"type":"object"},"output_schema":{"type":"object"},
        "required_permissions":[permission],"artifact":{"actor_id":artifact.actor_id,
            "build_id":artifact.build_id,"build_number":artifact.build_number},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,
        "local_configuration_schema":{},"acceptance_suite_ref":suite(false).suite_id,
        "verification":summary.kernel_report,"provenance":["loopback-upstream-summary-only"],
        "state":"APPROVED","published_at":null})
}
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    body: Value,
}
async fn observe(
    State(log): State<Arc<Mutex<Vec<Seen>>>>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    log.lock().unwrap().push(Seen {
        method: parts.method.to_string(),
        path: parts.uri.path().into(),
        body: if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    });
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}
#[derive(Clone, Copy)]
enum Behavior {
    Good,
    Wrong,
    Ambiguous,
}
struct World {
    behavior: Behavior,
    seen: Vec<Seen>,
    runs: BTreeMap<usize, usize>,
}
async fn apify(
    State(world): State<Arc<Mutex<World>>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    assert_eq!(request.headers()["authorization"], format!("Bearer {KEY}"));
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = if bytes.is_empty() {
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
    let pin = pin();
    if method == "GET" && path == format!("/v2/actor-builds/{}", pin.build_id) {
        return (
            StatusCode::OK,
            Json(json!({"data":{"id":pin.build_id,"actId":pin.actor_id,
            "buildNumber":pin.build_number,"status":"SUCCEEDED"}})),
        );
    }
    if method == "POST" && path == format!("/v2/actors/{}/runs", pin.actor_id) {
        let index = suite(true)
            .cases
            .iter()
            .position(|case| case.input == body)
            .expect("only B's own configured input may execute");
        let run = world.runs.len();
        world.runs.insert(run, index); // Provider effect happens before any ambiguous reply.
        if matches!(world.behavior, Behavior::Ambiguous) {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":PROVIDER_ERROR})),
            );
        }
        return (StatusCode::CREATED, Json(run_reply(run)));
    }
    if method == "GET" {
        if let Some(run) = path
            .strip_prefix("/v2/actor-runs/b-run-")
            .and_then(|s| s.parse::<usize>().ok())
        {
            if world.runs.contains_key(&run) {
                return (StatusCode::OK, Json(run_reply(run)));
            }
        }
        if let Some(run) = path
            .strip_prefix("/v2/datasets/b-dataset-")
            .and_then(|s| s.strip_suffix("/items"))
            .and_then(|s| s.parse::<usize>().ok())
        {
            if let Some(case) = world.runs.get(&run) {
                let mut output = receipts()[*case].output.clone();
                if matches!(world.behavior, Behavior::Wrong) && *case == 0 {
                    output["venues"][0]["capacity"] = json!(999);
                }
                return (StatusCode::OK, Json(json!([output])));
            }
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error":"unexpected loopback request"})),
    )
}
fn run_reply(run: usize) -> Value {
    json!({"data":{"id":format!("b-run-{run}"),"actId":pin().actor_id,"buildId":pin().build_id,
        "status":"SUCCEEDED","defaultDatasetId":format!("b-dataset-{run}")}})
}

struct Fixture {
    _dir: tempfile::TempDir,
    _registry: Server,
    provider: Server,
    registry_log: Arc<Mutex<Vec<Seen>>>,
    world: Arc<Mutex<World>>,
    server: Server,
    app: AppState,
    config: OrgConfig,
    http: reqwest::Client,
    body: Value,
}
impl Fixture {
    async fn new(behavior: Behavior, automatic: bool) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .unwrap();
        let registry_log = Arc::new(Mutex::new(Vec::new()));
        let registry = Server::start(
            registry::router(
                RegistryState::open_in_memory(
                    parse_publishers("org-a:a-registry,org-b:b-registry"),
                    vec![],
                )
                .unwrap(),
            )
            .layer(middleware::from_fn_with_state(
                registry_log.clone(),
                observe,
            )),
        )
        .await;
        for (capability, publisher, permission, token) in [
            (CAPABILITY, "org-a", "capability.venue_lookup", "a-registry"),
            (
                "self-venue-reader",
                "org-b",
                "capability.venue_lookup",
                "b-registry",
            ),
            (
                "forbidden-venue-reader",
                "org-a",
                "capability.private",
                "a-registry",
            ),
        ] {
            let response = http
                .post(format!("{}/registry/v1/manifests", registry.url))
                .bearer_auth(token)
                .json(&manifest(capability, publisher, permission))
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::CREATED, "{body}");
        }
        registry_log.lock().unwrap().clear();
        let world = Arc::new(Mutex::new(World {
            behavior,
            seen: vec![],
            runs: BTreeMap::new(),
        }));
        let provider = Server::start(Router::new().fallback(apify).with_state(world.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev("org-b", "Independent adopter");
        config.db = Some(dir.path().join("b.sqlite"));
        config.registry_url = Some(registry.url.clone());
        config.registry_token = Some("b-registry".into());
        config.oracle_suites = vec![suite(true)];
        let app = build_org(&config, Some(providers(&provider.url))).unwrap();
        let body = seed(&app).await;
        let server = if automatic {
            Server::org(app.clone()).await
        } else {
            Server::start(app.clone().router()).await
        };
        Self {
            _dir: dir,
            _registry: registry,
            provider,
            registry_log,
            world,
            server,
            app,
            config,
            http,
            body,
        }
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
            .bearer_auth(format!("org-b-{principal}"));
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
        if path.starts_with(BASE) {
            redact(&body);
        }
        body
    }
    async fn create(&self) -> Value {
        let (status, body) = self.post(BASE, self.body.clone()).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        redact(&body);
        body
    }
    async fn step(&self, id: &str) -> Value {
        let (status, body) = self.post(&format!("{BASE}/{id}/step"), json!({})).await;
        assert!(status.is_success(), "{status}: {body}");
        redact(&body);
        body
    }
    async fn wait(&self, id: &str, phase: &str) -> Value {
        for _ in 0..240 {
            let body = self.get(&format!("{BASE}/{id}")).await;
            if body["phase"] == phase {
                return body;
            }
            assert!(
                !["attention", "fenced", "rejected"]
                    .contains(&body["phase"].as_str().unwrap_or("")),
                "unexpected terminal state: {body}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "adoption did not reach {phase}: {}",
            self.get(&format!("{BASE}/{id}")).await
        );
    }
    async fn restart(&mut self, automatic: bool) {
        self.server.stop().await;
        self.app = build_org(&self.config, Some(providers(&self.provider.url))).unwrap();
        self.server = if automatic {
            Server::org(self.app.clone()).await
        } else {
            Server::start(self.app.clone().router()).await
        };
    }
    fn posts(&self) -> usize {
        self.world
            .lock()
            .unwrap()
            .seen
            .iter()
            .filter(|seen| seen.method == "POST")
            .count()
    }
    fn assert_private_inputs(&self) {
        for request in &self.world.lock().unwrap().seen {
            assert!(!request.body.to_string().contains(ORACLE));
            assert!(!request.body.to_string().contains("expected"));
            if request.method == "POST" {
                assert_eq!(request.path, format!("/v2/actors/{}/runs", pin().actor_id));
            }
        }
    }
    async fn handoff(&self) -> String {
        let old = RoleId::new(self.body["role_id"].as_str().unwrap());
        let observation = self
            .app
            .mutate(EventMeta::default(), |state| {
                engine::record_observation(
                    state,
                    &self.body["mission_id"].as_str().unwrap().into(),
                    "operator",
                    "Independent acceptance complete; continue with successor",
                    None,
                    Utc::now(),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap()
            .value;
        let state = self.app.current_state().unwrap();
        let handoffs: Vec<_> = state.open_responsibilities_of(&old).iter().map(|r|
            json!({"responsibility_id":r.responsibility_id,"to":{"kind":"new","key":"successor"}})).collect();
        let (status, change) = self.post("/org-changes",json!({"client_request_id":"handoff-adoption",
            "proposal":{"expected_org_revision":state.revision,"trigger_evidence_ids":[observation],
                "rationale":"Continue inherited capability after temporary verifier retirement",
                "operations":[{"op":"CreateRole","key":"successor","name":"Successor",
                    "charter":"Continue venue lookup","authority":{"scopes":["capability.venue_lookup"],"budget_cents":10000},
                    "provider":"internal","justification":"lifecycle"},
                    {"op":"RetireRole","role_id":old,"handoffs":handoffs},
                    {"op":"AttachCapability","role":{"kind":"new","key":"successor"},
                        "capability":{"capability_id":CAPABILITY,"version":"1.0.0"}}]}})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{change}");
        for _ in 0..100 {
            let saved = self
                .get(&format!(
                    "/org-changes/{}",
                    change["change_id"].as_str().unwrap()
                ))
                .await;
            if saved["status"] == "COMMITTED" {
                let state = self.app.current_state().unwrap();
                assert_eq!(state.roles[&old].state, RoleState::Retired);
                return state
                    .roles
                    .values()
                    .find(|r| r.name == "Successor")
                    .unwrap()
                    .role_id
                    .to_string();
            }
            assert!(
                !["FAILED", "REJECTED"].contains(&saved["status"].as_str().unwrap()),
                "{saved}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("handoff did not finish");
    }
    async fn expire(&self) {
        let role = RoleId::new(self.body["role_id"].as_str().unwrap());
        self.app
            .mutate(EventMeta::default(), |state| {
                let mut next = state.clone();
                next.roles.get_mut(&role).unwrap().state = RoleState::Retired;
                next.revision += 1;
                Ok((
                    engine::Outcome {
                        state: next,
                        effects: vec![],
                    },
                    (),
                ))
            })
            .await
            .unwrap();
    }
}
fn providers(url: &str) -> ProviderSet {
    let client = ApifyClient::for_local_testing(
        KEY,
        RunLimits {
            request_timeout: Duration::from_secs(2),
            deadline: Duration::from_secs(5),
            poll_interval: Duration::from_millis(1),
            ..RunLimits::default()
        },
        url,
    )
    .unwrap();
    ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .with_executor(Arc::new(ApifyCapabilityExecutor::new(client)))
}
fn redact(body: &Value) {
    let text = body.to_string();
    for private in [
        ORACLE,
        KEY,
        PROVIDER_ERROR,
        REQUIREMENT,
        "sourceFiles",
        "oracle_suite",
        "expected",
    ] {
        assert!(
            !text.contains(private),
            "private data in adoption status: {text}"
        );
    }
}
fn id(body: &Value) -> &str {
    body["id"].as_str().expect("adoption id")
}
async fn seed(app: &AppState) -> Value {
    let spec: MissionBootstrap = serde_json::from_value(json!({"goal":"Independently inherit venue lookup",
        "roles":[{"key":"verifier","name":"Verifier","charter":"Independently validate inherited work",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Coordinate local acceptance","owner":"verifier"}]})).unwrap();
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
                "operator",
                "Missing venue capability",
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
                REQUIREMENT,
                std::slice::from_ref(&observation),
                "Own oracle independently accepts venue output",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let role = state.roles.keys().next().unwrap().clone();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({"expected_org_revision":state.revision,
        "trigger_evidence_ids":[observation],"rationale":"Assign independent requirement","budget_limit_cents":0,
        "operations":[{"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"adoption",
            "requirement_id":requirement,"purpose":"Adopt venue capability","trigger":"Missing capability",
            "termination":"Locally accepted capability"},"owner":{"kind":"existing","role_id":role}}]})).unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    let responsibility = allocation.responsibilities["adoption"].clone();
    app.mutate(EventMeta::default(), |state| {
        Ok((
            engine::commit(
                state,
                &proposal,
                &allocation,
                &Default::default(),
                &engine::local_catalog(state),
                Utc::now(),
            )
            .map_err(validation_error)?,
            (),
        ))
    })
    .await
    .unwrap();
    json!({"client_request_id":"independent-adoption","mission_id":mission,"requirement_id":requirement,
        "role_id":role,"responsibility_id":responsibility,"suite_id":suite(true).suite_id,
        "provider_access_acknowledged":true,"deadline":(Utc::now()+ChronoDuration::minutes(15)).to_rfc3339()})
}

#[tokio::test]
async fn create_only_discovers_from_own_requirement_and_accepts_fresh_independent_evidence() {
    let mut f = Fixture::new(Behavior::Good, true).await;
    assert!(f.config.acquisition.is_none());
    assert!(f.config.forge_checkpoints.is_empty());
    // Discard the first HTTP response: replay must recover the original durable intent.
    let response = f
        .http
        .post(format!("{}/api/v1{BASE}", f.server.url))
        .bearer_auth("org-b-operator")
        .json(&f.body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    drop(response);
    let (status, replay) = f.post(BASE, f.body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    let accepted = f.wait(id(&replay), "accepted").await;
    let state = f.app.current_state().unwrap();
    assert_eq!(state.installations.len(), 1);
    let installation = state.installations.values().next().unwrap();
    assert_eq!(installation.status, InstallationStatus::Accepted);
    assert_eq!(installation.capability.capability_id.as_str(), CAPABILITY);
    assert_eq!(installation.capability.version, "1.0.0");
    let pinned = f
        .app
        .store
        .capability_manifest(&installation.installation_id)
        .unwrap();
    assert_eq!(pinned.publisher.as_str(), "org-a");
    let proof = f
        .app
        .store
        .capability_acceptance(&installation.installation_id)
        .unwrap()
        .unwrap();
    assert_eq!(proof.suite_id, suite(true).suite_id);
    assert_eq!(proof.job_ids.len(), 3);
    assert_ne!(proof.report_ref, pinned.verification.oracle_ref);
    for job in &proof.job_ids {
        let run = f.app.store.get_capability_run(job).unwrap();
        assert_eq!(run.phase, CapabilityRunPhase::Collected);
        assert_eq!(run.request.artifact, pin());
        assert!(suite(true)
            .cases
            .iter()
            .any(|case| case.input == run.request.input));
        assert!(!suite(false)
            .cases
            .iter()
            .any(|case| case.input == run.request.input));
        assert!(matches!(
            run.purpose,
            ExecutionPurpose::AdoptionAcceptance { .. }
        ));
        assert!(run.evidence.unwrap().run_id.starts_with("b-run-"));
    }
    assert_eq!(f.posts(), 3);
    assert_eq!(f.app.store.list_jobs().unwrap().len(), 3);
    assert!(f.app.store.commitments().unwrap().is_empty());
    let requests = f.registry_log.lock().unwrap().clone();
    let searches: Vec<_> = requests
        .iter()
        .filter(|r| r.path == "/registry/v1/search")
        .collect();
    assert_eq!(searches.len(), 1);
    assert_eq!(searches[0].body["query"], REQUIREMENT);
    assert_eq!(searches[0].body["limit"], 5);
    assert!(searches[0].body["available_scopes"]
        .as_array()
        .unwrap()
        .contains(&json!("capability.venue_lookup")));
    assert!(requests
        .iter()
        .any(|r| r.path == format!("/registry/v1/capabilities/{CAPABILITY}/versions/1.0.0")));
    f.assert_private_inputs();
    let successor = f.handoff().await;
    f.restart(true).await;
    let reopened = f.get(&format!("{BASE}/{}", id(&accepted))).await;
    assert_eq!(reopened, accepted);
    let (status, replay) = f.post(BASE, f.body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(id(&replay), id(&accepted));
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.posts(), 3);
    assert_eq!(f.app.current_state().unwrap().installations.len(), 1);
    let installation_id = installation.installation_id.clone();
    let (status, invocation) = f.post("/capability-runs",json!({"client_request_id":"successor-after-restart",
        "installation_id":installation_id,"role_id":successor,"responsibility_id":f.body["responsibility_id"],
        "input":suite(true).cases[0].input,"suite_id":suite(true).suite_id,"required_scope":"capability.venue_lookup"})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{invocation}");
    let job = invocation["job_id"].as_str().unwrap();
    for _ in 0..120 {
        let (status, proof) = f
            .request(
                Method::GET,
                &format!("/capability-runs/{job}/verification"),
                "operator",
                None,
            )
            .await;
        if status == StatusCode::OK {
            assert_eq!(proof["decision"], "verified", "{proof}");
            assert_eq!(proof["acceptance_report_ref"], accepted["acceptance_ref"]);
            assert_eq!(f.posts(), 4);
            assert!(f.app.store.commitments().unwrap().is_empty());
            return;
        }
        assert_eq!(status, StatusCode::NOT_FOUND, "{proof}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("successor invocation did not finalize");
}

#[tokio::test]
async fn operator_only_strict_intent_and_access_acknowledgment_precede_all_effects() {
    let f = Fixture::new(Behavior::Good, false).await;
    for principal in ["agent", "adapter", "unknown"] {
        let (status, body) = f
            .request(Method::POST, BASE, principal, Some(f.body.clone()))
            .await;
        assert_eq!(
            status,
            if principal == "unknown" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::FORBIDDEN
            },
            "{body}"
        );
    }
    for field in [
        "capability_id",
        "artifact",
        "manifest",
        "oracle_suite",
        "checkpoint",
        "model",
        "query",
        "passed",
        "job_ids",
    ] {
        let mut body = f.body.clone();
        body[field] = json!("HTTP caller cannot import authority");
        let (status, reply) = f.post(BASE, body).await;
        assert!(status.is_client_error(), "{field}: {reply}");
    }
    let mut unacknowledged = f.body.clone();
    unacknowledged["provider_access_acknowledged"] = json!(false);
    assert!(f.post(BASE, unacknowledged).await.0.is_client_error());
    assert!(f.registry_log.lock().unwrap().is_empty());
    assert_eq!(f.posts(), 0);
    let created = f.create().await;
    let listing = f.get(BASE).await;
    assert_eq!(listing["items"].as_array().unwrap().len(), 1);
    assert_eq!(listing["items"][0]["id"], created["id"]);
    assert!(f
        .request(
            Method::GET,
            &format!("{BASE}?untrusted=1"),
            "operator",
            None
        )
        .await
        .0
        .is_client_error());
    for principal in ["agent", "adapter"] {
        assert_eq!(
            f.request(Method::GET, BASE, principal, None).await.0,
            StatusCode::FORBIDDEN
        );
        for (method, path, body) in [
            (Method::GET, format!("{BASE}/{}", id(&created)), None),
            (
                Method::POST,
                format!("{BASE}/{}/step", id(&created)),
                Some(json!({})),
            ),
        ] {
            let (status, reply) = f.request(method, &path, principal, body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
        }
    }
    let (status, _) = f
        .post(
            &format!("{BASE}/{}/step", id(&created)),
            json!({"retry_read":true}),
        )
        .await;
    assert!(status.is_client_error());
    let mut conflict = f.body.clone();
    conflict["deadline"] = (Utc::now() + ChronoDuration::minutes(10))
        .to_rfc3339()
        .into();
    assert_eq!(f.post(BASE, conflict).await.0, StatusCode::CONFLICT);
    let (status, body) = f
        .request(
            Method::GET,
            &format!("{BASE}/adoption-foreign"),
            "operator",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(f.registry_log.lock().unwrap().is_empty());
    assert_eq!(f.posts(), 0);
}

#[tokio::test]
async fn restart_after_atomic_install_and_batch_preserves_identity_without_repeating_posts() {
    let mut f = Fixture::new(Behavior::Good, false).await;
    let created = f.create().await;
    let selected = f.step(id(&created)).await;
    assert_eq!(selected["phase"], "selected", "{selected}");
    assert_eq!(f.app.current_state().unwrap().installations.len(), 1);
    assert!(f.app.store.list_jobs().unwrap().is_empty());
    let installation = f
        .app
        .current_state()
        .unwrap()
        .installations
        .keys()
        .next()
        .unwrap()
        .clone();
    f.restart(false).await;
    assert_eq!(f.get(&format!("{BASE}/{}", id(&created))).await, selected);
    let checking = f.step(id(&created)).await;
    assert_eq!(checking["phase"], "checking", "{checking}");
    let jobs = f.app.store.list_jobs().unwrap();
    assert_eq!(jobs.len(), 3);
    assert_eq!(
        f.posts(),
        0,
        "adoption stages delegate effects to existing run workers"
    );
    f.restart(true).await;
    f.wait(id(&created), "accepted").await;
    assert_eq!(
        f.app
            .current_state()
            .unwrap()
            .installations
            .keys()
            .collect::<Vec<_>>(),
        vec![&installation]
    );
    let reopened = f.app.store.list_jobs().unwrap();
    assert_eq!(
        jobs.iter().map(|j| &j.job_id).collect::<Vec<_>>(),
        reopened.iter().map(|j| &j.job_id).collect::<Vec<_>>()
    );
    assert_eq!(f.posts(), 3);
    assert_eq!(
        f.registry_log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/registry/v1/search")
            .count(),
        1
    );
}

#[tokio::test]
async fn advertised_success_cannot_override_fresh_local_semantic_rejection() {
    let mut f = Fixture::new(Behavior::Wrong, true).await;
    let created = f.create().await;
    let rejected = f.wait(id(&created), "rejected").await;
    let state = f.app.current_state().unwrap();
    assert_eq!(state.installations.len(), 1);
    let installation = state.installations.values().next().unwrap();
    assert_eq!(installation.status, InstallationStatus::Rejected);
    assert!(f
        .app
        .store
        .capability_acceptance(&installation.installation_id)
        .unwrap()
        .is_none());
    assert!(f.app.store.commitments().unwrap().is_empty());
    assert_eq!(f.posts(), 3);
    f.restart(true).await;
    assert_eq!(f.get(&format!("{BASE}/{}", id(&created))).await, rejected);
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        f.posts(),
        3,
        "semantic rejection cannot select another candidate or re-synthesize"
    );
}

#[tokio::test]
async fn ambiguous_run_submission_requires_attention_and_is_not_replayed_after_restart() {
    let mut f = Fixture::new(Behavior::Ambiguous, true).await;
    let created = f.create().await;
    let attention = f.wait(id(&created), "attention").await;
    let posts = f.posts();
    assert!((1..=3).contains(&posts));
    let jobs = f.app.store.list_jobs().unwrap();
    assert_eq!(jobs.len(), 3);
    let ambiguous: Vec<_> = jobs
        .iter()
        .filter_map(|job| {
            let run = f.app.store.get_capability_run(&job.job_id).unwrap();
            (run.phase == CapabilityRunPhase::NeedsReconciliation)
                .then(|| (job.job_id.clone(), serde_json::to_value(run).unwrap()))
        })
        .collect();
    assert!(!ambiguous.is_empty());
    let stopped_installation = assert_stopped_installation(&f, &attention);
    f.restart(true).await;
    assert_eq!(f.get(&format!("{BASE}/{}", id(&created))).await, attention);
    for _ in 0..3 {
        f.step(id(&created)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(f.posts(), posts);
    assert_eq!(f.app.store.list_jobs().unwrap().len(), 3);
    assert!(f
        .app
        .current_state()
        .unwrap()
        .installations
        .values()
        .all(|i| i.status != InstallationStatus::Accepted));
    for (job_id, saved) in &ambiguous {
        assert_eq!(
            serde_json::to_value(f.app.store.get_capability_run(job_id).unwrap()).unwrap(),
            *saved,
            "unknown submissions remain reconciliation evidence, not retryable work"
        );
    }
    assert_stopped_installation(&f, &attention);
    f.assert_private_inputs();
    // A fresh explicit intent can reconsider the same candidate. It neither
    // resumes the unknown native request nor submits any child jobs yet.
    f.restart(false).await;
    let mut fresh_body = f.body.clone();
    fresh_body["client_request_id"] = json!("explicit-new-adoption-after-attention");
    let (status, fresh) = f.post(BASE, fresh_body).await;
    assert_eq!(status, StatusCode::CREATED, "{fresh}");
    assert_ne!(id(&fresh), id(&attention));
    let selected = f.step(id(&fresh)).await;
    assert_eq!(selected["phase"], "selected", "{selected}");
    assert_eq!(selected["selection"]["capability_id"], CAPABILITY);
    let next_installation = InstallationId::new(selected["installation_id"].as_str().unwrap());
    assert_ne!(next_installation, stopped_installation);
    let state = f.app.current_state().unwrap();
    assert_eq!(state.installations.len(), 2);
    assert_eq!(
        state.installations[&next_installation].status,
        InstallationStatus::Checking
    );
    assert_eq!(
        state.installations[&stopped_installation].status,
        InstallationStatus::Rejected
    );
    assert_eq!(f.posts(), posts);
    assert_eq!(f.app.store.list_jobs().unwrap().len(), 3);
    for (job_id, saved) in ambiguous {
        assert_eq!(
            serde_json::to_value(f.app.store.get_capability_run(&job_id).unwrap()).unwrap(),
            saved
        );
    }
}

fn assert_stopped_installation(f: &Fixture, status: &Value) -> InstallationId {
    assert!(matches!(
        status["phase"].as_str(),
        Some("attention" | "fenced")
    ));
    assert!(status["acceptance_ref"].is_null());
    assert!(
        status["rejection_ref"].is_null(),
        "stopping work is not a semantic oracle rejection"
    );
    let installation_id = InstallationId::new(status["installation_id"].as_str().unwrap());
    let state = f.app.current_state().unwrap();
    let installation = &state.installations[&installation_id];
    assert_eq!(installation.status, InstallationStatus::Rejected);
    let expected = format!(
        "adoption-stopped:{}:{}",
        id(status),
        status["version"].as_u64().unwrap()
    );
    assert_eq!(
        installation.acceptance_report_ref.as_deref(),
        Some(expected.as_str())
    );
    assert!(f
        .app
        .store
        .capability_acceptance(&installation_id)
        .unwrap()
        .is_none());
    installation_id
}

#[tokio::test]
async fn revoked_requirement_executor_cannot_prepare_or_run_acceptance_checks() {
    let f = Fixture::new(Behavior::Good, false).await;
    let created = f.create().await;
    let selected = f.step(id(&created)).await;
    assert_eq!(selected["phase"], "selected");
    f.expire().await;
    let before = f.app.current_state().unwrap();
    let installation_id = InstallationId::new(selected["installation_id"].as_str().unwrap());
    let sql = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_fenced BEFORE UPDATE ON capability_adoptions WHEN NEW.phase='fenced' BEGIN SELECT RAISE(ABORT,'fixture terminal commit failure'); END;").unwrap();
    let (status, _) = f
        .post(&format!("{BASE}/{}/step", id(&created)), json!({}))
        .await;
    assert!(status.is_server_error() || status.is_client_error());
    assert_eq!(
        f.app.current_state().unwrap(),
        before,
        "failed terminal save must roll back installation closure"
    );
    assert_eq!(
        f.app.current_state().unwrap().installations[&installation_id].status,
        InstallationStatus::Checking
    );
    assert_eq!(f.get(&format!("{BASE}/{}", id(&created))).await, selected);
    sql.execute_batch("DROP TRIGGER fail_fenced;").unwrap();
    let fenced = f.step(id(&created)).await;
    assert_eq!(fenced["phase"], "fenced", "{fenced}");
    assert_stopped_installation(&f, &fenced);
    let committed = f.app.current_state().unwrap();
    assert!(f.app.store.list_jobs().unwrap().is_empty());
    assert_eq!(f.posts(), 0);
    assert_eq!(f.step(id(&created)).await, fenced);
    assert_eq!(
        f.app.current_state().unwrap(),
        committed,
        "terminal replay must not close installation twice"
    );
}

#[tokio::test]
async fn intent_stage_failure_rolls_back_installation_and_whole_check_batch() {
    let mut f = Fixture::new(Behavior::Good, false).await;
    let created = f.create().await;
    let sql = rusqlite::Connection::open(f.config.db.as_ref().unwrap()).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_selected BEFORE UPDATE ON capability_adoptions WHEN NEW.phase='selected' BEGIN SELECT RAISE(ABORT,'fixture selected commit failure'); END;").unwrap();
    let (status, _) = f
        .post(&format!("{BASE}/{}/step", id(&created)), json!({}))
        .await;
    assert!(status.is_server_error() || status.is_client_error());
    assert!(f.app.current_state().unwrap().installations.is_empty());
    assert!(f.app.store.list_jobs().unwrap().is_empty());
    assert_eq!(
        f.get(&format!("{BASE}/{}", id(&created))).await["phase"],
        "discovering"
    );
    sql.execute_batch("DROP TRIGGER fail_selected;").unwrap();
    assert_eq!(f.step(id(&created)).await["phase"], "selected");
    let installation = f
        .app
        .current_state()
        .unwrap()
        .installations
        .keys()
        .next()
        .unwrap()
        .clone();
    let (status, refusal) = f
        .post(
            &format!("/capability-installations/{installation}/checks"),
            json!({"client_request_id":"attempt-unowned-checks", "role_id":f.body["role_id"],
            "responsibility_id":f.body["responsibility_id"], "suite_id":suite(true).suite_id}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["error"], "adoption_owned");
    assert!(f.app.store.list_jobs().unwrap().is_empty());
    let (status, refusal) = f
        .post(
            &format!("/capability-installations/{installation}/acceptance"),
            json!({"suite_id":suite(true).suite_id, "job_ids":[]}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(
        refusal["error"], "adoption_owned",
        "installation ownership is checked before interpreting supplied evidence"
    );
    sql.execute_batch("CREATE TRIGGER fail_checking BEFORE UPDATE ON capability_adoptions WHEN NEW.phase='checking' BEGIN SELECT RAISE(ABORT,'fixture batch commit failure'); END;").unwrap();
    let before = f.app.current_state().unwrap();
    let (status, _) = f
        .post(&format!("{BASE}/{}/step", id(&created)), json!({}))
        .await;
    assert!(status.is_server_error() || status.is_client_error());
    assert_eq!(f.app.current_state().unwrap(), before);
    assert!(f.app.store.list_jobs().unwrap().is_empty());
    let batches: u64 = sql
        .query_row("SELECT COUNT(*) FROM capability_check_batches", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(batches, 0);
    assert_eq!(
        f.get(&format!("{BASE}/{}", id(&created))).await["phase"],
        "selected"
    );
    sql.execute_batch("DROP TRIGGER fail_checking;").unwrap();
    assert_eq!(f.step(id(&created)).await["phase"], "checking");
    let jobs = f.app.store.list_jobs().unwrap();
    for job in &jobs {
        for _ in 0..2 {
            let (status, _) = f
                .post(&format!("/capability-runs/{}/step", job.job_id), json!({}))
                .await;
            assert_eq!(status, StatusCode::OK);
        }
        assert_eq!(
            f.app.store.get_capability_run(&job.job_id).unwrap().phase,
            CapabilityRunPhase::Collected
        );
    }
    let installation = f
        .app
        .current_state()
        .unwrap()
        .installations
        .keys()
        .next()
        .unwrap()
        .clone();
    let (status, body) = f.post(&format!("/capability-installations/{installation}/acceptance"),
        json!({"suite_id":suite(true).suite_id,"job_ids":jobs.iter().map(|job| &job.job_id).collect::<Vec<_>>()})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "adoption_owned");
    assert!(f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .is_none());
    sql.execute_batch("CREATE TRIGGER fail_accepted BEFORE UPDATE ON capability_adoptions WHEN NEW.phase='accepted' BEGIN SELECT RAISE(ABORT,'fixture acceptance commit failure'); END;").unwrap();
    let (status, _) = f
        .post(&format!("{BASE}/{}/step", id(&created)), json!({}))
        .await;
    assert!(status.is_server_error() || status.is_client_error());
    assert_eq!(
        f.app.current_state().unwrap().installations[&installation].status,
        InstallationStatus::Checking
    );
    assert!(f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .is_none());
    assert_eq!(
        f.get(&format!("{BASE}/{}", id(&created))).await["phase"],
        "checking"
    );
    sql.execute_batch("DROP TRIGGER fail_accepted;").unwrap();
    f.restart(true).await;
    f.wait(id(&created), "accepted").await;
    assert_eq!(f.posts(), 3);
    assert_eq!(f.app.current_state().unwrap().installations.len(), 1);
}

#[tokio::test]
async fn prepared_child_checks_reject_changed_requirement_or_suite_before_the_adoption_worker_runs()
{
    for change_suite in [false, true] {
        let mut f = Fixture::new(Behavior::Good, false).await;
        let created = f.create().await;
        assert_eq!(f.step(id(&created)).await["phase"], "selected");
        assert_eq!(f.step(id(&created)).await["phase"], "checking");
        let job = f.app.store.list_jobs().unwrap().remove(0);
        if change_suite {
            f.config.oracle_suites[0].scope.push_str(" changed policy");
            f.restart(false).await;
        } else {
            let requirement = f.body["requirement_id"].as_str().unwrap().into();
            f.app
                .mutate(EventMeta::default(), |state| {
                    let mut next = state.clone();
                    next.requirements
                        .get_mut(&requirement)
                        .unwrap()
                        .summary
                        .push_str(" changed mission need");
                    next.revision += 1;
                    Ok((
                        engine::Outcome {
                            state: next,
                            effects: vec![],
                        },
                        (),
                    ))
                })
                .await
                .unwrap();
        }
        let current = f.app.current_state().unwrap();
        let assignment = current.active_assignment(&job.responsibility_id).unwrap();
        assert_eq!(assignment.assignment_id, job.assignment_id);
        assert_eq!(assignment.epoch, job.epoch);
        assert_eq!(current.roles[&job.role_id].state, RoleState::Active);
        let (status, child) = f
            .post(&format!("/capability-runs/{}/step", job.job_id), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK, "{child}");
        assert_eq!(child["phase"], "rejected", "{child}");
        assert!(child["evidence"].is_null());
        assert!(
            f.world.lock().unwrap().seen.is_empty(),
            "no provider call is authorized by an unchanged grant alone"
        );
        let terminal = f.step(id(&created)).await;
        assert_eq!(
            terminal["phase"],
            if change_suite { "attention" } else { "fenced" },
            "{terminal}"
        );
        assert!(terminal["acceptance_ref"].is_null());
    }
}
