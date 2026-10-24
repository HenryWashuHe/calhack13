//! Real HTTP adapter calls against a local synthetic provider. These tests do
//! not contact CREAO, establish cloud credentials, or prove mission acceptance.

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{Response, StatusCode},
    routing::any,
    Router,
};
use genesis_creao::{
    runtime::{
        OperationPhase, OperationPurpose, OperationRecord, OperationSpec, Runtime, RuntimeError,
        RuntimeFailure, RuntimeStore,
    },
    AccountLimiter, ApiVersion, CreaoClient, CreateAgentRequest, ErrorKind, Limits,
    NativeRunStatus, OperationRequest, PreEffectGuard, QuotaLimits, RunRequest,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};

const ORG: &str = "organization-a";
const ACCOUNT: &str = "synthetic-creao-account";
const AGENT: &str = "00000000-0000-4000-8000-000000000001";
const CONVERSATION: &str = "00000000-0000-4000-8000-000000000002";
const RUN_A: &str = "00000000-0000-4000-8000-000000000003";
const RUN_B: &str = "00000000-0000-4000-8000-000000000004";
const RUN_C: &str = "00000000-0000-4000-8000-000000000005";
const INTERVAL: Duration = Duration::from_millis(5);

struct Reply {
    status: u16,
    body: Value,
    hold: Option<Arc<Semaphore>>,
    retry_after: Option<&'static str>,
}
impl Reply {
    fn accepted(id: &str, agent: Option<&str>) -> Self {
        Self::new(202, run(id, "pending", agent))
    }
    fn observed(id: &str, status: &str, agent: Option<&str>) -> Self {
        Self::new(200, run(id, status, agent))
    }
    fn new(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            hold: None,
            retry_after: None,
        }
    }
    fn held(mut self, hold: &Arc<Semaphore>) -> Self {
        self.hold = Some(hold.clone());
        self
    }
}

#[derive(Clone)]
struct MockState {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    requests: Arc<Mutex<Vec<(String, String, Value)>>>,
}
struct Mock {
    state: MockState,
    base: String,
    task: JoinHandle<()>,
}
impl Mock {
    async fn start(replies: Vec<Reply>) -> Self {
        let state = MockState {
            replies: Arc::new(Mutex::new(replies.into())),
            requests: Arc::new(Mutex::new(vec![])),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/{*path}", any(serve))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { state, base, task }
    }
    fn client(&self, namespace: &str) -> CreaoClient {
        CreaoClient::for_local_testing(
            "cr_sk_synthetic_test_only",
            Limits {
                request_timeout: Duration::from_secs(1),
                ..Limits::default()
            },
            AccountLimiter::in_memory(namespace, QuotaLimits::default()).unwrap(),
            &self.base,
        )
        .unwrap()
    }
    fn client_v2(&self, namespace: &str) -> CreaoClient {
        CreaoClient::for_local_testing_v2(
            "cr_sk_synthetic_test_only",
            Limits {
                request_timeout: Duration::from_secs(1),
                ..Limits::default()
            },
            AccountLimiter::in_memory(namespace, QuotaLimits::default()).unwrap(),
            &self.base.replace("/v1", "/v2"),
        )
        .unwrap()
    }
    fn runtime(&self, store: RuntimeStore) -> Runtime {
        Runtime::new(self.client(ACCOUNT), store, INTERVAL).unwrap()
    }
    fn requests(&self) -> Vec<(String, String, Value)> {
        self.state.requests.lock().unwrap().clone()
    }
    async fn wait_requests(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.state.requests.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("expected local HTTP request");
    }
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(State(state): State<MockState>, request: Request) -> Response<Body> {
    assert_eq!(
        request.headers().get("authorization").unwrap(),
        "Bearer cr_sk_synthetic_test_only"
    );
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 512 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    state.requests.lock().unwrap().push((method, path, body));
    let reply = state
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_else(|| Reply::new(500, json!({"error":"UNEXPECTED_TEST_REQUEST"})));
    if let Some(hold) = reply.hold {
        let _ = tokio::time::timeout(Duration::from_secs(2), hold.acquire()).await;
    }
    let mut response = Response::builder()
        .status(StatusCode::from_u16(reply.status).unwrap())
        .header("Content-Type", "application/json");
    if let Some(seconds) = reply.retry_after {
        response = response.header("Retry-After", seconds);
    }
    response.body(Body::from(reply.body.to_string())).unwrap()
}

fn run(id: &str, status: &str, agent: Option<&str>) -> Value {
    json!({"run_id":id,"status":status,"agent_id":agent,"conversation_id":CONVERSATION,"model":"synthetic-model"})
}
fn creation(id: &str) -> OperationSpec {
    OperationSpec {
        schema_version: 1,
        api_version: genesis_creao::ApiVersion::V1,
        operation_id: id.into(),
        org_id: ORG.into(),
        role_id: "role-specialist".into(),
        account_namespace: ACCOUNT.into(),
        purpose: OperationPurpose::Provision {
            change_id: "change-create-specialist".into(),
            config_digest: "a".repeat(64),
        },
        request: OperationRequest::CreateAgent(CreateAgentRequest {
            input: "Create a venue monitoring specialist. Local synthetic test only.".into(),
            model: None,
            conversation_id: None,
        }),
    }
}

#[tokio::test]
async fn api_version_is_frozen_before_submission_and_on_reopen() {
    let mock = Mock::start(vec![]).await;
    let store = RuntimeStore::in_memory(ORG).unwrap();
    let v2 = Runtime::new(mock.client_v2(ACCOUNT), store.clone(), INTERVAL).unwrap();
    let old = creation("protocol-frozen");
    assert!(matches!(
        v2.prepare(old.clone()),
        Err(RuntimeError::ContextMismatch)
    ));
    let mut hosted = old;
    hosted.api_version = ApiVersion::V2;
    v2.prepare(hosted).unwrap();
    let v1 = mock.runtime(store);
    assert!(matches!(
        v1.get("protocol-frozen"),
        Err(RuntimeError::ContextMismatch)
    ));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn hosted_v2_creation_without_agent_id_retains_run_and_requires_exact_get() {
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, None),
        Reply::new(
            200,
            json!({"run_id":RUN_A,"agent_id":AGENT,"status":"succeeded"}),
        ),
        Reply::new(
            200,
            json!({"run_id":RUN_B,"agent_id":AGENT,"status":"succeeded"}),
        ),
        Reply::new(
            200,
            json!({"run_id":RUN_A,"agent_id":AGENT,"status":"succeeded"}),
        ),
    ])
    .await;
    let private = tempfile::tempdir().unwrap();
    let path = private.path().join("v2-creation.sqlite");
    let runtime = Runtime::new(
        mock.client_v2(ACCOUNT),
        RuntimeStore::open(&path, ORG).unwrap(),
        INTERVAL,
    )
    .unwrap();
    let mut spec = creation("hosted-no-agent-id");
    spec.api_version = ApiVersion::V2;
    runtime.prepare(spec.clone()).unwrap();
    let unknown = runtime.step(&spec.operation_id, false).await.unwrap();
    assert_eq!(unknown.phase, OperationPhase::NeedsReconciliation);
    assert_eq!(unknown.ticket.as_ref().unwrap().run_id(), RUN_A);
    assert_eq!(unknown.ticket.as_ref().unwrap().agent_id(), None);
    assert!(matches!(
        unknown.failure,
        Some(RuntimeFailure::MissingCreationAgent)
    ));
    assert_eq!(mock.requests().len(), 1);
    drop(runtime);

