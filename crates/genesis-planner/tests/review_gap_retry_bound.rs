//! PR73: a stubborn planner reaches a durable terminal decision after the
//! kernel's refusal bound, while its first retry remains legal. Scripted model,
//! real HTTP kernel/runner; no native provider effects.
use genesis_core::model::CapabilityGapStatus;
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::{build_org, capability_gaps, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn repeated_refusal_repair_escalates_and_restart_does_not_replay_it() {
    let cfg = OrgConfig::dev("org_a", "A");
    let app = build_org(&cfg, None).unwrap();
    let addr = spawn_org(app.clone(), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{addr}");
    let http = reqwest::Client::new();
    let post = |path: String, body: Value| {
        let req = http
            .post(format!("{url}/api/v1{path}"))
            .bearer_auth(&cfg.tokens.operator)
            .json(&body);
        async move {
            req.send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };
    let mission = post(
        "/missions".into(),
        json!({
            "goal":"Find a venue", "roles":[{"key":"lead", "name":"Lead", "charter":"Own plan",
                "authority":{"scopes":["mission.coordinate"], "budget_cents":0}}],
            "responsibilities":[{"purpose":"Coordinate", "owner":"lead"}]
        }),
    )
    .await;
    let mission_id = mission["mission_id"].as_str().unwrap();
    post(
        format!("/missions/{mission_id}/observations"),
        json!({
            "source":"portal", "summary":"Venue closed; read alternatives",
            "evidence_ref":"https://portal.example/a"
        }),
    )
    .await;
    let lead = app
        .current_state()
        .unwrap()
        .roles
        .keys()
        .next()
        .unwrap()
        .to_string();
    let initial = json!({
        "assessment":"Need an executable portal reader", "verdict":"change",
        "requirement":{"summary":"Read alternatives", "acceptance":"Verified venue data"},
        "rationale":"Lead needs a reader", "budget_limit_cents":0,
        "operations":[
            {"op":"AssignResponsibility", "target":{"kind":"from_requirement", "key":"read",
                "requirement_id":"$REQUIREMENT", "purpose":"Read venue data", "trigger":"closure", "termination":"data ready"},
             "owner":{"kind":"existing", "role_id":lead}},
            {"op":"RequestCapability", "key":"reader", "requirement_id":"$REQUIREMENT",
             "role":{"kind":"existing", "role_id":lead}, "responsibility":{"kind":"new", "key":"read"},
             "capability_id":"venue-reader", "purpose":"Read alternatives", "observed_sources":["https://portal.example/a"]}
        ]
    }).to_string();
    let first_model = Arc::new(ReplayClient::new([initial]));
    let make_runner = |model: Arc<ReplayClient>| {
        let mut planner = Planner::new(KernelClient::new(&url, &cfg.tokens.agent), model);
        planner.max_attempts = 2;
        Runner::new(planner, Duration::from_millis(5))
    };
    let initial_out = make_runner(first_model.clone()).tick().await.unwrap();
    assert!(matches!(
        initial_out[0].1,
        PlanOutcome::Proposed { attempts: 1, .. }
    ));
    wait(|| app.current_state().unwrap().capability_gaps.len() == 1).await;
    let gap = app
        .current_state()
        .unwrap()
        .capability_gaps
        .values()
        .next()
        .unwrap()
        .clone();
    let retry = json!({
        "assessment":"Ask for the same reader again", "verdict":"change", "requirement":null,
        "rationale":"Retry refused reader", "budget_limit_cents":0,
        "operations":[{"op":"RequestCapability", "key":"retry", "requirement_id":gap.requirement_id,
            "role":{"kind":"existing", "role_id":lead},
            "responsibility":{"kind":"existing", "responsibility_id":gap.responsibility_id},
            "capability_id":"venue-reader", "purpose":"Read alternatives", "observed_sources":["https://portal.example/a"]}]
    }).to_string();
    capability_gaps::transition(
        &app,
        &gap.gap_id,
        CapabilityGapStatus::Refused {
            reason: "No acquisition policy".into(),
            acquisition_id: None,
        },
    )
    .await
    .unwrap();
    let model = Arc::new(ReplayClient::new([retry.clone(), retry.clone(), retry]));
    // Fresh runner replays the durable log and handles only the new refusal.
    let retried = make_runner(model.clone()).tick().await.unwrap();
    assert_eq!(retried.len(), 1);
    assert!(
        matches!(retried[0].1, PlanOutcome::Proposed { attempts: 1, .. }),
        "{retried:?}"
    );
    wait(|| app.current_state().unwrap().capability_gaps.len() == 2).await;
    let live = app
        .current_state()
        .unwrap()
        .capability_gaps
        .values()
        .find(|g| !g.status.is_terminal())
        .unwrap()
        .gap_id
        .clone();
    capability_gaps::transition(
        &app,
        &live,
        CapabilityGapStatus::Refused {
            reason: "Still no acquisition policy".into(),
            acquisition_id: None,
        },
    )
    .await
    .unwrap();
    let before = app.current_state().unwrap();
    let stopped = make_runner(model.clone()).tick().await.unwrap();
    assert_eq!(stopped.len(), 1);
    assert!(
        matches!(stopped[0].1, PlanOutcome::Escalated { attempts: 2, .. }),
        "{stopped:?}"
    );
    assert_eq!(
        model.requests().len(),
        3,
        "one legal retry plus two bounded repair calls"
    );
    let after = app.current_state().unwrap();
    assert_eq!(after.capability_gaps, before.capability_gaps);
    assert_eq!(after.requirements, before.requirements);
    assert_eq!(
        after.revision, before.revision,
        "rejected proposals append decisions only"
    );
    let requests = model.requests();
    assert!(
        requests[2]
            .messages
            .iter()
            .any(|m| m.content.contains("was already refused 2 times")),
        "repair receives the actual bound reason"
    );
    assert!(make_runner(model.clone()).tick().await.unwrap().is_empty());
    assert_eq!(
        model.requests().len(),
        3,
        "restart does not restart the stopped loop"
    );
}

async fn wait(mut ready: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("kernel change did not finish");
}
