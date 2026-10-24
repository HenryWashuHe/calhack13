use genesis_core::development_method::{ExecutionOutcome, ResourceUsage};
use genesis_forge::method_accounting::*;
use genesis_forge::method_synthesis::MethodSynthesisPhase;
#[path = "common/method_accounting_fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn cross_org_reuse_charges_each_generation_once_including_unused_and_rejected() {
    let generations = [
        generation("used", true),
        generation("unused", true),
        generation("rejected", false),
    ];
    let p = trial(&generations[0]);
    let mut scope = scope(&generations, Some(&p));
    scope.generated_links = p
        .slots
        .iter()
        .map(|s| GeneratedMethodLink {
            org_id: "org_a".into(),
            operation_id: "used".into(),
            slot_id: s.slot_id.clone(),
        })
        .collect();
    let evidence = [evidence(&p, "control"), evidence(&p, "candidate")];
    let report = assess_costs(&scope, &generations, Some((&p, &evidence))).unwrap();
    assert_eq!(report.generation[0].known_lower_bound, 3);
    assert_eq!(report.combined[0].known_lower_bound, 7);
    assert!(!report.combined[0].unknown);
    assert_eq!(report.matched_package_links, vec![true, true]);
    assert!(matches!(
        report.rows[2].observation,
        CostObservation::Generation {
            phase: MethodSynthesisPhase::RejectedSource,
            ..
        }
    ));
    let mut wrong = scope.clone();
    wrong.generations[0].checkpoint_sha256 = "0".repeat(64);
    assert!(assess_costs(&wrong, &generations, Some((&p, &evidence))).is_err());
    wrong = scope.clone();
    wrong.generations.push(wrong.generations[0].clone());
    assert!(assess_costs(&wrong, &generations, None).is_err());
}

#[test]
fn missing_rows_preserve_known_subtotals_and_known_overruns() {
    let generations = [generation("known", false), generation("missing", true)];
    let p = trial(&generations[1]);
    let mut scope = scope(&generations, Some(&p));
    scope.total_grant.model_calls = 0;
    let report = assess_costs(&scope, &generations[..1], None).unwrap();
    assert_eq!(report.rows.len(), 6);
    assert_eq!(report.combined[0].known_lower_bound, 1);
    assert!(report.combined[0].unknown && report.combined[0].limit_exceeded);
    assert!(!report.combined[0].overflow);
    assert!(matches!(
        report.rows[1].observation,
        CostObservation::Missing
    ));
}

#[test]
fn failed_no_output_keeps_its_overrun_and_missing_scoring_unknown() {
    let generation = generation("used", true);
    let p = trial(&generation);
    let scope = scope(std::slice::from_ref(&generation), Some(&p));
    let mut failed = evidence(&p, "candidate");
    failed.execution.outcome = ExecutionOutcome::Failed;
    failed.execution.outputs.clear();
    failed.execution.usage.wall_time_ms = ResourceUsage::Measured { value: 200 };
    failed.scoring = None;
    let report = assess_costs(&scope, &[generation], Some((&p, &[failed]))).unwrap();
    assert_eq!(report.rows[1].individual_limit_exceeded, Some(true));
    assert!(matches!(
        report.rows[1].observation,
        CostObservation::Execution {
            outcome: ExecutionOutcome::Failed,
            has_output: false
        }
    ));
    assert!(matches!(
        report.rows[2].observation,
        CostObservation::Missing
    ));
    assert!(report.combined[3].unknown);
    assert_eq!(report.combined[3].known_lower_bound, 201);
}
