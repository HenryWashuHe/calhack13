//! Real model/Apify adapters against loopback providers. Provider execution is
//! simulated, but persisted intents, quota debits, grants and recovery are real.
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::engine::{
    bootstrap_mission, record_observation, record_requirement, MissionBootstrap,
};
use genesis_core::ids::*;
use genesis_core::lifecycle::{RequirementStatus, RoleState};
use genesis_core::model::{OrgState, Owner};
use genesis_forge::{
    AcquisitionPhase, AnthropicGenerator, CandidateSpec, ModelConfig, SynthesisPhase,
};
use genesis_server::capability_acquisition_runtime::{AcquisitionRuntime, AcquisitionRuntimeError};
use genesis_server::capability_acquisition_store::{
    AcquisitionPolicy, AcquisitionQuotas, AcquisitionRecord, AcquisitionStoreError,
    CreateAcquisition,
};
use genesis_server::capability_evidence::EvidencePolicy;
use genesis_server::store::{EventMeta, Store};
use genesis_verification::{decode_json, ExecutionEvidence, OracleSuite};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

const MODEL: &str = "claude-sonnet-4-6";
const KEY: &str = "local-acquisition-runtime-fixture-key";
const PARSER: &str = "export function extract() { throw new Error('generated source is never executed on this host'); }\n";

