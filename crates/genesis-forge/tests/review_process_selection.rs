//! Independent checks using unchanged retained #116 bytes as data only.
//! No method source, scorer, provider or evaluator is executed.
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use genesis_forge::process_selection::*;
use serde_json::Value;
use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search",
    )
}
fn retained() -> TrialInput {
    let root = root();
    let plan: TrialPlan =
        serde_json::from_slice(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    let scoring: EvaluationReceipt =
        serde_json::from_slice(&fs::read(root.join("candidate-score/evaluation.json")).unwrap())
            .unwrap();
    let EvaluationOutcome::Scored { metrics } = &scoring.outcome else {
        panic!()
    };
    let metrics = fs::read(root.join("candidate-score/artifacts").join(&metrics.sha256)).unwrap();
    TrialInput {
        plan,
        criterion: fs::read(root.join("criterion.json")).unwrap(),
        evidence: Some(TrialEvidence {
            slot_id: "candidate".into(),
            execution: serde_json::from_slice(
                &fs::read(root.join("candidate/receipt.json")).unwrap(),
            )
            .unwrap(),
            scoring: Some(scoring),
        }),
        metrics: Some(metrics),
    }
}
fn rounds(input: TrialInput) -> [RoundInput; 3] {
    [
        RoundInput::Trial {
            input: Box::new(input),
        },
        RoundInput::NoMethod {
            reason: "generation status is caller declared; cost unresolved".into(),
        },
        RoundInput::NoMethod {
            reason: "unprepared; cost unresolved".into(),
        },
    ]
}
fn assess(input: TrialInput) -> SelectionReport {
    select_development(Arm::Workflow, Family::RoughTransport, &rounds(input)).unwrap()
}

#[test]
fn retained_rejected_child_is_selectable_without_becoming_accepted() {
    let original_verdict: Value =
        serde_json::from_slice(&fs::read(root().join("verdict.json")).unwrap()).unwrap();
    assert_eq!(original_verdict["verdict"], "rejected");
    let original = retained();
    let expected_method = original
        .plan
        .slots
        .iter()
        .find(|s| s.slot_id == "candidate")
        .unwrap()
        .method
        .clone();
    let report = assess(original);
    assert_eq!(report.scope, "single_arm_family_development");
    assert_eq!(report.selected_round, Some(1));
    assert_eq!(report.rows.len(), 3);
    assert_eq!(report.rows[0].successes, Some(3));
    assert_eq!(report.rows[0].method.as_ref(), Some(&expected_method));
    assert!(report.rows[0].eligible);
    assert!(report.rows[1..]
        .iter()
        .all(|r| !r.eligible && r.successes.is_none()));
    // Selection can retain a usable candidate that lost to its control. It is
    // neither a replacement acceptance verdict nor evidence of an improvement.
    assert!(serde_json::to_value(report)
        .unwrap()
        .get("verdict")
        .is_none());
}

#[test]
fn typed_input_roundtrip_is_stable_but_arm_and_absence_changes_are_bound() {
    let inputs = rounds(retained());
    let encoded = serde_json::to_vec_pretty(&inputs).unwrap();
    let roundtrip: [RoundInput; 3] = serde_json::from_slice(&encoded).unwrap();
    let first = select_development(Arm::Workflow, Family::RoughTransport, &inputs).unwrap();
    let second = select_development(Arm::Workflow, Family::RoughTransport, &roundtrip).unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    let fresh = select_development(Arm::Fresh, Family::RoughTransport, &inputs).unwrap();
    assert_ne!(first.input_sha256, fresh.input_sha256);
    let mut changed = inputs;
    changed[2] = RoundInput::NoMethod {
        reason: "different retained absence explanation".into(),
    };
    let changed = select_development(Arm::Workflow, Family::RoughTransport, &changed).unwrap();
    assert_ne!(first.input_sha256, changed.input_sha256);
    assert_eq!(changed.selected_round, Some(1));
}

#[test]
fn missing_and_unscored_receipts_preserve_absence_without_zero_or_selection() {
    let mut missing_metrics = retained();
    missing_metrics.metrics = None;
    let missing = assess(missing_metrics);
    assert!(matches!(
        missing.rows[0].scoring,
        Some(EvaluationOutcome::Scored { .. })
    ));
    assert_eq!(missing.rows[0].successes, None);
    assert_eq!(missing.selected_round, None);
    for outcome in [
        None,
        Some(EvaluationOutcome::Unknown {
            reason: "unresolved evaluator result".into(),
        }),
        Some(EvaluationOutcome::Failed {
            reason: "confirmed scorer failure".into(),
        }),
        Some(EvaluationOutcome::Invalid {
            reason: "invalid retained design".into(),
        }),
    ] {
        let mut input = retained();
        input.metrics = None;
        let evidence = input.evidence.as_mut().unwrap();
        if let Some(outcome) = outcome {
            evidence.scoring.as_mut().unwrap().outcome = outcome;
        } else {
            evidence.scoring = None;
        }
        let report = assess(input);
        assert_eq!(report.selected_round, None);
        assert_eq!(report.rows[0].successes, None);
        assert!(report.rows[0].development_usage.is_some());
        assert!(!report.rows[0].eligible);
    }
}

#[test]
fn metrics_with_no_attributable_scored_receipt_are_refused() {
    for mode in 0..3 {
        let mut input = retained();
        match mode {
            0 => input.evidence = None,
            1 => input.evidence.as_mut().unwrap().scoring = None,
            _ => {
                input
                    .evidence
                    .as_mut()
                    .unwrap()
                    .scoring
                    .as_mut()
                    .unwrap()
                    .outcome = EvaluationOutcome::Unknown {
                    reason: "unknown".into(),
                }
            }
        }
        let result = select_development(Arm::Workflow, Family::RoughTransport, &rounds(input));
        assert!(result.is_err(), "orphan metrics mode {mode}");
    }
}
