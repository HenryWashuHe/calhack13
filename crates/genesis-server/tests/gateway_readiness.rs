//! Gateway readiness: a created asynchronous agent becomes ready only after
//! the backend receives a one-time challenge back through the receipt route.

use async_trait::async_trait;
use chrono::Utc;
use genesis_core::change::{OrgChangeProposal, OrgChangeRecord, ProviderOperationState as S};
use genesis_core::engine::MissionBootstrap;
use genesis_core::ids::ChangeId;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::ChangeStatus;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{
    DevStub, ProviderError, ProviderSet, ProvisionRequest, SpecialistProgress, SpecialistRuntime,
};
use genesis_server::app::AppState;
use genesis_server::gateway_readiness::{
    self, GatewayConfig, ProbeDispatch, ProbeRequest, ReadinessProbe,
};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const OP: &str = "creao-role-op-1";
const AGENT: &str = "creao-agent-42";

/// Runtime whose role operation is created on the first advance.
struct Created;

#[async_trait]
impl SpecialistRuntime for Created {
    fn provider(&self) -> Provider {
        Provider::Creao
    }
    fn org_id(&self) -> &str {
        "org_a"
    }
    fn prepare_role(&self, _: &ProvisionRequest) -> Result<SpecialistProgress, ProviderError> {
        Ok(SpecialistProgress::Pending {
            operation_id: OP.into(),
            run_id: None,
            native_status: None,
        })
    }
    fn prepare_job(&self, _: &Job, _: &AgentBinding) -> Result<SpecialistProgress, ProviderError> {
        Err(ProviderError::Rejected("not used".into()))
    }
    fn get(&self, _: &str) -> Result<SpecialistProgress, ProviderError> {
        Err(ProviderError::Rejected("not used".into()))
    }
    async fn advance(&self, _: &str, _: bool) -> Result<SpecialistProgress, ProviderError> {
        Ok(SpecialistProgress::Created {
            operation_id: OP.into(),
            run_id: "run-1".into(),
            provider_agent_id: AGENT.into(),
            config_digest: "sha256:cfg".into(),
            readiness_verified: false,
        })
    }
}

#[derive(Clone, Copy)]
enum Answer {
    Accept,
    Unknown,
    Reject,
}

/// Records each probe request (as the provider-side readiness run would
/// receive it) and answers from a script.
struct Probe {
    answers: Mutex<VecDeque<Answer>>,
    seen: Mutex<Vec<ProbeRequest>>,
}

impl Probe {
    fn new(answers: &[Answer]) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.iter().copied().collect()),
            seen: Mutex::new(vec![]),
        })
    }
    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
    fn last(&self) -> ProbeRequest {
        self.seen.lock().unwrap().last().cloned().unwrap()
    }
}

#[async_trait]
impl ReadinessProbe for Probe {
    async fn dispatch(&self, req: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let answer = {
            let mut a = self.answers.lock().unwrap();
            if a.len() > 1 {
                a.pop_front().unwrap()
            } else {
                *a.front().unwrap()
            }
        };
        match answer {
            Answer::Accept => Ok(ProbeDispatch {
                probe_ref: Some(format!("probe-run-{}", self.count())),
            }),
            Answer::Unknown => Err(ProviderError::Unknown("connection dropped".into())),
            Answer::Reject => Err(ProviderError::Rejected("quota exhausted".into())),
        }
    }
}

struct Harness {
    app: AppState,
    base: String,
    http: reqwest::Client,
    _server: tokio::task::JoinHandle<()>,
}

async fn harness(
    probe: Arc<Probe>,
    cfg: impl FnOnce(&mut GatewayConfig),
    db: Option<std::path::PathBuf>,
) -> Harness {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut org_cfg = OrgConfig::dev("org_a", "Org A");
    org_cfg.db = db;
    let providers = ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .with_specialist(Arc::new(Created));
    let app = build_org(&org_cfg, Some(providers)).unwrap();
    let mut gw = GatewayConfig::from_public_url(&base).unwrap();
    cfg(&mut gw);
    let app = gateway_readiness::attach(app, Provider::Creao, probe, gw).unwrap();
    let router = app.clone().router();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Harness {
        app,
        base,
        http: reqwest::Client::new(),
        _server: server,
    }
}

