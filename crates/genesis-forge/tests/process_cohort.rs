//! Inert typed plans and receipts only; no method/evaluator/provider execution.
use genesis_core::{development_evaluation::*, development_method::*, digest::canonical_sha256};
use genesis_forge::{process_cohort::*, process_selection::*, process_selection_record::*};
use serde_json::json;
use sha2::{Digest, Sha256};
#[allow(dead_code)]
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

fn pin(record: &RetainedSelection) -> SelectionPin {
    SelectionPin {
        arm: record.inputs.arm,
        family: record.inputs.family,
        selection_sha256: record.sha256(),
        rounds: record.inputs.rounds.clone().map(|r| match r {
            RoundInput::NoMethod { reason } => ExpectedRound::NoMethod { reason },
            RoundInput::Trial { input } => ExpectedRound::Trial {
                plan: input.plan.reference().unwrap(),
            },
        }),
    }
}
fn declaration(records: &[RetainedSelection; 8]) -> CohortDeclaration {
    CohortDeclaration {
        schema: "genesis-process-cohort-declaration/1".into(),
        selections: records.clone().map(|r| pin(&r)),
    }
}
fn records() -> [RetainedSelection; 8] {
    let base = fixture::trial(&fixture::generation("inert", true));
    let families = [
        Family::RavineCrossing,
        Family::RoughTransport,
        Family::ReturnMix,
        Family::ProjectileTarget,
    ];
    std::array::from_fn(|cell| {
        let family = families[cell % 4];
        RetainedSelection::capture(SelectionInputs {
            arm: if cell < 4 { Arm::Workflow } else { Arm::Fresh }, family,
            rounds: std::array::from_fn(|round| {
                let mut plan = base.clone();
                plan.id = format!("trial-{cell}-{round}");
                plan.context.public_task.id = family.name().into();
                plan.context.instance_set.id = family.name().into();
                plan.context.generator.id = family.name().into();
                plan.context.scoring_rule.id = family.name().into();
                plan.development_instances = (0..6).map(|seed| ExactContentRef {
                    domain: "genesis-lab".into(), schema: "lab-instance/1".into(),
                    id: format!("{}/1/{seed}", family.name()), version: "1".into(), sha256: "a".repeat(64),
                }).collect();
                let criterion = serde_json::to_vec(&json!({"rule":"success-count-margin", "min_margin":1,
                    "instances":plan.development_instances.iter().map(|r| &r.id).collect::<Vec<_>>()})).unwrap();
                plan.criterion.sha256 = format!("{:x}", Sha256::digest(&criterion));
                plan.development_grant = ResourceGrant { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 60000, simulation_steps: 72000 };
                plan.scoring_grant = ResourceGrant { simulation_steps: if cell % 4 == 3 { 7200 } else { 14400 }, wall_time_ms: 600000, ..plan.development_grant.clone() };
                for slot in &mut plan.slots {
                    slot.run_id = format!("{cell}-{round}-{}", slot.slot_id);
                    slot.attempt_id = slot.run_id.clone().into();
                    slot.scoring_request_id = slot.run_id.clone();
                }
                plan.slots[0].method.id = format!("method-{}-{round}", cell / 4);
                RoundInput::Trial { input: Box::new(TrialInput { plan, criterion, evidence: None, metrics: None }) }
            }),
        }).unwrap()
    })
}
fn trial(record: &mut RetainedSelection, round: usize) -> &mut TrialInput {
    let RoundInput::Trial { input } = &mut record.inputs.rounds[round] else {
        panic!()
    };
    input
}
fn recapture(record: &mut RetainedSelection) {
    *record = RetainedSelection::capture(record.inputs.clone()).unwrap();
}

