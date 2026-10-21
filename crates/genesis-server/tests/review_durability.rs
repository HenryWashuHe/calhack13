//! Regression tests for the PR #3 durability findings (docs/reviews/PR-3.md).
//! Adapted from the reviewer's probes, which asserted the faulty behavior;
//! these assert the intended behavior.

use axum::http::StatusCode;
use chrono::Utc;
use genesis_core::change::{KeyAllocation, OrgChangeProposal, OrgChangeRecord};
use genesis_core::engine::MissionBootstrap;
use genesis_core::ids::*;
use genesis_core::jobs::{DispatchRequest, Job};
use genesis_core::lifecycle::{ChangeStatus, JobStatus};
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{ProviderError, ProviderSet, ProvisionRequest, RoleProvisioner};
use genesis_server::app::AppState;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

async fn seed(app: &AppState) -> MissionId {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Review durability", "roles":[{"key":"lead","name":"Lead","charter":"Own work",
        "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
        "responsibilities":[{"purpose":"Coordinate work","owner":"lead"}]
    }))
    .unwrap();
    app.mutate(EventMeta::default(), |state| {
        genesis_core::engine::bootstrap_mission(state, &spec, Utc::now())
            .map_err(genesis_server::error::validation_error)
    })
    .await
    .unwrap()
    .value
}

fn dispatch_request(app: &AppState) -> DispatchRequest {
    let state = app.current_state().unwrap();
    DispatchRequest {
        client_request_id: "durable-job-request".into(),
        role_id: state.roles.keys().next().unwrap().clone(),
        responsibility_id: state.responsibilities.keys().next().unwrap().clone(),
        task: "Perform work exactly once".into(),
        input: json!({}),
        required_scope: Some("mission.coordinate".into()),
        capability: None,
        provider: Some(Provider::Elastic),
        deadline: None,
    }
}

async fn wait_terminal(app: &AppState, id: &ChangeId) -> OrgChangeRecord {
    for _ in 0..200 {
        let r = app.store.get_change(id).unwrap();
        if matches!(
            r.status,
            ChangeStatus::Committed | ChangeStatus::Failed | ChangeStatus::Rejected
        ) {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("change {id} did not reach a terminal state");
}

/// Finding 4: a failed dispatch transaction leaves no job behind, and a retry
/// with the same request ID is delivered.
#[tokio::test]
async fn failed_dispatch_transaction_leaves_no_job_and_retry_is_delivered() {
    let directory = tempfile::tempdir().unwrap();
    let mut cfg = OrgConfig::dev("review", "Review");
    let path = directory.path().join("org.db");
    cfg.db = Some(path.clone());
    let app = build_org(&cfg, None).unwrap();
    seed(&app).await;
    let request = dispatch_request(&app);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/v1/jobs", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let http = reqwest::Client::new();
    let fault = rusqlite::Connection::open(path).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER review_fail_outbox BEFORE INSERT ON outbox \
             BEGIN SELECT RAISE(ABORT, 'simulated commit failure'); END;",
        )
        .unwrap();
    let response = http
        .post(&endpoint)
        .bearer_auth(&cfg.tokens.operator)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(app.store.list_jobs().unwrap().is_empty(), "job rolled back");
    assert!(app.store.list_outbox(100).unwrap().is_empty());

    fault
        .execute_batch("DROP TRIGGER review_fail_outbox;")
        .unwrap();
    let response = http
        .post(&endpoint)
        .bearer_auth(&cfg.tokens.operator)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    let job: Job = response.json().await.unwrap();
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(app.store.list_jobs().unwrap().len(), 1);
    assert!(
        !app.store.list_outbox(100).unwrap().is_empty(),
        "retried job is enqueued for delivery"
    );
    assert!(app
        .store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.kind.type_name() == "job_dispatched"));
    server.abort();
}

