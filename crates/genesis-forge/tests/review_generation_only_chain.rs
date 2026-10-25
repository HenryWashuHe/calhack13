//! Independent zero-trial boundary checks over inert contract records only.
//! Historical source is parsed as data; no model or generated source is run.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_accounting::CostScope,
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStage, ChainStageBinding, ChainWork},
    method_chain_costs::{assess_chain_costs, ChainStageEvidence},
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::MethodSynthesisSession,
};
#[allow(dead_code)]
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
    prepared: MethodSynthesisSession,
    admission: GenerationAdmission,
    allocation: StageAllocation,
    chain: ChainAllocation,
    binding: ChainStageBinding,
    scope: CostScope,
}
impl Case {
    fn new() -> Self {
        let generated = fixture::generation("source-only", true);
        let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
        let mut admission = GenerationAdmission::new(
            GenerationRoster::freeze(
                "roster".into(),
                prepared.plan().settings.clone(),
                grant(2000),
                std::slice::from_ref(&prepared),
            )
            .unwrap(),
        )
        .unwrap();
        let limits = StageLimits {
            id: "source".into(),
            max_trial_slots: 0,
            development_grant: grant(0),
            scoring_grant: grant(0),
            total_grant: grant(2000),
        };
        let allocation =
            StageAllocation::prepare(limits.clone(), &admission, std::slice::from_ref(&prepared))
                .unwrap();
        let chain = ChainAllocation::new(
            "chain".into(),
            grant(2000),
            vec![ChainStage {
                org_id: prepared.plan().org_id.clone(),
                limits,
                trial_model_settings: fixture::trial(&generated).model_settings,
                work: ChainWork::Generation {
                    settings: prepared.plan().settings.clone(),
                    max_attempts: 1,
                    grant: grant(2000),
                },
            }],
        )
        .unwrap();
        let binding = chain.bind_generation("source", &allocation).unwrap();
        let mut scope = fixture::scope(std::slice::from_ref(&prepared), None);
        scope.total_grant = grant(2000);
        admission.reserve(&prepared).unwrap();
        Self {
            prepared,
            admission,
            allocation,
            chain,
            binding,
            scope,
        }
    }
    fn input<'a>(&'a self, snapshots: &'a [MethodSynthesisSession]) -> ChainStageEvidence<'a> {
        ChainStageEvidence::Generation {
            binding: &self.binding,
            allocation: &self.allocation,
            admission: &self.admission,
            scope: &self.scope,
            snapshots,
            trial: None,
        }
    }
}

#[test]
fn zero_slots_reject_trial_only_and_every_nonzero_trial_resource() {
    let c = Case::new();
    let mut trial_only = c.chain.stages()[0].clone();
    trial_only.work = ChainWork::TrialOnly;
    assert!(ChainAllocation::new("trial-only".into(), grant(2000), vec![trial_only]).is_err());
    let mut encoded = serde_json::to_value(&c.chain).unwrap();
    encoded["stages"][0]["work"] = serde_json::json!({"kind":"trial_only"});
    assert!(serde_json::from_value::<ChainAllocation>(encoded).is_err());

    let unused = GenerationAdmission::new(c.admission.roster().clone()).unwrap();
    for kind in ["development_grant", "scoring_grant"] {
        for dimension in [
            "model_calls",
            "input_tokens",
            "output_tokens",
            "wall_time_ms",
            "simulation_steps",
        ] {
            let mut value = serde_json::to_value(c.allocation.limits()).unwrap();
            value[kind][dimension] = 1.into();
            let limits: StageLimits = serde_json::from_value(value).unwrap();
            assert!(
                StageAllocation::prepare(
                    limits.clone(),
                    &unused,
                    std::slice::from_ref(&c.prepared)
                )
                .is_err(),
                "{kind}/{dimension}"
            );
            let mut stage = c.chain.stages()[0].clone();
            stage.limits = limits;
            assert!(ChainAllocation::new("bad".into(), grant(2000), vec![stage]).is_err());
            let mut encoded = serde_json::to_value(&c.allocation).unwrap();
            encoded["limits"][kind][dimension] = 1.into();
            assert!(serde_json::from_value::<StageAllocation>(encoded).is_err());
        }
    }
}

