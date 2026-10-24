//! Real CREAO HTTP client and kernel gateway routes against loopback fixtures.
//! These tests prove durable protocol behavior, not live CREAO skill execution.

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{Response, StatusCode},
    routing::any,
    Router,
};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_core::{
    engine::RoleToProvision,
    ids::{OrgId, RoleId},
    model::{AgentBinding, AuthorityGrant, Provider},
};
use genesis_creao::{
    AccountLimiter, CreaoClient, Limits, OperationPhase, OperationPurpose, OperationRequest,
    QuotaLimits, Runtime, RuntimeStore,
};
use genesis_providers::ProviderError;
use genesis_server::{
    app::AppState,
    build_org,
    creao_readiness::CreaoReadinessProbe,
    gateway_readiness::{
        GatewayConfig, GatewayReadiness, ProbeRequest, ReadinessProbe, RECEIPT_PATH,
    },
    specialist::{Readiness, ReadinessVerifier},
    OrgConfig,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};

const ORG: &str = "org_probe";
const ACCOUNT: &str = "synthetic-readiness-account";
const TOKEN: &str = "cr_sk_loopback_readiness_only";
const AGENT: &str = "00000000-0000-4000-8000-000000000001";
const OTHER_AGENT: &str = "00000000-0000-4000-8000-000000000002";
const RUN: &str = "00000000-0000-4000-8000-000000000003";
const CONVERSATION: &str = "00000000-0000-4000-8000-000000000004";
const INTERVAL: Duration = Duration::from_millis(1);

struct Reply {
    status: u16,
    body: Value,
    hold: Option<Arc<Semaphore>>,
}

impl Reply {
    fn run(status: u16, native: &str) -> Self {
        Self {
            status,
            body: json!({"run_id":RUN,"status":native,"agent_id":AGENT,
                "conversation_id":CONVERSATION,"model":"synthetic-model"}),
            hold: None,
        }
    }

    fn ambiguous() -> Self {
        Self {
            status: 503,
            body: json!({"error":"PRIVATE_PROVIDER_BODY_CANARY"}),
            hold: None,
        }
    }

    fn held(mut self, hold: &Arc<Semaphore>) -> Self {
        self.hold = Some(hold.clone());
        self
    }
}

#[derive(Clone)]
struct Seen {
    method: String,
    path: String,
    body: Value,
}

#[derive(Clone)]
struct ProviderState {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
    entered: Arc<Semaphore>,
}

struct ProviderFixture {
    state: ProviderState,
    client: CreaoClient,
    server: JoinHandle<()>,
}

impl ProviderFixture {
    async fn new(replies: Vec<Reply>) -> Self {
        let state = ProviderState {
            replies: Arc::new(Mutex::new(replies.into())),
            seen: Arc::new(Mutex::new(vec![])),
            entered: Arc::new(Semaphore::new(0)),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/{*path}", any(provider_response))
            .with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = CreaoClient::for_local_testing(
            TOKEN,
            Limits {
                request_timeout: Duration::from_secs(3),
                ..Limits::default()
            },
            AccountLimiter::in_memory(ACCOUNT, QuotaLimits::default()).unwrap(),
            &endpoint,
        )
        .unwrap();
        Self {
            state,
            client,
            server,
        }
    }

    fn runtime(&self, path: &Path) -> (Runtime, RuntimeStore) {
        let store = RuntimeStore::open(path, ORG).unwrap();
        let runtime = Runtime::new(self.client.clone(), store.clone(), INTERVAL).unwrap();
        (runtime, store)
    }

    fn seen(&self) -> Vec<Seen> {
        self.state.seen.lock().unwrap().clone()
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.state.entered.acquire())
            .await
            .expect("loopback provider request reached handler")
            .unwrap()
            .forget();
    }
}

impl Drop for ProviderFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn provider_response(State(state): State<ProviderState>, request: Request) -> Response<Body> {
    assert_eq!(
        request.headers()["authorization"],
        format!("Bearer {TOKEN}")
    );
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 256 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    state.seen.lock().unwrap().push(Seen { method, path, body });
    state.entered.add_permits(1);
    let reply = state.replies.lock().unwrap().pop_front().unwrap_or(Reply {
        status: 500,
        body: json!({"error":"unexpected fixture request"}),
        hold: None,
    });
    if let Some(hold) = reply.hold {
        hold.acquire().await.unwrap().forget();
    }
    Response::builder()
        .status(StatusCode::from_u16(reply.status).unwrap())
        .header("content-type", "application/json")
        .body(Body::from(reply.body.to_string()))
        .unwrap()
}