async fn observation(app: &AppState, mission: &MissionId, text: &str) -> ObservationId {
    app.mutate(EventMeta::default(), |state| {
        genesis_core::engine::record_observation(
            state,
            mission,
            "public-fixture",
            text,
            None,
            Utc::now(),
        )
        .map_err(Into::into)
    })
    .await
    .unwrap()
    .value
}

fn root_proposal(app: &AppState, obs: &ObservationId) -> OrgChangeProposal {
    let state = app.current_state().unwrap();
    serde_json::from_value(json!({
        "expected_org_revision":state.revision, "trigger_evidence_ids":[obs], "rationale":"Keep lead at root",
        "budget_limit_cents":0, "operations":[{"op":"ChangeReportingEdge",
            "role":{"kind":"existing","role_id":state.roles.keys().next().unwrap()},"reports_to":null}]
    }))
    .unwrap()
}

/// Finding 5: a failed change transaction stores nothing, and a retry with the
/// same request ID progresses to a terminal state.
#[tokio::test]
async fn failed_change_transaction_stores_nothing_and_retry_progresses() {
    let directory = tempfile::tempdir().unwrap();
    let mut cfg = OrgConfig::dev("review", "Review");
    let path = directory.path().join("org.db");
    cfg.db = Some(path.clone());
    let app = build_org(&cfg, None).unwrap();
    let mission = seed(&app).await;
    let obs = observation(&app, &mission, "Lead should remain at root").await;
    let proposal = root_proposal(&app, &obs);

    let fault = rusqlite::Connection::open(path).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER review_fail_events BEFORE INSERT ON events \
             BEGIN SELECT RAISE(ABORT, 'simulated commit failure'); END;",
        )
        .unwrap();
    let error = saga::submit(&app, "operator", "durable-change-request", proposal.clone())
        .await
        .unwrap_err();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        app.store.list_changes().unwrap().is_empty(),
        "no change record without its events"
    );
    fault
        .execute_batch("DROP TRIGGER review_fail_events;")
        .unwrap();

    let restarted = build_org(&cfg, None).unwrap();
    assert_eq!(saga::resume_pending(&restarted).await.unwrap(), 0);
    let (_, record) = saga::submit(&restarted, "operator", "durable-change-request", proposal)
        .await
        .unwrap();
    let done = wait_terminal(&restarted, &record.change_id).await;
    assert_eq!(done.status, ChangeStatus::Committed);
}

/// Finding 5: a record left PROPOSED by an older build is picked up both by
/// restart recovery and by a retried request.
#[tokio::test]
async fn stranded_proposed_record_is_resumed() {
    let directory = tempfile::tempdir().unwrap();
    let mut cfg = OrgConfig::dev("review", "Review");
    cfg.db = Some(directory.path().join("org.db"));
    let app = build_org(&cfg, None).unwrap();
    let mission = seed(&app).await;
    let obs = observation(&app, &mission, "Lead should remain at root").await;
    let proposal = root_proposal(&app, &obs);
    let now = Utc::now();
    let stranded = |id: &str| OrgChangeRecord {
        change_id: ChangeId::new(id),
        status: ChangeStatus::Proposed,
        allocation: KeyAllocation::allocate(&proposal),
        proposal: proposal.clone(),
        provisioned: Default::default(),
        ready: Default::default(),
        errors: vec![],
        committed_revision: None,
        submitted_by: "operator".into(),
        created_at: now,
        updated_at: now,
    };
    app.store
        .insert_change("legacy-request", &stranded("chg-legacy"))
        .unwrap();

    // Retry path.
    let (_, record) = saga::submit(&app, "operator", "legacy-request", proposal.clone())
        .await
        .unwrap();
    assert_eq!(record.change_id, ChangeId::new("chg-legacy"));
    assert_ne!(record.status, ChangeStatus::Proposed);
    let done = wait_terminal(&app, &record.change_id).await;
    assert_eq!(done.status, ChangeStatus::Committed);

    // Restart path: a second stranded record (now stale against the new
    // revision) is still driven to a terminal verdict rather than ignored.
    app.store
        .insert_change("legacy-request-2", &stranded("chg-legacy-2"))
        .unwrap();
    let restarted = build_org(&cfg, None).unwrap();
    assert_eq!(saga::resume_pending(&restarted).await.unwrap(), 1);
    let done = wait_terminal(&restarted, &ChangeId::new("chg-legacy-2")).await;
    assert_ne!(done.status, ChangeStatus::Proposed);
}