    let reopened = Runtime::new(
        mock.client_v2(ACCOUNT),
        RuntimeStore::open(&path, ORG).unwrap(),
        INTERVAL,
    )
    .unwrap();
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 0);
    assert!(matches!(
        reopened.step(&spec.operation_id, false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(mock.requests().len(), 1);
    assert!(matches!(
        reopened
            .reconcile_creation_agent(&spec.operation_id, "invalid")
            .await,
        Err(RuntimeError::Invalid)
    ));
    assert_eq!(mock.requests().len(), 1);
    assert!(matches!(
        reopened
            .reconcile_creation_agent(&spec.operation_id, RUN_B)
            .await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(reopened.get(&spec.operation_id).unwrap(), unknown);
    assert!(matches!(
        reopened
            .reconcile_creation_agent(&spec.operation_id, AGENT)
            .await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(reopened.get(&spec.operation_id).unwrap(), unknown);
    let completed = reopened
        .reconcile_creation_agent(&spec.operation_id, AGENT)
        .await
        .unwrap();
    assert_eq!(completed.phase, OperationPhase::NativeCompleted);
    assert_eq!(completed.ticket.as_ref().unwrap().run_id(), RUN_A);
    assert_eq!(completed.ticket.as_ref().unwrap().agent_id(), Some(AGENT));
    assert_eq!(mock.requests().len(), 4);
    assert_eq!(mock.requests()[0].0, "POST");
    assert!(mock.requests()[1]
        .1
        .ends_with(&format!("/agents/{RUN_B}/edits/{RUN_A}")));
    assert!(mock.requests()[2]
        .1
        .ends_with(&format!("/agents/{AGENT}/edits/{RUN_A}")));
    assert!(mock.requests()[3]
        .1
        .ends_with(&format!("/agents/{AGENT}/edits/{RUN_A}")));
}

#[tokio::test]
async fn hosted_v2_creation_uses_edit_poll_and_keeps_exact_agent() {
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, Some(AGENT)),
        Reply::new(
            200,
            json!({"run_id":RUN_A,"agent_id":AGENT,
            "status":"succeeded","result":{"agent_id":AGENT}}),
        ),
    ])
    .await;
    let runtime = Runtime::new(
        mock.client_v2(ACCOUNT),
        RuntimeStore::in_memory(ORG).unwrap(),
        INTERVAL,
    )
    .unwrap();
    let mut spec = creation("hosted-success");
    spec.api_version = ApiVersion::V2;
    runtime.prepare(spec.clone()).unwrap();
    let submitted = runtime.step(&spec.operation_id, false).await.unwrap();
    assert_eq!(submitted.phase, OperationPhase::Waiting);
    assert_eq!(submitted.ticket.as_ref().unwrap().agent_id(), Some(AGENT));
    due(&submitted).await;
    let completed = runtime.step(&spec.operation_id, false).await.unwrap();
    assert_eq!(completed.phase, OperationPhase::NativeCompleted);
    assert_eq!(
        completed.observation.as_ref().unwrap().agent_id.as_deref(),
        Some(AGENT)
    );
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].1, "/v2/agents");
    assert_eq!(requests[1].1, format!("/v2/agents/{AGENT}/edits/{RUN_A}"));

    // A stored ticket cannot be relabeled as a v1 operation even when the
    // operation fingerprint is recomputed for the changed spec.
    let mut mismatched = completed.clone();
    mismatched.spec.api_version = ApiVersion::V1;
    mismatched.fingerprint = mismatched.spec.fingerprint().unwrap();
    assert!(matches!(mismatched.validate(), Err(RuntimeError::Invalid)));
}
fn execution(id: &str, event: u64) -> OperationSpec {
    OperationSpec {
        schema_version: 1,
        api_version: genesis_creao::ApiVersion::V1,
        operation_id: id.into(),
        org_id: ORG.into(),
        role_id: "role-specialist".into(),
        account_namespace: ACCOUNT.into(),
        purpose: OperationPurpose::Execution {
            job_id: format!("job-{event}"),
            assignment_id: "assignment-specialist".into(),
            epoch: 7,
        },
        request: OperationRequest::CreateRun(RunRequest {
            agent_id: AGENT.into(),
            input: json!({"event":event,"task":"Observe venue availability"}),
            model: None,
            conversation_id: None,
            workspace_id: None,
        }),
    }
}
const READINESS_NONCE: &str = "READINESS_NONCE_MUST_NOT_APPEAR_IN_DEBUG";

fn readiness(id: &str, expires_at: chrono::DateTime<chrono::Utc>) -> OperationSpec {
    let mut spec = execution(id, 1);
    spec.purpose = OperationPurpose::Readiness {
        challenge_id: "rc-readiness-test".into(),
        config_digest: "b".repeat(64),
        expires_at,
    };
    if let OperationRequest::CreateRun(request) = &mut spec.request {
        request.input = json!({"challenge_id":"rc-readiness-test", "nonce":READINESS_NONCE});
    }
    spec
}

async fn due(record: &OperationRecord) {
    if let Some(at) = record.next_poll_at {
        let wait = (at - chrono::Utc::now()).to_std().unwrap_or_default();
        assert!(
            wait < Duration::from_secs(1),
            "test must not wait for a long cooldown"
        );
        tokio::time::sleep(wait + Duration::from_millis(2)).await;
    }
}
fn mutate(path: &Path, id: &str, edit: impl FnOnce(&mut Value)) {
    let connection = Connection::open(path).unwrap();
    let encoded: String = connection
        .query_row(
            "SELECT record_json FROM creao_operations WHERE operation_id=?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    let mut value: Value = serde_json::from_str(&encoded).unwrap();
    edit(&mut value);
    connection
        .execute(
            "UPDATE creao_operations SET record_json=?1 WHERE operation_id=?2",
            params![value.to_string(), id],
        )
        .unwrap();
}

#[tokio::test]
async fn creation_survives_restart_and_two_events_reuse_one_persistent_agent() {
    let mut created = run(RUN_A, "completed", None);
    created["result"] =
        json!({"agent_id":AGENT,"agent_name":"Synthetic specialist","text":"Created"});
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, None),
        Reply::observed(RUN_A, "running", None),
        Reply::new(200, created),
        Reply::accepted(RUN_B, Some(AGENT)),
        Reply::observed(RUN_B, "completed", Some(AGENT)),
        Reply::accepted(RUN_C, Some(AGENT)),
        Reply::observed(RUN_C, "completed", Some(AGENT)),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let spec = creation("create-specialist");
    runtime.prepare(spec.clone()).unwrap();
    let submitted = runtime.step(&spec.operation_id, false).await.unwrap();
    assert_eq!(submitted.phase, OperationPhase::Waiting);
    assert_eq!(submitted.ticket.as_ref().unwrap().agent_id(), None);
    assert_eq!(
        mock.requests().len(),
        1,
        "submission must not implicitly poll"
    );
    due(&submitted).await;
    let running = runtime.step(&spec.operation_id, false).await.unwrap();
    assert_eq!(
        running.observation.as_ref().unwrap().status,
        NativeRunStatus::Running
    );
    drop(runtime);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    assert_eq!(runtime.prepare(spec.clone()).unwrap(), running);
    due(&running).await;
    let completed = runtime.step(&spec.operation_id, false).await.unwrap();
    assert_eq!(completed.phase, OperationPhase::NativeCompleted);
    assert_eq!(
        completed.observation.as_ref().unwrap().agent_id.as_deref(),
        Some(AGENT)
    );
    for (index, id) in ["event-1", "event-2"].iter().enumerate() {
        runtime.prepare(execution(id, index as u64 + 1)).unwrap();
        let submitted = runtime.step(id, false).await.unwrap();
        due(&submitted).await;
        assert_eq!(
            runtime.step(id, false).await.unwrap().phase,
            OperationPhase::NativeCompleted
        );
    }
    let requests = mock.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|(method, path, _)| method == "POST" && path == "/v1/agents")
            .count(),
        1
    );
    let runs: Vec<_> = requests
        .iter()
        .filter(|(method, path, _)| method == "POST" && path == "/v1/runs")
        .collect();
    assert_eq!(runs.len(), 2);
    assert!(runs
        .iter()
        .all(|(_, _, body)| body["agent_id"] == AGENT && body["should_notify"] == false));
    assert_ne!(runs[0].2["input"], runs[1].2["input"]);
    assert_eq!(
        runtime.step("create-specialist", false).await.unwrap(),
        completed
    );
    assert_eq!(mock.requests().len(), 7);
}

