use genesis_lab_eval::design::Design;
use genesis_lab_eval::score::Outcome;
use genesis_lab_eval_mixed::{family, score, sim};

fn five_wheel() -> Design {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../evaluator/designs/reference-five-wheel.json"
    ))
    .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn instances_are_deterministic_and_in_range() {
    for seed in 50000..50036 {
        let i = family::instance(seed);
        assert_eq!(i, family::instance(seed));
        assert_eq!(
            family::instance_digest(&i),
            family::instance_digest(&family::instance(seed))
        );
        assert_eq!((i.family.as_str(), i.version), ("mixed-terrain", 1));
        assert!((2.5..=4.5).contains(&i.bump_end));
        assert!((0.08..=0.3).contains(&i.bump_height));
        assert!((0.6..=1.4).contains(&i.bump_spacing));
        assert!((0.6..=1.8).contains(&i.gap));
        assert!((-0.15..=0.15).contains(&i.step_height));
        assert!((0.4..=0.9).contains(&i.friction));
        assert!(i.pushes.len() <= 2);
        assert_eq!(i.gap_start, i.bump_end + family::RUN_UP);
    }
}

#[test]
fn v1_is_unchanged_and_folded_into_the_identity() {
    // The v1 evaluator digest every retained v1 record carries.
    assert_eq!(
        genesis_lab_eval::score::evaluator_digest(),
        "1f03d1b6499721e6ddae6e600b688c123a8684e34c56f53cde8104d361246f42"
    );
    assert_ne!(
        score::evaluator_digest(),
        genesis_lab_eval::score::evaluator_digest()
    );
}

#[test]
fn an_episode_is_reproducible_and_never_exceeds_the_episode() {
    let d = five_wheel();
    let i = family::instance(50002);
    let (a, b) = (sim::run(&d, &i), sim::run(&d, &i));
    assert_eq!(a.trace_sha256, b.trace_sha256);
    assert_eq!(a.success, b.success);
    assert!(a.steps_run <= family::MAX_STEPS);
}

#[test]
fn receipts_and_refusals_follow_v1() {
    let d = five_wheel();
    let r = score::score_acceptance(&d, 50000).unwrap();
    assert_eq!(r.outcome, Outcome::Scored);
    assert_eq!(r.family, "mixed-terrain");
    assert_eq!(r.evaluator, "genesis-lab-eval-mixed@0.1.0");
    assert!(r.simulation_steps.unwrap() <= family::MAX_STEPS);
    assert!(matches!(
        score::experiment(&d, 50000, (family::MAX_STEPS - 1) as u64),
        Err(score::Refusal::ReservationTooSmall { .. })
    ));
    assert!(matches!(
        score::score_acceptance(&d, 1 << 63),
        Err(score::Refusal::HiddenInstance)
    ));
}
