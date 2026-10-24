//! Comparison scoring on a committed hidden set, after every arm has finished.
//! In-process; the methods are hand-written controls, not generated methods.
use genesis_core::development_evaluation::{EvaluationOutcome, RequestStatus};
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage,
};
use genesis_core::development_trial::{assess_trials, TrialEvidence, TrialPlan};
use genesis_lab_eval::commit::Descriptor;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::hidden::{comparison_plan, score_hidden};
use genesis_lab_score::ScoringError;
use serde_json::json;
use std::path::PathBuf;

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");
const SALT: &str = "salt-held-by-the-evaluator-owner";

fn descriptor(count: u32) -> Descriptor {
    Descriptor::new("lab-comparison-1", SALT, Family::RavineCrossing, count)
}

fn plan(count: u32) -> TrialPlan {
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "cand", "1", b"cand"),
        control: refs::content("lab-method/1", "ctrl", "1", b"ctrl"),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: 2400,
    };
    let base = example::acceptance_plan(&methods, Mode::None, "Python 3", 2, grant);
    comparison_plan(&base, "t1", &descriptor(count))
}

fn dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lab-hidden-{name}-{}", std::process::id()))
}

/// Runs one arm in-process, submitting `design` when given.
fn run_arm(p: &TrialPlan, name: &str, slot: &str, design: Option<&str>) -> ExecutionReceipt {
    let root = dir(name).join(slot);
    let _ = std::fs::remove_dir_all(&root);
    let mut s = Session {
        plan: p,
        slot_id: slot,
        store: Store::open(&root).unwrap(),
        ledger: ExperimentLedger::new(ExperimentScope {
            plan: p,
            slot_id: slot,
        })
        .unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding(slot).unwrap().method),
            ..Default::default()
        },
    };
    let outcome = match design {
        Some(d) => {
            let d: serde_json::Value = serde_json::from_str(d).unwrap();
            let reply = s
                .handle(&json!({"op":"submit","design":d}).to_string())
                .unwrap();
            assert_eq!(reply["ok"], true, "{reply}");
            ExecutionOutcome::OutputGenerated
        }
        None => ExecutionOutcome::Failed,
    };
    finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        outcome,
        ResourceUsage::Measured { value: 5 },
    )
    .unwrap()
    .receipt
}

fn output(name: &str, slot: &str, r: &ExecutionReceipt) -> Vec<u8> {
    std::fs::read(
        dir(name)
            .join(slot)
            .join("artifacts")
            .join(&r.outputs[0].sha256),
    )
    .unwrap()
}

fn score_store(name: &str) -> Store {
    let d = dir(name).join("score");
    let _ = std::fs::remove_dir_all(&d);
    Store::open(&d).unwrap()
}

