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

fn binding() -> DevelopmentRunBinding {
    DevelopmentRunBinding {
        org_id: "org_a".into(),
        run_id: "run_1".into(),
        attempt_id: "attempt_1".into(),
        method: reference("method"),
        dependencies: vec![reference("dependency")],
        public_task: reference("public-task"),
        evaluator: reference("evaluator"),
        model_settings: reference("model-settings"),
        environment: reference("environment"),
        purpose: ExecutionPurpose::Ordinary {},
        budget: ResourceGrant {
            model_calls: 2,
            input_tokens: 100,
            output_tokens: 50,
            wall_time_ms: 1000,
            simulation_steps: 200,
        },
    }
}

fn measured(value: u64) -> ResourceUsage {
    ResourceUsage::Measured { value }
}

fn receipt(expected: &DevelopmentRunBinding) -> ExecutionReceipt {
    ExecutionReceipt {
        binding: expected.clone(),
        binding_sha256: expected.canonical_hash().unwrap(),
        actual_method: expected.method.clone(),
        actual_environment: expected.environment.clone(),
        outcome: ExecutionOutcome::OutputGenerated,
        outputs: vec![reference("design")],
        transcript: reference("transcript"),
        usage: Usage {
            model_calls: measured(2),
            input_tokens: measured(100),
            output_tokens: measured(50),
            wall_time_ms: measured(1000),
            simulation_steps: measured(200),
        },
    }
}

#[test]
fn exact_identity_rejects_missing_fields_floating_versions_and_bad_digests() {
    for field in ["domain", "schema", "id", "version"] {
        let mut wire = serde_json::to_value(reference("method")).unwrap();
        wire[field] = json!("  ");
        assert!(serde_json::from_value::<ExactContentRef>(wire)
            .unwrap()
            .validate()
            .is_err());
    }
    for version in [
        "latest", "HEAD", "current", "v*", "^1.0", "~2", ">=1", "1|2",
    ] {
        let mut value = reference("method");
        value.version = version.into();
        assert!(value.validate().is_err(), "{version}");
    }
    for sha in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        "é".repeat(32),
    ] {
        let mut value = reference("method");
        value.sha256 = sha;
        assert!(value.validate().is_err());
    }
    assert!(reference("method").validate().is_ok());
}

#[test]
fn dependency_identities_are_unique_and_distinct_dependency_order_is_frozen() {
    for conflicting_digest in [false, true] {
        let mut value = binding();
        let mut duplicate = value.dependencies[0].clone();
        if conflicting_digest {
            duplicate.sha256 = "b".repeat(64);
        }
        value.dependencies.push(duplicate);
        assert_eq!(
            value.canonical_hash(),
            Err(ContractError::Invalid("duplicate dependency identity"))
        );
    }
    let mut expected = binding();
    expected.dependencies.push(reference("another-dependency"));
    let mut reordered = expected.clone();
    reordered.dependencies.reverse();
    assert_ne!(
        expected.canonical_hash().unwrap(),
        reordered.canonical_hash().unwrap()
    );
    assert_eq!(
        receipt(&reordered).validate_against(&expected),
        Err(ContractError::BindingMismatch)
    );
}

#[test]
fn nonblank_context_is_required_before_hashing() {
    for field in ["org_id", "run_id", "attempt_id"] {
        let mut wire = serde_json::to_value(binding()).unwrap();
        wire[field] = json!("\n");
        assert!(serde_json::from_value::<DevelopmentRunBinding>(wire)
            .unwrap()
            .canonical_hash()
            .is_err());
    }
    for (trial_id, arm_id) in [(" ", "treatment"), ("trial", "")] {
        let mut value = binding();
        value.purpose = ExecutionPurpose::Comparison {
            plan: reference("plan"),
            trial_id: trial_id.into(),
            arm_id: arm_id.into(),
        };
        assert!(value.validate().is_err());
    }
}

