//! Henry's Claude: scoring-primitive tests from docs/invention-lab/evaluator.md,
//! plus reservation, purpose and digest checks. Local simulation only.
use genesis_lab_eval::commit::Descriptor;
use genesis_lab_eval::design::Design;
use genesis_lab_eval::family::{instance, Family};
use genesis_lab_eval::score::{
    experiment, is_hidden_seed, score_acceptance, score_comparison, Outcome, Purpose, Refusal,
};

fn load(name: &str) -> Design {
    let path = format!("{}/designs/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn brick() -> Design {
    serde_json::from_value(serde_json::json!({
        "language": 1, "payload_on": "b",
        "bodies": [{"shape":"rect","id":"b","x":-1.0,"y":0.3,"w":0.5,"h":0.5,"material":"wood"}]
    }))
    .unwrap()
}

#[test]
fn same_design_instance_and_version_give_the_same_trace() {
    let d = load("reference-five-wheel.json");
    for f in Family::all() {
        let a = score_acceptance(&d, f, 3).unwrap();
        let b = score_acceptance(&d, f, 3).unwrap();
        assert_eq!(a, b, "{f:?}");
        assert!(a.trace_sha256.is_some());
    }
    // A different instance gives a different trace.
    let c = score_acceptance(&d, Family::RavineCrossing, 4).unwrap();
    assert_ne!(
        c.trace_sha256,
        score_acceptance(&d, Family::RavineCrossing, 3)
            .unwrap()
            .trace_sha256
    );
}

#[test]
fn invalid_designs_are_refused_without_simulating() {
    let base = serde_json::to_value(load("reference-five-wheel.json")).unwrap();
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
    v["joints"] = serde_json::json!([]);
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
        let r = score_acceptance(&d, Family::RavineCrossing, 0).unwrap();
        assert_eq!(r.outcome, Outcome::Invalid, "{what}");
        assert!(r.reason.is_some(), "{what}");
        assert_eq!(r.simulation_steps, Some(0), "{what}");
        assert!(r.metrics.is_none() && r.trace_sha256.is_none(), "{what}");
    }
    // Materials are a closed table: an unknown one doesn't parse.
    let mut v = base;
    v["bodies"][0]["material"] = "unobtainium".into();
    assert!(serde_json::from_value::<Design>(v).is_err());
}

#[test]
fn experiments_refuse_hidden_instances_and_short_reservations_and_measure_steps() {
    let d = load("reference-five-wheel.json");
    let hidden = Descriptor::new("cmp", "salt", Family::RavineCrossing, 1).hidden_seed("salt", 0);
    assert_eq!(
        experiment(&d, Family::RavineCrossing, hidden, u64::MAX).unwrap_err(),
        Refusal::HiddenInstance
    );
    assert_eq!(
        score_acceptance(&d, Family::RavineCrossing, hidden).unwrap_err(),
        Refusal::HiddenInstance
    );
    let max = Family::RavineCrossing.max_steps() as u64;
    assert!(matches!(
        experiment(&d, Family::RavineCrossing, 0, max - 1),
        Err(Refusal::ReservationTooSmall { .. })
    ));
    // No balance is kept here: two identical calls each measure the same
    // steps, and charging them cumulatively is the trusted ledger's job.
    let r = experiment(&d, Family::RavineCrossing, 0, max).unwrap();
    assert_eq!(r.receipt.purpose, Purpose::Experiment);
    let used = r.receipt.simulation_steps.unwrap() as u64;
    assert!(used > 0 && used <= max);
    assert!(!r.frames.is_empty());
    assert_eq!(experiment(&d, Family::RavineCrossing, 0, max).unwrap(), r);
}

#[test]
fn a_design_that_misses_its_task_is_scored_not_failed() {
    let d = load("cart-long.json");
    let r = score_acceptance(&d, Family::RavineCrossing, 0).unwrap();
    assert_eq!(r.outcome, Outcome::Scored);
    assert!(!r.metrics.unwrap().success);
}

#[test]
fn comparison_scoring_needs_the_salt_that_opens_the_descriptor() {
    let d = load("reference-five-wheel.json");
    let f = Family::ReturnMix;
    let set = Descriptor::new("cmp-1", "secret-salt", f, 5);
    let c = set.commitment.clone();
    let seeds: Vec<u64> = (0..5).map(|i| set.hidden_seed("secret-salt", i)).collect();
    assert!(seeds.iter().all(|s| is_hidden_seed(*s)));
    assert_eq!(
        score_comparison(&d, &set, "wrong-salt", 0).unwrap_err(),
        Refusal::WrongSalt
    );
    assert_eq!(
        score_comparison(&d, &set, "secret-salt", 5).unwrap_err(),
        Refusal::IndexOutOfRange
    );
    let r = score_comparison(&d, &set, "secret-salt", 0).unwrap();
    assert_eq!(r.purpose, Purpose::Comparison);
    assert_eq!(r.seed, seeds[0]);
    assert_eq!(r.hidden_commitment.as_deref(), Some(c.as_str()));
    // After the reveal anyone regenerates the same instance and score.
    assert_eq!(instance(f, seeds[0]), instance(f, r.seed));
    assert_eq!(score_comparison(&d, &set, "secret-salt", 0).unwrap(), r);
}

#[test]
fn a_trivial_design_fails_everywhere_and_the_reference_crosses_some_ravines() {
    let b = brick();
    for f in Family::all() {
        for seed in 0..5 {
            let r = score_acceptance(&b, f, seed).unwrap();
            assert_eq!(r.outcome, Outcome::Scored, "{f:?} {seed}");
            assert!(!r.metrics.unwrap().success, "{f:?} {seed}");
        }
    }
    // Sanity only: the hand-built reference is never a method or a baseline.
    let d = load("reference-five-wheel.json");
    let crossed = (0..20)
        .filter(|s| {
            score_acceptance(&d, Family::RavineCrossing, *s)
                .unwrap()
                .metrics
                .is_some_and(|m| m.success)
        })
        .count();
    assert!((5..20).contains(&crossed), "reference crossed {crossed}/20");
}

#[test]
fn the_evaluator_digest_covers_every_scoring_source_file() {
    let root = env!("CARGO_MANIFEST_DIR");
    let covered = genesis_lab_eval::score::digest_files();
    for entry in std::fs::read_dir(format!("{root}/src")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".rs") {
            assert!(
                covered.contains(&format!("src/{name}").as_str()),
                "src/{name} decides results but isn't in the evaluator digest"
            );
        }
    }
    assert!(covered.contains(&"Cargo.lock"));
    let d = genesis_lab_eval::score::evaluator_digest();
    assert_eq!(d.len(), 64);
    assert_eq!(d, genesis_lab_eval::score::evaluator_digest());
}
