//! Independent review of PR124 e6940b2. Retained JSON and pure contracts only;
//! no evaluator, method, subprocess or provider is invoked.
use genesis_core::development_catalogue::*;
use genesis_core::development_experiment::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::TrialPlan;
use serde_json::json;

fn retained() -> (TrialPlan, ExperimentLedger, ExecutionReceipt) {
    (
        serde_json::from_str(include_str!("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search/plan.json")).unwrap(),
        serde_json::from_str(include_str!("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search/candidate/ledger.json")).unwrap(),
        serde_json::from_str(include_str!("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search/candidate/receipt.json")).unwrap(),
    )
}
fn scope(plan: &TrialPlan) -> ExperimentScope<'_> {
    ExperimentScope {
        plan,
        slot_id: "candidate",
    }
}
fn catalogue(plan: &TrialPlan, receipt: &ExecutionReceipt) -> DesignCatalogue {
    let output = receipt.outputs[0].clone();
    let mut experiment = output.clone();
    experiment.id = "submission".into();
    DesignCatalogue::new(scope(plan), vec![CatalogueEntry { experiment, output }]).unwrap()
}

#[test]
fn membership_uses_purpose_even_when_same_bytes_have_crossed_aliases() {
    let (plan, _, mut receipt) = retained();
    let a = receipt.outputs[0].clone();
    let mut b = a.clone();
    b.id = "another-explicit-alias".into();
    let catalogue = DesignCatalogue::new(
        scope(&plan),
        vec![
            CatalogueEntry {
                experiment: a.clone(),
                output: b.clone(),
            },
            CatalogueEntry {
                experiment: b,
                output: a.clone(),
            },
        ],
    )
    .unwrap();
    // A synthetic failed observation: fixture metadata, not a fresh actual run.
    let mut ledger = ExperimentLedger::new(scope(&plan)).unwrap();
    let call = ledger
        .reserve(scope(&plan), plan.development_instances[0].clone(), a, 1)
        .unwrap();
    ledger
        .settle(
            scope(&plan),
            &call,
            ExperimentSettlement {
                outcome: ExperimentOutcome::Failed {
                    reason: "fixture failure".into(),
                },
                simulation_steps: ResourceUsage::Measured { value: 0 },
            },
        )
        .unwrap();
    ledger.seal(scope(&plan)).unwrap();
    receipt.transcript = ledger.reference(scope(&plan)).unwrap();
    receipt.usage.simulation_steps = ResourceUsage::Measured { value: 0 };
    receipt.outcome = ExecutionOutcome::Failed;
    let audit = catalogue
        .audit(scope(&plan), Some(&ledger), Some(&receipt))
        .unwrap();
    assert_eq!(audit.experiments[0].catalogue_index, Some(0));
    assert_eq!(audit.output.unwrap().catalogue_index, Some(1));
    assert!(audit.recorded_evidence_complete);
    assert_eq!(audit.execution_outcome, Some(ExecutionOutcome::Failed));
}

#[test]
fn an_overrun_does_not_mask_foreign_or_malformed_retained_evidence() {
    let (plan, ledger, mut receipt) = retained();
    let catalogue = catalogue(&plan, &receipt);
    receipt.usage.wall_time_ms = ResourceUsage::Measured {
        value: plan.development_grant.wall_time_ms + 1,
    };
    let audit = catalogue
        .audit(scope(&plan), Some(&ledger), Some(&receipt))
        .unwrap();
    assert_eq!(audit.execution_budget, Some(CatalogueBudget::Exceeded));
    assert_eq!(audit.experiments.len(), 32);
    assert!(!audit.recorded_evidence_complete);
    for variant in 0..5 {
        let mut bad = receipt.clone();
        match variant {
            0 => bad.actual_environment.domain = "foreign".into(),
            1 => bad.binding.org_id = "foreign".into(),
            2 => bad.transcript.schema = "foreign/1".into(),
            3 => bad.outputs.push(bad.outputs[0].clone()),
            _ => bad.usage.simulation_steps = ResourceUsage::Unknown { reason: " ".into() },
        }
        assert!(
            catalogue
                .audit(scope(&plan), Some(&ledger), Some(&bad))
                .is_err(),
            "variant {variant}"
        );
    }
    let mut ledger_wire = serde_json::to_value(&ledger).unwrap();
    ledger_wire["records"][0]["call"]["binding_sha256"] = json!("a".repeat(64));
    let foreign: ExperimentLedger = serde_json::from_value(ledger_wire).unwrap();
    assert!(catalogue
        .audit(scope(&plan), Some(&foreign), Some(&receipt))
        .is_err());
}

#[test]
fn restored_duplicates_fail_and_missing_ledger_stays_explicit() {
    let (plan, ledger, receipt) = retained();
    let catalogue = catalogue(&plan, &receipt);
    let audit = catalogue.audit(scope(&plan), None, Some(&receipt)).unwrap();
    assert_eq!(audit.ledger_state, CatalogueLedgerState::Missing);
    assert!(!audit.recorded_evidence_complete);
    assert_eq!(audit.execution_usage, Some(receipt.usage.clone()));
    assert_eq!(audit.output.unwrap().design, receipt.outputs[0]);
    let mut wire = serde_json::to_value(&catalogue).unwrap();
    let duplicate = wire["entries"][0].clone();
    wire["entries"].as_array_mut().unwrap().push(duplicate);
    let restored: DesignCatalogue = serde_json::from_value(wire).unwrap();
    assert!(restored.validate_against(scope(&plan)).is_err());
    assert!(restored
        .audit(scope(&plan), Some(&ledger), Some(&receipt))
        .is_err());
    let mut wire = serde_json::to_value(&catalogue).unwrap();
    wire["entries"][0]["experiment"]["extra"] = json!(true);
    assert!(serde_json::from_value::<DesignCatalogue>(wire).is_err());
}