fn request(receipt_url: &str) -> ProbeRequest {
    ProbeRequest {
        org_id: OrgId::new(ORG),
        challenge_id: "rc-readiness-probe-test".into(),
        nonce: "4".repeat(64),
        receipt_url: receipt_url.into(),
        role_id: "role-monitor".into(),
        role_key: "monitor".into(),
        provider_agent_id: AGENT.into(),
        config_digest: "a".repeat(64),
        expires_at: Utc::now() + ChronoDuration::minutes(5),
    }
}

fn assert_payload(body: &Value, request: &ProbeRequest) {
    let fields: Vec<_> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(fields.len(), 3);
    assert!(
        fields.contains(&"agent_id")
            && fields.contains(&"input")
            && fields.contains(&"should_notify")
    );
    assert_eq!(body["agent_id"], AGENT);
    assert_eq!(body["should_notify"], false);
    let input = &body["input"];
    assert_eq!(input.as_object().unwrap().len(), 6);
    assert_eq!(input["schema_version"], 1);
    assert_eq!(input["task"], "genesis_gateway_readiness");
    assert!(input["instructions"]
        .as_str()
        .is_some_and(|text| !text.is_empty()));
    assert_eq!(input["receipt_url"], request.receipt_url);
    assert!(
        input["receipt"] == request.expected_receipt(),
        "exact one-time receipt sent"
    );
    assert!(
        input["context"]
            == json!({"org_id":request.org_id,"role_id":request.role_id,
        "role_key":request.role_key,"config_digest":request.config_digest,"expires_at":request.expires_at}),
        "exact role and expiry context sent"
    );
    let text = body.to_string();
    for secret in [
        TOKEN,
        "org_probe-operator",
        "org_probe-adapter",
        "org_probe-agent",
        "Authorization",
        "Bearer",
    ] {
        assert!(
            !text.contains(secret),
            "provider body must not contain credentials"
        );
    }
    assert!(body.get("model").is_none());
    assert!(body.get("conversation_id").is_none());
    assert!(body.get("workspace_id").is_none());
}

fn assert_unknown(error: ProviderError) {
    assert!(matches!(
        error,
        ProviderError::Unknown(_) | ProviderError::Timeout
    ));
    let text = error.to_string();
    assert!(!text.contains("PRIVATE_PROVIDER_BODY_CANARY"));
    assert!(!text.contains(TOKEN));
}

#[tokio::test]
async fn exact_readiness_run_is_durable_and_replays_never_poll_or_submit_again() {
    let hold = Arc::new(Semaphore::new(0));
    let provider = ProviderFixture::new(vec![Reply::run(202, "completed").held(&hold)]).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("creao.db");
    let (runtime, store) = provider.runtime(&path);
    let request = request(&format!("http://127.0.0.1:19001{RECEIPT_PATH}"));
    let probe = CreaoReadinessProbe::new(runtime.clone(), request.receipt_url.clone()).unwrap();
    let concurrent = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    let sending = request.clone();
    let first = tokio::spawn(async move { probe.dispatch(&sending).await });
    provider.entered().await;
    assert_unknown(concurrent.dispatch(&request).await.unwrap_err());
    hold.add_permits(1);
    assert_eq!(
        first.await.unwrap().unwrap().probe_ref.as_deref(),
        Some(RUN)
    );
    assert_eq!(
        concurrent
            .dispatch(&request)
            .await
            .unwrap()
            .probe_ref
            .as_deref(),
        Some(RUN)
    );
    let records = store.list().unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.phase, OperationPhase::Waiting);
    match &record.spec.purpose {
        OperationPurpose::Readiness {
            challenge_id,
            config_digest,
            expires_at,
        } => {
            assert_eq!(challenge_id, &request.challenge_id);
            assert_eq!(config_digest, &request.config_digest);
            assert_eq!(expires_at, &request.expires_at);
        }
        _ => panic!("readiness must have its own durable purpose"),
    }
    assert_eq!(record.spec.role_id, request.role_id);
    assert_eq!(record.spec.org_id, ORG);
    assert_eq!(record.spec.account_namespace, ACCOUNT);
    assert!(
        matches!(&record.spec.request, OperationRequest::CreateRun(run)
        if run.agent_id == AGENT && run.model.is_none() && run.workspace_id.is_none() && run.conversation_id.is_none())
    );
    let seen = provider.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, "/v1/runs");
    assert_payload(&seen[0].body, &request);
    drop(concurrent);
    drop(records);
    drop(store);
    let (runtime, _) = provider.runtime(&path);
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    let restarted = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    assert_eq!(
        restarted
            .dispatch(&request)
            .await
            .unwrap()
            .probe_ref
            .as_deref(),
        Some(RUN)
    );
    assert_eq!(
        provider.seen().len(),
        1,
        "restart must neither poll nor resubmit"
    );
}