#[test]
fn idempotency_binds_the_whole_request_and_organization_context() {
    let store = RuntimeStore::in_memory(ORG).unwrap();
    let spec = execution("same-key", 1);
    let original = store.prepare(spec.clone()).unwrap();
    assert_eq!(store.prepare(spec.clone()).unwrap(), original);
    let mut variants = vec![];
    let mut v = spec.clone();
    v.role_id = "other-role".into();
    variants.push(v);
    let mut v = spec.clone();
    v.account_namespace = "other-account".into();
    variants.push(v);
    let mut v = spec.clone();
    if let OperationPurpose::Execution { epoch, .. } = &mut v.purpose {
        *epoch += 1;
    }
    variants.push(v);
    let mut v = spec.clone();
    if let OperationPurpose::Execution { assignment_id, .. } = &mut v.purpose {
        *assignment_id = "other-assignment".into();
    }
    variants.push(v);
    let mut v = spec.clone();
    if let OperationPurpose::Execution { job_id, .. } = &mut v.purpose {
        *job_id = "other-job".into();
    }
    variants.push(v);
    let mut v = spec.clone();
    if let OperationRequest::CreateRun(r) = &mut v.request {
        r.input = json!({"event":99});
    }
    variants.push(v);
    for variant in variants {
        assert!(matches!(
            store.prepare(variant),
            Err(RuntimeError::Conflict)
        ));
    }
    let mut foreign = spec.clone();
    foreign.org_id = "organization-b".into();
    assert!(matches!(
        store.prepare(foreign),
        Err(RuntimeError::ContextMismatch)
    ));
    let mut wrong_kind = creation("bad-kind");
    wrong_kind.request = spec.request;
    assert!(matches!(
        store.prepare(wrong_kind),
        Err(RuntimeError::Invalid)
    ));
    assert_eq!(store.list().unwrap(), vec![original]);
}

