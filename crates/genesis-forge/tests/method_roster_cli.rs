use genesis_forge::{method_roster::GenerationRoster, method_synthesis::MethodSynthesisSession};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "common/method_revision_fixture.rs"]
mod fixtures;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-roster-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let files = Self(path);
        let (spec, parent) = fixtures::fixture();
        let session = spec.prepare(&parent).unwrap();
        files.write("generation.json", &session);
        files.write("declaration.json", &json!({"schema":"genesis-generation-roster-input/1", "id":"test",
            "settings":session.plan().settings,"prepared_checkpoints":["generation.json"],
            "total_grant":{"model_calls":1,"input_tokens":100,"output_tokens":16384,"wall_time_ms":10000,"simulation_steps":0}}));
        files.write("snapshots.json", &json!({"schema":"genesis-generation-roster-snapshots/1","generation_checkpoints":["generation.json"]}));
        files
    }
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn session(&self) -> MethodSynthesisSession {
        serde_json::from_slice(&fs::read(self.0.join("generation.json")).unwrap()).unwrap()
    }
    fn run(&self, command: &str, manifest: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .arg(command)
            .arg("--manifest")
            .arg(self.0.join(manifest))
            .arg("--roster")
            .arg(self.0.join("roster.json"))
            .output()
            .unwrap()
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        files.sort();
        files
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn prepare_is_exclusive_and_does_not_copy_source_or_modify_generation() {
    let files = Files::new();
    let before = fs::read(files.0.join("generation.json")).unwrap();
    let output = files.run("method-roster-prepare", "declaration.json");
    assert!(output.status.success(), "{output:?}");
    let bytes = fs::read(files.0.join("roster.json")).unwrap();
    assert!(!String::from_utf8(bytes.clone())
        .unwrap()
        .contains("MUST_NOT_EXECUTE"));
    let roster: GenerationRoster = serde_json::from_slice(&bytes).unwrap();
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["roster_sha256"], roster.sha256().unwrap());
    assert_eq!(fs::read(files.0.join("generation.json")).unwrap(), before);
    let before = files.snapshot();
    let output = files.run("method-roster-prepare", "declaration.json");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}

#[test]
fn started_attempt_cannot_be_declared_and_leaves_no_output_or_lock() {
    let files = Files::new();
    let mut session = files.session();
    session.begin().unwrap();
    files.write("generation.json", &session);
    let before = files.snapshot();
    let output = files.run("method-roster-prepare", "declaration.json");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}

#[test]
fn assessment_reads_pending_and_advanced_snapshots_without_recovery_or_locking() {
    let files = Files::new();
    assert!(files
        .run("method-roster-prepare", "declaration.json")
        .status
        .success());
    fs::write(files.0.join("roster.json.lock"), b"roster sentinel").unwrap();
    fs::write(files.0.join("generation.json.lock"), b"pending sentinel").unwrap();
    let prepared = files.session();
    let mut pending = prepared.clone();
    pending.begin().unwrap();
    let mut generated = prepared.clone();
    fixtures::complete(&mut generated, fixtures::SOURCE);
    for session in [pending, generated] {
        files.write("generation.json", &session);
        let before = files.snapshot();
        let output = files.run("method-roster-assess", "snapshots.json");
        assert!(output.status.success(), "{output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let roster: GenerationRoster =
            serde_json::from_slice(&fs::read(files.0.join("roster.json")).unwrap()).unwrap();
        assert_eq!(
            report,
            serde_json::to_value(roster.assess(&[session]).unwrap()).unwrap()
        );
        assert_eq!(before, files.snapshot());
    }
}

#[test]
fn omitted_snapshot_is_unknown_but_missing_named_file_is_an_error() {
    let files = Files::new();
    assert!(files
        .run("method-roster-prepare", "declaration.json")
        .status
        .success());
    files.write(
        "snapshots.json",
        &json!({"schema":"genesis-generation-roster-snapshots/1","generation_checkpoints":[]}),
    );
    let before = files.snapshot();
    let output = files.run("method-roster-assess", "snapshots.json");
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["costs"]["rows"][0]["observation"]["kind"], "missing");
    assert_eq!(report["costs"]["rows"].as_array().unwrap().len(), 1);
    assert_eq!(before, files.snapshot());
    files.write("snapshots.json", &json!({"schema":"genesis-generation-roster-snapshots/1","generation_checkpoints":["absent.json"]}));
    let before = files.snapshot();
    let output = files.run("method-roster-assess", "snapshots.json");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}
