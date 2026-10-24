use genesis_core::development_method::ResourceUsage;
use genesis_forge::method_accounting::*;
use genesis_forge::method_synthesis::*;
#[path = "common/method_accounting_fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn overflow_retains_maximum_lower_bound_and_unknown_other_rows() {
    let generation = generation("used", true);
    let p = trial(&generation);
    let mut scope = scope(std::slice::from_ref(&generation), Some(&p));
    scope.total_grant.wall_time_ms = u64::MAX;
    let mut item = evidence(&p, "candidate");
    item.execution.usage.wall_time_ms = ResourceUsage::Measured { value: u64::MAX };
    item.scoring = None;
    let report = assess_costs(&scope, &[generation], Some((&p, &[item]))).unwrap();
    let total = &report.combined[3];
    assert_eq!(total.known_lower_bound, u64::MAX);
    assert!(total.overflow && total.unknown && total.limit_exceeded);
    assert!(serde_json::to_string(&report)
        .unwrap()
        .contains("genesis-method-cost-report/1"));
}

#[test]
fn all_generation_phases_retain_accounting_without_manufacturing_a_package() {
    use genesis_forge::text_generation::*;
    let original = generation("source", true);
    let mut plan = original.plan().clone();
    plan.operation_id = "prepared".into();
    let prepared = MethodSynthesisSession::new(plan.clone()).unwrap();
    plan.operation_id = "pending".into();
    let mut pending = MethodSynthesisSession::new(plan.clone()).unwrap();
    pending.begin().unwrap();
    plan.operation_id = "unknown".into();
    let mut unknown = MethodSynthesisSession::new(plan.clone()).unwrap();
    unknown.begin().unwrap();
    unknown.recover_interrupted().unwrap();
    plan.operation_id = "failed".into();
    let mut failed = MethodSynthesisSession::new(plan).unwrap();
    let request = failed.begin().unwrap();
    let mut failure = TextCompletion::interrupted(&request).unwrap();
    failure.outcome = TextOutcome::Failed {
        reason: TextFailure::Protocol,
    };
    failure.usage.input_tokens = ResourceUsage::Measured { value: 42 };
    failed.complete(failure).unwrap();
    let sessions = [prepared, pending, unknown, failed];
    let scope = scope(&sessions, None);
    let report = assess_costs(&scope, &sessions, None).unwrap();
    assert_eq!(report.rows.len(), 4);
    assert!(matches!(
        report.rows[0].observation,
        CostObservation::Generation {
            transport: None,
            ..
        }
    ));
    assert!(matches!(
        report.rows[2].observation,
        CostObservation::Generation {
            phase: MethodSynthesisPhase::NeedsReconciliation,
            transport: Some(GenerationTransport::Unobserved),
        }
    ));
    assert_eq!(report.combined[1].known_lower_bound, 42);
    assert!(report.combined[1].unknown);
    assert!(sessions.iter().all(|s| s.package().is_err()));
    assert!(matches!(
        report.rows[3].observation,
        CostObservation::Generation {
            phase: MethodSynthesisPhase::Failed,
            transport: Some(GenerationTransport::Unobserved),
        }
    ));
}

