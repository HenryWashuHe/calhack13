//! Pure receipt-boundary regressions. No code execution or provider calls.
use super::*;
use genesis_core::development_evaluation::{EvaluationError, RequestStatus};
use genesis_core::development_verdict::{decide, SlotEvidence};

fn receipt(p: &TrialPlan) -> ExecutionReceipt {
    let binding = p.expected_binding("use").unwrap();
    let zero = ResourceUsage::Measured { value: 0 };
    ExecutionReceipt {
        binding_sha256: binding.canonical_hash().unwrap(),
        actual_method: binding.method.clone(),
        actual_environment: binding.environment.clone(),
        binding,
        outcome: ExecutionOutcome::OutputGenerated,
        outputs: vec![serde_json::from_value(reference("output")).unwrap()],
        transcript: serde_json::from_value(reference("transcript")).unwrap(),
        usage: Usage {
            model_calls: zero.clone(),
            input_tokens: zero.clone(),
            output_tokens: zero.clone(),
            wall_time_ms: zero.clone(),
            simulation_steps: zero,
        },
    }
}

#[test]
fn review_receipt_cannot_cross_any_grant_identity_or_organization() {
    let original = plan(
        vec![slot("use", invocation())],
        json!({"kind":"development"}),
    )
    .unwrap();
    let retained = receipt(&original);
    for field in [
        "adoption_id",
        "requirement_id",
        "role_id",
        "responsibility_id",
        "assignment_id",
        "epoch",
        "org_id",
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        if field == "org_id" {
            value["slots"][0][field] = json!("other-organization");
        } else {
            value["slots"][0]["purpose"]["grant"][field] = if field == "epoch" {
                json!(4)
            } else {
                json!("other-identity")
            };
        }
        let changed: TrialPlan = serde_json::from_value(value).unwrap();
        let expected = changed.expected_binding("use").unwrap();
        assert_eq!(
            retained.validate_against(&expected),
            Err(ContractError::BindingMismatch),
            "{field}"
        );
    }
}

#[test]
fn review_invocation_scoring_cannot_be_relabelled_acceptance() {
    let p = plan(
        vec![slot("use", invocation())],
        json!({"kind":"development"}),
    )
    .unwrap();
    let retained = receipt(&p);
    let request = p.evaluation_request("use", &retained).unwrap().unwrap();
    assert!(matches!(
        request.purpose,
        ExecutionPurpose::Invocation { .. }
    ));
    let mut forged = request.clone();
    forged.purpose = ExecutionPurpose::Acceptance {
        plan: p.reference().unwrap(),
        role: AcceptanceRole::Candidate,
    };
    assert_eq!(
        forged.validate_against(&retained, &retained.binding),
        Err(EvaluationError::BindingMismatch)
    );
    let criterion = br#"{"rule":"success-count-margin","min_margin":1,"instances":["dev-1"]}"#;
    let mut p = p;
    use sha2::{Digest, Sha256};
    p.criterion.sha256 = format!("{:x}", Sha256::digest(criterion));
    let evidence = SlotEvidence {
        execution: receipt(&p),
        scoring: None,
        metrics: None,
    };
    assert_eq!(
        decide(&p, criterion, &evidence, &evidence).unwrap_err(),
        "no candidate slot"
    );
}

#[test]
fn review_missing_and_unknown_invocations_remain_explicit_evidence() {
    let p = plan(
        vec![slot("use", invocation())],
        json!({"kind":"development"}),
    )
    .unwrap();
    let missing = assess_trials(&p, &[]).unwrap();
    assert_eq!(missing[0].development_outcome, None);
    assert_eq!(missing[0].development_accounting, None);
    assert_eq!(missing[0].scoring_status, RequestStatus::Missing);
    let mut unknown = receipt(&p);
    unknown.outcome = ExecutionOutcome::Unknown;
    unknown.outputs.clear();
    unknown.usage.wall_time_ms = ResourceUsage::Unknown {
        reason: "interrupted measurement".into(),
    };
    let rows = assess_trials(
        &p,
        &[TrialEvidence {
            slot_id: "use".into(),
            execution: unknown,
            scoring: None,
        }],
    )
    .unwrap();
    assert_eq!(rows[0].development_outcome, Some(ExecutionOutcome::Unknown));
    assert_eq!(
        rows[0].development_accounting,
        Some(BudgetAssessment::Unknown)
    );
    assert_eq!(rows[0].scoring_status, RequestStatus::Missing);
    assert!(rows[0].output.is_none());
}
