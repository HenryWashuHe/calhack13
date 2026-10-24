//! Independent verdict regressions. Local in-process fixtures; no cloud or namespace proof.
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
fn unknown_development_accounting_is_not_an_acceptance() {
    let p = plan();
    let cand = evidence(
        &p,
        "candidate",
        "unknown-c",
        Some(design(REFERENCE)),
        true,
        None,
        false,
    );
    let ctrl = evidence(
        &p,
        "control",
        "unknown-k",
        Some(design(CART)),
        true,
        None,
        false,
    );
    assert!(matches!(
        cand.execution.usage.model_calls,
        ResourceUsage::Unknown { .. }
    ));
    let v = decide(&p, &criterion(&p, 1), &cand, &ctrl).unwrap();
    assert!(
        matches!(v, Verdict::Insufficient { .. }),
        "unknown development resource accounting accepted: {v:?}"
    );
}

#[test]
fn a_failed_development_attempt_with_a_scored_partial_output_is_not_accepted() {
    let p = plan();
    // In-process fixture has zero actual model calls; failure retains its output.
    let cand = evidence(
        &p,
        "candidate",
        "failed-c",
        Some(design(REFERENCE)),
        true,
        Some(ExecutionOutcome::Failed),
        true,
    );
    let ctrl = evidence(
        &p,
        "control",
        "failed-k",
        Some(design(CART)),
        true,
        None,
        true,
    );
    assert_eq!(cand.execution.outcome, ExecutionOutcome::Failed);
    assert!(matches!(
        cand.scoring.as_ref().unwrap().outcome,
        EvaluationOutcome::Scored { .. }
    ));
    let v = decide(&p, &criterion(&p, 1), &cand, &ctrl).unwrap();
    assert!(
        matches!(v, Verdict::Insufficient { .. }),
        "failed development became accepted because its partial output scored well: {v:?}"
    );
}

#[test]
fn unnamed_extra_metric_rows_cannot_invent_candidate_successes() {
    let p = plan();
    let mut cand = evidence(
        &p,
        "candidate",
        "extra-c",
        Some(design(CART)),
        true,
        None,
        true,
    );
    let ctrl = evidence(
        &p,
        "control",
        "extra-k",
        Some(design(CART)),
        true,
        None,
        true,
    );
    assert!(matches!(
        decide(&p, &criterion(&p, 1), &cand, &ctrl).unwrap(),
        Verdict::Rejected { .. }
    ));
    let mut metrics: Value = serde_json::from_slice(cand.metrics.as_ref().unwrap()).unwrap();
    metrics["instances"]
        .as_array_mut()
        .unwrap()
        .push(json!({"success":true}));
    let bytes = serde_json::to_vec(&metrics).unwrap();
    let EvaluationOutcome::Scored { metrics: reference } =
        &mut cand.scoring.as_mut().unwrap().outcome
    else {
        panic!("expected scored fixture")
    };
    // Bind the malformed artifact honestly: this is a schema-validation test,
    // not an assertion that hashes authenticate a scoring source.
    reference.sha256 = refs::sha256_hex(&bytes);
    cand.metrics = Some(bytes);
    let v = decide(&p, &criterion(&p, 1), &cand, &ctrl);
    assert!(
        !matches!(v, Ok(Verdict::Accepted { .. })),
        "unnamed row counted as a real criterion instance: {v:?}"
    );
}