struct HttpServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl HttpServer {
    async fn start(router: axum::Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, task }
    }
}
impl Drop for HttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Reply {
    status: u16,
    body: Value,
    pause: bool,
}
impl Reply {
    fn json(body: Value) -> Self {
        Self {
            status: 200,
            body,
            pause: false,
        }
    }
    fn paused(body: Value) -> Self {
        Self {
            status: 200,
            body,
            pause: true,
        }
    }
    fn unavailable() -> Self {
        Self {
            status: 503,
            body: json!({"private": "never expose provider output"}),
            pause: false,
        }
    }
}
struct Mock {
    base: String,
    requests: Arc<StdMutex<Vec<String>>>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Mock {
    async fn serve(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (captured, on_enter, on_release) = (requests.clone(), entered.clone(), release.clone());
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0; 4096];
                    let read = socket.read(&mut buf).await.unwrap();
                    assert_ne!(read, 0);
                    bytes.extend_from_slice(&buf[..read]);
                    assert!(bytes.len() < 4 * 1024 * 1024);
                    if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                        let length = String::from_utf8_lossy(&bytes[..end])
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                if reply.pause {
                    on_enter.notify_one();
                    on_release.notified().await;
                }
                let body = reply.body.to_string();
                let response = format!("HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.status, body.len(), body);
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self {
            base,
            requests,
            entered,
            release,
            task,
        }
    }
    fn model(&self) -> Arc<AnthropicGenerator> {
        Arc::new(
            AnthropicGenerator::with_test_endpoint(&format!("{}/v1/messages", self.base), KEY)
                .unwrap(),
        )
    }
    fn apify(&self) -> Arc<ApifyClient> {
        Arc::new(
            ApifyClient::for_local_testing(
                KEY,
                RunLimits {
                    request_timeout: Duration::from_secs(5),
                    deadline: Duration::from_secs(10),
                    poll_interval: Duration::from_millis(1),
                    ..RunLimits::default()
                },
                &self.base,
            )
            .unwrap(),
        )
    }
    async fn finish(self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}
fn generated() -> Value {
    json!({"id":"msg_runtime", "type":"message", "role":"assistant", "model":MODEL,
        "content":[{"type":"text","text":json!({"extractor_typescript":PARSER}).to_string()}],
        "stop_reason":"end_turn", "stop_sequence":null, "usage":{"input_tokens":100,"output_tokens":60}})
}
fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.scope = "PRIVATE_RUNTIME_ORACLE_SENTINEL".into();
    suite
}
fn outputs() -> Vec<Value> {
    let runs: Vec<ExecutionEvidence> = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap();
    runs.into_iter().map(|run| run.output).collect()
}

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    store: Store,
    kernel: Arc<Mutex<()>>,
    gate: Arc<Mutex<()>>,
    policy: AcquisitionPolicy,
    evidence: Arc<EvidencePolicy>,
    suite: OracleSuite,
    intent: CreateAcquisition,
    id: String,
}
impl Fixture {
    fn new(quotas: AcquisitionQuotas) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("organization.db");
        let store = Store::open(&path, OrgId::new("org-runtime")).unwrap();
        let mut constitution = genesis_server::demo_constitution();
        constitution.scopes.insert("capability.acquire".into());
        let initial = OrgState::new(OrgId::new("org-runtime"), "Runtime org", constitution);
        store.init(&initial).unwrap();
        let bootstrap: MissionBootstrap = serde_json::from_value(json!({"goal":"Find a replacement venue",
            "roles":[{"key":"lead","name":"Lead","charter":"Own missing venue capability","authority":{"scopes":["capability.acquire"],"budget_cents":0}}],
            "responsibilities":[{"purpose":"Acquire venue extraction","owner":"lead"}]})).unwrap();
        let (boot, mission) = bootstrap_mission(&initial, &bootstrap, Utc::now()).unwrap();
        let (observed, observation) = record_observation(
            &boot.state,
            &mission,
            "public venue portal",
            "Missing venue extraction",
            None,
            Utc::now(),
        )
        .unwrap();
        let (mut required, requirement) = record_requirement(
            &observed.state,
            &mission,
            "Extract venue listings from public HTML",
            &[observation],
            "Pass independent venue extraction checks",
        )
        .unwrap();
        let role_id = required
            .state
            .roles
            .values()
            .next()
            .unwrap()
            .role_id
            .clone();
        let responsibility = required.state.responsibilities.values_mut().next().unwrap();
        responsibility.requirement_id = Some(requirement.clone());
        let responsibility_id = responsibility.responsibility_id.clone();
        let requirement_record = required.state.requirements.get_mut(&requirement).unwrap();
        requirement_record.owner = Owner::Role(role_id.clone());
        requirement_record.status = RequirementStatus::Assigned;
        store
            .commit_state(
                0,
                &required.state,
                required.effects,
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        let suite = suite();
        let policy = AcquisitionPolicy {
            model: ModelConfig {
                model: MODEL.into(),
                max_tokens: 1024,
            },
            quotas,
            max_duration_seconds: 3600,
        };
        store.init_acquisitions(&policy).unwrap();
        let urls: Vec<_> = suite
            .cases
            .iter()
            .map(|case| case.source.url.clone())
            .collect();
        let intent: CreateAcquisition = serde_json::from_value(json!({
            "client_request_id":"runtime-intent", "mission_id":mission,"requirement_id":requirement,
            "role_id":role_id,"responsibility_id":responsibility_id,"suite_id":suite.suite_id,
            "candidate":{"capability_id":"venue-extractor","capability_version":"1","actor_name":"genesis-runtime-acquisition","source_version":"0.1"},
            "repair":null,"allowed_source_urls":urls,"examples":[{"url":urls[0],"html":"<html>PUBLIC_RUNTIME_EXAMPLE</html>"}],"deadline":null
        })).unwrap();
        let (record, created) = store
            .create_acquisition(&intent, &policy, &suite, Utc::now())
            .unwrap();
        assert!(created);
        let evidence = Arc::new(EvidencePolicy::new(vec![suite.clone()], vec![]).unwrap());
        Self {
            _directory: directory,
            path,
            store,
            kernel: Arc::new(Mutex::new(())),
            gate: Arc::new(Mutex::new(())),
            policy,
            evidence,
            suite,
            intent,
            id: record.id().into(),
        }
    }
    fn runtime(
        &self,
        model: Option<Arc<AnthropicGenerator>>,
        apify: Option<Arc<ApifyClient>>,
    ) -> AcquisitionRuntime {
        AcquisitionRuntime::new(
            self.store.clone(),
            self.evidence.clone(),
            self.kernel.clone(),
            self.gate.clone(),
            model,
            apify,
        )
    }
    fn restart(
        &self,
        model: Option<Arc<AnthropicGenerator>>,
        apify: Option<Arc<ApifyClient>>,
    ) -> AcquisitionRuntime {
        let store = Store::open(&self.path, OrgId::new("org-runtime")).unwrap();
        store.init_acquisitions(&self.policy).unwrap();
        AcquisitionRuntime::new(
            store,
            self.evidence.clone(),
            self.kernel.clone(),
            Arc::new(Mutex::new(())),
            model,
            apify,
        )
    }
    fn saved(&self) -> AcquisitionRecord {
        self.store.get_acquisition(&self.id, &self.suite).unwrap()
    }
    async fn role_state(&self, role_state: RoleState) {
        let _guard = self.kernel.lock().await;
        let previous = self.store.current_state().unwrap();
        let mut next = previous.clone();
        next.roles.get_mut(&self.intent.role_id).unwrap().state = role_state;
        next.revision += 1;
        self.store
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
    async fn prepare_forge(&self) -> CandidateSpec {
        let mock = Mock::serve(vec![Reply::json(generated())]).await;
        let runtime = self.runtime(Some(mock.model()), None);
        let generation = runtime.step(&self.id, false).await.unwrap();
        assert_eq!(
            generation.session().generation().phase(),
            SynthesisPhase::Generated
        );
        let forging = runtime.step(&self.id, false).await.unwrap();
        assert_eq!(forging.session().phase(), AcquisitionPhase::Forging);
        let requests = mock.finish().await;
        assert!(requests[0].contains(MODEL));
        assert!(!requests[0].contains("PRIVATE_RUNTIME_ORACLE_SENTINEL"));
        forging.session().forge().unwrap().spec().clone()
    }
}
fn snapshot(spec: &CandidateSpec) -> Value {
    json!({"versionNumber":spec.source_version,"sourceType":"SOURCE_FILES","envVars":[],"applyEnvVarsToBuild":false,
        "sourceFiles":spec.source.files().iter().map(|file|json!({"name":file.name,"content":file.content,"format":"TEXT"})).collect::<Vec<_>>()})
}
fn actor(spec: &CandidateSpec) -> Value {
    json!({"data":{"id":"actor-runtime","name":spec.actor_name,"isPublic":false,"actorPermissionLevel":"LIMITED_PERMISSIONS","versions":[snapshot(spec)]}})
}
fn build(spec: &CandidateSpec, status: &str) -> Value {
    json!({"data":{"id":"build-runtime","actId":"actor-runtime","buildNumber":"0.1.1","status":status,"actVersion":snapshot(spec)}})
}
fn run(index: usize) -> Value {
    json!({"data":{"id":format!("run-runtime-{index}"),"actId":"actor-runtime","buildId":"build-runtime","status":"SUCCEEDED","defaultDatasetId":format!("dataset-runtime-{index}")}})
}
fn build_replies(spec: &CandidateSpec) -> Vec<Reply> {
    vec![
        Reply::json(actor(spec)),
        Reply::json(json!({"data":snapshot(spec)})),
        Reply::json(build(spec, "RUNNING")),
    ]
}

#[tokio::test]
async fn unconfigured_providers_and_invalid_retry_never_mutate_intent_or_quota() {
    let f = Fixture::new(AcquisitionQuotas::default());
    let runtime = f.runtime(None, None);
    let before = serde_json::to_value(f.saved()).unwrap();
    let counters = f.store.acquisition_counters().unwrap();
    assert!(matches!(
        runtime.step(&f.id, false).await,
        Err(AcquisitionRuntimeError::Unconfigured("anthropic"))
    ));
    assert!(matches!(
        runtime.step(&f.id, true).await,
        Err(AcquisitionRuntimeError::UnexpectedRetry)
    ));
    assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
    assert_eq!(f.store.acquisition_counters().unwrap(), counters);
    let mut external = serde_json::to_value(&f.intent).unwrap();
    external["model"] = json!({"model":"client-override","max_tokens":9999});
    assert!(serde_json::from_value::<CreateAcquisition>(external).is_err());
    f.prepare_forge().await;
    let before = serde_json::to_value(f.saved()).unwrap();
    assert!(matches!(
        runtime.step(&f.id, false).await,
        Err(AcquisitionRuntimeError::Unconfigured("apify"))
    ));
    assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
}

#[tokio::test]
async fn concurrent_steps_share_gate_and_durable_model_debit_precedes_network() {
    let f = Fixture::new(AcquisitionQuotas::default());
    let mock = Mock::serve(vec![Reply::paused(generated())]).await;
    let runtime = f.runtime(Some(mock.model()), None);
    let first = tokio::spawn({
        let runtime = runtime.clone();
        let id = f.id.clone();
        async move { runtime.step(&id, false).await }
    });
    mock.entered.notified().await;
    let second_connection = Store::open(&f.path, OrgId::new("org-runtime")).unwrap();
    assert_eq!(
        second_connection
            .get_acquisition(&f.id, &f.suite)
            .unwrap()
            .session()
            .generation()
            .phase(),
        SynthesisPhase::Requesting
    );
    assert_eq!(
        second_connection
            .acquisition_counters()
            .unwrap()
            .model_calls,
        1
    );
    assert!(
        f.kernel.try_lock().is_ok(),
        "network must not hold kernel lock"
    );
    let second = tokio::spawn({
        let runtime = runtime.clone();
        let id = f.id.clone();
        async move { runtime.step(&id, false).await }
    });
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    mock.release.notify_one();
    first.await.unwrap().unwrap();
    assert_eq!(
        second.await.unwrap().unwrap().session().phase(),
        AcquisitionPhase::Forging
    );
    assert_eq!(f.store.acquisition_counters().unwrap().model_calls, 1);
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn revoked_grants_preserve_inflight_model_and_actor_receipts_and_never_revive() {
    for actor_effect in [false, true] {
        let f = Fixture::new(AcquisitionQuotas::default());
        let spec = if actor_effect {
            Some(f.prepare_forge().await)
        } else {
            None
        };
        let mock = Mock::serve(vec![Reply::paused(
            spec.as_ref().map(actor).unwrap_or_else(generated),
        )])
        .await;
        let runtime = if actor_effect {
            f.runtime(None, Some(mock.apify()))
        } else {
            f.runtime(Some(mock.model()), None)
        };
        let worker = tokio::spawn({
            let runtime = runtime.clone();
            let id = f.id.clone();
            async move { runtime.step(&id, false).await }
        });
        mock.entered.notified().await;
        f.role_state(RoleState::Draining).await;
        mock.release.notify_one();
        let record = worker.await.unwrap().unwrap();
        assert!(record.is_fenced());
        if actor_effect {
            assert!(serde_json::to_string(record.session().forge().unwrap())
                .unwrap()
                .contains("actor-runtime"));
        } else {
            assert_eq!(
                record
                    .session()
                    .generation()
                    .receipt()
                    .unwrap()
                    .message_id(),
                "msg_runtime"
            );
        }
        f.role_state(RoleState::Active).await;
        let before = serde_json::to_value(f.saved()).unwrap();
        assert!(runtime.step(&f.id, false).await.unwrap().is_fenced());
        assert!(matches!(
            runtime.verified_record(&f.id).await,
            Err(AcquisitionRuntimeError::Fenced)
        ));
        assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
        assert_eq!(f.store.acquisition_counters().unwrap().active, 0);
        assert_eq!(mock.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn cancelled_model_or_actor_post_recovers_without_replay_or_refunding_quota() {
    for actor_effect in [false, true] {
        let f = Fixture::new(AcquisitionQuotas::default());
        let spec = if actor_effect {
            Some(f.prepare_forge().await)
        } else {
            None
        };
        let mock = Mock::serve(vec![Reply::paused(
            spec.as_ref().map(actor).unwrap_or_else(generated),
        )])
        .await;
        let runtime = if actor_effect {
            f.runtime(None, Some(mock.apify()))
        } else {
            f.runtime(Some(mock.model()), None)
        };
        let worker = tokio::spawn({
            let runtime = runtime.clone();
            let id = f.id.clone();
            async move { runtime.step(&id, false).await }
        });
        mock.entered.notified().await;
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        mock.release.notify_one();
        assert_eq!(mock.finish().await.len(), 1);
        let restarted = f.restart(None, None);
        let record = restarted.step(&f.id, false).await.unwrap();
        assert_eq!(
            record.session().phase(),
            AcquisitionPhase::NeedsReconciliation
        );
        let counters = f.store.acquisition_counters().unwrap();
        assert_eq!(counters.model_calls, 1);
        assert_eq!(counters.actor_creates, u64::from(actor_effect));
        let before = serde_json::to_value(&record).unwrap();
        assert_eq!(restarted.recover_interrupted().await.unwrap(), 0);
        assert_eq!(
            serde_json::to_value(restarted.step(&f.id, false).await.unwrap()).unwrap(),
            before
        );
        assert_eq!(f.store.acquisition_counters().unwrap(), counters);
    }
}

#[tokio::test]
async fn cancelled_get_resumes_same_build_and_charges_only_a_new_read() {
    let f = Fixture::new(AcquisitionQuotas::default());
    let spec = f.prepare_forge().await;
    let mut replies = build_replies(&spec);
    replies.extend([
        Reply::paused(build(&spec, "SUCCEEDED")),
        Reply::json(build(&spec, "SUCCEEDED")),
    ]);
    let mock = Mock::serve(replies).await;
    let runtime = f.runtime(None, Some(mock.apify()));
    runtime.step(&f.id, false).await.unwrap();
    runtime.step(&f.id, false).await.unwrap();
    let worker = tokio::spawn({
        let runtime = runtime.clone();
        let id = f.id.clone();
        async move { runtime.step(&id, false).await }
    });
    mock.entered.notified().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    mock.release.notify_one();
    let pending = f.saved();
    assert_eq!(
        pending.session().forge().unwrap().pending_action(),
        Some(genesis_forge::ActionKind::PollBuild)
    );
    let restarted = f.restart(None, Some(mock.apify()));
    let recovered = restarted.step(&f.id, false).await.unwrap();
    assert_eq!(
        recovered
            .session()
            .forge()
            .unwrap()
            .artifact()
            .unwrap()
            .build_id,
        "build-runtime"
    );
    let counters = f.store.acquisition_counters().unwrap();
    assert_eq!(counters.actor_creates, 1);
    assert_eq!(counters.builds, 1);
    assert_eq!(counters.read_actions, 2);
    let requests = mock.finish().await;
    assert_eq!(
        requests.iter().filter(|r| r.starts_with("POST ")).count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("GET /v2/actor-builds/build-runtime "))
            .count(),
        2
    );
}

#[tokio::test]
async fn only_saved_failed_gets_allow_retry_and_failed_posts_remain_reconciliation() {
    let f = Fixture::new(AcquisitionQuotas::default());
    let spec = f.prepare_forge().await;
    let mut replies = build_replies(&spec);
    replies.extend([Reply::unavailable(), Reply::json(build(&spec, "SUCCEEDED"))]);
    let mock = Mock::serve(replies).await;
    let runtime = f.runtime(None, Some(mock.apify()));
    for _ in 0..3 {
        runtime.step(&f.id, false).await.unwrap();
    }
    let before = serde_json::to_value(f.saved()).unwrap();
    assert!(matches!(
        runtime.step(&f.id, false).await,
        Err(AcquisitionRuntimeError::RetryRequired)
    ));
    assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
    runtime.step(&f.id, true).await.unwrap();
    assert_eq!(f.store.acquisition_counters().unwrap().read_actions, 2);
    mock.finish().await;

    let f = Fixture::new(AcquisitionQuotas::default());
    f.prepare_forge().await;
    let mock = Mock::serve(vec![Reply::unavailable()]).await;
    let runtime = f.runtime(None, Some(mock.apify()));
    let record = runtime.step(&f.id, false).await.unwrap();
    assert_eq!(
        record.session().phase(),
        AcquisitionPhase::NeedsReconciliation
    );
    assert_eq!(
        runtime.step(&f.id, false).await.unwrap().version(),
        record.version()
    );
    assert!(matches!(
        runtime.step(&f.id, true).await,
        Err(AcquisitionRuntimeError::UnexpectedRetry)
    ));
    assert_eq!(f.store.acquisition_counters().unwrap().actor_creates, 1);
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn exhausted_model_quota_survives_restart_and_stops_before_new_intent() {
    let f = Fixture::new(AcquisitionQuotas {
        model_calls: 1,
        ..AcquisitionQuotas::default()
    });
    let mock = Mock::serve(vec![Reply::unavailable()]).await;
    let client = mock.model();
    let runtime = f.runtime(Some(client.clone()), None);
    assert_eq!(
        runtime.step(&f.id, false).await.unwrap().session().phase(),
        AcquisitionPhase::Failed
    );
    mock.finish().await;
    let mut intent = f.intent.clone();
    intent.client_request_id = "second-intent".into();
    let (second, created) = f
        .store
        .create_acquisition(&intent, &f.policy, &f.suite, Utc::now())
        .unwrap();
    assert!(created);
    let restarted = f.restart(Some(client), None);
    let before = serde_json::to_value(&second).unwrap();
    assert!(matches!(
        restarted.step(second.id(), false).await,
        Err(AcquisitionRuntimeError::Store(
            AcquisitionStoreError::QuotaExceeded("model_calls")
        ))
    ));
    assert_eq!(
        serde_json::to_value(restarted.status(second.id()).unwrap()).unwrap(),
        before
    );
    assert_eq!(f.store.acquisition_counters().unwrap().model_calls, 1);
}

#[tokio::test]
async fn full_provider_pipeline_produces_proof_only_while_original_grant_is_current() {
    let f = Fixture::new(AcquisitionQuotas::default());
    let spec = f.prepare_forge().await;
    let mut replies = build_replies(&spec);
    replies.push(Reply::json(build(&spec, "SUCCEEDED")));
    for (index, output) in outputs().into_iter().enumerate() {
        replies.extend([
            Reply::json(build(&spec, "SUCCEEDED")),
            Reply::json(run(index)),
            Reply::json(build(&spec, "SUCCEEDED")),
            Reply::json(run(index)),
            Reply::json(json!([output])),
        ]);
    }
    let mock = Mock::serve(replies).await;
    let runtime = f.runtime(None, Some(mock.apify()));
    for _ in 0..3 + 2 * f.suite.cases.len() {
        runtime.step(&f.id, false).await.unwrap();
    }
    let record = runtime.verified_record(&f.id).await.unwrap();
    assert!(!record.is_fenced());
    let verified = runtime.verified(&f.id).await.unwrap();
    assert_eq!(verified.artifact.build_id, "build-runtime");
    assert_eq!(verified.verification.cases.len(), 3);
    let counters = f.store.acquisition_counters().unwrap();
    assert_eq!(counters.active, 0);
    assert_eq!(counters.model_calls, 1);
    assert_eq!(counters.actor_creates, 1);
    assert_eq!(counters.builds, 1);
    assert_eq!(counters.test_submits, 3);
    assert_eq!(counters.read_actions, 4);
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 19);
    assert!(requests
        .iter()
        .all(|r| !r.contains("PRIVATE_RUNTIME_ORACLE_SENTINEL")));

    // Publication resolves the persisted acquisition through the real HTTP
    // evidence gate; caller-supplied verification claims are overwritten.
    let registry = HttpServer::start(genesis_server::registry::router(
        genesis_server::registry::RegistryState::open_in_memory(
            HashMap::from([("runtime-registry".into(), OrgId::new("org-runtime"))]),
            vec![],
        )
        .unwrap(),
    ))
    .await;
    let mut cfg = genesis_server::OrgConfig::dev("org-runtime", "Runtime org");
    cfg.dev_stub = false;
    cfg.db = Some(f.path.clone());
    cfg.oracle_suites = vec![f.suite.clone()];
    cfg.registry_url = Some(registry.url.clone());
    cfg.registry_token = Some("runtime-registry".into());
    cfg.acquisition = Some(genesis_server::AcquisitionConfig::offline(f.policy.clone()));
    let mut app = genesis_server::build_org(&cfg, None).unwrap();
    app.kernel = f.kernel.clone();
    app.acquisition_runtime = Arc::new(f.runtime(None, None));
    let api = HttpServer::start(app.router()).await;
    let manifest = json!({
        "capability_id":verified.artifact.capability_id,"version":verified.artifact.capability_version,
        "publisher":"org-runtime","source_digest":verified.artifact.source_sha256,
        "intent":"Read public venue fixtures","input_schema":{"type":"object"},"output_schema":{"type":"object"},
        "required_permissions":["capability.acquire"],
        "artifact":{"actor_id":verified.artifact.actor_id,"build_id":verified.artifact.build_id,"build_number":verified.artifact.build_number},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,"acceptance_suite_ref":"caller-cannot-select-oracle",
        "verification":{"passed":false,"cases":[],"tested_environment":"caller assertion","oracle_ref":"untrusted","verified_at":Utc::now()},
        "state":"APPROVED","published_at":null
    });
    let publication =
        json!({"manifest":manifest,"verification_ref":format!("acquisition:{}",f.id)});
    let client = reqwest::Client::new();
    let published = client
        .post(format!("{}/api/v1/capabilities/publish", api.url))
        .bearer_auth(&cfg.tokens.operator)
        .json(&publication)
        .send()
        .await
        .unwrap();
    let status = published.status();
    let published: Value = published.json().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::CREATED, "{published}");
    assert_eq!(published["verification"]["passed"], true);
    assert_eq!(published["acceptance_suite_ref"], f.suite.suite_id);
    assert_eq!(published["artifact"]["build_id"], "build-runtime");
    assert_eq!(published["source_digest"], verified.artifact.source_sha256);
    assert!(published["provenance"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == &json!(format!("genesis:forge:{}", verified.operation_id))));

    f.role_state(RoleState::Draining).await;
    let denied = client
        .post(format!("{}/api/v1/capabilities/publish", api.url))
        .bearer_auth(&cfg.tokens.operator)
        .json(&publication)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
    assert!(matches!(
        runtime.verified_record(&f.id).await,
        Err(AcquisitionRuntimeError::Fenced)
    ));
    assert!(f.saved().is_fenced());
    assert_eq!(f.saved().session().phase(), AcquisitionPhase::Verified);
    f.role_state(RoleState::Active).await;
    assert!(matches!(
        runtime.verified(&f.id).await,
        Err(AcquisitionRuntimeError::Fenced)
    ));
}
