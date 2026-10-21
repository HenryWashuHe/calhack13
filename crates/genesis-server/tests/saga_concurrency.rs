use chrono::Utc;
use genesis_core::change::{KeyAllocation, OrgChangeProposal, OrgChangeRecord};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::events::EventKind;
use genesis_core::ids::ChangeId;
use genesis_core::lifecycle::ChangeStatus;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{ProviderError, ProviderSet, ProvisionRequest, RoleProvisioner};
use genesis_server::app::AppState;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use std::{future::Future, pin::Pin};
use tokio::sync::{Notify, Semaphore};

enum Pause {
    Creation,
    Readiness,
}
struct CountingProvider {
    calls: AtomicUsize,
    readiness_calls: AtomicUsize,
    entered: Notify,
    duplicate_creation: Notify,
    release: Semaphore,
    pause: Pause,
}
impl CountingProvider {
    fn new(pause: Pause) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            readiness_calls: AtomicUsize::new(0),
            entered: Notify::new(),
            duplicate_creation: Notify::new(),
            release: Semaphore::new(0),
            pause,
        })
    }
}
type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

impl RoleProvisioner for CountingProvider {
    fn provider(&self) -> Provider {
        Provider::Elastic
    }
    fn provision<'a, 'b, 'future>(
        &'a self,
        request: &'b ProvisionRequest,
    ) -> ProviderFuture<'future, AgentBinding>
    where
        'a: 'future,
        'b: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if count > 1 {
                self.duplicate_creation.notify_one();
            }
            if matches!(self.pause, Pause::Creation) {
                self.entered.notify_one();
                self.release.acquire().await.unwrap().forget();
            }
            Ok(AgentBinding {
                provider: Provider::Elastic,
                provider_agent_id: format!("native-agent-{count}"),
                config_digest: request.config_digest(),
                conversation_ref: None,
            })
        })
    }
    fn check_ready<'a, 'b, 'future>(&'a self, _: &'b AgentBinding) -> ProviderFuture<'future, bool>
    where
        'a: 'future,
        'b: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            let count = self.readiness_calls.fetch_add(1, Ordering::SeqCst);
            if matches!(self.pause, Pause::Readiness) && count == 0 {
                self.entered.notify_one();
                // The cancellation test interrupts precisely after the binding is
                // persisted. A new driver must reuse it, not create another agent.
                std::future::pending::<()>().await;
            }
            Ok(true)
        })
    }
}

fn app(provider: Arc<CountingProvider>) -> AppState {
    let mut config = OrgConfig::dev("concurrency", "Concurrency");
    config.provision_timeout = Duration::from_secs(30);
    build_org(
        &config,
        Some(ProviderSet::new().with_provisioner(Provider::Elastic, provider)),
    )
    .unwrap()
}

