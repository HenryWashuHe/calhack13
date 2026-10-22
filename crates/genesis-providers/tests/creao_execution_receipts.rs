//! Loopback provider receipts, not live CREAO or domain-success evidence.
//! Every positive receipt below comes from the real adapter's POST/GET journal.
use chrono::Utc;
use genesis_core::ids::*;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::JobStatus;
use genesis_core::model::{AgentBinding, CapabilityRef, Provider};
use genesis_creao::{AccountLimiter, CreaoClient, Limits, QuotaLimits, Runtime, RuntimeStore};
use genesis_providers::{CreaoSpecialist, SpecialistProgress, SpecialistRuntime};
use genesis_verification::json_sha256;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const ORG: &str = "receipt-org";
const ACCOUNT: &str = "loopback-receipt-account";
const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const RUN: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const CONVERSATION: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const PRIVATE_OUTPUT: &str = "PRIVATE_PROVIDER_OUTPUT_MUST_NOT_APPEAR_IN_DEBUG";

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("genesis-receipt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }
    fn open(&self) -> RuntimeStore {
        RuntimeStore::open(self.0.join("journal.sqlite"), ORG).unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Mock {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Mock {
    async fn serve(replies: Vec<(u16, Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let task = tokio::spawn(async move {
            let mut replies: VecDeque<_> = replies.into();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let bytes = tokio::time::timeout(Duration::from_secs(1), async {
                    let mut bytes = Vec::new();
                    loop {
                        let mut buf = [0u8; 4096];
                        let n = stream.read(&mut buf).await.unwrap();
                        assert_ne!(n, 0, "incomplete test HTTP request");
                        bytes.extend_from_slice(&buf[..n]);
                        assert!(bytes.len() <= 512 * 1024);
                        if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            let size = headers
                                .lines()
                                .find_map(|line| {
                                    let (key, value) = line.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if bytes.len() >= end + 4 + size {
                                break bytes;
                            }
                        }
                    }
                })
                .await
                .unwrap();
                received
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let (status, body) = replies
                    .pop_front()
                    .unwrap_or((500, json!({"unexpected_request":true})));
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn specialist(base: &str, store: RuntimeStore, account: &str) -> CreaoSpecialist {
    let client = CreaoClient::for_local_testing(
        "cr_sk_loopback_only",
        Limits {
            request_timeout: Duration::from_secs(1),
            ..Default::default()
        },
        AccountLimiter::in_memory(account, QuotaLimits::default()).unwrap(),
        base,
    )
    .unwrap();
    CreaoSpecialist::new(Runtime::new(client, store, Duration::from_millis(1)).unwrap())
}
fn binding() -> AgentBinding {
    AgentBinding {
        provider: Provider::Creao,
        provider_agent_id: AGENT.into(),
        config_digest: "a".repeat(64),
        conversation_ref: Some("creation-conversation-is-not-execution".into()),
    }
}
fn job() -> Job {
    let now = Utc::now();
    Job {
        job_id: JobId::new("job-monitor"),
        org_id: OrgId::new(ORG),
        mission_id: MissionId::new("mission-venue"),
        role_id: RoleId::new("role-monitor"),
        responsibility_id: ResponsibilityId::new("responsibility-monitor"),
        assignment_id: AssignmentId::new("assignment-monitor"),
        epoch: 7,
        task: "Inspect the changed venue".into(),
        input: json!({"event":"closure","attendees":120}),
        attempt_id: AttemptId::new("attempt-one"),
        attempt_no: 3,
        correlation_id: CorrelationId::new("correlation-monitor"),
        idempotency_key: "receipt-org:monitor-one".into(),
        deadline: Some(now + chrono::Duration::hours(1)),
        capability: Some(CapabilityRef::new("venue-extraction", "1")),
        provider: Some(Provider::Creao),
        status: JobStatus::Queued,
        provider_refs: BTreeMap::new(),
        raw_status: None,
        result: None,
        rejection_reason: None,
        created_at: now,
        updated_at: now,
    }
}
fn response(status: &str, output: Option<Value>) -> Value {
    let mut response = json!({"run_id":RUN,"status":status,"agent_id":AGENT,
        "conversation_id":CONVERSATION,"model":null});
    if let Some(output) = output {
        response["result"] = output;
    }
    response
}
async fn due(store: &RuntimeStore, operation_id: &str) {
    let at = store.get(operation_id).unwrap().next_poll_at.unwrap();
    let wait = (at - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(1);
    assert!(wait < Duration::from_secs(1));
    tokio::time::sleep(wait).await;
}
async fn complete(
    adapter: &CreaoSpecialist,
    store: &RuntimeStore,
    job: &Job,
    binding: &AgentBinding,
) -> String {
    let operation = adapter
        .prepare_job(job, binding)
        .unwrap()
        .operation_id()
        .to_owned();
    assert!(matches!(
        adapter.advance(&operation, false).await.unwrap(),
        SpecialistProgress::Pending {
            run_id: Some(_),
            ..
        }
    ));
    due(store, &operation).await;
    assert!(matches!(
        adapter.advance(&operation, false).await.unwrap(),
        SpecialistProgress::Collected {
            domain_verified: false,
            ..
        }
    ));
    operation
}

#[tokio::test]
async fn completed_receipt_survives_reopen_without_http_and_binds_the_frozen_execution() {
    let output = json!({"text":PRIVATE_OUTPUT,"capacity":120,"passed":true});
    let mock = Mock::serve(vec![
        (202, response("pending", None)),
        (200, response("completed", Some(output.clone()))),
    ])
    .await;
    let database = Database::new();
    let store = database.open();
    let adapter = specialist(&mock.base, store.clone(), ACCOUNT);
    let original = job();
    let original_binding = binding();
    let operation = complete(&adapter, &store, &original, &original_binding).await;
    let checkpoint = store.get(&operation).unwrap();
    let receipt = adapter
        .execution_receipt(&original, &original_binding)
        .unwrap();
    assert_eq!(receipt.schema_version, 1);
    assert_eq!(receipt.provider, Provider::Creao);
    assert_eq!(receipt.org_id, original.org_id);
    assert_eq!(receipt.mission_id, original.mission_id);
    assert_eq!(receipt.role_id, original.role_id);
    assert_eq!(receipt.responsibility_id, original.responsibility_id);
    assert_eq!(receipt.job_id, original.job_id);
    assert_eq!(receipt.attempt_id, original.attempt_id);
    assert_eq!(receipt.attempt_no, original.attempt_no);
    assert_eq!(receipt.assignment_id, original.assignment_id);
    assert_eq!(receipt.epoch, original.epoch);
    assert_eq!(receipt.operation_id, operation);
    assert_eq!(receipt.account_namespace, ACCOUNT);
    assert_eq!(receipt.provider_agent_id, AGENT);
    assert_eq!(
        receipt.binding_config_digest,
        original_binding.config_digest
    );
    assert_eq!(receipt.run_id, RUN);
    assert_eq!(receipt.conversation_id.as_deref(), Some(CONVERSATION));
    assert!(receipt.workspace_id.is_none());
    assert_eq!(
        receipt.request_sha256,
        checkpoint.spec.request.request_sha256().unwrap()
    );
    assert_eq!(
        receipt.operation_fingerprint,
        checkpoint.spec.fingerprint().unwrap()
    );
    assert_eq!(receipt.checkpoint_revision, checkpoint.revision);
    assert_eq!(
        receipt.checkpoint_sha256,
        json_sha256(&json!({
            "domain":"genesis-specialist-execution-checkpoint-v1",
            "record":serde_json::to_value(&checkpoint).unwrap()
        }))
    );
    assert_eq!(receipt.output, Some(output.clone()));
    assert_eq!(receipt.output_sha256, json_sha256(&output));
    assert!(!format!("{receipt:?}").contains(PRIVATE_OUTPUT));
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert_eq!(encoded["output"], output);
    assert!(encoded.get("domain_verified").is_none());
    assert!(encoded.get("readiness_verified").is_none());

    for field in [
        "input",
        "task",
        "mission",
        "responsibility",
        "assignment",
        "epoch",
        "attempt_id",
        "attempt_no",
        "role",
        "job",
        "org",
        "capability",
        "correlation",
        "idempotency",
        "deadline",
        "job_provider",
        "agent",
        "config",
        "binding_provider",
    ] {
        let mut altered = original.clone();
        let mut altered_binding = original_binding.clone();
        match field {
            "input" => altered.input["attendees"] = json!(200),
            "task" => altered.task = "UNTRUSTED_JOB_INPUT_MUST_NOT_LEAK".into(),
            "mission" => altered.mission_id = MissionId::new("other-mission"),
            "responsibility" => {
                altered.responsibility_id = ResponsibilityId::new("other-responsibility")
            }
            "assignment" => altered.assignment_id = AssignmentId::new("other-assignment"),
            "epoch" => altered.epoch += 1,
            "attempt_id" => altered.attempt_id = AttemptId::new("other-attempt"),
            "attempt_no" => altered.attempt_no += 1,
            "role" => altered.role_id = RoleId::new("other-role"),
            "job" => altered.job_id = JobId::new("other-job"),
            "org" => altered.org_id = OrgId::new("other-org"),
            "capability" => altered.capability.as_mut().unwrap().version = "2".into(),
            "correlation" => altered.correlation_id = CorrelationId::new("other-correlation"),
            "idempotency" => altered.idempotency_key = "other-key".into(),
            "deadline" => altered.deadline = None,
            "job_provider" => altered.provider = Some(Provider::Elastic),
            "agent" => {
                altered_binding.provider_agent_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into()
            }
            "config" => altered_binding.config_digest = "b".repeat(64),
            "binding_provider" => altered_binding.provider = Provider::DevStub,
            _ => unreachable!(),
        }
        let error = adapter
            .execution_receipt(&altered, &altered_binding)
            .expect_err(field);
        for message in [error.to_string(), format!("{error:?}")] {
            assert!(!message.contains(PRIVATE_OUTPUT), "{field}");
            assert!(
                !message.contains("UNTRUSTED_JOB_INPUT_MUST_NOT_LEAK"),
                "{field}"
            );
        }
    }
    assert_eq!(store.list().unwrap(), vec![checkpoint.clone()]);
    drop(adapter);
    drop(store);
    let store = database.open();
    let reopened = specialist(&mock.base, store.clone(), ACCOUNT);
    assert_eq!(reopened.recover_interrupted().await.unwrap(), 0);
    let mut decorated = original.clone();
    decorated.status = JobStatus::Succeeded;
    decorated.result = Some(json!({"forged":true}));
    decorated
        .provider_refs
        .insert("run_id".into(), "forged-run".into());
    decorated
        .provider_refs
        .insert("operation_id".into(), "forged-operation".into());
    decorated.raw_status = Some("completed".into());
    decorated.updated_at = Utc::now();
    let restored = reopened
        .execution_receipt(&decorated, &original_binding)
        .unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), encoded);
    let wrong_account = specialist(&mock.base, store.clone(), "different-account");
    assert!(wrong_account
        .execution_receipt(&original, &original_binding)
        .is_err());
    assert_eq!(store.list().unwrap(), vec![checkpoint]);
    let requests = mock.requests();
    assert_eq!(requests.len(), 2, "reading evidence must not POST or GET");
    assert!(requests[0].starts_with("POST /v1/runs "));
    assert!(requests[1].starts_with(&format!("GET /v1/runs/{RUN} ")));
    let sent: Value = serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(sent["input"]["input"], original.input);
    assert_eq!(sent["agent_id"], AGENT);
}

#[tokio::test]
async fn fabricated_generic_success_does_not_create_a_missing_journal_operation() {
    let mock = Mock::serve(vec![]).await;
    let store = RuntimeStore::in_memory(ORG).unwrap();
    let adapter = specialist(&mock.base, store.clone(), ACCOUNT);
    let mut job = job();
    job.status = JobStatus::Succeeded;
    job.result = Some(json!({"passed":true,"output":PRIVATE_OUTPUT}));
    job.raw_status = Some("completed".into());
    job.provider_refs.insert("run_id".into(), RUN.into());
    job.provider_refs.insert("agent_id".into(), AGENT.into());
    assert!(adapter.execution_receipt(&job, &binding()).is_err());
    assert!(store.list().unwrap().is_empty());
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn prepared_pending_failed_and_unknown_provider_states_are_not_receipts() {
    for status in ["pending", "failed", "future-provider-state"] {
        let mock = Mock::serve(vec![
            (202, response("pending", None)),
            (
                200,
                response(status, Some(json!({"passed":true,"text":PRIVATE_OUTPUT}))),
            ),
        ])
        .await;
        let store = RuntimeStore::in_memory(ORG).unwrap();
        let adapter = specialist(&mock.base, store.clone(), ACCOUNT);
        let mut job = job();
        let binding = binding();
        let operation = adapter
            .prepare_job(&job, &binding)
            .unwrap()
            .operation_id()
            .to_owned();
        assert!(adapter.execution_receipt(&job, &binding).is_err());
        assert!(mock.requests().is_empty());
        adapter.advance(&operation, false).await.unwrap();
        assert!(adapter.execution_receipt(&job, &binding).is_err());
        assert_eq!(mock.requests().len(), 1);
        due(&store, &operation).await;
        adapter.advance(&operation, false).await.unwrap();
        let before = store.get(&operation).unwrap();
        job.status = JobStatus::Succeeded;
        job.result = Some(json!({"passed":true}));
        job.provider_refs.insert("run_id".into(), RUN.into());
        assert!(
            adapter.execution_receipt(&job, &binding).is_err(),
            "{status}"
        );
        assert_eq!(store.get(&operation).unwrap(), before);
        assert_eq!(mock.requests().len(), 2);
    }
}

#[tokio::test]
async fn native_completion_without_output_records_provenance_without_inventing_output() {
    let mock = Mock::serve(vec![
        (202, response("pending", None)),
        (200, response("completed", None)),
    ])
    .await;
    let store = RuntimeStore::in_memory(ORG).unwrap();
    let adapter = specialist(&mock.base, store.clone(), ACCOUNT);
    let job = job();
    let binding = binding();
    complete(&adapter, &store, &job, &binding).await;
    let receipt = adapter.execution_receipt(&job, &binding).unwrap();
    assert!(receipt.output.is_none());
    assert_eq!(receipt.output_sha256, json_sha256(&Value::Null));
    assert_eq!(mock.requests().len(), 2);
}