#[tokio::test]
async fn cancelled_submission_recovers_without_a_second_post() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![Reply::accepted(RUN_A, None).held(&hold)]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let store = RuntimeStore::open(&path, ORG).unwrap();
    let runtime = mock.runtime(store.clone());
    runtime.prepare(creation("create")).unwrap();
    let task = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.step("create", false).await }
    });
    mock.wait_requests(1).await;
    assert_eq!(
        store.get("create").unwrap().phase,
        OperationPhase::Submitting,
        "POST intent must already be durable"
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(runtime);
    drop(store);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    assert_eq!(
        runtime.get("create").unwrap().failure,
        Some(RuntimeFailure::InterruptedSubmission)
    );
    assert!(matches!(
        runtime.step("create", false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(
        runtime.prepare(creation("create")).unwrap().phase,
        OperationPhase::NeedsReconciliation
    );
    assert_eq!(mock.requests().len(), 1);
    hold.add_permits(1);
}

#[tokio::test]
async fn cancelled_poll_restores_saved_ticket_and_only_repeats_get() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, Some(AGENT)),
        Reply::observed(RUN_A, "running", Some(AGENT)).held(&hold),
        Reply::observed(RUN_A, "completed", Some(AGENT)),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(execution("run", 1)).unwrap();
    let waiting = runtime.step("run", false).await.unwrap();
    due(&waiting).await;
    let task = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.step("run", false).await }
    });
    mock.wait_requests(2).await;
    let interrupted = runtime.get("run").unwrap();
    assert_eq!(interrupted.phase, OperationPhase::Polling);
    assert_eq!(interrupted.ticket, waiting.ticket);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(runtime);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    let recovered = runtime.get("run").unwrap();
    assert_eq!(recovered.ticket, waiting.ticket);
    due(&recovered).await;
    assert_eq!(
        runtime.step("run", false).await.unwrap().phase,
        OperationPhase::NativeCompleted
    );
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|(m, _, _)| m == "POST")
            .count(),
        1
    );
    hold.add_permits(1);
}

#[tokio::test]
async fn failed_intent_commit_prevents_http() {
    let mock = Mock::start(vec![]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(creation("create")).unwrap();
    Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER deny_checkpoint BEFORE UPDATE ON creao_operations BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
    assert!(matches!(
        runtime.step("create", false).await,
        Err(RuntimeError::Storage)
    ));
    assert_eq!(
        runtime.get("create").unwrap().phase,
        OperationPhase::Prepared
    );
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn lost_receipt_commit_leaves_ambiguous_post_intent() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![Reply::accepted(RUN_A, None).held(&hold)]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(creation("create")).unwrap();
    let task = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.step("create", false).await }
    });
    mock.wait_requests(1).await;
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER deny_receipt BEFORE UPDATE ON creao_operations BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
    hold.add_permits(1);
    assert!(matches!(task.await.unwrap(), Err(RuntimeError::Storage)));
    assert_eq!(
        runtime.get("create").unwrap().phase,
        OperationPhase::Submitting
    );
    db.execute_batch("DROP TRIGGER deny_receipt").unwrap();
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    assert!(matches!(
        runtime.step("create", false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn transient_get_retry_is_explicit_and_retains_identity() {
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, Some(AGENT)),
        Reply::new(503, json!({"error":"SERVICE_UNAVAILABLE"})),
        Reply::observed(RUN_A, "completed", Some(AGENT)),
    ])
    .await;
    let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
    runtime.prepare(execution("run", 1)).unwrap();
    let waiting = runtime.step("run", false).await.unwrap();
    due(&waiting).await;
    let failed = runtime.step("run", false).await.unwrap();
    assert_eq!(failed.phase, OperationPhase::Waiting);
    assert!(failed.retryable_read);
    assert_eq!(failed.ticket, waiting.ticket);
    assert!(matches!(
        runtime.step("run", false).await,
        Err(RuntimeError::RetryRequired)
    ));
    assert_eq!(mock.requests().len(), 2);
    due(&failed).await;
    assert_eq!(
        runtime.step("run", true).await.unwrap().phase,
        OperationPhase::NativeCompleted
    );
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|(m, _, _)| m == "POST")
            .count(),
        1
    );
}

#[tokio::test]
async fn retry_after_and_regular_poll_due_time_survive_restart() {
    let mut limited = Reply::new(429, json!({"error":"RATE_LIMITED"}));
    limited.retry_after = Some("60");
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT)), limited]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(execution("run", 1)).unwrap();
    let waiting = runtime.step("run", false).await.unwrap();
    due(&waiting).await;
    let limited = runtime.step("run", false).await.unwrap();
    assert!(limited.next_poll_at.unwrap() >= limited.updated_at + chrono::Duration::seconds(60));
    drop(runtime);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(runtime.step("run", true).await.unwrap(), limited);
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn unknown_native_status_and_wrong_run_identity_cannot_complete() {
    for (status, id, expected) in [
        ("future_status", RUN_A, OperationPhase::NeedsReconciliation),
        ("completed", RUN_B, OperationPhase::Failed),
    ] {
        let mock = Mock::start(vec![
            Reply::accepted(RUN_A, Some(AGENT)),
            Reply::observed(id, status, Some(AGENT)),
        ])
        .await;
        let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
        runtime.prepare(execution("run", 1)).unwrap();
        let waiting = runtime.step("run", false).await.unwrap();
        due(&waiting).await;
        let rejected = runtime.step("run", false).await.unwrap();
        assert_eq!(rejected.phase, expected);
        assert_eq!(rejected.ticket, waiting.ticket);
        assert_eq!(mock.requests().len(), 2);
    }
}

