//! `lab-score eligibility` prints the shared process slot assessment for the
//! caller's expected plan and refuses evidence that doesn't bind to it. Reads
//! retained #116 records only; nothing executes.
use std::path::{Path, PathBuf};
use std::process::Command;

fn evidence() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search")
}

fn run(plan: &Path, slot: &str) -> (bool, serde_json::Value, String) {
    let d = evidence();
    let out = Command::new(env!("CARGO_BIN_EXE_lab-score"))
        .arg("eligibility")
        .arg(plan)
        .arg(slot)
        .arg(d.join(slot))
        .arg(d.join(format!("{slot}-score")))
        .output()
        .unwrap();
    let value = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    (
        out.status.success(),
        value,
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

#[test]
fn retained_slots_are_assessed_against_their_expected_plan() {
    let plan = evidence().join("plan.json");
    let (ok, cand, err) = run(&plan, "candidate");
    assert!(ok, "{err}");
    assert_eq!(cand["schema"], "genesis-process-slot-assessment/1");
    assert_eq!(
        (cand["eligible"].as_bool(), cand["successes"].as_u64()),
        (Some(true), Some(3))
    );
    assert_eq!(
        cand["method"]["sha256"],
        "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d"
    );
    let (ok, ctrl, err) = run(&plan, "control");
    assert!(ok, "{err}");
    assert_eq!(ctrl["successes"].as_u64(), Some(4));
}

#[test]
fn a_different_expected_plan_refuses_the_same_records() {
    // Same records, but the caller expected another candidate method.
    let mut plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence().join("plan.json")).unwrap()).unwrap();
    plan["slots"][0]["method"]["sha256"] = serde_json::json!("0".repeat(64));
    let path = std::env::temp_dir().join(format!("lab-score-expected-{}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let (ok, value, err) = run(&path, "candidate");
    let _ = std::fs::remove_file(&path);
    assert!(!ok, "a mismatched expected plan must not assess: {value}");
    assert!(!err.is_empty());
}