#[tokio::test]
async fn changed_challenge_secret_or_binding_conflicts_without_new_provider_work() {
    let provider = ProviderFixture::new(vec![Reply::run(202, "pending")]).await;
    let dir = tempfile::tempdir().unwrap();
    let (runtime, store) = provider.runtime(&dir.path().join("creao.db"));
    let original = request(&format!("http://127.0.0.1:19001{RECEIPT_PATH}"));
    let probe = CreaoReadinessProbe::new(runtime, original.receipt_url.clone()).unwrap();
    probe.dispatch(&original).await.unwrap();
    let initial = store.list().unwrap().remove(0).fingerprint;
    for field in [
        "nonce", "role_id", "role_key", "agent", "config", "expiry", "url",
    ] {
        let mut changed = original.clone();
        match field {
            "nonce" => changed.nonce = "5".repeat(64),
            "role_id" => changed.role_id = "other-role".into(),
            "role_key" => changed.role_key = "other-key".into(),
            "agent" => changed.provider_agent_id = OTHER_AGENT.into(),
            "config" => changed.config_digest = "b".repeat(64),
            "expiry" => changed.expires_at += ChronoDuration::seconds(1),
            "url" => changed.receipt_url = format!("http://127.0.0.1:19002{RECEIPT_PATH}"),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                probe.dispatch(&changed).await,
                Err(ProviderError::Rejected(_))
            ),
            "changed {field} accepted"
        );
        let records = store.list().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].fingerprint, initial);
    }
    assert_eq!(provider.seen().len(), 1);
}

#[tokio::test]
async fn expired_and_foreign_challenges_never_prepare_or_submit() {
    let provider = ProviderFixture::new(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let (runtime, store) = provider.runtime(&dir.path().join("creao.db"));
    let original = request(&format!("http://127.0.0.1:19001{RECEIPT_PATH}"));
    let probe = CreaoReadinessProbe::new(runtime, original.receipt_url.clone()).unwrap();
    for field in ["expired", "foreign_org", "foreign_url"] {
        let mut changed = original.clone();
        match field {
            "expired" => changed.expires_at = Utc::now(),
            "foreign_org" => changed.org_id = OrgId::new("other-org"),
            "foreign_url" => changed.receipt_url = format!("http://127.0.0.1:19002{RECEIPT_PATH}"),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                probe.dispatch(&changed).await,
                Err(ProviderError::Rejected(_))
            ),
            "invalid {field} dispatched"
        );
    }
    assert!(store.list().unwrap().is_empty());
    assert!(provider.seen().is_empty());
}

#[tokio::test]
async fn ambiguous_post_stays_unknown_after_replay_and_restart() {
    let provider = ProviderFixture::new(vec![Reply::ambiguous()]).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("creao.db");
    let (runtime, store) = provider.runtime(&path);
    let request = request(&format!("http://127.0.0.1:19001{RECEIPT_PATH}"));
    let probe = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    assert_unknown(probe.dispatch(&request).await.unwrap_err());
    assert_unknown(probe.dispatch(&request).await.unwrap_err());
    assert_eq!(
        store.list().unwrap()[0].phase,
        OperationPhase::NeedsReconciliation
    );
    drop(probe);
    drop(store);
    let (runtime, _) = provider.runtime(&path);
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    let probe = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    assert_unknown(probe.dispatch(&request).await.unwrap_err());
    assert_eq!(provider.seen().len(), 1);
}

