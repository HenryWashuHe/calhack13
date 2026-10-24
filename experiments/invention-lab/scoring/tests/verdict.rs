//! Acceptance verdicts from validated candidate/control evidence. In-process.
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
) -> SlotEvidence {
    let dir = std::env::temp_dir().join(format!("lab-verdict-{name}-{}", std::process::id()));
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
        true, // in-process fixture: no model can have been called
        outcome,
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
fn a_candidate_that_beats_the_control_by_the_margin_is_accepted_and_ties_are_not() {
    let p = plan();
    let cand = evidence(&p, "candidate", "acc-c", Some(design(REFERENCE)), true);
    let ctrl = evidence(&p, "control", "acc-k", Some(design(CART)), true);
    let v = decide(&p, &criterion(&p, 1), &cand, &ctrl).unwrap();
    let Verdict::Accepted {
        candidate,
        control,
        of,
    } = v
    else {
        panic!("{v:?}")
    };
    assert_eq!((control, of), (0, 6), "the long cart crosses no ravine");
    assert!(candidate >= 1);
    // The same design on both sides never clears a margin of one.
    let same = evidence(&p, "control", "acc-same", Some(design(REFERENCE)), true);
    assert!(matches!(
        decide(&p, &criterion(&p, 1), &cand, &same).unwrap(),
        Verdict::Rejected { .. }
    ));
}

#[test]
fn missing_or_failed_evidence_is_insufficient_and_an_invalid_candidate_is_rejected() {
    let p = plan();
    let cand = evidence(&p, "candidate", "ins-c", Some(design(REFERENCE)), true);
    let unscored = evidence(&p, "control", "ins-k", Some(design(CART)), false);
    assert!(matches!(
        decide(&p, &criterion(&p, 1), &cand, &unscored).unwrap(),
        Verdict::Insufficient { .. }
    ));
    let no_output = evidence(&p, "control", "ins-none", None, true);
    assert!(matches!(
        decide(&p, &criterion(&p, 1), &cand, &no_output).unwrap(),
        Verdict::Insufficient { .. }
    ));
    let mut outside = design(REFERENCE);
    outside["bodies"][0]["x"] = 5.0.into();
    let invalid = evidence(&p, "candidate", "ins-bad", Some(outside), true);
    let ctrl = evidence(&p, "control", "ins-k2", Some(design(CART)), true);
    assert!(matches!(
        decide(&p, &criterion(&p, 1), &invalid, &ctrl).unwrap(),
        Verdict::Rejected { .. }
    ));
}

#[test]
fn substituted_criterion_metrics_or_slots_are_refused() {
    let p = plan();
    let cand = evidence(&p, "candidate", "sub-c", Some(design(REFERENCE)), true);
    let ctrl = evidence(&p, "control", "sub-k", Some(design(CART)), true);
    // A looser criterion than the plan pinned.
    assert!(decide(&p, &criterion(&p, 0), &cand, &ctrl).is_err());
    // Metrics bytes that don't match the receipt.
    let mut forged = evidence(&p, "control", "sub-f", Some(design(CART)), true);
    forged.metrics = cand.metrics.clone();
    assert!(decide(&p, &criterion(&p, 1), &cand, &forged).is_err());
    // Control evidence offered as the candidate's.
    assert!(decide(&p, &criterion(&p, 1), &ctrl, &cand).is_err());
}

#[test]
fn unknown_model_usage_needs_an_explicit_criterion_allowance() {
    // The real sandbox can't establish zero model use, so its receipts say
    // unknown; a plan accepts that only if its frozen criterion says so.
    let mut p = plan();
    let ids: Vec<String> = p
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let allowing = genesis_core::digest::canonical_json(&json!({
        "rule": "success-count-margin", "min_margin": 1, "instances": ids,
        "unknown_usage_allowed": ["model_calls", "input_tokens", "output_tokens"],
    }))
    .into_bytes();
    p.criterion = refs::content("lab-criterion/1", "success-count-margin", "1", &allowing);
    let cand = evidence_unknown(&p, "candidate", "allow-c", design(REFERENCE));
    let ctrl = evidence_unknown(&p, "control", "allow-k", design(CART));
    assert!(matches!(
        decide(&p, &allowing, &cand, &ctrl).unwrap(),
        Verdict::Accepted { .. }
    ));
    // The same evidence under the strict criterion is insufficient.
    let mut strict = p.clone();
    strict.criterion = refs::content(
        "lab-criterion/1",
        "success-count-margin",
        "1",
        &criterion(&p, 1),
    );
    let cand = evidence_unknown(&strict, "candidate", "strict-c", design(REFERENCE));
    let ctrl = evidence_unknown(&strict, "control", "strict-k", design(CART));
    assert!(matches!(
        decide(&strict, &criterion(&p, 1), &cand, &ctrl).unwrap(),
        Verdict::Insufficient { .. }
    ));
    // An allowance naming a field that doesn't exist is refused.
    let bogus = genesis_core::digest::canonical_json(&json!({
        "rule": "success-count-margin", "min_margin": 1, "instances": p.development_instances.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        "unknown_usage_allowed": ["gpu_hours"],
    }))
    .into_bytes();
    let mut bp = p.clone();
    bp.criterion = refs::content("lab-criterion/1", "success-count-margin", "1", &bogus);
    let cand = evidence_unknown(&bp, "candidate", "bogus-c", design(REFERENCE));
    let ctrl = evidence_unknown(&bp, "control", "bogus-k", design(CART));
    assert!(decide(&bp, &bogus, &cand, &ctrl).is_err());
}

/// Like `evidence`, but with model usage recorded as unknown (as the
/// namespace sandbox records it), and always scored.
fn evidence_unknown(p: &TrialPlan, slot: &'static str, name: &str, d: Value) -> SlotEvidence {
    let dir = std::env::temp_dir().join(format!("lab-verdict-u-{name}-{}", std::process::id()));
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
    s.handle(&json!({"op":"submit","design":d}).to_string())
        .unwrap();
    let execution = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        ExecutionOutcome::OutputGenerated,
        ResourceUsage::Measured { value: 1 },
    )
    .unwrap()
    .receipt;
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