async fn change(app: &AppState) -> OrgChangeRecord {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Delegate continuing work", "roles":[{"key":"lead","name":"Lead","charter":"Coordinate",
        "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
        "responsibilities":[{"purpose":"Coordinate","owner":"lead"}]
    })).unwrap();
    let mission = app
        .mutate(EventMeta::default(), |state| {
            engine::bootstrap_mission(state, &spec, Utc::now())
                .map_err(genesis_server::error::validation_error)
        })
        .await
        .unwrap()
        .value;
    let observation = app
        .mutate(EventMeta::default(), |state| {
            engine::record_observation(state, &mission, "test", "New requirement", None, Utc::now())
                .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let requirement = app
        .mutate(EventMeta::default(), |state| {
            engine::record_requirement(
                state,
                &mission,
                "Needs a specialist",
                std::slice::from_ref(&observation),
                "Work owned",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision":state.revision, "trigger_evidence_ids":[observation],
        "rationale":"A specialist owns continuing work", "budget_limit_cents":0,
        "operations":[{"op":"CreateRole","key":"specialist","name":"Specialist","charter":"Own continuing work",
          "authority":{"scopes":["mission.coordinate"],"budget_cents":0},"reports_to":null,
          "provider":"elastic","justification":"independent_ownership"},
          {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"new-work",
           "requirement_id":requirement,"purpose":"Own continuing work","trigger":"Changes","termination":"Mission ends"},
           "owner":{"kind":"new","key":"specialist"}}]
    })).unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    engine::validate(
        &state,
        &proposal,
        &allocation,
        &engine::local_catalog(&state),
        Utc::now(),
    )
    .unwrap();
    let record = OrgChangeRecord {
        change_id: ChangeId::generate(),
        status: ChangeStatus::Validated,
        proposal,
        allocation,
        provisioned: Default::default(),
        ready: Default::default(),
        errors: vec![],
        committed_revision: None,
        submitted_by: "operator".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    app.store.insert_change("parallel-change", &record).unwrap();
    record
}

fn spawn_run(
    app: &AppState,
    id: &ChangeId,
) -> tokio::task::JoinHandle<genesis_server::error::ApiResult<OrgChangeRecord>> {
    let app = app.clone();
    let id = id.clone();
    tokio::spawn(async move { saga::run(&app, &id).await })
}
async fn entered(provider: &CountingProvider) {
    tokio::time::timeout(Duration::from_secs(2), provider.entered.notified())
        .await
        .unwrap();
}
fn assert_one_commit(app: &AppState, record: &OrgChangeRecord) {
    let events = app.store.events_after(0, 100).unwrap();
    for status in [
        ChangeStatus::Provisioning,
        ChangeStatus::Ready,
        ChangeStatus::Committed,
    ] {
        assert_eq!(events.iter().filter(|event| matches!(&event.kind, EventKind::ChangeStatusChanged { change_id, to, .. } if change_id==&record.change_id && *to==status)).count(), 1, "one transition to {status:?}");
    }
    assert_eq!(events.iter().filter(|event| matches!(&event.kind, EventKind::RoleProvisioned { change_id, .. } if change_id==&record.change_id)).count(), 1);
    assert_eq!(
        app.current_state().unwrap().revision,
        record.proposal.expected_org_revision + 1
    );
    assert_eq!(
        app.store.get_change(&record.change_id).unwrap().status,
        ChangeStatus::Committed
    );
}

#[tokio::test]
async fn concurrent_run_recovery_and_retry_share_one_driver_and_commit() {
    let provider = CountingProvider::new(Pause::Creation);
    let app = app(provider.clone());
    let record = change(&app).await;
    let first = spawn_run(&app, &record.change_id);
    entered(&provider).await;
    // Holding the per-change gate must not hold the kernel while the native
    // provider is awaiting I/O, so unrelated kernel work remains possible.
    drop(
        tokio::time::timeout(Duration::from_millis(100), app.kernel.lock())
            .await
            .unwrap(),
    );
    let second = spawn_run(&app.clone(), &record.change_id);
    assert_eq!(saga::resume_pending(&app).await.unwrap(), 1);
    let (_, retried) = saga::submit(&app, "operator", "parallel-change", record.proposal.clone())
        .await
        .unwrap();
    assert_eq!(retried.status, ChangeStatus::Provisioning);
    assert!(tokio::time::timeout(
        Duration::from_millis(30),
        provider.duplicate_creation.notified()
    )
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(!second.is_finished());
    provider.release.add_permits(1);
    for worker in [first, second] {
        let finished = tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(finished.status, ChangeStatus::Committed);
        assert_eq!(
            finished.provisioned["specialist"].provider_agent_id,
            "native-agent-1"
        );
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.readiness_calls.load(Ordering::SeqCst), 1);
    assert_one_commit(&app, &record);
}

#[tokio::test]
async fn cancelling_runner_releases_gate_and_waiter_reuses_saved_binding() {
    let provider = CountingProvider::new(Pause::Readiness);
    let app = app(provider.clone());
    let record = change(&app).await;
    let first = spawn_run(&app, &record.change_id);
    entered(&provider).await;
    assert_eq!(
        app.store.get_change(&record.change_id).unwrap().provisioned["specialist"]
            .provider_agent_id,
        "native-agent-1"
    );
    let second = spawn_run(&app, &record.change_id);
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let finished = tokio::time::timeout(Duration::from_secs(2), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(finished.status, ChangeStatus::Committed);
    assert_eq!(
        finished.provisioned["specialist"].provider_agent_id,
        "native-agent-1"
    );
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "known binding survives cancellation"
    );
    assert_eq!(provider.readiness_calls.load(Ordering::SeqCst), 2);
    assert_one_commit(&app, &record);
}

#[tokio::test]
async fn distinct_changes_can_provision_concurrently_and_kernel_resolves_revision_conflict() {
    let provider = CountingProvider::new(Pause::Creation);
    let app = app(provider.clone());
    let first_record = change(&app).await;
    // Two independent proposals may race for the same requirement. Their
    // provider calls should overlap; the existing commit revision check decides
    // which proposal becomes organization state.
    let mut other_record = first_record.clone();
    other_record.change_id = ChangeId::generate();
    other_record.allocation = KeyAllocation::allocate(&other_record.proposal);
    app.store
        .insert_change("other-change", &other_record)
        .unwrap();
    let first = spawn_run(&app, &first_record.change_id);
    entered(&provider).await;
    let second = spawn_run(&app, &other_record.change_id);
    entered(&provider).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    provider.release.add_permits(2);
    let mut committed = 0;
    let mut rejected = 0;
    for worker in [first, second] {
        let result = tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match result.status {
            ChangeStatus::Committed => committed += 1,
            ChangeStatus::Rejected => rejected += 1,
            other => panic!("unexpected state {other:?}"),
        }
        assert!(result.provisioned.contains_key("specialist"));
    }
    assert_eq!((committed, rejected), (1, 1));
    assert_eq!(
        app.current_state().unwrap().revision,
        first_record.proposal.expected_org_revision + 1
    );
}