#[tokio::test]
async fn wrong_account_and_corrupt_stored_context_are_rejected_before_http() {
    let mock = Mock::start(vec![]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let store = RuntimeStore::open(&path, ORG).unwrap();
    let runtime = mock.runtime(store.clone());
    runtime.prepare(execution("run", 1)).unwrap();
    let foreign = Runtime::new(mock.client("foreign-account"), store, INTERVAL).unwrap();
    assert!(matches!(
        foreign.step("run", false).await,
        Err(RuntimeError::ContextMismatch)
    ));
    assert!(matches!(
        RuntimeStore::open(&path, "other-org"),
        Err(RuntimeError::ContextMismatch)
    ));
    mutate(&path, "run", |value| {
        value["spec"]["request"]["request"]["input"] = json!({"event":999})
    });
    assert!(matches!(
        runtime.step("run", false).await,
        Err(RuntimeError::Invalid)
    ));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn forged_completed_label_and_duplicate_checkpoint_keys_are_rejected() {
    let mock = Mock::start(vec![]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(creation("forged")).unwrap();
    runtime.prepare(creation("duplicate")).unwrap();
    mutate(&path, "forged", |v| v["phase"] = json!("native_completed"));
    assert!(matches!(runtime.get("forged"), Err(RuntimeError::Invalid)));
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE creao_operations SET record_json=substr(record_json,1,length(record_json)-1)||',\"phase\":\"prepared\"}' WHERE operation_id='duplicate'",[]).unwrap();
    assert!(matches!(
        runtime.get("duplicate"),
        Err(RuntimeError::Invalid)
    ));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn independent_runtime_handles_cannot_claim_one_post_twice() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![Reply::accepted(RUN_A, None).held(&hold)]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let first = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let second = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    first.prepare(creation("create")).unwrap();
    let task = tokio::spawn(async move { first.step("create", false).await });
    mock.wait_requests(1).await;
    assert!(matches!(
        second.step("create", false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    hold.add_permits(1);
    assert_eq!(task.await.unwrap().unwrap().phase, OperationPhase::Waiting);
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn learned_creation_identity_is_pinned_across_restart() {
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, None),
        Reply::observed(RUN_A, "running", Some(AGENT)),
        Reply::observed(RUN_A, "completed", Some(RUN_B)),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("org.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(creation("create")).unwrap();
    let waiting = runtime.step("create", false).await.unwrap();
    due(&waiting).await;
    let running = runtime.step("create", false).await.unwrap();
    assert_eq!(running.ticket.as_ref().unwrap().agent_id(), Some(AGENT));
    drop(runtime);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    due(&running).await;
    let rejected = runtime.step("create", false).await.unwrap();
    assert_eq!(rejected.phase, OperationPhase::Failed);
    assert_eq!(rejected.ticket, running.ticket);
    assert_eq!(rejected.observation, running.observation);
    assert_eq!(mock.requests().len(), 3);
}

#[tokio::test]
async fn learned_conversation_and_model_cannot_drift_between_polls() {
    for field in ["conversation_id", "model"] {
        let mut accepted = run(RUN_A, "pending", Some(AGENT));
        accepted[field] = Value::Null;
        let mut changed = run(RUN_A, "completed", Some(AGENT));
        changed[field] = if field == "model" {
            json!("different-model")
        } else {
            json!(RUN_C)
        };
        let mock = Mock::start(vec![
            Reply::new(202, accepted),
            Reply::observed(RUN_A, "running", Some(AGENT)),
            Reply::new(200, changed),
        ])
        .await;
        let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
        runtime.prepare(execution("run", 1)).unwrap();
        let waiting = runtime.step("run", false).await.unwrap();
        due(&waiting).await;
        let running = runtime.step("run", false).await.unwrap();
        due(&running).await;
        assert_eq!(
            runtime.step("run", false).await.unwrap().phase,
            OperationPhase::Failed
        );
        assert_eq!(mock.requests().len(), 3);
    }
}

#[tokio::test]
async fn readiness_binding_is_immutable_and_expired_history_remains_readable() {
    let mock = Mock::start(vec![]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("readiness.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let spec = readiness(
        "ready-expired",
        chrono::Utc::now() - chrono::Duration::seconds(1),
    );
    let original = runtime.prepare(spec.clone()).unwrap();
    let error = runtime
        .submit_prepared(&spec.operation_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Expired));
    assert!(!format!("{error:?} {error}").contains(READINESS_NONCE));
    assert!(matches!(
        runtime.step(&spec.operation_id, false).await,
        Err(RuntimeError::Expired)
    ));
    assert_eq!(runtime.get(&spec.operation_id).unwrap(), original);
    for field in ["challenge", "config", "expiry", "nonce"] {
        let mut changed = spec.clone();
        match (&mut changed.purpose, field) {
            (OperationPurpose::Readiness { challenge_id, .. }, "challenge") => {
                *challenge_id = "other-challenge".into()
            }
            (OperationPurpose::Readiness { config_digest, .. }, "config") => {
                *config_digest = "c".repeat(64)
            }
            (OperationPurpose::Readiness { expires_at, .. }, "expiry") => {
                *expires_at += chrono::Duration::seconds(1)
            }
            (_, "nonce") => {
                if let OperationRequest::CreateRun(r) = &mut changed.request {
                    r.input["nonce"] = json!("changed");
                }
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(runtime.prepare(changed), Err(RuntimeError::Conflict)),
            "{field}"
        );
    }
    for field in ["challenge", "config", "request_kind"] {
        let mut invalid = spec.clone();
        invalid.operation_id = format!("invalid-{field}");
        match (&mut invalid.purpose, field) {
            (OperationPurpose::Readiness { challenge_id, .. }, "challenge") => {
                *challenge_id = "\n".into()
            }
            (OperationPurpose::Readiness { config_digest, .. }, "config") => {
                *config_digest = "not-a-sha256".into()
            }
            (_, "request_kind") => invalid.request = creation("unused").request,
            _ => unreachable!(),
        }
        assert!(
            matches!(runtime.prepare(invalid), Err(RuntimeError::Invalid)),
            "{field}"
        );
    }
    drop(runtime);
    let reopened = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 0);
    assert_eq!(reopened.get(&spec.operation_id).unwrap(), original);
    assert_eq!(reopened.prepare(spec).unwrap(), original);
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn readiness_submit_only_is_atomic_and_redacted_without_changing_persisted_input() {
    let hold = Arc::new(Semaphore::new(0));
    let mut complete = Reply::observed(RUN_A, "completed", Some(AGENT));
    complete.body["result"] = json!({"echo": READINESS_NONCE});
    complete.body["extra_provider_content"] = json!(READINESS_NONCE);
    let mock = Mock::start(vec![
        Reply::accepted(RUN_A, Some(AGENT)).held(&hold),
        complete,
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("readiness.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let spec = readiness(
        "ready-once",
        chrono::Utc::now() + chrono::Duration::minutes(1),
    );
    let prepared = runtime.prepare(spec.clone()).unwrap();
    let wire = serde_json::to_vec(&prepared).unwrap();
    for output in [
        format!("{:?}", spec.request),
        format!("{spec:?}"),
        format!("{prepared:?}"),
    ] {
        assert!(!output.contains(READINESS_NONCE));
    }
    assert_eq!(serde_json::to_vec(&prepared).unwrap(), wire);
    assert!(String::from_utf8(wire).unwrap().contains(READINESS_NONCE));
    let first = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.submit_prepared("ready-once").await }
    });
    mock.wait_requests(1).await;
    assert_eq!(
        runtime.get("ready-once").unwrap().phase,
        OperationPhase::Submitting
    );
    let second = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.submit_prepared("ready-once").await }
    });
    tokio::task::yield_now().await;
    hold.add_permits(1);
    let submitted = first.await.unwrap().unwrap();
    assert_eq!(submitted.phase, OperationPhase::Waiting);
    assert_eq!(second.await.unwrap().unwrap(), submitted);
    assert_eq!(mock.requests().len(), 1, "submit-only must not poll");
    assert_eq!(mock.requests()[0].0, "POST");
    assert_eq!(mock.requests()[0].1, "/v1/runs");
    assert_eq!(mock.requests()[0].2["input"]["nonce"], READINESS_NONCE);
    drop(runtime);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    assert_eq!(
        runtime.submit_prepared("ready-once").await.unwrap(),
        submitted
    );
    due(&submitted).await;
    let completed = runtime.step("ready-once", false).await.unwrap();
    assert_eq!(completed.phase, OperationPhase::NativeCompleted);
    let observation = completed.observation.as_ref().unwrap();
    assert_eq!(
        observation.result.as_ref().unwrap()["echo"],
        READINESS_NONCE
    );
    assert!(!format!("{observation:?}").contains(READINESS_NONCE));
    assert!(!format!("{completed:?}").contains(READINESS_NONCE));
    let serialized = serde_json::to_vec(&completed).unwrap();
    let restored: OperationRecord = serde_json::from_slice(&serialized).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, completed);
    assert_eq!(
        runtime.submit_prepared("ready-once").await.unwrap(),
        completed
    );
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn provider_model_echo_is_redacted_in_ticket_and_record_without_changing_receipts() {
    let nonce = "0123456789abcdef".repeat(4);
    let mut accepted = Reply::accepted(RUN_A, Some(AGENT));
    accepted.body["model"] = json!(nonce);
    let mut complete = Reply::observed(RUN_A, "completed", Some(AGENT));
    complete.body["model"] = json!(nonce);
    let mock = Mock::start(vec![accepted, complete]).await;
    let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
    let mut spec = readiness(
        "ready-model-echo",
        chrono::Utc::now() + chrono::Duration::minutes(1),
    );
    let OperationRequest::CreateRun(request) = &mut spec.request else {
        unreachable!()
    };
    assert!(request.model.is_none());
    request.input["nonce"] = json!(nonce);
    runtime.prepare(spec).unwrap();
    let submitted = runtime.submit_prepared("ready-model-echo").await.unwrap();
    assert_eq!(submitted.phase, OperationPhase::Waiting);
    due(&submitted).await;
    let completed = runtime.step("ready-model-echo", false).await.unwrap();
    assert_eq!(completed.phase, OperationPhase::NativeCompleted);
    for record in [submitted, completed] {
        record.validate().unwrap();
        let ticket = record.ticket.as_ref().unwrap();
        let ticket_wire = serde_json::to_vec(ticket).unwrap();
        let record_wire = serde_json::to_vec(&record).unwrap();
        assert_eq!(serde_json::to_value(ticket).unwrap()["model"], nonce);
        assert!(!format!("{ticket:?}").contains(&nonce));
        assert!(!format!("{record:?}").contains(&nonce));
        assert_eq!(serde_json::to_vec(ticket).unwrap(), ticket_wire);
        assert_eq!(serde_json::to_vec(&record).unwrap(), record_wire);
        let restored: OperationRecord = serde_json::from_slice(&record_wire).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, record);
        assert!(!format!("{restored:?}").contains(&nonce));
    }
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn cancelled_readiness_submission_is_recovered_without_replaying_or_polling() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT)).held(&hold)]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("readiness.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let spec = readiness(
        "ready-lost",
        chrono::Utc::now() + chrono::Duration::minutes(1),
    );
    runtime.prepare(spec.clone()).unwrap();
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.submit_prepared("ready-lost").await }
    });
    mock.wait_requests(1).await;
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    let interrupted = runtime.get("ready-lost").unwrap();
    assert_eq!(interrupted.phase, OperationPhase::Submitting);
    assert_eq!(
        runtime.submit_prepared("ready-lost").await.unwrap(),
        interrupted
    );
    drop(runtime);
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    let recovered = runtime.get("ready-lost").unwrap();
    assert_eq!(recovered.phase, OperationPhase::NeedsReconciliation);
    assert_eq!(
        runtime.submit_prepared("ready-lost").await.unwrap(),
        recovered
    );
    assert_eq!(runtime.prepare(spec).unwrap(), recovered);
    assert!(matches!(
        runtime.step("ready-lost", false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(mock.requests().len(), 1);
    hold.add_permits(1);
}

fn allow_effect() -> PreEffectGuard {
    Arc::new(|| Box::pin(async { true }))
}

fn assert_guard_rejected(record: &OperationRecord) {
    record.validate().unwrap();
    assert_eq!(record.phase, OperationPhase::Failed);
    assert!(!record.retryable_read);
    assert!(matches!(
        &record.failure,
        Some(RuntimeFailure::Provider(error))
            if error.kind == ErrorKind::GuardRejected && !error.post_may_have_run
    ));
}

#[tokio::test]
async fn guarded_job_rechecks_revocation_after_waiting_for_the_runtime_gate() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT)).held(&hold)]).await;
    let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
    runtime.prepare(execution("gate-holder", 1)).unwrap();
    let second = runtime.prepare(execution("queued-job", 2)).unwrap();
    let first = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.step("gate-holder", false).await }
    });
    mock.wait_requests(1).await;
    let active = Arc::new(AtomicBool::new(true));
    let checks = Arc::new(AtomicUsize::new(0));
    let guard: PreEffectGuard = Arc::new({
        let active = active.clone();
        let checks = checks.clone();
        move || {
            let active = active.clone();
            let checks = checks.clone();
            Box::pin(async move {
                checks.fetch_add(1, Ordering::SeqCst);
                active.load(Ordering::SeqCst)
            })
        }
    });
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.step_guarded("queued-job", false, guard).await }
    });
    tokio::task::yield_now().await;
    assert_eq!(checks.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.get("queued-job").unwrap(), second);
    active.store(false, Ordering::SeqCst);
    hold.add_permits(1);
    assert_eq!(first.await.unwrap().unwrap().phase, OperationPhase::Waiting);
    let denied = pending.await.unwrap().unwrap();
    assert_guard_rejected(&denied);
    assert!(denied.ticket.is_none());
    assert_eq!(checks.load(Ordering::SeqCst), 1);
    assert_eq!(mock.requests().len(), 1, "only the gate holder sent a POST");
    assert_eq!(
        runtime
            .step_guarded("queued-job", false, allow_effect())
            .await
            .unwrap(),
        denied,
        "a later grant does not revive the denied operation"
    );
}

