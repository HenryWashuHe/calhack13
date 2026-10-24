//! Henry's Claude: independent regressions for guards in the merged #28/#32
//! contracts that the existing suites don't exercise. Each test fails when the
//! named guard is disabled (mutation-tested) and passes on the current code.
use genesis_core::development_experiment::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::TrialPlan;
use serde_json::{json, Value};

fn reference(id: &str) -> ExactContentRef {
    serde_json::from_value(json!({"domain":"test","schema":"test/1","id":id,
        "version":"1","sha256":"a".repeat(64)}))
    .unwrap()
}
fn plan_value() -> Value {
    let mut slots = vec![];
    for role in ["candidate", "control"] {
        slots.push(
            json!({"slot_id":role,"scoring_request_id":format!("score-{role}"),"org_id":"org",
            "run_id":role,"attempt_id":role,"method":reference(role),"dependencies":[],
            "purpose":{"kind":"acceptance","role":role}}),
        );
    }
    let grant = json!({"model_calls":0,"input_tokens":0,"output_tokens":0,"wall_time_ms":100,"simulation_steps":100});
    json!({"id":"plan","version":"1","criterion":reference("criterion"),
        "development_instances":[reference("public")],"scoring_instances":{"kind":"development"},
        "context":{"public_task":reference("task"),"evaluator":reference("evaluator"),"environment":reference("env"),
            "generator":reference("generator"),"instance_set":reference("set"),"design_schema":reference("design-schema"),
            "metric_schema":reference("metrics"),"scoring_rule":reference("rule")},
        "model_settings":reference("model"),"development_environment":reference("env"),
        "development_grant":grant,"scoring_grant":grant,"slots":slots})
}
fn plan() -> TrialPlan {
    serde_json::from_value(plan_value()).unwrap()
}
fn scope(p: &TrialPlan) -> ExperimentScope<'_> {
    ExperimentScope {
        plan: p,
        slot_id: "candidate",
    }
}
fn settled(p: &TrialPlan, steps: u64) -> ExperimentLedger {
    let mut ledger = ExperimentLedger::new(scope(p)).unwrap();
    let call = ledger
        .reserve(scope(p), reference("public"), reference("design"), 50)
        .unwrap();
    ledger
        .settle(
            scope(p),
            &call,
            ExperimentSettlement {
                outcome: ExperimentOutcome::Returned {
                    observed_evaluator: reference("evaluator"),
                    result: Box::new(reference("result")),
                },
                simulation_steps: ResourceUsage::Measured { value: steps },
            },
        )
        .unwrap();
    ledger
}

/// development_experiment.rs `reason`: failure/unknown reasons are evidence
/// text and must be nonblank, trimmed and free of control characters.
#[test]
fn experiment_reasons_must_be_nonblank_trimmed_text() {
    let p = plan();
    for bad in ["", "   ", " padded", "line\nbreak"] {
        for outcome in [
            ExperimentOutcome::Failed { reason: bad.into() },
            ExperimentOutcome::Unknown { reason: bad.into() },
        ] {
            let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
            let call = ledger
                .reserve(scope(&p), reference("public"), reference("design"), 10)
                .unwrap();
            let settlement = ExperimentSettlement {
                outcome,
                simulation_steps: ResourceUsage::Measured { value: 1 },
            };
            assert!(
                ledger.settle(scope(&p), &call, settlement).is_err(),
                "{bad:?} accepted"
            );
        }
        let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
        let call = ledger
            .reserve(scope(&p), reference("public"), reference("design"), 10)
            .unwrap();
        let settlement = ExperimentSettlement {
            outcome: ExperimentOutcome::Failed {
                reason: "crashed".into(),
            },
            simulation_steps: ResourceUsage::Unknown { reason: bad.into() },
        };
        assert!(
            ledger.settle(scope(&p), &call, settlement).is_err(),
            "usage {bad:?} accepted"
        );
    }
}

/// development_experiment.rs `assess`, the per-record step-limit check: a
/// restored (untrusted) ledger whose call claims a zero or oversized allowance
/// is refused, rather than accounted (or underflowing the remaining grant).
#[test]
fn a_restored_ledger_with_a_zero_or_oversized_allowance_is_refused() {
    let p = plan();
    let good = settled(&p, 40);
    assert_eq!(good.assess(scope(&p)).unwrap().remaining_steps, Some(60));
    for (limit, steps) in [(0u64, 0u64), (1000, 99), (1000, 150)] {
        let mut v = serde_json::to_value(&good).unwrap();
        v["records"][0]["call"]["step_limit"] = limit.into();
        v["records"][0]["settlement"]["simulation_steps"]["value"] = steps.into();
        let tampered: ExperimentLedger = serde_json::from_value(v).unwrap();
        let assessed = std::panic::catch_unwind(|| tampered.assess(scope(&p)));
        assert!(
            matches!(assessed, Ok(Err(_))),
            "limit {limit} / steps {steps}: {assessed:?}"
        );
    }
}

/// development_trial.rs `validate`: a plan with no public development
/// instances (or no slots) is not a frozen plan.
#[test]
fn plans_without_public_instances_or_slots_are_invalid() {
    let mut v = plan_value();
    v["development_instances"] = json!([]);
    let p: TrialPlan = serde_json::from_value(v).unwrap();
    assert!(p.validate().is_err());
    assert!(p.expected_binding("candidate").is_err());
    let mut v = plan_value();
    v["slots"] = json!([]);
    let p: TrialPlan = serde_json::from_value(v).unwrap();
    assert!(p.validate().is_err());
}

/// development_trial.rs `nonblank`: slot and scoring-request identifiers are
/// exact identities, never blank or padded.
#[test]
fn blank_or_padded_trial_identifiers_are_invalid() {
    for (field, bad) in [
        ("slot_id", ""),
        ("slot_id", " candidate"),
        ("scoring_request_id", "score\t"),
    ] {
        let mut v = plan_value();
        v["slots"][0][field] = bad.into();
        let p: TrialPlan = serde_json::from_value(v).unwrap();
        assert!(p.validate().is_err(), "{field} = {bad:?} accepted");
    }
}

/// development_trial.rs `evaluation_request`: each slot's request carries that
/// slot's own predeclared scoring request id.
#[test]
fn each_scoring_request_carries_its_own_slots_request_id() {
    let p = plan();
    for slot in ["candidate", "control"] {
        let binding = p.expected_binding(slot).unwrap();
        let measured = json!({"kind":"measured","value":0});
        let execution: ExecutionReceipt = serde_json::from_value(json!({
            "binding": binding, "binding_sha256": binding.canonical_hash().unwrap(),
            "actual_method": binding.method, "actual_environment": binding.environment,
            "outcome": "output_generated", "outputs": [reference("output")],
            "transcript": reference("transcript"),
            "usage": {"model_calls":measured,"input_tokens":measured,"output_tokens":measured,
                "wall_time_ms":measured,"simulation_steps":measured}}))
        .unwrap();
        let request = p.evaluation_request(slot, &execution).unwrap().unwrap();
        assert_eq!(request.request_id, format!("score-{slot}"));
    }
}
