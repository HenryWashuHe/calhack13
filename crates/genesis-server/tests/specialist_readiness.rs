//! The pending/readiness hook for asynchronously created agents (CREAO-style
//! `SpecialistRuntime`), driven by a scripted runtime and readiness verifier.

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
use genesis_server::specialist::{Readiness, ReadinessVerifier};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum Step {
    Pending,
    Created,
    Failed,
    Reconcile,
    Error,
}

struct Scripted {
    org: String,
    script: Mutex<VecDeque<Step>>,
    current: Mutex<Option<SpecialistProgress>>,
    prepares: AtomicUsize,
    advances: AtomicUsize,
}

impl Scripted {
    fn new(steps: &[Step]) -> Arc<Self> {
        Arc::new(Self {
            org: "org_a".into(),
            script: Mutex::new(steps.iter().copied().collect()),
            current: Mutex::new(None),
            prepares: AtomicUsize::new(0),
            advances: AtomicUsize::new(0),
        })
    }
}

const OP: &str = "creao-role-op-1";

#[async_trait]
impl SpecialistRuntime for Scripted {
    fn provider(&self) -> Provider {
        Provider::Creao
    }
    fn org_id(&self) -> &str {
        &self.org
    }
    fn prepare_role(&self, req: &ProvisionRequest) -> Result<SpecialistProgress, ProviderError> {
        assert_eq!(req.role.provider, Provider::Creao);
        self.prepares.fetch_add(1, Ordering::SeqCst);
        let p = SpecialistProgress::Pending {
            operation_id: OP.into(),
            run_id: None,
            native_status: None,
        };
        *self.current.lock().unwrap() = Some(p.clone());
        Ok(p)
    }
    fn prepare_job(&self, _: &Job, _: &AgentBinding) -> Result<SpecialistProgress, ProviderError> {
        Err(ProviderError::Rejected("not used".into()))
    }
    fn get(&self, _: &str) -> Result<SpecialistProgress, ProviderError> {
        self.current
            .lock()
            .unwrap()
            .clone()
            .ok_or(ProviderError::Rejected("unknown".into()))
    }
    async fn advance(
        &self,
        operation_id: &str,
        _retry: bool,
    ) -> Result<SpecialistProgress, ProviderError> {
        assert_eq!(operation_id, OP);
        self.advances.fetch_add(1, Ordering::SeqCst);
        let step = {
            let mut s = self.script.lock().unwrap();
            if s.len() > 1 {
                s.pop_front().unwrap()
            } else {
                *s.front().unwrap()
            }
        };
        let run = Some("run-1".to_owned());
        let p = match step {
            Step::Pending => SpecialistProgress::Pending {
                operation_id: OP.into(),
                run_id: run,
                native_status: Some("running".into()),
            },
            Step::Created => SpecialistProgress::Created {
                operation_id: OP.into(),
                run_id: "run-1".into(),
                provider_agent_id: "creao-agent-42".into(),
                config_digest: "sha256:cfg".into(),
                readiness_verified: false,
            },
            Step::Failed => SpecialistProgress::Failed {
                operation_id: OP.into(),
                run_id: run,
            },
            Step::Reconcile => SpecialistProgress::NeedsReconciliation {
                operation_id: OP.into(),
                run_id: None,
            },
            Step::Error => return Err(ProviderError::Rejected("transient read failure".into())),
        };
        *self.current.lock().unwrap() = Some(p.clone());
        Ok(p)
    }
}

struct Verifier {
    answers: Mutex<VecDeque<Readiness>>,
    calls: AtomicUsize,
}

