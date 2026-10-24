use genesis_core::development_evaluation::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::*;
use serde_json::json;

fn reference(id: &str) -> ExactContentRef {
    serde_json::from_value(
        json!({"domain":"test","schema":"test/1","id":id,"version":"v1","sha256":"a".repeat(64)}),
    )
    .unwrap()
}
fn plan() -> TrialPlan {
    let mut slots = vec![];
    for trial in ["first", "second"] {
        for arm in ["control", "network"] {
            let id = format!("{trial}-{arm}");
            slots.push(json!({"slot_id":id,"scoring_request_id":id,"org_id":arm,"run_id":id,"attempt_id":id,
                "method":reference(arm),"dependencies":[],"purpose":{"kind":"comparison","trial_id":trial,"arm_id":arm}}));
        }
    }
    let grant = json!({"model_calls":1,"input_tokens":10,"output_tokens":10,"wall_time_ms":100,"simulation_steps":100});
    serde_json::from_value(json!({"id":"batch","version":"v1","criterion":reference("criterion"),
        "development_instances":[reference("dev-1")],"scoring_instances":{"kind":"hidden_commitment","protocol":reference("commit-protocol")},
        "context":{"public_task":reference("task"),"evaluator":reference("evaluator"),"environment":reference("scoring-env"),
            "generator":reference("generator"),"instance_set":reference("commitment"),"design_schema":reference("design-schema"),
            "metric_schema":reference("metric-schema"),"scoring_rule":reference("scoring-rule")},
        "model_settings":reference("model"),"development_environment":reference("dev-env"),
        "development_grant":grant,"scoring_grant":grant,"slots":slots})).unwrap()
}
fn execution(plan: &TrialPlan, index: usize, outcome: ExecutionOutcome) -> ExecutionReceipt {
    let binding = plan.expected_binding(&plan.slots[index].slot_id).unwrap();
    let measured = json!({"kind":"measured","value":0});
    serde_json::from_value(json!({"binding":binding,"binding_sha256":binding.canonical_hash().unwrap(),
        "actual_method":binding.method,"actual_environment":binding.environment,"outcome":outcome,
        "outputs":[],"transcript":reference("transcript"),"usage":{"model_calls":measured,
        "input_tokens":measured,"output_tokens":measured,"wall_time_ms":measured,"simulation_steps":measured}})).unwrap()
}
fn evidence(plan: &TrialPlan, index: usize, outcome: ExecutionOutcome) -> TrialEvidence {
    TrialEvidence {
        slot_id: plan.slots[index].slot_id.clone(),
        execution: execution(plan, index, outcome),
        scoring: None,
    }
}

#[test]
fn every_declared_slot_survives_missing_failed_interrupted_and_unknown_runs() {
    let p = plan();
    let cases = vec![
        evidence(&p, 2, ExecutionOutcome::Unknown),
        evidence(&p, 0, ExecutionOutcome::Failed),
        evidence(&p, 1, ExecutionOutcome::Interrupted),
    ];
    let rows = assess_trials(&p, &cases).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter().map(|r| r.slot_id.clone()).collect::<Vec<_>>(),
        p.slots
            .iter()
            .map(|s| s.slot_id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        rows.iter()
            .map(|r| r.development_outcome)
            .collect::<Vec<_>>(),
        vec![
            Some(ExecutionOutcome::Failed),
            Some(ExecutionOutcome::Interrupted),
            Some(ExecutionOutcome::Unknown),
            None
        ]
    );
    assert!(rows.iter().all(|r| r.output.is_none()
        && r.scoring_status == RequestStatus::Missing
        && r.scoring_accounting.is_none()));
    assert_eq!(rows[3].development_accounting, None);
}

#[test]
fn changed_criterion_commitment_grants_or_task_cannot_reuse_frozen_execution() {
    let p = plan();
    for path in [
        "criterion",
        "context/instance_set",
        "context/public_task",
        "model_settings",
        "development_environment",
    ] {
        let mut wire = serde_json::to_value(&p).unwrap();
        wire.pointer_mut(&format!("/{path}/sha256"))
            .unwrap()
            .clone_from(&json!("b".repeat(64)));
        let changed: TrialPlan = serde_json::from_value(wire).unwrap();
        assert_ne!(p.reference().unwrap(), changed.reference().unwrap());
        assert!(assess_trials(&changed, &[evidence(&p, 0, ExecutionOutcome::Failed)]).is_err());
    }
    let mut changed = p.clone();
    changed.development_grant.model_calls += 1;
    assert!(assess_trials(&changed, &[evidence(&p, 0, ExecutionOutcome::Failed)]).is_err());
    changed = p.clone();
    changed.scoring_grant.simulation_steps += 1;
    assert!(assess_trials(&changed, &[evidence(&p, 0, ExecutionOutcome::Failed)]).is_err());
}

