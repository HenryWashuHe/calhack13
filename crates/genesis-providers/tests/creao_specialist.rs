//! Native client + durable SQLite adapter tests. Provider completion remains
//! separate from readiness, organization authorization, and domain verification.
use chrono::Utc;
use genesis_core::engine::RoleToProvision;
use genesis_core::ids::*;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::JobStatus;
use genesis_core::model::{AgentBinding, AuthorityGrant, CapabilityRef, Provider};
use genesis_creao::{
    AccountLimiter, CreaoClient, Limits, OperationPhase, OperationPurpose, OperationRequest,
    OperationSpec, QuotaLimits, RunRequest, Runtime, RuntimeStore,
};
use genesis_providers::{
    CreaoSpecialist, DevStub, ProviderError, ProviderSet, ProvisionRequest, SpecialistProgress,
    SpecialistRuntime,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const RUN_A: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const RUN_B: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const CREATION_CONVERSATION: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

struct Reply {
    status: u16,
    body: Value,
}
struct Mock {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Mock {
    async fn serve(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let size = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + size {
                            break;
                        }
                    }
                    assert!(bytes.len() <= 512 * 1024);
                }
                received
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let body = reply.body.to_string();
                let response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.status, body.len(), body);
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    async fn finish(self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

fn native_runtime(base: &str) -> (Runtime, RuntimeStore) {
    let store = RuntimeStore::in_memory("org-a").unwrap();
    let client = CreaoClient::for_local_testing(
        "cr_sk_local_test_only",
        Limits {
            request_timeout: Duration::from_secs(1),
            ..Default::default()
        },
        AccountLimiter::in_memory("shared-account", QuotaLimits::default()).unwrap(),
        base,
    )
    .unwrap();
    let runtime = Runtime::new(client, store.clone(), Duration::from_millis(1)).unwrap();
    (runtime, store)
}
fn specialist(base: &str) -> (CreaoSpecialist, RuntimeStore) {
    let (runtime, store) = native_runtime(base);
    (CreaoSpecialist::new(runtime), store)
}
fn local() -> (CreaoSpecialist, RuntimeStore) {
    specialist("http://127.0.0.1:9/v1")
}
fn role() -> ProvisionRequest {
    ProvisionRequest {
        org_id: OrgId::new("org-a"),
        change_id: ChangeId::new("change-monitor"),
        role: RoleToProvision {
            key: "venue-monitor".into(),
            role_id: RoleId::new("role-monitor"),
            name: "Venue monitor".into(),
            charter: "Handle continuing venue changes".into(),
            authority: AuthorityGrant::new(["capability.venue_lookup"], 0),
            provider: Provider::Creao,
        },
    }
}
fn binding() -> AgentBinding {
    AgentBinding {
        provider: Provider::Creao,
        provider_agent_id: AGENT.into(),
        config_digest: "a".repeat(64),
        conversation_ref: Some(CREATION_CONVERSATION.into()),
    }
}
fn job() -> Job {
    let now = Utc::now();
    Job {
        job_id: JobId::new("job-event-one"),
        org_id: OrgId::new("org-a"),
        mission_id: MissionId::new("mission-event"),
        role_id: RoleId::new("role-monitor"),
        responsibility_id: ResponsibilityId::new("responsibility-monitor"),
        assignment_id: AssignmentId::new("assignment-monitor"),
        epoch: 7,
        task: "Check the changed venue".into(),
        input: json!({"event":"venue_closed","attendees":120}),
        attempt_id: AttemptId::new("attempt-one"),
        attempt_no: 3,
        correlation_id: CorrelationId::new("correlation-event"),
        idempotency_key: "org-a:event-one".into(),
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
fn response(run: &str, agent: Option<&str>, status: &str, result: Option<Value>) -> Value {
    let mut value =
        json!({"run_id":run,"status":status,"agent_id":agent,"conversation_id":null,"model":null});
    if let Some(result) = result {
        value["result"] = result;
    }
    value
}
async fn due(store: &RuntimeStore, operation_id: &str) {
    let at = store.get(operation_id).unwrap().next_poll_at.unwrap();
    let wait = (at - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(1);
    tokio::time::sleep(wait).await;
}
fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn role_intent_is_idempotent_and_binds_complete_contract_without_http() {
    let (adapter, store) = local();
    let request = role();
    let first = adapter.prepare_role(&request).unwrap();
    assert!(matches!(
        first,
        SpecialistProgress::Pending { run_id: None, .. }
    ));
    assert_eq!(
        first.operation_id(),
        adapter.prepare_role(&request).unwrap().operation_id()
    );
    let record = store.get(first.operation_id()).unwrap();
    assert_eq!(record.spec.org_id, request.org_id.as_str());
    assert_eq!(record.spec.role_id, request.role.role_id.as_str());
    assert_eq!(
        record.spec.purpose,
        OperationPurpose::Provision {
            change_id: request.change_id.to_string(),
            config_digest: request.config_digest()
        }
    );
    let OperationRequest::CreateAgent(native) = record.spec.request else {
        panic!("wrong native operation");
    };
    assert!(native
        .input
        .contains(&serde_json::to_string(&request).unwrap()));
    assert!(native.conversation_id.is_none());
    assert!(native.input.contains("GENESIS readiness gateway skill"));
    assert!(native.input.contains("challenge-bound readiness receipt"));
    assert!(native.input.contains("never grants mission authority"));
    assert!(native
        .input
        .contains("Require an explicit GENESIS job and scoped grant for all mission work"));
    assert!(native.input.contains(
        "Do not claim the skill is installed or readiness is proven without returned evidence"
    ));
    for field in ["charter", "name", "authority", "key"] {
        let mut changed = request.clone();
        match field {
            "charter" => changed.role.charter = "Different authority claim".into(),
            "name" => changed.role.name = "Different specialist".into(),
            "authority" => {
                changed.role.authority.scopes.insert("ledger.commit".into());
            }
            "key" => changed.role.key = "replacement-key".into(),
            _ => unreachable!(),
        }
        assert!(adapter.prepare_role(&changed).is_err(), "{field}");
    }
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn wrong_role_organization_and_provider_are_rejected_before_local_creation() {
    let (adapter, store) = local();
    for bad in ["org", "provider"] {
        let mut request = role();
        if bad == "org" {
            request.org_id = OrgId::new("org-b");
        } else {
            request.role.provider = Provider::Elastic;
        }
        assert!(matches!(
            adapter.prepare_role(&request),
            Err(ProviderError::Rejected(_))
        ));
    }
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn job_intent_preserves_assignment_attempt_capability_and_has_no_creation_conversation() {
    let (adapter, store) = local();
    let job = job();
    let binding = binding();
    let progress = adapter.prepare_job(&job, &binding).unwrap();
    assert_eq!(
        progress.operation_id(),
        adapter.prepare_job(&job, &binding).unwrap().operation_id()
    );
    let record = store.get(progress.operation_id()).unwrap();
    assert_eq!(
        record.spec.purpose,
        OperationPurpose::Execution {
            job_id: job.job_id.to_string(),
            assignment_id: job.assignment_id.to_string(),
            epoch: job.epoch
        }
    );
    let OperationRequest::CreateRun(native) = record.spec.request else {
        panic!("wrong native operation");
    };
    assert_eq!(native.agent_id, binding.provider_agent_id);
    assert!(native.conversation_id.is_none());
    assert!(native.workspace_id.is_none());
    assert_eq!(native.input["task"], job.task);
    assert_eq!(native.input["input"], job.input);
    let context = &native.input["context"];
    assert_eq!(context["org_id"], json!(job.org_id));
    assert_eq!(context["mission_id"], json!(job.mission_id));
    assert_eq!(context["role_id"], json!(job.role_id));
    assert_eq!(context["responsibility_id"], json!(job.responsibility_id));
    assert_eq!(context["assignment_id"], json!(job.assignment_id));
    assert_eq!(context["epoch"], job.epoch);
    assert_eq!(context["job_id"], json!(job.job_id));
    assert_eq!(context["attempt_id"], json!(job.attempt_id));
    assert_eq!(context["attempt_no"], job.attempt_no);
    assert_eq!(context["binding_config_digest"], binding.config_digest);
    assert_eq!(context["correlation_id"], json!(job.correlation_id));
    assert_eq!(context["idempotency_key"], job.idempotency_key);
    assert_eq!(context["capability"], json!(job.capability));
    assert_eq!(context["deadline"], json!(job.deadline));
}

#[test]
fn same_job_attempt_cannot_be_rebound_to_changed_execution_context() {
    let (adapter, store) = local();
    let original = job();
    let binding = binding();
    adapter.prepare_job(&original, &binding).unwrap();
    for field in [
        "assignment",
        "epoch",
        "role",
        "responsibility",
        "mission",
        "task",
        "input",
        "capability",
        "attempt_no",
        "correlation",
        "idempotency",
        "deadline",
    ] {
        let mut changed = original.clone();
        match field {
            "assignment" => changed.assignment_id = AssignmentId::new("new-assignment"),
            "epoch" => changed.epoch += 1,
            "role" => changed.role_id = RoleId::new("new-role"),
            "responsibility" => {
                changed.responsibility_id = ResponsibilityId::new("new-responsibility")
            }
            "mission" => changed.mission_id = MissionId::new("new-mission"),
            "task" => changed.task = "Different task".into(),
            "input" => changed.input["attendees"] = json!(200),
            "capability" => changed.capability.as_mut().unwrap().version = "2".into(),
            "attempt_no" => changed.attempt_no += 1,
            "correlation" => changed.correlation_id = CorrelationId::new("new-correlation"),
            "idempotency" => changed.idempotency_key = "different-key".into(),
            "deadline" => changed.deadline = None,
            _ => unreachable!(),
        }
        assert!(adapter.prepare_job(&changed, &binding).is_err(), "{field}");
    }
    let mut different_config = binding.clone();
    different_config.config_digest = "b".repeat(64);
    assert!(adapter.prepare_job(&original, &different_config).is_err());
    let mut different_agent = binding.clone();
    different_agent.provider_agent_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into();
    assert!(adapter.prepare_job(&original, &different_agent).is_err());
    let mut nonsemantic = original.clone();
    nonsemantic.created_at = Utc::now();
    nonsemantic.updated_at = Utc::now();
    let mut ignored_creation_conversation = binding.clone();
    ignored_creation_conversation.conversation_ref = Some("another-creation-conversation".into());
    assert!(adapter
        .prepare_job(&nonsemantic, &ignored_creation_conversation)
        .is_ok());
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn dispatch_requires_queued_unexpired_job_and_matching_provider_and_org() {
    let (adapter, store) = local();
    for field in [
        "org",
        "job_provider",
        "binding_provider",
        "running",
        "succeeded",
        "expired",
        "invalid_agent",
    ] {
        let mut job = job();
        let mut binding = binding();
        match field {
            "org" => job.org_id = OrgId::new("org-b"),
            "job_provider" => job.provider = Some(Provider::Elastic),
            "binding_provider" => binding.provider = Provider::DevStub,
            "running" => job.status = JobStatus::Running,
            "succeeded" => job.status = JobStatus::Succeeded,
            "expired" => job.deadline = Some(Utc::now() - chrono::Duration::seconds(1)),
            "invalid_agent" => binding.provider_agent_id = "not-an-agent-uuid".into(),
            _ => unreachable!(),
        }
        assert!(adapter.prepare_job(&job, &binding).is_err(), "{field}");
    }
    assert!(store.list().unwrap().is_empty());
}

#[tokio::test]
async fn completed_creation_is_a_durable_agent_identity_without_readiness_authority() {
    let mock = Mock::serve(vec![
        Reply {
            status: 202,
            body: response(RUN_A, None, "pending", None),
        },
        Reply {
            status: 200,
            body: response(RUN_A, None, "completed", Some(json!({"agent_id":AGENT}))),
        },
    ])
    .await;
    let (adapter, store) = specialist(&mock.base);
    let request = role();
    let operation = adapter
        .prepare_role(&request)
        .unwrap()
        .operation_id()
        .to_owned();
    let submitted = adapter.advance(&operation, false).await.unwrap();
    assert!(
        matches!(submitted, SpecialistProgress::Pending { run_id:Some(ref id), .. } if id == RUN_A)
    );
    let saved = store.get(&operation).unwrap();
    assert_eq!(saved.ticket.unwrap().run_id(), RUN_A);
    due(&store, &operation).await;
    let created = adapter.advance(&operation, false).await.unwrap();
    assert!(
        matches!(created, SpecialistProgress::Created { provider_agent_id, config_digest, readiness_verified:false, .. }
        if provider_agent_id == AGENT && config_digest == request.config_digest())
    );
    assert!(matches!(
        adapter.get(&operation).unwrap(),
        SpecialistProgress::Created {
            readiness_verified: false,
            ..
        }
    ));
    assert!(matches!(
        adapter.advance(&operation, false).await.unwrap(),
        SpecialistProgress::Created {
            readiness_verified: false,
            ..
        }
    ));
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /v1/agents "));
    assert!(requests[1].starts_with(&format!("GET /v1/runs/{RUN_A} ")));
}

#[tokio::test]
async fn same_persistent_agent_handles_two_events_as_separate_unverified_runs() {
    let mock = Mock::serve(vec![
        Reply {
            status: 202,
            body: response(RUN_A, Some(AGENT), "pending", None),
        },
        Reply {
            status: 200,
            body: response(
                RUN_A,
                Some(AGENT),
                "completed",
                Some(json!({"text":"First event processed","passed":true})),
            ),
        },
        Reply {
            status: 202,
            body: response(RUN_B, Some(AGENT), "pending", None),
        },
        Reply {
            status: 200,
            body: response(
                RUN_B,
                Some(AGENT),
                "completed",
                Some(json!({"text":"Second event processed","passed":true})),
            ),
        },
    ])
    .await;
    let (adapter, store) = specialist(&mock.base);
    let first = job();
    let mut second = first.clone();
    second.job_id = JobId::new("job-event-two");
    second.attempt_id = AttemptId::new("attempt-two");
    second.idempotency_key = "org-a:event-two".into();
    second.input["event"] = json!("venue_reopened");
    let binding = binding();
    let one = adapter
        .prepare_job(&first, &binding)
        .unwrap()
        .operation_id()
        .to_owned();
    let two = adapter
        .prepare_job(&second, &binding)
        .unwrap()
        .operation_id()
        .to_owned();
    assert_ne!(one, two);
    for operation in [&one, &two] {
        assert!(matches!(
            adapter.advance(operation, false).await.unwrap(),
            SpecialistProgress::Pending {
                run_id: Some(_),
                ..
            }
        ));
        due(&store, operation).await;
        assert!(
            matches!(adapter.advance(operation, false).await.unwrap(), SpecialistProgress::Collected { provider_agent_id, domain_verified:false, output:Some(_), .. } if provider_agent_id == AGENT)
        );
    }
    assert_eq!(store.list().unwrap().len(), 2);
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    for index in [0, 2] {
        assert!(requests[index].starts_with("POST /v1/runs "));
        let body = body(&requests[index]);
        assert_eq!(body["agent_id"], AGENT);
        assert_eq!(body["should_notify"], false);
        assert!(body.get("conversation_id").is_none());
        assert!(body.get("workspace_id").is_none());
        assert!(!requests[index].contains(CREATION_CONVERSATION));
    }
    assert_ne!(
        body(&requests[0])["input"]["context"]["job_id"],
        body(&requests[2])["input"]["context"]["job_id"]
    );
}

#[test]
fn registered_specialist_does_not_claim_synchronous_readiness_or_implicit_stub_support() {
    let (adapter, _) = local();
    let providers = ProviderSet::new().with_specialist(Arc::new(adapter));
    assert_eq!(providers.configured_specialists(), vec![Provider::Creao]);
    assert_eq!(
        providers.specialist(Provider::Creao).unwrap().org_id(),
        "org-a"
    );
    assert!(matches!(
        providers.provisioner(Provider::Creao),
        Err(ProviderError::NotConfigured {
            provider: Provider::Creao
        })
    ));
    let stub_only = ProviderSet::new().with_dev_stub(DevStub::default());
    assert!(stub_only.configured_specialists().is_empty());
    assert!(matches!(
        stub_only.specialist(Provider::Creao),
        Err(ProviderError::NotConfigured {
            provider: Provider::Creao
        })
    ));
}

#[tokio::test]
async fn readiness_records_are_never_exposed_or_driven_as_role_or_job_progress() {
    const NONCE: &str = "READINESS_SECRET_NOT_FOR_SPECIALIST_PROGRESS";
    for (status, phase) in [
        ("completed", OperationPhase::NativeCompleted),
        ("failed", OperationPhase::Failed),
        ("future_status", OperationPhase::NeedsReconciliation),
    ] {
        let mock = Mock::serve(vec![
            Reply {
                status: 202,
                body: response(RUN_A, Some(AGENT), "pending", None),
            },
            Reply {
                status: 200,
                body: response(RUN_A, Some(AGENT), status, Some(json!({"echo": NONCE}))),
            },
        ])
        .await;
        let (runtime, store) = native_runtime(&mock.base);
        let specialist = CreaoSpecialist::new(runtime.clone());
        runtime
            .prepare(OperationSpec {
                schema_version: 1,
                operation_id: "readiness-only".into(),
                org_id: "org-a".into(),
                role_id: "role-monitor".into(),
                account_namespace: "shared-account".into(),
                purpose: OperationPurpose::Readiness {
                    challenge_id: "rc-test".into(),
                    config_digest: "a".repeat(64),
                    expires_at: Utc::now() + chrono::Duration::minutes(1),
                },
                request: OperationRequest::CreateRun(RunRequest {
                    agent_id: AGENT.into(),
                    input: json!({"nonce": NONCE}),
                    model: None,
                    conversation_id: None,
                    workspace_id: None,
                }),
            })
            .unwrap();
        for stage in [OperationPhase::Prepared, OperationPhase::Waiting, phase] {
            let saved = store.get("readiness-only").unwrap();
            assert_eq!(saved.phase, stage);
            let before_requests = mock.requests.lock().unwrap().len();
            for result in [
                specialist.get("readiness-only"),
                specialist.advance("readiness-only", false).await,
            ] {
                let error = result.unwrap_err();
                assert!(matches!(error, ProviderError::Rejected(_)));
                assert!(!format!("{error:?} {error}").contains(NONCE));
            }
            assert_eq!(
                store.get("readiness-only").unwrap(),
                saved,
                "generic adapter must not mutate readiness"
            );
            assert_eq!(
                mock.requests.lock().unwrap().len(),
                before_requests,
                "generic adapter must not perform readiness I/O"
            );
            if stage == OperationPhase::Prepared {
                runtime.submit_prepared("readiness-only").await.unwrap();
            } else if stage == OperationPhase::Waiting {
                due(&store, "readiness-only").await;
                runtime.step("readiness-only", false).await.unwrap();
            }
        }
        let requests = mock.finish().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("POST /v1/runs "));
        assert!(requests[1].starts_with(&format!("GET /v1/runs/{RUN_A} ")));
    }
}
