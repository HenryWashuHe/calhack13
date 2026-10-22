//! Review regression: terminal gap observations must not revive closed missions.
//! Real kernel + HTTP planner client; scripted model only, no sponsor calls.
use chrono::Utc;
use genesis_core::engine::Outcome;
use genesis_core::ids::{CapabilityGapId, MissionId};
use genesis_core::lifecycle::MissionStatus;
use genesis_core::model::CapabilityGapStatus as S;
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, capability_gaps, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const SOURCE: &str = "https://portal.example/listings/index.html";

#[tokio::test]
async fn refused_gap_after_deadline_must_not_create_fresh_work_on_restart() {
    exercise(Some(MissionStatus::Active)).await;
}

#[tokio::test]
async fn refused_gap_after_mission_failure_must_not_create_fresh_work_on_restart() {
    exercise(Some(MissionStatus::Failed)).await;
}

#[tokio::test]
async fn active_mission_refusal_plans_once_across_runner_restart() {
    exercise(None).await;
}

async fn exercise(closed: Option<MissionStatus>) {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let app = build_org(&cfg, None).unwrap();
    let addr = spawn_org(app.clone(), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{addr}");
    let http = reqwest::Client::new();
    let op = |path: &str| format!("{url}/api/v1{path}");
    let post = |path: &str, body: Value| {
        let req = http
            .post(op(path))
            .bearer_auth(&cfg.tokens.operator)
            .json(&body);
        async move { req.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let m = post(
        "/missions",
        json!({"goal": "Deliver the program", "roles": [{"key": "lead", "name": "Lead", "charter": "Owns the plan",
               "authority": {"scopes": ["mission.coordinate"], "budget_cents": 10000}}],
               "responsibilities": [{"purpose": "Coordinate the plan", "owner": "lead"}]}),
    )
    .await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    post(
        &format!("/missions/{mission}/observations"),
        json!({"source": "portal", "summary": "The listing site changed to a layout nothing here can read",
               "evidence_ref": SOURCE}),
    )
    .await;
    let lead = app
        .current_state()
        .unwrap()
        .roles
        .values()
        .find(|r| r.name == "Lead")
        .unwrap()
        .role_id
        .to_string();

    let request = json!({
        "assessment": "No installed capability reads this source.", "verdict": "change",
        "requirement": {"summary": "Read options from the listing site", "acceptance": "options extracted"},
        "rationale": "The lead owns planning but lacks a reader.", "budget_limit_cents": 0,
        "operations": [
            {"op": "AssignResponsibility", "target": {"kind": "from_requirement", "key": "read", "requirement_id": "$REQUIREMENT",
              "purpose": "Keep listing options current", "trigger": "listing changes", "termination": "plan fixed"},
             "owner": {"kind": "existing", "role_id": lead}},
            {"op": "RequestCapability", "key": "reader", "requirement_id": "$REQUIREMENT",
             "role": {"kind": "existing", "role_id": lead}, "responsibility": {"kind": "new", "key": "read"},
             "capability_id": "listing-reader", "purpose": "Extract options from the listing site",
             "observed_sources": [SOURCE]}
        ]
    })
    .to_string();

    let fallback = json!({
        "assessment": "The reader was refused; assign a manual fallback.", "verdict": "change",
        "requirement": {"summary": "Find listing options manually", "acceptance": "manual options available"},
        "rationale": "Keep making progress after reader refusal.", "budget_limit_cents": 0,
        "operations": [
            {"op": "AssignResponsibility", "target": {"kind": "from_requirement", "key": "fallback", "requirement_id": "$REQUIREMENT",
              "purpose": "Do new fallback listing work", "trigger": "reader refused", "termination": "options found"},
             "owner": {"kind": "existing", "role_id": lead}}
        ]
    }).to_string();
    let model = Arc::new(ReplayClient::new([request, fallback]));
    let make_runner = || {
        Runner::new(
            Planner::new(KernelClient::new(&url, &cfg.tokens.agent), model.clone()),
            Duration::from_millis(10),
        )
    };
    let first = make_runner().tick().await.unwrap();
    assert!(
        matches!(first[0].1, PlanOutcome::Proposed { .. }),
        "{first:?}"
    );
    wait(|| !app.current_state().unwrap().capability_gaps.is_empty()).await;
    let gap: CapabilityGapId = app
        .current_state()
        .unwrap()
        .capability_gaps
        .keys()
        .next()
        .unwrap()
        .clone();
    let before = app.current_state().unwrap();
    let initial_work = before.responsibilities.len();
    let initial_requirements = before.requirements.len();
    if let Some(status) = closed {
        let mission_id = MissionId::new(&mission);
        app.mutate(EventMeta::default(), move |s| {
            let mut st = s.clone();
            let m = st.missions.get_mut(&mission_id).unwrap();
            m.status = status;
            if status == MissionStatus::Active {
                m.deadline = Some(Utc::now() - chrono::Duration::seconds(1));
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
    }
    capability_gaps::transition(
        &app,
        &gap,
        S::Refused {
            reason: "Acquisition authority expired or source unavailable".into(),
            acquisition_id: None,
        },
    )
    .await
    .unwrap();
    // A fresh process rebuilds its cursor from durable events before planning.
    let outcomes = make_runner().tick().await.unwrap();
    if !outcomes.is_empty() && matches!(outcomes[0].1, PlanOutcome::Proposed { .. }) {
        wait(|| app.current_state().unwrap().responsibilities.len() > initial_work).await;
    }
    let after = app.current_state().unwrap();
    let actual = (
        model.requests().len(),
        after.requirements.len(),
        after.responsibilities.len(),
    );
    let expected = if closed.is_some() {
        (1, initial_requirements, initial_work)
    } else {
        (2, initial_requirements + 1, initial_work + 1)
    };
    // A subsequent restart must not repeat an already decided observation.
    assert!(make_runner().tick().await.unwrap().is_empty());
    assert_eq!(actual, expected,
        "terminal gap wakeup revived closed work: model calls, requirements, responsibilities; outcomes={outcomes:?}");
}

async fn wait(mut ok: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("condition not reached");
}