#[tokio::test]
async fn cancelled_post_has_a_durable_intent_and_restart_does_not_replay_it() {
    let hold = Arc::new(Semaphore::new(0));
    let provider = ProviderFixture::new(vec![Reply::run(202, "pending").held(&hold)]).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("creao.db");
    let (runtime, store) = provider.runtime(&path);
    let request = request(&format!("http://127.0.0.1:19001{RECEIPT_PATH}"));
    let probe = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    let sent = request.clone();
    let task = tokio::spawn(async move { probe.dispatch(&sent).await });
    provider.entered().await;
    // Read through a separately opened SQLite connection before the POST returns.
    let independent = RuntimeStore::open(&path, ORG).unwrap();
    assert_eq!(
        independent.list().unwrap()[0].phase,
        OperationPhase::Submitting
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    hold.add_permits(1);
    drop(independent);
    drop(store);
    let (runtime, restarted_store) = provider.runtime(&path);
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    assert_eq!(
        restarted_store.list().unwrap()[0].phase,
        OperationPhase::NeedsReconciliation
    );
    let probe = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    assert_unknown(probe.dispatch(&request).await.unwrap_err());
    assert_eq!(provider.seen().len(), 1);
}

struct GatewayFixture {
    app: AppState,
    config: GatewayConfig,
    server: JoinHandle<()>,
}

impl GatewayFixture {
    async fn new(path: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let config = GatewayConfig::from_public_url(&base).unwrap();
        let mut org = OrgConfig::dev(ORG, "Readiness probe fixture");
        org.db = Some(path.to_owned());
        let app = build_org(&org, None).unwrap();
        let router = app.clone().router();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            app,
            config,
            server,
        }
    }

    fn verifier(&self, runtime: Runtime) -> GatewayReadiness {
        GatewayReadiness::new(
            self.app.store.clone(),
            self.app.org_id.clone(),
            Provider::Creao,
            Arc::new(CreaoReadinessProbe::new(runtime, self.config.receipt_url.clone()).unwrap()),
            self.config.clone(),
        )
        .unwrap()
    }

    async fn receipt(&self, receipt: &Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(&self.config.receipt_url)
            .json(receipt)
            .send()
            .await
            .unwrap();
        (response.status().as_u16(), response.json().await.unwrap())
    }
}

impl Drop for GatewayFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn role() -> RoleToProvision {
    RoleToProvision {
        key: "monitor".into(),
        role_id: RoleId::new("role-monitor"),
        name: "Monitor".into(),
        charter: "Monitor the event".into(),
        authority: AuthorityGrant::default(),
        provider: Provider::Creao,
    }
}

fn binding() -> AgentBinding {
    AgentBinding {
        provider: Provider::Creao,
        provider_agent_id: AGENT.into(),
        config_digest: "a".repeat(64),
        conversation_ref: None,
    }
}

