use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use genesis_forge::{
    method_accounting::{CostObservation, CostScope, GeneratedMethodLink},
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStage, ChainStageBinding, ChainWork},
    method_chain_costs::{assess_chain_costs, ChainObservedCosts, ChainStageEvidence},
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits, StageTrialBinding},
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
    text_generation::{TextFailure, TextOutcome},
};

#[path = "common/method_accounting_fixture.rs"]
mod fixture;

fn grant(value: u64) -> ResourceGrant {
    ResourceGrant {
        model_calls: value,
        input_tokens: value,
        output_tokens: value,
        wall_time_ms: value,
        simulation_steps: value,
    }
}

struct Case {
    stage: ChainStage,
    allocation: Option<StageAllocation>,
    admission: Option<GenerationAdmission>,
    snapshots: Vec<MethodSynthesisSession>,
    scope: CostScope,
    trial: TrialPlan,
    trial_binding: Option<StageTrialBinding>,
    rows: Vec<TrialEvidence>,
}
impl Case {
    fn new(id: &str, operation: &str, generation: bool) -> Self {
        let generated = fixture::generation(operation, true);
        let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
        let mut trial = fixture::trial(&generated);
        trial.id = id.into();
        for slot in &mut trial.slots {
            slot.org_id = "org_a".into();
            slot.run_id = format!("{id}-{}", slot.slot_id);
            slot.attempt_id = format!("{id}-{}", slot.slot_id).into();
        }
        let limits = StageLimits {
            id: id.into(),
            max_trial_slots: 2,
            development_grant: trial.development_grant.clone(),
            scoring_grant: trial.scoring_grant.clone(),
            total_grant: grant(3000),
        };
        let mut admission = GenerationAdmission::new(
            GenerationRoster::freeze(
                format!("{id}-roster"),
                prepared.plan().settings.clone(),
                grant(2000),
                std::slice::from_ref(&prepared),
            )
            .unwrap(),
        )
        .unwrap();
        let allocation =
            StageAllocation::prepare(limits.clone(), &admission, std::slice::from_ref(&prepared))
                .unwrap();
        admission.reserve(&prepared).unwrap();
        let mut scope = fixture::scope(
            if generation {
                std::slice::from_ref(&prepared)
            } else {
                &[]
            },
            Some(&trial),
        );
        scope.total_grant = limits.total_grant.clone();
        if generation {
            scope.generated_links = trial
                .slots
                .iter()
                .map(|slot| GeneratedMethodLink {
                    org_id: "org_a".into(),
                    operation_id: operation.into(),
                    slot_id: slot.slot_id.clone(),
                })
                .collect();
        }
        let trial_binding = generation.then(|| allocation.bind(&trial, &scope).unwrap());
        let rows = trial
            .slots
            .iter()
            .map(|slot| fixture::evidence(&trial, &slot.slot_id))
            .collect();
        Self {
            stage: ChainStage {
                org_id: "org_a".into(),
                limits,
                trial_model_settings: trial.model_settings.clone(),
                work: if generation {
                    ChainWork::Generation {
                        settings: prepared.plan().settings.clone(),
                        max_attempts: 1,
                        grant: grant(2000),
                    }
                } else {
                    ChainWork::TrialOnly
                },
            },
            allocation: generation.then_some(allocation),
            admission: generation.then_some(admission),
            snapshots: if generation { vec![generated] } else { vec![] },
            scope,
            trial,
            trial_binding,
            rows,
        }
    }
    fn binding(&self, chain: &ChainAllocation) -> ChainStageBinding {
        if let Some(allocation) = &self.allocation {
            chain
                .bind_generation(&self.stage.limits.id, allocation)
                .unwrap()
        } else {
            chain
                .bind_trial_only(&self.stage.limits.id, &self.trial, &self.scope)
                .unwrap()
        }
    }
    fn input<'a>(&'a self, binding: &'a ChainStageBinding) -> ChainStageEvidence<'a> {
        if let Some(allocation) = &self.allocation {
            ChainStageEvidence::Generation {
                binding,
                allocation,
                admission: self.admission.as_ref().unwrap(),
                scope: &self.scope,
                snapshots: &self.snapshots,
                trial: self
                    .trial_binding
                    .as_ref()
                    .map(|b| (b, &self.trial, self.rows.as_slice())),
            }
        } else {
            ChainStageEvidence::TrialOnly {
                binding,
                scope: &self.scope,
                trial: &self.trial,
                evidence: &self.rows,
            }
        }
    }
    fn generation_only(&mut self) {
        self.scope.trial = None;
        self.scope.generated_links.clear();
        self.trial_binding = None;
    }
    fn rebuild_trial_only(&mut self) {
        assert!(self.allocation.is_none());
        self.scope = fixture::scope(&[], Some(&self.trial));
        self.scope.total_grant = self.stage.limits.total_grant.clone();
        self.rows = self
            .trial
            .slots
            .iter()
            .map(|s| fixture::evidence(&self.trial, &s.slot_id))
            .collect();
    }
}
fn chain(cases: &[&Case]) -> ChainAllocation {
    ChainAllocation::new(
        "chain".into(),
        grant(3000 * cases.len() as u64),
        cases.iter().map(|c| c.stage.clone()).collect(),
    )
    .unwrap()
}

