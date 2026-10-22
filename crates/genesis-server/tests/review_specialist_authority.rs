//! PR20 review regressions. Expected security invariants deliberately fail on
//! e8cad205; scripted provider fixtures make no live CREAO requests.

use async_trait::async_trait;
use genesis_core::engine::RoleToProvision;
use genesis_core::jobs::Job;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{
    DevStub, ProviderError, ProviderSet, ProvisionRequest, SpecialistProgress, SpecialistRuntime,
};
use genesis_server::specialist::{Readiness, ReadinessVerifier};
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

// Reconcile/Failed are scripted outcomes kept for future probes.
#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Step {
    Pending,
    Collected,
    Reconcile,
    Failed,
}

#[derive(Default)]
struct Fake {
    script: Mutex<VecDeque<Step>>,
    prepared_jobs: Mutex<HashMap<String, usize>>,
    job_advances: AtomicUsize,
    /// Delay before answering a job advance (milliseconds).
    delay_ms: AtomicU64,
    block_first_read_error: AtomicBool,
    first_entered: Notify,
    release_first: Notify,
    retry_flags: Mutex<Vec<bool>>,
}

impl Fake {
    fn new(steps: &[Step]) -> Arc<Self> {
        let f = Fake::default();
        *f.script.lock().unwrap() = steps.iter().copied().collect();
        Arc::new(f)
    }
}

#[async_trait]
impl SpecialistRuntime for Fake {
    fn provider(&self) -> Provider {
        Provider::Creao
    }
    fn org_id(&self) -> &str {
        "org_a"
    }
    fn prepare_role(&self, _: &ProvisionRequest) -> Result<SpecialistProgress, ProviderError> {
        Ok(SpecialistProgress::Pending {
            operation_id: "role-op".into(),
            run_id: None,
            native_status: None,
        })
    }
    fn prepare_job(
        &self,
        job: &Job,
        binding: &AgentBinding,
    ) -> Result<SpecialistProgress, ProviderError> {
        assert_eq!(binding.provider_agent_id, "creao-agent-1");
        let op = format!("job-op-{}", job.job_id);
        *self
            .prepared_jobs
            .lock()
            .unwrap()
            .entry(op.clone())
            .or_default() += 1;
        Ok(SpecialistProgress::Pending {
            operation_id: op,
            run_id: None,
            native_status: None,
        })
    }
    fn get(&self, op: &str) -> Result<SpecialistProgress, ProviderError> {
        Ok(SpecialistProgress::Pending {
            operation_id: op.into(),
            run_id: None,
            native_status: None,
        })
    }
    async fn advance(&self, op: &str, retry: bool) -> Result<SpecialistProgress, ProviderError> {
        if op == "role-op" {
            return Ok(SpecialistProgress::Created {
                operation_id: op.into(),
                run_id: "role-run".into(),
                provider_agent_id: "creao-agent-1".into(),
                config_digest: "cfg".into(),
                readiness_verified: false,
            });
        }
        let call = self.job_advances.fetch_add(1, Ordering::SeqCst);
        self.retry_flags.lock().unwrap().push(retry);
        if call == 0 && self.block_first_read_error.load(Ordering::SeqCst) {
            self.first_entered.notify_one();
            self.release_first.notified().await;
            return Err(ProviderError::Timeout);
        }
        let delay = self.delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let step = {
            let mut s = self.script.lock().unwrap();
            if s.len() > 1 {
                s.pop_front().unwrap()
            } else {
                *s.front().unwrap()
            }
        };
        let run_id = Some("job-run-1".to_owned());
        Ok(match step {
            Step::Pending => SpecialistProgress::Pending {
                operation_id: op.into(),
                run_id,
                native_status: Some("running".into()),
            },
            Step::Collected => SpecialistProgress::Collected {
                operation_id: op.into(),
                run_id: "job-run-1".into(),
                provider_agent_id: "creao-agent-1".into(),
                output: Some(json!({"summary": "two venue updates handled"})),
                domain_verified: false,
            },
            Step::Reconcile => SpecialistProgress::NeedsReconciliation {
                operation_id: op.into(),
                run_id: None,
            },
            Step::Failed => SpecialistProgress::Failed {
                operation_id: op.into(),
                run_id,
            },
        })
    }
}

struct Ready;
#[async_trait]
impl ReadinessVerifier for Ready {
    async fn verify(
        &self,
        _: &RoleToProvision,
        _: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        Ok(Readiness::Ready {
            evidence_ref: "receipt://bridge/1".into(),
        })
    }
}