#[tokio::test]
async fn native_completion_is_unproven_until_an_exact_gateway_receipt_survives_restart() {
    let provider = ProviderFixture::new(vec![
        Reply::run(202, "pending"),
        Reply::run(200, "completed"),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let runtime_path = dir.path().join("creao.db");
    let (runtime, runtime_store) = provider.runtime(&runtime_path);
    let gateway = GatewayFixture::new(&dir.path().join("org.db")).await;
    let verifier = gateway.verifier(runtime.clone());
    assert!(matches!(
        verifier.verify(&role(), &binding()).await.unwrap(),
        Readiness::Waiting(_)
    ));
    let wire = &provider.seen()[0].body;
    let receipt = wire["input"]["receipt"].clone();
    let challenge = receipt["challenge_id"].as_str().unwrap();
    let operation = runtime_store.list().unwrap().remove(0);
    let due = operation.next_poll_at.unwrap();
    tokio::time::sleep((due - Utc::now()).to_std().unwrap_or_default()).await;
    assert_eq!(
        runtime
            .step(&operation.spec.operation_id, false)
            .await
            .unwrap()
            .phase,
        OperationPhase::NativeCompleted
    );
    assert!(
        matches!(
            verifier.verify(&role(), &binding()).await.unwrap(),
            Readiness::Waiting(_)
        ),
        "native success is not gateway evidence"
    );
    let mut wrong = receipt.clone();
    wrong["nonce"] = json!("wrong-nonce");
    assert_eq!(gateway.receipt(&wrong).await.0, 403);
    wrong = receipt.clone();
    wrong["provider_agent_id"] = json!(OTHER_AGENT);
    assert_eq!(gateway.receipt(&wrong).await.0, 403);
    let (status, accepted) = gateway.receipt(&receipt).await;
    assert_eq!(status, 200);
    assert_eq!(accepted["duplicate"], false);
    assert_eq!(gateway.receipt(&receipt).await.1["duplicate"], true);
    let reference = format!("gateway-receipt:{challenge}");
    assert_eq!(
        verifier.verify(&role(), &binding()).await.unwrap(),
        Readiness::Ready {
            evidence_ref: reference.clone()
        }
    );
    let challenge_url = gateway.config.receipt_url.replace(
        "readiness-receipts",
        &format!("readiness-challenges/{challenge}"),
    );
    let public = reqwest::Client::new()
        .get(challenge_url)
        .bearer_auth("org_probe-operator")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!public.contains(receipt["nonce"].as_str().unwrap()));
    assert!(!public.contains("nonce_sha256"));
    assert!(!public.contains(TOKEN));
    assert_eq!(provider.seen().len(), 2);
    drop(verifier);
    drop(runtime);
    drop(runtime_store);
    let (runtime, _) = provider.runtime(&runtime_path);
    runtime.recover_interrupted().await.unwrap();
    let mut cfg = OrgConfig::dev(ORG, "Readiness probe fixture");
    cfg.db = Some(dir.path().join("org.db"));
    let restored_app = build_org(&cfg, None).unwrap();
    let restored = GatewayReadiness::new(
        restored_app.store.clone(),
        OrgId::new(ORG),
        Provider::Creao,
        Arc::new(CreaoReadinessProbe::new(runtime, gateway.config.receipt_url.clone()).unwrap()),
        gateway.config.clone(),
    )
    .unwrap();
    assert_eq!(
        restored.verify(&role(), &binding()).await.unwrap(),
        Readiness::Ready {
            evidence_ref: reference
        }
    );
    assert_eq!(
        provider.seen().len(),
        2,
        "historical receipt does not execute another run"
    );
    assert!(
        restored_app.current_state().unwrap().roles.is_empty(),
        "readiness evidence alone never activates a role"
    );
}

#[tokio::test]
async fn valid_receipt_arriving_before_an_ambiguous_post_response_is_not_lost() {
    let hold = Arc::new(Semaphore::new(0));
    let provider = ProviderFixture::new(vec![Reply::ambiguous().held(&hold)]).await;
    let dir = tempfile::tempdir().unwrap();
    let (runtime, runtime_store) = provider.runtime(&dir.path().join("creao.db"));
    let gateway = GatewayFixture::new(&dir.path().join("org.db")).await;
    let verifier = Arc::new(gateway.verifier(runtime));
    let running = verifier.clone();
    let task = tokio::spawn(async move { running.verify(&role(), &binding()).await });
    provider.entered().await;
    let receipt = provider.seen()[0].body["input"]["receipt"].clone();
    assert_eq!(gateway.receipt(&receipt).await.0, 200);
    hold.add_permits(1);
    let reference = format!(
        "gateway-receipt:{}",
        receipt["challenge_id"].as_str().unwrap()
    );
    assert_eq!(
        task.await.unwrap().unwrap(),
        Readiness::Ready {
            evidence_ref: reference.clone()
        }
    );
    assert_eq!(
        runtime_store.list().unwrap()[0].phase,
        OperationPhase::NeedsReconciliation
    );
    assert_eq!(
        verifier.verify(&role(), &binding()).await.unwrap(),
        Readiness::Ready {
            evidence_ref: reference
        }
    );
    assert_eq!(provider.seen().len(), 1);
}
