use genesis_core::development_experiment::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::TrialPlan;
use serde_json::json;

fn reference(id: &str) -> ExactContentRef {
    serde_json::from_value(json!({"domain":"test","schema":"test/1","id":id,
        "version":"1","sha256":"a".repeat(64)}))
    .unwrap()
}
fn plan() -> TrialPlan {
    let mut slots = vec![];
    for role in ["candidate", "control"] {
        slots.push(
            json!({"slot_id":role,"scoring_request_id":role,"org_id":"org",
            "run_id":role,"attempt_id":role,"method":reference(role),"dependencies":[],
            "purpose":{"kind":"acceptance","role":role}}),
        );
    }
    let grant = json!({"model_calls":0,"input_tokens":0,"output_tokens":0,"wall_time_ms":100,"simulation_steps":100});
    serde_json::from_value(json!({"id":"plan","version":"1","criterion":reference("criterion"),
        "development_instances":[reference("public")],"scoring_instances":{"kind":"development"},
        "context":{"public_task":reference("task"),"evaluator":reference("evaluator"),"environment":reference("env"),
            "generator":reference("generator"),"instance_set":reference("scoring-only"),"design_schema":reference("design-schema"),
            "metric_schema":reference("metrics"),"scoring_rule":reference("rule")},
        "model_settings":reference("model"),"development_environment":reference("env"),
        "development_grant":grant,"scoring_grant":grant,"slots":slots})).unwrap()
}
fn scope(p: &TrialPlan) -> ExperimentScope<'_> {
    ExperimentScope {
        plan: p,
        slot_id: "candidate",
    }
}
fn measured(value: u64) -> ResourceUsage {
    ResourceUsage::Measured { value }
}
fn returned(steps: u64) -> ExperimentSettlement {
    ExperimentSettlement {
        outcome: ExperimentOutcome::Returned {
            observed_evaluator: reference("evaluator"),
            result: Box::new(reference("result")),
        },
        simulation_steps: measured(steps),
    }
}
fn reserve(ledger: &mut ExperimentLedger, p: &TrialPlan, steps: u64) -> ExperimentCall {
    ledger
        .reserve(scope(p), reference("public"), reference("design"), steps)
        .unwrap()
}
fn restored(ledger: &ExperimentLedger) -> ExperimentLedger {
    serde_json::from_value(serde_json::to_value(ledger).unwrap()).unwrap()
}
fn execution(ledger: &ExperimentLedger, p: &TrialPlan) -> ExecutionReceipt {
    let binding = p.expected_binding("candidate").unwrap();
    ExecutionReceipt {
        binding_sha256: binding.canonical_hash().unwrap(),
        actual_method: binding.method.clone(),
        actual_environment: binding.environment.clone(),
        binding,
        outcome: ExecutionOutcome::Failed,
        outputs: vec![],
        transcript: ledger.reference(scope(p)).unwrap(),
        usage: Usage {
            model_calls: measured(0),
            input_tokens: measured(0),
            output_tokens: measured(0),
            wall_time_ms: measured(1),
            simulation_steps: ledger.assess(scope(p)).unwrap().simulation_steps,
        },
    }
}

#[test]
fn measured_failures_and_successes_share_one_grant_across_restore() {
    let p = plan();
    let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
    let call = reserve(&mut ledger, &p, 80);
    assert!(ledger
        .reserve(scope(&p), reference("public"), reference("design"), 1)
        .is_err());
    ledger
        .settle(
            scope(&p),
            &call,
            ExperimentSettlement {
                outcome: ExperimentOutcome::Failed {
                    reason: "invalid design".into(),
                },
                simulation_steps: measured(30),
            },
        )
        .unwrap();
    let mut ledger = restored(&ledger);
    assert_eq!(ledger.assess(scope(&p)).unwrap().remaining_steps, Some(70));
    assert!(ledger
        .reserve(scope(&p), reference("public"), reference("design"), 80)
        .is_err());
    let call = reserve(&mut ledger, &p, 70);
    ledger.settle(scope(&p), &call, returned(70)).unwrap();
    assert!(ledger.settle(scope(&p), &call, returned(70)).is_err());
    assert_eq!(
        ledger.assess(scope(&p)).unwrap().simulation_steps,
        measured(100)
    );
    assert!(ledger
        .reserve(scope(&p), reference("public"), reference("design"), 1)
        .is_err());
}

