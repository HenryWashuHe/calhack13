//! The planner with an Elastic Agent Builder mission-planner agent as its
//! model (fake Kibana), against a real in-process kernel.

use genesis_core::ids::ObservationId;
use genesis_elastic::fake::start_kibana;
use genesis_elastic::{AgentBuilder, AgentBuilderConfig};
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::elastic::ElasticAgentLlm;
use genesis_planner::kernel::KernelClient;
use genesis_planner::prompt::SYSTEM_PROMPT;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn elastic_mission_agent_proposes_and_the_kernel_commits() {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let addr = spawn_org(build_org(&cfg, None).unwrap(), "127.0.0.1:0")
        .await
        .unwrap();
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| {
        let http = http.clone();
        let url = format!("{base}/api/v1{path}");
        let token = cfg.tokens.operator.clone();
        async move {
            http.post(url)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };
    let m = post("/missions", json!({"goal": "Deliver the program", "roles": [{"key": "lead", "name": "Lead",
        "charter": "Owns the plan", "authority": {"scopes": ["mission.coordinate"], "budget_cents": 10000}}],
        "responsibilities": [{"purpose": "Coordinate the plan", "owner": "lead"}]})).await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let obs = post(
        &format!("/missions/{mission}/observations"),
        json!({"source": "portal", "summary": "The planned location closed"}),
    )
    .await;
    let observation = ObservationId::new(obs["observation_id"].as_str().unwrap());
    let state: Value = http
        .get(format!("{base}/api/v1/organization"))
        .bearer_auth(&cfg.tokens.operator)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let lead = state["roles"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();

    let (kibana_url, kibana) = start_kibana().await;
    kibana.lock().unwrap().converse_reply = Some(json!({
        "assessment": "The closure creates continuing work no current role owns.",
        "verdict": "change",
        "requirement": {"summary": "Find an alternative location", "acceptance": "An alternative is recorded"},
        "rationale": "A dedicated owner is needed.",
        "budget_limit_cents": 0,
        "operations": [
            {"op": "CreateRole", "key": "r", "name": "Location lead", "charter": "Find an alternative",
             "authority": {"scopes": ["mission.coordinate"], "budget_cents": 0},
             "reports_to": {"kind": "existing", "role_id": lead},
             "provider": "elastic", "justification": "independent_ownership"},
            {"op": "AssignResponsibility",
             "target": {"kind": "from_requirement", "key": "work", "requirement_id": "$REQUIREMENT",
                        "purpose": "Find an alternative", "trigger": "new evidence", "termination": "found"},
             "owner": {"kind": "new", "key": "r"}}]
    }).to_string());
    let builder = AgentBuilder::new(AgentBuilderConfig {
        kibana_url,
        api_key: "bWdtdA==".into(),
        space: None,
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    let model = Arc::new(ElasticAgentLlm::new(builder.clone(), "org_a"));
    let planner = Planner::new(KernelClient::new(&base, &cfg.tokens.agent), model);
    let outcome = planner.plan(&observation).await.unwrap();
    let PlanOutcome::Proposed { change, attempts } = outcome else {
        panic!("expected a proposal, got {outcome:?}");
    };
    assert_eq!(attempts, 1);

    // The planner agent carries the system prompt and only the org's evidence tool.
    {
        let k = kibana.lock().unwrap();
        assert_eq!(k.agents.len(), 1);
        let agent = k.agents.values().next().unwrap();
        assert!(agent["configuration"]["instructions"]
            .as_str()
            .unwrap()
            .starts_with(SYSTEM_PROMPT));
        assert_eq!(
            agent["configuration"]["tools"][0]["tool_ids"],
            json!([builder.evidence_tool_id("org_a")])
        );
        assert!(k.converse_inputs[0].contains("The planned location closed"));
    }

    // The kernel validated and committed it; the decision names the Elastic model.
    for _ in 0..100 {
        let c: Value = http
            .get(format!("{base}/api/v1/org-changes/{}", change.change_id))
            .bearer_auth(&cfg.tokens.operator)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if c["status"] == "COMMITTED" {
            let events: Value = http
                .get(format!("{base}/api/v1/events/log?after=0&limit=1000"))
                .bearer_auth(&cfg.tokens.operator)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let decision = events
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["type"] == "planner_decision")
                .unwrap();
            assert_eq!(decision["payload"]["model"], "elastic-agent-builder/org_a");
            return;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("change did not commit");
}
