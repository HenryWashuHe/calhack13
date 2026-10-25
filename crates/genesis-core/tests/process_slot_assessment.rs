//! Pure retained-record parsing and inert public-panel fixtures. No execution.
use genesis_core::{
    development_evaluation::{EvaluationOutcome, EvaluationReceipt},
    development_method::{ExecutionOutcome, ResourceUsage},
    development_trial::{ScoringInstances, TrialPlan, TrialPurpose},
    development_verdict::{assess_process_slot, decide, SlotEvidence, Verdict},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-fixed-cart",
    )
}
fn retained(slot: &str) -> (TrialPlan, SlotEvidence) {
    let root = root();
    let scoring: EvaluationReceipt = serde_json::from_slice(
        &fs::read(root.join(format!("{slot}-score/evaluation.json"))).unwrap(),
    )
    .unwrap();
    let EvaluationOutcome::Scored { metrics } = &scoring.outcome else {
        panic!()
    };
    let bytes = fs::read(root.join(format!("{slot}-score/artifacts/{}", metrics.sha256))).unwrap();
    (
        serde_json::from_slice(&fs::read(root.join("plan.json")).unwrap()).unwrap(),
        SlotEvidence {
            execution: serde_json::from_slice(
                &fs::read(root.join(format!("{slot}/receipt.json"))).unwrap(),
            )
            .unwrap(),
            scoring: Some(scoring),
            metrics: Some(bytes),
        },
    )
}
fn rebind(plan: &TrialPlan, slot: &str, evidence: &mut SlotEvidence) {
    let binding = plan.expected_binding(slot).unwrap();
    evidence.execution.binding = binding.clone();
    evidence.execution.binding_sha256 = binding.canonical_hash().unwrap();
    evidence.execution.actual_method = binding.method.clone();
    evidence.execution.actual_environment = binding.environment.clone();
    let request = plan
        .evaluation_request_for_accounting(slot, &evidence.execution)
        .unwrap()
        .unwrap();
    if let Some(scoring) = &mut evidence.scoring {
        scoring.request_sha256 = request
            .identity_hash(&evidence.execution, &binding)
            .unwrap();
        scoring.observed_context = Some(request.context.clone());
        scoring.request = request;
        if let (EvaluationOutcome::Scored { metrics }, Some(bytes)) =
            (&mut scoring.outcome, &evidence.metrics)
        {
            metrics.sha256 = hash(bytes);
        }
    }
}

#[test]
fn twelve_public_instances_and_both_slot_roles_use_the_same_bound_rule() {
    for slot in ["candidate", "control"] {
        let (mut plan, mut evidence) = retained(slot);
        let template = plan.development_instances[0].clone();
        plan.id = format!("inert-fresh-{slot}");
        plan.development_instances = (1000..1012)
            .map(|seed| {
                let mut r = template.clone();
                r.id = format!("rough-transport/1/{seed}");
                r.sha256 = hash(r.id.as_bytes());
                r
            })
            .collect();
        plan.context.instance_set.sha256 = hash(b"inert fresh panel");
        evidence.metrics = Some(
            serde_json::to_vec(&json!({"instances": plan.development_instances.iter()
            .enumerate().map(|(i,r)|json!({"instance":r.id,"success":i<5})).collect::<Vec<_>>()}))
            .unwrap(),
        );
        rebind(&plan, slot, &mut evidence);
        let result = assess_process_slot(&plan, slot, &evidence).unwrap();
        assert!(result.eligible);
        assert_eq!(result.successes, Some(5));
        assert_eq!(result.plan, plan.reference().unwrap());
        assert_eq!(result.slot_id, slot);
        assert_eq!(result.method, plan.expected_binding(slot).unwrap().method);
        assert!(result.reasons.is_empty());
        assert_eq!(
            serde_json::to_value(result).unwrap()["schema"],
            "genesis-process-slot-assessment/1"
        );
    }
}

#[test]
fn acceptance_semantics_stay_distinct_from_strict_process_eligibility() {
    for outcome in [
        ExecutionOutcome::Failed,
        ExecutionOutcome::Interrupted,
        ExecutionOutcome::Unknown,
    ] {
        let (plan, mut candidate) = retained("candidate");
        let (_, control) = retained("control");
        candidate.execution.outcome = outcome;
        rebind(&plan, "candidate", &mut candidate);
        let observed = assess_process_slot(&plan, "candidate", &candidate).unwrap();
        assert_eq!(observed.successes, Some(3));
        assert_eq!(observed.execution, outcome);
        assert!(!observed.eligible);
        let criterion = fs::read(root().join("criterion.json")).unwrap();
        let verdict = decide(&plan, &criterion, &candidate, &control).unwrap();
        if outcome == ExecutionOutcome::Unknown {
            assert!(matches!(verdict, Verdict::Insufficient { .. }));
        } else {
            assert_eq!(
                verdict,
                Verdict::Accepted {
                    candidate: 3,
                    control: 2,
                    of: 6
                }
            );
        }
    }
}

