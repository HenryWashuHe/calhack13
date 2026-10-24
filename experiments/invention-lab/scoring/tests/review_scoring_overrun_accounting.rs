//! Independent final-instance overrun accounting check. Local in-process evidence only.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage,
};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::score_slot;
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
fn a_final_instance_overrun_retains_accounting_and_is_not_validated() {
    let mut p = plan();
    assert_eq!(p.development_instances.len(), 1);
    p.scoring_grant.wall_time_ms = 1;
    let (execution, bytes, store) = executed(&p, "final-wall");
    let result = score_slot(&p, "candidate", &execution, &bytes, &store);
    let kept: genesis_core::development_evaluation::EvaluationReceipt =
        serde_json::from_slice(&store.read("evaluation.json").unwrap()).unwrap();
    let ResourceUsage::Measured { value: elapsed } = kept.usage.wall_time_ms else {
        panic!("wall accounting must be measured")
    };
    assert!(
        elapsed > 1,
        "fixture must exhaust its grant, observed {elapsed}ms"
    );
    // #22 separates an observed scoring result from budget compliance. An
    // episode can finish Scored while its measured overrun prevents validation.
    assert!(
        matches!(
            &result,
            Err(genesis_lab_score::ScoringError::Contract(
                genesis_core::development_evaluation::EvaluationError::Contract(
                    genesis_core::development_method::ContractError::BudgetExceeded("wall_time_ms")
                )
            ))
        ),
        "the retained overrun must fail budget validation: {result:?}"
    );
    let request = p
        .evaluation_request("candidate", &execution)
        .unwrap()
        .unwrap();
    let binding = p.expected_binding("candidate").unwrap();
    assert!(kept
        .validate_against(&request, &execution, &binding)
        .is_err());
}
