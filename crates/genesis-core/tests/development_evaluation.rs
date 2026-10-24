use genesis_core::development_evaluation::*;
use genesis_core::development_method::*;
use serde_json::{json, Value};

fn reference(id: &str) -> ExactContentRef {
    ExactContentRef {
        domain: "test".into(),
        schema: "test/1".into(),
        id: id.into(),
        version: "v1".into(),
        sha256: "a".repeat(64),
    }
}

fn grant() -> ResourceGrant {
    ResourceGrant {
        model_calls: 1,
        input_tokens: 100,
        output_tokens: 50,
        wall_time_ms: 1000,
        simulation_steps: 200,
    }
}

fn usage() -> Usage {
    Usage {
        model_calls: ResourceUsage::Measured { value: 1 },
        input_tokens: ResourceUsage::Measured { value: 100 },
        output_tokens: ResourceUsage::Measured { value: 50 },
        wall_time_ms: ResourceUsage::Measured { value: 1000 },
        simulation_steps: ResourceUsage::Measured { value: 200 },
    }
}

fn fixture() -> (DevelopmentRunBinding, ExecutionReceipt, EvaluationRequest) {
    let binding = DevelopmentRunBinding {
        org_id: "org_a".into(),
        run_id: "run_1".into(),
        attempt_id: "attempt_1".into(),
        method: reference("method"),
        dependencies: vec![],
        public_task: reference("task"),
        evaluator: reference("evaluator"),
        model_settings: reference("model-settings"),
        environment: reference("development-env"),
        purpose: ExecutionPurpose::Comparison {
            plan: reference("plan"),
            trial_id: "trial-1".into(),
            arm_id: "method".into(),
        },
        budget: grant(),
    };
    let execution = ExecutionReceipt {
        binding: binding.clone(),
        binding_sha256: binding.canonical_hash().unwrap(),
        actual_method: binding.method.clone(),
        actual_environment: binding.environment.clone(),
        outcome: ExecutionOutcome::OutputGenerated,
        outputs: vec![reference("design")],
        transcript: reference("development-transcript"),
        usage: usage(),
    };
    let request = EvaluationRequest {
        request_id: "request-1".into(),
        execution_sha256: execution_receipt_hash(&execution, &binding).unwrap(),
        output: execution.outputs[0].clone(),
        context: ScoringContext {
            public_task: binding.public_task.clone(),
            evaluator: binding.evaluator.clone(),
            environment: reference("scoring-env"),
            generator: reference("generator"),
            instance_set: reference("private-instances"),
            design_schema: reference("design-schema"),
            metric_schema: reference("metric-schema"),
            scoring_rule: reference("scoring-rule"),
        },
        purpose: binding.purpose.clone(),
        budget: grant(),
    };
    (binding, execution, request)
}

fn scored(
    request: &EvaluationRequest,
    execution: &ExecutionReceipt,
    binding: &DevelopmentRunBinding,
) -> EvaluationReceipt {
    EvaluationReceipt {
        request: request.clone(),
        request_sha256: request.canonical_hash(execution, binding).unwrap(),
        observed_context: Some(request.context.clone()),
        outcome: EvaluationOutcome::Scored {
            metrics: reference("metrics"),
        },
        trace: reference("evaluation-trace"),
        usage: usage(),
    }
}

fn expected<'a>(
    request: &'a EvaluationRequest,
    execution: &'a ExecutionReceipt,
    binding: &'a DevelopmentRunBinding,
) -> EvaluationExpectation<'a> {
    EvaluationExpectation {
        request,
        execution,
        binding,
    }
}

