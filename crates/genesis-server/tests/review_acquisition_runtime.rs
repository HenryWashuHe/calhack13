//! Review test for the post-I/O checkpoint guard in
//! `AcquisitionRuntime::step`: after a model or Actor POST returns, the
//! runtime re-reads the saved acquisition and must refuse to apply the
//! completion when the checkpoint changed while the request was in flight.
//!
//! Inside one runtime the shared gate serializes every runtime entry point, so
//! the in-flight step can only be raced by a second runtime over the same
//! database: a second process (the module documents that the gate is not a
//! distributed lease and that database CAS rejects conflicting writes). The
//! fixture's `restart()` models that process with its own store connection and
//! its own gate. Its local reconcile marks the interrupted POST ambiguous and
//! saves a newer version while the original request is still paused.
//!
//! Fixtures are trimmed copies from `tests/acquisition_runtime.rs`. Providers
//! are loopback only; generated source is never executed.
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::engine::{
    bootstrap_mission, record_observation, record_requirement, MissionBootstrap,
};
use genesis_core::ids::*;
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{OrgState, Owner};
use genesis_forge::{
    AcquisitionPhase, AnthropicGenerator, CandidateSpec, ModelConfig, SynthesisPhase,
};
use genesis_server::capability_acquisition_runtime::{AcquisitionRuntime, AcquisitionRuntimeError};
use genesis_server::capability_acquisition_store::{
    AcquisitionPolicy, AcquisitionQuotas, AcquisitionRecord, CreateAcquisition,
};
use genesis_server::capability_evidence::EvidencePolicy;
use genesis_server::store::{EventMeta, Store};
use genesis_verification::{decode_json, OracleSuite};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

const MODEL: &str = "claude-sonnet-4-6";
const KEY: &str = "review-acquisition-runtime-fixture-key";
const PARSER: &str = "export function extract() { throw new Error('generated source is never executed on this host'); }\n";

struct Reply {
    body: Value,
    pause: bool,
}
impl Reply {
    fn json(body: Value) -> Self {
        Self { body, pause: false }
    }
    fn paused(body: Value) -> Self {
        Self { body, pause: true }
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
                let response = format!("HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
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
    json!({"id":"msg_review_runtime", "type":"message", "role":"assistant", "model":MODEL,
        "content":[{"type":"text","text":json!({"extractor_typescript":PARSER}).to_string()}],
        "stop_reason":"end_turn", "stop_sequence":null, "usage":{"input_tokens":100,"output_tokens":60}})
}
fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.scope = "PRIVATE_REVIEW_RUNTIME_ORACLE_SENTINEL".into();
    suite
}
fn snapshot(spec: &CandidateSpec) -> Value {
    json!({"versionNumber":spec.source_version,"sourceType":"SOURCE_FILES","envVars":[],"applyEnvVarsToBuild":false,
        "sourceFiles":spec.source.files().iter().map(|file|json!({"name":file.name,"content":file.content,"format":"TEXT"})).collect::<Vec<_>>()})
}
fn actor(spec: &CandidateSpec) -> Value {
    json!({"data":{"id":"actor-review-runtime","name":spec.actor_name,"isPublic":false,"actorPermissionLevel":"LIMITED_PERMISSIONS","versions":[snapshot(spec)]}})
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
    id: String,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("organization.db");
        let store = Store::open(&path, OrgId::new("org-review-runtime")).unwrap();
        let mut constitution = genesis_server::demo_constitution();
        constitution.scopes.insert("capability.acquire".into());
        let initial = OrgState::new(
            OrgId::new("org-review-runtime"),
            "Review runtime org",
            constitution,
        );
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
            quotas: AcquisitionQuotas::default(),
            max_duration_seconds: 3600,
        };
        store.init_acquisitions(&policy).unwrap();
        let urls: Vec<_> = suite
            .cases
            .iter()
            .map(|case| case.source.url.clone())
            .collect();
        let intent: CreateAcquisition = serde_json::from_value(json!({
            "client_request_id":"review-runtime-intent", "mission_id":mission,"requirement_id":requirement,
            "role_id":role_id,"responsibility_id":responsibility_id,"suite_id":suite.suite_id,
            "candidate":{"capability_id":"venue-extractor","capability_version":"1","actor_name":"genesis-review-runtime","source_version":"0.1"},
            "repair":null,"allowed_source_urls":urls,"examples":[{"url":urls[0],"html":"<html>PUBLIC_REVIEW_EXAMPLE</html>"}],"deadline":null
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
    /// A second process over the same database: new connection, new gate.
    fn second_process(&self) -> AcquisitionRuntime {
        let store = Store::open(&self.path, OrgId::new("org-review-runtime")).unwrap();
        store.init_acquisitions(&self.policy).unwrap();
        AcquisitionRuntime::new(
            store,
            self.evidence.clone(),
            Arc::new(Mutex::new(())),
            Arc::new(Mutex::new(())),
            None,
            None,
        )
    }
    fn saved(&self) -> AcquisitionRecord {
        self.store.get_acquisition(&self.id, &self.suite).unwrap()
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
        assert_eq!(mock.finish().await.len(), 1);
        forging.session().forge().unwrap().spec().clone()
    }
}

#[tokio::test]
async fn completion_after_a_concurrent_checkpoint_change_is_refused_without_overwrite() {
    for actor_effect in [false, true] {
        let f = Fixture::new();
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
        // The debited intent is durable before the provider sees the request.
        let in_flight = f.saved();

        // Another process reconciles the same checkpoint while the POST is
        // paused: the interrupted request becomes ambiguous at version + 1.
        let other = f.second_process();
        let changed = other
            .reconcile_if_current(&f.id, in_flight.version())
            .await
            .unwrap();
        assert_eq!(changed.version(), in_flight.version() + 1);
        assert_eq!(
            changed.session().phase(),
            AcquisitionPhase::NeedsReconciliation
        );
        let newer = serde_json::to_value(f.saved()).unwrap();
        assert_eq!(newer, serde_json::to_value(&changed).unwrap());
        let counters = f.store.acquisition_counters().unwrap();

        // The original response now arrives. Its completion belongs to the
        // superseded checkpoint and must be refused with the specific error.
        mock.release.notify_one();
        let completed = worker.await.unwrap();
        assert!(
            matches!(completed, Err(AcquisitionRuntimeError::ConcurrentChange)),
            "expected ConcurrentChange, got {:?}",
            completed.as_ref().map(|record| record.version())
        );
        assert_eq!(serde_json::to_value(f.saved()).unwrap(), newer);
        assert_eq!(
            f.saved().session().phase(),
            AcquisitionPhase::NeedsReconciliation
        );
        assert_eq!(f.store.acquisition_counters().unwrap(), counters);
        assert_eq!(mock.finish().await.len(), 1);
    }
}
