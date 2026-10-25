//! Offline JSON captures only: no generation, provider, scorer or method runs.
use genesis_core::digest::canonical_sha256;
use genesis_forge::{process_cohort::*, process_selection::*, process_selection_record::*};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Case {
    path: PathBuf,
    declaration: CohortDeclaration,
    selections: [RetainedSelection; 8],
}
impl Case {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-cohort-origins-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("input")).unwrap();
        let families = [
            Family::RavineCrossing,
            Family::RoughTransport,
            Family::ReturnMix,
            Family::ProjectileTarget,
        ];
        let selections: [RetainedSelection; 8] = std::array::from_fn(|i| {
            RetainedSelection::capture(SelectionInputs {
                arm: if i < 4 { Arm::Workflow } else { Arm::Fresh },
                family: families[i % 4],
                rounds: std::array::from_fn(|r| RoundInput::NoMethod {
                    reason: format!("no attempt in round {}", r + 1),
                }),
            })
            .unwrap()
        });
        let declaration = CohortDeclaration {
            schema: "genesis-process-cohort-declaration/1".into(),
            selections: std::array::from_fn(|i| SelectionPin {
                arm: selections[i].inputs.arm,
                family: selections[i].inputs.family,
                selection_sha256: selections[i].sha256(),
                rounds: std::array::from_fn(|r| ExpectedRound::NoMethod {
                    reason: format!("no attempt in round {}", r + 1),
                }),
            }),
        };
        let case = Self {
            path,
            declaration,
            selections,
        };
        case.write("input/declaration.json", &json!(case.declaration));
        for (i, selection) in case.selections.iter().enumerate() {
            case.write(&format!("input/selection-{i}.json"), &json!(selection));
        }
        case.write("input/manifest.json", &json!({
            "schema":"genesis-process-cohort-input/1", "declaration":"declaration.json",
            "selections":(0..8).map(|i|format!("selection-{i}.json")).collect::<Vec<_>>()
        }));
        case
    }
    fn write(&self, file: &str, value: &Value) {
        fs::write(self.path.join(file), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read(&self, file: &str) -> Value {
        serde_json::from_slice(&fs::read(self.path.join(file)).unwrap()).unwrap()
    }
    fn enable_origins(&self, value: &Value) {
        self.write("input/origins.json", value);
        let mut manifest = self.read("input/manifest.json");
        manifest["generation_origins"] = "origins.json".into();
        self.write("input/manifest.json", &manifest);
    }
    fn run(&self, command: &str) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        cmd.current_dir(&self.path)
            .args([command, "--cohort", "cohort.json"]);
        if command.ends_with("prepare") {
            cmd.args(["--manifest", "input/manifest.json"]);
        }
        cmd.output().unwrap()
    }
    fn prepare(&self) -> Value {
        let out = self.run("process-cohort-prepare");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn refuses_before_publication(&self) {
        assert!(!self.run("process-cohort-prepare").status.success());
        assert!(!self.path.join("cohort.json").exists());
        assert!(!self.path.join("cohort.json.lock").exists());
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn legacy_capture_bytes_and_summary_identity_remain_unchanged() {
    let case = Case::new();
    let report = serde_json::to_value(assess_cohort(&case.declaration, &case.selections).unwrap()).unwrap();
    // Frozen /1 field order and compact newline encoding, before optional origins existed.
    let expected = format!(
        "{{\"schema\":\"genesis-retained-process-cohort/1\",\"declaration\":{},\"selections\":{},\"report\":{}}}\n",
        serde_json::to_string(&case.declaration).unwrap(),
        serde_json::to_string(&case.selections).unwrap(),
        serde_json::to_string(&report).unwrap()
    );
    let summary = case.prepare();
    assert_eq!(fs::read(case.path.join("cohort.json")).unwrap(), expected.as_bytes());
    let retained: Value = serde_json::from_str(&expected).unwrap();
    assert_eq!(summary["cohort_sha256"], canonical_sha256(&retained));
    assert_eq!(summary["report_sha256"], canonical_sha256(&report));
    assert!(summary.get("generation_origins_sha256").is_none());
    assert!(summary.get("generation_report_sha256").is_none());
    let checked = case.run("process-cohort-check");
    assert!(checked.status.success());
    assert_eq!(serde_json::from_slice::<Value>(&checked.stdout).unwrap(), summary);
}

#[test]
fn malformed_or_unavailable_origin_files_refuse_without_creating_a_capture() {
    for variant in 0..5 {
        let case = Case::new();
        case.enable_origins(&json!({"schema":"unsupported-origins/99"}));
        let path = case.path.join("input/origins.json");
        match variant {
            0 => fs::remove_file(path).unwrap(),
            1 => fs::write(path, b"{").unwrap(),
            2 => {
                let mut bytes = fs::read(&path).unwrap();
                bytes.resize(8 * 1024 * 1024 + 1, b' ');
                fs::write(path, bytes).unwrap();
            }
            3 => {
                let mut manifest = case.read("input/manifest.json");
                manifest["generation_origins"] = "".into();
                case.write("input/manifest.json", &manifest);
            }
            _ => {}
        }
        case.refuses_before_publication();
    }
}

#[test]
fn schema_v1_cannot_smuggle_null_or_partial_origin_fields() {
    for (field, value) in [
        ("generation_origins", Value::Null),
        ("generation_report", Value::Null),
        ("generation_report", json!({})),
    ] {
        let case = Case::new();
        case.prepare();
        let mut retained = case.read("cohort.json");
        retained[field] = value;
        case.write("cohort.json", &retained);
        let original = fs::read(case.path.join("cohort.json")).unwrap();
        assert!(!case.run("process-cohort-check").status.success());
        assert_eq!(fs::read(case.path.join("cohort.json")).unwrap(), original);
        assert!(!case.path.join("cohort.json.lock").exists());
    }
}