#[test]
fn every_frozen_identity_and_grant_resists_rebinding_even_with_a_recomputed_hash() {
    let expected = binding();
    let mutations: &[fn(&mut DevelopmentRunBinding)] = &[
        |b| b.org_id.0.push('b'),
        |b| b.run_id.push('2'),
        |b| b.attempt_id.0.push('2'),
        |b| b.method.sha256 = "b".repeat(64),
        |b| b.dependencies.push(reference("extra")),
        |b| b.public_task.version = "v2".into(),
        |b| b.evaluator.schema = "test/2".into(),
        |b| b.model_settings.id = "other-model".into(),
        |b| b.environment.sha256 = "b".repeat(64),
        |b| b.budget.model_calls += 1,
        |b| b.budget.input_tokens += 1,
        |b| b.budget.output_tokens += 1,
        |b| b.budget.wall_time_ms += 1,
        |b| b.budget.simulation_steps += 1,
        |b| {
            b.purpose = ExecutionPurpose::Acceptance {
                plan: reference("plan"),
                role: AcceptanceRole::Candidate,
            }
        },
    ];
    for mutate in mutations {
        let mut forged = receipt(&expected);
        mutate(&mut forged.binding);
        forged.binding_sha256 = forged.binding.canonical_hash().unwrap();
        assert_ne!(forged.binding_sha256, expected.canonical_hash().unwrap());
        assert_eq!(
            forged.validate_against(&expected),
            Err(ContractError::BindingMismatch)
        );
    }
}

#[test]
fn acceptance_roles_and_comparison_plan_trial_arm_are_not_interchangeable() {
    let mut purposes = vec![ExecutionPurpose::Ordinary {}];
    for role in [AcceptanceRole::Candidate, AcceptanceRole::Control] {
        purposes.push(ExecutionPurpose::Acceptance {
            plan: reference("plan"),
            role,
        });
    }
    for (plan, trial, arm) in [
        ("plan", "t1", "a1"),
        ("other-plan", "t1", "a1"),
        ("plan", "t2", "a1"),
        ("plan", "t1", "a2"),
    ] {
        purposes.push(ExecutionPurpose::Comparison {
            plan: reference(plan),
            trial_id: trial.into(),
            arm_id: arm.into(),
        });
    }
    for mut purpose in [purposes[1].clone(), purposes[3].clone()] {
        match &mut purpose {
            ExecutionPurpose::Acceptance { plan, .. }
            | ExecutionPurpose::Comparison { plan, .. }
            | ExecutionPurpose::Invocation { plan, .. } => plan.sha256 = "b".repeat(64),
            ExecutionPurpose::Ordinary {} => unreachable!(),
        }
        purposes.push(purpose);
    }
    for expected_purpose in &purposes {
        let mut expected = binding();
        expected.purpose = expected_purpose.clone();
        for actual_purpose in &purposes {
            let mut other = expected.clone();
            other.purpose = actual_purpose.clone();
            assert_eq!(
                receipt(&other).validate_against(&expected).is_ok(),
                expected_purpose == actual_purpose
            );
        }
    }
}

#[test]
fn actual_execution_refs_and_declared_hash_must_match() {
    let expected = binding();
    for method_changed in [true, false] {
        let mut value = receipt(&expected);
        if method_changed {
            value.actual_method.version = "v2".into();
        } else {
            value.actual_environment.sha256 = "b".repeat(64);
        }
        assert_eq!(
            value.validate_against(&expected),
            Err(ContractError::IdentityMismatch)
        );
    }
    let mut value = receipt(&expected);
    value.binding_sha256 = "b".repeat(64);
    assert_eq!(
        value.validate_against(&expected),
        Err(ContractError::BindingMismatch)
    );
}

