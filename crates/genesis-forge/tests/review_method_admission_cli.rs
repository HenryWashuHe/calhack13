//! Independent review guards for the two-file admission CLI.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission, method_roster::GenerationRoster,
    method_synthesis::MethodSynthesisSession,
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "common/method_plan.rs"]
mod fixture;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-review-admission-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let session = MethodSynthesisSession::new(fixture::plan()).unwrap();
        let roster = GenerationRoster::freeze(
            "review".into(),
            session.plan().settings.clone(),
            ResourceGrant {
                model_calls: 1,
                input_tokens: 1,
                output_tokens: u64::from(session.plan().settings.max_tokens),
                wall_time_ms: 1,
                simulation_steps: 0,
            },
            std::slice::from_ref(&session),
        )
        .unwrap();
        fs::write(
            path.join("generation.json"),
            serde_json::to_vec(&session).unwrap(),
        )
        .unwrap();
        fs::write(
            path.join("roster.json"),
            serde_json::to_vec(&roster).unwrap(),
        )
        .unwrap();
        fs::write(
            path.join("admission.json"),
            serde_json::to_vec(&GenerationAdmission::new(roster).unwrap()).unwrap(),
        )
        .unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(&self.0)
            .args(args)
            .env_remove("GENESIS_LLM_PROVIDER")
            .env_remove("GENESIS_LLM_MODEL")
            .env_remove("GENESIS_LLM_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .unwrap()
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut result: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        result.sort();
        result
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn occupied_session_lock_missing_journal_and_aliases_preserve_all_inputs() {
    let files = Files::new();
    fs::write(files.0.join("generation.json.lock"), b"other owner").unwrap();
    let before = files.snapshot();
    let blocked = files.run(&[
        "method-generate-step",
        "--checkpoint",
        "generation.json",
        "--admission",
        "admission.json",
    ]);
    assert_eq!(blocked.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("lock already exists"));
    assert_eq!(
        files.snapshot(),
        before,
        "must release only its own admission lock"
    );
    fs::remove_file(files.0.join("generation.json.lock")).unwrap();

    let before = files.snapshot();
    for args in [
        vec![
            "method-generate-step",
            "--checkpoint",
            "generation.json",
            "--admission",
            "missing.json",
        ],
        vec![
            "method-generate-step",
            "--checkpoint",
            "generation.json",
            "--admission",
            "generation.json",
        ],
        vec![
            "method-generate-step",
            "--checkpoint",
            "admission.json",
            "--admission",
            "admission.json",
        ],
        vec![
            "method-admission-prepare",
            "--roster",
            "roster.json",
            "--admission",
            "roster.json",
        ],
    ] {
        let result = files.run(&args);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(
            result.stdout.is_empty(),
            "must not emit successful admission"
        );
        assert!(!String::from_utf8_lossy(&result.stderr).contains("API_KEY"));
        assert_eq!(files.snapshot(), before, "{args:?}");
    }
    let journal: GenerationAdmission =
        serde_json::from_slice(&fs::read(files.0.join("admission.json")).unwrap()).unwrap();
    assert_eq!(journal.summary().unwrap().reserved_attempts, 0);
}
