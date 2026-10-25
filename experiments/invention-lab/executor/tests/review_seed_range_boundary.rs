//! Independent PR #121 review at 7200921e4000d0c531cbcc9d1abc94d201b62d39.
//! Plan metadata only: no executor, scorer, subprocess, physics or provider calls.
use genesis_core::development_method::ResourceGrant;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::{example, refs, sandbox::Mode};

fn plan(first: u64, count: u64) -> genesis_core::development_trial::TrialPlan {
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "candidate", "1", b"candidate"),
        control: refs::content("lab-method/1", "control", "1", b"control"),
    };
    example::acceptance_plan_from(
        Family::RoughTransport,
        &methods,
        Mode::None,
        "Python metadata fixture",
        first,
        count,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 10_000,
            simulation_steps: 2400,
        },
    )
}

// Rejection may happen in this infallible example constructor (a documented
// panic already rejects arithmetic overflow) or in its returned plan's validity.
fn assert_rejected(first: u64, count: u64) {
    if let Ok(plan) = std::panic::catch_unwind(|| plan(first, count)) {
        let unresolved: Vec<_> = plan
            .development_instances
            .iter()
            .filter(|r| refs::resolve_instance(r).is_none())
            .map(|r| r.id.clone())
            .collect();
        assert!(
            plan.validate().is_err(),
            "accepted a public plan containing unresolved instances: {unresolved:?}"
        );
    }
}

#[test]
fn last_public_seed_remains_valid_and_resolvable() {
    let plan = plan((1_u64 << 63) - 1, 1);
    plan.validate().unwrap();
    assert_eq!(plan.development_instances.len(), 1);
    assert!(refs::resolve_instance(&plan.development_instances[0]).is_some());
}

#[test]
fn entirely_reserved_range_must_be_rejected() {
    assert_rejected(1_u64 << 63, 1);
}

#[test]
fn range_crossing_reserved_boundary_must_be_rejected() {
    assert_rejected((1_u64 << 63) - 1, 2);
}

#[test]
fn arithmetic_overflow_is_rejected_before_range_allocation() {
    assert_rejected(u64::MAX, 1);
}
