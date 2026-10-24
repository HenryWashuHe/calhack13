use genesis_core::development_evaluation::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::*;
use serde_json::json;

fn reference(id: &str) -> ExactContentRef {
    serde_json::from_value(json!({"domain":"test","schema":"test/1","id":id,
        "version":"1","sha256":"a".repeat(64)}))
    .unwrap()
}
fn plan() -> TrialPlan {
    let grant = json!({"model_calls":1,"input_tokens":10,"output_tokens":10,
        "wall_time_ms":100,"simulation_steps":100});
    let slots: Vec<_> = ["candidate", "control"]
        .into_iter()
        .map(|id| {
            json!({
        "slot_id":id,"scoring_request_id":id,"org_id":"org_a","run_id":id,"attempt_id":id,
        "method":reference(id),"dependencies":[],"purpose":{"kind":"acceptance","role":id}})
        })
        .collect();
    serde_json::from_value(
        json!({"id":"cost-test","version":"1","criterion":reference("criterion"),
        "development_instances":[reference("instance")],"scoring_instances":{"kind":"development"},
        "context":{"public_task":reference("task"),"evaluator":reference("evaluator"),
            "environment":reference("scoring-env"),"generator":reference("generator"),
            "instance_set":reference("instances"),"design_schema":reference("design"),
            "metric_schema":reference("metrics"),"scoring_rule":reference("rule")},
        "model_settings":reference("model"),"development_environment":reference("exec-env"),
        "development_grant":grant,"scoring_grant":grant,"slots":slots}),
    )
    .unwrap()
}
fn execution(p: &TrialPlan) -> ExecutionReceipt {
    let binding = p.expected_binding("candidate").unwrap();
    let measured = json!({"kind":"measured","value":1});
    serde_json::from_value(json!({"binding":binding,"binding_sha256":binding.canonical_hash().unwrap(),
        "actual_method":binding.method,"actual_environment":binding.environment,"outcome":"output_generated",
        "outputs":[reference("output")],"transcript":reference("trace"),"usage":{"model_calls":measured,
        "input_tokens":measured,"output_tokens":measured,"wall_time_ms":measured,"simulation_steps":measured}})).unwrap()
}
fn evaluation(p: &TrialPlan, e: &ExecutionReceipt) -> EvaluationReceipt {
    let binding = p.expected_binding("candidate").unwrap();
    let request = p
        .evaluation_request_for_accounting("candidate", e)
        .unwrap()
        .unwrap();
    EvaluationReceipt {
        request_sha256: request.identity_hash(e, &binding).unwrap(),
        observed_context: Some(request.context.clone()),
        request,
        outcome: EvaluationOutcome::Scored {
            metrics: reference("metrics"),
        },
        trace: reference("scoring-trace"),
        usage: e.usage.clone(),
    }
}

#[test]
fn accounting_identity_does_not_change_existing_valid_hashes_or_requests() {
    let p = plan();
    let b = p.expected_binding("candidate").unwrap();
    let e = execution(&p);
    let r = evaluation(&p, &e);
    assert_eq!(
        execution_receipt_hash(&e, &b).unwrap(),
        execution_receipt_identity_hash(&e, &b).unwrap()
    );
    assert_eq!(
        p.evaluation_request("candidate", &e).unwrap(),
        p.evaluation_request_for_accounting("candidate", &e)
            .unwrap()
    );
    assert_eq!(
        r.request.canonical_hash(&e, &b).unwrap(),
        r.request.identity_hash(&e, &b).unwrap()
    );
    e.validate_identity_against(&b).unwrap();
    r.validate_identity_against(&r.request, &e, &b).unwrap();
    assert_eq!(
        e.validate_against(&b).unwrap(),
        BudgetAssessment::WithinLimits
    );
    assert_eq!(
        r.validate_against(&r.request, &e, &b).unwrap().scoring,
        BudgetAssessment::WithinLimits
    );
}

#[test]
fn overruns_remain_bound_evidence_but_never_pass_legacy_budget_gates() {
    let p = plan();
    let b = p.expected_binding("candidate").unwrap();
    let mut e = execution(&p);
    e.usage.model_calls = ResourceUsage::Measured { value: 2 };
    let original = serde_json::to_vec(&e).unwrap();
    let r = evaluation(&p, &e);
    e.validate_identity_against(&b).unwrap();
    r.validate_identity_against(&r.request, &e, &b).unwrap();
    assert!(matches!(
        e.validate_against(&b),
        Err(ContractError::BudgetExceeded("model_calls"))
    ));
    assert!(execution_receipt_hash(&e, &b).is_err());
    assert!(p.evaluation_request("candidate", &e).is_err());
    assert!(r.request.canonical_hash(&e, &b).is_err());
    assert!(r.validate_against(&r.request, &e, &b).is_err());
    assert_eq!(serde_json::to_vec(&e).unwrap(), original);
    let e = execution(&p);
    let mut r = evaluation(&p, &e);
    r.usage.simulation_steps = ResourceUsage::Measured { value: 101 };
    r.validate_identity_against(&r.request, &e, &b).unwrap();
    assert!(r.validate_against(&r.request, &e, &b).is_err());
}

#[test]
fn an_early_overrun_cannot_hide_foreign_identity_or_invalid_later_usage() {
    let p = plan();
    let b = p.expected_binding("candidate").unwrap();
    let mut e = execution(&p);
    e.usage.model_calls = ResourceUsage::Measured { value: 2 };
    let r = evaluation(&p, &e);
    let mut foreign = r.clone();
    foreign.observed_context.as_mut().unwrap().environment = reference("foreign");
    assert!(matches!(
        foreign.validate_identity_against(&r.request, &e, &b),
        Err(EvaluationError::ContextMismatch)
    ));
    let mut foreign = r.clone();
    foreign.request_sha256 = "b".repeat(64);
    assert!(foreign
        .validate_identity_against(&r.request, &e, &b)
        .is_err());
    let mut foreign = e.clone();
    foreign.actual_method = reference("foreign");
    assert!(foreign.validate_identity_against(&b).is_err());
    assert!(p.evaluation_request_for_accounting("control", &e).is_err());
    for reason in ["", " ", " leading", "new\nline"] {
        let mut bad = e.clone();
        bad.usage.simulation_steps = ResourceUsage::Unknown {
            reason: reason.into(),
        };
        assert!(bad.validate_identity_against(&b).is_err());
        assert!(execution_receipt_identity_hash(&bad, &b).is_err());
        let mut bad = r.clone();
        bad.usage.simulation_steps = ResourceUsage::Unknown {
            reason: reason.into(),
        };
        assert!(bad.validate_identity_against(&r.request, &e, &b).is_err());
    }
}

#[test]
fn failed_no_output_and_unknown_usage_are_not_success_or_free_scoring() {
    let p = plan();
    let b = p.expected_binding("candidate").unwrap();
    let mut e = execution(&p);
    e.outcome = ExecutionOutcome::Failed;
    e.outputs.clear();
    e.usage.wall_time_ms = ResourceUsage::Measured { value: 101 };
    e.usage.output_tokens = ResourceUsage::Unknown {
        reason: "counter missing".into(),
    };
    e.validate_identity_against(&b).unwrap();
    assert!(p
        .evaluation_request_for_accounting("candidate", &e)
        .unwrap()
        .is_none());
    assert!(p.evaluation_request("candidate", &e).is_err());
    e.outcome = ExecutionOutcome::OutputGenerated;
    assert!(e.validate_identity_against(&b).is_err());
}