#[test]
fn two_generations_and_repeated_trial_only_uses_charge_each_attempt_once() {
    let a = Case::new("develop-a", "generate-a", true);
    let b = Case::new("develop-b", "generate-b", true);
    let reuse = Case::new("reuse-b", "generate-a", false);
    let returned = Case::new("return-a", "generate-b", false);
    // Independently generated identical packages still have two generation costs.
    assert_eq!(
        a.snapshots[0].package().unwrap().reference().unwrap(),
        b.snapshots[0].package().unwrap().reference().unwrap()
    );
    let cases = [&a, &b, &reuse, &returned];
    let chain = chain(&cases);
    let bindings: Vec<_> = cases.iter().map(|c| c.binding(&chain)).collect();
    let before = serde_json::to_vec(&(
        &chain,
        &a.admission,
        &a.snapshots,
        &b.admission,
        &b.snapshots,
        &a.rows[0].execution,
    ))
    .unwrap();
    let input: Vec<_> = cases
        .iter()
        .zip(&bindings)
        .map(|(c, b)| c.input(b))
        .collect();
    let report = assess_chain_costs(&chain, &input).unwrap();
    assert_eq!(report.chain_sha256, chain.sha256().unwrap());
    assert_eq!(
        report
            .stages
            .iter()
            .map(|s| s.stage_id.as_str())
            .collect::<Vec<_>>(),
        ["develop-a", "develop-b", "reuse-b", "return-a"]
    );
    for (index, total) in report.combined.iter().enumerate() {
        assert_eq!(total.known_lower_bound, if index == 4 { 16 } else { 18 });
        assert!(!total.unknown && !total.overflow && !total.limit_exceeded);
    }
    let ChainObservedCosts::Generation(costs) = report.stages[0].observed.as_ref().unwrap() else {
        panic!("generation report")
    };
    assert_eq!(costs.generation.costs.rows.len(), 1);
    assert_eq!(costs.costs.rows.len(), 5);
    assert_eq!(costs.costs.matched_package_links, [true, true]);
    let ChainObservedCosts::TrialOnly(costs) = report.stages[2].observed.as_ref().unwrap() else {
        panic!("trial-only report")
    };
    assert!(costs.generation.iter().all(|t| t.known_lower_bound == 0));
    assert_eq!(
        before,
        serde_json::to_vec(&(
            &chain,
            &a.admission,
            &a.snapshots,
            &b.admission,
            &b.snapshots,
            &a.rows[0].execution
        ))
        .unwrap()
    );
    let reversed: Vec<_> = cases
        .iter()
        .zip(&bindings)
        .rev()
        .map(|(c, b)| c.input(b))
        .collect();
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::to_value(assess_chain_costs(&chain, &reversed).unwrap()).unwrap()
    );
}

