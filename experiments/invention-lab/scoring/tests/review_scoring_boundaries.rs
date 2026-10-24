//! Independent adapter checks: unsupported contexts and scoring admission.
//! In-process fixtures only; no model, cloud or sandbox execution.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage,
};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::{score_slot, ScoringError};
use serde_json::json;

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");

fn plan() -> TrialPlan {
    example::acceptance_plan(
        &example::ExampleMethods {
            candidate: refs::content("lab-method/1", "candidate", "1", b"candidate"),
            control: refs::content("lab-method/1", "control", "1", b"control"),
        },
        Mode::None,
        "Python review fixture",
        1,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 10_000,
            simulation_steps: 2400,
        },
    )
}

fn executed(plan: &TrialPlan, name: &str) -> (ExecutionReceipt, Vec<u8>, Store) {
    let dir = std::env::temp_dir().join(format!(
        "genesis-score-review-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut session = Session {
        plan,
        slot_id: "candidate",
        store: Store::open(&dir.join("execution")).unwrap(),
        ledger: ExperimentLedger::new(ExperimentScope {
            plan,
            slot_id: "candidate",
        })
        .unwrap(),
        state: RunState {
            started: true,
            method: Some(plan.expected_binding("candidate").unwrap().method),
            ..Default::default()
        },
    };
    let design: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    let submitted = session
        .handle(&json!({"op":"submit", "design": design}).to_string())
        .unwrap();
    assert_eq!(submitted["ok"], true);
    let receipt = finish(
        &mut session,
        refs::environment(Mode::None, "Python review fixture"),
        false,
        ExecutionOutcome::OutputGenerated,
        ResourceUsage::Measured { value: 1 },
    )
    .unwrap()
    .receipt;
    session
        .ledger
        .validate_execution(session.scope(), &receipt)
        .unwrap();
    let bytes = session
        .store
        .read(&format!("artifacts/{}", receipt.outputs[0].sha256))
        .unwrap();
    (receipt, bytes, Store::open(&dir.join("scoring")).unwrap())
}

#[test]
fn unsupported_scoring_context_is_not_certified_as_observed() {
    let mut certified = Vec::new();
    for field in [
        "environment",
        "generator",
        "instance_set",
        "design_schema",
        "metric_schema",
        "scoring_rule",
    ] {
        let mut p = plan();
        let unsupported = refs::content(
            "unimplemented/9",
            field,
            "9",
            b"not implemented by this scorer",
        );
        match field {
            "environment" => p.context.environment = unsupported,
            "generator" => p.context.generator = unsupported,
            "instance_set" => p.context.instance_set = unsupported,
            "design_schema" => p.context.design_schema = unsupported,
            "metric_schema" => p.context.metric_schema = unsupported,
            "scoring_rule" => p.context.scoring_rule = unsupported,
            _ => unreachable!(),
        }
        // The independently frozen plan itself names this unsupported context;
        // no execution hash or output bytes are tampered with after freezing.
        let (execution, bytes, store) = executed(&p, field);
        if let Ok(receipt) = score_slot(&p, "candidate", &execution, &bytes, &store) {
            if receipt.observed_context == Some(p.context.clone()) {
                certified.push(field);
            }
        }
        std::fs::remove_dir_all(store.path("").parent().unwrap()).unwrap();
    }
    assert!(
        certified.is_empty(),
        "adapter certified unsupported context fields as observed: {certified:?}"
    );
}

#[test]
fn zero_scoring_wall_grant_refuses_before_simulation_or_artifacts() {
    let mut p = plan();
    p.scoring_grant.wall_time_ms = 0;
    let (execution, bytes, store) = executed(&p, "zero-wall");
    let result = score_slot(&p, "candidate", &execution, &bytes, &store);
    let artifact_count = std::fs::read_dir(store.path("artifacts")).unwrap().count();
    std::fs::remove_dir_all(store.path("").parent().unwrap()).unwrap();
    assert!(
        matches!(result, Err(ScoringError::Refused(_))),
        "zero wall-time must refuse before work, got {result:?}; wrote {artifact_count} artifacts"
    );
    assert_eq!(
        artifact_count, 0,
        "an inadmissible scoring request wrote simulation evidence"
    );
}