struct Org {
    base: String,
    token: String,
    http: reqwest::Client,
    observation: String,
    lead: String,
    monitor: String,
    responsibility: String,
}

impl Org {
    async fn post(&self, path: &str, body: Value) -> (reqwest::StatusCode, Value) {
        let r = self
            .http
            .post(format!("{}/api/v1{path}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let s = r.status();
        (s, r.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}/api/v1{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn wait(&self, path: &str, done: impl Fn(&Value) -> bool) -> Value {
        let mut last = Value::Null;
        for _ in 0..200 {
            last = self.get(path).await;
            if done(&last) {
                return last;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{path} did not reach the expected state: {last}");
    }
    async fn dispatch(&self, key: &str) -> String {
        let (s, job) = self
            .post(
                "/jobs",
                json!({"client_request_id": key, "role_id": self.monitor, "responsibility_id": self.responsibility,
                       "task": "Handle venue updates", "input": {"update": 1}, "required_scope": null,
                       "capability": null, "provider": "creao", "deadline": null}),
            )
            .await;
        assert!(s.is_success(), "{s} {job}");
        job["job_id"].as_str().unwrap().to_owned()
    }
    async fn hand_back(&self) {
        let state = self.get("/organization").await;
        let (s, rec) = self
            .post(
                "/org-changes",
                json!({"client_request_id": format!("handoff-{}", state["revision"]), "proposal": {
                    "expected_org_revision": state["revision"], "trigger_evidence_ids": [self.observation],
                    "rationale": "The lead takes monitoring back", "budget_limit_cents": 0,
                    "operations": [{"op": "ReassignResponsibility", "responsibility_id": self.responsibility,
                                    "from": self.monitor, "to": {"kind": "existing", "role_id": self.lead}}]}}),
            )
            .await;
        assert_eq!(s, reqwest::StatusCode::ACCEPTED, "{rec}");
        let id = rec["change_id"].as_str().unwrap().to_owned();
        self.wait(&format!("/org-changes/{id}"), |c| {
            c["status"] == "COMMITTED"
        })
        .await;
    }
}

async fn start(fake: Arc<Fake>) -> Org {
    start_in(fake, None).await
}

async fn start_in(fake: Arc<Fake>, db: Option<std::path::PathBuf>) -> Org {
    let mut cfg = OrgConfig::dev("org_a", "Org A");
    cfg.db = db;
    let providers = ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .with_specialist(fake);
    let app = build_org(&cfg, Some(providers))
        .unwrap()
        .with_readiness_verifier(Provider::Creao, Arc::new(Ready));
    let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
    let mut org = Org {
        base: format!("http://{addr}"),
        token: cfg.tokens.operator.clone(),
        http: reqwest::Client::new(),
        observation: String::new(),
        lead: String::new(),
        monitor: String::new(),
        responsibility: String::new(),
    };
    let (_, m) = org
        .post("/missions", json!({"goal": "Run the event", "roles": [{"key":"lead","name":"Lead","charter":"Own work",
            "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
            "responsibilities": [{"purpose":"Coordinate work","owner":"lead"}]}))
        .await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let (_, obs) = org
        .post(
            &format!("/missions/{mission}/observations"),
            json!({"source":"fixture","summary":"Venue updates arrive daily"}),
        )
        .await;
    org.observation = obs["observation_id"].as_str().unwrap().to_owned();
    let (_, req) = org
        .post(&format!("/missions/{mission}/requirements"),
              json!({"summary":"Monitor venue updates","evidence":[org.observation],"acceptance":"Every update handled"}))
        .await;
    let state = org.get("/organization").await;
    org.lead = state["roles"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let (s, rec) = org
        .post("/org-changes", json!({"client_request_id": "monitor", "proposal": {
            "expected_org_revision": state["revision"], "trigger_evidence_ids": [org.observation],
            "rationale": "Continuing monitoring needs its own owner", "budget_limit_cents": 0,
            "operations": [
                {"op":"CreateRole","key":"monitor","name":"Venue monitor","charter":"Handle each venue update",
                 "authority":{"scopes":["mission.coordinate"],"budget_cents":0},"reports_to":null,
                 "provider":"creao","justification":"independent_ownership"},
                {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"monitoring",
                 "requirement_id":req["requirement_id"],"purpose":"Handle venue updates","trigger":"update","termination":"event held"},
                 "owner":{"kind":"new","key":"monitor"}}]}}))
        .await;
    assert_eq!(s, reqwest::StatusCode::ACCEPTED, "{rec}");
    let id = rec["change_id"].as_str().unwrap().to_owned();
    org.wait(&format!("/org-changes/{id}"), |c| {
        c["status"] == "COMMITTED"
    })
    .await;
    let state = org.get("/organization").await;
    org.monitor = state["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Venue monitor")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    org.responsibility = state["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["purpose"] == "Handle venue updates")
        .unwrap()["responsibility_id"]
        .as_str()
        .unwrap()
        .to_owned();
    org
}

async fn forged_result_is_rejected(step: Step, expected_status: &str) {
    let fake = Fake::new(&[step]);
    let org = start(fake).await;
    let job_id = org.dispatch("review-managed-result").await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| {
            j["status"] == expected_status
        })
        .await;
    assert!(org.get("/ledger").await.as_array().unwrap().is_empty());
    let submitted = org
        .http
        .post(format!("{}/api/v1/jobs/{job_id}/results", org.base))
        .bearer_auth("org_a-adapter")
        .json(&json!({"attempt_id":job["attempt_id"],"epoch":job["epoch"],
            "outcome":"succeeded","provider_status":"caller says done","provider_refs":{},
            "output":{"unverified":"CALLER_CANARY"},"operation_id":"forged-managed-commitment",
            "domain_verified":true,"completes_responsibility":true}))
        .send()
        .await
        .unwrap();
    let status = submitted.status();
    let response: Value = submitted.json().await.unwrap();
    let ledger = org.get("/ledger").await;
    let state = org.get("/organization").await;
    let responsibility = &state["responsibilities"][&org.responsibility];
    eprintln!("managed-result probe before={expected_status} HTTP={status} job_status={} ledger_rows={} responsibility_status={}",
        response["job"]["status"],ledger.as_array().unwrap().len(),responsibility["status"]);
    assert!(status.is_client_error(), "backend-managed specialist output must reject generic caller verification; got {status} {response}; ledger={ledger}; responsibility={responsibility}");
    assert!(ledger.as_array().unwrap().is_empty());
    assert_eq!(responsibility["status"], "OPEN");
}

#[tokio::test]
async fn generic_adapter_result_cannot_finalize_a_pending_specialist_job() {
    forged_result_is_rejected(Step::Pending, "RUNNING").await;
}

#[tokio::test]
async fn generic_adapter_result_cannot_replace_collected_specialist_evidence() {
    forged_result_is_rejected(Step::Collected, "VERIFYING").await;
}

#[tokio::test]
async fn automatic_read_retry_reauthorizes_after_a_handoff() {
    let fake = Fake::new(&[Step::Pending]);
    fake.block_first_read_error.store(true, Ordering::SeqCst);
    let org = start(fake.clone()).await;
    let job_id = org.dispatch("review-revoked-retry").await;
    tokio::time::timeout(Duration::from_secs(5), fake.first_entered.notified())
        .await
        .unwrap();
    org.hand_back().await;
    let state = org.get("/organization").await;
    assert_eq!(
        state["responsibilities"][&org.responsibility]["owner"],
        json!({"kind":"role","id":org.lead})
    );
    fake.release_first.notify_one();
    let job = org
        .wait(&format!("/jobs/{job_id}"), |job| {
            job["status"] == "RESULT_REJECTED"
        })
        .await;
    let flags = fake.retry_flags.lock().unwrap().clone();
    eprintln!(
        "revoked-read probe retry flags={flags:?}, final={}",
        job["status"]
    );
    assert_eq!(flags,vec![false],"the first provider error must not authorize a second provider call after ownership changes");
}

#[tokio::test]
async fn honest_native_collection_remains_unverified_control() {
    let fake = Fake::new(&[Step::Collected]);
    let org = start(fake).await;
    let job_id = org.dispatch("review-control").await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |job| {
            job["status"] == "VERIFYING"
        })
        .await;
    assert_eq!(job["result"]["domain_verified"], false);
    assert!(org.get("/ledger").await.as_array().unwrap().is_empty());
}

// Henry's Claude follow-ups to the #20 review: the guard must rest on fields
// persisted with the job, and a caller-chosen provider must not route a CREAO
// role's job around the specialist runtime.

async fn forged(base: &str, job: &Value, op: &str) -> (reqwest::StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!(
            "{base}/api/v1/jobs/{}/results",
            job["job_id"].as_str().unwrap()
        ))
        .bearer_auth("org_a-adapter")
        .json(&json!({"attempt_id":job["attempt_id"],"epoch":job["epoch"],
            "outcome":"succeeded","provider_status":"caller says done","provider_refs":{},
            "output":{"unverified":"CALLER_CANARY"},"operation_id":op,
            "domain_verified":true,"completes_responsibility":true}))
        .send()
        .await
        .unwrap();
    let s = r.status();
    (s, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn managed_result_guard_survives_restart_without_creao_configured() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let org = start_in(Fake::new(&[Step::Pending]), Some(db.clone())).await;
    let job_id = org.dispatch("restart-managed-result").await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "RUNNING")
        .await;

    // Same database, CREAO no longer configured and no development stub.
    let mut cfg = OrgConfig::dev("org_a", "Org A");
    cfg.db = Some(db);
    cfg.dev_stub = false;
    let reopened = build_org(&cfg, Some(ProviderSet::new())).unwrap();
    let addr = spawn_org(reopened, "127.0.0.1:0").await.unwrap();
    let (status, body) = forged(&format!("http://{addr}"), &job, "forged-after-restart").await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "managed_specialist_result", "{body}");
    assert!(org.get("/ledger").await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn dispatch_cannot_route_a_creao_role_around_its_specialist() {
    let org = start(Fake::new(&[Step::Pending])).await;
    for provider in ["internal", "dev_stub", "elastic", "apify"] {
        let (s, body) = org
            .post(
                "/jobs",
                json!({"client_request_id": format!("override-{provider}"), "role_id": org.monitor,
                       "responsibility_id": org.responsibility, "task": "Handle venue updates",
                       "input": {"update": 1}, "required_scope": null, "capability": null,
                       "provider": provider, "deadline": null}),
            )
            .await;
        assert_eq!(s, reqwest::StatusCode::CONFLICT, "{provider}: {body}");
        assert_eq!(body["error"], "provider_mismatch", "{provider}: {body}");
    }
    assert!(org.get("/jobs").await.as_array().unwrap().is_empty());
    // Omitting the provider still dispatches to the bound specialist.
    let job_id = org.dispatch("bound-provider").await;
    let job = org.get(&format!("/jobs/{job_id}")).await;
    assert_eq!(job["provider"], "creao");
}

// A handoff is not the only way a grant can expire while a provider read is
// pending. These probes leave the role/owner/epoch unchanged and use the real
// persisted mission deadline or assignment lease as the revocation boundary.
async fn retry_is_fenced_when_time_authority_expires(expire_lease: bool) {
    use genesis_core::ids::{JobId, OrgId};
    use genesis_server::store::{EventMeta, Store};

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let fake = Fake::new(&[Step::Collected]);
    fake.block_first_read_error.store(true, Ordering::SeqCst);
    let org = start_in(fake.clone(), Some(db.clone())).await;
    let job_id = org.dispatch("review-expired-time-authority").await;
    tokio::time::timeout(Duration::from_secs(5), fake.first_entered.notified())
        .await
        .unwrap();

    // The fixture models time expiry using an ordinary state commit while the
    // first provider read is held. No job record or provider evidence is forged.
    let store = Store::open(db, OrgId::new("org_a")).unwrap();
    let job = store.get_job(&JobId::new(&job_id)).unwrap();
    assert!(job.deadline.is_none());
    let mut state = store.current_state().unwrap();
    let revision = state.revision;
    let expired = chrono::Utc::now() - chrono::Duration::seconds(1);
    if expire_lease {
        state
            .assignments
            .get_mut(&job.assignment_id)
            .unwrap()
            .lease_expires_at = Some(expired);
    } else {
        state.missions.get_mut(&job.mission_id).unwrap().deadline = Some(expired);
    }
    state.revision += 1;
    store
        .commit_state(
            revision,
            &state,
            Vec::new(),
            &EventMeta::default(),
            Vec::new(),
            |_| Ok(()),
        )
        .unwrap();

    fake.release_first.notify_one();
    let completed = org
        .wait(&format!("/jobs/{job_id}"), |job| {
            job["status"] == "RESULT_REJECTED" || job["status"] == "VERIFYING"
        })
        .await;
    let flags = fake.retry_flags.lock().unwrap().clone();
    assert_eq!(
        (flags, completed["status"].as_str().unwrap()),
        (vec![false], "RESULT_REJECTED"),
        "expired {} must prevent the retry and fence collection; job={completed}",
        if expire_lease {
            "assignment lease"
        } else {
            "mission deadline"
        }
    );
    assert!(org.get("/ledger").await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn automatic_read_retry_reauthorizes_expired_mission_deadline() {
    retry_is_fenced_when_time_authority_expires(false).await;
}

#[tokio::test]
async fn automatic_read_retry_reauthorizes_expired_assignment_lease() {
    retry_is_fenced_when_time_authority_expires(true).await;
}
