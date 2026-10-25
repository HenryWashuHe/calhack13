use genesis_core::development_catalogue::*;
use genesis_core::development_experiment::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::TrialPlan;
use serde_json::json;

fn reference(id: &str) -> ExactContentRef {
    serde_json::from_value(json!({"domain":"test","schema":"design/1","id":id,
        "version":"1","sha256":"a".repeat(64)}))
    .unwrap()
}
fn entry(id: &str) -> CatalogueEntry {
    CatalogueEntry {
        experiment: reference(id),
        output: reference(id),
    }
}
fn plan() -> TrialPlan {
    let slots: Vec<_> = ["candidate", "control"]
        .into_iter()
        .map(|role| {
            json!({
        "slot_id":role,"scoring_request_id":role,"org_id":"org","run_id":role,
        "attempt_id":role,"method":reference(role),"dependencies":[],
        "purpose":{"kind":"acceptance","role":role}})
        })
        .collect();
    let grant = json!({"model_calls":0,"input_tokens":0,"output_tokens":0,"wall_time_ms":100,"simulation_steps":100});
    serde_json::from_value(json!({"id":"plan","version":"1","criterion":reference("criterion"),
        "development_instances":[reference("public")],"scoring_instances":{"kind":"development"},
        "context":{"public_task":reference("task"),"evaluator":reference("evaluator"),"environment":reference("env"),
            "generator":reference("generator"),"instance_set":reference("instances"),"design_schema":reference("schema"),
            "metric_schema":reference("metrics"),"scoring_rule":reference("rule")},
        "model_settings":reference("model"),"development_environment":reference("env"),
        "development_grant":grant,"scoring_grant":grant,"slots":slots})).unwrap()
}
fn scope(plan: &TrialPlan) -> ExperimentScope<'_> {
    ExperimentScope {
        plan,
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
fn ledger(p: &TrialPlan) -> ExperimentLedger {
    let mut l = ExperimentLedger::new(scope(p)).unwrap();
    let call = l
        .reserve(scope(p), reference("public"), reference("design"), 10)
        .unwrap();
    l.settle(scope(p), &call, returned(3)).unwrap();
    l.seal(scope(p)).unwrap();
    l
}
fn execution(p: &TrialPlan, l: &ExperimentLedger) -> ExecutionReceipt {
    let binding = p.expected_binding("candidate").unwrap();
    ExecutionReceipt {
        binding_sha256: binding.canonical_hash().unwrap(),
        actual_method: binding.method.clone(),
        actual_environment: binding.environment.clone(),
        binding,
        outcome: ExecutionOutcome::OutputGenerated,
        outputs: vec![reference("design")],
        transcript: l.reference(scope(p)).unwrap(),
        usage: Usage {
            model_calls: measured(0),
            input_tokens: measured(0),
            output_tokens: measured(0),
            wall_time_ms: measured(1),
            simulation_steps: l.assess(scope(p)).unwrap().simulation_steps,
        },
    }
}

#[test]
fn catalogue_is_ordered_bounded_exact_and_strictly_restored() {
    let p = plan();
    let designs = vec![entry("one"), entry("two")];
    let c = DesignCatalogue::new(scope(&p), designs.clone()).unwrap();
    let restored: DesignCatalogue =
        serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
    assert_eq!(restored, c);
    let reversed = DesignCatalogue::new(scope(&p), designs.into_iter().rev().collect()).unwrap();
    assert_ne!(
        c.canonical_hash(scope(&p)).unwrap(),
        reversed.canonical_hash(scope(&p)).unwrap()
    );
    for bad in [
        vec![],
        vec![entry("one"), entry("one")],
        (0..=MAX_CATALOGUE_DESIGNS)
            .map(|i| entry(&i.to_string()))
            .collect(),
    ] {
        assert!(DesignCatalogue::new(scope(&p), bad).is_err());
    }
    let mut different_bytes = reference("one");
    different_bytes.sha256 = "b".repeat(64);
    // Runtime designs commonly share the label "submission" but differ in bytes.
    assert!(DesignCatalogue::new(
        scope(&p),
        vec![
            entry("one"),
            CatalogueEntry {
                experiment: different_bytes.clone(),
                output: different_bytes
            }
        ]
    )
    .is_ok());
    for field in ["domain", "schema", "id", "version", "sha256"] {
        let mut wire = serde_json::to_value(&c).unwrap();
        wire["entries"][0]["experiment"][field] = json!("");
        let bad: DesignCatalogue = serde_json::from_value(wire).unwrap();
        assert!(bad.validate_against(scope(&p)).is_err());
    }
    let mut wire = serde_json::to_value(&c).unwrap();
    wire["unexpected"] = json!(true);
    assert!(serde_json::from_value::<DesignCatalogue>(wire).is_err());
}

#[test]
fn purpose_aliases_are_explicit_consistent_and_unique_in_each_role() {
    let p = plan();
    let pair = CatalogueEntry {
        experiment: reference("submission"),
        output: reference("output"),
    };
    assert!(DesignCatalogue::new(scope(&p), vec![pair.clone()]).is_ok());
    for role in ["experiment", "output"] {
        let mut wire = serde_json::to_value(&pair).unwrap();
        wire[role]["id"] = json!("different");
        let other = serde_json::from_value(wire).unwrap();
        assert!(DesignCatalogue::new(scope(&p), vec![pair.clone(), other]).is_err());
    }
    for field in ["domain", "schema", "version", "sha256"] {
        let mut wire = serde_json::to_value(&pair).unwrap();
        wire["output"][field] = json!(if field == "sha256" {
            "b".repeat(64)
        } else {
            "other".into()
        });
        assert!(
            DesignCatalogue::new(scope(&p), vec![serde_json::from_value(wire).unwrap()]).is_err()
        );
    }
}

#[test]
fn positive_control_and_all_reference_fields_determine_membership() {
    let p = plan();
    let c = DesignCatalogue::new(scope(&p), vec![entry("design")]).unwrap();
    let l = ledger(&p);
    let r = execution(&p, &l);
    let before = serde_json::to_string(&(&c, &l, &r)).unwrap();
    let audit = c.audit(scope(&p), Some(&l), Some(&r)).unwrap();
    assert!(audit.recorded_evidence_complete);
    assert_eq!(audit.experiments[0].catalogue_index, Some(0));
    assert_eq!(audit.output.unwrap().catalogue_index, Some(0));
    assert_eq!(before, serde_json::to_string(&(&c, &l, &r)).unwrap());
    for field in ["domain", "schema", "id", "version", "sha256"] {
        let mut wire = serde_json::to_value(reference("design")).unwrap();
        wire[field] = json!(if field == "sha256" {
            "b".repeat(64)
        } else {
            "other".into()
        });
        let outside: ExactContentRef = serde_json::from_value(wire).unwrap();
        let mut l = ExperimentLedger::new(scope(&p)).unwrap();
        let call = l
            .reserve(scope(&p), reference("public"), outside.clone(), 10)
            .unwrap();
        l.settle(scope(&p), &call, returned(3)).unwrap();
        l.seal(scope(&p)).unwrap();
        let mut r = execution(&p, &l);
        r.outputs = vec![outside.clone()];
        let audit = c.audit(scope(&p), Some(&l), Some(&r)).unwrap();
        assert!(audit.recorded_evidence_complete); // Completeness is NOT membership.
        assert_eq!(audit.experiments[0].record.call.design, outside);
        assert_eq!(audit.experiments[0].record.call.index, 0);
        assert_eq!(audit.experiments[0].catalogue_index, None);
        assert_eq!(audit.output.unwrap().catalogue_index, None);
    }
}

#[test]
fn foreign_plan_slot_and_receipt_links_are_rejected() {
    let p = plan();
    let c = DesignCatalogue::new(scope(&p), vec![entry("design")]).unwrap();
    let l = ledger(&p);
    let r = execution(&p, &l);
    let mut changed = p.clone();
    changed.development_grant.wall_time_ms += 1;
    assert!(c.audit(scope(&changed), Some(&l), Some(&r)).is_err());
    let changed_catalogue = DesignCatalogue::new(scope(&changed), vec![entry("design")]).unwrap();
    assert!(changed_catalogue
        .audit(scope(&changed), Some(&l), Some(&r))
        .is_err());
    let foreign_ledger = ExperimentLedger::new(ExperimentScope {
        plan: &p,
        slot_id: "control",
    })
    .unwrap();
    assert!(c.audit(scope(&p), Some(&foreign_ledger), None).is_err());
    assert!(c
        .audit(
            ExperimentScope {
                plan: &p,
                slot_id: "control"
            },
            Some(&l),
            Some(&r)
        )
        .is_err());
    for field in ["plan", "slot_id", "binding_sha256", "schema_version"] {
        let mut wire = serde_json::to_value(&c).unwrap();
        if field == "plan" {
            wire[field]["id"] = json!("foreign");
        } else {
            wire[field] = if field == "schema_version" {
                json!(2)
            } else {
                json!("other")
            };
        }
        assert!(serde_json::from_value::<DesignCatalogue>(wire)
            .unwrap()
            .validate_against(scope(&p))
            .is_err());
    }
    for variant in 0..5 {
        let mut bad = r.clone();
        match variant {
            0 => bad.binding_sha256 = "b".repeat(64),
            1 => bad.actual_method.id = "foreign".into(),
            2 => bad.transcript.id = "foreign".into(),
            3 => bad.usage.simulation_steps = measured(0),
            _ => bad.outputs.push(reference("second")),
        }
        assert!(c.audit(scope(&p), Some(&l), Some(&bad)).is_err());
    }
}

#[test]
fn missing_unsealed_pending_and_unknown_evidence_never_complete() {
    let p = plan();
    let c = DesignCatalogue::new(scope(&p), vec![entry("design")]).unwrap();
    let l = ledger(&p);
    let r = execution(&p, &l);
    for (l, r) in [(None, None), (Some(&l), None), (None, Some(&r))] {
        let a = c.audit(scope(&p), l, r).unwrap();
        assert!(!a.recorded_evidence_complete);
        assert_eq!(a.execution_outcome, r.map(|r| r.outcome));
    }
    let unsealed = ExperimentLedger::new(scope(&p)).unwrap();
    let a = c.audit(scope(&p), Some(&unsealed), Some(&r)).unwrap();
    assert_eq!(a.ledger_state, CatalogueLedgerState::Unsealed);
    assert!(!a.recorded_evidence_complete);
    for settlement in [
        None,
        Some(ExperimentSettlement {
            outcome: ExperimentOutcome::Unknown {
                reason: "worker lost".into(),
            },
            simulation_steps: measured(3),
        }),
        Some(ExperimentSettlement {
            simulation_steps: ResourceUsage::Unknown {
                reason: "meter lost".into(),
            },
            ..returned(3)
        }),
    ] {
        let mut l = ExperimentLedger::new(scope(&p)).unwrap();
        let call = l
            .reserve(scope(&p), reference("public"), reference("design"), 10)
            .unwrap();
        if let Some(s) = settlement {
            l.settle(scope(&p), &call, s).unwrap();
        }
        l.seal(scope(&p)).unwrap();
        let a = c
            .audit(scope(&p), Some(&l), Some(&execution(&p, &l)))
            .unwrap();
        assert!(!a.recorded_evidence_complete);
        assert_eq!(a.experiments[0].record, l.records()[0]);
    }
}

#[test]
fn known_failure_with_output_is_not_erased_or_reported_as_success() {
    let p = plan();
    let c = DesignCatalogue::new(scope(&p), vec![entry("design")]).unwrap();
    let mut l = ExperimentLedger::new(scope(&p)).unwrap();
    let call = l
        .reserve(scope(&p), reference("public"), reference("design"), 10)
        .unwrap();
    l.settle(
        scope(&p),
        &call,
        ExperimentSettlement {
            outcome: ExperimentOutcome::Failed {
                reason: "invalid design".into(),
            },
            simulation_steps: measured(3),
        },
    )
    .unwrap();
    l.seal(scope(&p)).unwrap();
    let mut r = execution(&p, &l);
    r.outcome = ExecutionOutcome::Failed;
    let a = c.audit(scope(&p), Some(&l), Some(&r)).unwrap();
    assert!(a.recorded_evidence_complete);
    assert_eq!(a.execution_outcome, Some(ExecutionOutcome::Failed));
    assert_eq!(a.output.unwrap().design, reference("design"));
    assert_eq!(a.experiments[0].record, l.records()[0]);
    r.outcome = ExecutionOutcome::Unknown;
    assert!(
        !c.audit(scope(&p), Some(&l), Some(&r))
            .unwrap()
            .recorded_evidence_complete
    );
}

#[test]
fn overrun_retention_cannot_hide_foreign_transcripts_usage_or_unknowns() {
    let p = plan();
    let c = DesignCatalogue::new(scope(&p), vec![entry("design")]).unwrap();
    let mut l = ExperimentLedger::new(scope(&p)).unwrap();
    let call = l
        .reserve(scope(&p), reference("public"), reference("design"), 1)
        .unwrap();
    l.settle(scope(&p), &call, returned(8)).unwrap();
    l.seal(scope(&p)).unwrap();
    let mut r = execution(&p, &l);
    let a = c.audit(scope(&p), Some(&l), Some(&r)).unwrap();
    assert!(a.ledger_accounting.unwrap().limit_exceeded);
    assert_eq!(a.execution_budget, Some(CatalogueBudget::WithinLimits));
    assert!(!a.recorded_evidence_complete);
    r.usage.wall_time_ms = measured(101);
    r.usage.input_tokens = ResourceUsage::Unknown {
        reason: "meter lost".into(),
    };
    let a = c.audit(scope(&p), Some(&l), Some(&r)).unwrap();
    assert_eq!(a.execution_budget, Some(CatalogueBudget::Exceeded));
    assert_eq!(a.execution_usage, Some(r.usage.clone()));
    for variant in 0..3 {
        let mut bad = r.clone();
        match variant {
            0 => bad.transcript.id = "foreign".into(),
            1 => bad.usage.simulation_steps = measured(0),
            _ => bad.usage.input_tokens = ResourceUsage::Unknown { reason: " ".into() },
        }
        assert!(c.audit(scope(&p), Some(&l), Some(&bad)).is_err());
    }
}

#[test]
fn retained_b_run_maps_submission_and_output_aliases_without_rewriting_evidence() {
    // Parse-only compatibility with PR116, never source/executor/scorer execution.
    let p: TrialPlan = serde_json::from_str(include_str!("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search/plan.json")).unwrap();
    let l: ExperimentLedger = serde_json::from_str(include_str!("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search/candidate/ledger.json")).unwrap();
    let r: ExecutionReceipt = serde_json::from_str(include_str!("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search/candidate/receipt.json")).unwrap();
    let before = serde_json::to_string(&(&p, &l, &r)).unwrap();
    let mut entries: Vec<CatalogueEntry> = vec![];
    for record in l.records() {
        if !entries.iter().any(|e| e.experiment == record.call.design) {
            let mut output = record.call.design.clone();
            output.id = "output".into(); // Explicit host declaration, not library inference.
            entries.push(CatalogueEntry {
                experiment: record.call.design.clone(),
                output,
            });
        }
    }
    let c = DesignCatalogue::new(scope(&p), entries).unwrap();
    let a = c.audit(scope(&p), Some(&l), Some(&r)).unwrap();
    assert_eq!(a.experiments.len(), 32);
    assert!(a.experiments.iter().all(|e| e.catalogue_index.is_some()));
    assert!(a.output.unwrap().catalogue_index.is_some());
    assert!(a.recorded_evidence_complete);
    assert_eq!(before, serde_json::to_string(&(&p, &l, &r)).unwrap());
}
