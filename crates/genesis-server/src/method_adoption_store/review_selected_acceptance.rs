//! Independent source-only probes. Only retained JSON and an in-memory kernel
//! snapshot are inspected; no method, evaluator or provider is executed.
use super::*;
use crate::method_adoption::{assess_selected, Acceptance};
use genesis_core::development_trial::TrialPlan;

#[test]
fn review_a_different_retained_acceptance_plan_can_authorize_the_requirement() {
    let f = Fixture::new();
    let expected: TrialPlan = serde_json::from_slice(
        &std::fs::read(
            f.records
                .receiver
                .records_root
                .join("vs-random-search/plan.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let expected_ref = expected.reference().unwrap();
    let expected_result = assess_selected(
        &f.records.receiver,
        &OrgId::new("org_example_b"),
        &f.records.selected(),
        "vs-random-search",
    )
    .unwrap();
    assert!(matches!(
        expected_result.acceptance,
        Acceptance::Rejected { .. }
    ));

    let mut state = f.store.current_state().unwrap();
    let revision = state.revision;
    // This is a characterization of the missing typed policy binding, not a
    // request to parse English. The receiver's intent names its chosen plan,
    // but create_method_adoption has no trusted policy input with which to
    // enforce it; the caller-selected trial can instead use the weaker control.
    state
        .requirements
        .get_mut(&f.intent.requirement_id)
        .unwrap()
        .acceptance = format!(
        "Use only the receiver's random-search acceptance plan {}",
        expected_ref.sha256
    );
    state.revision += 1;
    f.store
        .commit_state(
            revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();

    let (record, _) = f.create(&f.intent).unwrap();
    let MethodAdoptionOutcome::Adopted { evidence } = &record.outcome else {
        panic!("expected characterization of the currently accepted alternative");
    };
    assert_ne!(evidence.plan, expected_ref);
    assert_eq!(evidence.public_task, expected.context.public_task);
    assert_eq!(record.requirement_id, f.intent.requirement_id);
    assert!(record
        .authorize(&f.store.current_state().unwrap(), f.now)
        .is_ok());
}

#[test]
fn review_exact_method_pin_cannot_change_before_authorization() {
    let f = Fixture::new();
    let (mut record, _) = f.create(&f.intent).unwrap();
    let state = f.store.current_state().unwrap();
    assert!(record.authorize(&state, f.now).is_ok());
    // The returned type exposes this field publicly. No database write,
    // deserialization or caller-authored acceptance evidence is involved.
    record.reference.version = "unaccepted-next-version".into();
    record.reference.sha256 = "1".repeat(64);
    assert!(record.reference.validate().is_ok());
    assert!(
        record.authorize(&state, f.now).is_err(),
        "authorization returned a current grant for a reference never assessed"
    );
}

#[test]
fn review_accepted_variant_must_retain_an_accepted_verdict() {
    let f = Fixture::new();
    let (mut record, _) = f.create(&f.intent).unwrap();
    if let MethodAdoptionOutcome::Adopted { evidence } = &mut record.outcome {
        evidence.acceptance = Acceptance::Rejected {
            reason: "receiver rejected".into(),
        };
    } else {
        panic!("fixture must begin accepted");
    }
    assert!(
        record
            .authorize(&f.store.current_state().unwrap(), f.now)
            .is_err(),
        "authorization trusted the adopted label despite its rejected evidence"
    );
}
