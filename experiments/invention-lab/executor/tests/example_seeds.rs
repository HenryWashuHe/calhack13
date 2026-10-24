//! A separate instance split is a seed range of the same example plan.
use genesis_core::development_method::ResourceGrant;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::{example, refs, sandbox::Mode};

fn methods() -> example::ExampleMethods {
    example::ExampleMethods {
        candidate: refs::content("lab-method/1", "cand", "1", b"cand"),
        control: refs::content("lab-method/1", "ctrl", "1", b"ctrl"),
    }
}

fn grant() -> ResourceGrant {
    ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: 2400,
    }
}

#[test]
fn seed_zero_is_exactly_the_existing_plan() {
    let m = methods();
    let a = example::acceptance_plan_for(Family::ReturnMix, &m, Mode::None, "Python 3", 4, grant());
    let b =
        example::acceptance_plan_from(Family::ReturnMix, &m, Mode::None, "Python 3", 0, 4, grant());
    assert_eq!(a.reference().unwrap(), b.reference().unwrap());
}

#[test]
fn a_later_seed_range_is_a_disjoint_valid_split() {
    let m = methods();
    let dev = example::acceptance_plan_for(
        Family::RoughTransport,
        &m,
        Mode::None,
        "Python 3",
        6,
        grant(),
    );
    let assess = example::acceptance_plan_from(
        Family::RoughTransport,
        &m,
        Mode::None,
        "Python 3",
        1000,
        12,
        grant(),
    );
    assess.validate().unwrap();
    let ids: Vec<&str> = assess
        .development_instances
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids.len(), 12);
    assert_eq!(ids[0], "rough-transport/1/1000");
    assert_eq!(ids[11], "rough-transport/1/1011");
    for r in &assess.development_instances {
        assert!(refs::resolve_instance(r).is_some(), "{}", r.id);
        assert!(!dev.development_instances.contains(r));
    }
    assert_ne!(assess.context.instance_set, dev.context.instance_set);
    assert_ne!(assess.id, dev.id);
    assert_eq!(assess.scoring_grant.simulation_steps, 12 * 2400);
}