fn nothing_simulated(store: &Store) -> bool {
    std::fs::read_dir(store.path("artifacts"))
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

#[test]
fn a_closed_comparison_is_scored_on_the_committed_draw_and_validates() {
    let p = plan(3);
    p.validate().unwrap();
    assert!(
        p.development_instances
            .iter()
            .all(|r| refs::resolve_instance(r).is_some()),
        "development stays on public instances"
    );
    let cand = run_arm(&p, "ok", "candidate", Some(REFERENCE));
    let ctrl = run_arm(&p, "ok", "control", None);
    let store = score_store("ok");
    let closure = [("candidate", &cand), ("control", &ctrl)];
    let receipt = score_hidden(
        &p,
        "candidate",
        &output("ok", "candidate", &cand),
        &closure,
        &descriptor(3),
        SALT,
        &store,
    )
    .unwrap();
    let EvaluationOutcome::Scored { metrics } = &receipt.outcome else {
        panic!("{:?}", receipt.outcome)
    };
    let m: serde_json::Value = serde_json::from_slice(
        &store
            .read(&format!("artifacts/{}", metrics.sha256))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(m["of"], 3);
    let ids: Vec<_> = m["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["instance"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["hidden/0", "hidden/1", "hidden/2"]);

    // The private trace names hidden seeds under the published commitment.
    let trace: serde_json::Value = serde_json::from_slice(
        &store
            .read(&format!("artifacts/{}", receipt.trace.sha256))
            .unwrap(),
    )
    .unwrap();
    for row in trace["runs"].as_array().unwrap() {
        let r = &row["receipt"];
        assert_eq!(r["hidden_commitment"], descriptor(3).commitment.as_str());
        assert!(genesis_lab_eval::score::is_hidden_seed(
            r["seed"].as_u64().unwrap()
        ));
    }
    match receipt.usage.simulation_steps {
        ResourceUsage::Measured { value } => assert!(value > 0 && value <= 3 * 2400),
        ref other => panic!("{other:?}"),
    }

    let rows = assess_trials(
        &p,
        &[
            TrialEvidence {
                slot_id: "candidate".into(),
                execution: cand,
                scoring: Some(receipt),
            },
            TrialEvidence {
                slot_id: "control".into(),
                execution: ctrl,
                scoring: None,
            },
        ],
    )
    .unwrap();
    assert_eq!(rows[0].scoring_status, RequestStatus::Scored);
    assert_eq!(rows[1].scoring_status, RequestStatus::Missing);
}

#[test]
fn hidden_scoring_refuses_before_simulating_anything() {
    let p = plan(3);
    let cand = run_arm(&p, "refuse", "candidate", Some(REFERENCE));
    let ctrl = run_arm(&p, "refuse", "control", Some(REFERENCE));
    let bytes = output("refuse", "candidate", &cand);
    let closed = [("candidate", &cand), ("control", &ctrl)];
    let refused = |name: &str,
                   p: &TrialPlan,
                   closure: &[(&str, &ExecutionReceipt)],
                   d: &Descriptor,
                   salt: &str| {
        let store = score_store(name);
        let r = score_hidden(p, "candidate", &bytes, closure, d, salt, &store);
        assert!(matches!(r, Err(ScoringError::Refused(_))), "{name}: {r:?}");
        assert!(
            nothing_simulated(&store),
            "{name}: simulated after refusing"
        );
    };

    refused("salt", &p, &closed, &descriptor(3), "a guessed salt");
    // A different descriptor than the plan pinned (another draw size).
    refused("descriptor", &p, &closed, &descriptor(4), SALT);
    // Not closed: the control arm hasn't finished, or appears twice.
    refused("open", &p, &[("candidate", &cand)], &descriptor(3), SALT);
    refused(
        "twice",
        &p,
        &[("candidate", &cand), ("control", &ctrl), ("control", &ctrl)],
        &descriptor(3),
        SALT,
    );
    refused(
        "stranger",
        &p,
        &[("candidate", &cand), ("control", &ctrl), ("other", &ctrl)],
        &descriptor(3),
        SALT,
    );
    // One episode short of the draw.
    let mut short = p.clone();
    short.scoring_grant.simulation_steps -= 1;
    let s_cand = run_arm(&short, "short", "candidate", Some(REFERENCE));
    let s_ctrl = run_arm(&short, "short", "control", Some(REFERENCE));
    let store = score_store("short");
    assert!(matches!(
        score_hidden(
            &short,
            "candidate",
            &output("short", "candidate", &s_cand),
            &[("candidate", &s_cand), ("control", &s_ctrl)],
            &descriptor(3),
            SALT,
            &store
        ),
        Err(ScoringError::Refused(_))
    ));
    assert!(nothing_simulated(&store));
    // An acceptance plan is never scored on the hidden set.
    let mut acceptance = p.clone();
    acceptance.scoring_instances =
        genesis_core::development_trial::ScoringInstances::Development {};
    refused("acceptance", &acceptance, &closed, &descriptor(3), SALT);
    // Substituted output bytes.
    let store = score_store("bytes");
    let mut other: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    other["bodies"][0]["w"] = 3.0.into();
    assert!(matches!(
        score_hidden(
            &p,
            "candidate",
            &serde_json::to_vec(&other).unwrap(),
            &closed,
            &descriptor(3),
            SALT,
            &store
        ),
        Err(ScoringError::Refused(_))
    ));
    assert!(nothing_simulated(&store));
}

#[test]
fn a_slot_without_output_has_nothing_to_score() {
    let p = plan(2);
    let cand = run_arm(&p, "none", "candidate", None);
    let ctrl = run_arm(&p, "none", "control", Some(REFERENCE));
    let store = score_store("none");
    assert!(matches!(
        score_hidden(
            &p,
            "candidate",
            b"",
            &[("candidate", &cand), ("control", &ctrl)],
            &descriptor(2),
            SALT,
            &store
        ),
        Err(ScoringError::NoOutput)
    ));
}