struct ReadinessPause {
    calls: AtomicUsize,
    entered: Notify,
    hang: AtomicBool,
    fail_readiness: bool,
}

impl ReadinessPause {
    fn new(hang: bool, fail_readiness: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: Notify::new(),
            hang: AtomicBool::new(hang),
            fail_readiness,
        })
    }
}

impl RoleProvisioner for ReadinessPause {
    fn provider(&self) -> Provider {
        Provider::Elastic
    }
    fn provision<'a, 'b, 'future>(
        &'a self,
        req: &'b ProvisionRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AgentBinding, ProviderError>> + Send + 'future>,
    >
    where
        'a: 'future,
        'b: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            let number = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(AgentBinding {
                provider: Provider::Elastic,
                provider_agent_id: format!("provider-agent-{number}"),
                config_digest: req.config_digest(),
                conversation_ref: None,
            })
        })
    }
    fn check_ready<'a, 'b, 'future>(
        &'a self,
        _binding: &'b AgentBinding,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<bool, ProviderError>> + Send + 'future>,
    >
    where
        'a: 'future,
        'b: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            if self.fail_readiness {
                return Err(ProviderError::Rejected("readiness check failed".into()));
            }
            if self.hang.load(Ordering::SeqCst) {
                self.entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(true)
        })
    }
}

async fn validated_change(app: &AppState) -> OrgChangeRecord {
    let mission = seed(app).await;
    let obs = observation(app, &mission, "New continuing work").await;
    let requirement = app
        .mutate(EventMeta::default(), |state| {
            genesis_core::engine::record_requirement(
                state,
                &mission,
                "New owner required",
                std::slice::from_ref(&obs),
                "Work owned",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision":state.revision, "trigger_evidence_ids":[obs],
        "rationale":"A specialist owns continuing work", "budget_limit_cents":0,
        "operations":[{"op":"CreateRole","key":"specialist","name":"Specialist","charter":"Own continuing work",
          "authority":{"scopes":["mission.coordinate"],"budget_cents":0},"reports_to":null,
          "provider":"elastic","justification":"independent_ownership"},
          {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"new-work",
           "requirement_id":requirement,"purpose":"Own continuing work","trigger":"Changes","termination":"Mission ends"},
           "owner":{"kind":"new","key":"specialist"}}]
    }))
    .unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    genesis_core::engine::validate(
        &state,
        &proposal,
        &allocation,
        &genesis_core::engine::local_catalog(&state),
        Utc::now(),
    )
    .unwrap();
    let now = Utc::now();
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
        created_at: now,
        updated_at: now,
    };
    app.store
        .insert_change("readiness-change", &record)
        .unwrap();
    record
}

/// Finding 6: readiness is bounded by the provisioning timeout, and the
/// created binding is recorded on the failed change.
#[tokio::test]
async fn readiness_is_bounded_by_provision_timeout() {
    let provider = ReadinessPause::new(true, false);
    let providers = ProviderSet::new().with_provisioner(Provider::Elastic, provider.clone());
    let mut cfg = OrgConfig::dev("review", "Review");
    cfg.provision_timeout = Duration::from_millis(50);
    let app = build_org(&cfg, Some(providers)).unwrap();
    let record = validated_change(&app).await;
    let failed = tokio::time::timeout(Duration::from_secs(2), saga::run(&app, &record.change_id))
        .await
        .expect("readiness must not exceed the provisioning timeout")
        .unwrap();
    assert_eq!(failed.status, ChangeStatus::Failed);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        failed.provisioned["specialist"].provider_agent_id,
        "provider-agent-1"
    );
}