#[tokio::test]
async fn guarded_job_expires_during_account_cooldown_without_claiming_or_posting() {
    let mock = Mock::start(vec![]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("guarded.sqlite");
    let limiter = AccountLimiter::in_memory(ACCOUNT, QuotaLimits::default()).unwrap();
    limiter.defer_for(Duration::from_millis(150)).unwrap();
    let client = CreaoClient::for_local_testing(
        "cr_sk_synthetic_test_only",
        Limits {
            request_timeout: Duration::from_secs(1),
            ..Limits::default()
        },
        limiter,
        &mock.base,
    )
    .unwrap();
    let runtime = Runtime::new(client, RuntimeStore::open(&path, ORG).unwrap(), INTERVAL).unwrap();
    let initial = runtime
        .prepare(execution("expiry-at-admission", 1))
        .unwrap();
    let expires_at = tokio::time::Instant::now() + Duration::from_millis(40);
    let checks = Arc::new(AtomicUsize::new(0));
    let guard: PreEffectGuard = Arc::new({
        let checks = checks.clone();
        move || {
            let checks = checks.clone();
            Box::pin(async move {
                checks.fetch_add(1, Ordering::SeqCst);
                tokio::time::Instant::now() < expires_at
            })
        }
    });
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        async move {
            runtime
                .step_guarded("expiry-at-admission", false, guard)
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(runtime.get("expiry-at-admission").unwrap(), initial);
    assert_eq!(
        checks.load(Ordering::SeqCst),
        0,
        "guard must run after quota admission"
    );
    let rejected = pending.await.unwrap().unwrap();
    assert_guard_rejected(&rejected);
    assert_eq!(checks.load(Ordering::SeqCst), 1);
    assert!(mock.requests().is_empty());
    drop(runtime);
    let reopened = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 0);
    assert_eq!(
        reopened
            .step_guarded("expiry-at-admission", false, allow_effect())
            .await
            .unwrap(),
        rejected
    );
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn cancelling_a_waiting_guard_preserves_prepared_operation_for_safe_resumption() {
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT))]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cancel-guard.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let initial = runtime.prepare(execution("cancelled-guard", 1)).unwrap();
    let entered = Arc::new(Semaphore::new(0));
    let guard: PreEffectGuard = Arc::new({
        let entered = entered.clone();
        move || {
            let entered = entered.clone();
            Box::pin(async move {
                entered.add_permits(1);
                std::future::pending().await
            })
        }
    });
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.step_guarded("cancelled-guard", false, guard).await }
    });
    tokio::time::timeout(Duration::from_secs(1), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert_eq!(runtime.get("cancelled-guard").unwrap(), initial);
    assert!(mock.requests().is_empty());
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    drop(runtime);
    let reopened = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 0);
    assert_eq!(reopened.get("cancelled-guard").unwrap(), initial);
    let posted = reopened
        .step_guarded("cancelled-guard", false, allow_effect())
        .await
        .unwrap();
    assert_eq!(posted.phase, OperationPhase::Waiting);
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn guard_timeout_is_a_definite_local_failure_without_a_post() {
    let mock = Mock::start(vec![]).await;
    let client = CreaoClient::for_local_testing(
        "cr_sk_synthetic_test_only",
        Limits {
            request_timeout: Duration::from_millis(40),
            ..Limits::default()
        },
        AccountLimiter::in_memory(ACCOUNT, QuotaLimits::default()).unwrap(),
        &mock.base,
    )
    .unwrap();
    let runtime = Runtime::new(client, RuntimeStore::in_memory(ORG).unwrap(), INTERVAL).unwrap();
    runtime.prepare(execution("guard-timeout", 1)).unwrap();
    let guard: PreEffectGuard = Arc::new(|| Box::pin(std::future::pending()));
    let failed = runtime
        .step_guarded("guard-timeout", false, guard)
        .await
        .unwrap();
    assert_eq!(failed.phase, OperationPhase::Failed);
    assert!(
        matches!(&failed.failure, Some(RuntimeFailure::Provider(error))
        if error.kind == ErrorKind::DeadlineExceeded && !error.post_may_have_run)
    );
    assert!(failed.ticket.is_none());
    assert!(mock.requests().is_empty());
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
}