#[test]
fn selected_output_is_bound_to_the_entire_execution_not_only_the_run_binding() {
    let (binding, execution, request) = fixture();
    assert_eq!(
        request.validate_against(&execution, &binding).unwrap(),
        BudgetAssessment::WithinLimits
    );
    for field in ["outputs", "transcript", "usage", "outcome"] {
        let mut wire = serde_json::to_value(&execution).unwrap();
        match field {
            "outputs" => wire[field][0]["sha256"] = json!("b".repeat(64)),
            "transcript" => wire[field]["sha256"] = json!("b".repeat(64)),
            "usage" => wire[field]["input_tokens"]["value"] = json!(99),
            "outcome" => wire[field] = json!("interrupted"),
            _ => unreachable!(),
        }
        let changed: ExecutionReceipt = serde_json::from_value(wire).unwrap();
        assert_eq!(changed.binding_sha256, execution.binding_sha256);
        assert!(changed.validate_against(&binding).is_ok());
        assert_ne!(
            execution_receipt_hash(&changed, &binding).unwrap(),
            request.execution_sha256
        );
        assert_eq!(
            request.validate_against(&changed, &binding),
            Err(EvaluationError::BindingMismatch)
        );
    }
    let mut absent_output = request.clone();
    absent_output.output.sha256 = "c".repeat(64);
    assert_eq!(
        absent_output.validate_against(&execution, &binding),
        Err(EvaluationError::OutputNotProduced)
    );
}

#[test]
fn changing_task_evaluator_or_purpose_does_not_reclassify_execution() {
    let (binding, execution, request) = fixture();
    let mut cases = vec![];
    let mut task = request.clone();
    task.context.public_task.sha256 = "b".repeat(64);
    cases.push(task);
    let mut evaluator = request.clone();
    evaluator.context.evaluator.sha256 = "b".repeat(64);
    cases.push(evaluator);
    let mut ordinary = request.clone();
    ordinary.purpose = ExecutionPurpose::Ordinary {};
    cases.push(ordinary);
    let mut acceptance = request;
    acceptance.purpose = ExecutionPurpose::Acceptance {
        plan: reference("plan"),
        role: AcceptanceRole::Candidate,
    };
    cases.push(acceptance);
    for changed in cases {
        assert_eq!(
            changed.validate_against(&execution, &binding),
            Err(EvaluationError::BindingMismatch)
        );
    }
}

#[test]
fn recomputing_a_substituted_request_hash_cannot_replace_the_frozen_request() {
    let (binding, execution, request) = fixture();
    for field in [
        "environment",
        "generator",
        "instance_set",
        "design_schema",
        "metric_schema",
        "scoring_rule",
    ] {
        let mut wire = serde_json::to_value(&request).unwrap();
        wire["context"][field]["sha256"] = json!("b".repeat(64));
        let changed: EvaluationRequest = serde_json::from_value(wire).unwrap();
        let substituted = scored(&changed, &execution, &binding);
        assert!(substituted
            .validate_against(&changed, &execution, &binding)
            .is_ok());
        assert_eq!(
            substituted.validate_against(&request, &execution, &binding),
            Err(EvaluationError::BindingMismatch)
        );
        let mut observed = scored(&request, &execution, &binding);
        observed.observed_context = Some(changed.context);
        assert_eq!(
            observed.validate_against(&request, &execution, &binding),
            Err(EvaluationError::ContextMismatch)
        );
    }
    for field in ["request_id", "budget"] {
        let mut changed = request.clone();
        if field == "request_id" {
            changed.request_id = "another".into();
        } else {
            changed.budget.simulation_steps += 1;
        }
        assert_eq!(
            scored(&changed, &execution, &binding).validate_against(&request, &execution, &binding),
            Err(EvaluationError::BindingMismatch)
        );
    }
    let mut bad_hash = scored(&request, &execution, &binding);
    bad_hash.request_sha256 = "b".repeat(64);
    assert_eq!(
        bad_hash.validate_against(&request, &execution, &binding),
        Err(EvaluationError::BindingMismatch)
    );
}