impl Harness {
    async fn post_receipt(&self, url: &str, body: Value) -> (u16, Value) {
        assert!(
            url.starts_with(&self.base),
            "receipt URL points at this kernel"
        );
        let r = self.http.post(url).json(&body).send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }
    async fn challenge(&self, id: &str) -> Value {
        self.http
            .get(format!(
                "{}/api/v1/gateway/readiness-challenges/{id}",
                self.base
            ))
            .bearer_auth("org_a-operator")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

/// Submit a change creating a CREAO "monitor" role that owns a new requirement.
async fn submit(app: &AppState) -> ChangeId {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": "Run the event", "roles": [{"key":"lead","name":"Lead","charter":"Own work",
        "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
        "responsibilities": [{"purpose":"Coordinate work","owner":"lead"}]
    }))
    .unwrap();
    let mission = app
        .mutate(EventMeta::default(), |s| {
            genesis_core::engine::bootstrap_mission(s, &spec, Utc::now())
                .map_err(genesis_server::error::validation_error)
        })
        .await
        .unwrap()
        .value;
    let obs = app
        .mutate(EventMeta::default(), |s| {
            genesis_core::engine::record_observation(
                s,
                &mission,
                "fixture",
                "Venue updates arrive daily",
                None,
                Utc::now(),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let req = app
        .mutate(EventMeta::default(), |s| {
            genesis_core::engine::record_requirement(
                s,
                &mission,
                "Monitor venue updates",
                std::slice::from_ref(&obs),
                "Every update handled",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision": state.revision, "trigger_evidence_ids": [obs],
        "rationale": "Continuing monitoring needs its own owner", "budget_limit_cents": 0,
        "operations": [
            {"op":"CreateRole","key":"monitor","name":"Venue monitor","charter":"Handle each venue update",
             "authority":{"scopes":["mission.coordinate"],"budget_cents":0},"reports_to":null,
             "provider":"creao","justification":"independent_ownership"},
            {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"monitoring",
             "requirement_id":req,"purpose":"Handle venue updates","trigger":"update","termination":"event held"},
             "owner":{"kind":"new","key":"monitor"}}
        ]
    }))
    .unwrap();
    let (_, rec) = saga::submit(
        app,
        "agent",
        &format!("monitor-{}", state.revision),
        proposal,
    )
    .await
    .unwrap();
    rec.change_id
}

async fn drive(app: &AppState, id: &ChangeId) -> OrgChangeRecord {
    saga::run(app, id).await.unwrap()
}

fn monitor_bound(app: &AppState) -> bool {
    app.current_state()
        .unwrap()
        .roles
        .into_values()
        .any(|r| r.name == "Venue monitor" && r.binding.is_some())
}

#[tokio::test]
async fn receipt_through_the_gateway_commits_the_role() {
    let probe = Probe::new(&[Answer::Accept]);
    let h = harness(probe.clone(), |_| {}, None).await;
    let id = submit(&h.app).await;

    let r = drive(&h.app, &id).await; // created; challenge issued and dispatched
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::AwaitingReadiness);
    assert_eq!(probe.count(), 1);
    let req = probe.last();
    assert_eq!(req.provider_agent_id, AGENT);
    assert_eq!(req.role_key, "monitor");
    assert!(req.nonce.len() >= 64, "nonce has enough entropy");

    let r = drive(&h.app, &id).await; // no receipt yet: still waiting, no new dispatch
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(probe.count(), 1);
    assert!(!monitor_bound(&h.app));

    // The agent's gateway skill posts the challenge back.
    let (status, body) = h
        .post_receipt(&req.receipt_url, req.expected_receipt())
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["duplicate"], false);

    let r = drive(&h.app, &id).await;
    assert_eq!(r.status, ChangeStatus::Committed);
    assert_eq!(r.operations["monitor"].state, S::Ready);
    let evidence = format!("gateway-receipt:{}", req.challenge_id);
    assert_eq!(
        r.operations["monitor"].readiness_evidence.as_deref(),
        Some(evidence.as_str())
    );
    assert!(monitor_bound(&h.app));
    assert_eq!(probe.count(), 1, "one challenge, one dispatch");

    // Operators can inspect the receipt; no secret material is exposed.
    let c = h.challenge(&req.challenge_id).await;
    assert_eq!(c["status"], "received");
    assert_eq!(c["probe_ref"], "probe-run-1");
    assert_eq!(c["provider_agent_id"], AGENT);
    let text = c.to_string();
    assert!(!text.contains(&req.nonce));
    assert!(!text.contains("nonce"));

    // Replaying the same receipt changes nothing.
    let (status, body) = h
        .post_receipt(&req.receipt_url, req.expected_receipt())
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["duplicate"], true);
}