#[test]
fn generation_only_missing_and_unbound_stages_remain_visible_and_unknown() {
    let mut generated = Case::new("generated", "one", true);
    generated.generation_only();
    let mut missing = Case::new("missing", "two", true);
    missing.generation_only();
    missing.snapshots.clear();
    let unbound = Case::new("unbound-return", "one", false);
    let chain = chain(&[&generated, &missing, &unbound]);
    let bindings = [generated.binding(&chain), missing.binding(&chain)];
    let report = assess_chain_costs(
        &chain,
        &[generated.input(&bindings[0]), missing.input(&bindings[1])],
    )
    .unwrap();
    assert_eq!(report.stages.len(), 3);
    assert!(report.stages.iter().all(|s| s.unresolved_trial));
    assert!(report.stages[2].binding.is_none() && report.stages[2].observed.is_none());
    assert_eq!(report.stages[2].reserved_grant, grant(3000));
    assert!(report.combined.iter().all(|t| t.unknown && !t.overflow));
    assert_eq!(report.combined[0].known_lower_bound, 1);
    let ChainObservedCosts::Generation(costs) = report.stages[1].observed.as_ref().unwrap() else {
        panic!("missing generation report")
    };
    assert_eq!(costs.admission.reserved_attempts, 1);
    assert!(matches!(
        costs.costs.rows[0].observation,
        CostObservation::Missing
    ));
    let empty = assess_chain_costs(&chain, &[]).unwrap();
    assert!(empty
        .combined
        .iter()
        .all(|t| t.unknown && t.known_lower_bound == 0));
}

#[test]
fn failed_unknown_missing_and_exceeded_usage_survive_aggregation() {
    let mut case = Case::new("failed-stage", "failed-generation", true);
    let mut failed = MethodSynthesisSession::new(case.snapshots[0].plan().clone()).unwrap();
    failed.begin().unwrap();
    let mut completion = case.snapshots[0].completion().unwrap().clone();
    completion.outcome = TextOutcome::Failed {
        reason: TextFailure::Protocol,
    };
    completion.usage.input_tokens = ResourceUsage::Measured { value: 3001 };
    failed.complete(completion).unwrap();
    case.snapshots = vec![failed];
    case.rows[0].execution.outcome = ExecutionOutcome::Failed;
    case.rows[0].execution.usage.simulation_steps = ResourceUsage::Measured { value: 2001 };
    case.rows[0].scoring = None;
    case.rows[1].execution.outcome = ExecutionOutcome::Unknown;
    case.rows[1].execution.usage.input_tokens = ResourceUsage::Unknown {
        reason: "interrupted meter".into(),
    };
    case.rows[1].scoring = None;
    let chain = chain(&[&case]);
    let binding = case.binding(&chain);
    let report = assess_chain_costs(&chain, &[case.input(&binding)]).unwrap();
    let ChainObservedCosts::Generation(costs) = report.stages[0].observed.as_ref().unwrap() else {
        panic!("generation report")
    };
    assert!(matches!(
        costs.costs.rows[0].observation,
        CostObservation::Generation {
            phase: MethodSynthesisPhase::Failed,
            ..
        }
    ));
    assert!(matches!(
        costs.costs.rows[1].observation,
        CostObservation::Execution {
            outcome: ExecutionOutcome::Failed,
            has_output: true
        }
    ));
    assert!(matches!(
        costs.costs.rows[3].observation,
        CostObservation::Execution {
            outcome: ExecutionOutcome::Unknown,
            ..
        }
    ));
    assert!(matches!(
        costs.costs.rows[2].observation,
        CostObservation::Missing
    ));
    assert!(costs.generation.costs.combined[1].limit_exceeded);
    assert_eq!(costs.costs.rows[1].individual_limit_exceeded, Some(true));
    assert_eq!(report.combined[1].known_lower_bound, 3002);
    assert!(report.combined[1].limit_exceeded && report.combined[1].unknown);
    assert_eq!(report.combined[4].known_lower_bound, 2002);
    assert!(report.combined[4].unknown);
    assert_eq!(costs.costs.matched_package_links, [false, false]);
}

