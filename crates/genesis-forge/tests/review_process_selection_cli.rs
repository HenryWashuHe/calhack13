//! Independent offline CLI guards. Retained #116 JSON is inert input only;
//! no generated method, evaluator, provider or namespace is executed.
use genesis_core::digest::canonical_sha256;
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
        let dir = std::env::temp_dir().join(format!(
            "genesis-independent-selection-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("inputs")).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-parent",
        );
        for name in ["plan.json", "criterion.json"] {
            fs::copy(source.join(name), dir.join("inputs").join(name)).unwrap();
        }
        let execution = read(&source.join("candidate/receipt.json"));
        let scoring = read(&source.join("candidate-score/evaluation.json"));
        let digest = scoring["outcome"]["metrics"]["sha256"].as_str().unwrap();
        fs::copy(
            source.join(format!("candidate-score/artifacts/{digest}")),
            dir.join("inputs/metrics.json"),
        )
        .unwrap();
        write(
            &dir.join("inputs/evidence.json"),
            &json!({"slot_id":"candidate","execution":execution,"scoring":scoring}),
        );
        write(
            &dir.join("inputs/manifest.json"),
            &json!({
                "schema":"genesis-process-selection-input/1",
                "arm":"workflow", "family":"rough-transport",
                "rounds":[
                    {"kind":"trial","plan":"plan.json","criterion":"criterion.json",
                     "evidence":"evidence.json","metrics":"metrics.json"},
                    {"kind":"no_method","reason":"generation unresolved"},
                    {"kind":"no_method","reason":"no parent"}
                ]
            }),
        );
        Self(dir)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn prepare(&self) -> Output {
        self.run(&[
            "process-select-prepare",
            "--manifest",
            "inputs/manifest.json",
            "--selection",
            "selection.json",
        ])
    }
    fn no_publication(&self, output: Output) -> String {
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!self.0.join("selection.json").exists());
        assert!(!self.0.join("selection.json.lock").exists());
        String::from_utf8(output.stderr).unwrap()
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
fn successful(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn each_file_limit_fails_before_publication_including_retained_check() {
    for (name, limit) in [
        ("manifest.json", 8 * 1024 * 1024),
        ("plan.json", 8 * 1024 * 1024),
        ("evidence.json", 8 * 1024 * 1024),
        ("criterion.json", 64 * 1024),
        ("metrics.json", 1024 * 1024),
    ] {
        let case = Case::new();
        fs::write(case.0.join("inputs").join(name), vec![b' '; limit + 1]).unwrap();
        let error = case.no_publication(case.prepare());
        assert!(
            error.contains(&format!("input exceeds {limit} byte limit")),
            "{name}: {error}"
        );
    }
    let case = Case::new();
    let bytes = vec![b' '; 8 * 1024 * 1024 + 1];
    fs::write(case.0.join("selection.json"), &bytes).unwrap();
    let output = case.run(&["process-select-check", "--selection", "selection.json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("input exceeds 8388608 byte limit"));
    assert_eq!(fs::read(case.0.join("selection.json")).unwrap(), bytes);
    assert!(!case.0.join("selection.json.lock").exists());
}

#[test]
fn paths_are_manifest_relative_and_snapshot_digest_ignores_json_whitespace() {
    let case = Case::new();
    // A CWD-relative implementation would read these invalid decoys.
    for name in [
        "plan.json",
        "criterion.json",
        "evidence.json",
        "metrics.json",
    ] {
        fs::write(case.0.join(name), "invalid decoy").unwrap();
    }
    let summary = successful(case.prepare());
    let retained = read(&case.0.join("selection.json"));
    assert_eq!(summary["selection_sha256"], canonical_sha256(&retained));
    assert_eq!(
        summary["report_sha256"],
        canonical_sha256(&retained["report"])
    );
    assert_eq!(summary["input_sha256"], retained["report"]["input_sha256"]);
    assert_eq!(
        summary["selected"]["method"],
        retained["report"]["rows"][0]["method"]
    );
    assert_eq!(
        summary["selected"]["plan"],
        retained["report"]["rows"][0]["plan"]
    );
    let reformatted = serde_json::to_vec_pretty(&retained).unwrap();
    fs::write(case.0.join("selection.json"), &reformatted).unwrap();
    assert_eq!(
        successful(case.run(&["process-select-check", "--selection", "selection.json"])),
        summary
    );
    assert_eq!(
        fs::read(case.0.join("selection.json")).unwrap(),
        reformatted
    );
}

#[test]
fn unknown_usage_survives_capture_and_check_without_zero_or_selection() {
    let case = Case::new();
    let path = case.0.join("inputs/evidence.json");
    let mut evidence = read(&path);
    let unknown = json!({"kind":"unknown","reason":"meter did not report"});
    evidence["execution"]["usage"]["simulation_steps"] = unknown.clone();
    evidence["scoring"] = Value::Null;
    write(&path, &evidence);
    let path = case.0.join("inputs/manifest.json");
    let mut manifest = read(&path);
    manifest["rounds"][0]["metrics"] = Value::Null;
    write(&path, &manifest);
    let summary = successful(case.prepare());
    assert!(summary["selected"].is_null());
    let retained = read(&case.0.join("selection.json"));
    let row = &retained["report"]["rows"][0];
    assert_eq!(row["development_usage"]["simulation_steps"], unknown);
    assert!(row["successes"].is_null());
    assert_eq!(row["eligible"], false);
    assert_eq!(
        successful(case.run(&["process-select-check", "--selection", "selection.json"])),
        summary
    );
}

#[test]
fn same_digest_foreign_method_identity_and_ambiguous_json_are_refused() {
    for mode in 0..3 {
        let case = Case::new();
        let path = case.0.join("inputs/evidence.json");
        let mut evidence = read(&path);
        if mode == 0 {
            // An unchanged content digest is not enough: id is part of identity.
            evidence["execution"]["actual_method"]["id"] = "foreign-method".into();
            write(&path, &evidence);
        } else if mode == 1 {
            evidence["execution"]["actual_method"]["trusted"] = true.into();
            write(&path, &evidence);
        } else {
            let encoded = serde_json::to_string(&evidence).unwrap();
            fs::write(
                &path,
                encoded.replacen(
                    "\"slot_id\":\"candidate\"",
                    "\"slot_id\":\"candidate\",\"slot_id\":\"candidate\"",
                    1,
                ),
            )
            .unwrap();
        }
        case.no_publication(case.prepare());
    }
}
