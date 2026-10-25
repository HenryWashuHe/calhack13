//! Published-app Trigger runs through the durable provider boundary. Loopback
//! responses are synthetic; native completion never proves readiness or domain success.
use chrono::Utc;
use genesis_core::{
    engine::RoleToProvision,
    ids::*,
    jobs::Job,
    lifecycle::JobStatus,
    model::{AgentBinding, AuthorityGrant, Provider},
};
use genesis_creao::{
    AccountLimiter, ApiVersion, CreaoClient, Limits, OperationPhase, OperationPurpose,
    OperationRequest, QuotaLimits, Runtime, RuntimeStore,
};
use genesis_providers::{CreaoSpecialist, ProvisionRequest, SpecialistProgress, SpecialistRuntime};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const APP: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const OTHER_APP: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const BOOT_A: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const BOOT_B: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
const RUN_A: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const RUN_B: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

struct Mock {
    url: String,
    seen: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Mock {
    async fn new(replies: Vec<Value>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/api/v1/apps/{APP}/runs",
            listener.local_addr().unwrap()
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        let capture = seen.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    assert!(bytes.len() <= 512 * 1024);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let size = headers
                            .lines()
                            .find_map(|line| {
                                let (k, v) = line.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + size {
                            break;
                        }
                    }
                }
                let is_post = bytes.starts_with(b"POST ");
                capture
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let status = if is_post { "202 Accepted" } else { "200 OK" };
                let body = reply.to_string();
                stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        Self { url, seen, task }
    }
    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn adapter(url: &str, org: &str) -> (CreaoSpecialist, RuntimeStore) {
    let store = RuntimeStore::in_memory(org).unwrap();
    (adapter_from_store(url, store.clone()), store)
}
fn adapter_from_store(url: &str, store: RuntimeStore) -> CreaoSpecialist {
    let client = CreaoClient::for_local_testing_trigger(
        "capi_loopback_only",
        APP,
        Limits {
            request_timeout: Duration::from_secs(1),
            ..Default::default()
        },
        AccountLimiter::in_memory("shared-trigger-account", QuotaLimits::default()).unwrap(),
        url,
    )
    .unwrap();
    CreaoSpecialist::new(Runtime::new(client, store, Duration::from_millis(1)).unwrap())
}
fn role(org: &str, id: &str) -> ProvisionRequest {
    ProvisionRequest {
        org_id: OrgId::new(org),
        change_id: ChangeId::new("change-method"),
        role: RoleToProvision {
            key: id.into(),
            role_id: RoleId::new(id),
            name: "Method reviewer".into(),
            charter: "Review the explicitly assigned method evidence".into(),
            authority: AuthorityGrant::new(["method.review"], 0),
            provider: Provider::Creao,
        },
    }
}
fn binding() -> AgentBinding {
    AgentBinding {
        provider: Provider::Creao,
        provider_agent_id: APP.into(),
        config_digest: "a".repeat(64),
        conversation_ref: Some("never-forward-provisioning-conversation".into()),
    }
}
fn job(org: &str, role: &str) -> Job {
    let now = Utc::now();
    Job {
        job_id: JobId::new("job-one"),
        org_id: OrgId::new(org),
        mission_id: MissionId::new("mission-one"),
        role_id: RoleId::new(role),
        responsibility_id: ResponsibilityId::new("responsibility-one"),
        assignment_id: AssignmentId::new("assignment-one"),
        epoch: 3,
        task: "Review the assigned evidence".into(),
        input: json!({"package":"exact-retained-package"}),
        attempt_id: AttemptId::new("attempt-one"),
        attempt_no: 1,
        correlation_id: CorrelationId::new("correlation-one"),
        idempotency_key: format!("{org}:one"),
        deadline: Some(now + chrono::Duration::hours(1)),
        capability: None,
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
fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}
async fn due(store: &RuntimeStore, op: &str) {
    let delay = (store.get(op).unwrap().next_poll_at.unwrap() - Utc::now())
        .to_std()
        .unwrap_or_default();
    assert!(delay < Duration::from_secs(1));
    tokio::time::sleep(delay + Duration::from_millis(1)).await;
}

#[tokio::test]
async fn bootstrap_binds_existing_app_only_after_native_run_without_readiness() {
    let mock = Mock::new(vec![
        json!({"id":RUN_A,"status":"pending","appId":APP}),
        json!({"id":RUN_A,"status":"running"}),
        json!({"id":RUN_A,"status":"completed","result":{"message":"bootstrap processed"}}),
    ])
    .await;
    let (adapter, store) = adapter(&mock.url, "org-a");
    let request = role("org-a", "role-a");
    let prepared = adapter.prepare_role(&request).unwrap();
    let op = prepared.operation_id();
    assert!(matches!(
        prepared,
        SpecialistProgress::Pending { run_id: None, .. }
    ));
    let record = store.get(op).unwrap();
    assert_eq!(record.spec.api_version, ApiVersion::Trigger);
    assert_eq!(
        record.spec.purpose,
        OperationPurpose::Provision {
            change_id: request.change_id.to_string(),
            config_digest: request.config_digest()
        }
    );
    let OperationRequest::CreateRun(run) = &record.spec.request else {
        panic!("must not create an agent")
    };
    assert_eq!(run.agent_id, APP);
    assert_eq!(run.input["role_contract"], json!(request));
    assert_eq!(
        run.input["context"]["config_digest"],
        request.config_digest()
    );
    assert!(run.conversation_id.is_none() && run.workspace_id.is_none());
    assert!(
        matches!(adapter.advance(op, false).await.unwrap(), SpecialistProgress::Pending { run_id: Some(id), .. } if id == RUN_A)
    );
    due(&store, op).await;
    assert!(matches!(
        adapter.advance(op, false).await.unwrap(),
        SpecialistProgress::Pending { .. }
    ));
    due(&store, op).await;
    assert!(
        matches!(adapter.advance(op, false).await.unwrap(), SpecialistProgress::Created { provider_agent_id, config_digest, readiness_verified: false, .. }
        if provider_agent_id == APP && config_digest == request.config_digest())
    );
    assert_eq!(
        store.get(op).unwrap().phase,
        OperationPhase::NativeCompleted
    );
    adapter.advance(op, false).await.unwrap();
    let seen = mock.requests();
    assert_eq!(seen.len(), 3);
    assert!(seen[0].starts_with(&format!("POST /api/v1/apps/{APP}/runs ")));
    assert!(seen[1].starts_with(&format!("GET /api/v1/apps/{APP}/runs/{RUN_A} ")));
    assert_eq!(body(&seen[0]), json!({"inputs":run.input}));
    assert!(!body(&seen[0]).to_string().contains("capi_"));
}

async fn complete_bootstrap(
    adapter: &CreaoSpecialist,
    store: &RuntimeStore,
    request: &ProvisionRequest,
) -> AgentBinding {
    let op = adapter
        .prepare_role(request)
        .unwrap()
        .operation_id()
        .to_owned();
    adapter.advance(&op, false).await.unwrap();
    due(store, &op).await;
    assert!(matches!(
        adapter.advance(&op, false).await.unwrap(),
        SpecialistProgress::Created {
            readiness_verified: false,
            ..
        }
    ));
    AgentBinding {
        config_digest: request.config_digest(),
        ..binding()
    }
}

#[tokio::test]
async fn trigger_jobs_have_separate_native_receipts_and_self_contained_identity() {
    let mock = Mock::new(vec![
        json!({"id":BOOT_A,"status":"pending","appId":APP}),
        json!({"id":BOOT_A,"status":"completed","result":{}}),
        json!({"id":RUN_A,"status":"pending","appId":APP}),
        json!({"id":RUN_A,"status":"completed","result":{"role":"a"}}),
        json!({"id":BOOT_B,"status":"pending","appId":APP}),
        json!({"id":BOOT_B,"status":"completed","result":{}}),
        json!({"id":RUN_B,"status":"pending","appId":APP}),
        json!({"id":RUN_B,"status":"completed","result":{"role":"b"}}),
    ])
    .await;
    let (a, a_store) = adapter(&mock.url, "org-a");
    let (b, b_store) = adapter(&mock.url, "org-b");
    let a_job = job("org-a", "role-a");
    let b_job = job("org-b", "role-b");
    let a_contract = role("org-a", "role-a");
    let mut b_contract = role("org-b", "role-b");
    b_contract.role.charter = "Independently evaluate method applicability".into();
    let mut receipts = Vec::new();
    for (adapter, store, job, contract) in [
        (&a, &a_store, &a_job, &a_contract),
        (&b, &b_store, &b_job, &b_contract),
    ] {
        let binding = complete_bootstrap(adapter, store, contract).await;
        let op = adapter
            .prepare_job(job, &binding)
            .unwrap()
            .operation_id()
            .to_owned();
        adapter.advance(&op, false).await.unwrap();
        due(store, &op).await;
        assert!(matches!(
            adapter.advance(&op, false).await.unwrap(),
            SpecialistProgress::Collected {
                domain_verified: false,
                ..
            }
        ));
        let receipt = adapter.execution_receipt(job, &binding).unwrap();
        assert_eq!(receipt.org_id, job.org_id);
        assert_eq!(receipt.role_id, job.role_id);
        assert_eq!(
            receipt.request_sha256,
            store.get(&op).unwrap().spec.request_sha256().unwrap()
        );
        assert!(receipt.conversation_id.is_none() && receipt.workspace_id.is_none());
        // A fresh adapter reconstructs exactly the frozen execution and charter.
        let restarted = adapter_from_store(&mock.url, store.clone());
        assert_eq!(restarted.execution_receipt(job, &binding).unwrap(), receipt);
        let mut swapped = job.clone();
        swapped.role_id = RoleId::new("role-other");
        assert!(restarted.execution_receipt(&swapped, &binding).is_err());
        let mut wrong_config = binding.clone();
        wrong_config.config_digest = "b".repeat(64);
        assert!(restarted.prepare_job(job, &wrong_config).is_err());
        receipts.push(receipt);
    }
    assert_ne!(receipts[0].operation_id, receipts[1].operation_id);
    assert_ne!(receipts[0].request_sha256, receipts[1].request_sha256);
    assert_eq!(receipts[0].run_id, RUN_A);
    assert_eq!(receipts[1].run_id, RUN_B);
    let seen = mock.requests();
    assert_eq!(seen.len(), 8);
    for (i, (wire, job, contract)) in [
        (&seen[2], &a_job, &a_contract),
        (&seen[6], &b_job, &b_contract),
    ]
    .into_iter()
    .enumerate()
    {
        let native = body(wire);
        assert_eq!(native.as_object().unwrap().len(), 1);
        assert_eq!(native["inputs"]["context"]["org_id"], json!(job.org_id));
        assert_eq!(native["inputs"]["context"]["role_id"], json!(job.role_id));
        assert_eq!(
            native["inputs"]["context"]["binding_config_digest"],
            contract.config_digest()
        );
        assert_eq!(native["inputs"]["role_contract"], json!(contract));
        assert!(native["inputs"]["provisioning"]["operation_id"]
            .as_str()
            .unwrap()
            .starts_with("creao-role-"));
        assert_eq!(
            receipts[i].request_sha256,
            genesis_verification::json_sha256(&json!({
                "schema_version":3,"api_version":"trigger", "path":format!("/api/v1/apps/{APP}/runs"),"payload":native
            }))
        );
        assert!(!native.to_string().contains("never-forward"));
    }
}

#[tokio::test]
async fn mismatched_app_org_and_rebound_role_are_rejected_before_effects() {
    let mock = Mock::new(vec![
        json!({"id":BOOT_A,"status":"pending","appId":APP}),
        json!({"id":BOOT_A,"status":"completed","result":{}}),
    ])
    .await;
    let (adapter, store) = adapter(&mock.url, "org-a");
    let a_job = job("org-a", "role-a");
    let contract = role("org-a", "role-a");
    let binding = AgentBinding {
        config_digest: contract.config_digest(),
        ..binding()
    };
    let mut bad = binding.clone();
    bad.provider_agent_id = OTHER_APP.into();
    assert!(adapter.prepare_job(&a_job, &bad).is_err());
    assert!(adapter
        .prepare_job(&job("org-b", "role-a"), &binding)
        .is_err());
    assert!(adapter.prepare_role(&role("org-b", "role-a")).is_err());
    assert!(
        adapter.prepare_job(&a_job, &binding).is_err(),
        "missing contract"
    );
    assert!(store.list().unwrap().is_empty());
    adapter.prepare_role(&contract).unwrap();
    assert!(
        adapter.prepare_job(&a_job, &binding).is_err(),
        "uncompleted contract"
    );
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(mock.requests().is_empty());
    complete_bootstrap(&adapter, &store, &contract).await;
    adapter.prepare_job(&a_job, &binding).unwrap();
    let mut changed = a_job;
    changed.role_id = RoleId::new("role-b");
    assert!(adapter.prepare_job(&changed, &binding).is_err());
    assert_eq!(store.list().unwrap().len(), 2);
    assert_eq!(mock.requests().len(), 2, "only bootstrap sent HTTP");
}

#[tokio::test]
async fn later_contract_does_not_change_frozen_job_replay() {
    let mock = Mock::new(vec![
        json!({"id":BOOT_A,"status":"pending","appId":APP}),
        json!({"id":BOOT_A,"status":"completed","result":{}}),
        json!({"id":BOOT_B,"status":"pending","appId":APP}),
        json!({"id":BOOT_B,"status":"completed","result":{}}),
    ])
    .await;
    let (adapter, store) = adapter(&mock.url, "org-a");
    let contract = role("org-a", "role-a");
    let binding = complete_bootstrap(&adapter, &store, &contract).await;
    let job = job("org-a", "role-a");
    let op = adapter
        .prepare_job(&job, &binding)
        .unwrap()
        .operation_id()
        .to_owned();
    let before = store.get(&op).unwrap();
    let mut later = contract.clone();
    later.change_id = ChangeId::new("later-change");
    later.role.charter = "A newer contract must not overwrite a frozen job".into();
    let new_binding = complete_bootstrap(&adapter, &store, &later).await;
    let restarted = adapter_from_store(&mock.url, store.clone());
    assert_eq!(
        restarted
            .prepare_job(&job, &binding)
            .unwrap()
            .operation_id(),
        op
    );
    assert!(restarted.prepare_job(&job, &new_binding).is_err());
    assert_eq!(store.get(&op).unwrap(), before);
    let OperationRequest::CreateRun(run) = before.spec.request else {
        unreachable!()
    };
    assert_eq!(run.input["role_contract"], json!(contract));
    assert_eq!(
        mock.requests().len(),
        4,
        "preparation/replay performs no job POST"
    );
}

#[test]
fn bootstrap_contract_is_frozen_and_roles_do_not_share_local_intent() {
    let (adapter, store) = adapter(
        &format!("http://127.0.0.1:9/api/v1/apps/{APP}/runs"),
        "org-a",
    );
    let a = role("org-a", "role-a");
    let first = adapter.prepare_role(&a).unwrap().operation_id().to_owned();
    assert_eq!(first, adapter.prepare_role(&a).unwrap().operation_id());
    for field in ["charter", "name", "authority", "key"] {
        let mut changed = a.clone();
        match field {
            "charter" => changed.role.charter.push('!'),
            "name" => changed.role.name.push('!'),
            "authority" => {
                changed
                    .role
                    .authority
                    .scopes
                    .insert("method.develop".into());
            }
            "key" => changed.role.key.push('!'),
            _ => unreachable!(),
        }
        assert!(adapter.prepare_role(&changed).is_err(), "{field}");
    }
    let b = role("org-a", "role-b");
    let second = adapter.prepare_role(&b).unwrap().operation_id().to_owned();
    assert_ne!(first, second);
    let a_record = store.get(&first).unwrap();
    let b_record = store.get(&second).unwrap();
    assert_ne!(a_record.fingerprint, b_record.fingerprint);
    assert_eq!(store.list().unwrap().len(), 2);
}
