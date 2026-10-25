//! Independent cohort checks using inert copied plans; no source is executed.
use genesis_core::{development_trial::TrialPlan, digest::canonical_sha256};
use genesis_forge::{process_cohort::*, process_selection::*, process_selection_record::*};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn families() -> [Family; 4] {
    [
        Family::RavineCrossing,
        Family::RoughTransport,
        Family::ReturnMix,
        Family::ProjectileTarget,
    ]
}
fn capture(inputs: SelectionInputs) -> RetainedSelection {
    RetainedSelection::capture(inputs).unwrap()
}
fn declaration(records: &[RetainedSelection; 8]) -> CohortDeclaration {
    CohortDeclaration {
        schema: "genesis-process-cohort-declaration/1".into(),
        selections: std::array::from_fn(|i| SelectionPin {
            arm: records[i].inputs.arm,
            family: records[i].inputs.family,
            selection_sha256: records[i].sha256(),
            rounds: records[i].inputs.rounds.clone().map(|r| match r {
                RoundInput::NoMethod { reason } => ExpectedRound::NoMethod { reason },
                RoundInput::Trial { input } => ExpectedRound::Trial {
                    plan: input.plan.reference().unwrap(),
                },
            }),
        }),
    }
}
fn absent() -> [RetainedSelection; 8] {
    std::array::from_fn(|i| {
        capture(SelectionInputs {
            arm: if i < 4 { Arm::Workflow } else { Arm::Fresh },
            family: families()[i % 4],
            rounds: std::array::from_fn(|round| RoundInput::NoMethod {
                reason: format!("round {round}: no generated method; cost unresolved"),
            }),
        })
    })
}
fn trials() -> [RetainedSelection; 8] {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-parent/plan.json",
    );
    let base: TrialPlan = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let mut records = absent();
    for (cell, record) in records.iter_mut().enumerate() {
        let family = record.inputs.family.name();
        record.inputs.rounds = std::array::from_fn(|round| {
            let mut plan = base.clone();
            plan.id = format!("independent-{cell}-{round}");
            for reference in [
                &mut plan.context.public_task,
                &mut plan.context.generator,
                &mut plan.context.scoring_rule,
                &mut plan.context.instance_set,
            ] {
                reference.id = family.into();
                reference.sha256 = format!(
                    "{:x}",
                    Sha256::digest(format!("{family}:{}", reference.schema))
                );
            }
            for (seed, instance) in plan.development_instances.iter_mut().enumerate() {
                instance.id = format!("{family}/1/{seed}");
                instance.sha256 = format!("{:x}", Sha256::digest(&instance.id));
            }
            let criterion =
                serde_json::to_vec(&json!({"rule":"success-count-margin","min_margin":1,
                "instances":plan.development_instances.iter().map(|r| &r.id).collect::<Vec<_>>()}))
                .unwrap();
            plan.criterion.sha256 = format!("{:x}", Sha256::digest(&criterion));
            plan.scoring_grant.simulation_steps = if family == "projectile-target" {
                7200
            } else {
                14400
            };
            for slot in &mut plan.slots {
                slot.run_id = format!("{cell}-{round}-{}", slot.slot_id);
                slot.attempt_id = format!("att-{cell}-{round}-{}", slot.slot_id).into();
                slot.scoring_request_id = format!("score-{cell}-{round}-{}", slot.slot_id);
            }
            plan.slots[0].method.id = format!("arm-{}-round-{round}", cell / 4);
            RoundInput::Trial {
                input: Box::new(TrialInput {
                    plan,
                    criterion,
                    evidence: None,
                    metrics: None,
                }),
            }
        });
        *record = capture(record.inputs.clone());
    }
    records
}
fn trial(record: &mut RetainedSelection, round: usize) -> &mut TrialInput {
    let RoundInput::Trial { input } = &mut record.inputs.rounds[round] else {
        panic!()
    };
    input
}

#[test]
fn all_absent_positions_survive_wire_roundtrip_without_zero_or_authority() {
    let original = absent();
    let bytes = serde_json::to_vec_pretty(&original).unwrap();
    let decoded: [RetainedSelection; 8] = serde_json::from_slice(&bytes).unwrap();
    for (a, b) in original.iter().zip(&decoded) {
        assert_eq!(a.sha256(), b.sha256());
    }
    let declared = declaration(&decoded);
    let result = assess_cohort(&declared, &decoded).unwrap();
    assert_eq!(
        result.declaration_sha256,
        canonical_sha256(&json!(declared))
    );
    assert_eq!(result.cells.len(), 8);
    for cell in result.cells {
        assert_eq!(cell.selection.rows.len(), 3);
        assert_eq!(cell.selection.selected_round, None);
        for row in cell.selection.rows {
            assert!(row.no_method.is_some());
            assert!(row.successes.is_none());
            assert!(row.development_usage.is_none());
            assert!(!row.eligible);
        }
    }
}

#[test]
fn differing_family_refs_and_numeric_ceilings_are_valid_but_cross_arm_drift_is_not() {
    let records = trials();
    let good = assess_cohort(&declaration(&records), &records).unwrap();
    assert_eq!(good.cells.len(), 8);
    // All context/criterion/instance hashes vary across families legitimately.
    // The same family still must keep those identities across both arms.
    for field in 0..3 {
        let mut bad = records.clone();
        for round in 0..3 {
            let plan = &mut trial(&mut bad[0], round).plan;
            match field {
                0 => plan.context.generator.sha256 = "f".repeat(64),
                1 => plan.context.scoring_rule.version = "foreign".into(),
                _ => plan.development_instances[0].sha256 = "f".repeat(64),
            }
        }
        bad[0] = capture(bad[0].inputs.clone());
        assert!(
            assess_cohort(&declaration(&bad), &bad).is_err(),
            "field {field}"
        );
    }
}

#[test]
fn every_expected_reference_field_and_record_schema_is_bound() {
    let records = trials();
    for field in 0..4 {
        let mut expected = declaration(&records);
        let ExpectedRound::Trial { plan } = &mut expected.selections[0].rounds[0] else {
            panic!()
        };
        match field {
            0 => plan.domain = "foreign".into(),
            1 => plan.schema = "foreign/1".into(),
            2 => plan.version = "foreign".into(),
            _ => plan.sha256 = "f".repeat(64),
        }
        assert!(assess_cohort(&expected, &records).is_err(), "field {field}");
    }
    let mut bad = records;
    bad[0].schema = "genesis-retained-process-selection/foreign".into();
    assert!(assess_cohort(&declaration(&bad), &bad).is_err());
}

#[test]
fn ordered_candidate_dependencies_cannot_change_between_families() {
    let mut records = trials();
    for record in &mut records {
        for round in 0..3 {
            let plan = &mut trial(record, round).plan;
            plan.slots[0].dependencies =
                vec![plan.model_settings.clone(), plan.context.evaluator.clone()];
        }
        *record = capture(record.inputs.clone());
    }
    assess_cohort(&declaration(&records), &records).unwrap();
    trial(&mut records[0], 0).plan.slots[0]
        .dependencies
        .reverse();
    records[0] = capture(records[0].inputs.clone());
    assert!(assess_cohort(&declaration(&records), &records).is_err());
}