#[test]
fn complete_roster_keeps_missing_evidence_and_all_positions_without_zero_success() {
    let mut r = records();
    let d = declaration(&r);
    r.reverse(); // Evidence arrival order is irrelevant; output order is stable.
    let report = assess_cohort(&d, &r).unwrap();
    assert_eq!(report.cells.len(), 8);
    assert_eq!(report.declaration_sha256, canonical_sha256(&json!(d)));
    for cell in &report.cells {
        assert_eq!(cell.selection.rows.len(), 3);
        assert_eq!(cell.selection.selected_round, None);
        assert!(cell.selection.rows.iter().all(|r| r.successes.is_none()));
    }
}
#[test]
fn every_arm_family_and_exact_external_pin_is_required() {
    let original = records();
    for mutation in 0..5 {
        let mut r = original.clone();
        let mut d = declaration(&r);
        match mutation {
            0 => r[7] = r[6].clone(),
            1 => d.selections[7] = d.selections[6].clone(),
            2 => d.selections[0].selection_sha256 = "f".repeat(64),
            3 => {
                if let ExpectedRound::Trial { plan } = &mut d.selections[0].rounds[0] {
                    plan.id = "same-digest-foreign-id".into();
                }
            }
            _ => {
                r[0].report["selected_round"] = json!(1);
                d.selections[0].selection_sha256 = r[0].sha256();
            }
        }
        assert!(assess_cohort(&d, &r).is_err(), "mutation {mutation}");
    }
}
#[test]
fn candidates_control_policy_and_numeric_caps_cannot_diverge_even_when_repinned() {
    let original = records();
    for mutation in 0..9 {
        let mut r = original.clone();
        // All three rounds stay locally coherent; this must fail at cohort level.
        for round in 0..3 {
            let i = trial(&mut r[1], round);
            match mutation {
                0 => i.plan.slots[0].method.id.push_str("-foreign"),
                1 => {
                    let dependency = i.plan.model_settings.clone();
                    i.plan.slots[0].dependencies.push(dependency);
                }
                2 => i.plan.slots[1].method.id.push_str("-foreign"),
                3 => i.plan.model_settings.id.push_str("-foreign"),
                4 => i.plan.context.public_task.id.push_str("-foreign"),
                5 => i.plan.development_grant.simulation_steps += 1,
                6 => i.plan.scoring_grant.simulation_steps += 1,
                7 => i.plan.development_grant.input_tokens = 1,
                _ => i.plan.context.evaluator.id.push_str("-foreign"),
            }
        }
        recapture(&mut r[1]);
        assert!(
            assess_cohort(&declaration(&r), &r).is_err(),
            "mutation {mutation}"
        );
    }
}
#[test]
fn repeated_trial_run_attempt_and_scoring_ids_across_families_are_refused() {
    for mutation in 0..4 {
        let mut r = records();
        let a = trial(&mut r[0], 0).plan.clone();
        let b = &mut trial(&mut r[1], 0).plan;
        match mutation {
            0 => b.id = a.id,
            1 => b.slots[1].run_id = a.slots[1].run_id.clone(),
            2 => b.slots[1].attempt_id = a.slots[1].attempt_id.clone(),
            _ => b.slots[1].scoring_request_id = a.slots[1].scoring_request_id.clone(),
        }
        recapture(&mut r[1]);
        assert!(
            assess_cohort(&declaration(&r), &r).is_err(),
            "mutation {mutation}"
        );
    }
}
#[test]
fn no_method_is_arm_round_wide_and_never_a_zero_cost_generation_claim() {
    let mut r = records();
    for record in &mut r[..4] {
        record.inputs.rounds[1] = RoundInput::NoMethod {
            reason: "generation unresolved".into(),
        };
        recapture(record);
    }
    let report = assess_cohort(&declaration(&r), &r).unwrap();
    assert!(report
        .cells
        .iter()
        .filter(|c| c.selection.arm == Arm::Workflow)
        .all(|c| c.selection.rows[1].no_method.as_deref() == Some("generation unresolved")));
    r[0].inputs.rounds[1] = RoundInput::NoMethod {
        reason: "no_parent".into(),
    };
    recapture(&mut r[0]);
    assert!(assess_cohort(&declaration(&r), &r).is_err());
}
#[test]
fn selected_zero_score_keeps_complete_original_method_and_plan_identity() {
    let mut r = records();
    let i = trial(&mut r[0], 2);
    let mut e = fixture::evidence(&i.plan, "candidate");
    e.execution.usage = serde_json::from_value(json!({
        "model_calls":{"kind":"measured","value":0}, "input_tokens":{"kind":"measured","value":0},
        "output_tokens":{"kind":"measured","value":0}, "wall_time_ms":{"kind":"measured","value":1},
        "simulation_steps":{"kind":"measured","value":1}}))
    .unwrap();
    let binding = i.plan.expected_binding("candidate").unwrap();
    let request = i
        .plan
        .evaluation_request_for_accounting("candidate", &e.execution)
        .unwrap()
        .unwrap();
    let metrics = serde_json::to_vec(&json!({"instances":i.plan.development_instances.iter()
        .map(|r| json!({"instance":r.id,"success":false})).collect::<Vec<_>>()}))
    .unwrap();
    let s = e.scoring.as_mut().unwrap();
    s.request_sha256 = request.identity_hash(&e.execution, &binding).unwrap();
    s.request = request;
    s.usage = e.execution.usage.clone();
    s.outcome = EvaluationOutcome::Scored {
        metrics: ExactContentRef {
            domain: "genesis-lab".into(),
            schema: "lab-scoring-metrics/1".into(),
            id: "lab-scoring-metrics/1".into(),
            version: "1".into(),
            sha256: format!("{:x}", Sha256::digest(&metrics)),
        },
    };
    let method = i.plan.slots[0].method.clone();
    let plan = i.plan.reference().unwrap();
    i.evidence = Some(e);
    i.metrics = Some(metrics);
    recapture(&mut r[0]);
    let report = assess_cohort(&declaration(&r), &r).unwrap();
    let selected = &report
        .cells
        .iter()
        .find(|c| c.selection.arm == Arm::Workflow && c.selection.family == Family::RavineCrossing)
        .unwrap()
        .selection;
    assert_eq!(selected.selected_round, Some(3));
    assert_eq!(selected.rows[2].method.as_ref(), Some(&method));
    assert_eq!(selected.rows[2].plan.as_ref(), Some(&plan));
    assert_eq!(selected.rows[2].successes, Some(0));
}