#[tokio::test]
async fn forged_receipts_are_rejected_indistinguishably() {
    let probe = Probe::new(&[Answer::Accept]);
    let h = harness(probe.clone(), |_| {}, None).await;
    let id = submit(&h.app).await;
    drive(&h.app, &id).await;
    let req = probe.last();
    let url = req.receipt_url.clone();

    let mut wrong_nonce = req.expected_receipt();
    wrong_nonce["nonce"] = json!("0".repeat(64));
    let mut wrong_agent = req.expected_receipt();
    wrong_agent["provider_agent_id"] = json!("creao-agent-other");
    let mut unknown = req.expected_receipt();
    unknown["challenge_id"] = json!("rc-unknown");
    let mut wrong_probe = req.expected_receipt();
    wrong_probe["probe_ref"] = json!("probe-run-999");

    let mut bodies = vec![];
    for body in [wrong_nonce, wrong_agent, unknown, wrong_probe] {
        let (status, body) = h.post_receipt(&url, body).await;
        assert_eq!(status, 403);
        bodies.push(body);
    }
    assert!(
        bodies.windows(2).all(|w| w[0] == w[1]),
        "no oracle: {bodies:?}"
    );

    // Unknown fields and oversized values are refused before lookup.
    let mut extra = req.expected_receipt();
    extra["ready"] = json!(true);
    let (status, _) = h.post_receipt(&url, extra).await;
    assert!((400..500).contains(&status));
    let mut huge = req.expected_receipt();
    huge["nonce"] = json!("x".repeat(10_000));
    let (status, _) = h.post_receipt(&url, huge).await;
    assert!((400..500).contains(&status));

    let r = drive(&h.app, &id).await;
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::AwaitingReadiness);
    assert!(!monitor_bound(&h.app));

    // The genuine receipt still works.
    let (status, _) = h.post_receipt(&url, req.expected_receipt()).await;
    assert_eq!(status, 200);
    assert_eq!(drive(&h.app, &id).await.status, ChangeStatus::Committed);
}

#[tokio::test]
async fn expired_challenges_are_reissued_then_readiness_fails() {
    let probe = Probe::new(&[Answer::Accept]);
    let h = harness(
        probe.clone(),
        |c| {
            c.ttl = Duration::from_millis(500);
            c.max_attempts = 2;
        },
        None,
    )
    .await;
    let id = submit(&h.app).await;
    drive(&h.app, &id).await;
    let first = probe.last();
    tokio::time::sleep(Duration::from_millis(600)).await;

    // A late receipt for an expired challenge is refused.
    let (status, _) = h
        .post_receipt(&first.receipt_url, first.expected_receipt())
        .await;
    assert_eq!(status, 410);

    let r = drive(&h.app, &id).await; // attempt 2 issued
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(probe.count(), 2);
    let second = probe.last();
    assert_ne!(first.challenge_id, second.challenge_id);
    assert_ne!(first.nonce, second.nonce);
    // The first nonce does not unlock the second challenge.
    let mut stale = second.expected_receipt();
    stale["nonce"] = json!(first.nonce);
    assert_eq!(h.post_receipt(&second.receipt_url, stale).await.0, 403);

    tokio::time::sleep(Duration::from_millis(600)).await;
    let r = drive(&h.app, &id).await;
    assert_eq!(r.status, ChangeStatus::Failed);
    let events = serde_json::to_string(&h.app.store.events_after(0, 1000).unwrap()).unwrap();
    assert!(events.contains("no gateway readiness receipt after 2 attempt(s)"));
    assert_eq!(
        r.provisioned["monitor"].provider_agent_id, AGENT,
        "created ID kept"
    );
    assert_eq!(probe.count(), 2);
    assert!(!monitor_bound(&h.app));
}

