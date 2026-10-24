//! Trusted acceptance scoring of a frozen slot's retained output. In-process.
use genesis_core::development_evaluation::EvaluationOutcome;
use genesis_core::development_evaluation::RequestStatus;
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage,
};
use genesis_core::development_trial::{assess_trials, TrialEvidence, TrialPlan};
use genesis_lab_exec::scoring::{score_slot, ScoringError};
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use serde_json::json;

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");

fn plan(scoring_steps: u64) -> TrialPlan {
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
    let mut p = example::acceptance_plan(&methods, Mode::None, "Python 3", 4, grant);
    p.scoring_grant.simulation_steps = scoring_steps;
    p
}

/// Runs the slot in-process, optionally submitting `design`.
fn executed(
    p: &TrialPlan,
    name: &str,
    design: Option<serde_json::Value>,
) -> (ExecutionReceipt, Store) {
    let dir = std::env::temp_dir().join(format!("lab-score-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let scope = ExperimentScope {
        plan: p,
        slot_id: "candidate",
    };
    let mut s = Session {
        plan: p,
        slot_id: "candidate",
        store: Store::open(&dir.join("slot")).unwrap(),
        ledger: ExperimentLedger::new(scope).unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding("candidate").unwrap().method),
            ..Default::default()
        },
    };
    let outcome = match design {
        Some(d) => {
            assert!(s
                .handle(&json!({"op":"submit","design":d}).to_string())
                .unwrap()["ok"]
                .as_bool()
                .unwrap());
            ExecutionOutcome::OutputGenerated
        }
        None => ExecutionOutcome::Failed,
    };
    let done = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        outcome,
        ResourceUsage::Measured { value: 5 },
    )
    .unwrap();
    (done.receipt, Store::open(&dir.join("score")).unwrap())
}

fn output_bytes(r: &ExecutionReceipt, name: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("lab-score-{name}-{}", std::process::id()));
    std::fs::read(dir.join("slot/artifacts").join(&r.outputs[0].sha256)).unwrap()
}

#[test]
fn a_retained_output_is_scored_on_every_public_instance_and_validates() {
    let p = plan(4 * 2400);
    let design: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    let (execution, store) = executed(&p, "scored", Some(design));
    let receipt = score_slot(
        &p,
        "candidate",
        &execution,
        &output_bytes(&execution, "scored"),
        &store,
    )
    .unwrap();
    let EvaluationOutcome::Scored { metrics } = &receipt.outcome else {
        panic!("{:?}", receipt.outcome)
    };
    let m: serde_json::Value = serde_json::from_slice(
        &store
            .read(&format!("artifacts/{}", metrics.sha256))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(m["of"], 4);
    let listed = m["instances"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["success"] == true)
        .count();
    assert_eq!(m["success_count"], listed);
    match receipt.usage.simulation_steps {
        ResourceUsage::Measured { value } => assert!(value > 0 && value <= 4 * 2400),
        ref other => panic!("{other:?}"),
    }
    let rows = assess_trials(
        &p,
        &[TrialEvidence {
            slot_id: "candidate".into(),
            execution,
            scoring: Some(receipt),
        }],
    )
    .unwrap();
    assert_eq!(rows[0].scoring_status, RequestStatus::Scored);
    assert_eq!(rows[1].scoring_status, RequestStatus::Missing);
}

#[test]
fn scoring_refuses_substituted_bytes_a_short_grant_and_missing_output() {
    let design: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    let p = plan(4 * 2400);
    let (execution, store) = executed(&p, "refuse", Some(design.clone()));
    let mut other = design.clone();
    other["bodies"][0]["w"] = 3.0.into();
    let substituted = serde_json::from_value::<genesis_lab_eval::design::Design>(other)
        .unwrap()
        .canonical();
    assert!(matches!(
        score_slot(&p, "candidate", &execution, &substituted, &store),
        Err(ScoringError::Refused(_))
    ));

    // One episode short of the instances: refused before anything runs.
    let short = plan(3 * 2400);
    let (execution, store) = executed(&short, "short", Some(design));
    let bytes = output_bytes(&execution, "short");
    assert!(matches!(
        score_slot(&short, "candidate", &execution, &bytes, &store),
        Err(ScoringError::Refused(_))
    ));
    assert!(
        std::fs::read_dir(store.path("artifacts"))
            .unwrap()
            .next()
            .is_none(),
        "nothing was simulated"
    );

    let (execution, store) = executed(&p, "none", None);
    assert!(matches!(
        score_slot(&p, "candidate", &execution, b"", &store),
        Err(ScoringError::NoOutput)
    ));
}
