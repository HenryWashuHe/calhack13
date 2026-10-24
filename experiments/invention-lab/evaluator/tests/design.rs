//! Design language v1 validation and task-family generation. No simulation.
use genesis_lab_eval::design::Design;
use genesis_lab_eval::family::{instance, instance_digest, Family};

fn load(name: &str) -> Design {
    let path = format!("{}/designs/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn invalid_designs_are_refused_with_a_reason() {
    let rules = instance(Family::RavineCrossing, 0).rules;
    let base = serde_json::to_value(load("reference-five-wheel.json")).unwrap();
    assert!(load("reference-five-wheel.json").validate(&rules).is_ok());
    let mut cases: Vec<(&str, serde_json::Value)> = vec![];
    let mut v = base.clone();
    v["language"] = 2.into();
    cases.push(("language", v));
    let mut v = base.clone();
    v["bodies"][0]["x"] = 5.0.into();
    cases.push(("build zone", v));
    let mut v = base.clone();
    v["bodies"][0]["w"] = 3.9.into();
    v["bodies"][0]["h"] = 3.9.into();
    v["bodies"][0]["material"] = "steel".into();
    cases.push(("budget", v));
    let mut v = base.clone();
    v["bodies"][1]["x"] = v["bodies"][2]["x"].clone();
    cases.push(("overlap", v));
    let mut v = base.clone();
    v["joints"][0]["kind"] = "weld".into();
    cases.push(("motor on weld", v));
    let mut v = base.clone();
    v["payload_on"] = "nothing".into();
    cases.push(("payload", v));
    for (what, value) in cases {
        let d: Design = serde_json::from_value(value).unwrap();
        assert!(d.validate(&rules).is_err(), "{what}");
    }
    // Materials are a closed table: an unknown one doesn't parse.
    let mut v = base;
    v["bodies"][0]["material"] = "unobtainium".into();
    assert!(serde_json::from_value::<Design>(v).is_err());
}

#[test]
fn design_digest_is_stable_over_its_normalized_bytes() {
    let d = load("reference-five-wheel.json");
    let again: Design = serde_json::from_slice(&d.canonical()).unwrap();
    assert_eq!(d.canonical(), again.canonical());
    assert_eq!(d.digest(), again.digest());
    assert_eq!(d.digest().len(), 64);
    assert_ne!(d.digest(), load("cart-long.json").digest());
}

#[test]
fn families_are_deterministic_and_draw_different_worlds() {
    for f in Family::all() {
        assert_eq!(instance(f, 7), instance(f, 7));
        assert_eq!(
            instance_digest(&instance(f, 7)),
            instance_digest(&instance(f, 7))
        );
        assert_ne!(
            instance_digest(&instance(f, 7)),
            instance_digest(&instance(f, 8))
        );
        assert_eq!(Family::parse(f.name()), Some(f));
    }
    let a = instance(Family::RavineCrossing, 7);
    let b = instance(Family::ReturnMix, 7);
    assert_ne!(a.friction, b.friction);
    assert!(!b.pushes.is_empty());
    assert!(a.pushes.is_empty());
    assert!((1.0..=2.5).contains(&a.gap));
}