impl Verifier {
    fn new(answers: Vec<Readiness>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl ReadinessVerifier for Verifier {
    async fn verify(
        &self,
        role: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        assert_eq!(role.key, "monitor");
        assert_eq!(binding.provider_agent_id, "creao-agent-42");
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut a = self.answers.lock().unwrap();
        Ok(if a.len() > 1 {
            a.pop_front().unwrap()
        } else {
            a.front().cloned().unwrap()
        })
    }
}

fn org(
    runtime: Arc<Scripted>,
    verifier: Option<Arc<Verifier>>,
    db: Option<std::path::PathBuf>,
) -> AppState {
    let mut cfg = OrgConfig::dev("org_a", "Org A");
    cfg.db = db;
    let providers = ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .with_specialist(runtime);
    let app = build_org(&cfg, Some(providers)).unwrap();
    match verifier {
        Some(v) => app.with_readiness_verifier(Provider::Creao, v),
        None => app,
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

fn monitor_role(app: &AppState) -> Option<genesis_core::model::Role> {
    app.current_state()
        .unwrap()
        .roles
        .into_values()
        .find(|r| r.name == "Venue monitor")
}

fn progress_states(app: &AppState) -> Vec<String> {
    app.store
        .events_after(0, 1000)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind.type_name() == "provisioning_progress")
        .map(|e| {
            serde_json::to_value(&e.kind).unwrap()["payload"]["state"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn pending_creation_waits_then_commits_after_proven_readiness() {
    let rt = Scripted::new(&[Step::Pending, Step::Pending, Step::Created]);
    let v = Verifier::new(vec![
        Readiness::Pending,
        Readiness::Ready {
            evidence_ref: "receipt://gateway/1".into(),
        },
    ]);
    // No background poller here: drive passes explicitly.
    let app = org(rt.clone(), Some(v.clone()), None);
    let id = submit(&app).await;

    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::Pending);
    assert_eq!(r.operations["monitor"].run_id.as_deref(), Some("run-1"));
    assert!(
        monitor_role(&app).is_none(),
        "organization unchanged while pending"
    );

    let r = drive(&app, &id).await; // still pending
    assert_eq!(r.status, ChangeStatus::Provisioning);
    let r = drive(&app, &id).await; // created; readiness pending
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::AwaitingReadiness);
    assert_eq!(r.provisioned["monitor"].provider_agent_id, "creao-agent-42");
    assert!(monitor_role(&app).is_none());

    let r = drive(&app, &id).await; // readiness proven
    assert_eq!(r.status, ChangeStatus::Committed);
    assert_eq!(r.operations["monitor"].state, S::Ready);
    assert_eq!(
        r.operations["monitor"].readiness_evidence.as_deref(),
        Some("receipt://gateway/1")
    );
    let role = monitor_role(&app).unwrap();
    assert_eq!(role.binding.unwrap().provider_agent_id, "creao-agent-42");

    assert_eq!(rt.prepares.load(Ordering::SeqCst), 1);
    assert_eq!(
        rt.advances.load(Ordering::SeqCst),
        3,
        "no native calls after creation"
    );
    assert_eq!(v.calls.load(Ordering::SeqCst), 2);
    // Prepared, then the run ID learned, then created, then ready; the second
    // unchanged pending observation emits nothing.
    assert_eq!(
        progress_states(&app),
        vec!["pending", "pending", "awaiting_readiness", "ready"]
    );
}

#[tokio::test]
async fn created_agent_without_a_verifier_never_activates() {
    let rt = Scripted::new(&[Step::Created]);
    let app = org(rt.clone(), None, None);
    let id = submit(&app).await;
    let r = drive(&app, &id).await;
    let r2 = drive(&app, &id).await;
    for r in [r, r2] {
        assert_eq!(r.status, ChangeStatus::Provisioning);
        assert_eq!(r.operations["monitor"].state, S::AwaitingReadiness);
        assert!(r.operations["monitor"]
            .attention
            .as_deref()
            .unwrap()
            .contains("no readiness verifier"));
    }
    assert!(monitor_role(&app).is_none());
    assert_eq!(rt.advances.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ambiguous_creation_is_kept_for_reconciliation_without_retrying() {
    let rt = Scripted::new(&[Step::Reconcile]);
    let app = org(
        rt.clone(),
        Some(Verifier::new(vec![Readiness::Pending])),
        None,
    );
    let id = submit(&app).await;
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert_eq!(r.operations["monitor"].state, S::NeedsReconciliation);
    drive(&app, &id).await;
    drive(&app, &id).await;
    assert_eq!(
        rt.advances.load(Ordering::SeqCst),
        1,
        "no automatic retry of an unknown outcome"
    );
}

#[tokio::test]
async fn failed_operation_fails_the_change_with_its_ids() {
    let rt = Scripted::new(&[Step::Pending, Step::Failed]);
    let app = org(rt, Some(Verifier::new(vec![Readiness::Pending])), None);
    let id = submit(&app).await;
    drive(&app, &id).await;
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Failed);
    assert_eq!(r.operations["monitor"].state, S::Failed);
    let text = serde_json::to_string(&app.store.events_after(0, 1000).unwrap()).unwrap();
    assert!(text.contains(OP) && text.contains("run-1"));
}

#[tokio::test]
async fn failed_readiness_fails_the_change_and_keeps_the_agent_id() {
    let rt = Scripted::new(&[Step::Created]);
    let app = org(
        rt,
        Some(Verifier::new(vec![Readiness::NotReady(
            "bridge skill missing".into(),
        )])),
        None,
    );
    let id = submit(&app).await;
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Failed);
    assert_eq!(r.provisioned["monitor"].provider_agent_id, "creao-agent-42");
    let text = serde_json::to_string(&app.store.events_after(0, 1000).unwrap()).unwrap();
    assert!(text.contains("bridge skill missing") && text.contains("creao-agent-42"));
}

#[tokio::test]
async fn transient_advance_errors_are_reported_and_retried() {
    let rt = Scripted::new(&[Step::Error, Step::Error, Step::Created]);
    let app = org(
        rt.clone(),
        Some(Verifier::new(vec![Readiness::Ready {
            evidence_ref: "r".into(),
        }])),
        None,
    );
    let id = submit(&app).await;
    let r = drive(&app, &id).await; // both the read and its retry fail
    assert_eq!(r.status, ChangeStatus::Provisioning);
    assert!(r.operations["monitor"]
        .attention
        .as_deref()
        .unwrap()
        .contains("transient"));
    let r = drive(&app, &id).await;
    assert_eq!(r.status, ChangeStatus::Committed);
    assert_eq!(rt.prepares.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn restart_resumes_the_saved_operation_without_preparing_again() {
    let dir = tempfile::tempdir().unwrap();
    let db = Some(dir.path().join("org.db"));
    let rt = Scripted::new(&[Step::Pending, Step::Created]);
    let v = Verifier::new(vec![Readiness::Ready {
        evidence_ref: "r".into(),
    }]);
    let app = org(rt.clone(), Some(v.clone()), db.clone());
    let id = submit(&app).await;
    assert_eq!(drive(&app, &id).await.status, ChangeStatus::Provisioning);
    drop(app);

    let restarted = org(rt.clone(), Some(v), db);
    assert_eq!(saga::resume_pending(&restarted).await.unwrap(), 1);
    let mut status = ChangeStatus::Provisioning;
    for _ in 0..100 {
        status = restarted.store.get_change(&id).unwrap().status;
        if status == ChangeStatus::Committed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, ChangeStatus::Committed);
    assert_eq!(
        rt.prepares.load(Ordering::SeqCst),
        1,
        "the saved operation is reused"
    );
}

#[tokio::test]
async fn the_server_poller_drives_pending_changes_to_commit() {
    let rt = Scripted::new(&[Step::Pending, Step::Pending, Step::Created]);
    let app = org(
        rt,
        Some(Verifier::new(vec![Readiness::Ready {
            evidence_ref: "r".into(),
        }])),
        None,
    );
    genesis_server::spawn_org(app.clone(), "127.0.0.1:0")
        .await
        .unwrap();
    let id = submit(&app).await;
    let mut status = ChangeStatus::Provisioning;
    for _ in 0..100 {
        status = app.store.get_change(&id).unwrap().status;
        if status == ChangeStatus::Committed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status, ChangeStatus::Committed);
    assert!(monitor_role(&app).is_some());
}

#[tokio::test]
async fn operator_can_abandon_an_ambiguous_operation() {
    let rt = Scripted::new(&[Step::Reconcile]);
    let app = org(
        rt.clone(),
        Some(Verifier::new(vec![Readiness::Pending])),
        None,
    );
    let id = submit(&app).await;
    let r = drive(&app, &id).await;
    assert_eq!(r.operations["monitor"].state, S::NeedsReconciliation);

    let r = saga::abandon_operation(&app, &id, "monitor", "no native agent found\u{7}")
        .await
        .unwrap();
    assert_eq!(r.status, ChangeStatus::Failed);
    assert_eq!(r.operations["monitor"].state, S::Failed);
    assert_eq!(r.operations["monitor"].operation_id, OP, "IDs retained");
    assert!(r.operations["monitor"]
        .attention
        .as_deref()
        .unwrap()
        .ends_with("no native agent found"));
    assert!(monitor_role(&app).is_none(), "organization unchanged");
    assert_eq!(rt.advances.load(Ordering::SeqCst), 1, "nothing retried");

    let again = saga::abandon_operation(&app, &id, "monitor", "again")
        .await
        .unwrap_err();
    assert_eq!(again.status, axum::http::StatusCode::CONFLICT);
    let missing = saga::abandon_operation(&app, &id, "nobody", "x")
        .await
        .unwrap_err();
    assert_eq!(missing.status, axum::http::StatusCode::CONFLICT); // change already FAILED
}
