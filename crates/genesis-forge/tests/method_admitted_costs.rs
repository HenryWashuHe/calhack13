use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_accounting::{CostObservation, CostScope, GeneratedMethodLink},
    method_admission::GenerationAdmission,
    method_admitted_costs::assess_admitted_costs,
    method_roster::GenerationRoster,
    method_synthesis::MethodSynthesisSession,
};
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

fn prepared(id: &str) -> MethodSynthesisSession {
    MethodSynthesisSession::new(fixture::generation(id, true).plan().clone()).unwrap()
}
fn journal(sessions: &[MethodSynthesisSession]) -> GenerationAdmission {
    let settings = sessions[0].plan().settings.clone();
    let grant = ResourceGrant {
        model_calls: sessions.len() as u64,
        input_tokens: 0,
        output_tokens: sessions.len() as u64 * u64::from(settings.max_tokens),
        wall_time_ms: 0,
        simulation_steps: 0,
    };
    GenerationAdmission::new(
        GenerationRoster::freeze("stage".into(), settings, grant, sessions).unwrap(),
    )
    .unwrap()
}
fn link(scope: &mut CostScope, slot: &str, operation: &str) {
    scope.generated_links.push(GeneratedMethodLink {
        org_id: "org_a".into(),
        operation_id: operation.into(),
        slot_id: slot.into(),
    });
}

#[test]
fn whole_roster_survives_trial_join_with_separate_grants_and_no_mutation() {
    let initial: Vec<_> = ["used", "rejected", "stranded", "unused", "missing"]
        .into_iter()
        .map(prepared)
        .collect();
    let mut admission = journal(&initial);
    for index in [0, 1, 2, 4] {
        admission.reserve(&initial[index]).unwrap();
    }
    let generated = fixture::generation("used", true);
    let trial = fixture::trial(&generated);
    let evidence = [
        fixture::evidence(&trial, "candidate"),
        fixture::evidence(&trial, "control"),
    ];
    let mut scope = fixture::scope(&initial, Some(&trial));
    // Reuse by two slots still incurs one generation row.
    link(&mut scope, "candidate", "used");
    link(&mut scope, "control", "used");
    let snapshots = [
        generated,
        fixture::generation("rejected", false),
        initial[2].clone(),
        initial[3].clone(),
    ];
    let before = serde_json::to_vec(&(&admission, &scope, &snapshots)).unwrap();
    let report =
        assess_admitted_costs(&admission, &scope, &snapshots, Some((&trial, &evidence))).unwrap();
    assert_eq!(report.declared_scope_sha256, scope.sha256().unwrap());
    assert_ne!(report.costs.scope_sha256, report.declared_scope_sha256);
    assert_eq!(report.admission.reserved_attempts, 4);
    assert_eq!(report.generation.costs.rows.len(), 5);
    assert_eq!(report.costs.rows.len(), 9);
    assert_eq!(report.costs.matched_package_links, [true, true]);
    assert_eq!(report.costs.combined[0].known_lower_bound, 6);
    assert!(report.costs.combined[0].unknown);
    assert!(matches!(
        report.costs.rows[4].observation,
        CostObservation::Missing
    ));
    assert!(report.generation.costs.combined[1].limit_exceeded);
    assert!(!report.costs.combined[1].limit_exceeded);
    assert_eq!(report.generation.costs.combined[1].known_lower_bound, 2);
    let value = serde_json::to_value(&report).unwrap();
    assert!(!value.to_string().contains("print(1)"));
    assert_eq!(
        before,
        serde_json::to_vec(&(&admission, &scope, &snapshots)).unwrap()
    );
    let reversed: Vec<_> = snapshots.into_iter().rev().collect();
    assert_eq!(
        value,
        serde_json::to_value(
            assess_admitted_costs(&admission, &scope, &reversed, Some((&trial, &evidence)))
                .unwrap()
        )
        .unwrap()
    );
}

