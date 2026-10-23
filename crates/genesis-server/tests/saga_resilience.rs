//! A transient storage failure while a saga is running must not strand the
//! change until the next process restart. The background driver retries, and
//! a periodic sweep resumes any non-terminal change that has no driver.
use chrono::Utc;
use genesis_core::change::{OrgChangeProposal, OrgChangeRecord};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::lifecycle::ChangeStatus;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{ProviderError, ProviderSet, ProvisionRequest, RoleProvisioner};
use genesis_server::app::AppState;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

struct Slow;
type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;
impl RoleProvisioner for Slow {
    fn provider(&self) -> Provider {
        Provider::Elastic
    }
    fn provision<'a, 'b, 'f>(
        &'a self,
        req: &'b ProvisionRequest,
    ) -> ProviderFuture<'f, AgentBinding>
    where
        'a: 'f,
        'b: 'f,
        Self: 'f,
    {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            Ok(AgentBinding {
                provider: Provider::Elastic,
                provider_agent_id: format!("elastic-{}", req.role.role_id),
                config_digest: req.config_digest(),
                conversation_ref: None,
            })
        })
    }
    fn check_ready<'a, 'b, 'f>(&'a self, _: &'b AgentBinding) -> ProviderFuture<'f, bool>
    where
        'a: 'f,
        'b: 'f,
        Self: 'f,
    {
        Box::pin(async { Ok(true) })
    }
}

async fn setup(db: std::path::PathBuf) -> (AppState, OrgChangeProposal) {
    let mut config = OrgConfig::dev("org_resilience", "Resilience");
    config.db = Some(db);
    let app = build_org(
        &config,
        Some(ProviderSet::new().with_provisioner(Provider::Elastic, Arc::new(Slow))),
    )
    .unwrap();
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": "Run the event",
        "roles": [{"key":"lead","name":"Lead","charter":"Coordinate",
                   "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
        "responsibilities": [{"purpose":"Coordinate","owner":"lead"}]
    }))
    .unwrap();
    let mission = app
        .mutate(EventMeta::default(), |s| {
            engine::bootstrap_mission(s, &spec, Utc::now())
                .map_err(genesis_server::error::validation_error)
        })
        .await
        .unwrap()
        .value;
    let obs = app
        .mutate(EventMeta::default(), |s| {
            engine::record_observation(s, &mission, "test", "Venue closed", None, Utc::now())
                .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let req = app
        .mutate(EventMeta::default(), |s| {
            engine::record_requirement(
                s,
                &mission,
                "Relocate",
                std::slice::from_ref(&obs),
                "venue reserved",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision": state.revision, "trigger_evidence_ids": [obs],
        "rationale": "Relocation needs an owner",
        "operations": [
          {"op":"CreateRole","key":"reloc","name":"Relocation lead","charter":"Relocate",
           "authority":{"scopes":["ledger.reserve_venue"],"budget_cents":0},
           "provider":"elastic","justification":"independent_ownership"},
          {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"r",
           "requirement_id":req,"purpose":"Relocate the event","trigger":"t","termination":"e"},
           "owner":{"kind":"new","key":"reloc"}}]
    }))
    .unwrap();
    (app, proposal)
}

async fn settle(app: &AppState, record: &OrgChangeRecord, within: Duration) -> ChangeStatus {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let status = app.store.get_change(&record.change_id).unwrap().status;
        if matches!(
            status,
            ChangeStatus::Committed | ChangeStatus::Failed | ChangeStatus::Rejected
        ) || tokio::time::Instant::now() > deadline
        {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn outage(db: &std::path::Path, trigger: &str, ms: u64) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute_batch(trigger).unwrap();
    tokio::time::sleep(Duration::from_millis(ms)).await;
    conn.execute_batch("DROP TRIGGER IF EXISTS fixture_outage;")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_record_outage_mid_provisioning_does_not_strand_the_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let (app, proposal) = setup(db.clone()).await;
    let (_, record) = saga::submit(&app, "agent", "c1", proposal).await.unwrap();
    // The binding save (after ~300 ms of provisioning) falls inside the outage.
    outage(
        &db,
        "CREATE TRIGGER fixture_outage BEFORE UPDATE ON org_changes \
         BEGIN SELECT RAISE(ABORT, 'fixture storage outage'); END;",
        900,
    )
    .await;
    let status = settle(&app, &record, Duration::from_secs(15)).await;
    assert_eq!(
        status,
        ChangeStatus::Committed,
        "change stranded after a transient outage"
    );
    let rec = app.store.get_change(&record.change_id).unwrap();
    let state = app.current_state().unwrap();
    assert_eq!(rec.committed_revision, Some(state.revision));
    assert!(genesis_core::audit::audit(&state).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snapshot_outage_at_commit_does_not_strand_the_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let (app, proposal) = setup(db.clone()).await;
    // Block new revisions before the saga reaches commit.
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fixture_outage BEFORE INSERT ON org_snapshots \
         BEGIN SELECT RAISE(ABORT, 'fixture storage outage'); END;",
    )
    .unwrap();
    let (_, record) = saga::submit(&app, "agent", "c2", proposal).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let mid = app.store.get_change(&record.change_id).unwrap().status;
    assert_ne!(
        mid,
        ChangeStatus::Committed,
        "the outage must actually block the commit"
    );
    conn.execute_batch("DROP TRIGGER fixture_outage;").unwrap();
    let status = settle(&app, &record, Duration::from_secs(15)).await;
    assert_eq!(
        status,
        ChangeStatus::Committed,
        "change stranded after a commit-time outage"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweeper_drives_an_orphaned_change_once() {
    use genesis_core::change::KeyAllocation;
    use genesis_core::ids::ChangeId;
    let dir = tempfile::tempdir().unwrap();
    let (app, proposal) = setup(dir.path().join("org.db")).await;
    // A validated change with no driver, as left by a process that stopped
    // between recording the change and starting its saga.
    let state = app.current_state().unwrap();
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
        operations: Default::default(),
        errors: vec![],
        committed_revision: None,
        submitted_by: "agent".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    app.store.insert_change("orphan", &record).unwrap();
    assert_eq!(saga::sweep_once(&app).unwrap(), 1);
    // The driver is still provisioning (300 ms); a second sweep must not start another.
    assert_eq!(saga::sweep_once(&app).unwrap(), 0);
    assert_eq!(
        settle(&app, &record, Duration::from_secs(10)).await,
        ChangeStatus::Committed
    );
    assert_eq!(
        saga::sweep_once(&app).unwrap(),
        0,
        "terminal changes are left alone"
    );
}