#[test]
fn scoring_is_reconstructed_from_the_plan_and_preserves_failed_development_and_unknown_usage() {
    let p = plan();
    let mut item = evidence(&p, 0, ExecutionOutcome::Failed);
    item.execution.outputs.push(reference("design"));
    item.execution.usage.model_calls = ResourceUsage::Unknown {
        reason: "worker lost".into(),
    };
    let b = p.expected_binding(&item.slot_id).unwrap();
    let request = p
        .evaluation_request(&item.slot_id, &item.execution)
        .unwrap()
        .unwrap();
    let receipt = EvaluationReceipt {
        request_sha256: request.canonical_hash(&item.execution, &b).unwrap(),
        observed_context: Some(request.context.clone()),
        request,
        outcome: EvaluationOutcome::Scored {
            metrics: reference("metrics"),
        },
        trace: reference("trace"),
        usage: item.execution.usage.clone(),
    };
    item.scoring = Some(receipt.clone());
    let rows = assess_trials(&p, &[item]).unwrap();
    assert_eq!(rows[0].development_outcome, Some(ExecutionOutcome::Failed));
    assert_eq!(
        rows[0].development_accounting,
        Some(BudgetAssessment::Unknown)
    );
    assert_eq!(rows[0].scoring_status, RequestStatus::Scored);
    assert_eq!(rows[0].scoring_accounting, Some(BudgetAssessment::Unknown));
    let mut altered = evidence(&p, 0, ExecutionOutcome::Failed);
    altered.execution.outputs.push(reference("design"));
    let mut substituted = receipt;
    substituted.request.context.instance_set = reference("easier-instances");
    substituted.request.execution_sha256 = execution_receipt_hash(&altered.execution, &b).unwrap();
    substituted.request_sha256 = substituted
        .request
        .canonical_hash(&altered.execution, &b)
        .unwrap();
    substituted.observed_context = Some(substituted.request.context.clone());
    altered.scoring = Some(substituted);
    assert_eq!(
        assess_trials(&p, &[altered]),
        Err(EvaluationError::BindingMismatch)
    );
}

#[test]
fn one_attempt_and_one_retained_output_cannot_be_replaced_by_a_favorable_subset() {
    let p = plan();
    assert_eq!(
        assess_trials(
            &p,
            &[
                evidence(&p, 0, ExecutionOutcome::Failed),
                evidence(&p, 0, ExecutionOutcome::Failed)
            ]
        ),
        Err(EvaluationError::Duplicate)
    );
    let mut retry = evidence(&p, 0, ExecutionOutcome::Failed);
    retry.execution.binding.attempt_id = "new-attempt".into();
    retry.execution.binding_sha256 = retry.execution.binding.canonical_hash().unwrap();
    assert!(assess_trials(&p, &[retry]).is_err());
    let mut many = evidence(&p, 0, ExecutionOutcome::OutputGenerated);
    many.execution.outputs = vec![reference("first"), reference("second")];
    assert_eq!(
        assess_trials(&p, &[many]),
        Err(EvaluationError::Invalid("multiple outputs unsupported"))
    );
    let mut extra = evidence(&p, 0, ExecutionOutcome::Failed);
    extra.slot_id = "unplanned".into();
    assert_eq!(
        assess_trials(&p, &[extra]),
        Err(EvaluationError::Unexpected)
    );
}

#[test]
fn comparison_pairing_and_stable_arm_identity_are_required() {
    let p = plan();
    let mut unpaired = p.clone();
    unpaired.slots.pop();
    assert!(unpaired.validate().is_err());
    for field in ["method", "dependencies", "org_id"] {
        let mut changed = p.clone();
        match field {
            "method" => changed.slots[2].method = reference("replacement"),
            "dependencies" => changed.slots[2].dependencies.push(reference("extra")),
            _ => changed.slots[2].org_id = "another-org".into(),
        }
        assert_eq!(
            changed.validate(),
            Err(EvaluationError::Invalid("changing comparison treatment"))
        );
    }
    for field in ["slot_id", "scoring_request_id", "run_id", "attempt_id"] {
        let mut wire = serde_json::to_value(&p).unwrap();
        wire["slots"][2][field] = wire["slots"][0][field].clone();
        assert_eq!(
            serde_json::from_value::<TrialPlan>(wire)
                .unwrap()
                .validate(),
            Err(EvaluationError::Duplicate)
        );
    }
}

#[test]
fn local_acceptance_requires_both_roles_equal_grants_and_development_instances() {
    let mut p = plan();
    p.slots.truncate(2);
    p.scoring_instances = ScoringInstances::Development {};
    p.slots[0].purpose = TrialPurpose::Acceptance {
        role: AcceptanceRole::Candidate,
    };
    p.slots[1].purpose = TrialPurpose::Acceptance {
        role: AcceptanceRole::Control,
    };
    assert!(p.validate().is_err()); // separate orgs cannot stand in for local acceptance
    p.slots[1].org_id = p.slots[0].org_id.clone();
    p.validate().unwrap();
    assert_eq!(
        p.expected_binding(&p.slots[0].slot_id).unwrap().budget,
        p.expected_binding(&p.slots[1].slot_id).unwrap().budget
    );
    p.slots.pop();
    assert!(p.validate().is_err());
}

#[test]
fn malformed_and_conflicting_plan_inputs_do_not_become_identities() {
    let p = plan();
    let mut wire = serde_json::to_value(&p).unwrap();
    wire["retry"] = json!(true);
    assert!(serde_json::from_value::<TrialPlan>(wire).is_err());
    let mut empty = p.clone();
    empty.slots.clear();
    assert!(empty.reference().is_err());
    let mut conflict = p.clone();
    let mut instance = conflict.development_instances[0].clone();
    instance.sha256 = "b".repeat(64);
    conflict.development_instances.push(instance);
    assert_eq!(conflict.validate(), Err(EvaluationError::Duplicate));
    let mut floating = p;
    floating.version = "latest".into();
    assert!(floating.reference().is_err());
}
