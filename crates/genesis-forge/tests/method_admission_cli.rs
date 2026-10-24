use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission, method_roster::GenerationRoster,
    method_synthesis::MethodSynthesisSession,
};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "common/method_revision_fixture.rs"]
mod fixture;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-admission-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let files = Self(path);
        let (spec, parent) = fixture::fixture();
        let session = spec.prepare(&parent).unwrap();
        let roster = GenerationRoster::freeze(
            "cli".into(),
            session.plan().settings.clone(),
            ResourceGrant {
                model_calls: 1,
                input_tokens: 100,
                output_tokens: 16384,
                wall_time_ms: 10000,
                simulation_steps: 0,
            },
            std::slice::from_ref(&session),
        )
        .unwrap();
        files.write("generation.json", &session);
        files.write("roster.json", &roster);
        files
    }
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn session(&self) -> MethodSynthesisSession {
        serde_json::from_slice(&fs::read(self.0.join("generation.json")).unwrap()).unwrap()
    }
    fn journal(&self) -> GenerationAdmission {
        serde_json::from_slice(&fs::read(self.0.join("admission.json")).unwrap()).unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(&self.0)
            .args(args)
            .env("GENESIS_LLM_PROVIDER", "anthropic")
            .env("GENESIS_LLM_MODEL", "fixture-model-1")
            .env("GENESIS_LLM_API_KEY", "")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .unwrap()
    }
    fn prepare(&self) -> Output {
        self.run(&[
            "method-admission-prepare",
            "--roster",
            "roster.json",
            "--admission",
            "admission.json",
        ])
    }
    fn step(&self) -> Output {
        self.run(&[
            "method-generate-step",
            "--checkpoint",
            "generation.json",
            "--admission",
            "admission.json",
        ])
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
fn prepare_never_replaces_and_status_is_read_only_with_existing_locks() {
    let files = Files::new();
    assert!(files.prepare().status.success());
    let before = files.snapshot();
    assert_eq!(files.prepare().status.code(), Some(2));
    assert_eq!(files.snapshot(), before);
    fs::write(files.0.join("admission.json.lock"), "sentinel").unwrap();
    let before = files.snapshot();
    let result = files.run(&["method-admission-status", "--admission", "admission.json"]);
    assert!(result.status.success());
    let status: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["reserved_attempts"], 0);
    assert_eq!(files.snapshot(), before);
    assert_eq!(files.step().status.code(), Some(2));
    assert_eq!(files.snapshot(), before);
}

#[test]
fn invalid_credentials_and_foreign_snapshot_do_not_reserve_or_change_inputs() {
    let files = Files::new();
    assert!(files.prepare().status.success());
    let before = files.snapshot();
    let result = files.step();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("API_KEY is required"));
    assert_eq!(files.snapshot(), before);
    let mut plan = files.session().plan().clone();
    plan.instruction.push_str(" different");
    files.write(
        "generation.json",
        &MethodSynthesisSession::new(plan).unwrap(),
    );
    let before = files.snapshot();
    let result = files.step();
    assert_eq!(result.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("API_KEY"));
    assert_eq!(files.snapshot(), before);
}

#[test]
fn stranded_reservation_blocks_even_a_copied_prepared_checkpoint_before_credentials() {
    let files = Files::new();
    assert!(files.prepare().status.success());
    let mut journal = files.journal();
    journal.reserve(&files.session()).unwrap();
    files.write("admission.json", &journal);
    fs::copy(files.0.join("generation.json"), files.0.join("copy.json")).unwrap();
    let before = files.snapshot();
    let result = files.run(&[
        "method-generate-step",
        "--checkpoint",
        "copy.json",
        "--admission",
        "admission.json",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("already reserved"));
    assert!(result.stdout.is_empty());
    assert_eq!(files.snapshot(), before);
}

#[test]
fn advanced_unreserved_is_refused_but_reserved_recovery_and_legacy_step_need_no_key() {
    let files = Files::new();
    assert!(files.prepare().status.success());
    let prepared = files.session();
    let mut pending = prepared.clone();
    pending.begin().unwrap();
    files.write("generation.json", &pending);
    let before = files.snapshot();
    assert_eq!(files.step().status.code(), Some(2));
    assert_eq!(files.snapshot(), before);
    let mut journal = files.journal();
    journal.reserve(&prepared).unwrap();
    files.write("admission.json", &journal);
    let reserved = fs::read(files.0.join("admission.json")).unwrap();
    assert_eq!(files.step().status.code(), Some(1));
    assert_eq!(
        files.session().phase(),
        genesis_forge::method_synthesis::MethodSynthesisPhase::NeedsReconciliation
    );
    assert_eq!(fs::read(files.0.join("admission.json")).unwrap(), reserved);
    let mut generated = prepared;
    fixture::complete(&mut generated, fixture::SOURCE);
    files.write("generation.json", &generated);
    assert!(files.step().status.success());
    let before = files.snapshot();
    assert!(files
        .run(&["method-generate-step", "--checkpoint", "generation.json"])
        .status
        .success());
    assert_eq!(files.snapshot(), before);
    // Terminal success cannot be retroactively labeled admitted by a fresh journal.
    let roster: GenerationRoster =
        serde_json::from_slice(&fs::read(files.0.join("roster.json")).unwrap()).unwrap();
    files.write("admission.json", &GenerationAdmission::new(roster).unwrap());
    let before = files.snapshot();
    assert_eq!(files.step().status.code(), Some(2));
    assert_eq!(files.snapshot(), before);
}