#[test]
fn unknown_usage_and_both_overruns_keep_observations_without_defaults() {
    for mode in 0..4 {
        let (plan, mut evidence) = retained("candidate");
        match mode {
            0 => {
                evidence.execution.usage.simulation_steps = ResourceUsage::Unknown {
                    reason: "unmeasured".into(),
                }
            }
            1 => {
                evidence.scoring.as_mut().unwrap().usage.model_calls = ResourceUsage::Unknown {
                    reason: "unmeasured".into(),
                }
            }
            2 => {
                evidence.scoring.as_mut().unwrap().usage.wall_time_ms = ResourceUsage::Measured {
                    value: plan.scoring_grant.wall_time_ms + 1,
                }
            }
            _ => {
                evidence.execution.usage.wall_time_ms = ResourceUsage::Measured {
                    value: plan.development_grant.wall_time_ms + 1,
                }
            }
        }
        rebind(&plan, "candidate", &mut evidence);
        let r = assess_process_slot(&plan, "candidate", &evidence).unwrap();
        assert_eq!(r.successes, Some(3));
        assert_eq!(r.eligible, mode == 3);
        assert_eq!(r.development_overrun, mode == 3);
        assert_eq!(r.development_steps.is_none(), mode == 0);
        assert_eq!(r.scoring_within_grant, mode == 0 || mode == 3);
    }
}

#[test]
fn absent_metrics_and_failed_scoring_are_unknown_counts_not_zero() {
    for mode in 0..3 {
        let (plan, mut evidence) = retained("candidate");
        evidence.metrics = None;
        if mode == 1 {
            evidence.scoring = None;
        }
        if mode == 2 {
            evidence.scoring.as_mut().unwrap().outcome = EvaluationOutcome::Unknown {
                reason: "unresolved".into(),
            };
        }
        let result = assess_process_slot(&plan, "candidate", &evidence).unwrap();
        assert_eq!(result.successes, None);
        assert!(!result.eligible);
        assert!(!result.reasons.is_empty());
    }
}

#[test]
fn foreign_slot_binding_context_and_hidden_instances_are_refused() {
    let (plan, evidence) = retained("candidate");
    assert!(assess_process_slot(&plan, "control", &evidence).is_err());
    for mode in 0..3 {
        let (plan, mut evidence) = retained("candidate");
        match mode {
            0 => evidence.execution.actual_method.sha256 = "a".repeat(64),
            1 => evidence.scoring.as_mut().unwrap().request.request_id = "foreign".into(),
            _ => {
                evidence
                    .scoring
                    .as_mut()
                    .unwrap()
                    .observed_context
                    .as_mut()
                    .unwrap()
                    .evaluator
                    .sha256 = "a".repeat(64)
            }
        }
        assert!(assess_process_slot(&plan, "candidate", &evidence).is_err());
    }
    let (mut hidden, evidence) = retained("candidate");
    hidden.scoring_instances = ScoringInstances::HiddenCommitment {
        protocol: hidden.criterion.clone(),
    };
    for slot in &mut hidden.slots {
        slot.purpose = TrialPurpose::Comparison {
            trial_id: "pair".into(),
            arm_id: slot.slot_id.clone(),
        };
    }
    hidden.validate().unwrap();
    assert!(assess_process_slot(&hidden, "candidate", &evidence)
        .unwrap_err()
        .contains("public Development"));
}

#[test]
fn metrics_identity_bounds_hash_order_and_boolean_shape_are_enforced() {
    for mode in 0..6 {
        let (plan, mut evidence) = retained("candidate");
        if mode == 0 {
            let EvaluationOutcome::Scored { metrics } =
                &mut evidence.scoring.as_mut().unwrap().outcome
            else {
                panic!()
            };
            metrics.schema = "foreign/1".into();
        } else if mode == 1 {
            evidence.metrics.as_mut().unwrap().push(b' ');
        } else if mode == 2 {
            evidence.metrics = Some(vec![b' '; 1024 * 1024 + 1]);
            rebind(&plan, "candidate", &mut evidence);
        } else {
            let mut value: Value =
                serde_json::from_slice(evidence.metrics.as_ref().unwrap()).unwrap();
            match mode {
                3 => value["instances"].as_array_mut().unwrap().swap(0, 1),
                4 => {
                    value["instances"][0]["success"] = "false".into();
                }
                _ => {
                    value["instances"].as_array_mut().unwrap().pop();
                }
            }
            evidence.metrics = Some(serde_json::to_vec(&value).unwrap());
            rebind(&plan, "candidate", &mut evidence);
        }
        assert!(
            assess_process_slot(&plan, "candidate", &evidence).is_err(),
            "mutation {mode}"
        );
    }
}
