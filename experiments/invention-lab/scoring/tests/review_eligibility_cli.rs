//! Independent eligibility-only checks over copied, inert #116 records.
//! Never invokes score, an evaluator, method source, or a provider.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Case(PathBuf);
impl Case {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-eligibility-review-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(path.join("candidate-score/artifacts")).unwrap();
        fs::create_dir(path.join("candidate")).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search",
        );
        for name in [
            "plan.json",
            "candidate/receipt.json",
            "candidate-score/evaluation.json",
        ] {
            fs::copy(root.join(name), path.join(name)).unwrap();
        }
        let score = read(&path.join("candidate-score/evaluation.json"));
        let digest = score["outcome"]["metrics"]["sha256"].as_str().unwrap();
        let name = format!("candidate-score/artifacts/{digest}");
        fs::copy(root.join(&name), path.join(&name)).unwrap();
        Self(path)
    }
    fn run(&self, slot: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lab-score"))
            .arg("eligibility")
            .arg(self.0.join("plan.json"))
            .arg(slot)
            .arg(self.0.join("candidate"))
            .arg(self.0.join("candidate-score"))
            .output()
            .unwrap()
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn oversized_valid_json_is_refused_before_assessment() {
    // Trailing whitespace keeps each document valid and its parsed identity
    // unchanged. Reject oversized bytes at the input boundary, before decoding.
    let mut accepted = Vec::new();
    for name in [
        "plan.json",
        "candidate/receipt.json",
        "candidate-score/evaluation.json",
    ] {
        let case = Case::new();
        let path = case.0.join(name);
        let mut bytes = fs::read(&path).unwrap();
        bytes.resize(8 * 1024 * 1024 + 1, b' ');
        fs::write(&path, &bytes).unwrap();
        let output = case.run("candidate");
        if output.status.success() {
            accepted.push(name);
        }
        assert_eq!(fs::read(path).unwrap(), bytes, "read-only input: {name}");
    }
    assert!(
        accepted.is_empty(),
        "accepted JSON larger than 8 MiB: {accepted:?}"
    );
}

#[test]
fn unknown_usage_and_absent_scoring_are_retained_without_zero_or_writes() {
    let case = Case::new();
    let path = case.0.join("candidate/receipt.json");
    let mut execution = read(&path);
    let unknown = json!({"kind":"unknown","reason":"meter unavailable"});
    execution["usage"]["simulation_steps"] = unknown.clone();
    write(&path, &execution);
    fs::remove_file(case.0.join("candidate-score/evaluation.json")).unwrap();
    let before = fs::read(&path).unwrap();
    let output = case.run("candidate");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let assessment: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(assessment["development_usage"]["simulation_steps"], unknown);
    assert!(assessment["development_steps"].is_null());
    assert!(assessment["scoring"].is_null());
    assert!(assessment["successes"].is_null());
    assert_eq!(assessment["eligible"], false);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!case.0.join("candidate-score/evaluation.json").exists());
}

#[test]
fn foreign_slot_and_same_digest_foreign_method_identity_are_refused() {
    let case = Case::new();
    assert!(!case.run("control").status.success());
    let path = case.0.join("candidate/receipt.json");
    let mut receipt = read(&path);
    // Identity is the whole content reference, not only its digest.
    receipt["actual_method"]["id"] = "foreign-method".into();
    write(&path, &receipt);
    let output = case.run("candidate");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(read(&path), receipt);
}
