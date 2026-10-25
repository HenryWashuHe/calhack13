//! Read-only/capture subprocesses over inert JSON, never generated source.
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
struct Case(PathBuf);
impl Case {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-cohort-cli-{}-{}-{}",
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
                    reason: format!("unresolved generation {r}"),
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
                    reason: format!("unresolved generation {r}"),
                }),
            }),
        };
        for (i, s) in selections.iter().enumerate() {
            fs::write(
                path.join(format!("input/selection-{i}.json")),
                serde_json::to_vec(s).unwrap(),
            )
            .unwrap();
        }
        fs::write(
            path.join("input/declaration.json"),
            serde_json::to_vec(&declaration).unwrap(),
        )
        .unwrap();
        fs::write(path.join("input/manifest.json"),serde_json::to_vec(&json!({"schema":"genesis-process-cohort-input/1","declaration":"declaration.json","selections":(0..8).map(|i|format!("selection-{i}.json")).collect::<Vec<_>>()})).unwrap()).unwrap();
        Self(path)
    }
    fn run(&self, command: &str) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        cmd.current_dir(&self.0)
            .args([command, "--cohort", "cohort.json"]);
        if command.ends_with("prepare") {
            cmd.args(["--manifest", "input/manifest.json"]);
        }
        cmd.output().unwrap()
    }
    fn prepare(&self) -> Value {
        let out = self.run("process-cohort-prepare");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn capture_is_self_contained_create_only_and_check_is_read_only() {
    let c = Case::new();
    let prepared = c.prepare();
    assert_eq!(prepared["selections"].as_array().unwrap().len(), 8);
    assert!(prepared["selections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["selected"].is_null()));
    let path = c.0.join("cohort.json");
    let original = fs::read(&path).unwrap();
    let parsed: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(prepared["cohort_sha256"], canonical_sha256(&parsed));
    assert!(parsed["selections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["inputs"]["rounds"].as_array().unwrap().len() == 3));
    assert!(!c.run("process-cohort-prepare").status.success());
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_dir_all(c.0.join("input")).unwrap();
    fs::write(c.0.join("cohort.json.lock"), b"stale fixture").unwrap();
    let checked = c.run("process-cohort-check");
    assert!(checked.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&checked.stdout).unwrap(),
        prepared
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        fs::read(c.0.join("cohort.json.lock")).unwrap(),
        b"stale fixture"
    );
}
#[test]
fn inconsistent_pins_records_or_reports_never_publish_or_pass_recheck() {
    for kind in 0..4 {
        let c = Case::new();
        let p = c.0.join("input/declaration.json");
        let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        match kind {
            0 => v["selections"][0]["selection_sha256"] = "f".repeat(64).into(),
            1 => v["selections"][7] = v["selections"][0].clone(),
            2 => v["selections"][0]["rounds"][0]["reason"] = "different".into(),
            _ => v["surprise"] = true.into(),
        }
        fs::write(p, serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(!c.run("process-cohort-prepare").status.success());
        assert!(!c.0.join("cohort.json").exists());
        assert!(!c.0.join("cohort.json.lock").exists());
    }
    for kind in 0..3 {
        let c = Case::new();
        c.prepare();
        let p = c.0.join("cohort.json");
        let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        match kind {
            0 => v["report"]["cells"][0]["selection"]["selected_round"] = 1.into(),
            1 => v["selections"][0]["inputs"]["rounds"][0]["reason"] = "altered".into(),
            _ => v["schema"] = "foreign/1".into(),
        }
        let bytes = serde_json::to_vec(&v).unwrap();
        fs::write(&p, &bytes).unwrap();
        assert!(!c.run("process-cohort-check").status.success());
        assert_eq!(fs::read(p).unwrap(), bytes);
    }
}
#[test]
fn each_input_and_retained_capture_is_bounded_before_decoding() {
    for name in [
        "input/manifest.json",
        "input/declaration.json",
        "input/selection-7.json",
    ] {
        let c = Case::new();
        let p = c.0.join(name);
        let mut bytes = fs::read(&p).unwrap();
        bytes.resize(8 * 1024 * 1024 + 1, b' ');
        fs::write(p, bytes).unwrap();
        assert!(!c.run("process-cohort-prepare").status.success());
        assert!(!c.0.join("cohort.json").exists());
        assert!(!c.0.join("cohort.json.lock").exists());
    }
    let c = Case::new();
    c.prepare();
    let p = c.0.join("cohort.json");
    let mut bytes = fs::read(&p).unwrap();
    bytes.resize(8 * 1024 * 1024 + 1, b' ');
    fs::write(&p, &bytes).unwrap();
    assert!(!c.run("process-cohort-check").status.success());
    assert_eq!(fs::read(p).unwrap(), bytes);
}