/// Finding 6: a binding returned by the provider is persisted before readiness,
/// so an interrupted saga resumes with it instead of creating a second agent.
#[tokio::test]
async fn interrupted_readiness_resumes_with_known_binding() {
    let provider = ReadinessPause::new(true, false);
    let providers = ProviderSet::new().with_provisioner(Provider::Elastic, provider.clone());
    let mut cfg = OrgConfig::dev("review", "Review");
    cfg.provision_timeout = Duration::from_secs(30);
    let app = build_org(&cfg, Some(providers)).unwrap();
    let record = validated_change(&app).await;
    let worker_app = app.clone();
    let change_id = record.change_id.clone();
    let worker = tokio::spawn(async move { saga::run(&worker_app, &change_id).await });
    tokio::time::timeout(Duration::from_secs(2), provider.entered.notified())
        .await
        .unwrap();
    let stored = app.store.get_change(&record.change_id).unwrap();
    assert_eq!(
        stored.provisioned["specialist"].provider_agent_id, "provider-agent-1",
        "binding persisted before readiness"
    );
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());

    provider.hang.store(false, Ordering::SeqCst);
    let resumed = saga::run(&app, &record.change_id).await.unwrap();
    assert_eq!(resumed.status, ChangeStatus::Committed);
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "no reprovisioning"
    );
    assert_eq!(
        resumed.provisioned["specialist"].provider_agent_id,
        "provider-agent-1"
    );
}

/// Finding 7: a readiness failure keeps the created provider identity in the
/// terminal record and its events so it can be cleaned up.
#[tokio::test]
async fn readiness_failure_retains_provider_identity() {
    let provider = ReadinessPause::new(false, true);
    let providers = ProviderSet::new().with_provisioner(Provider::Elastic, provider.clone());
    let cfg = OrgConfig::dev("review", "Review");
    let app = build_org(&cfg, Some(providers)).unwrap();
    let record = validated_change(&app).await;
    let failed = saga::run(&app, &record.change_id).await.unwrap();
    assert_eq!(failed.status, ChangeStatus::Failed);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        failed.provisioned["specialist"].provider_agent_id,
        "provider-agent-1"
    );
    let events = app.store.events_after(0, 100).unwrap();
    assert!(events.iter().any(|e| {
        e.kind.type_name() == "provisioning_failed"
            && serde_json::to_string(&e.kind)
                .unwrap()
                .contains("provider-agent-1")
    }));
    assert_eq!(saga::resume_pending(&app).await.unwrap(), 0);
}

/// Finding 8: a subscriber that lags by more than one catch-up page plus the
/// broadcast buffer still receives every event, in order, exactly once.
#[tokio::test]
async fn lagging_sse_subscriber_receives_every_event() {
    use genesis_core::events::EventKind;
    const N: u64 = 8_000;
    let cfg = OrgConfig::dev("review", "Review");
    let app = build_org(&cfg, None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/v1/events", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut response = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&cfg.tokens.operator)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let start = app.store.events_after(0, 100).unwrap().len() as u64;

    // Broadcast without yielding (current-thread runtime): the subscriber
    // overflows the broadcast buffer before it reads anything.
    let effects = (0..N)
        .map(|i| EventKind::RoleActivated {
            role_id: RoleId::new(format!("role-{i}")),
        })
        .collect();
    let events = app
        .store
        .append_events(effects, &EventMeta::default(), |_| Ok(()))
        .unwrap();
    app.broadcast(&events);

    let mut ids = Vec::new();
    let mut buf = String::new();
    let read = async {
        while ids.last().copied() != Some(start + N) {
            let chunk = response.chunk().await.unwrap().expect("stream ended");
            buf.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = buf.find('\n') {
                let line: String = buf.drain(..=end).collect();
                if let Some(id) = line.trim_end().strip_prefix("id:") {
                    ids.push(id.trim().parse::<u64>().unwrap());
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), read)
        .await
        .expect("all events delivered");
    assert_eq!(ids, (1..=start + N).collect::<Vec<_>>());
    server.abort();
}
