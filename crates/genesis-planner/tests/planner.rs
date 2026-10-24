//! Planner against a real in-process kernel, with scripted model replies.
//! The replay model stands in for an LLM; the kernel, validation and saga are real.

use genesis_core::ids::ObservationId;
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

struct Org {
    url: String,
    operator: String,
    agent: String,
    http: reqwest::Client,
}

impl Org {
    async fn start() -> Org {
        let cfg = OrgConfig::dev("org_a", "Org A");
        let app = build_org(&cfg, None).unwrap();
        let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
        Org {
            url: format!("http://{addr}"),
            operator: cfg.tokens.operator.clone(),
            agent: cfg.tokens.agent.clone(),
            http: reqwest::Client::new(),
        }
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        self.http
            .post(format!("{}/api/v1{}", self.url, path))
            .bearer_auth(&self.operator)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}/api/v1{}", self.url, path))
            .bearer_auth(&self.operator)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    fn kernel(&self) -> KernelClient {
        KernelClient::new(&self.url, &self.agent)
    }
    async fn events_of(&self, kind: &str) -> Vec<Value> {
        self.get("/events/log?after=0&limit=5000")
            .await
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == kind)
            .cloned()
            .collect()
    }
    async fn wait_committed(&self, change_id: &str) -> Value {
        for _ in 0..100 {
            let c = self.get(&format!("/org-changes/{change_id}")).await;
            if ["COMMITTED", "FAILED", "REJECTED"].contains(&c["status"].as_str().unwrap_or("")) {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        panic!("change did not finish");
    }
}

/// Seed a mission and one observation. Returns (coordinator role id, observation id).
async fn seed(org: &Org, summary: &str) -> (String, ObservationId) {
    let m = org
        .post(
            "/missions",
            json!({
                "goal": "Deliver the program on the planned date",
                "roles": [{"key": "lead", "name": "Lead", "charter": "Owns the plan",
                           "authority": {"scopes": ["mission.coordinate"], "budget_cents": 10000}}],
                "responsibilities": [{"purpose": "Coordinate the plan", "owner": "lead"}]
            }),
        )
        .await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let obs = org
        .post(
            &format!("/missions/{mission}/observations"),
            json!({"source": "portal", "summary": summary}),
        )
        .await;
    let state = org.get("/organization").await;
    let lead = state["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Lead")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    (
        lead,
        ObservationId::new(obs["observation_id"].as_str().unwrap()),
    )
}

fn new_role_output(lead: &str, scopes: &[&str]) -> String {
    json!({
        "assessment": "The disruption creates continuing work no current role owns.",
        "verdict": "change",
        "requirement": {"summary": "Restore the plan after the disruption", "acceptance": "A feasible alternative is recorded"},
        "rationale": "A dedicated owner is needed for independent ongoing work.",
        "budget_limit_cents": 50000,
        "operations": [
            {"op": "CreateRole", "key": "r", "name": "Recovery lead", "charter": "Restore the plan",
             "authority": {"scopes": scopes, "budget_cents": 20000},
             "reports_to": {"kind": "existing", "role_id": lead},
             "provider": "elastic", "justification": "independent_ownership"},
            {"op": "AssignResponsibility",
             "target": {"kind": "from_requirement", "key": "work", "requirement_id": "$REQUIREMENT",
                        "purpose": "Restore the plan", "trigger": "new evidence", "termination": "plan restored"},
             "owner": {"kind": "new", "key": "r"}}
        ]
    })
    .to_string()
}

#[tokio::test]
async fn planner_proposal_is_validated_and_committed_by_the_kernel() {
    let org = Org::start().await;
    let (lead, obs) = seed(&org, "The planned location is unavailable on the date").await;
    let model = Arc::new(ReplayClient::new([format!(
        "```json\n{}\n```",
        new_role_output(&lead, &["ledger.reserve_venue"])
    )]));
    let planner = Planner::new(org.kernel(), model.clone());
    let outcome = planner.plan(&obs).await.unwrap();
    let PlanOutcome::Proposed { change, attempts } = outcome else {
        panic!("expected a proposal, got {outcome:?}");
    };
    assert_eq!(attempts, 1);
    let done = org.wait_committed(change.change_id.as_str()).await;
    assert_eq!(done["status"], "COMMITTED", "{done}");

    // The planner filled evidence and revision; the model never sees or sets them.
    assert_eq!(done["proposal"]["trigger_evidence_ids"][0], obs.as_str());
    // The requirement was recorded by the agent and now has an owner.
    let state = org.get("/organization").await;
    let req = state["requirements"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(req["status"], "ASSIGNED");
    // The prompt carried the observation and current state.
    let sent = &model.requests()[0].messages[0].content;
    assert!(sent.contains("The planned location is unavailable"));
    assert!(sent.contains("Grantable scopes:"));
    // Decision is in the evidence log with the replay model labeled.
    let d = org.events_of("planner_decision").await;
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["payload"]["verdict"], "change_proposed");
    assert_eq!(d[0]["payload"]["model"], "replay");
    assert_eq!(d[0]["payload"]["change_id"], change.change_id.as_str());
}

#[tokio::test]
async fn no_change_verdict_leaves_organization_untouched() {
    let org = Org::start().await;
    let (_, obs) = seed(&org, "The catering vendor confirmed the order as planned").await;
    let before = org.get("/organization").await["revision"].clone();
    let model = Arc::new(ReplayClient::new([json!({
        "assessment": "Confirms the existing plan.", "verdict": "no_change",
        "requirement": null, "rationale": "Nothing new to own.", "operations": []
    })
    .to_string()]));
    let outcome = Planner::new(org.kernel(), model).plan(&obs).await.unwrap();
    assert!(matches!(outcome, PlanOutcome::NoChange { attempts: 1 }));
    assert_eq!(org.get("/organization").await["revision"], before);
    assert!(org.events_of("change_proposed").await.is_empty());
    let d = org.events_of("planner_decision").await;
    assert_eq!(d[0]["payload"]["verdict"], "no_change");
}

#[tokio::test]
async fn kernel_rejection_is_fed_back_and_the_repair_commits() {
    let org = Org::start().await;
    let (lead, obs) = seed(&org, "The planned location is unavailable on the date").await;
    let model = Arc::new(ReplayClient::new([
        // Asks for a scope the constitution does not grant.
        new_role_output(&lead, &["treasury.wire_funds"]),
        new_role_output(&lead, &["ledger.reserve_venue"]),
    ]));
    let outcome = Planner::new(org.kernel(), model.clone())
        .plan(&obs)
        .await
        .unwrap();
    let PlanOutcome::Proposed { change, attempts } = outcome else {
        panic!("expected a repaired proposal, got {outcome:?}");
    };
    assert_eq!(attempts, 2);
    assert_eq!(
        org.wait_committed(change.change_id.as_str()).await["status"],
        "COMMITTED"
    );

    // The second model call saw the kernel's exact error code.
    let second = &model.requests()[1];
    let feedback = &second.messages.last().unwrap().content;
    assert!(
        feedback.contains("authority_exceeds_constitution"),
        "{feedback}"
    );
    // Only one requirement was recorded across both attempts.
    assert_eq!(
        org.get("/organization").await["requirements"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let verdicts: Vec<_> = org
        .events_of("planner_decision")
        .await
        .iter()
        .map(|d| d["payload"]["verdict"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(verdicts, ["rejected", "change_proposed"]);
}

#[tokio::test]
async fn unusable_output_is_retried_then_escalated() {
    let org = Org::start().await;
    let (_, obs) = seed(&org, "Something happened").await;
    let model = Arc::new(ReplayClient::new([
        "I would hire a new person.",
        r#"{"assessment":"x","verdict":"change","requirement":null,"rationale":"y","operations":[{"op":"HireEveryone"}]}"#,
        "still not json",
    ]));
    let outcome = Planner::new(org.kernel(), model.clone())
        .plan(&obs)
        .await
        .unwrap();
    assert!(
        matches!(outcome, PlanOutcome::Escalated { attempts: 3, .. }),
        "{outcome:?}"
    );
    assert_eq!(model.requests().len(), 3);
    let d = org.events_of("planner_decision").await;
    assert_eq!(d.last().unwrap()["payload"]["verdict"], "escalated");
    // Nothing was submitted to the kernel.
    assert!(org.events_of("change_proposed").await.is_empty());
}

#[tokio::test]
async fn runner_plans_each_observation_once() {
    let org = Org::start().await;
    let (lead, _) = seed(&org, "The planned location is unavailable on the date").await;
    let model = Arc::new(ReplayClient::new([new_role_output(
        &lead,
        &["ledger.reserve_venue"],
    )]));
    let mut runner = Runner::new(
        Planner::new(org.kernel(), model.clone()),
        Duration::from_millis(10),
    );
    let first = runner.tick().await.unwrap();
    assert_eq!(first.len(), 1);
    let second = runner.tick().await.unwrap();
    assert!(second.is_empty());
    assert_eq!(model.requests().len(), 1);

    // A fresh runner (e.g. after a restart) also skips it.
    let mut restarted = Runner::new(
        Planner::new(org.kernel(), model.clone()),
        Duration::from_millis(10),
    );
    assert!(restarted.tick().await.unwrap().is_empty());
    assert_eq!(model.requests().len(), 1);
}

#[tokio::test]
async fn agent_token_cannot_commit_or_create_missions() {
    let org = Org::start().await;
    let resp = org
        .http
        .post(format!("{}/api/v1/missions", org.url))
        .bearer_auth(&org.agent)
        .json(&json!({"goal": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

/// A run that died after its change was accepted but before its decision
/// was recorded is recovered on restart without another model call.
#[tokio::test]
async fn restart_after_accepted_change_records_decision_without_model_call() {
    let org = Org::start().await;
    let (lead, obs) = seed(&org, "Main hall closed on the event date").await;
    // First run: the model proposes, the change is accepted.
    let first = Planner::new(
        org.kernel(),
        Arc::new(ReplayClient::new([new_role_output(
            &lead,
            &["ledger.reserve_venue"],
        )])),
    );
    let PlanOutcome::Proposed { change, .. } = first.plan(&obs).await.unwrap() else {
        panic!("expected a proposal");
    };
    org.wait_committed(change.change_id.as_str()).await;
    // Simulate the crash window: the change exists under the planner's
    // deterministic request ID, so a fresh planner must find it.
    let found = org
        .kernel()
        .change_by_request(&format!("planner:{obs}:a1:s0"))
        .await
        .unwrap()
        .expect("change is found by its request ID");
    assert_eq!(found.change_id, change.change_id);

    let model = Arc::new(ReplayClient::new(Vec::<String>::new()));
    let second = Planner::new(org.kernel(), model.clone());
    let outcome = second.plan(&obs).await.unwrap();
    assert!(matches!(outcome, PlanOutcome::Proposed { attempts: 0, .. }));
    assert!(model.requests().is_empty(), "no new model call");
    // The terminal decision is recorded once, even though both runs sent one.
    let decisions = org.events_of("planner_decision").await;
    assert_eq!(decisions.len(), 1, "{decisions:?}");
    assert_eq!(
        decisions[0]["payload"]["change_id"],
        change.change_id.as_str()
    );
    let state = org.get("/organization").await;
    assert_eq!(state["requirements"].as_object().unwrap().len(), 1);
}

#[tokio::test]
async fn unknown_request_id_is_not_found() {
    let org = Org::start().await;
    assert!(org
        .kernel()
        .change_by_request("planner:missing:a1:s0")
        .await
        .unwrap()
        .is_none());
}
