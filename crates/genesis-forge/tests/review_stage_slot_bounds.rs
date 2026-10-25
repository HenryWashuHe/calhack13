//! Henry's Claude, review of #153: once zero slots moved to their own check,
//! nothing pinned the remaining identity and 64-slot bounds of
//! `validate_limits` (mutation: deleting that guard, or `> 64` → `>= 64`,
//! passed every stage/chain target). Inert records only; nothing runs.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
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

#[test]
fn stage_identity_and_slot_count_bounds_are_exact() {
    let generated = fixture::generation("slot-bounds", true);
    let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
    let admission = GenerationAdmission::new(
        GenerationRoster::freeze(
            "roster".into(),
            prepared.plan().settings.clone(),
            grant(2000),
            std::slice::from_ref(&prepared),
        )
        .unwrap(),
    )
    .unwrap();
    let prepare = |id: String, slots: u32| {
        StageAllocation::prepare(
            StageLimits {
                id,
                max_trial_slots: slots,
                development_grant: grant(1),
                scoring_grant: grant(1),
                total_grant: grant(2200),
            },
            &admission,
            std::slice::from_ref(&prepared),
        )
    };
    assert!(prepare("stage".into(), 64).is_ok());
    assert!(prepare("stage".into(), 65).is_err());
    assert!(prepare("s".repeat(128), 1).is_ok());
    assert!(prepare("s".repeat(129), 1).is_err());
    assert!(prepare(String::new(), 1).is_err());
}
