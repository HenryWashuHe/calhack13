//! CLI tests over retained JSON. The only process is genesis-forge's offline
//! selection command; no method source, scorer or provider is invoked.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "genesis-selection-cli-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn run(dir: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
        .current_dir(dir)
        .args(arguments)
        .output()
        .unwrap()
}
fn ok(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn invalid(output: Output) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
fn fixture(dir: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-parent",
    );
    let data = dir.join("data");
    fs::create_dir(&data).unwrap();
    for name in ["plan.json", "criterion.json"] {
        fs::copy(source.join(name), data.join(name)).unwrap();
    }
    let execution: Value =
        serde_json::from_slice(&fs::read(source.join("candidate/receipt.json")).unwrap()).unwrap();
    let scoring: Value =
        serde_json::from_slice(&fs::read(source.join("candidate-score/evaluation.json")).unwrap())
            .unwrap();
    let digest = scoring["outcome"]["metrics"]["sha256"].as_str().unwrap();
    fs::copy(
        source.join(format!("candidate-score/artifacts/{digest}")),
        data.join("metrics.json"),
    )
    .unwrap();
    write(
        &data.join("evidence.json"),
        &json!({"slot_id":"candidate", "execution":execution,"scoring":scoring}),
    );
    write(
        &dir.join("input.json"),
        &json!({
            "schema":"genesis-process-selection-input/1", "arm":"workflow", "family":"rough-transport",
            "rounds":[
                {"kind":"trial","plan":"data/plan.json","criterion":"data/criterion.json",
                    "evidence":"data/evidence.json","metrics":"data/metrics.json"},
                {"kind":"no_method","reason":"generation failed"},
                {"kind":"no_method","reason":"no_parent"}
            ]
        }),
    );
}
fn prepare(dir: &Path) -> Output {
    run(
        dir,
        &[
            "process-select-prepare",
            "--manifest",
            "input.json",
            "--selection",
            "selection.json",
        ],
    )
}

#[test]
fn captures_exact_rejected_trial_selection_and_checks_without_original_paths() {
    let dir = TempDir::new().unwrap();
    fixture(dir.path());
    let summary = ok(prepare(dir.path()));
    assert_eq!(summary["selected"]["round"], 1);
    assert_eq!(
        summary["selected"]["method"]["sha256"],
        "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d"
    );
    let original = fs::read(dir.path().join("selection.json")).unwrap();
    let record: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(record["report"]["rows"][0]["successes"], 3);
    assert_eq!(record["report"]["rows"].as_array().unwrap().len(), 3);
    assert_eq!(summary["scope"], "single_arm_family_development");
    // The snapshot is self-contained. No late package/path substitutions.
    fs::remove_dir_all(dir.path().join("data")).unwrap();
    fs::remove_file(dir.path().join("input.json")).unwrap();
    fs::write(
        dir.path().join("selection.json.lock"),
        "crashed preparation",
    )
    .unwrap();
    assert_eq!(
        ok(run(
            dir.path(),
            &["process-select-check", "--selection", "selection.json"]
        )),
        summary
    );
    assert_eq!(
        fs::read(dir.path().join("selection.json")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("selection.json.lock")).unwrap(),
        "crashed preparation"
    );
}

#[test]
fn prepare_is_create_only_and_failed_validation_leaves_no_destination() {
    let dir = TempDir::new().unwrap();
    fixture(dir.path());
    ok(prepare(dir.path()));
    let original = fs::read(dir.path().join("selection.json")).unwrap();
    invalid(prepare(dir.path()));
    assert_eq!(
        fs::read(dir.path().join("selection.json")).unwrap(),
        original
    );
    assert!(!dir.path().join("selection.json.lock").exists());
    fs::remove_file(dir.path().join("selection.json")).unwrap();
    let mut input: Value =
        serde_json::from_slice(&fs::read(dir.path().join("input.json")).unwrap()).unwrap();
    input["rounds"][0]["metrics"] = "data/missing.json".into();
    write(&dir.path().join("input.json"), &input);
    invalid(prepare(dir.path()));
    assert!(!dir.path().join("selection.json").exists());
    assert!(!dir.path().join("selection.json.lock").exists());
}

#[test]
fn changed_reports_input_bindings_and_unknown_fields_fail_read_only_check() {
    let dir = TempDir::new().unwrap();
    fixture(dir.path());
    ok(prepare(dir.path()));
    let original: Value =
        serde_json::from_slice(&fs::read(dir.path().join("selection.json")).unwrap()).unwrap();
    for mode in 0..5 {
        let mut record = original.clone();
        match mode {
            0 => record["report"]["selected_round"] = 2.into(),
            1 => {
                record["inputs"]["rounds"][0]["input"]["plan"]["slots"][0]["method"]["sha256"] =
                    "a".repeat(64).into()
            }
            2 => record["inputs"]["rounds"][1]["reason"] = "different absence".into(),
            3 => record["report"]["extra"] = true.into(),
            _ => record["extra"] = true.into(),
        }
        write(&dir.path().join("selection.json"), &record);
        let bytes = fs::read(dir.path().join("selection.json")).unwrap();
        invalid(run(
            dir.path(),
            &["process-select-check", "--selection", "selection.json"],
        ));
        assert_eq!(fs::read(dir.path().join("selection.json")).unwrap(), bytes);
        assert!(!dir.path().join("selection.json.lock").exists());
    }
}

#[test]
fn no_candidate_is_a_successful_report_and_manifest_shape_is_strict() {
    let dir = TempDir::new().unwrap();
    fixture(dir.path());
    let mut input: Value =
        serde_json::from_slice(&fs::read(dir.path().join("input.json")).unwrap()).unwrap();
    input["rounds"][0] = json!({"kind":"no_method","reason":"no generated source"});
    write(&dir.path().join("input.json"), &input);
    let summary = ok(prepare(dir.path()));
    assert!(summary["selected"].is_null());
    assert_eq!(
        ok(run(
            dir.path(),
            &["process-select-check", "--selection", "selection.json"]
        )),
        summary
    );
    fs::remove_file(dir.path().join("selection.json")).unwrap();
    for mode in 0..5 {
        let mut malformed = input.clone();
        match mode {
            0 => {
                malformed["rounds"].as_array_mut().unwrap().pop();
            }
            1 => malformed["rounds"][0]["eligible"] = true.into(),
            2 => malformed["schema"] = "foreign".into(),
            3 => malformed["arm"] = "unknown".into(),
            _ => malformed["family"] = "unknown".into(),
        }
        write(&dir.path().join("input.json"), &malformed);
        invalid(prepare(dir.path()));
        assert!(!dir.path().join("selection.json").exists());
    }
    invalid(run(
        dir.path(),
        &[
            "process-select-check",
            "--selection",
            "missing.json",
            "--manifest",
            "input.json",
        ],
    ));
}