#[test]
fn scoring_and_invalid_design_require_observed_context_but_transport_failures_do_not() {
    let (binding, execution, request) = fixture();
    for outcome in [
        EvaluationOutcome::Scored {
            metrics: reference("metrics"),
        },
        EvaluationOutcome::Invalid {
            reason: "design failed schema validation".into(),
        },
    ] {
        let mut receipt = scored(&request, &execution, &binding);
        receipt.outcome = outcome;
        assert!(receipt
            .validate_against(&request, &execution, &binding)
            .is_ok());
        receipt.observed_context = None;
        assert_eq!(
            receipt.validate_against(&request, &execution, &binding),
            Err(EvaluationError::Invalid("missing observed scoring context"))
        );
    }
    for outcome in [
        EvaluationOutcome::Failed {
            reason: "worker did not start".into(),
        },
        EvaluationOutcome::Unknown {
            reason: "response lost after send".into(),
        },
    ] {
        let mut receipt = scored(&request, &execution, &binding);
        receipt.outcome = outcome;
        receipt.observed_context = None;
        assert!(receipt
            .validate_against(&request, &execution, &binding)
            .is_ok());
        receipt.trace.sha256 = "bad".into();
        assert!(receipt
            .validate_against(&request, &execution, &binding)
            .is_err());
    }
    let mut receipt = scored(&request, &execution, &binding);
    receipt.outcome = EvaluationOutcome::Unknown {
        reason: "  ".into(),
    };
    assert!(receipt
        .validate_against(&request, &execution, &binding)
        .is_err());
}

#[test]
fn unknown_development_and_scoring_accounting_stay_separate_and_overruns_still_reject() {
    let (binding, mut execution, mut request) = fixture();
    execution.usage.model_calls = ResourceUsage::Unknown {
        reason: "worker telemetry unavailable".into(),
    };
    request.execution_sha256 = execution_receipt_hash(&execution, &binding).unwrap();
    let mut receipt = scored(&request, &execution, &binding);
    assert_eq!(
        receipt
            .validate_against(&request, &execution, &binding)
            .unwrap(),
        EvaluationAccounting {
            development: BudgetAssessment::Unknown,
            scoring: BudgetAssessment::WithinLimits
        }
    );
    receipt.usage.wall_time_ms = ResourceUsage::Unknown {
        reason: "completion timestamp missing".into(),
    };
    assert_eq!(
        receipt
            .validate_against(&request, &execution, &binding)
            .unwrap()
            .scoring,
        BudgetAssessment::Unknown
    );
    receipt.usage.simulation_steps = ResourceUsage::Measured { value: 201 };
    assert_eq!(
        receipt.validate_against(&request, &execution, &binding),
        Err(EvaluationError::Contract(ContractError::BudgetExceeded(
            "simulation_steps"
        )))
    );
}

#[test]
fn roster_retains_missing_failed_invalid_unknown_and_scored_requests_in_declared_order() {
    let (binding, execution, request) = fixture();
    let mut runs = vec![];
    let mut executions = vec![];
    let mut requests = vec![];
    let mut receipts = vec![];
    for n in 0..5 {
        let mut b = binding.clone();
        b.run_id = format!("run-{n}");
        b.purpose = ExecutionPurpose::Comparison {
            plan: reference("plan"),
            trial_id: format!("trial-{n}"),
            arm_id: "method".into(),
        };
        let mut e = execution.clone();
        e.binding = b.clone();
        e.binding_sha256 = b.canonical_hash().unwrap();
        // A scorer can score retained partial output; this does not erase a failed development run.
        if n == 0 {
            e.outcome = ExecutionOutcome::Failed;
        }
        let mut r = request.clone();
        r.request_id = format!("request-{n}");
        r.purpose = b.purpose.clone();
        r.execution_sha256 = execution_receipt_hash(&e, &b).unwrap();
        if n != 4 {
            let mut receipt = scored(&r, &e, &b);
            receipt.outcome = match n {
                0 => EvaluationOutcome::Scored {
                    metrics: reference("metrics"),
                },
                1 => EvaluationOutcome::Invalid {
                    reason: "invalid design".into(),
                },
                2 => EvaluationOutcome::Failed {
                    reason: "worker failure".into(),
                },
                _ => EvaluationOutcome::Unknown {
                    reason: "lost response".into(),
                },
            };
            receipts.push(receipt);
        }
        runs.push(b);
        executions.push(e);
        requests.push(r);
    }
    receipts.reverse();
    let roster: Vec<_> = (0..5)
        .map(|i| expected(&requests[i], &executions[i], &runs[i]))
        .collect();
    let assessed = assess_request_roster(&roster, &receipts).unwrap();
    assert_eq!(
        assessed.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![
            RequestStatus::Scored,
            RequestStatus::Invalid,
            RequestStatus::Failed,
            RequestStatus::Unknown,
            RequestStatus::Missing
        ]
    );
    assert_eq!(assessed[0].development_outcome, ExecutionOutcome::Failed);
    assert_eq!(assessed[4].request_id, "request-4");
    assert_eq!(assessed[4].scoring_accounting, None);
}

