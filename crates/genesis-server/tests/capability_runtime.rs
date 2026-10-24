//! Durable coordination tests use a synthetic typed executor. Provider HTTP
//! provenance is tested independently by genesis-apify/genesis-providers.
use chrono::Utc;
use genesis_apify::{RunRequest, RunTicket};
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::*;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{InstallationStatus, JobStatus, MissionStatus, RoleState};
use genesis_core::model::{CapabilityRef, Installation, OrgState, Provider};
use genesis_providers::{CapabilityExecutor, CapabilityRunReceipt, ProviderError, ProviderSet};
use genesis_server::capability_runtime::{
    insert_in_tx, CapabilityRunPhase as Phase, CapabilityRunRecord, CapabilityRuntime,
    CapabilityRuntimeError,
};
use genesis_server::store::{EventMeta, Store};
use genesis_verification::{
    json_sha256, ArtifactPin, ExecutionEvidence, RunStatus, SourceSnapshot,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, Notify};

type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

#[derive(Clone, Copy)]
enum SubmitMode {
    Success,
    Pause,
    Ambiguous,
    BadReceipt,
}
#[derive(Clone, Copy)]
enum ReadMode {
    Success,
    Pause,
    Timeout,
    Rejected,
    BadEvidence,
}

struct MockExecutor {
    path: PathBuf,
    job: JobId,
    submits: AtomicUsize,
    reads: AtomicUsize,
    submit_mode: StdMutex<SubmitMode>,
    read_mode: StdMutex<ReadMode>,
    entered: Notify,
    release: Notify,
}

fn ticket(request: &RunRequest) -> RunTicket {
    // Synthetic local receipt, never a claim of authenticated provider work.
    let fingerprint = json_sha256(&json!({"schema_version":1,"case_id":request.case_id,
        "artifact":request.artifact,"source":request.source,"input_sha256":json_sha256(&request.input)}));
    serde_json::from_value(json!({"schema_version":1,"run_id":"local-run-1","dataset_id":"local-dataset-1","request_sha256":fingerprint})).unwrap()
}

fn receipt(request: &RunRequest) -> CapabilityRunReceipt {
    CapabilityRunReceipt {
        provider: Provider::Apify,
        raw_status: "APIFY_SUBMITTED".into(),
        ticket: Some(ticket(request)),
        provider_refs: BTreeMap::from([
            ("actor_id".into(), request.artifact.actor_id.clone()),
            ("build_id".into(), request.artifact.build_id.clone()),
            ("build_number".into(), request.artifact.build_number.clone()),
            ("run_id".into(), "local-run-1".into()),
        ]),
    }
}

impl CapabilityExecutor for MockExecutor {
    fn provider(&self) -> Provider {
        Provider::Apify
    }
    fn submit<'a, 'b, 'future>(
        &'a self,
        request: &'b RunRequest,
    ) -> ProviderFuture<'future, CapabilityRunReceipt>
    where
        'a: 'future,
        'b: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            self.submits.fetch_add(1, Ordering::SeqCst);
            // A second SQLite connection must observe intent before this effect.
            let store = Store::open(&self.path, OrgId::new("org-a")).unwrap();
            assert_eq!(
                store.get_capability_run(&self.job).unwrap().phase,
                Phase::Submitting
            );
            assert_eq!(
                store.get_job(&self.job).unwrap().status,
                JobStatus::Submitting
            );
            let mode = *self.submit_mode.lock().unwrap();
            if matches!(mode, SubmitMode::Pause) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            if matches!(mode, SubmitMode::Ambiguous) {
                return Err(ProviderError::Unknown(
                    "SECRET raw provider response".into(),
                ));
            }
            let mut receipt = receipt(request);
            if matches!(mode, SubmitMode::BadReceipt) {
                receipt
                    .provider_refs
                    .insert("build_id".into(), "wrong-build".into());
            }
            Ok(receipt)
        })
    }
    fn resume<'a, 'b, 'c, 'future>(
        &'a self,
        request: &'b RunRequest,
        saved: &'c RunTicket,
    ) -> ProviderFuture<'future, ExecutionEvidence>
    where
        'a: 'future,
        'b: 'future,
        'c: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            saved.validate_for(request).unwrap();
            let store = Store::open(&self.path, OrgId::new("org-a")).unwrap();
            assert_eq!(
                store.get_capability_run(&self.job).unwrap().phase,
                Phase::Collecting
            );
            let mode = *self.read_mode.lock().unwrap();
            if matches!(mode, ReadMode::Pause) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            if matches!(mode, ReadMode::Timeout) {
                return Err(ProviderError::Timeout);
            }
            if matches!(mode, ReadMode::Rejected) {
                return Err(ProviderError::Rejected(
                    "SECRET raw provider response".into(),
                ));
            }
            let mut evidence = ExecutionEvidence {
                case_id: request.case_id.clone(),
                artifact: request.artifact.clone(),
                input_sha256: json_sha256(&request.input),
                source: request.source.clone(),
                run_id: "local-run-1".into(),
                dataset_id: "local-dataset-1".into(),
                status: RunStatus::Succeeded,
                output: json!({"status":"unsupported","reason":"unsupported_format","evidence":{
                    "source_url":request.source.url,"snapshot_sha256":request.source.sha256,"locator":"body"}}),
            };
            if matches!(mode, ReadMode::BadEvidence) {
                evidence.input_sha256 = "f".repeat(64);
            }
            Ok(evidence)
        })
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    store: Store,
    kernel: Arc<Mutex<()>>,
    executor: Arc<MockExecutor>,
    providers: ProviderSet,
    runtime: CapabilityRuntime,
    job: Job,
    record: CapabilityRunRecord,
}

