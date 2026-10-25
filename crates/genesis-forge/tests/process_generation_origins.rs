//! Inert records only: fixtures are never executed or sent to a provider.
use genesis_forge::{
    method_synthesis::*, process_generation_origins::*, process_selection::*,
    process_selection_record::*,
};
#[allow(dead_code)]
#[path = "common/process_origins.rs"]
mod fixture;

#[test]
fn six_original_attempts_bind_all_twenty_four_positions() {
    let (mut origins, mut selections) = fixture::fixture();
    origins.positions.reverse();
    selections.reverse();
    let report =
        assess_generation_origins(&origins, &fixture::declaration(&selections), &selections)
            .unwrap();
    assert_eq!(report["positions"].as_array().unwrap().len(), 6);
    assert_eq!(report["unverified"].as_array().unwrap().len(), 6);
    assert!(report["positions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["method"].is_object()));
    assert!(!report.to_string().contains("inert source fixture"));
}

#[test]
fn repinned_cohort_cannot_substitute_or_drop_a_generated_method() {
    for absent in [false, true] {
        let (origins, mut selections) = fixture::fixture();
        for selection in selections.iter_mut().filter(|s| s.inputs.arm == Arm::Fresh) {
            if absent {
                selection.inputs.rounds[0] = RoundInput::NoMethod {
                    reason: "omitted".into(),
                };
            } else if let RoundInput::Trial { input } = &mut selection.inputs.rounds[0] {
                input.plan.slots[0].method.id.push_str("-foreign");
            }
            *selection = RetainedSelection::capture(selection.inputs.clone()).unwrap();
        }
        assert!(assess_generation_origins(
            &origins,
            &fixture::declaration(&selections),
            &selections
        )
        .is_err());
    }
}

#[test]
fn source_settings_inputs_and_initial_identity_are_checked_even_after_rebinding() {
    for mutation in 0..5 {
        let (mut origins, mut selections) = fixture::fixture();
        let mut plan = origins.positions[3]
            .snapshot
            .as_ref()
            .unwrap()
            .plan()
            .clone();
        match mutation {
            0 => plan.settings.max_tokens += 1,
            1 => plan.instruction.push_str(" use earlier history"),
            2 => plan.public_inputs.push(plan.public_task.clone()),
            3 => origins.positions[3].prepared_checkpoint_sha256 = Some("f".repeat(64)),
            _ => origins.positions[3] = origins.positions[4].clone(),
        }
        if mutation < 3 {
            let snapshot = fixture::complete(plan);
            origins.positions[3].prepared_checkpoint_sha256 =
                Some(snapshot.prepared_checkpoint_sha256().unwrap());
            origins.positions[3].snapshot = Some(snapshot);
            fixture::bind(&origins, &mut selections);
        }
        assert!(
            assess_generation_origins(&origins, &fixture::declaration(&selections), &selections)
                .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn pending_and_missing_attempts_remain_absent_and_unknown() {
    let (mut origins, mut selections) = fixture::fixture();
    for index in 0..6 {
        let original = origins.positions[index].snapshot.take().unwrap();
        origins.positions[index].revision = None;
        if index == 0 || index >= 3 {
            origins.positions[index].snapshot =
                Some(MethodSynthesisSession::new(original.plan().clone()).unwrap());
        } else {
            origins.positions[index].prepared_checkpoint_sha256 = None;
        }
        origins.positions[index].no_method_reason = Some(
            if index == 1 || index == 2 {
                "no_parent"
            } else {
                "pending"
            }
            .into(),
        );
    }
    fixture::bind(&origins, &mut selections);
    let report =
        assess_generation_origins(&origins, &fixture::declaration(&selections), &selections)
            .unwrap();
    assert!(report["positions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["method"].is_null() && r["usage"].is_null()));
}

#[test]
fn workflow_cannot_skip_an_immediate_generated_parent() {
    let (mut origins, selections) = fixture::fixture();
    origins.positions[1].snapshot = None;
    origins.positions[1].revision = None;
    origins.positions[1].no_method_reason = Some("no_parent".into());
    // Even if the trial roster is made consistent with the claimed absence,
    // round three still cannot use the erased round two as its parent.
    let mut selections = selections;
    fixture::bind(&origins, &mut selections);
    assert!(
        assess_generation_origins(&origins, &fixture::declaration(&selections), &selections)
            .is_err()
    );
}
