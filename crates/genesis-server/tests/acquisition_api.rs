//! Real HTTP boundary tests with trusted local setup and loopback-only clients.
use axum::{routing::post, Json, Router};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_core::change::{KeyAllocation, OrgChangeProposal};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_forge::{AnthropicGenerator, ModelConfig};
use genesis_server::app::AppState;
use genesis_server::capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas};
use genesis_server::error::validation_error;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, AcquisitionConfig, OrgConfig};
use genesis_verification::{decode_json, OracleSuite};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

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