#[test]
fn exact_public_inputs_bind_calls_and_settlements() {
    let p = plan();
    let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
    let mut changed = reference("public");
    changed.sha256 = "b".repeat(64);
    for instance in [reference("scoring-only"), changed] {
        assert!(ledger
            .reserve(scope(&p), instance, reference("design"), 1)
            .is_err());
        assert!(ledger.records().is_empty());
    }
    assert!(ledger
        .reserve(scope(&p), reference("public"), reference("design"), 0)
        .is_err());
    let call = reserve(&mut ledger, &p, 50);
    for field in ["design", "instance"] {
        let mut wire = serde_json::to_value(&call).unwrap();
        wire[field]["sha256"] = json!("b".repeat(64));
        let bad = serde_json::from_value(wire).unwrap();
        assert!(ledger.settle(scope(&p), &bad, returned(5)).is_err());
    }
    let mut bad = call.clone();
    bad.step_limit = 51;
    assert!(ledger.settle(scope(&p), &bad, returned(5)).is_err());
    let mut bad = returned(5);
    bad.outcome = ExperimentOutcome::Returned {
        observed_evaluator: reference("other"),
        result: Box::new(reference("result")),
    };
    assert!(ledger.settle(scope(&p), &call, bad).is_err());
    let mut changed = p.clone();
    changed.development_grant.simulation_steps += 1;
    assert!(ledger.assess(scope(&changed)).is_err());
    assert!(ledger.records()[0].settlement.is_none());
}

#[test]
fn pending_and_unknown_evidence_survive_sealing_without_refunds() {
    for settlement in [
        None,
        Some(ExperimentSettlement {
            outcome: ExperimentOutcome::Unknown {
                reason: "worker lost".into(),
            },
            simulation_steps: measured(8),
        }),
        Some(ExperimentSettlement {
            simulation_steps: ResourceUsage::Unknown {
                reason: "meter lost".into(),
            },
            ..returned(8)
        }),
    ] {
        let p = plan();
        let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
        let call = reserve(&mut ledger, &p, 20);
        if let Some(s) = settlement {
            ledger.settle(scope(&p), &call, s).unwrap();
        }
        let mut ledger = restored(&ledger);
        assert_eq!(ledger.assess(scope(&p)).unwrap().remaining_steps, None);
        assert!(ledger
            .reserve(scope(&p), reference("public"), reference("design"), 1)
            .is_err());
        assert!(ledger.reference(scope(&p)).is_err());
        ledger.seal(scope(&p)).unwrap();
        let receipt = execution(&ledger, &p);
        assert!(ledger.validate_execution(scope(&p), &receipt).is_ok());
        assert!(ledger.settle(scope(&p), &call, returned(8)).is_err());
        let mut false_zero = receipt;
        false_zero.usage.simulation_steps = measured(0);
        assert!(ledger.validate_execution(scope(&p), &false_zero).is_err());
    }
}

#[test]
fn overrun_is_retained_and_blocks_calls_including_integer_overflow() {
    for ceiling in [100, u64::MAX] {
        let mut p = plan();
        p.development_grant.simulation_steps = ceiling;
        let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
        let call = reserve(&mut ledger, &p, 1);
        ledger.settle(scope(&p), &call, returned(1)).unwrap();
        let call = reserve(&mut ledger, &p, 1);
        ledger.settle(scope(&p), &call, returned(ceiling)).unwrap();
        assert_eq!(ledger.records()[1].settlement, Some(returned(ceiling)));
        let accounting = restored(&ledger).assess(scope(&p)).unwrap();
        assert!(accounting.limit_exceeded);
        assert_eq!(accounting.remaining_steps, None);
        ledger.seal(scope(&p)).unwrap();
        assert!(ledger
            .validate_execution(scope(&p), &execution(&ledger, &p))
            .is_err());
    }
}

