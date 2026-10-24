//! Independent boundary regressions for chain generation caps: a stage whose
//! attempts exactly use its model-call and output-token grant is valid, and
//! one unit less is refused. A mutation pass found both equalities unpinned.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_chain::{ChainAllocation, ChainStage, ChainWork},
    method_stage::StageLimits,
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

/// A generation stage with `attempts` attempts under `generation` and
/// whole-stage ceilings large enough not to matter.
fn chain_with(attempts: u32, generation: ResourceGrant) -> Result<ChainAllocation, String> {
    let generated = fixture::generation("one", true);
    let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
    let trial = fixture::trial(&generated);
    let total = grant(1 << 40);
    let stage = ChainStage {
        org_id: "org_a".into(),
        limits: StageLimits {
            id: "develop-a".into(),
            max_trial_slots: 2,
            development_grant: trial.development_grant.clone(),
            scoring_grant: trial.scoring_grant.clone(),
            total_grant: total.clone(),
        },
        trial_model_settings: trial.model_settings.clone(),
        work: ChainWork::Generation {
            settings: prepared.plan().settings.clone(),
            max_attempts: attempts,
            grant: generation,
        },
    };
    ChainAllocation::new("chain".into(), total, vec![stage]).map_err(|e| e.to_string())
}

fn max_tokens() -> u64 {
    let generated = fixture::generation("one", true);
    u64::from(generated.plan().settings.max_tokens)
}

#[test]
fn attempts_may_use_exactly_the_granted_model_calls() {
    let mut g = grant(1 << 30);
    g.model_calls = 3;
    g.output_tokens = 3 * max_tokens();
    chain_with(3, g.clone()).expect("three attempts fit three granted calls");
    g.model_calls = 2;
    assert!(chain_with(3, g).is_err(), "three attempts exceed two calls");
}

#[test]
fn attempts_may_use_exactly_the_granted_output_tokens() {
    let mut g = grant(1 << 30);
    g.model_calls = 2;
    g.output_tokens = 2 * max_tokens();
    chain_with(2, g.clone()).expect("two full-length attempts fit exactly");
    g.output_tokens -= 1;
    assert!(
        chain_with(2, g).is_err(),
        "one output token short is refused"
    );
}
