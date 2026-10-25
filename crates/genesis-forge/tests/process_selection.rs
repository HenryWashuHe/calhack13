//! Pure inert receipt/metrics fixtures; no source, evaluator or provider runs.
use genesis_core::{development_evaluation::*, development_method::*};
use genesis_forge::process_selection::*;
use serde_json::json;
use sha2::{Digest, Sha256};
#[allow(dead_code)]
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn absent() -> RoundInput {
    RoundInput::NoMethod {
        reason: "not produced; costs unresolved".into(),
    }
}
fn input_mut(round: &mut RoundInput) -> &mut TrialInput {
    let RoundInput::Trial { input } = round else {
        panic!()
    };
    input
}
fn rebind(input: &mut TrialInput) {
    let Some(e) = &mut input.evidence else { return };
    let binding = input.plan.expected_binding("candidate").unwrap();
    e.execution.binding = binding.clone();
    e.execution.binding_sha256 = binding.canonical_hash().unwrap();
    e.execution.actual_method = binding.method.clone();
    e.execution.actual_environment = binding.environment.clone();
    let request = input
        .plan
        .evaluation_request_for_accounting("candidate", &e.execution)
        .unwrap()
        .unwrap();
    if let Some(s) = &mut e.scoring {
        s.request_sha256 = request.identity_hash(&e.execution, &binding).unwrap();
        s.observed_context = Some(request.context.clone());
        s.request = request;
        if let (EvaluationOutcome::Scored { metrics }, Some(bytes)) =
            (&mut s.outcome, &input.metrics)
        {
            metrics.sha256 = digest(bytes);
        }
    }
}
fn trial(round: usize, successes: usize, steps: u64) -> RoundInput {
    let mut plan = fixture::trial(&fixture::generation("inert", true));
    plan.id = format!("round-{round}");
    let original = plan.development_instances[0].clone();
    plan.development_instances = (0..6)
        .map(|seed| ExactContentRef {
            domain: "genesis-lab".into(),
            schema: "lab-instance/1".into(),
            id: format!("rough-transport/1/{seed}"),
            version: "1".into(),
            ..original.clone()
        })
        .collect();
    for slot in &mut plan.slots {
        slot.run_id = format!("round-{round}-{}", slot.slot_id);
        slot.attempt_id = format!("attempt-{round}-{}", slot.slot_id).into();
        slot.scoring_request_id = format!("score-{round}-{}", slot.slot_id);
    }
    plan.slots[0].method.sha256 = format!("{round:064x}");
    let criterion = serde_json::to_vec(&json!({"rule":"success-count-margin", "min_margin":1,
        "instances":plan.development_instances.iter().map(|r| &r.id).collect::<Vec<_>>()}))
    .unwrap();
    plan.criterion.sha256 = digest(&criterion);
    let metrics = serde_json::to_vec(
        &json!({"instances":plan.development_instances.iter().enumerate()
        .map(|(i,r)| json!({"instance":r.id,"success":i < successes})).collect::<Vec<_>>()}),
    )
    .unwrap();
    let mut evidence = fixture::evidence(&plan, "candidate");
    evidence.execution.usage.simulation_steps = ResourceUsage::Measured { value: steps };
    evidence.scoring.as_mut().unwrap().outcome = EvaluationOutcome::Scored {
        metrics: ExactContentRef {
            domain: "genesis-lab".into(),
            schema: "lab-scoring-metrics/1".into(),
            id: "lab-scoring-metrics/1".into(),
            version: "1".into(),
            sha256: digest(&metrics),
        },
    };
    let mut input = TrialInput {
        plan,
        criterion,
        evidence: Some(evidence),
        metrics: Some(metrics),
    };
    rebind(&mut input);
    RoundInput::Trial {
        input: Box::new(input),
    }
}
fn select(rounds: &[RoundInput; 3]) -> Result<SelectionReport, String> {
    select_development(Arm::Workflow, Family::RoughTransport, rounds)
}

