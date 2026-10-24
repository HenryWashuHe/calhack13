//! Henry's Claude: evidence for a mission that can no longer act gets a
//! recorded no-change decision and no model call. Real in-process kernel,
//! scripted model.

use chrono::Utc;
use genesis_core::engine::Outcome;
use genesis_core::ids::MissionId;
use genesis_core::lifecycle::MissionStatus;
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::app::AppState;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const CHANGE: &str = r#"{"assessment":"x","verdict":"change","requirement":{"summary":"Do the new work","acceptance":"done"},"rationale":"y","operations":[]}"#;

enum Close {
    Status(MissionStatus),
    DeadlinePassed,
    StillActive,
}

async fn run(
    close: Close,
) -> (
    Vec<(genesis_core::ids::ObservationId, PlanOutcome)>,
    usize,
    AppState,
    Runner,
) {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let app = build_org(&cfg, None).unwrap();
    let addr = spawn_org(app.clone(), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{addr}");
    let http = reqwest::Client::new();
    let post = |path: String, body: Value| {
        let req = http
            .post(format!("{url}/api/v1{path}"))
            .bearer_auth(&cfg.tokens.operator)
            .json(&body);
        async move { req.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let m = post(
        "/missions".into(),
        json!({"goal": "Deliver the program", "roles": [{"key": "lead", "name": "Lead", "charter": "Owns the plan",
               "authority": {"scopes": ["mission.coordinate"], "budget_cents": 10000}}],
               "responsibilities": [{"purpose": "Coordinate the plan", "owner": "lead"}]}),
    )
    .await;
    let mission = MissionId::new(m["mission_id"].as_str().unwrap());
    let id = mission.clone();
    app.mutate(EventMeta::default(), move |s| {
        let mut st = s.clone();
        let m = st.missions.get_mut(&id).unwrap();
        match close {
            Close::Status(status) => m.status = status,
            Close::DeadlinePassed => m.deadline = Some(Utc::now() - chrono::Duration::seconds(5)),
            Close::StillActive => {}
        }
        st.revision += 1;
        Ok((
            Outcome {
                state: st,
                effects: vec![],
            },
            (),
        ))
    })
    .await
    .unwrap();
    post(
        format!("/missions/{mission}/observations"),
        json!({"source": "portal", "summary": "A late disruption arrived"}),
    )
    .await;
    let model = Arc::new(ReplayClient::new([CHANGE]));
    let planner = Planner::new(KernelClient::new(&url, &cfg.tokens.agent), model.clone());
    let mut runner = Runner::new(planner, Duration::from_millis(10));
    let outcomes = runner.tick().await.unwrap();
    (outcomes, model.requests().len(), app, runner)
}

fn decisions(app: &AppState) -> Vec<String> {
    app.store
        .events_after(0, 10_000)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.kind {
            genesis_core::events::EventKind::PlannerDecision { verdict, .. } => {
                Some(format!("{verdict:?}"))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn closed_or_expired_missions_get_no_model_call_and_no_new_work() {
    for close in [
        Close::Status(MissionStatus::Failed),
        Close::Status(MissionStatus::Succeeded),
        Close::DeadlinePassed,
    ] {
        let (outcomes, calls, app, mut runner) = run(close).await;
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(outcomes[0].1, PlanOutcome::NoChange { attempts: 0 }),
            "{outcomes:?}"
        );
        assert_eq!(calls, 0, "no model call for a closed mission");
        assert_eq!(
            decisions(&app),
            ["NoChange"],
            "a terminal decision is recorded"
        );
        let st = app.current_state().unwrap();
        assert!(st.requirements.is_empty(), "no new requirement");
        assert!(runner.tick().await.unwrap().is_empty(), "decided once");
    }
}

#[tokio::test]
async fn an_active_mission_still_plans() {
    let (outcomes, calls, _app, _runner) = run(Close::StillActive).await;
    assert!(calls >= 1, "the active control calls the model");
    assert!(
        !matches!(outcomes[0].1, PlanOutcome::NoChange { attempts: 0 }),
        "{outcomes:?}"
    );
}