#[test]
fn foreign_duplicate_and_rebound_evidence_cannot_be_counted_as_this_scope() {
    let sessions = [generation("used", true)];
    let p = trial(&sessions[0]);
    let original = scope(&sessions, Some(&p));
    let items = [evidence(&p, "candidate"), evidence(&p, "control")];
    for field in ["plan", "request", "checkpoint"] {
        let mut changed = original.clone();
        match field {
            "plan" => changed.generations[0].plan_sha256 = "b".repeat(64),
            "request" => changed.generations[0].request_sha256 = "b".repeat(64),
            _ => changed.generations[0].checkpoint_sha256 = "b".repeat(64),
        }
        assert!(assess_costs(&changed, &sessions, Some((&p, &items))).is_err());
    }
    assert!(assess_costs(&original, &[sessions[0].clone(), sessions[0].clone()], None).is_err());
    assert!(assess_costs(&original, &[generation("foreign", true)], None).is_err());
    assert!(assess_costs(
        &original,
        &sessions,
        Some((&p, &[evidence(&p, "candidate"), evidence(&p, "candidate")]))
    )
    .is_err());
    for slot in [" padded", "line\nbreak", ""] {
        let mut malformed = original.clone();
        malformed.trial.as_mut().unwrap().slots[0] = slot.into();
        assert!(assess_costs(&malformed, &sessions, None).is_err());
    }
    let mut partial = original.clone();
    partial.trial.as_mut().unwrap().slots.pop();
    assert!(assess_costs(&partial, &sessions, Some((&p, &[]))).is_err());
    let mut wrong_order = original.clone();
    wrong_order.trial.as_mut().unwrap().slots.reverse();
    assert!(assess_costs(&wrong_order, &sessions, Some((&p, &[]))).is_err());
    let mut foreign = p.clone();
    foreign.id = "another-trial".into();
    assert!(assess_costs(&original, &sessions, Some((&foreign, &[]))).is_err());
}

#[test]
fn generated_links_match_exact_packages_and_unresolved_links_remain_explicit() {
    let sessions = [generation("used", true)];
    let p = trial(&sessions[0]);
    let mut scope = scope(&sessions, Some(&p));
    scope.generated_links.push(GeneratedMethodLink {
        org_id: "org_a".into(),
        operation_id: "used".into(),
        slot_id: "candidate".into(),
    });
    assert_eq!(
        assess_costs(&scope, &[], None)
            .unwrap()
            .matched_package_links,
        vec![false]
    );
    let mut substituted = p.clone();
    substituted.slots[0].method.sha256 = "b".repeat(64);
    scope.trial.as_mut().unwrap().plan = substituted.reference().unwrap();
    assert!(assess_costs(&scope, &sessions, Some((&substituted, &[]))).is_err());
}

#[test]
fn execution_overrun_does_not_mask_a_foreign_scoring_context() {
    let sessions = [generation("used", true)];
    let p = trial(&sessions[0]);
    let scope = scope(&sessions, Some(&p));
    let mut item = evidence(&p, "candidate");
    item.execution.usage.wall_time_ms = ResourceUsage::Measured { value: 101 };
    let binding = p.expected_binding("candidate").unwrap();
    let request = p
        .evaluation_request_for_accounting("candidate", &item.execution)
        .unwrap()
        .unwrap();
    let score = item.scoring.as_mut().unwrap();
    score.request_sha256 = request.identity_hash(&item.execution, &binding).unwrap();
    score.request = request;
    score.observed_context.as_mut().unwrap().environment.sha256 = "b".repeat(64);
    assert!(assess_costs(&scope, &sessions, Some((&p, &[item]))).is_err());
}

#[test]
fn a_scoring_overrun_remains_in_costs_and_never_passes_its_old_budget_gate() {
    let sessions = [generation("used", true)];
    let p = trial(&sessions[0]);
    let scope = scope(&sessions, Some(&p));
    let mut item = evidence(&p, "candidate");
    let score = item.scoring.as_mut().unwrap();
    score.usage.simulation_steps = ResourceUsage::Measured { value: 101 };
    assert!(score
        .validate_against(
            &score.request,
            &item.execution,
            &p.expected_binding("candidate").unwrap()
        )
        .is_err());
    let report = assess_costs(&scope, &sessions, Some((&p, &[item]))).unwrap();
    assert_eq!(report.rows[2].individual_limit_exceeded, Some(true));
    assert_eq!(report.scoring[4].known_lower_bound, 101);
    assert!(report.scoring[4].unknown);
    assert!(matches!(
        report.rows[0].observation,
        CostObservation::Generation {
            transport: Some(genesis_forge::text_generation::GenerationTransport::LoopbackFixture),
            ..
        }
    ));
    let mut foreign = evidence(&p, "candidate");
    foreign.execution.usage.wall_time_ms = ResourceUsage::Measured { value: 101 };
    foreign.execution.actual_method.sha256 = "b".repeat(64);
    foreign.scoring = None;
    assert!(assess_costs(&scope, &sessions, Some((&p, &[foreign]))).is_err());
}
