//! App-scoped Trigger contract over loopback fixtures only. No CREAO calls.
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::Response,
    routing::any,
    Router,
};
use genesis_creao::{
    AccountLimiter, ApiVersion, CreaoClient, CreateAgentRequest, ErrorKind, Limits, OperationPhase,
    OperationPurpose, OperationRequest, OperationSpec, QuotaLimits, RunObservation, RunRequest,
    RunTicket, Runtime, RuntimeError, RuntimeStore,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;

const APP: &str = "00000000-0000-4000-8000-000000000001";
const OTHER: &str = "00000000-0000-4000-8000-000000000002";
const RUN: &str = "00000000-0000-4000-8000-000000000003";
const THREAD: &str = "00000000-0000-4000-8000-000000000004";
const TOKEN: &str = "capi_synthetic_never_live";
const ACCOUNT: &str = "trigger-fixture";
type Captured = (String, String, Value, String);
#[derive(Clone)]
struct MockState {
    replies: Arc<Mutex<VecDeque<(u16, Value)>>>,
    requests: Arc<Mutex<Vec<Captured>>>,
}
struct Mock {
    url: String,
    state: MockState,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Mock {
    async fn start(replies: Vec<(u16, Value)>) -> Self {
        let state = MockState {
            replies: Arc::new(Mutex::new(replies.into())),
            requests: Arc::new(Mutex::new(vec![])),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/api/v1/apps/{APP}/runs",
            listener.local_addr().unwrap()
        );
        let router = Router::new()
            .route("/{*path}", any(reply))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, state, task }
    }
    fn client(&self) -> CreaoClient {
        CreaoClient::for_local_testing_trigger(TOKEN, APP, Limits::default(), limiter(), &self.url)
            .unwrap()
    }
    fn count(&self) -> usize {
        self.state.requests.lock().unwrap().len()
    }
}
async fn reply(State(state): State<MockState>, req: Request) -> Response<Body> {
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();
    let ua = req
        .headers()
        .get("user-agent")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(req.headers()["authorization"], format!("Bearer {TOKEN}"));
    let bytes = to_bytes(req.into_body(), 2 * 1024 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    state
        .requests
        .lock()
        .unwrap()
        .push((method, path, body, ua));
    let (status, body) = state
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected HTTP request");
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
fn limiter() -> AccountLimiter {
    AccountLimiter::in_memory(ACCOUNT, QuotaLimits::default()).unwrap()
}
fn run(app: &str) -> OperationRequest {
    OperationRequest::CreateRun(RunRequest {
        agent_id: app.into(),
        input: json!({"task":"bootstrap"}),
        model: None,
        conversation_id: None,
        workspace_id: None,
    })
}
fn spec(version: ApiVersion, app: &str) -> OperationSpec {
    OperationSpec {
        schema_version: 1,
        api_version: version,
        operation_id: "op-fixture".into(),
        org_id: "org-fixture".into(),
        role_id: "role-fixture".into(),
        account_namespace: ACCOUNT.into(),
        purpose: OperationPurpose::Provision {
            change_id: "change-fixture".into(),
            config_digest: "a".repeat(64),
        },
        request: run(app),
    }
}
fn post() -> Value {
    json!({"id":RUN,"appId":APP,"status":"pending"})
}

#[test]
fn trigger_configuration_accepts_only_exact_official_app_and_key_family() {
    let good = || CreaoClient::new_trigger(TOKEN, APP, None, Limits::default(), limiter());
    assert_eq!(good().unwrap().trigger_app_id(), Some(APP));
    for url in [
        "https://developer.creao.ai/v2/runs".to_owned(),
        format!("https://agent.creao.ai/api/v1/apps/{OTHER}/runs"),
        format!("https://agent.creao.ai/api/v1/apps/{APP}/runs?x=1"),
        format!("http://agent.creao.ai/api/v1/apps/{APP}/runs"),
        format!("https://evil.invalid/api/v1/apps/{APP}/runs"),
        format!("https://agent.creao.ai:443/api/v1/apps/{APP}/runs"),
        format!("https://AGENT.CREAO.AI/api/v1/apps/{APP}/runs"),
        format!("https://agent.creao.ai/x/../api/v1/apps/{APP}/runs"),
    ] {
        assert!(
            CreaoClient::new_trigger(TOKEN, APP, Some(&url), Limits::default(), limiter()).is_err()
        );
    }
    assert!(
        CreaoClient::new_trigger("cr_sk_synthetic", APP, None, Limits::default(), limiter())
            .is_err()
    );
    assert!(
        CreaoClient::new_for_version(TOKEN, Limits::default(), limiter(), ApiVersion::Trigger)
            .is_err()
    );
    assert!(CreaoClient::for_local_testing_trigger(
        TOKEN,
        APP,
        Limits::default(),
        limiter(),
        &format!("http://localhost/api/v1/apps/{APP}/runs")
    )
    .is_err());
}

#[tokio::test]
async fn exact_trigger_wire_and_get_omission_survive_durable_validation() {
    let mock = Mock::start(vec![
        (202, post()),
        (
            200,
            json!({"id":RUN,"status":"completed","threadId":THREAD,"result":{"text":"done"}}),
        ),
    ])
    .await;
    let client = mock.client();
    let request = run(APP);
    let ticket = client.submit(&request).await.unwrap();
    assert_eq!(
        serde_json::to_value(&ticket).unwrap()["api_version"],
        "trigger"
    );
    assert_eq!(ticket.agent_id(), Some(APP));
    let ticket: RunTicket = serde_json::from_value(serde_json::to_value(ticket).unwrap()).unwrap();
    let observation = client.poll(&request, &ticket).await.unwrap();
    observation.validate_for(&request, &ticket).unwrap();
    let refined = ticket.refine(&request, &observation).unwrap();
    let restored: RunObservation =
        serde_json::from_value(serde_json::to_value(&observation).unwrap()).unwrap();
    restored.validate_for(&request, &refined).unwrap();
    let mut altered = serde_json::to_value(restored).unwrap();
    altered["raw_response"]["id"] = json!(OTHER);
    assert!(serde_json::from_value::<RunObservation>(altered)
        .unwrap()
        .validate_for(&request, &refined)
        .is_err());
    let requests = mock.state.requests.lock().unwrap();
    assert_eq!(requests[0].0, "POST");
    assert_eq!(requests[0].1, format!("/api/v1/apps/{APP}/runs"));
    assert_eq!(requests[0].2, json!({"inputs":{"task":"bootstrap"}}));
    assert_eq!(requests[1].0, "GET");
    assert_eq!(requests[1].1, format!("/api/v1/apps/{APP}/runs/{RUN}"));
    assert!(requests.iter().all(|r| r.3.starts_with("Mozilla/5.0")));
}

#[tokio::test]
async fn unsupported_management_and_options_fail_before_any_http() {
    let mock = Mock::start(vec![]).await;
    let client = mock.client();
    let create = OperationRequest::CreateAgent(CreateAgentRequest {
        input: "create".into(),
        model: None,
        conversation_id: None,
    });
    assert!(!client.submit(&create).await.unwrap_err().post_may_have_run);
    assert_eq!(
        client.get_agent(APP).await.unwrap_err().kind,
        ErrorKind::InvalidRequest
    );
    assert!(
        !client
            .submit(&run(OTHER))
            .await
            .unwrap_err()
            .post_may_have_run
    );
    for field in ["model", "conversation_id", "workspace_id"] {
        let mut value = serde_json::to_value(run(APP)).unwrap();
        value["request"][field] = json!(OTHER);
        let request = serde_json::from_value(value).unwrap();
        assert!(!client.submit(&request).await.unwrap_err().post_may_have_run);
    }
    assert_eq!(mock.count(), 0);
}

#[tokio::test]
async fn alien_or_missing_post_app_requires_reconciliation_and_never_reposts() {
    for body in [
        json!({"id":RUN,"appId":OTHER,"status":"pending"}),
        json!({"id":RUN,"status":"pending"}),
        json!({"unexpected":"lost-receipt"}),
    ] {
        let mock = Mock::start(vec![(202, body)]).await;
        let runtime = Runtime::new(
            mock.client(),
            RuntimeStore::in_memory("org-fixture").unwrap(),
            Duration::from_millis(1),
        )
        .unwrap();
        runtime.prepare(spec(ApiVersion::Trigger, APP)).unwrap();
        let record = runtime.step("op-fixture", false).await.unwrap();
        assert_eq!(record.phase, OperationPhase::NeedsReconciliation);
        assert!(matches!(
            runtime.step("op-fixture", false).await,
            Err(RuntimeError::NeedsReconciliation)
        ));
        assert!(matches!(
            runtime.step("op-fixture", true).await,
            Err(RuntimeError::NeedsReconciliation)
        ));
        assert_eq!(mock.count(), 1);
    }
}

#[tokio::test]
async fn alien_get_app_or_run_and_changed_thread_never_validate() {
    for body in [
        json!({"id":OTHER,"status":"completed"}),
        json!({"id":RUN,"appId":OTHER,"status":"completed"}),
        json!({"id":RUN,"status":"completed","threadId":OTHER}),
    ] {
        let mut accepted = post();
        accepted["threadId"] = json!(THREAD);
        let mock = Mock::start(vec![(202, accepted), (200, body)]).await;
        let client = mock.client();
        let request = run(APP);
        let ticket = client.submit(&request).await.unwrap();
        assert_eq!(
            client.poll(&request, &ticket).await.unwrap_err().kind,
            ErrorKind::ProvenanceMismatch
        );
        assert_eq!(mock.count(), 2);
    }
}

#[tokio::test]
async fn runtime_pins_app_and_does_not_migrate_v2_or_other_app_checkpoints() {
    let mock = Mock::start(vec![]).await;
    let store = RuntimeStore::in_memory("org-fixture").unwrap();
    let mut old = spec(ApiVersion::V2, APP);
    old.purpose = OperationPurpose::Execution {
        job_id: "job".into(),
        assignment_id: "assignment".into(),
        epoch: 1,
    };
    store.prepare(old.clone()).unwrap();
    let runtime = Runtime::new(mock.client(), store, Duration::from_millis(1)).unwrap();
    assert_eq!(runtime.trigger_app_id(), Some(APP));
    assert!(matches!(
        runtime.get("op-fixture"),
        Err(RuntimeError::ContextMismatch)
    ));
    assert!(matches!(
        runtime.step("op-fixture", false).await,
        Err(RuntimeError::ContextMismatch)
    ));
    assert!(matches!(
        runtime.prepare(old),
        Err(RuntimeError::ContextMismatch)
    ));
    let mut foreign = spec(ApiVersion::Trigger, OTHER);
    foreign.operation_id = "foreign".into();
    assert!(matches!(
        runtime.prepare(foreign.clone()),
        Err(RuntimeError::ContextMismatch)
    ));
    let store = RuntimeStore::in_memory("org-fixture").unwrap();
    store.prepare(foreign).unwrap();
    let runtime = Runtime::new(mock.client(), store, Duration::from_millis(1)).unwrap();
    assert!(matches!(
        runtime.step("foreign", false).await,
        Err(RuntimeError::ContextMismatch)
    ));
    assert_ne!(
        spec(ApiVersion::Trigger, APP).request_sha256().unwrap(),
        spec(ApiVersion::Trigger, OTHER).request_sha256().unwrap()
    );
    assert!(spec(ApiVersion::V2, APP).validate().is_err()); // Provision/CreateRun is Trigger-only.
    assert_eq!(mock.count(), 0);
}

#[tokio::test]
async fn provision_is_one_app_run_and_unknown_status_never_completes_or_reposts() {
    let mock = Mock::start(vec![
        (202, post()),
        (200, json!({"id":RUN,"status":"future_status"})),
    ])
    .await;
    let runtime = Runtime::new(
        mock.client(),
        RuntimeStore::in_memory("org-fixture").unwrap(),
        Duration::from_millis(1),
    )
    .unwrap();
    runtime.prepare(spec(ApiVersion::Trigger, APP)).unwrap();
    assert_eq!(
        runtime.step("op-fixture", false).await.unwrap().phase,
        OperationPhase::Waiting
    );
    tokio::time::sleep(Duration::from_millis(3)).await;
    assert_eq!(
        runtime.step("op-fixture", false).await.unwrap().phase,
        OperationPhase::NeedsReconciliation
    );
    assert!(matches!(
        runtime.step("op-fixture", false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(mock.count(), 2);
}

#[tokio::test]
async fn trigger_guard_rejection_is_local_and_never_sends_http() {
    let mock = Mock::start(vec![]).await;
    let runtime = Runtime::new(
        mock.client(),
        RuntimeStore::in_memory("org-fixture").unwrap(),
        Duration::from_millis(1),
    )
    .unwrap();
    let initial = runtime.prepare(spec(ApiVersion::Trigger, APP)).unwrap();
    let guard: genesis_creao::PreEffectGuard = Arc::new(|| Box::pin(async { false }));
    let rejected = runtime
        .step_guarded("op-fixture", false, guard)
        .await
        .unwrap();
    assert_eq!(rejected.phase, OperationPhase::Failed);
    assert_eq!(mock.count(), 0);
    assert!(initial.ticket.is_none() && rejected.ticket.is_none());
    assert!(
        matches!(rejected.failure, Some(genesis_creao::RuntimeFailure::Provider(ref e)) if e.kind == ErrorKind::GuardRejected && !e.post_may_have_run)
    );
}

async fn completed(runtime: &Runtime, value: OperationSpec) -> genesis_creao::OperationRecord {
    let id = value.operation_id.clone();
    runtime.prepare(value).unwrap();
    assert_eq!(
        runtime.step(&id, false).await.unwrap().phase,
        OperationPhase::Waiting
    );
    tokio::time::sleep(Duration::from_millis(3)).await;
    let record = runtime.step(&id, false).await.unwrap();
    assert_eq!(record.phase, OperationPhase::NativeCompleted);
    record
}

#[tokio::test]
async fn completed_provision_requires_unique_exact_contract_and_never_mutates_records() {
    let mock = Mock::start(vec![
        (202, post()),
        (
            200,
            json!({"id":RUN,"status":"completed","result":{"text":"ready"}}),
        ),
        (202, json!({"id":OTHER,"appId":APP,"status":"pending"})),
        (
            200,
            json!({"id":OTHER,"status":"completed","result":{"text":"ready"}}),
        ),
    ])
    .await;
    let runtime = Runtime::new(
        mock.client(),
        RuntimeStore::in_memory("org-fixture").unwrap(),
        Duration::from_millis(1),
    )
    .unwrap();
    assert!(matches!(
        runtime.completed_provision("role-fixture", &"a".repeat(64)),
        Err(RuntimeError::NotFound)
    ));
    let first = completed(&runtime, spec(ApiVersion::Trigger, APP)).await;
    assert_eq!(
        runtime
            .completed_provision("role-fixture", &"a".repeat(64))
            .unwrap(),
        first
    );
    assert!(matches!(
        runtime.completed_provision("role-fixture", &"b".repeat(64)),
        Err(RuntimeError::NotFound)
    ));
    assert!(matches!(
        runtime.completed_provision("other-role", &"a".repeat(64)),
        Err(RuntimeError::NotFound)
    ));
    assert_eq!(runtime.get(&first.spec.operation_id).unwrap(), first);
    assert_eq!(mock.count(), 2);
    let mut next = spec(ApiVersion::Trigger, APP);
    next.operation_id = "second-provision".into();
    next.purpose = OperationPurpose::Provision {
        change_id: "another-change".into(),
        config_digest: "a".repeat(64),
    };
    let second = completed(&runtime, next).await;
    assert!(matches!(
        runtime.completed_provision("role-fixture", &"a".repeat(64)),
        Err(RuntimeError::Conflict)
    ));
    assert_eq!(runtime.get(&first.spec.operation_id).unwrap(), first);
    assert_eq!(runtime.get(&second.spec.operation_id).unwrap(), second);
    assert_eq!(mock.count(), 4);
}

#[tokio::test]
async fn completed_provision_never_returns_another_app_or_account_contract() {
    let mock = Mock::start(vec![
        (202, post()),
        (200, json!({"id":RUN,"status":"completed","result":{}})),
    ])
    .await;
    let store = RuntimeStore::in_memory("org-fixture").unwrap();
    let runtime = Runtime::new(mock.client(), store.clone(), Duration::from_millis(1)).unwrap();
    let first = completed(&runtime, spec(ApiVersion::Trigger, APP)).await;
    let foreign_app = CreaoClient::for_local_testing_trigger(
        TOKEN,
        OTHER,
        Limits::default(),
        limiter(),
        &mock.url.replace(APP, OTHER),
    )
    .unwrap();
    let foreign_app = Runtime::new(foreign_app, store.clone(), Duration::from_millis(1)).unwrap();
    assert!(matches!(
        foreign_app.completed_provision("role-fixture", &"a".repeat(64)),
        Err(RuntimeError::NotFound)
    ));
    let foreign_account = CreaoClient::for_local_testing_trigger(
        TOKEN,
        APP,
        Limits::default(),
        AccountLimiter::in_memory("another-account", QuotaLimits::default()).unwrap(),
        &mock.url,
    )
    .unwrap();
    let foreign_account = Runtime::new(foreign_account, store, Duration::from_millis(1)).unwrap();
    assert!(matches!(
        foreign_account.completed_provision("role-fixture", &"a".repeat(64)),
        Err(RuntimeError::NotFound)
    ));
    assert_eq!(runtime.get(&first.spec.operation_id).unwrap(), first);
    assert_eq!(mock.count(), 2);
}
