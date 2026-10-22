//! Jobs for Elastic specialists are delivered as Agent Builder converse turns,
//! with live authority checks before the call and before applying the answer.
//! Uses the in-process Kibana fake.

use axum::http::StatusCode;
use genesis_elastic::fake::{start_kibana, KibanaShared};
use genesis_elastic::{AgentBuilder, AgentBuilderConfig};
use genesis_providers::{DevStub, ProviderSet};
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::time::Duration;

struct Org {
    base: String,
    cfg: OrgConfig,
    http: reqwest::Client,
    kibana: KibanaShared,
}

impl Org {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = self
            .http
            .request(method, format!("{}/api/v1{path}", self.base))
            .bearer_auth(&self.cfg.tokens.operator);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }
    async fn get(&self, path: &str) -> Value {
        self.call(reqwest::Method::GET, path, None).await.1
    }
    async fn wait<F: Fn(&Value) -> bool>(&self, path: &str, done: F) -> Value {
        let mut last = Value::Null;
        for _ in 0..200 {
            last = self.get(path).await;
            if done(&last) {
                return last;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{path} never reached the expected state: {last}");
    }
}

struct Setup {
    org: Org,
    observation: String,
    lead: String,
    specialist: String,
    responsibility: String,
}

/// An organization with a committed Elastic specialist that owns a responsibility.
async fn setup() -> Setup {
    let (url, kibana) = start_kibana().await;
    let builder = AgentBuilder::new(AgentBuilderConfig {
        kibana_url: url,
        api_key: "bWdtdA==".into(),
        space: None,
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    let cfg = OrgConfig::dev("org_a", "Org A");
    let providers = ProviderSet::new().with_dev_stub(DevStub::default());
    let app = build_org(&cfg, Some(providers))
        .unwrap()
        .with_elastic_agents(builder);
    let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
    let org = Org {
        base: format!("http://{addr}"),
        cfg,
        http: reqwest::Client::new(),
        kibana,
    };

    let (_, m) = org
        .post(
            "/missions",
            json!({
                "goal": "Run the campus event",
                "roles": [{"key": "lead", "name": "Lead", "charter": "Owns the plan",
                           "authority": {"scopes": ["mission.coordinate"], "budget_cents": 0}}],
                "responsibilities": [{"purpose": "Coordinate the mission", "owner": "lead"}]
            }),
        )
        .await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let (_, obs) = org
        .post(
            &format!("/missions/{mission}/observations"),
            json!({"source": "venue-portal", "summary": "Main hall flooded"}),
        )
        .await;
    let observation = obs["observation_id"].as_str().unwrap().to_owned();
    let (_, req) = org
        .post(
            &format!("/missions/{mission}/requirements"),
            json!({"summary": "Find a replacement venue", "evidence": [observation], "acceptance": "venue chosen"}),
        )
        .await;
    let state = org.get("/organization").await;
    let lead = state["roles"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let (s, rec) = org
        .post(
            "/org-changes",
            json!({
                "client_request_id": "create-specialist",
                "proposal": {
                    "expected_org_revision": state["revision"],
                    "trigger_evidence_ids": [observation],
                    "rationale": "Venue research is continuing work",
                    "budget_limit_cents": 0,
                    "operations": [
                        {"op": "CreateRole", "key": "research", "name": "Venue researcher",
                         "charter": "Research replacement venues", "reports_to": null,
                         "authority": {"scopes": ["mission.coordinate"], "budget_cents": 0},
                         "provider": "elastic", "justification": "independent_ownership"},
                        {"op": "AssignResponsibility",
                         "target": {"kind": "from_requirement", "key": "research", "requirement_id": req["requirement_id"],
                                    "purpose": "Research replacement venues", "trigger": "venue news", "termination": "venue chosen"},
                         "owner": {"kind": "new", "key": "research"}}
                    ]
                }
            }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{rec}");
    let id = rec["change_id"].as_str().unwrap().to_owned();
    org.wait(&format!("/org-changes/{id}"), |c| {
        c["status"] == "COMMITTED"
    })
    .await;
    let state = org.get("/organization").await;
    let specialist = state["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Venue researcher")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let responsibility = state["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["purpose"] == "Research replacement venues")
        .unwrap()["responsibility_id"]
        .as_str()
        .unwrap()
        .to_owned();
    Setup {
        org,
        observation,
        lead,
        specialist,
        responsibility,
    }
}

async fn dispatch(s: &Setup, key: &str, task: &str) -> String {
    let (status, job) = s
        .org
        .post(
            "/jobs",
            json!({
                "client_request_id": key,
                "role_id": s.specialist,
                "responsibility_id": s.responsibility,
                "task": task,
                "input": {"attendees": 120},
                "required_scope": null,
                "capability": null,
                "provider": "elastic",
                "deadline": null
            }),
        )
        .await;
    assert!(status.is_success(), "{status} {job}");
    job["job_id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn elastic_job_becomes_a_converse_turn_and_waits_for_verification() {
    let s = setup().await;
    let job_id = dispatch(&s, "job-1", "Find venues for 120 people").await;
    let job = s
        .org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "VERIFYING")
        .await;
    let agent = job["provider_refs"]["agent_id"].as_str().unwrap();
    assert!(agent.starts_with("genesis-org_a-research-"));
    assert!(job["provider_refs"]["conversation_id"].is_string());
    let result = &job["result"];
    assert_eq!(result["kind"], "elastic_agent_answer");
    assert_eq!(result["domain_verified"], false);
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Find venues for 120 people"));
    assert_eq!(result["tool_calls"].as_array().unwrap().len(), 1);
    assert!(result["tool_calls"][0]
        .as_str()
        .unwrap()
        .ends_with("-evidence-search"));
    // Never a commitment; the job is not SUCCEEDED.
    assert!(s.org.get("/ledger").await.as_array().unwrap().is_empty());
    assert_eq!(s.org.kibana.lock().unwrap().posts.get("converse"), Some(&1));
}

#[tokio::test]
async fn answer_arriving_after_a_handoff_is_fenced() {
    let s = setup().await;
    s.org.kibana.lock().unwrap().converse_delay_ms = 800;
    let job_id = dispatch(&s, "job-late", "Compare the candidate venues").await;
    s.org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "RUNNING")
        .await;
    // Hand the responsibility back to the lead while the turn is in flight.
    let state = s.org.get("/organization").await;
    let (status, rec) = s
        .org
        .post(
            "/org-changes",
            json!({
                "client_request_id": "handoff",
                "proposal": {
                    "expected_org_revision": state["revision"],
                    "trigger_evidence_ids": [s.observation],
                    "rationale": "The lead takes venue research back",
                    "budget_limit_cents": 0,
                    "operations": [{"op": "ReassignResponsibility", "responsibility_id": s.responsibility,
                                    "from": s.specialist, "to": {"kind": "existing", "role_id": s.lead}}]
                }
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{rec}");
    let job = s
        .org
        .wait(&format!("/jobs/{job_id}"), |j| {
            j["status"] == "RESULT_REJECTED"
        })
        .await;
    assert!(job["rejection_reason"]
        .as_str()
        .unwrap()
        .contains("late result fenced"));
    assert!(job["result"].is_null(), "a fenced answer is not applied");
}

#[tokio::test]
async fn a_round_waiting_for_a_human_prompt_fails_the_job() {
    let s = setup().await;
    s.org.kibana.lock().unwrap().awaiting_prompt = true;
    let job_id = dispatch(&s, "job-prompt", "Book the venue").await;
    let job = s
        .org
        .wait(&format!("/jobs/{job_id}"), |j| j["status"] == "FAILED")
        .await;
    assert!(job["rejection_reason"]
        .as_str()
        .unwrap()
        .contains("human prompt"));
}
