use genesis_forge::{
    method_revision::MethodRevisionInput,
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
};
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
            "genesis-revision-input-{}-{}-{}",
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
        files.write("spec.json", &spec);
        fs::write(files.0.join("parent.json"), &parent).unwrap();
        files.write(
            "input.json",
            &MethodRevisionInput::new(spec, String::from_utf8(parent).unwrap()).unwrap(),
        );
        files
    }
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn input(&self) -> MethodRevisionInput {
        serde_json::from_slice(&fs::read(self.0.join("input.json")).unwrap()).unwrap()
    }
    fn run(&self, command: &str, flags: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        cmd.env_clear()
            .arg(command)
            .arg("--checkpoint")
            .arg(self.0.join("generation.json"));
        for (flag, file) in flags {
            cmd.arg(flag).arg(self.0.join(file));
        }
        cmd.output().unwrap()
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
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
const INPUT: &[(&str, &str)] = &[("--input", "input.json")];

#[test]
fn input_prepare_matches_legacy_checkpoint_and_cannot_overwrite_it() {
    let files = Files::new();
    let result = files.run("method-revise-prepare", INPUT);
    assert!(result.status.success(), "{result:?}");
    let session: MethodSynthesisSession =
        serde_json::from_slice(&fs::read(files.0.join("generation.json")).unwrap()).unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Prepared);
    assert!(session.completion().is_none());
    let status: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["checkpoint_sha256"], session.checkpoint_sha256());
    let legacy = Files::new();
    let result = legacy.run(
        "method-revise-prepare",
        &[("--plan", "spec.json"), ("--parent", "parent.json")],
    );
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        fs::read(files.0.join("generation.json")).unwrap(),
        fs::read(legacy.0.join("generation.json")).unwrap()
    );
    let before = files.snapshot();
    let result = files.run("method-revise-prepare", INPUT);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(before, files.snapshot());
}

#[test]
fn input_observe_is_read_only_and_never_recovers_a_pending_session() {
    let files = Files::new();
    let input = files.input();
    let mut session = input.prepare().unwrap();
    complete(&mut session, SOURCE);
    files.write("generation.json", &session);
    fs::write(
        files.0.join("generation.json.lock"),
        b"original lock sentinel",
    )
    .unwrap();
    let before = files.snapshot();
    let result = files.run("method-revise-observe", INPUT);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap(),
        serde_json::to_value(input.observe(&session).unwrap()).unwrap()
    );
    assert_eq!(before, files.snapshot());
    let mut pending = input.prepare().unwrap();
    pending.begin().unwrap();
    for recovered in [false, true] {
        if recovered {
            pending.recover_interrupted().unwrap();
        }
        files.write("generation.json", &pending);
        let before = files.snapshot();
        let result = files.run("method-revise-observe", INPUT);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert_eq!(before, files.snapshot());
    }
}

#[test]
fn malformed_input_and_conflicting_modes_have_no_checkpoint_effects() {
    let files = Files::new();
    let original = serde_json::to_value(files.input()).unwrap();
    let mut variants = vec![];
    let mut wrong = original.clone();
    wrong["schema"] = "other/1".into();
    variants.push(wrong.to_string());
    let mut wrong = original.clone();
    wrong["unexpected"] = true.into();
    variants.push(wrong.to_string());
    let mut wrong = original.clone();
    wrong["parent_package_json"] = "{}".into();
    variants.push(wrong.to_string());
    variants.push(format!(
        "{{\"schema\":\"duplicate\",{}",
        &original.to_string()[1..]
    ));
    variants.push(format!("{}{}", original, " ".repeat(1024 * 1024)));
    for bytes in variants {
        fs::write(files.0.join("input.json"), bytes).unwrap();
        let before = files.snapshot();
        let result = files.run("method-revise-prepare", INPUT);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert_eq!(before, files.snapshot());
    }
    files.write("input.json", &original);
    for flags in [
        vec![],
        vec![("--plan", "spec.json")],
        vec![("--parent", "parent.json")],
        vec![("--input", "input.json"), ("--plan", "spec.json")],
        vec![("--input", "input.json"), ("--parent", "parent.json")],
        vec![
            ("--input", "input.json"),
            ("--plan", "spec.json"),
            ("--parent", "parent.json"),
        ],
        vec![("--input", "input.json"), ("--input", "input.json")],
    ] {
        for command in ["method-revise-prepare", "method-revise-observe"] {
            let before = files.snapshot();
            let result = files.run(command, &flags);
            assert_eq!(result.status.code(), Some(2), "{flags:?}: {result:?}");
            assert!(result.stdout.is_empty());
            assert_eq!(before, files.snapshot());
        }
    }
}