impl Fixture {
    fn new(insert: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("org.db");
        let store = Store::open(&path, OrgId::new("org-a")).unwrap();
        let initial = OrgState::new(
            OrgId::new("org-a"),
            "Org A",
            genesis_server::demo_constitution(),
        );
        store.init(&initial).unwrap();
        store.init_capability_runs().unwrap();
        let bootstrap: MissionBootstrap = serde_json::from_value(json!({"goal":"Inspect venues",
            "roles":[{"key":"lead","name":"Lead","charter":"Own venue work","authority":{"scopes":["capability.venue_lookup"],"budget_cents":0}}],
            "responsibilities":[{"purpose":"Find venue","owner":"lead"}]})).unwrap();
        let (mut outcome, mission_id) =
            bootstrap_mission(&initial, &bootstrap, Utc::now()).unwrap();
        let role = outcome.state.roles.values_mut().next().unwrap();
        let capability = CapabilityRef::new("venue-extraction", "1");
        role.capabilities.push(capability.clone());
        let role_id = role.role_id.clone();
        let installation_id = InstallationId::generate();
        outcome.state.installations.insert(
            installation_id.clone(),
            Installation {
                installation_id,
                capability: capability.clone(),
                status: InstallationStatus::Accepted,
                configuration: json!({}),
                acceptance_report_ref: Some("trusted-local-suite".into()),
                source_digest: "a".repeat(64),
                installed_at: Utc::now(),
            },
        );
        store
            .commit_state(
                0,
                &outcome.state,
                outcome.effects,
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        let responsibility = outcome.state.responsibilities.values().next().unwrap();
        let assignment = outcome
            .state
            .active_assignment(&responsibility.responsibility_id)
            .unwrap();
        let request = RunRequest {
            case_id: "cards".into(),
            artifact: ArtifactPin {
                capability_id: "venue-extraction".into(),
                capability_version: "1".into(),
                source_sha256: "a".repeat(64),
                actor_id: "actor-1".into(),
                build_id: "build-1".into(),
                build_number: "0.1.2".into(),
            },
            input: json!({"source_url":"https://fixtures.example/venue","attendees":120}),
            source: SourceSnapshot {
                url: "https://fixtures.example/venue".into(),
                sha256: "b".repeat(64),
            },
        };
        let now = Utc::now();
        let job = Job {
            job_id: JobId::generate(),
            org_id: initial.org_id,
            mission_id,
            role_id,
            responsibility_id: responsibility.responsibility_id.clone(),
            assignment_id: assignment.assignment_id.clone(),
            epoch: assignment.epoch,
            task: "Extract venue data".into(),
            input: request.input.clone(),
            attempt_id: AttemptId::generate(),
            attempt_no: 1,
            correlation_id: CorrelationId::generate(),
            idempotency_key: "org-a:request-1".into(),
            deadline: None,
            capability: Some(capability),
            provider: Some(Provider::Apify),
            status: JobStatus::Queued,
            provider_refs: BTreeMap::new(),
            raw_status: None,
            result: None,
            rejection_reason: None,
            created_at: now,
            updated_at: now,
        };
        let record =
            CapabilityRunRecord::new(&job, request, Some("capability.venue_lookup".into()))
                .unwrap();
        if insert {
            store
                .commit_state(
                    outcome.state.revision,
                    &outcome.state,
                    vec![],
                    &EventMeta::default(),
                    vec![],
                    |tx| insert_in_tx(tx, "request-1", &job, &record),
                )
                .unwrap();
        }
        let executor = Arc::new(MockExecutor {
            path: path.clone(),
            job: job.job_id.clone(),
            submits: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            submit_mode: StdMutex::new(SubmitMode::Success),
            read_mode: StdMutex::new(ReadMode::Success),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let providers = ProviderSet::new().with_executor(executor.clone());
        let kernel = Arc::new(Mutex::new(()));
        let runtime = CapabilityRuntime::new(
            store.clone(),
            providers.clone(),
            kernel.clone(),
            Arc::new(Mutex::new(())),
        );
        Self {
            _directory: directory,
            path,
            store,
            kernel,
            executor,
            providers,
            runtime,
            job,
            record,
        }
    }
    fn restart(&self) -> (Store, CapabilityRuntime) {
        let store = Store::open(&self.path, OrgId::new("org-a")).unwrap();
        store.init_capability_runs().unwrap();
        let runtime = CapabilityRuntime::new(
            store.clone(),
            self.providers.clone(),
            self.kernel.clone(),
            Arc::new(Mutex::new(())),
        );
        (store, runtime)
    }
    async fn revoke(&self) {
        let _guard = self.kernel.lock().await;
        let state = self.store.current_state().unwrap();
        let mut next = state.clone();
        next.roles.get_mut(&self.job.role_id).unwrap().state = RoleState::Draining;
        next.revision += 1;
        self.store
            .commit_state(
                state.revision,
                &next,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
}

#[tokio::test]
async fn real_sqlite_restart_retains_ticket_and_collection_never_authorizes_success() {
    let f = Fixture::new(true);
    let submitted = f.runtime.step(&f.job.job_id, false).await.unwrap();
    assert_eq!(submitted.phase, Phase::Running);
    let (store, runtime) = f.restart();
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 0);
    let collected = runtime.step(&f.job.job_id, false).await.unwrap();
    assert_eq!(collected.phase, Phase::Collected);
    assert!(collected.evidence.is_some());
    assert_eq!(
        store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Verifying
    );
    assert!(store.commitments().unwrap().is_empty());
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
    assert_eq!(f.executor.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime.step(&f.job.job_id, false).await.unwrap().phase,
        Phase::Collected
    );
    assert_eq!(
        store
            .find_capability_run("request-1")
            .unwrap()
            .unwrap()
            .job_id,
        f.job.job_id
    );
}

#[tokio::test]
async fn intent_write_failure_never_calls_provider_and_rolls_back_job_projection() {
    let f = Fixture::new(true);
    let fault = rusqlite::Connection::open(&f.path).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_intent BEFORE UPDATE ON capability_runs WHEN NEW.phase='submitting' BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    let before_events = f.store.events_after(0, 100).unwrap().len();
    assert!(f.runtime.step(&f.job.job_id, false).await.is_err());
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Queued
    );
    assert_eq!(
        f.store.get_capability_run(&f.job.job_id).unwrap().phase,
        Phase::Prepared
    );
    assert_eq!(f.store.events_after(0, 100).unwrap().len(), before_events);
}

#[test]
fn job_and_request_insert_are_one_transaction_and_conflicts_do_not_replace_them() {
    let f = Fixture::new(false);
    let fault = rusqlite::Connection::open(&f.path).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON capability_runs BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    let state = f.store.current_state().unwrap();
    assert!(f
        .store
        .commit_state(
            state.revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| insert_in_tx(tx, "request-1", &f.job, &f.record)
        )
        .is_err());
    assert!(f.store.list_jobs().unwrap().is_empty());
    assert!(f.store.find_capability_run("request-1").unwrap().is_none());
    fault.execute_batch("DROP TRIGGER fail_insert;").unwrap();
    f.store
        .commit_state(
            state.revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| insert_in_tx(tx, "request-1", &f.job, &f.record),
        )
        .unwrap();
    assert!(f
        .store
        .commit_state(
            state.revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| insert_in_tx(tx, "request-1", &f.job, &f.record)
        )
        .is_err());
    assert_eq!(f.store.list_jobs().unwrap().len(), 1);
}

#[tokio::test]
async fn crash_after_post_intent_never_reposts_on_recovery() {
    let f = Fixture::new(true);
    *f.executor.submit_mode.lock().unwrap() = SubmitMode::Pause;
    let runtime = f.runtime.clone();
    let id = f.job.job_id.clone();
    let task = tokio::spawn(async move { runtime.step(&id, false).await });
    f.executor.entered.notified().await;
    assert!(
        f.kernel.try_lock().is_ok(),
        "kernel must be available while provider is awaited"
    );
    task.abort();
    let _ = task.await;
    let (store, runtime) = f.restart();
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    assert_eq!(
        store.get_capability_run(&f.job.job_id).unwrap().phase,
        Phase::NeedsReconciliation
    );
    assert_eq!(
        store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Unknown
    );
    assert!(matches!(
        runtime.step(&f.job.job_id, false).await,
        Err(CapabilityRuntimeError::NeedsReconciliation)
    ));
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn interrupted_collection_resumes_get_with_saved_ticket_only() {
    let f = Fixture::new(true);
    f.runtime.step(&f.job.job_id, false).await.unwrap();
    *f.executor.read_mode.lock().unwrap() = ReadMode::Pause;
    let runtime = f.runtime.clone();
    let id = f.job.job_id.clone();
    let task = tokio::spawn(async move { runtime.step(&id, false).await });
    f.executor.entered.notified().await;
    task.abort();
    let _ = task.await;
    *f.executor.read_mode.lock().unwrap() = ReadMode::Success;
    let (store, runtime) = f.restart();
    assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    assert!(store
        .get_capability_run(&f.job.job_id)
        .unwrap()
        .receipt
        .is_some());
    assert_eq!(
        runtime.step(&f.job.job_id, false).await.unwrap().phase,
        Phase::Collected
    );
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
    assert_eq!(f.executor.reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn transient_read_requires_explicit_retry_and_permanent_failure_stops() {
    let f = Fixture::new(true);
    f.runtime.step(&f.job.job_id, false).await.unwrap();
    *f.executor.read_mode.lock().unwrap() = ReadMode::Timeout;
    let failed = f.runtime.step(&f.job.job_id, false).await.unwrap();
    assert_eq!(failed.phase, Phase::Running);
    assert!(failed.retryable_read);
    assert!(failed.receipt.is_some());
    assert!(matches!(
        f.runtime.step(&f.job.job_id, false).await,
        Err(CapabilityRuntimeError::RetryRequired)
    ));
    assert_eq!(f.executor.reads.load(Ordering::SeqCst), 1);
    *f.executor.read_mode.lock().unwrap() = ReadMode::Rejected;
    let terminal = f.runtime.step(&f.job.job_id, true).await.unwrap();
    assert_eq!(terminal.phase, Phase::Failed);
    assert!(!terminal.retryable_read);
    assert!(!serde_json::to_string(&terminal).unwrap().contains("SECRET"));
    assert_eq!(
        f.store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Failed
    );
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ambiguous_or_malformed_submission_is_never_retried() {
    for mode in [SubmitMode::Ambiguous, SubmitMode::BadReceipt] {
        let f = Fixture::new(true);
        *f.executor.submit_mode.lock().unwrap() = mode;
        let record = f.runtime.step(&f.job.job_id, false).await.unwrap();
        assert_eq!(record.phase, Phase::NeedsReconciliation);
        assert!(!serde_json::to_string(&record).unwrap().contains("SECRET"));
        assert!(matches!(
            f.runtime.step(&f.job.job_id, false).await,
            Err(CapabilityRuntimeError::NeedsReconciliation)
        ));
        assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn authorization_is_rechecked_before_post_and_when_applying_its_receipt() {
    let before = Fixture::new(true);
    before.revoke().await;
    assert_eq!(
        before
            .runtime
            .step(&before.job.job_id, false)
            .await
            .unwrap()
            .phase,
        Phase::Rejected
    );
    assert_eq!(before.executor.submits.load(Ordering::SeqCst), 0);
    let after = Fixture::new(true);
    *after.executor.submit_mode.lock().unwrap() = SubmitMode::Pause;
    let runtime = after.runtime.clone();
    let id = after.job.job_id.clone();
    let task = tokio::spawn(async move { runtime.step(&id, false).await });
    after.executor.entered.notified().await;
    after.revoke().await;
    after.executor.release.notify_one();
    let record = task.await.unwrap().unwrap();
    assert_eq!(record.phase, Phase::Rejected);
    assert!(record.receipt.is_some());
    assert!(record.evidence.is_none());
    assert_eq!(
        after.store.get_job(&after.job.job_id).unwrap().status,
        JobStatus::ResultRejected
    );
    assert!(after.store.commitments().unwrap().is_empty());
}

#[tokio::test]
async fn authorization_is_rechecked_after_collection_before_output_is_applied() {
    let f = Fixture::new(true);
    f.runtime.step(&f.job.job_id, false).await.unwrap();
    *f.executor.read_mode.lock().unwrap() = ReadMode::Pause;
    let runtime = f.runtime.clone();
    let id = f.job.job_id.clone();
    let task = tokio::spawn(async move { runtime.step(&id, false).await });
    f.executor.entered.notified().await;
    f.revoke().await;
    f.executor.release.notify_one();
    assert_eq!(task.await.unwrap().unwrap().phase, Phase::Rejected);
    assert!(f.store.get_job(&f.job.job_id).unwrap().result.is_none());
    assert!(f.store.commitments().unwrap().is_empty());
}

fn invalidate_time_authority(f: &Fixture, reason: &str) {
    mutate_state(f, |state| match reason {
        "lease" => {
            state
                .assignments
                .get_mut(&f.job.assignment_id)
                .unwrap()
                .lease_expires_at = Some(Utc::now());
        }
        "mission_deadline" => {
            state.missions.get_mut(&f.job.mission_id).unwrap().deadline = Some(Utc::now());
        }
        "mission_closed" => {
            state.missions.get_mut(&f.job.mission_id).unwrap().status = MissionStatus::Succeeded;
        }
        _ => unreachable!(),
    });
}

#[tokio::test]
async fn expired_assignment_or_mission_blocks_both_post_and_collection() {
    for purpose in ["invocation", "acceptance"] {
        for reason in ["lease", "mission_deadline", "mission_closed"] {
            for collect in [false, true] {
                let f = Fixture::new(false);
                let record = if purpose == "acceptance" {
                    acceptance_record(&f)
                } else {
                    f.record.clone()
                };
                insert_record(&f, &record);
                if collect {
                    f.runtime.step(&f.job.job_id, false).await.unwrap();
                }
                invalidate_time_authority(&f, reason);
                assert_eq!(
                    f.runtime.step(&f.job.job_id, false).await.unwrap().phase,
                    Phase::Rejected,
                    "{purpose}/{reason}/{collect}"
                );
                assert_eq!(
                    f.executor.submits.load(Ordering::SeqCst),
                    usize::from(collect)
                );
                assert_eq!(f.executor.reads.load(Ordering::SeqCst), 0);
                assert!(f.store.commitments().unwrap().is_empty());
            }
        }
    }
}

#[tokio::test]
async fn expiry_during_provider_await_retains_ticket_without_accepting_output() {
    for reason in ["lease", "mission_deadline", "mission_closed"] {
        for collect in [false, true] {
            let f = Fixture::new(true);
            if collect {
                f.runtime.step(&f.job.job_id, false).await.unwrap();
                *f.executor.read_mode.lock().unwrap() = ReadMode::Pause;
            } else {
                *f.executor.submit_mode.lock().unwrap() = SubmitMode::Pause;
            }
            let runtime = f.runtime.clone();
            let id = f.job.job_id.clone();
            let task = tokio::spawn(async move { runtime.step(&id, false).await });
            f.executor.entered.notified().await;
            invalidate_time_authority(&f, reason);
            f.executor.release.notify_one();
            let rejected = task.await.unwrap().unwrap();
            assert_eq!(rejected.phase, Phase::Rejected, "{reason}/{collect}");
            assert!(rejected.receipt.is_some());
            assert!(rejected.evidence.is_none());
            assert!(f.store.get_job(&f.job.job_id).unwrap().result.is_none());
            assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
            assert_eq!(
                f.executor.reads.load(Ordering::SeqCst),
                usize::from(collect)
            );
            assert!(f.store.commitments().unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn concurrent_steps_share_one_post_and_cannot_skip_verification() {
    let f = Fixture::new(true);
    let (one, two) = tokio::join!(
        f.runtime.step(&f.job.job_id, false),
        f.runtime.step(&f.job.job_id, false)
    );
    assert_eq!(one.unwrap().phase, Phase::Running);
    assert_eq!(two.unwrap().phase, Phase::Collected);
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 1);
    assert_eq!(f.executor.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Verifying
    );
}

#[tokio::test]
async fn foreign_evidence_fails_closed_without_output_or_ledger() {
    let f = Fixture::new(true);
    f.runtime.step(&f.job.job_id, false).await.unwrap();
    *f.executor.read_mode.lock().unwrap() = ReadMode::BadEvidence;
    assert_eq!(
        f.runtime.step(&f.job.job_id, false).await.unwrap().phase,
        Phase::Failed
    );
    assert!(f.store.get_job(&f.job.job_id).unwrap().result.is_none());
    assert!(f.store.commitments().unwrap().is_empty());
}

#[tokio::test]
async fn restored_request_and_ticket_tampering_is_rejected_before_provider_calls() {
    for pointer in [
        "/request/input/attendees",
        "/request/artifact/build_id",
        "/required_scope",
        "/schema_version",
    ] {
        let f = Fixture::new(true);
        let mut changed = serde_json::to_value(&f.record).unwrap();
        *changed.pointer_mut(pointer).unwrap() =
            if pointer.ends_with("schema_version") || pointer.ends_with("attendees") {
                json!(999)
            } else {
                json!("changed")
            };
        let sql = rusqlite::Connection::open(&f.path).unwrap();
        sql.execute(
            "UPDATE capability_runs SET record_json=?1",
            [changed.to_string()],
        )
        .unwrap();
        assert!(f.runtime.step(&f.job.job_id, false).await.is_err());
        assert_eq!(f.executor.submits.load(Ordering::SeqCst), 0);
    }
    let f = Fixture::new(true);
    let running = f.runtime.step(&f.job.job_id, false).await.unwrap();
    let mut changed = serde_json::to_value(running).unwrap();
    changed["receipt"]["ticket"]["schema_version"] = json!(999);
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute(
            "UPDATE capability_runs SET record_json=?1",
            [changed.to_string()],
        )
        .unwrap();
    assert!(f.runtime.step(&f.job.job_id, false).await.is_err());
    assert_eq!(f.executor.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn repeated_client_request_must_match_all_semantic_inputs_and_pin() {
    let f = Fixture::new(true);
    let mut regenerated = f.job.clone();
    regenerated.job_id = JobId::generate();
    regenerated.attempt_id = AttemptId::generate();
    assert!(f.record.matches_submission(
        &f.job,
        &regenerated,
        &f.record.request,
        f.record.required_scope.as_deref()
    ));
    regenerated.task = "Different task".into();
    assert!(!f.record.matches_submission(
        &f.job,
        &regenerated,
        &f.record.request,
        f.record.required_scope.as_deref()
    ));
    let mut changed = f.record.request.clone();
    changed.artifact.build_id = "new-build".into();
    assert!(!f.record.matches_submission(
        &f.job,
        &f.job,
        &changed,
        f.record.required_scope.as_deref()
    ));
    assert!(!f
        .record
        .matches_submission(&f.job, &f.job, &f.record.request, Some("other.scope")));
}

fn mutate_state(f: &Fixture, edit: impl FnOnce(&mut OrgState)) {
    let previous = f.store.current_state().unwrap();
    let mut next = previous.clone();
    edit(&mut next);
    next.revision += 1;
    f.store
        .commit_state(
            previous.revision,
            &next,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
}

fn insert_record(f: &Fixture, record: &CapabilityRunRecord) {
    let state = f.store.current_state().unwrap();
    f.store
        .commit_state(
            state.revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| insert_in_tx(tx, "request-1", &f.job, record),
        )
        .unwrap();
}

fn acceptance_record(f: &Fixture) -> CapabilityRunRecord {
    let id = f
        .store
        .current_state()
        .unwrap()
        .installations
        .keys()
        .next()
        .unwrap()
        .clone();
    mutate_state(f, |state| {
        state
            .roles
            .get_mut(&f.job.role_id)
            .unwrap()
            .capabilities
            .clear();
        state.installations.get_mut(&id).unwrap().status = InstallationStatus::Checking;
    });
    CapabilityRunRecord::new_for_acceptance(
        &f.job,
        f.record.request.clone(),
        f.record.required_scope.clone(),
        id,
        "local-suite-v1".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn local_acceptance_can_test_checking_installation_without_attaching_or_accepting_it() {
    let f = Fixture::new(false);
    let record = acceptance_record(&f);
    insert_record(&f, &record);
    assert!(!record.matches_submission(
        &f.job,
        &f.job,
        &record.request,
        record.required_scope.as_deref()
    ));
    assert_eq!(
        f.runtime.step(&f.job.job_id, false).await.unwrap().phase,
        Phase::Running
    );
    let (store, runtime) = f.restart();
    assert_eq!(
        runtime.step(&f.job.job_id, false).await.unwrap().phase,
        Phase::Collected
    );
    let state = store.current_state().unwrap();
    assert_eq!(
        state.installations.values().next().unwrap().status,
        InstallationStatus::Checking
    );
    assert!(state.roles[&f.job.role_id].capabilities.is_empty());
    assert_eq!(
        store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Verifying
    );
    assert!(store.commitments().unwrap().is_empty());
}

#[tokio::test]
async fn acceptance_purpose_cannot_skip_installation_identity_source_or_state_checks() {
    for mutation in ["source", "capability", "accepted", "uninstalled", "missing"] {
        let f = Fixture::new(false);
        let record = acceptance_record(&f);
        insert_record(&f, &record);
        mutate_state(&f, |state| {
            let installation = state.installations.values_mut().next().unwrap();
            match mutation {
                "source" => installation.source_digest = "c".repeat(64),
                "capability" => installation.capability.version = "2".into(),
                "accepted" => installation.status = InstallationStatus::Accepted,
                "uninstalled" => installation.status = InstallationStatus::Uninstalled,
                "missing" => state.installations.clear(),
                _ => unreachable!(),
            }
        });
        assert_eq!(
            f.runtime.step(&f.job.job_id, false).await.unwrap().phase,
            Phase::Rejected,
            "{mutation}"
        );
        assert_eq!(f.executor.submits.load(Ordering::SeqCst), 0);
    }
    let f = Fixture::new(false);
    let _ = acceptance_record(&f);
    insert_record(&f, &f.record); // Same job in ordinary invocation mode.
    assert_eq!(
        f.runtime.step(&f.job.job_id, false).await.unwrap().phase,
        Phase::Rejected
    );
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn acceptance_state_is_rechecked_before_applying_collected_evidence() {
    let f = Fixture::new(false);
    let record = acceptance_record(&f);
    insert_record(&f, &record);
    f.runtime.step(&f.job.job_id, false).await.unwrap();
    *f.executor.read_mode.lock().unwrap() = ReadMode::Pause;
    let runtime = f.runtime.clone();
    let id = f.job.job_id.clone();
    let task = tokio::spawn(async move { runtime.step(&id, false).await });
    f.executor.entered.notified().await;
    {
        let _guard = f.kernel.lock().await;
        mutate_state(&f, |state| {
            state.installations.values_mut().next().unwrap().status = InstallationStatus::Rejected
        });
    }
    f.executor.release.notify_one();
    assert_eq!(task.await.unwrap().unwrap().phase, Phase::Rejected);
    assert!(f.store.get_job(&f.job.job_id).unwrap().result.is_none());
}

#[tokio::test]
async fn manifest_permissions_are_all_required_before_and_after_provider_work() {
    let denied = Fixture::new(false);
    let record = denied
        .record
        .clone()
        .with_required_permissions(
            &denied.job,
            ["capability.venue_lookup".into(), "web.read".into()].into(),
        )
        .unwrap();
    insert_record(&denied, &record);
    assert_eq!(
        denied
            .runtime
            .step(&denied.job.job_id, false)
            .await
            .unwrap()
            .phase,
        Phase::Rejected
    );
    assert_eq!(denied.executor.submits.load(Ordering::SeqCst), 0);

    let f = Fixture::new(false);
    mutate_state(&f, |state| {
        state
            .roles
            .get_mut(&f.job.role_id)
            .unwrap()
            .authority
            .scopes
            .insert("web.read".into());
    });
    let record = f
        .record
        .clone()
        .with_required_permissions(
            &f.job,
            ["capability.venue_lookup".into(), "web.read".into()].into(),
        )
        .unwrap();
    insert_record(&f, &record);
    *f.executor.submit_mode.lock().unwrap() = SubmitMode::Pause;
    let runtime = f.runtime.clone();
    let id = f.job.job_id.clone();
    let task = tokio::spawn(async move { runtime.step(&id, false).await });
    f.executor.entered.notified().await;
    {
        let _guard = f.kernel.lock().await;
        mutate_state(&f, |state| {
            state
                .roles
                .get_mut(&f.job.role_id)
                .unwrap()
                .authority
                .scopes
                .remove("web.read");
        });
    }
    f.executor.release.notify_one();
    let rejected = task.await.unwrap().unwrap();
    assert_eq!(rejected.phase, Phase::Rejected);
    assert!(rejected.receipt.is_some());
}

#[test]
fn purpose_and_permissions_are_bound_to_checkpoint_and_constructor_validates_labels() {
    let f = Fixture::new(false);
    let record = acceptance_record(&f);
    for pointer in ["/purpose/suite_id", "/purpose/installation_id"] {
        let mut changed = serde_json::to_value(&record).unwrap();
        *changed.pointer_mut(pointer).unwrap() = json!("different");
        let changed: CapabilityRunRecord = serde_json::from_value(changed).unwrap();
        assert!(changed.validate(&f.job).is_err());
    }
    let mut changed = record.clone();
    changed
        .required_permissions
        .insert("different.scope".into());
    assert!(changed.validate(&f.job).is_err());
    assert!(record
        .with_required_permissions(&f.job, ["invalid\nscope".into()].into())
        .is_err());
    assert!(CapabilityRunRecord::new_for_acceptance(
        &f.job,
        f.record.request.clone(),
        None,
        InstallationId::generate(),
        " ".into()
    )
    .is_err());
}

#[tokio::test]
async fn broadcasts_only_committed_events_with_their_durable_sequence() {
    let f = Fixture::new(true);
    let (sender, mut receiver) = tokio::sync::broadcast::channel(16);
    let runtime = f.runtime.clone().with_events(sender);
    runtime.step(&f.job.job_id, false).await.unwrap();
    let durable = f.store.events_after(0, 100).unwrap();
    let mut seen = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        assert!(durable.contains(&event));
        seen.push(event);
    }
    assert_eq!(seen.len(), 2); // Submitting, then Running.
    let fault = rusqlite::Connection::open(&f.path).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_collection BEFORE UPDATE ON capability_runs WHEN NEW.phase='collected' BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    assert!(runtime.step(&f.job.job_id, false).await.is_err());
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        f.store.get_job(&f.job.job_id).unwrap().status,
        JobStatus::Running
    );
    assert_eq!(
        f.store.get_capability_run(&f.job.job_id).unwrap().phase,
        Phase::Collecting
    );
}

#[tokio::test]
async fn restored_job_projection_cannot_replay_effects_but_collected_job_can_be_finalized() {
    let f = Fixture::new(true);
    let mut corrupted = f.job.clone();
    corrupted.status = JobStatus::Succeeded;
    let sql = rusqlite::Connection::open(&f.path).unwrap();
    sql.execute(
        "UPDATE jobs SET status='Succeeded',job_json=?1 WHERE job_id=?2",
        rusqlite::params![
            serde_json::to_string(&corrupted).unwrap(),
            f.job.job_id.as_str()
        ],
    )
    .unwrap();
    assert!(f.runtime.step(&f.job.job_id, false).await.is_err());
    assert_eq!(f.executor.submits.load(Ordering::SeqCst), 0);

    let f = Fixture::new(true);
    f.runtime.step(&f.job.job_id, false).await.unwrap();
    let collected = f.runtime.step(&f.job.job_id, false).await.unwrap();
    let mut finalized = f.store.get_job(&f.job.job_id).unwrap();
    finalized.status = JobStatus::Succeeded;
    assert!(collected.validate(&finalized).is_ok());
    let mut changed_output = finalized.clone();
    changed_output.result = Some(json!({"status":"fabricated"}));
    assert!(collected.validate(&changed_output).is_err());
    assert!(collected
        .with_required_permissions(&finalized, Default::default())
        .is_err());
}
