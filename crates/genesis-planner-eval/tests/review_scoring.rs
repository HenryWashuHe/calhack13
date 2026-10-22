//! Review-only negative cases using the real kernel and scripted model output.
use genesis_planner::llm::LlmClient;
use genesis_planner_eval::{
    load_scenarios, run_scenario, EvalOptions, Ids, Replay, RunResult, Scenario, Verdict,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

fn scenarios_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals/planner")
}

fn scenario(id: &str) -> Scenario {
    load_scenarios(&scenarios_dir())
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
}

async fn replay(s: &Scenario, responses: Vec<String>) -> RunResult {
    let factory = move |_: &Scenario, _: u32, ids: &Ids| {
        Ok(Arc::new(Replay::new(responses.clone(), ids)) as Arc<dyn LlmClient>)
    };
    run_scenario(s, 0, &factory, &EvalOptions::default()).await
}

#[tokio::test]
async fn unavailable_model_is_not_a_correct_budget_escalation() {
    let result = replay(&scenario("deposit-over-budget"), vec![]).await;
    assert_eq!(result.verdict, Some(Verdict::Escalate));
    assert!(result
        .escalation
        .as_ref()
        .unwrap()
        .contains("model call failed"));
    assert!(
        !result.passed,
        "model never supplied a decision, but scored pass: {result:?}"
    );
}

#[tokio::test]
async fn three_unparseable_replies_are_not_a_correct_budget_escalation() {
    let result = replay(&scenario("deposit-over-budget"), vec!["not JSON".into(); 3]).await;
    assert_eq!(result.verdict, Some(Verdict::Escalate));
    assert_eq!(result.attempts, 3);
    assert!(
        !result.passed,
        "no usable decision was produced, but scored pass: {result:?}"
    );
}

#[tokio::test]
async fn capacity_change_must_stay_with_the_existing_venue_owner() {
    let s = scenario("capacity-cut-existing-owner");
    let text =
        std::fs::read_to_string(scenarios_dir().join("replay/capacity-cut-existing-owner.json"))
            .unwrap();
    let golden: Vec<String> = serde_json::from_str(&text).unwrap();
    let mut answer: Value = serde_json::from_str(&golden[0]).unwrap();
    answer["operations"][0]["owner"]["role_id"] = Value::String("{{role:Program lead}}".into());
    answer["rationale"] = Value::String(
        "Give the program scheduler ownership of venue capacity despite the existing venue owner."
            .into(),
    );
    let result = replay(&s, vec![answer.to_string()]).await;
    assert_eq!(result.change_status.as_deref(), Some("COMMITTED"));
    assert!(
        !result.passed,
        "wrong existing owner received the work, but scored pass: {result:?}"
    );
}