#[test]
fn duplicate_extra_and_relabeled_comparison_requests_cannot_change_the_roster() {
    let (binding, execution, request) = fixture();
    let receipt = scored(&request, &execution, &binding);
    assert_eq!(
        assess_request_roster(&[], &[]),
        Err(EvaluationError::Invalid("empty request roster"))
    );
    assert_eq!(
        assess_request_roster(
            &[
                expected(&request, &execution, &binding),
                expected(&request, &execution, &binding)
            ],
            &[]
        ),
        Err(EvaluationError::Duplicate)
    );
    assert_eq!(
        assess_request_roster(
            &[expected(&request, &execution, &binding)],
            &[receipt.clone(), receipt.clone()]
        ),
        Err(EvaluationError::Duplicate)
    );
    let mut another = request.clone();
    another.request_id = "another-request".into();
    assert_eq!(
        assess_request_roster(
            &[expected(&request, &execution, &binding)],
            &[scored(&another, &execution, &binding)]
        ),
        Err(EvaluationError::Unexpected)
    );
    assert_eq!(
        assess_request_roster(
            &[
                expected(&request, &execution, &binding),
                expected(&another, &execution, &binding)
            ],
            &[]
        ),
        Err(EvaluationError::Duplicate)
    );
    let mut tampered = receipt;
    tampered.request.budget.simulation_steps += 1;
    assert_eq!(
        assess_request_roster(&[expected(&request, &execution, &binding)], &[tampered]),
        Err(EvaluationError::BindingMismatch)
    );
}

#[test]
fn no_output_failure_cannot_be_disguised_as_a_scoring_request() {
    let (binding, mut execution, mut request) = fixture();
    execution.outcome = ExecutionOutcome::Failed;
    execution.outputs.clear();
    request.execution_sha256 = execution_receipt_hash(&execution, &binding).unwrap();
    assert_eq!(
        request.validate_against(&execution, &binding),
        Err(EvaluationError::OutputNotProduced)
    );
    // These failures belong in a separate pre-execution trial ledger. An empty
    // scoring roster is not proof of either successful or complete development.
}

#[test]
fn wire_contract_rejects_unknown_fields_and_non_integer_or_negative_budgets() {
    let (binding, execution, request) = fixture();
    let original = serde_json::to_value(scored(&request, &execution, &binding)).unwrap();
    for pointer in ["", "/request", "/request/context", "/outcome"] {
        let mut wire = original.clone();
        wire.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unreviewed".into(), json!(true));
        assert!(
            serde_json::from_value::<EvaluationReceipt>(wire).is_err(),
            "{pointer}"
        );
    }
    for outcome in [
        json!({"kind":"failed","reason":"failure","extra":true}),
        json!({"kind":"unknown","reason":"lost","extra":true}),
        json!({"kind":"invalid","reason":"design","extra":true}),
    ] {
        assert!(serde_json::from_value::<EvaluationOutcome>(outcome).is_err());
    }
    for value in [json!(-1), json!(0.5), Value::Null] {
        let mut wire = serde_json::to_value(&request).unwrap();
        wire["budget"]["model_calls"] = value;
        assert!(serde_json::from_value::<EvaluationRequest>(wire).is_err());
    }
}