#[tokio::test]
async fn denied_read_preserves_the_native_ticket_and_never_sends_get_or_replays_post() {
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT))]).await;
    let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
    runtime.prepare(execution("guarded-read", 1)).unwrap();
    let submitted = runtime
        .step_guarded("guarded-read", false, allow_effect())
        .await
        .unwrap();
    due(&submitted).await;
    let denied = runtime
        .step_guarded(
            "guarded-read",
            false,
            Arc::new(|| Box::pin(async { false })),
        )
        .await
        .unwrap();
    assert_guard_rejected(&denied);
    assert_eq!(denied.ticket, submitted.ticket);
    assert_eq!(mock.requests().len(), 1);
    assert_eq!(
        runtime
            .step_guarded("guarded-read", true, allow_effect())
            .await
            .unwrap(),
        denied
    );
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn guarded_claim_must_commit_before_http_and_storage_failure_remains_prepared() {
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT))]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claim.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let initial = runtime.prepare(execution("claim-failure", 1)).unwrap();
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_guarded_claim BEFORE UPDATE ON creao_operations WHEN NEW.phase='submitting' BEGIN SELECT RAISE(ABORT,'test claim failure'); END;").unwrap();
    assert!(matches!(
        runtime
            .step_guarded("claim-failure", false, allow_effect())
            .await,
        Err(RuntimeError::Storage)
    ));
    assert_eq!(runtime.get("claim-failure").unwrap(), initial);
    assert!(mock.requests().is_empty());
    sql.execute_batch("DROP TRIGGER reject_guarded_claim;")
        .unwrap();
    assert_eq!(
        runtime
            .step_guarded("claim-failure", false, allow_effect())
            .await
            .unwrap()
            .phase,
        OperationPhase::Waiting
    );
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn guarded_post_with_unknown_response_still_requires_reconciliation() {
    let mock = Mock::start(vec![Reply::new(
        503,
        json!({"error":"synthetic ambiguity"}),
    )])
    .await;
    let runtime = mock.runtime(RuntimeStore::in_memory(ORG).unwrap());
    runtime.prepare(execution("guarded-unknown", 1)).unwrap();
    let unknown = runtime
        .step_guarded("guarded-unknown", false, allow_effect())
        .await
        .unwrap();
    assert_eq!(unknown.phase, OperationPhase::NeedsReconciliation);
    assert!(
        matches!(&unknown.failure, Some(RuntimeFailure::Provider(error)) if error.post_may_have_run)
    );
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    assert!(matches!(
        runtime
            .step_guarded("guarded-unknown", false, allow_effect())
            .await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn independent_guarded_handles_claim_at_most_one_post_after_both_guards_allow() {
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT))]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("guard-cas.sqlite");
    let a = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    let b = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    a.prepare(execution("guard-cas", 1)).unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let guard: PreEffectGuard = Arc::new(move || {
        let barrier = barrier.clone();
        Box::pin(async move {
            barrier.wait().await;
            true
        })
    });
    let (a, b) = tokio::join!(
        a.step_guarded("guard-cas", false, guard.clone()),
        b.step_guarded("guard-cas", false, guard)
    );
    assert!(matches!(
        (&a, &b),
        (Ok(_), Err(RuntimeError::ConcurrentChange)) | (Err(RuntimeError::ConcurrentChange), Ok(_))
    ));
    let saved = a.or(b).unwrap();
    assert_eq!(saved.phase, OperationPhase::Waiting);
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn guarded_cancellation_after_post_keeps_unknown_recovery_without_resubmission() {
    let hold = Arc::new(Semaphore::new(0));
    let mock = Mock::start(vec![Reply::accepted(RUN_A, Some(AGENT)).held(&hold)]).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cancel-post.sqlite");
    let runtime = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    runtime.prepare(execution("cancel-post", 1)).unwrap();
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        async move {
            runtime
                .step_guarded("cancel-post", false, allow_effect())
                .await
        }
    });
    mock.wait_requests(1).await;
    assert_eq!(
        runtime.get("cancel-post").unwrap().phase,
        OperationPhase::Submitting
    );
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    drop(runtime);
    let reopened = mock.runtime(RuntimeStore::open(&path, ORG).unwrap());
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 1);
    assert_eq!(
        reopened.get("cancel-post").unwrap().phase,
        OperationPhase::NeedsReconciliation
    );
    assert!(matches!(
        reopened
            .step_guarded("cancel-post", false, allow_effect())
            .await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(mock.requests().len(), 1);
    hold.add_permits(1);
}