#[test]
fn duplicate_attempt_stage_and_foreign_binding_are_rejected() {
    let a = Case::new("a", "same-operation", true);
    let b = Case::new("b", "same-operation", true);
    let chain = chain(&[&a, &b]);
    let (ba, bb) = (a.binding(&chain), b.binding(&chain));
    assert!(assess_chain_costs(&chain, &[a.input(&ba), a.input(&ba)]).is_err());
    assert!(assess_chain_costs(&chain, &[a.input(&ba), b.input(&bb)]).is_err());
    let other = ChainAllocation::new(
        "foreign-chain".into(),
        grant(6000),
        vec![a.stage.clone(), b.stage.clone()],
    )
    .unwrap();
    assert!(assess_chain_costs(&chain, &[a.input(&a.binding(&other))]).is_err());
    let mut altered = a.scope.clone();
    altered.total_grant.input_tokens += 1;
    assert!(assess_chain_costs(
        &chain,
        &[ChainStageEvidence::Generation {
            binding: &ba,
            allocation: a.allocation.as_ref().unwrap(),
            admission: a.admission.as_ref().unwrap(),
            scope: &altered,
            snapshots: &a.snapshots,
            trial: None,
        }]
    )
    .is_err());
}

#[test]
fn duplicate_trials_conflicting_versions_and_copied_invocations_are_rejected() {
    for change in ["exact-trial", "conflicting-trial", "invocation", "receipt"] {
        let a = Case::new("a", "method", false);
        let mut b = Case::new("b", "method", false);
        match change {
            "exact-trial" => b.trial = a.trial.clone(),
            "conflicting-trial" => b.trial.id = a.trial.id.clone(),
            "invocation" => {
                b.trial.slots[0].run_id = a.trial.slots[0].run_id.clone();
                b.trial.slots[0].attempt_id = a.trial.slots[0].attempt_id.clone();
            }
            _ => {}
        }
        b.rebuild_trial_only();
        if change == "receipt" {
            b.rows[0].execution = a.rows[0].execution.clone();
            b.rows[0].scoring = a.rows[0].scoring.clone();
        }
        let chain = chain(&[&a, &b]);
        let bindings = [a.binding(&chain), b.binding(&chain)];
        assert!(
            assess_chain_costs(&chain, &[a.input(&bindings[0]), b.input(&bindings[1])]).is_err(),
            "{change}"
        );
    }
}

#[test]
fn aggregate_overflow_preserves_known_lower_bound_and_individual_overruns() {
    let mut a = Case::new("a", "method", false);
    let mut b = Case::new("b", "method", false);
    for case in [&mut a, &mut b] {
        for row in &mut case.rows {
            row.scoring = None;
        }
        case.rows[0].execution.usage.input_tokens = ResourceUsage::Measured {
            value: u64::MAX / 2 + 1,
        };
    }
    let chain = chain(&[&a, &b]);
    let bindings = [a.binding(&chain), b.binding(&chain)];
    let report =
        assess_chain_costs(&chain, &[a.input(&bindings[0]), b.input(&bindings[1])]).unwrap();
    let input = &report.combined[1];
    assert_eq!(input.known_lower_bound, u64::MAX);
    assert!(input.overflow && input.limit_exceeded && input.unknown);
    assert!(!report.combined[0].overflow);
    for stage in &report.stages {
        let ChainObservedCosts::TrialOnly(costs) = stage.observed.as_ref().unwrap() else {
            panic!("trial-only report")
        };
        assert_eq!(costs.rows[0].individual_limit_exceeded, Some(true));
        assert!(!costs.combined[1].overflow);
    }
}
