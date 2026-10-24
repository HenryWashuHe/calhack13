//! Independent PR86 review: allocation validation precedes pending recovery.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
    text_generation::GenerationTransport,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};
#[path = "common/method_plan.rs"]
mod fixture;

struct Files(PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Files {
    fn run(&self, allocation: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(&self.0)
            .args([
                "method-generate-step",
                "--checkpoint",
                "generation.json",
                "--admission",
                "admission.json",
                "--allocation",
                allocation,
            ])
            .env("GENESIS_LLM_PROVIDER", "invalid-must-not-be-consulted")
            .env_remove("GENESIS_LLM_MODEL")
            .env_remove("GENESIS_LLM_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .unwrap()
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

#[test]
fn invalid_allocation_cannot_recover_pending_but_exact_allocation_recovers_without_refund() {
    let files = Files(std::env::temp_dir().join(format!(
        "genesis-review-stage-recovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    fs::create_dir(&files.0).unwrap();
    let mut session = MethodSynthesisSession::new(fixture::plan()).unwrap();
    let grant = ResourceGrant {
        model_calls: 1,
        input_tokens: 0,
        output_tokens: 512,
        wall_time_ms: 0,
        simulation_steps: 0,
    };
    let roster = GenerationRoster::freeze(
        "stage-roster".into(),
        session.plan().settings.clone(),
        grant.clone(),
        std::slice::from_ref(&session),
    )
    .unwrap();
    let mut admission = GenerationAdmission::new(roster).unwrap();
    let zero = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 0,
        simulation_steps: 0,
    };
    let allocation = StageAllocation::prepare(
        StageLimits {
            id: "stage-a".into(),
            max_trial_slots: 1,
            development_grant: zero.clone(),
            scoring_grant: zero,
            total_grant: grant,
        },
        &admission,
        std::slice::from_ref(&session),
    )
    .unwrap();
    let mut foreign = serde_json::to_value(&allocation).unwrap();
    foreign["roster"]["id"] = json!("other-valid-roster");
    let foreign: StageAllocation = serde_json::from_value(foreign).unwrap();
    admission.reserve(&session).unwrap();
    session.begin().unwrap();
    let request_hash = session.request_sha256().to_owned();
    for (name, bytes) in [
        ("generation.json", serde_json::to_vec(&session).unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        ("stage.json", serde_json::to_vec(&allocation).unwrap()),
        ("foreign.json", serde_json::to_vec(&foreign).unwrap()),
    ] {
        fs::write(files.0.join(name), bytes).unwrap();
    }
    let before = files.snapshot();
    for path in ["missing.json", "foreign.json"] {
        let output = files.run(path);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(files.snapshot(), before);
    }
    let output = files.run("stage.json");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["transport"], "unobserved");
    let recovered: MethodSynthesisSession =
        serde_json::from_slice(&fs::read(files.0.join("generation.json")).unwrap()).unwrap();
    assert_eq!(recovered.phase(), MethodSynthesisPhase::NeedsReconciliation);
    assert_eq!(recovered.request_sha256(), request_hash);
    assert_eq!(
        recovered.completion().unwrap().transport,
        GenerationTransport::Unobserved
    );
    for (path, bytes) in before {
        if path.file_name().unwrap() != "generation.json" {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    let after = files.snapshot();
    let repeated = files.run("stage.json");
    assert_eq!(repeated.status.code(), Some(1), "{repeated:?}");
    assert_eq!(files.snapshot(), after);
}
