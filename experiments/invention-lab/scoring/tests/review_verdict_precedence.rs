//! Independent corrected-head outcome precedence checks. Local in-process evidence only.
use genesis_core::development_evaluation::EvaluationOutcome;
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::score_slot;
use genesis_lab_score::verdict::{decide, SlotEvidence, Verdict};
use serde_json::{json, Value};

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");
const CART: &str = include_str!("../../evaluator/designs/cart-long.json");

fn plan() -> TrialPlan {
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "cand", "1", b"cand"),
        control: refs::content("lab-method/1", "ctrl", "1", b"ctrl"),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: 2400,
    };
    example::acceptance_plan(&methods, Mode::None, "Python 3", 6, grant)
}

fn criterion(p: &TrialPlan, margin: u64) -> Vec<u8> {
    let ids: Vec<String> = p
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    example::criterion_bytes(&ids, margin)
}

/// Runs `slot` in-process with `design` submitted (or nothing), then scores it.
fn evidence(
    p: &TrialPlan,
    slot: &'static str,
    name: &str,
    design: Option<Value>,
    score: bool,
    finish_outcome: Option<ExecutionOutcome>,
    known_model_usage: bool,
) -> SlotEvidence {
    let dir =
        std::env::temp_dir().join(format!("lab-review-verdict-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let scope = ExperimentScope {
        plan: p,
        slot_id: slot,
    };
    let mut s = Session {
        plan: p,
        slot_id: slot,
        store: Store::open(&dir.join("slot")).unwrap(),
        ledger: ExperimentLedger::new(scope).unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding(slot).unwrap().method),
            ..Default::default()
        },
    };
    let outcome = match &design {
        Some(d) => {
            s.handle(&json!({"op":"submit","design":d}).to_string())
                .unwrap();
            ExecutionOutcome::OutputGenerated
        }
        None => ExecutionOutcome::Failed,
    };
    let done = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        known_model_usage,
        finish_outcome.unwrap_or(outcome),
        ResourceUsage::Measured { value: 1 },
    )
    .unwrap();
    let execution = done.receipt;
    if !score || execution.outputs.is_empty() {
        return SlotEvidence {
            execution,
            scoring: None,
            metrics: None,
        };
    }
    let out = s
        .store
        .read(&format!("artifacts/{}", execution.outputs[0].sha256))
        .unwrap();
    let store = Store::open(&dir.join("score")).unwrap();
    let receipt = score_slot(p, slot, &execution, &out, &store).unwrap();
    let metrics = match &receipt.outcome {
        EvaluationOutcome::Scored { metrics } => Some(
            store
                .read(&format!("artifacts/{}", metrics.sha256))
                .unwrap(),
        ),
        _ => None,
    };
    SlotEvidence {
        execution,
        scoring: Some(receipt),
        metrics,
    }
}

fn design(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn an_invalid_design_does_not_hide_an_unknown_candidate_execution() {
    let p = plan();
    let mut invalid = design(REFERENCE);
    invalid["bodies"][0]["x"] = 5.0.into();
    let cand = evidence(
        &p,
        "candidate",
        "invalid-unknown-c",
        Some(invalid),
        true,
        Some(ExecutionOutcome::Unknown),
        true,
    );
    let ctrl = evidence(
        &p,
        "control",
        "invalid-known-k",
        Some(design(CART)),
        true,
        None,
        true,
    );
    assert_eq!(cand.execution.outcome, ExecutionOutcome::Unknown);
    assert!(matches!(
        cand.scoring.as_ref().unwrap().outcome,
        EvaluationOutcome::Invalid { .. }
    ));
    let verdict = decide(&p, &criterion(&p, 1), &cand, &ctrl).unwrap();
    assert!(matches!(verdict, Verdict::Insufficient { .. }),
        "unknown executor must remain unresolved even when its retained design is invalid: {verdict:?}");
}

#[test]
fn an_invalid_candidate_does_not_hide_an_unknown_control_execution() {
    let p = plan();
    let mut invalid = design(REFERENCE);
    invalid["bodies"][0]["x"] = 5.0.into();
    let cand = evidence(
        &p,
        "candidate",
        "invalid-known-c",
        Some(invalid),
        true,
        None,
        true,
    );
    let ctrl = evidence(
        &p,
        "control",
        "invalid-unknown-k",
        Some(design(CART)),
        true,
        Some(ExecutionOutcome::Unknown),
        true,
    );
    assert_eq!(ctrl.execution.outcome, ExecutionOutcome::Unknown);
    assert!(matches!(
        cand.scoring.as_ref().unwrap().outcome,
        EvaluationOutcome::Invalid { .. }
    ));
    let verdict = decide(&p, &criterion(&p, 1), &cand, &ctrl).unwrap();
    assert!(
        matches!(verdict, Verdict::Insufficient { .. }),
        "unknown control must remain unresolved before an acceptance verdict: {verdict:?}"
    );
}
