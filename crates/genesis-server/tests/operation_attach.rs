//! Operator attach: a role operation whose creation outcome is unknown
//! (`needs_reconciliation`) can be bound to the native agent the operator
//! found, but the role still activates only after readiness is proven.

use async_trait::async_trait;
use chrono::Utc;
use genesis_core::change::{OrgChangeProposal, OrgChangeRecord, ProviderOperationState as S};
use genesis_core::engine::{MissionBootstrap, RoleToProvision};
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
use genesis_server::specialist::{Readiness, ReadinessVerifier};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const OP: &str = "creao-role-op-1";
const FOUND: &str = "6f1c2a9e-1111-4c3b-9d2e-000000000042";

/// Runtime whose creation POST was lost: every advance reports
/// `needs_reconciliation`.
#[derive(Default)]
struct Ambiguous {
    prepares: AtomicUsize,
    advances: AtomicUsize,
}

#[async_trait]
impl SpecialistRuntime for Ambiguous {
    fn provider(&self) -> Provider {
        Provider::Creao
    }
    fn org_id(&self) -> &str {
        "org_a"
    }
    fn prepare_role(&self, _: &ProvisionRequest) -> Result<SpecialistProgress, ProviderError> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
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
        self.advances.fetch_add(1, Ordering::SeqCst);
        Ok(SpecialistProgress::NeedsReconciliation {
            operation_id: OP.into(),
            run_id: Some("run-lost".into()),
        })
    }
}

/// Verifier with a fixed answer that records what it was asked to verify.
struct Fixed {
    answer: Readiness,
    seen: Mutex<Vec<AgentBinding>>,
}

#[async_trait]
impl ReadinessVerifier for Fixed {
    async fn verify(
        &self,
        _: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        self.seen.lock().unwrap().push(binding.clone());
        Ok(self.answer.clone())
    }
}

fn fixed(answer: Readiness) -> Arc<Fixed> {
    Arc::new(Fixed {
        answer,
        seen: Mutex::new(vec![]),
    })
}

fn org(runtime: Arc<Ambiguous>) -> AppState {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let providers = ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .with_specialist(runtime);
    build_org(&cfg, Some(providers)).unwrap()
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

/// Drive until the lost creation is reported as needing reconciliation.
async fn ambiguous_change(app: &AppState) -> ChangeId {
    let id = submit(app).await;
    let r = drive(app, &id).await;
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::NeedsReconciliation);
    assert!(r.provisioned.is_empty());
    id
}

fn monitor_agent(app: &AppState) -> Option<String> {
    app.current_state()
        .unwrap()
        .roles
        .into_values()
        .find(|r| r.name == "Venue monitor")
        .and_then(|r| r.binding)
        .map(|b| b.provider_agent_id)
}

fn expected_digest(app: &AppState, r: &OrgChangeRecord) -> String {
    let role = genesis_core::engine::roles_to_provision(&r.proposal, &r.allocation)
        .into_iter()
        .find(|x| x.key == "monitor")
        .unwrap();
    ProvisionRequest {
        org_id: app.org_id.clone(),
        change_id: r.change_id.clone(),
        role,
    }
    .config_digest()
}

#[tokio::test]
async fn attached_agent_commits_only_after_readiness() {
    let rt = Arc::new(Ambiguous::default());
    let v = fixed(Readiness::Ready {
        evidence_ref: "gateway-receipt:rc-test".into(),
    });
    let app = org(rt.clone()).with_readiness_verifier(Provider::Creao, v.clone());
    let id = ambiguous_change(&app).await;
    let advances = rt.advances.load(Ordering::SeqCst);

    let r = saga::attach_operation(&app, &id, "monitor", FOUND, "found in CREAO console")
        .await
        .unwrap();
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::AwaitingReadiness);
    assert_eq!(r.provisioned["monitor"].provider_agent_id, FOUND);
    assert_eq!(
        r.provisioned["monitor"].config_digest,
        expected_digest(&app, &r)
    );
    assert_eq!(
        r.operations["monitor"].native_status.as_deref(),
        Some(saga::OPERATOR_ATTACHED)
    );
    assert!(monitor_agent(&app).is_none(), "not active before readiness");

    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Committed);
    assert_eq!(monitor_agent(&app).as_deref(), Some(FOUND));
    assert_eq!(v.seen.lock().unwrap()[0].provider_agent_id, FOUND);
    assert_eq!(rt.prepares.load(Ordering::SeqCst), 1);
    assert_eq!(
        rt.advances.load(Ordering::SeqCst),
        advances,
        "no further provider calls after attach"
    );

    let log = serde_json::to_string(&app.store.events_after(0, 1000).unwrap()).unwrap();
    assert!(log.contains("attached by operator (found in CREAO console)"));
    assert!(
        !log.contains("role_provisioned"),
        "the provider never reported this agent"
    );
}

#[tokio::test]
async fn attached_agent_without_verifier_stays_inactive() {
    let rt = Arc::new(Ambiguous::default());
    let app = org(rt);
    let id = ambiguous_change(&app).await;
    saga::attach_operation(&app, &id, "monitor", FOUND, "found")
        .await
        .unwrap();
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::AwaitingReadiness);
    assert!(r.operations["monitor"]
        .attention
        .as_deref()
        .unwrap_or_default()
        .contains("no readiness verifier"));
    assert!(monitor_agent(&app).is_none());
}

#[tokio::test]
async fn attached_agent_that_fails_readiness_fails_the_change() {
    let rt = Arc::new(Ambiguous::default());
    let app = org(rt).with_readiness_verifier(
        Provider::Creao,
        fixed(Readiness::NotReady("agent not found".into())),
    );
    let id = ambiguous_change(&app).await;
    saga::attach_operation(&app, &id, "monitor", FOUND, "found")
        .await
        .unwrap();
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Failed);
    assert_eq!(r.provisioned["monitor"].provider_agent_id, FOUND, "ID kept");
    assert!(monitor_agent(&app).is_none());
}