#[tokio::test]
async fn unknown_dispatch_keeps_the_challenge_live() {
    let probe = Probe::new(&[Answer::Unknown, Answer::Accept]);
    let h = harness(probe.clone(), |_| {}, None).await;
    let id = submit(&h.app).await;
    let r = drive(&h.app, &id).await;
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert!(r.operations["monitor"]
        .attention
        .as_deref()
        .unwrap_or_default()
        .contains("readiness check"));
    let req = probe.last();
    let c = h.challenge(&req.challenge_id).await;
    assert_eq!(c["status"], "dispatching");

    let r = drive(&h.app, &id).await; // not re-dispatched while live
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(probe.count(), 1);

    // The run did reach the agent after all; its receipt counts.
    assert_eq!(
        h.post_receipt(&req.receipt_url, req.expected_receipt())
            .await
            .0,
        200
    );
    assert_eq!(drive(&h.app, &id).await.status, ChangeStatus::Committed);
    assert_eq!(probe.count(), 1);
}

#[tokio::test]
async fn definite_dispatch_failure_retries_with_a_new_challenge() {
    let probe = Probe::new(&[Answer::Reject, Answer::Accept]);
    let h = harness(
        probe.clone(),
        |c| c.retry_after = Duration::from_millis(500),
        None,
    )
    .await;
    let id = submit(&h.app).await;
    drive(&h.app, &id).await;
    let failed = probe.last();
    assert_eq!(
        h.challenge(&failed.challenge_id).await["status"],
        "dispatch_failed"
    );
    // The nonce never left the backend, so its receipt is invalid.
    assert_eq!(
        h.post_receipt(&failed.receipt_url, failed.expected_receipt())
            .await
            .0,
        403
    );

    drive(&h.app, &id).await; // backing off: no new attempt yet
    assert_eq!(probe.count(), 1);
    tokio::time::sleep(Duration::from_millis(600)).await;
    drive(&h.app, &id).await;
    assert_eq!(probe.count(), 2);
    let second = probe.last();
    assert_eq!(
        h.post_receipt(&second.receipt_url, second.expected_receipt())
            .await
            .0,
        200
    );
    assert_eq!(drive(&h.app, &id).await.status, ChangeStatus::Committed);
}

#[tokio::test]
async fn receipts_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org_a.sqlite");
    let probe = Probe::new(&[Answer::Accept]);
    let (id, req) = {
        let h = harness(probe.clone(), |_| {}, Some(db.clone())).await;
        let id = submit(&h.app).await;
        drive(&h.app, &id).await;
        let req = probe.last();
        assert_eq!(
            h.post_receipt(&req.receipt_url, req.expected_receipt())
                .await
                .0,
            200
        );
        (id, req)
    };
    // A new process with a fresh probe: the stored receipt is the evidence.
    let probe2 = Probe::new(&[Answer::Accept]);
    let h = harness(probe2.clone(), |_| {}, Some(db)).await;
    let r = drive(&h.app, &id).await;
    assert_eq!(r.status, ChangeStatus::Committed);
    assert_eq!(
        r.operations["monitor"].readiness_evidence.as_deref(),
        Some(format!("gateway-receipt:{}", req.challenge_id).as_str())
    );
    assert_eq!(probe2.count(), 0, "no new challenge after restart");
}
