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
use genesis_server::specialist::{Readiness, ReadinessVerifier};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::json;
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
    fn prepare_role(
        &self,
        request: &ProvisionRequest,
    ) -> Result<SpecialistProgress, ProviderError> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Ok(SpecialistProgress::Pending {
            operation_id: format!("{OP}-{}", request.role.key),
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
    async fn advance(
        &self,
        operation_id: &str,
        _: bool,
    ) -> Result<SpecialistProgress, ProviderError> {
        self.advances.fetch_add(1, Ordering::SeqCst);
        Ok(SpecialistProgress::NeedsReconciliation {
            operation_id: operation_id.into(),
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
    let req_two = app
        .mutate(EventMeta::default(), |s| {
            genesis_core::engine::record_requirement(
                s,
                &mission,
                "Independently audit venue updates",
                std::slice::from_ref(&obs),
                "Every update independently audited",
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
             "owner":{"kind":"new","key":"monitor"}},
            {"op":"CreateRole","key":"auditor","name":"Venue auditor","charter":"Independently audit updates",
             "authority":{"scopes":["mission.coordinate"],"budget_cents":0},"reports_to":null,
             "provider":"creao","justification":"independent_ownership"},
            {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"auditing",
             "requirement_id":req_two,"purpose":"Audit venue updates","trigger":"update","termination":"event held"},
             "owner":{"kind":"new","key":"auditor"}}
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

#[tokio::test]
async fn review_same_native_agent_must_not_attach_to_two_roles_in_one_change() {
    let rt = Arc::new(Ambiguous::default());
    let app = org(rt.clone()).with_readiness_verifier(
        Provider::Creao,
        fixed(Readiness::Ready {
            evidence_ref: "fixture-readiness".into(),
        }),
    );
    let id = ambiguous_change(&app).await;
    let saved = app.store.get_change(&id).unwrap();
    assert_eq!(saved.operations["auditor"].state, S::NeedsReconciliation);
    assert_ne!(
        saved.operations["monitor"].operation_id,
        saved.operations["auditor"].operation_id
    );
    saga::attach_operation(&app, &id, "monitor", FOUND, "found monitor")
        .await
        .unwrap();
    let second = saga::attach_operation(&app, &id, "auditor", FOUND, "same native agent").await;
    if second.is_ok() {
        let final_record = drive(&app, &id).await;
        let duplicate_roles = app
            .current_state()
            .unwrap()
            .roles
            .values()
            .filter(|role| {
                role.binding
                    .as_ref()
                    .is_some_and(|binding| binding.provider_agent_id == FOUND)
            })
            .count();
        assert_ne!(
            duplicate_roles, 2,
            "same agent committed as two independent roles; change={:?}",
            final_record.status
        );
    }
    let error = second.expect_err("one native agent may own only one role binding");
    assert_eq!(
        (error.status.as_u16(), error.error.as_str()),
        (409, "agent_already_bound")
    );
}