#[test]
fn zero_is_eligible_missing_is_not_and_all_three_positions_survive() {
    let mut missing = trial(2, 6, 1);
    let i = input_mut(&mut missing);
    i.evidence = None;
    i.metrics = None;
    let r = select(&[trial(1, 0, 5), missing, absent()]).unwrap();
    assert_eq!(r.selected_round, Some(1));
    assert_eq!(r.rows.len(), 3);
    assert_eq!(r.rows[0].successes, Some(0));
    assert_eq!(r.rows[1].successes, None);
    assert!(r.rows[1].plan.is_some());
    assert!(r.rows[2].no_method.is_some());
    assert_eq!(
        select(&[absent(), absent(), absent()])
            .unwrap()
            .selected_round,
        None
    );
}
#[test]
fn failed_interrupted_and_unknown_scored_runs_keep_counts_without_becoming_eligible() {
    let mut rounds = [trial(1, 6, 1), trial(2, 6, 1), trial(3, 6, 1)];
    for (round, outcome) in rounds.iter_mut().zip([
        ExecutionOutcome::Failed,
        ExecutionOutcome::Interrupted,
        ExecutionOutcome::Unknown,
    ]) {
        let i = input_mut(round);
        i.evidence.as_mut().unwrap().execution.outcome = outcome;
        rebind(i);
    }
    let r = select(&rounds).unwrap();
    assert_eq!(r.selected_round, None);
    assert!(r.rows.iter().all(|r| !r.eligible && r.successes == Some(6)));
}
#[test]
fn unknown_usage_scoring_overrun_and_missing_metrics_are_observations_not_zero() {
    let mut rounds = [trial(1, 6, 1), trial(2, 6, 1), trial(3, 6, 1)];
    let i = input_mut(&mut rounds[0]);
    i.evidence.as_mut().unwrap().execution.usage.model_calls = ResourceUsage::Unknown {
        reason: "unmeasured".into(),
    };
    rebind(i);
    input_mut(&mut rounds[1])
        .evidence
        .as_mut()
        .unwrap()
        .scoring
        .as_mut()
        .unwrap()
        .usage
        .wall_time_ms = ResourceUsage::Measured { value: 101 };
    input_mut(&mut rounds[2]).metrics = None;
    let r = select(&rounds).unwrap();
    assert_eq!(r.selected_round, None);
    assert_eq!(r.rows[0].successes, Some(6));
    assert!(!r.rows[1].scoring_within_grant);
    assert_eq!(r.rows[2].successes, None);
}
#[test]
fn ranking_is_successes_then_steps_then_original_round_not_input_hash() {
    assert_eq!(
        select(&[trial(1, 2, 5), trial(2, 3, 9), trial(3, 1, 1)])
            .unwrap()
            .selected_round,
        Some(2)
    );
    assert_eq!(
        select(&[trial(1, 3, 9), trial(2, 3, 2), trial(3, 3, 5)])
            .unwrap()
            .selected_round,
        Some(2)
    );
    assert_eq!(
        select(&[trial(1, 3, 2), trial(2, 3, 2), trial(3, 3, 2)])
            .unwrap()
            .selected_round,
        Some(1)
    );
}
#[test]
fn frozen_criterion_family_cohort_and_receipt_identity_cannot_be_swapped() {
    let original = [trial(1, 1, 1), trial(2, 2, 1), trial(3, 3, 1)];
    for mutate in 0..8 {
        let mut rounds = original.clone();
        let i = input_mut(&mut rounds[1]);
        match mutate {
            0 => {
                i.criterion.push(b' ');
            }
            1 => {
                i.plan.development_instances[0].id = "rough-transport/1/1000".into();
            }
            2 => {
                i.plan.context.evaluator.sha256 = "f".repeat(64);
                rebind(i);
            }
            3 => {
                i.plan.scoring_grant.wall_time_ms += 1;
                rebind(i);
            }
            4 => {
                i.plan.slots[1].method.sha256 = "f".repeat(64);
                rebind(i);
            }
            5 => {
                i.evidence.as_mut().unwrap().execution.actual_method.sha256 = "f".repeat(64);
            }
            6 => {
                i.evidence.as_mut().unwrap().slot_id = "control".into();
            }
            _ => {
                i.evidence
                    .as_mut()
                    .unwrap()
                    .scoring
                    .as_mut()
                    .unwrap()
                    .request
                    .request_id = "foreign".into();
            }
        }
        assert!(select(&rounds).is_err(), "mutation {mutate}");
    }
    assert!(select_development(Arm::Fresh, Family::RavineCrossing, &original).is_err());
}
#[test]
fn duplicate_identity_is_rejected_but_same_package_in_distinct_trials_is_allowed() {
    let mut rounds = [trial(1, 1, 1), trial(2, 2, 1), trial(3, 3, 1)];
    let candidate = input_mut(&mut rounds[0]).plan.slots[0].method.clone();
    let i = input_mut(&mut rounds[1]);
    i.plan.slots[0].method = candidate;
    rebind(i);
    assert_eq!(select(&rounds).unwrap().selected_round, Some(3));
    for field in ["plan", "run", "attempt", "scoring"] {
        let mut r = rounds.clone();
        let a = input_mut(&mut r[0]).plan.clone();
        let b = input_mut(&mut r[1]);
        match field {
            "plan" => b.plan.id = a.id,
            "run" => b.plan.slots[0].run_id = a.slots[0].run_id.clone(),
            "attempt" => b.plan.slots[0].attempt_id = a.slots[0].attempt_id.clone(),
            _ => b.plan.slots[0].scoring_request_id = a.slots[0].scoring_request_id.clone(),
        }
        rebind(b);
        assert!(select(&r).is_err(), "{field}");
    }
}
#[test]
fn bound_metrics_require_supported_identity_order_boolean_and_digest() {
    for mutation in 0..5 {
        let mut rounds = [trial(1, 6, 1), absent(), absent()];
        let i = input_mut(&mut rounds[0]);
        if mutation == 0 {
            i.metrics.as_mut().unwrap().push(b' ');
        } else if mutation == 1 {
            let EvaluationOutcome::Scored { metrics } = &mut i
                .evidence
                .as_mut()
                .unwrap()
                .scoring
                .as_mut()
                .unwrap()
                .outcome
            else {
                panic!()
            };
            metrics.schema = "foreign-metrics/1".into();
        } else {
            let mut value: serde_json::Value =
                serde_json::from_slice(i.metrics.as_ref().unwrap()).unwrap();
            match mutation {
                2 => {
                    value["instances"][0]["instance"] = "foreign".into();
                }
                3 => {
                    value["instances"][0]["success"] = "true".into();
                }
                _ => {
                    value["instances"].as_array_mut().unwrap().pop();
                }
            }
            i.metrics = Some(serde_json::to_vec(&value).unwrap());
            rebind(i);
        }
        assert!(select(&rounds).is_err(), "metrics mutation {mutation}");
    }
}
#[test]
fn known_development_overrun_is_visible_without_changing_the_frozen_eligibility_rule() {
    let r = select(&[trial(1, 2, 101), absent(), absent()]).unwrap();
    assert!(r.rows[0].development_overrun);
    assert!(r.rows[0].eligible);
    assert_eq!(r.rows[0].successes, Some(2));
    // No authorization, accepted verdict, endpoint or cost-zero inference.
    assert_eq!(r.selected_round, Some(1));
}
