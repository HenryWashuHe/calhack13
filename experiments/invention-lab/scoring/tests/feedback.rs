//! Receiver feedback is built only from retained, cross-checked records.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::feedback::{receiver_feedback, SCHEMA};
use genesis_lab_score::{score_slot, ScoringError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");

fn plan() -> TrialPlan {
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
    let mut p = example::acceptance_plan(&methods, Mode::None, "Python 3", 3, grant);
    p.scoring_grant.simulation_steps = 3 * 2400;
    p
}

/// One experiment, then a submission, then trusted scoring. Returns the
/// slot and score directories.
fn run_and_score(p: &TrialPlan, name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("lab-feedback-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (slot, score) = (dir.join("slot"), dir.join("score"));
    let mut s = Session {
        plan: p,
        slot_id: "candidate",
        store: Store::open(&slot).unwrap(),
        ledger: ExperimentLedger::new(ExperimentScope {
            plan: p,
            slot_id: "candidate",
        })
        .unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding("candidate").unwrap().method),
            ..Default::default()
        },
    };
    let design: Value = serde_json::from_str(REFERENCE).unwrap();
    let instance = p.development_instances[1].id.clone();
    let r = s
        .handle(&json!({"op": "experiment", "instance": instance, "design": design}).to_string())
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let r = s
        .handle(&json!({"op": "submit", "design": design}).to_string())
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let done = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        ExecutionOutcome::OutputGenerated,
        ResourceUsage::Measured { value: 5 },
    )
    .unwrap();
    let output =
        std::fs::read(slot.join("artifacts").join(&done.receipt.outputs[0].sha256)).unwrap();
    score_slot(
        p,
        "candidate",
        &done.receipt,
        &output,
        &Store::open(&score).unwrap(),
    )
    .unwrap();
    (slot, score)
}

fn feedback(p: &TrialPlan, slot: &Path, score: &Path) -> Result<Value, ScoringError> {
    let (reference, bytes) = receiver_feedback(
        p,
        "candidate",
        &Store::open(slot).unwrap(),
        &Store::open(score).unwrap(),
    )?;
    assert_eq!(reference.schema, SCHEMA);
    assert_eq!(reference.sha256, refs::sha256_hex(&bytes));
    Ok(serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn feedback_lists_public_experiments_the_submission_and_its_scores() {
    let p = plan();
    let (slot, score) = run_and_score(&p, "ok");
    let doc = feedback(&p, &slot, &score).unwrap();
    assert_eq!(doc["schema"], SCHEMA);
    assert_eq!(doc["receiver_org"], "org_example_b");
    assert_eq!(doc["slot"], "candidate");
    let ids: Vec<&str> = p
        .development_instances
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(doc["public_instances"], json!(ids));
    let dev = &doc["development"];
    assert_eq!(dev["experiments_total"], 1);
    let e = &dev["experiments"][0];
    assert_eq!(e["instance"], ids[1]);
    assert_eq!(e["result"]["outcome"], "scored");
    assert!(e["result"]["metrics"]["progress"].is_number(), "{e}");
    assert_eq!(e["design"], doc["submitted_design"]);
    assert_eq!(doc["scoring"]["of"], 3);
    assert_eq!(doc["scoring"]["instances"].as_array().unwrap().len(), 3);
    // Deterministic: the same records give the same bytes.
    assert_eq!(doc, feedback(&p, &slot, &score).unwrap());
}

#[test]
fn a_tampered_artifact_or_mismatched_records_are_refused() {
    let p = plan();
    let (slot, score) = run_and_score(&p, "tamper");
    // A stored experiment result whose bytes no longer match their digest.
    let ledger: Value =
        serde_json::from_slice(&std::fs::read(slot.join("ledger.json")).unwrap()).unwrap();
    let result = ledger.to_string();
    let sha = result
        .split("\"result\":")
        .nth(1)
        .and_then(|s| s.split("\"sha256\":\"").nth(1))
        .map(|s| s[..64].to_owned())
        .unwrap();
    let path = slot.join("artifacts").join(&sha);
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{\"metrics\":{\"success\":true}}").unwrap();
    assert!(matches!(
        feedback(&p, &slot, &score),
        Err(ScoringError::Refused(_))
    ));
    std::fs::write(&path, &original).unwrap();
    assert!(feedback(&p, &slot, &score).is_ok());

    // An execution receipt changed after scoring no longer binds to the
    // scoring receipt (or its ledger), so nothing is reported.
    let original = std::fs::read(slot.join("receipt.json")).unwrap();
    let mut receipt: Value = serde_json::from_slice(&original).unwrap();
    receipt["usage"]["wall_time_ms"]["value"] = json!(6);
    std::fs::write(slot.join("receipt.json"), receipt.to_string()).unwrap();
    assert!(feedback(&p, &slot, &score).is_err());
    std::fs::write(slot.join("receipt.json"), &original).unwrap();
    assert!(feedback(&p, &slot, &score).is_ok());
}

#[test]
fn an_unscored_slot_has_no_feedback() {
    let p = plan();
    let (slot, score) = run_and_score(&p, "unscored");
    std::fs::remove_file(score.join("evaluation.json")).unwrap();
    assert!(feedback(&p, &slot, &score).is_err());
}