#[test]
fn declared_scope_cannot_drop_reorder_or_replace_original_attempts() {
    let initial = [prepared("one"), prepared("two")];
    let admission = journal(&initial);
    let scope = fixture::scope(&initial, None);
    for change in 0..8 {
        let mut altered = scope.clone();
        match change {
            0 => {
                altered.generations.pop();
            }
            1 => altered.generations.reverse(),
            2 => altered.generations[0].org_id = "other".into(),
            3 => altered.generations[0].operation_id = "other".into(),
            4 => altered.generations[0].plan_sha256 = "a".repeat(64),
            5 => altered.generations[0].request_sha256 = "a".repeat(64),
            6 => {
                altered.generations[0].checkpoint_sha256 =
                    fixture::generation("one", true).checkpoint_sha256().into()
            }
            _ => altered.generations.push(altered.generations[0].clone()),
        }
        assert!(
            assess_admitted_costs(&admission, &altered, &[], None).is_err(),
            "change {change}"
        );
    }
}

#[test]
fn advanced_unreserved_snapshots_are_rejected_but_unused_or_missing_is_unknown() {
    let initial = [prepared("one")];
    let mut admission = journal(&initial);
    let scope = fixture::scope(&initial, None);
    let mut pending = initial[0].clone();
    pending.begin().unwrap();
    let mut interrupted = pending.clone();
    interrupted.recover_interrupted().unwrap();
    for advanced in [
        pending,
        interrupted,
        fixture::generation("one", true),
        fixture::generation("one", false),
    ] {
        assert!(
            assess_admitted_costs(&admission, &scope, std::slice::from_ref(&advanced), None)
                .is_err()
        );
    }
    for reserved in [false, true] {
        if reserved {
            admission.reserve(&initial[0]).unwrap();
        }
        for snapshots in [&initial[..], &[][..]] {
            let report = assess_admitted_costs(&admission, &scope, snapshots, None).unwrap();
            assert!(report.costs.combined.iter().all(|t| t.unknown));
            assert_eq!(report.costs.combined[0].known_lower_bound, 0);
            assert_eq!(report.admission.reserved_attempts, u64::from(reserved));
        }
    }
    assert!(assess_admitted_costs(
        &admission,
        &scope,
        &[initial[0].clone(), initial[0].clone()],
        None
    )
    .is_err());
    assert!(assess_admitted_costs(&admission, &scope, &[prepared("foreign")], None).is_err());
    let mut substituted = initial[0].plan().clone();
    substituted.instruction.push_str(" substituted");
    assert!(assess_admitted_costs(
        &admission,
        &scope,
        &[MethodSynthesisSession::new(substituted).unwrap()],
        None
    )
    .is_err());
}

#[test]
fn trial_and_package_guards_remain_and_missing_trial_retains_all_rows() {
    let initial = [prepared("one")];
    let mut admission = journal(&initial);
    admission.reserve(&initial[0]).unwrap();
    let generated = fixture::generation("one", true);
    let trial = fixture::trial(&generated);
    let mut scope = fixture::scope(&initial, Some(&trial));
    link(&mut scope, "candidate", "one");
    let report =
        assess_admitted_costs(&admission, &scope, std::slice::from_ref(&generated), None).unwrap();
    assert_eq!(report.costs.rows.len(), 5);
    assert_eq!(report.costs.matched_package_links, [false]);
    assert!(report.costs.rows[1..]
        .iter()
        .all(|r| matches!(r.observation, CostObservation::Missing)));
    let mut changed = trial.clone();
    changed.slots[0].method.sha256 = "b".repeat(64);
    assert!(assess_admitted_costs(
        &admission,
        &scope,
        std::slice::from_ref(&generated),
        Some((&changed, &[]))
    )
    .is_err());
    // Even a declared changed trial cannot claim the original generated package.
    scope.trial.as_mut().unwrap().plan = changed.reference().unwrap();
    assert!(
        assess_admitted_costs(&admission, &scope, &[generated], Some((&changed, &[]))).is_err()
    );
    let mut scope = fixture::scope(&initial, Some(&trial));
    scope.trial.as_mut().unwrap().slots.reverse();
    assert!(assess_admitted_costs(&admission, &scope, &[], Some((&trial, &[]))).is_err());
}
