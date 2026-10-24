//! Independent PR #104 probes. All HTTP is synthetic loopback traffic.
use axum::{extract::Request, routing::any, Json, Router};
use genesis_creao::{
    AccountLimiter, ApiVersion, CreaoClient, CreateAgentRequest, Error, ErrorKind, Limits,
    OperationPhase, OperationPurpose, OperationRequest, OperationSpec, QuotaLimits, Runtime,
    RuntimeError, RuntimeFailure, RuntimeStore, Stage,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

const RUN: &str = "10000000-0000-4000-8000-000000000001";
const AGENT: &str = "20000000-0000-4000-8000-000000000002";
const OTHER: &str = "30000000-0000-4000-8000-000000000003";

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn new(replies: Vec<(u16, Value)>) -> Self {
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let router = Router::new().route(
            "/{*path}",
            any(move |request: Request| {
                let replies = replies.clone();
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(format!(
                        "{} {}",
                        request.method(),
                        request.uri().path()
                    ));
                    let (status, body) = replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("unexpected HTTP");
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        Json(body),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v2", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            url,
            requests,
            task,
        }
    }
    fn runtime(&self, store: RuntimeStore, account: &str, version: ApiVersion) -> Runtime {
        let limiter = AccountLimiter::in_memory(account, QuotaLimits::default()).unwrap();
        let constructor = if version == ApiVersion::V2 {
            CreaoClient::for_local_testing_v2
        } else {
            CreaoClient::for_local_testing
        };
        let client = constructor(
            "cr_sk_independent_fixture_only",
            Limits::default(),
            limiter,
            &self.url.replace(
                "/v2",
                if version == ApiVersion::V2 {
                    "/v2"
                } else {
                    "/v1"
                },
            ),
        )
        .unwrap();
        Runtime::new(client, store, Duration::from_secs(3600)).unwrap()
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn spec() -> OperationSpec {
    OperationSpec {
        schema_version: 1,
        api_version: ApiVersion::V2,
        operation_id: "create".into(),
        org_id: "org".into(),
        role_id: "role".into(),
        account_namespace: "account".into(),
        purpose: OperationPurpose::Provision {
            change_id: "change".into(),
            config_digest: "a".repeat(64),
        },
        request: OperationRequest::CreateAgent(CreateAgentRequest {
            input: "Synthetic independent specialist".into(),
            model: None,
            conversation_id: None,
        }),
    }
}
fn accepted() -> (u16, Value) {
    (
        202,
        json!({"run_id":RUN,"agent_id":null,"status":"pending","model":null,"conversation_id":null}),
    )
}
fn observed(status: &str) -> (u16, Value) {
    (200, json!({"run_id":RUN,"agent_id":AGENT,"status":status}))
}

#[tokio::test]
async fn reads_and_failed_commit_preserve_exact_ticket_until_success() {
    let fixture = Fixture::new(vec![
        accepted(),
        (503, json!({"error":"synthetic"})),
        (
            200,
            json!({"run_id":OTHER,"agent_id":AGENT,"status":"succeeded"}),
        ),
        (
            200,
            json!({"run_id":RUN,"agent_id":OTHER,"status":"succeeded"}),
        ),
        (
            200,
            json!({"run_id":RUN,"agent_id":null,"status":"succeeded"}),
        ),
        observed("succeeded"),
        observed("succeeded"),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("org.sqlite");
    let store = RuntimeStore::open(&path, "org").unwrap();
    let runtime = fixture.runtime(store.clone(), "account", ApiVersion::V2);
    runtime.prepare(spec()).unwrap();
    let before = runtime.step("create", false).await.unwrap();
    for (account, version) in [("other", ApiVersion::V2), ("account", ApiVersion::V1)] {
        let foreign = fixture.runtime(store.clone(), account, version);
        assert!(matches!(
            foreign.reconcile_creation_agent("create", AGENT).await,
            Err(RuntimeError::ContextMismatch)
        ));
    }
    for candidate in [
        "",
        " invalid ",
        "../agents/elsewhere",
        "20000000-0000-4000-8000-00000000000G",
    ] {
        assert!(matches!(
            runtime.reconcile_creation_agent("create", candidate).await,
            Err(RuntimeError::Invalid)
        ));
    }
    let mut changed = serde_json::to_value(&before).unwrap();
    changed["ticket"]["request_sha256"] = json!("0".repeat(64));
    assert!(
        serde_json::from_value::<genesis_creao::OperationRecord>(changed)
            .unwrap()
            .validate()
            .is_err()
    );
    assert_eq!(fixture.count(), 1);
    for _ in 0..4 {
        assert!(matches!(
            runtime.reconcile_creation_agent("create", AGENT).await,
            Err(RuntimeError::NeedsReconciliation)
        ));
        assert_eq!(runtime.get("create").unwrap(), before);
    }
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER deny_binding BEFORE UPDATE ON creao_operations BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(matches!(
        runtime.reconcile_creation_agent("create", AGENT).await,
        Err(RuntimeError::Storage)
    ));
    assert_eq!(runtime.get("create").unwrap(), before);
    sql.execute_batch("DROP TRIGGER deny_binding;").unwrap();
    drop(runtime);
    drop(store);
    let reopened = fixture.runtime(
        RuntimeStore::open(&path, "org").unwrap(),
        "account",
        ApiVersion::V2,
    );
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 0);
    assert!(matches!(
        reopened.step("create", false).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    let after = reopened
        .reconcile_creation_agent("create", AGENT)
        .await
        .unwrap();
    assert_eq!(after.phase, OperationPhase::NativeCompleted);
    assert_eq!(after.ticket.as_ref().unwrap().run_id(), RUN);
    assert_eq!(after.ticket.as_ref().unwrap().agent_id(), Some(AGENT));
    assert_eq!(fixture.count(), 7);
    assert!(fixture.requests.lock().unwrap()[1..]
        .iter()
        .all(|p| p == &format!("GET /v2/agents/{AGENT}/edits/{RUN}")));
}

#[tokio::test]
async fn reconciled_native_status_is_retained_without_completion_inference_or_replay() {
    for (status, expected) in [
        ("queued", OperationPhase::Waiting),
        ("running", OperationPhase::Waiting),
        ("succeeded", OperationPhase::NativeCompleted),
        ("failed", OperationPhase::Failed),
        ("cancelled", OperationPhase::Failed),
        ("future_status", OperationPhase::NeedsReconciliation),
    ] {
        let fixture = Fixture::new(vec![accepted(), observed(status)]).await;
        let runtime = fixture.runtime(
            RuntimeStore::in_memory("org").unwrap(),
            "account",
            ApiVersion::V2,
        );
        runtime.prepare(spec()).unwrap();
        runtime.step("create", false).await.unwrap();
        let record = runtime
            .reconcile_creation_agent("create", AGENT)
            .await
            .unwrap();
        record.validate().unwrap();
        assert_eq!(record.phase, expected);
        assert_eq!(runtime.get("create").unwrap(), record);
        assert!(matches!(
            runtime.reconcile_creation_agent("create", OTHER).await,
            Err(RuntimeError::NeedsReconciliation)
        ));
        let next = runtime.step("create", false).await;
        if expected == OperationPhase::NeedsReconciliation {
            assert!(matches!(next, Err(RuntimeError::NeedsReconciliation)));
        } else {
            assert_eq!(next.unwrap(), record);
        }
        assert_eq!(fixture.count(), 2);
    }
}

#[tokio::test]
async fn old_malformed_checkpoint_remains_inspectable_and_cannot_be_recovered_by_guessing() {
    let fixture = Fixture::new(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    let store = RuntimeStore::open(&path, "org").unwrap();
    let mut old = store.prepare(spec()).unwrap();
    old.phase = OperationPhase::NeedsReconciliation;
    old.failure = Some(RuntimeFailure::Provider(Error {
        kind: ErrorKind::MalformedResponse,
        stage: Stage::Submission,
        http_status: None,
        post_may_have_run: true,
        retry_after_secs: None,
    }));
    old.revision = 2;
    old.validate().unwrap();
    let encoded = serde_json::to_string(&old).unwrap();
    Connection::open(&path).unwrap().execute(
        "UPDATE creao_operations SET phase='needs_reconciliation', revision=2, record_json=?1 WHERE operation_id='create'",
        params![encoded]).unwrap();
    let runtime = fixture.runtime(store, "account", ApiVersion::V2);
    assert_eq!(runtime.get("create").unwrap(), old);
    assert!(matches!(
        runtime.reconcile_creation_agent("create", AGENT).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert!(matches!(
        runtime.step("create", true).await,
        Err(RuntimeError::NeedsReconciliation)
    ));
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    let after: String = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT record_json FROM creao_operations WHERE operation_id='create'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, encoded);
    assert_eq!(fixture.count(), 0);
}