#[test]
fn restored_history_cannot_reorder_duplicate_or_continue_unresolved_calls() {
    let p = plan();
    let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
    let call = reserve(&mut ledger, &p, 10);
    ledger.settle(scope(&p), &call, returned(5)).unwrap();
    reserve(&mut ledger, &p, 10);
    for variant in 0..5 {
        let mut wire = serde_json::to_value(&ledger).unwrap();
        match variant {
            0 => wire["records"].as_array_mut().unwrap().swap(0, 1),
            1 => wire["records"][1]["call"]["index"] = json!(0),
            2 => wire["records"][0]["settlement"] = json!(null),
            3 => wire["slot_id"] = json!("control"),
            _ => wire["records"][1]["call"]["binding_sha256"] = json!("b".repeat(64)),
        }
        let altered: ExperimentLedger = serde_json::from_value(wire).unwrap();
        assert!(altered.assess(scope(&p)).is_err());
    }
    let control = ExperimentScope {
        plan: &p,
        slot_id: "control",
    };
    let mut wrong_slot = ExperimentLedger::new(control).unwrap();
    wrong_slot.seal(control).unwrap();
    let binding = p.expected_binding("control").unwrap();
    let mut candidate = ExperimentLedger::new(scope(&p)).unwrap();
    candidate.seal(scope(&p)).unwrap();
    let mut receipt = execution(&candidate, &p);
    receipt.transcript = wrong_slot.reference(control).unwrap();
    receipt.binding = binding.clone();
    receipt.binding_sha256 = binding.canonical_hash().unwrap();
    receipt.actual_method = binding.method;
    assert!(wrong_slot.validate_execution(control, &receipt).is_ok());
    assert!(wrong_slot.validate_execution(scope(&p), &receipt).is_err());
}

#[test]
fn sealed_exact_transcript_and_usage_bind_execution() {
    use sha2::{Digest, Sha256};
    let p = plan();
    let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
    let call = reserve(&mut ledger, &p, 20);
    ledger.settle(scope(&p), &call, returned(12)).unwrap();
    ledger.seal(scope(&p)).unwrap();
    assert_eq!(
        ledger.reference(scope(&p)).unwrap().sha256,
        format!(
            "{:x}",
            Sha256::digest(ledger.transcript_json(scope(&p)).unwrap().as_bytes())
        )
    );
    let receipt = execution(&ledger, &p);
    assert_eq!(
        ledger.validate_execution(scope(&p), &receipt).unwrap(),
        BudgetAssessment::WithinLimits
    );
    let mut altered = receipt.clone();
    altered.transcript.sha256 = "b".repeat(64);
    assert!(ledger.validate_execution(scope(&p), &altered).is_err());
    let mut altered = receipt;
    altered.usage.simulation_steps = measured(11);
    assert!(ledger.validate_execution(scope(&p), &altered).is_err());
    assert!(ledger
        .reserve(scope(&p), reference("public"), reference("design"), 1)
        .is_err());
}

#[test]
fn comparison_development_can_experiment_only_on_public_instances() {
    use genesis_core::development_trial::{ScoringInstances, TrialPurpose};
    let mut p = plan();
    p.scoring_instances = ScoringInstances::HiddenCommitment {
        protocol: reference("protocol"),
    };
    for slot in &mut p.slots {
        slot.purpose = TrialPurpose::Comparison {
            trial_id: "trial".into(),
            arm_id: slot.slot_id.clone(),
        };
    }
    let mut ledger = ExperimentLedger::new(scope(&p)).unwrap();
    assert!(ledger
        .reserve(
            scope(&p),
            p.context.instance_set.clone(),
            reference("design"),
            1
        )
        .is_err());
    let call = reserve(&mut ledger, &p, 10);
    ledger.settle(scope(&p), &call, returned(10)).unwrap();
    assert_eq!(ledger.assess(scope(&p)).unwrap().remaining_steps, Some(90));
}