#[test]
fn explicit_no_trial_never_turns_missing_or_uncertain_generation_into_zero_cost() {
    let c = Case::new();
    let omitted = assess_chain_costs(&c.chain, &[]).unwrap();
    assert!(omitted.stages[0].unresolved_trial);
    assert!(omitted.stages[0].observed.is_none());
    assert!(omitted
        .combined
        .iter()
        .all(|v| v.unknown && v.known_lower_bound == 0));
    let mut pending = c.prepared.clone();
    pending.begin().unwrap();
    let mut unknown = pending.clone();
    assert!(unknown.recover_interrupted().unwrap());
    for snapshots in [
        vec![],
        vec![c.prepared.clone()],
        vec![pending],
        vec![unknown],
    ] {
        let report = assess_chain_costs(&c.chain, &[c.input(&snapshots)]).unwrap();
        assert!(!report.stages[0].unresolved_trial); // No trial exists; generation is still unknown.
        assert!(report.stages[0].observed.is_some());
        assert!(report.combined[..4]
            .iter()
            .all(|v| v.unknown && v.known_lower_bound == 0));
    }
}

#[test]
fn zero_slot_allocation_cannot_be_used_to_bind_a_real_trial() {
    let c = Case::new();
    let mut trial = fixture::trial(&fixture::generation("source-only", true));
    trial.development_grant = grant(0);
    trial.scoring_grant = grant(0);
    for slot in &mut trial.slots {
        slot.org_id = c.prepared.plan().org_id.clone().into();
    }
    let mut scope = fixture::scope(std::slice::from_ref(&c.prepared), Some(&trial));
    scope.total_grant = c.scope.total_grant.clone();
    trial.reference().unwrap();
    scope.validate().unwrap();
    assert!(c.allocation.bind(&trial, &scope).is_err());
    assert!(c.chain.bind_trial_only("source", &trial, &scope).is_err());
    assert!(c
        .binding
        .validate_trial(&c.chain, None, &trial, &scope)
        .is_err());
}

#[test]
fn retained_nonzero_source_stage_keeps_exact_identity_and_unresolved_future_trial() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/invention-lab/evidence/live-method-20261024-b-revision");
    let read = |name| std::fs::read(path.join(name)).unwrap();
    let chain: ChainAllocation = serde_json::from_slice(&read("chain.json")).unwrap();
    let allocation: StageAllocation = serde_json::from_slice(&read("allocation.json")).unwrap();
    let admission: GenerationAdmission = serde_json::from_slice(&read("admission.json")).unwrap();
    let generation: MethodSynthesisSession =
        serde_json::from_slice(&read("generation.json")).unwrap();
    let binding: ChainStageBinding =
        serde_json::from_slice(&read("chain.json.stage-revise-b-source.json")).unwrap();
    assert_eq!(
        chain.sha256().unwrap(),
        "efac55be8a2bf09ca0b7d23d05e230442c1c25e07e4776b0a1f3666436e957a4"
    );
    assert_eq!(
        allocation.sha256().unwrap(),
        "0252f80d8b5dfdeb426a071a6c44c7703cc1cd072e5f13253c101498cb10398c"
    );
    assert_eq!(allocation.limits().max_trial_slots, 1);
    assert_eq!(allocation.limits().development_grant, grant(0));
    assert_eq!(allocation.limits().scoring_grant, grant(0));
    let scope = CostScope {
        id: "retained-source-only".into(),
        generations: admission.roster().attempts().to_vec(),
        trial: None,
        generated_links: vec![],
        total_grant: allocation.limits().total_grant.clone(),
    };
    let report = assess_chain_costs(
        &chain,
        &[ChainStageEvidence::Generation {
            binding: &binding,
            allocation: &allocation,
            admission: &admission,
            scope: &scope,
            snapshots: &[generation],
            trial: None,
        }],
    )
    .unwrap();
    assert!(report.stages[0].unresolved_trial);
    assert!(report.combined.iter().all(|v| v.unknown));
    assert_eq!(
        report
            .combined
            .iter()
            .map(|v| v.known_lower_bound)
            .collect::<Vec<_>>(),
        [1, 20798, 5741, 40223, 0]
    );
}
