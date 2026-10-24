//! Independent boundary for the final generation-grant addition.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::MethodSynthesisSession,
};
#[path = "common/method_plan.rs"]
mod fixture;

fn grant(model_calls: u64, output_tokens: u64) -> ResourceGrant {
    ResourceGrant {
        model_calls,
        input_tokens: 0,
        output_tokens,
        wall_time_ms: 0,
        simulation_steps: 0,
    }
}

#[test]
fn full_generation_allowance_must_fit_final_sum_without_saturation() {
    let prepared = MethodSynthesisSession::new(fixture::plan()).unwrap();
    let output_cap = u64::from(prepared.plan().settings.max_tokens);
    let limits = StageLimits {
        id: "last-addition-boundary".into(),
        max_trial_slots: 2,
        development_grant: grant(u64::MAX / 2, 0),
        scoring_grant: grant(0, 0),
        total_grant: grant(u64::MAX, output_cap),
    };
    // Two slot grants sum to MAX-1. A one-call generation grant exactly fits;
    // a two-call grant overflows even though the roster still has ONE attempt.
    for generation_allowance in [1, 2] {
        let admission = GenerationAdmission::new(
            GenerationRoster::freeze(
                "one-attempt".into(),
                prepared.plan().settings.clone(),
                grant(generation_allowance, output_cap),
                std::slice::from_ref(&prepared),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(admission.roster().attempts().len(), 1);
        let before = serde_json::to_vec(&(&admission, &prepared)).unwrap();
        let result =
            StageAllocation::prepare(limits.clone(), &admission, std::slice::from_ref(&prepared));
        if generation_allowance == 1 {
            let allocation = result.expect("exact u64::MAX is representable");
            let restored: StageAllocation =
                serde_json::from_slice(&serde_json::to_vec(&allocation).unwrap()).unwrap();
            assert_eq!(allocation.sha256().unwrap(), restored.sha256().unwrap());
        } else {
            assert!(
                result.is_err(),
                "the full generation grant must not saturate"
            );
        }
        assert_eq!(
            before,
            serde_json::to_vec(&(&admission, &prepared)).unwrap()
        );
    }
}
