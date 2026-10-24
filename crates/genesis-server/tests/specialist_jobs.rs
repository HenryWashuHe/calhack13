//! Jobs for roles served by an asynchronous specialist runtime (CREAO-style):
//! one durable operation per job attempt, live authority before each advance
//! and before applying a result, and VERIFYING rather than success.

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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    async fn advance(&self, op: &str, _retry: bool) -> Result<SpecialistProgress, ProviderError> {
        if op == "role-op" {
            return Ok(SpecialistProgress::Created {
                operation_id: op.into(),
                run_id: "role-run".into(),
                provider_agent_id: "creao-agent-1".into(),
                config_digest: "cfg".into(),
                readiness_verified: false,
            });
        }
        self.job_advances.fetch_add(1, Ordering::SeqCst);
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
    let cfg = OrgConfig::dev("org_a", "Org A");
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

#[tokio::test]
async fn collected_output_waits_for_verification() {
    let fake = Fake::new(&[Step::Pending, Step::Collected]);
    let org = start(fake.clone()).await;
    let job_id = org.dispatch("job-1").await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "VERIFYING")
        .await;
    assert_eq!(job["result"]["kind"], "specialist_run_output");
    assert_eq!(job["result"]["domain_verified"], false);
    assert_eq!(
        job["result"]["output"]["summary"],
        "two venue updates handled"
    );
    assert_eq!(job["provider_refs"]["run_id"], "job-run-1");
    assert_eq!(
        job["provider_refs"]["operation_id"],
        format!("job-op-{job_id}")
    );
    assert_eq!(
        fake.prepared_jobs.lock().unwrap().values().sum::<usize>(),
        1,
        "one durable operation"
    );
    assert!(
        org.get("/ledger").await.as_array().unwrap().is_empty(),
        "no commitment"
    );
}

#[tokio::test]
async fn authority_lost_while_running_stops_advancing() {
    let fake = Fake::new(&[Step::Pending]);
    let org = start(fake.clone()).await;
    let job_id = org.dispatch("job-2").await;
    org.wait(&format!("/jobs/{job_id}"), |j| j["status"] == "RUNNING")
        .await;
    org.hand_back().await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| {
            j["status"] == "RESULT_REJECTED"
        })
        .await;
    assert!(job["rejection_reason"]
        .as_str()
        .unwrap()
        .contains("authority lost before advance"));
    let advances = fake.job_advances.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        fake.job_advances.load(Ordering::SeqCst),
        advances,
        "no advance after authority loss"
    );
}

#[tokio::test]
async fn result_collected_after_a_handoff_is_fenced() {
    let fake = Fake::new(&[Step::Collected]);
    fake.delay_ms.store(900, Ordering::SeqCst);
    let org = start(fake.clone()).await;
    let job_id = org.dispatch("job-3").await;
    // Wait until an advance is in flight (the fake answers after 900 ms).
    for _ in 0..100 {
        if fake.job_advances.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(fake.job_advances.load(Ordering::SeqCst) > 0);
    org.hand_back().await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| {
            j["status"] == "RESULT_REJECTED"
        })
        .await;
    let reason = job["rejection_reason"].as_str().unwrap();
    assert!(reason.contains("late result fenced"), "{reason}");
    assert!(job["result"].is_null());
}

#[tokio::test]
async fn unknown_outcome_and_failure_are_not_success() {
    let fake = Fake::new(&[Step::Reconcile]);
    let org = start(fake).await;
    let job_id = org.dispatch("job-4").await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "UNKNOWN")
        .await;
    assert!(job["rejection_reason"]
        .as_str()
        .unwrap()
        .contains("reconcile"));

    let fake = Fake::new(&[Step::Failed]);
    let org = start(fake).await;
    let job_id = org.dispatch("job-5").await;
    let job = org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "FAILED")
        .await;
    assert_eq!(job["provider_refs"]["run_id"], "job-run-1");
}
