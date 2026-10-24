//! Deterministic simulation and the commit/reveal hidden draw.
use genesis_lab_eval::commit::Descriptor;
use genesis_lab_eval::design::Design;
use genesis_lab_eval::family::{instance, Family};
use genesis_lab_eval::sim::run;

fn load(name: &str) -> Design {
    let path = format!("{}/designs/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn same_design_and_instance_give_the_same_trace() {
    let d = load("reference-five-wheel.json");
    for f in Family::all() {
        let a = run(&d, &instance(f, 3));
        let b = run(&d, &instance(f, 3));
        assert_eq!(a.trace_sha256, b.trace_sha256, "{f:?}");
        assert_eq!(a.steps_run, b.steps_run, "{f:?}");
        assert!(a.steps_run > 0 && a.steps_run <= f.max_steps(), "{f:?}");
        assert!(!a.diverged, "{f:?}");
    }
    let other = run(&d, &instance(Family::RavineCrossing, 4));
    assert_ne!(
        other.trace_sha256,
        run(&d, &instance(Family::RavineCrossing, 3)).trace_sha256
    );
}

#[test]
fn a_long_cart_never_crosses_a_ravine() {
    let d = load("cart-long.json");
    for seed in 0..5 {
        let e = run(&d, &instance(Family::RavineCrossing, seed));
        assert!(!e.success, "seed {seed}");
        assert!(e.steps_to_target.is_none(), "seed {seed}");
    }
}

#[test]
fn hidden_draws_are_bound_to_an_immutable_descriptor() {
    let f = Family::ReturnMix;
    let set = Descriptor::new("cmp-1", "secret-salt", f, 5);
    assert_eq!(set, Descriptor::new("cmp-1", "secret-salt", f, 5));
    assert!(set.opens("secret-salt") && !set.opens("other-salt"));
    // Comparison id, salt and count are all bound.
    assert_ne!(
        set.commitment,
        Descriptor::new("cmp-2", "secret-salt", f, 5).commitment
    );
    assert_ne!(
        set.commitment,
        Descriptor::new("cmp-1", "other-salt", f, 5).commitment
    );
    assert_ne!(
        set.commitment,
        Descriptor::new("cmp-1", "secret-salt", f, 6).commitment
    );
    // Fields are length-prefixed, so moving a separator can't collide.
    assert_ne!(
        Descriptor::new("a:b", "c", f, 5).commitment,
        Descriptor::new("a", "b:c", f, 5).commitment
    );
    // A descriptor edited after publication no longer opens.
    let mut edited = set.clone();
    edited.count = 6;
    assert!(!edited.opens("secret-salt"));
    let seeds: Vec<u64> = (0..5).map(|i| set.hidden_seed("secret-salt", i)).collect();
    assert!(seeds.iter().all(|s| *s >= 1 << 63));
    let mut unique = seeds.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 5);
    let other = Descriptor::new("cmp-2", "secret-salt", f, 5);
    assert_ne!(seeds[0], other.hidden_seed("secret-salt", 0));
    // The descriptor round-trips without loss and rejects unknown fields.
    let json = serde_json::to_value(&set).unwrap();
    assert_eq!(
        serde_json::from_value::<Descriptor>(json.clone()).unwrap(),
        set
    );
    let mut extra = json;
    extra["salt"] = "secret-salt".into();
    assert!(serde_json::from_value::<Descriptor>(extra).is_err());
}
