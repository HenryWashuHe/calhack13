use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "common/method_revision_fixture.rs"]
mod fixtures;
use fixtures::*;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-revision-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let files = Self(path);
        let (spec, parent) = fixture();
        files.write("revision.json", &spec);
        fs::write(files.0.join("parent.json"), parent).unwrap();
        files
    }
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn run(&self, command: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GENESIS_LLM_PROVIDER")
            .env_remove("GENESIS_LLM_MODEL")
            .env_remove("GENESIS_LLM_API_KEY")
            .arg(command)
            .arg("--plan")
            .arg(self.0.join("revision.json"))
            .arg("--parent")
            .arg(self.0.join("parent.json"))
            .arg("--checkpoint")
            .arg(self.0.join("generation.json"))
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
fn prepare_uses_existing_checkpoint_and_never_overwrites_or_executes_source() {
    let files = Files::new();
    let result = files.run("method-revise-prepare");
    assert!(result.status.success(), "{:?}", result);
    let status: Value = serde_json::from_slice(&result.stdout).unwrap();
    let session: MethodSynthesisSession =
        serde_json::from_slice(&fs::read(files.0.join("generation.json")).unwrap()).unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Prepared);
    assert_eq!(status["request_sha256"], session.request_sha256());
    assert_eq!(status["checkpoint_sha256"], session.checkpoint_sha256());
    assert!(!String::from_utf8(result.stdout)
        .unwrap()
        .contains("MUST_NOT_EXECUTE"));
    let before = files.snapshot();
    let result = files.run("method-revise-prepare");
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}

#[test]
fn invalid_parent_never_creates_checkpoint_or_lock() {
    let files = Files::new();
    fs::write(files.0.join("parent.json"), b"{}").unwrap();
    let before = files.snapshot();
    let result = files.run("method-revise-prepare");
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}

#[test]
fn observe_reports_exact_fixture_provenance_without_writes_even_with_a_lock() {
    let files = Files::new();
    let (spec, parent) = fixture();
    let mut session = spec.prepare(&parent).unwrap();
    complete(&mut session, SOURCE);
    files.write("generation.json", &session);
    fs::write(
        files.0.join("generation.json.lock"),
        b"existing lock sentinel",
    )
    .unwrap();
    let before = files.snapshot();
    let result = files.run("method-revise-observe");
    assert!(result.status.success(), "{:?}", result);
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        report,
        serde_json::to_value(spec.observe(&parent, &session).unwrap()).unwrap()
    );
    assert_eq!(report["source_changed"], false);
    assert_eq!(report["org_id"], "org_b");
    assert_ne!(report["parent"], report["child"]);
    assert!(!String::from_utf8(result.stdout)
        .unwrap()
        .contains("MUST_NOT_EXECUTE"));
    assert_eq!(before, files.snapshot());
}

#[test]
fn observing_pending_or_failed_attempt_never_recovers_or_replays_it() {
    let files = Files::new();
    let (spec, parent) = fixture();
    let mut session = spec.prepare(&parent).unwrap();
    session.begin().unwrap();
    fs::write(
        files.0.join("generation.json.lock"),
        b"pending original lock",
    )
    .unwrap();
    for recover in [false, true] {
        if recover {
            session.recover_interrupted().unwrap();
        }
        files.write("generation.json", &session);
        let before = files.snapshot();
        let result = files.run("method-revise-observe");
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert_eq!(before, files.snapshot());
    }
}

#[test]
fn valid_foreign_spec_cannot_relabel_a_saved_generation() {
    let files = Files::new();
    let (mut spec, parent) = fixture();
    let mut session = spec.prepare(&parent).unwrap();
    complete(&mut session, "print('changed')");
    files.write("generation.json", &session);
    spec.generation.org_id = "org_other".into();
    files.write("revision.json", &spec);
    let before = files.snapshot();
    let result = files.run("method-revise-observe");
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}
