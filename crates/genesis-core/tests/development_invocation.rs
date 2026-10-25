//! A one-slot invocation plan: one adopted method run once under a named
//! grant, on public Development instances. Pure contract checks; no execution.
use genesis_core::development_method::*;
use genesis_core::development_trial::*;
use serde_json::{json, Value};

fn reference(id: &str) -> Value {
    json!({"domain":"test","schema":"test/1","id":id,"version":"v1","sha256":"a".repeat(64)})
}
fn grant() -> Value {
    json!({"adoption_id":"mad_1","requirement_id":"req_1","role_id":"role_b",
        "responsibility_id":"rsp_1","assignment_id":"asg_1","epoch":3})
}
fn slot(id: &str, purpose: Value) -> Value {
    json!({"slot_id":id,"scoring_request_id":id,"org_id":"org_example_b","run_id":id,
        "attempt_id":id,"method":reference("adopted"),"dependencies":[],"purpose":purpose})
}
fn plan(slots: Vec<Value>, scoring: Value) -> Result<TrialPlan, String> {
    let limits = json!({"model_calls":0,"input_tokens":0,"output_tokens":0,
        "wall_time_ms":100,"simulation_steps":100});
    let plan: TrialPlan = serde_json::from_value(json!({"id":"invoke","version":"v1",
        "criterion":reference("criterion"),"development_instances":[reference("dev-1")],
        "scoring_instances":scoring,
        "context":{"public_task":reference("task"),"evaluator":reference("evaluator"),
            "environment":reference("scoring-env"),"generator":reference("generator"),
            "instance_set":reference("set"),"design_schema":reference("design"),
            "metric_schema":reference("metric"),"scoring_rule":reference("rule")},
        "model_settings":reference("model"),"development_environment":reference("dev-env"),
        "development_grant":limits,"scoring_grant":limits,"slots":slots}))
    .map_err(|e| e.to_string())?;
    plan.validate().map_err(|e| e.to_string())?;
    Ok(plan)
}
fn invocation() -> Value {
    json!({"kind":"invocation","grant":grant()})
}

#[test]
fn the_binding_names_the_plan_and_the_exact_grant() {
    let p = plan(
        vec![slot("use", invocation())],
        json!({"kind":"development"}),
    )
    .unwrap();
    let binding = p.expected_binding("use").unwrap();
    match &binding.purpose {
        ExecutionPurpose::Invocation { plan, grant: g } => {
            assert_eq!(plan, &p.reference().unwrap());
            assert_eq!(serde_json::to_value(g).unwrap(), grant());
        }
        other => panic!("{other:?}"),
    }
    // A different grant (here a later epoch) is a different binding, so a
    // receipt from one grant can't stand in for another.
    let mut later = grant();
    later["epoch"] = json!(4);
    let other = plan(
        vec![slot("use", json!({"kind":"invocation","grant":later}))],
        json!({"kind":"development"}),
    )
    .unwrap();
    assert_ne!(
        other
            .expected_binding("use")
            .unwrap()
            .canonical_hash()
            .unwrap(),
        binding.canonical_hash().unwrap()
    );
}

#[test]
fn an_invocation_plan_has_exactly_one_slot_on_its_public_panel() {
    let accept = |role| json!({"kind":"acceptance","role":role});
    let refused = [
        // Two invocations, or one beside an acceptance pair or comparison arm.
        vec![slot("a", invocation()), slot("b", invocation())],
        vec![
            slot("use", invocation()),
            slot("candidate", accept("candidate")),
            slot("control", accept("control")),
        ],
        vec![
            slot("use", invocation()),
            slot(
                "arm",
                json!({"kind":"comparison","trial_id":"t","arm_id":"x"}),
            ),
        ],
    ];
    for slots in refused {
        assert!(plan(slots, json!({"kind":"development"})).is_err());
    }
    // Never scored on a hidden commitment.
    let hidden = json!({"kind":"hidden_commitment","protocol":reference("protocol")});
    assert!(plan(vec![slot("use", invocation())], hidden).is_err());
}

#[test]
fn a_blank_grant_identity_is_refused() {
    for field in [
        "adoption_id",
        "requirement_id",
        "role_id",
        "responsibility_id",
        "assignment_id",
    ] {
        let mut g = grant();
        g[field] = json!(" ");
        let slots = vec![slot("use", json!({"kind":"invocation","grant":g}))];
        assert!(
            plan(slots, json!({"kind":"development"})).is_err(),
            "blank {field}"
        );
    }
    let mut extra = grant();
    extra["note"] = json!("x");
    let slots = vec![slot("use", json!({"kind":"invocation","grant":extra}))];
    assert!(plan(slots, json!({"kind":"development"})).is_err());
}
