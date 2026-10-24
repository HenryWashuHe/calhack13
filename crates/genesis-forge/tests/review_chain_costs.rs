//! Independent chain accounting checks over synthetic contract records only.
//! Fixture source is packaged as data; nothing is executed or sent to a provider.
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::TrialPlan;
use genesis_forge::method_accounting::CostScope;
use genesis_forge::method_chain::{ChainAllocation, ChainStage, ChainWork};
use genesis_forge::method_chain_costs::{
    assess_chain_costs, ChainObservedCosts, ChainStageEvidence,
};
use genesis_forge::method_stage::StageLimits;
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
fn trial(id: &str) -> TrialPlan {
    let mut plan = fixture::trial(&fixture::generation("fixture-only", true));
    plan.id = id.into();
    for slot in &mut plan.slots {
        slot.run_id = format!("{id}-{}", slot.slot_id);
        slot.attempt_id = format!("{id}-{}", slot.slot_id).into();
    }
    plan
}
fn stage(id: &str, plan: &TrialPlan) -> ChainStage {
    ChainStage {
        org_id: plan.slots[0].org_id.to_string(),
        limits: StageLimits {
            id: id.into(),
            max_trial_slots: 2,
            development_grant: plan.development_grant.clone(),
            scoring_grant: plan.scoring_grant.clone(),
            total_grant: grant(5000),
        },
        trial_model_settings: plan.model_settings.clone(),
        work: ChainWork::TrialOnly,
    }
}
fn scope(plan: &TrialPlan) -> CostScope {
    let mut scope = fixture::scope(&[], Some(plan));
    scope.total_grant = grant(5000);
    scope
}

#[test]
fn reversed_input_keeps_declaration_order_and_local_overrun_without_double_counting() {
    let mut a = trial("first");
    a.development_grant.model_calls = 0;
    let b = trial("second");
    let chain = ChainAllocation::new(
        "chain".into(),
        grant(10000),
        vec![stage("z-first", &a), stage("a-second", &b)],
    )
    .unwrap();
    let (sa, sb) = (scope(&a), scope(&b));
    let ba = chain.bind_trial_only("z-first", &a, &sa).unwrap();
    let bb = chain.bind_trial_only("a-second", &b, &sb).unwrap();
    let ea = [
        fixture::evidence(&a, "candidate"),
        fixture::evidence(&a, "control"),
    ];
    let eb = [
        fixture::evidence(&b, "candidate"),
        fixture::evidence(&b, "control"),
    ];
    let report = assess_chain_costs(
        &chain,
        &[
            ChainStageEvidence::TrialOnly {
                binding: &bb,
                scope: &sb,
                trial: &b,
                evidence: &eb,
            },
            ChainStageEvidence::TrialOnly {
                binding: &ba,
                scope: &sa,
                trial: &a,
                evidence: &ea,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        report
            .stages
            .iter()
            .map(|s| s.stage_id.as_str())
            .collect::<Vec<_>>(),
        ["z-first", "a-second"]
    );
    assert_eq!(a.slots[0].method, b.slots[0].method); // Equal packages do not deduplicate runs.
    for resource in &report.combined {
        assert_eq!(resource.known_lower_bound, 8);
        assert!(!resource.unknown && !resource.overflow && !resource.limit_exceeded);
    }
    let ChainObservedCosts::TrialOnly(first) = report.stages[0].observed.as_ref().unwrap() else {
        panic!("wrong stage kind")
    };
    assert_eq!(first.rows[0].individual_limit_exceeded, Some(true));
    assert_eq!(first.rows[2].individual_limit_exceeded, Some(true));
}

#[test]
fn saturation_keeps_unknown_costs_and_foreign_overrun_evidence_still_fails() {
    let (a, b) = (trial("first"), trial("second"));
    let chain = ChainAllocation::new(
        "chain".into(),
        grant(10000),
        vec![stage("first", &a), stage("second", &b)],
    )
    .unwrap();
    let (sa, sb) = (scope(&a), scope(&b));
    let ba = chain.bind_trial_only("first", &a, &sa).unwrap();
    let bb = chain.bind_trial_only("second", &b, &sb).unwrap();
    let failure = |plan: &TrialPlan| {
        let mut e = fixture::evidence(plan, "candidate");
        e.execution.outcome = ExecutionOutcome::Failed;
        e.execution.outputs.clear();
        e.execution.usage.wall_time_ms = ResourceUsage::Measured { value: u64::MAX };
        e.scoring = None;
        e
    };
    let ea = [failure(&a)];
    let mut eb = [failure(&b)];
    let assess = |eb: &[_]| {
        assess_chain_costs(
            &chain,
            &[
                ChainStageEvidence::TrialOnly {
                    binding: &ba,
                    scope: &sa,
                    trial: &a,
                    evidence: &ea,
                },
                ChainStageEvidence::TrialOnly {
                    binding: &bb,
                    scope: &sb,
                    trial: &b,
                    evidence: eb,
                },
            ],
        )
    };
    let report = assess(&eb).unwrap();
    let wall = &report.combined[3];
    assert_eq!(wall.known_lower_bound, u64::MAX);
    assert!(wall.unknown && wall.overflow && wall.limit_exceeded);
    assert_eq!(report.combined[0].known_lower_bound, 2);
    assert!(report.combined[0].unknown);
    eb[0].execution.actual_environment.id = "foreign".into();
    assert!(assess(&eb).is_err());
}

#[test]
fn run_and_attempt_labels_are_each_unique_within_an_organization() {
    let a = trial("first");
    for variant in ["same-run", "same-attempt", "different-org"] {
        let mut b = trial("second");
        for (old, new) in a.slots.iter().zip(&mut b.slots) {
            if variant != "same-attempt" {
                new.run_id = old.run_id.clone();
            }
            if variant != "same-run" {
                new.attempt_id = old.attempt_id.clone();
            }
            if variant == "different-org" {
                new.org_id = "org_other".into();
            }
        }
        let chain = ChainAllocation::new(
            "chain".into(),
            grant(10000),
            vec![stage("first", &a), stage("second", &b)],
        )
        .unwrap();
        let (sa, sb) = (scope(&a), scope(&b));
        let ba = chain.bind_trial_only("first", &a, &sa).unwrap();
        let bb = chain.bind_trial_only("second", &b, &sb).unwrap();
        let result = assess_chain_costs(
            &chain,
            &[
                ChainStageEvidence::TrialOnly {
                    binding: &ba,
                    scope: &sa,
                    trial: &a,
                    evidence: &[],
                },
                ChainStageEvidence::TrialOnly {
                    binding: &bb,
                    scope: &sb,
                    trial: &b,
                    evidence: &[],
                },
            ],
        );
        assert_eq!(result.is_ok(), variant == "different-org", "{variant}");
    }
}
