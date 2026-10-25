//! Pure JSON guards. The imported rehearsal remains ignored and is never run.
#[path = "rehearse_live_planner.rs"]
mod driver;
use serde_json::{json, Value};

fn assigned() -> Value {
    json!({"requirements":{"req":{"requirement_id":"req","mission_id":"mission",
        "triggering_evidence":["obs"],"status":"ASSIGNED","owner":{"kind":"role","id":"lead"}}},
        "responsibilities":{"rsp":{"responsibility_id":"rsp","requirement_id":"req",
            "mission_id":"mission","owner":{"kind":"role","id":"lead"},"status":"OPEN"}}})
}

#[test]
fn assignment_waits_for_commit_after_requirement_recorded() {
    let mut state = assigned();
    state["requirements"]["req"]["status"] = json!("UNMET");
    state["requirements"]["req"]["owner"] = json!({"kind":"mission_controller"});
    state["responsibilities"] = json!({});
    assert_eq!(driver::assigned_work(&state, "mission", "obs"), None);
    assert_eq!(
        driver::assigned_work(&json!({"requirements":{}}), "mission", "obs"),
        None
    );
    assert_eq!(
        driver::assigned_work(&assigned(), "mission", "obs"),
        Some(("req".into(), "lead".into(), "rsp".into()))
    );
}

#[test]
fn assignment_refuses_foreign_observation_mission_or_responsibility() {
    for key in ["observation", "mission", "owner", "responsibility"] {
        let mut state = assigned();
        match key {
            "observation" => state["requirements"]["req"]["triggering_evidence"] = json!(["other"]),
            "mission" => state["requirements"]["req"]["mission_id"] = json!("other"),
            "owner" => state["responsibilities"]["rsp"]["owner"]["id"] = json!("other"),
            _ => state["responsibilities"]["rsp"]["requirement_id"] = json!("other"),
        }
        assert_eq!(
            driver::assigned_work(&state, "mission", "obs"),
            None,
            "{key}"
        );
    }
}

#[test]
fn assignment_refuses_ambiguous_matches_and_closed_responsibility() {
    let mut state = assigned();
    state["requirements"]["other"] = state["requirements"]["req"].clone();
    assert_eq!(driver::assigned_work(&state, "mission", "obs"), None);
    let mut state = assigned();
    state["responsibilities"]["other"] = state["responsibilities"]["rsp"].clone();
    assert_eq!(driver::assigned_work(&state, "mission", "obs"), None);
    let mut state = assigned();
    state["responsibilities"]["rsp"]["status"] = json!("CLOSED");
    assert_eq!(driver::assigned_work(&state, "mission", "obs"), None);
}

#[test]
fn assignment_matches_unchanged_historical_existing_owner_record() {
    let state: Value = serde_json::from_str(include_str!(
        "../../../docs/invention-lab/evidence/rehearsal-live-planner-2-20261025/orgs/b-organization-after-planner.json"
    )).unwrap();
    assert_eq!(
        driver::assigned_work(
            &state,
            "mis_6f0171db2f964fcda0fe65187b67c6b2",
            "obs_a5ef72d741674b0aa6a9997673b57d30"
        ),
        Some((
            "req_2d1dbd8187ac48c78b5479c284726e25".into(),
            "role_9649bd2660b94b2fa9ec2ea25b1ffde0".into(),
            "rsp_10c34e929dc848abba941635f3441b86".into()
        ))
    );
}
