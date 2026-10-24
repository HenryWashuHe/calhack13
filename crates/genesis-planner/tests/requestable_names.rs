//! Henry's Claude: the planner is told which capability names the server's
//! trusted gap policy accepts, so a live model requests one the policy can
//! act on instead of inventing a name the policy would refuse.
use genesis_planner::agent::Planner;
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

async fn prompt_for(policy: Option<Value>) -> String {
    let cfg = OrgConfig::dev("org_names", "Names");
    let mut app = build_org(&cfg, None).unwrap();
    if let Some(policy) = policy {
        app.gap_acquisition_policy = Some(Arc::new(serde_json::from_value(policy).unwrap()));
    }
    let addr = spawn_org(app.clone(), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{addr}");
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| {
        let req = http
            .post(format!("{url}/api/v1{path}"))
            .bearer_auth(&cfg.tokens.operator)
            .json(&body);
        async move { req.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let m = post(
        "/missions",
        json!({"goal": "Book a venue", "roles": [{"key": "lead", "name": "Lead", "charter": "Owns venues",
               "authority": {"scopes": ["mission.coordinate"], "budget_cents": 1000}}],
               "responsibilities": [{"purpose": "Keep a venue", "owner": "lead"}]}),
    )
    .await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    post(
        &format!("/missions/{mission}/observations"),
        json!({"source": "portal", "summary": "The venue portal changed layout",
               "evidence_ref": "https://portal.example/venues/index.html"}),
    )
    .await;
    let no_change = json!({"assessment": "x", "verdict": "no_change", "requirement": null,
        "rationale": "x", "budget_limit_cents": null, "operations": []})
    .to_string();
    let model = Arc::new(ReplayClient::new([no_change]));
    let planner = Planner::new(KernelClient::new(&url, &cfg.tokens.agent), model.clone());
    let mut runner = Runner::new(planner, Duration::from_millis(10));
    runner.tick().await.unwrap();
    model.requests()[0].messages[0].content.clone()
}

#[tokio::test]
async fn the_prompt_lists_the_policy_names_and_nothing_else_about_the_policy() {
    let rule = |id: &str| {
        json!({"capability_id": id, "suite_id": "PRIVATE_SUITE", "actor_name_prefix": "gap-reader",
               "allowed_source_urls": ["https://portal.example/PRIVATE.html"],
               "examples": [{"url": "https://portal.example/PRIVATE.html", "html": "<html>PRIVATE</html>"}],
               "repair": false})
    };
    let prompt = prompt_for(Some(
        json!({"rules": [rule("venue-reader"), rule("capacity-checker")]}),
    ))
    .await;
    assert!(
        prompt.contains("## Requestable capability names (operator policy)"),
        "{prompt}"
    );
    assert!(
        prompt.contains("capacity-checker, venue-reader"),
        "{prompt}"
    );
    assert!(!prompt.contains("PRIVATE"), "{prompt}");
}

#[tokio::test]
async fn without_a_policy_the_prompt_is_unchanged() {
    let prompt = prompt_for(None).await;
    assert!(!prompt.contains("Requestable capability names"), "{prompt}");
}