#[tokio::test]
async fn attach_is_refused_unless_the_outcome_is_unknown() {
    let rt = Arc::new(Ambiguous::default());
    let app = org(rt).with_readiness_verifier(
        Provider::Creao,
        fixed(Readiness::Ready {
            evidence_ref: "x".into(),
        }),
    );
    let id = ambiguous_change(&app).await;

    let code = |e: genesis_server::error::ApiError| (e.status.as_u16(), e.error);
    let e = saga::attach_operation(&app, &id, "lead", FOUND, "x")
        .await
        .unwrap_err();
    assert_eq!(
        code(e).0,
        404,
        "existing role is not provisioned by this change"
    );
    let e = saga::attach_operation(&app, &id, "monitor", "bad id/../x", "x")
        .await
        .unwrap_err();
    assert_eq!(code(e).0, 400);

    saga::attach_operation(&app, &id, "monitor", FOUND, "found")
        .await
        .unwrap();
    // Already bound: a second attach (even with another ID) is refused.
    let e = saga::attach_operation(&app, &id, "monitor", "other-agent", "x")
        .await
        .unwrap_err();
    assert_eq!(code(e), (409, "operation_not_reconcilable".into()));

    drive(&app, &id).await; // committed
    let e = saga::attach_operation(&app, &id, "monitor", FOUND, "x")
        .await
        .unwrap_err();
    assert_eq!(code(e), (409, "not_provisioning".into()));
}

struct Probe(Mutex<Vec<ProbeRequest>>);

#[async_trait]
impl ReadinessProbe for Probe {
    async fn dispatch(&self, req: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        self.0.lock().unwrap().push(req.clone());
        Ok(ProbeDispatch {
            probe_ref: Some("readiness-run-1".into()),
        })
    }
}

/// Through HTTP, with gateway readiness: only operators may attach, and the
/// attached agent must answer its own challenge.
#[tokio::test]
async fn attach_over_http_then_prove_with_a_gateway_receipt() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let probe = Arc::new(Probe(Mutex::new(vec![])));
    let app = gateway_readiness::attach(
        org(Arc::new(Ambiguous::default())),
        Provider::Creao,
        probe.clone(),
        GatewayConfig::from_public_url(&base).unwrap(),
    )
    .unwrap();
    let router = app.clone().router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    let id = ambiguous_change(&app).await;
    let url = format!("{base}/api/v1/org-changes/{id}/operations/monitor/attach");
    let post = |token: &'static str, body: Value| {
        let (http, url) = (http.clone(), url.clone());
        async move {
            let r = http
                .post(&url)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await
                .unwrap();
            r.status().as_u16()
        }
    };

    let body = json!({"provider_agent_id": FOUND, "reason": "found in CREAO console"});
    assert_eq!(post("org_a-agent", body.clone()).await, 403);
    assert_eq!(post("org_a-adapter", body.clone()).await, 403);
    assert_eq!(
        post(
            "org_a-operator",
            json!({"provider_agent_id": FOUND, "reason": " "})
        )
        .await,
        400
    );
    assert_eq!(
        post(
            "org_a-operator",
            json!({"provider_agent_id": FOUND, "reason": "x", "ready": true})
        )
        .await,
        422
    );
    assert_eq!(post("org_a-operator", body.clone()).await, 200);
    assert_eq!(post("org_a-operator", body).await, 409);

    let r = drive(&app, &id).await; // challenge issued to the attached agent
    assert_eq!(r.status, ChangeStatus::Provisioning);
    let req = probe.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(req.provider_agent_id, FOUND);

    // A receipt naming a different agent does not count.
    let mut other = req.expected_receipt();
    other["provider_agent_id"] = json!("some-other-agent");
    let s = http
        .post(&req.receipt_url)
        .json(&other)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(s.as_u16(), 403);
    assert_eq!(drive(&app, &id).await.status, ChangeStatus::Provisioning);

    let s = http
        .post(&req.receipt_url)
        .json(&req.expected_receipt())
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(s.as_u16(), 200);
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Committed);
    assert_eq!(monitor_agent(&app).as_deref(), Some(FOUND));
}

#[tokio::test]
async fn an_agent_already_bound_elsewhere_cannot_be_attached() {
    let rt = Arc::new(Ambiguous::default());
    let app = org(rt).with_readiness_verifier(
        Provider::Creao,
        fixed(Readiness::Ready {
            evidence_ref: "x".into(),
        }),
    );
    // First change: attach and commit, so the agent is bound to an active role.
    let first = ambiguous_change(&app).await;
    saga::attach_operation(&app, &first, "monitor", FOUND, "found")
        .await
        .unwrap();
    assert_eq!(drive(&app, &first).await.status, ChangeStatus::Committed);

    // A second lost creation cannot claim the same agent.
    let second = ambiguous_change(&app).await;
    let e = saga::attach_operation(&app, &second, "monitor", FOUND, "same agent")
        .await
        .unwrap_err();
    assert_eq!(
        (e.status.as_u16(), e.error.as_str()),
        (409, "agent_already_bound")
    );
    // Nor an agent bound by another change that is still provisioning.
    let third = ambiguous_change(&app).await;
    saga::attach_operation(&app, &second, "monitor", "agent-two", "found")
        .await
        .unwrap();
    let e = saga::attach_operation(&app, &third, "monitor", "agent-two", "again")
        .await
        .unwrap_err();
    assert_eq!(e.error, "agent_already_bound");
}