#[test]
fn integer_budget_boundaries_unknowns_and_all_dimensions_are_checked() {
    let expected = binding();
    assert_eq!(
        receipt(&expected).validate_against(&expected),
        Ok(BudgetAssessment::WithinLimits)
    );
    let mut value = receipt(&expected);
    value.usage.model_calls = ResourceUsage::Unknown {
        reason: "provider timed out".into(),
    };
    assert_eq!(
        value.validate_against(&expected),
        Ok(BudgetAssessment::Unknown)
    );
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        serde_json::from_value::<ExecutionReceipt>(wire).unwrap(),
        value
    );
    for field in [
        "model_calls",
        "input_tokens",
        "output_tokens",
        "wall_time_ms",
        "simulation_steps",
    ] {
        let mut wire = serde_json::to_value(&value).unwrap();
        wire["usage"][field] = json!({"kind":"measured", "value": u64::MAX});
        assert!(matches!(
            serde_json::from_value::<ExecutionReceipt>(wire)
                .unwrap()
                .validate_against(&expected),
            Err(ContractError::BudgetExceeded(_))
        ));
    }
    value.usage.model_calls = ResourceUsage::Unknown { reason: "".into() };
    assert!(value.validate_against(&expected).is_err());
    let mut zero = expected.clone();
    zero.budget.model_calls = 0;
    let mut value = receipt(&zero);
    value.usage.model_calls = measured(0);
    assert_eq!(
        value.validate_against(&zero),
        Ok(BudgetAssessment::WithinLimits)
    );
    value.usage.model_calls = measured(1);
    assert_eq!(
        value.validate_against(&zero),
        Err(ContractError::BudgetExceeded("model_calls"))
    );
    zero.budget.model_calls = u64::MAX;
    let mut value = receipt(&zero);
    value.usage.model_calls = measured(u64::MAX);
    assert_eq!(
        value.validate_against(&zero),
        Ok(BudgetAssessment::WithinLimits)
    );
}

#[test]
fn execution_outcomes_do_not_claim_evaluation_success() {
    let expected = binding();
    let mut value = receipt(&expected);
    value.outputs.clear();
    assert!(value.validate_against(&expected).is_err());
    for outcome in [
        ExecutionOutcome::Failed,
        ExecutionOutcome::Interrupted,
        ExecutionOutcome::Unknown,
    ] {
        value.outcome = outcome;
        assert_eq!(
            value.validate_against(&expected),
            Ok(BudgetAssessment::WithinLimits)
        );
    }
    value.outputs.push(reference("partial-output"));
    assert!(value.validate_against(&expected).is_ok());
    value.transcript.sha256.clear();
    assert!(value.validate_against(&expected).is_err());
}

#[test]
fn wire_shapes_reject_unknown_fields_missing_transcripts_and_non_integer_resources() {
    let wire = serde_json::to_value(receipt(&binding())).unwrap();
    for pointer in [
        "",
        "/binding",
        "/binding/method",
        "/binding/budget",
        "/binding/purpose",
        "/usage",
        "/usage/model_calls",
        "/outputs/0",
        "/transcript",
    ] {
        let mut altered = wire.clone();
        altered
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("typo".into(), json!(true));
        assert!(
            serde_json::from_value::<ExecutionReceipt>(altered).is_err(),
            "{pointer}"
        );
    }
    let mut altered = wire.clone();
    altered.as_object_mut().unwrap().remove("transcript");
    assert!(serde_json::from_value::<ExecutionReceipt>(altered).is_err());
    for invalid in [
        json!(-1),
        json!(0.5),
        json!("5"),
        serde_json::from_str::<Value>("18446744073709551616").unwrap(),
    ] {
        for pointer in ["/binding/budget/model_calls", "/usage/model_calls/value"] {
            let mut altered = wire.clone();
            *altered.pointer_mut(pointer).unwrap() = invalid.clone();
            assert!(serde_json::from_value::<ExecutionReceipt>(altered).is_err());
        }
    }
}

#[test]
fn canonical_binding_hash_is_domain_separated_and_round_trip_stable() {
    let expected = binding();
    // Independent Python hashlib/json fixture vector for the documented v1 envelope.
    assert_eq!(
        expected.canonical_hash().unwrap(),
        "6d18df6daa6675749a729010aa1e4efb4a0a4d809eeca17960505c0e9edac23e"
    );
    let wire = serde_json::to_value(&expected).unwrap();
    let round_trip: DevelopmentRunBinding = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(expected.canonical_hash(), round_trip.canonical_hash());
    assert_ne!(
        expected.canonical_hash().unwrap(),
        genesis_core::digest::canonical_sha256(&wire)
    );
    assert_ne!(
        expected.canonical_hash().unwrap(),
        genesis_core::digest::canonical_sha256(&json!({
            "domain":"genesis.development_run_binding", "version":2, "binding":expected,
        }))
    );
}
