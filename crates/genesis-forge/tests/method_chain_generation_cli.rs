//! Optional retained-chain checks precede recovery, credentials and reservation.
//! These tests never call a provider or execute generated source.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStage, ChainWork},
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
    text_generation::GenerationTransport,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "common/method_plan.rs"]
mod fixture;
static NEXT: AtomicU64 = AtomicU64::new(0);
const COMPANION: &str = "chain.json.stage-develop-a.json";

fn grant(value: u64) -> ResourceGrant {
    ResourceGrant {
        model_calls: value,
        input_tokens: value,
        output_tokens: value,
        wall_time_ms: value,
        simulation_steps: value,
    }
}

struct Files(PathBuf);
impl Files {
    fn new(pending: bool) -> Self {
        let f = Self(std::env::temp_dir().join(format!(
            "genesis-chain-generation-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        fs::create_dir(&f.0).unwrap();
        let mut session = MethodSynthesisSession::new(fixture::plan()).unwrap();
        let limits = StageLimits {
            id: "develop-a".into(),
            max_trial_slots: 2,
            development_grant: grant(0),
            scoring_grant: grant(0),
            total_grant: grant(1000),
        };
        let roster = GenerationRoster::freeze(
            "roster".into(),
            session.plan().settings.clone(),
            grant(1000),
            std::slice::from_ref(&session),
        )
        .unwrap();
        let mut journal = GenerationAdmission::new(roster).unwrap();
        let allocation =
            StageAllocation::prepare(limits.clone(), &journal, std::slice::from_ref(&session))
                .unwrap();
        let chain = ChainAllocation::new(
            "chain".into(),
            grant(1000),
            vec![ChainStage {
                org_id: session.plan().org_id.clone(),
                limits,
                trial_model_settings: fixture::reference("trial-settings", b"settings"),
                work: ChainWork::Generation {
                    settings: session.plan().settings.clone(),
                    max_attempts: 1,
                    grant: grant(1000),
                },
            }],
        )
        .unwrap();
        f.save("chain.json", &chain);
        f.save(
            COMPANION,
            &chain.bind_generation("develop-a", &allocation).unwrap(),
        );
        f.save("allocation.json", &allocation);
        if pending {
            journal.reserve(&session).unwrap();
            session.begin().unwrap();
        }
        f.save("admission.json", &journal);
        f.save("generation.json", &session);
        f
    }
    fn save(&self, path: &str, value: &impl Serialize) {
        fs::write(self.0.join(path), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read<T: serde::de::DeserializeOwned>(&self, path: &str) -> T {
        serde_json::from_slice(&fs::read(self.0.join(path)).unwrap()).unwrap()
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let p = entry.unwrap().path();
                let bytes = fs::read(&p).unwrap();
                (p, bytes)
            })
            .collect();
        files.sort();
        files
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(&self.0)
            .args(args)
            .env("GENESIS_LLM_PROVIDER", "invalid-must-not-be-consulted")
            .env_remove("GENESIS_LLM_MODEL")
            .env_remove("GENESIS_LLM_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .unwrap()
    }
    fn step(&self, chain: bool) -> Output {
        let mut args = vec![
            "method-generate-step",
            "--checkpoint",
            "generation.json",
            "--admission",
            "admission.json",
            "--allocation",
            "allocation.json",
        ];
        if chain {
            args.extend(["--chain", "chain.json"]);
        }
        self.run(&args)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn refused(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn invalid_chain_or_companion_cannot_change_prepared_or_pending_state() {
    for pending in [false, true] {
        for mutation in [
            "missing-chain",
            "invalid-chain",
            "foreign-chain",
            "missing-companion",
            "invalid-companion",
            "foreign-companion",
            "wrong-stage",
            "wrong-allocation",
        ] {
            let f = Files::new(pending);
            match mutation {
                "missing-chain" => fs::remove_file(f.0.join("chain.json")).unwrap(),
                "invalid-chain" => f.save("chain.json", &serde_json::json!({})),
                "foreign-chain" | "foreign-companion" => {
                    let mut chain: Value = f.read("chain.json");
                    chain["id"] = "another-valid-chain".into();
                    let chain: ChainAllocation = serde_json::from_value(chain).unwrap();
                    if mutation == "foreign-chain" {
                        f.save("chain.json", &chain);
                    } else {
                        let allocation: StageAllocation = f.read("allocation.json");
                        f.save(
                            COMPANION,
                            &chain.bind_generation("develop-a", &allocation).unwrap(),
                        );
                    }
                }
                "missing-companion" => fs::remove_file(f.0.join(COMPANION)).unwrap(),
                "invalid-companion" => f.save(COMPANION, &serde_json::json!({})),
                _ => {
                    let mut binding: Value = f.read(COMPANION);
                    if mutation == "wrong-stage" {
                        binding["stage_id"] = "other-stage".into();
                    } else {
                        binding["target"]["allocation_sha256"] = "a".repeat(64).into();
                    }
                    f.save(COMPANION, &binding);
                }
            }
            let before = f.snapshot();
            let error = refused(&f.step(true));
            assert!(
                !error.contains("GENESIS_LLM_PROVIDER"),
                "{mutation}: {error}"
            );
            assert_eq!(f.snapshot(), before, "pending={pending}/{mutation}");
        }
    }
}

#[test]
fn valid_chain_prepared_step_reaches_credentials_without_reserving() {
    let f = Files::new(false);
    let before = f.snapshot();
    assert!(refused(&f.step(true)).contains("GENESIS_LLM_PROVIDER=anthropic"));
    assert_eq!(f.snapshot(), before);
    let journal: GenerationAdmission = f.read("admission.json");
    assert_eq!(journal.summary().unwrap().reserved_attempts, 0);
}

#[test]
fn valid_chain_recovers_pending_once_without_refund_or_credentials() {
    let f = Files::new(true);
    let pending: MethodSynthesisSession = f.read("generation.json");
    let before = f.snapshot();
    let output = f.step(true);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["transport"], "unobserved");
    let recovered: MethodSynthesisSession = f.read("generation.json");
    assert_eq!(recovered.phase(), MethodSynthesisPhase::NeedsReconciliation);
    assert_eq!(recovered.request_sha256(), pending.request_sha256());
    assert_eq!(
        recovered.completion().unwrap().transport,
        GenerationTransport::Unobserved
    );
    let journal: GenerationAdmission = f.read("admission.json");
    assert_eq!(journal.summary().unwrap().reserved_attempts, 1);
    for (path, bytes) in before {
        if path.file_name().unwrap() != "generation.json" {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    let after = f.snapshot();
    assert_eq!(f.step(true).status.code(), Some(1));
    assert_eq!(f.snapshot(), after);
}

#[test]
fn chain_requires_allocation_and_admission_and_is_not_a_general_generation_flag() {
    let f = Files::new(true);
    let before = f.snapshot();
    assert!(refused(&f.run(&[
        "method-generate-step",
        "--checkpoint",
        "generation.json",
        "--chain",
        "chain.json",
    ]))
    .contains("--chain requires --allocation"));
    assert!(refused(&f.run(&[
        "method-generate-step",
        "--checkpoint",
        "generation.json",
        "--allocation",
        "allocation.json",
        "--chain",
        "chain.json",
    ]))
    .contains("--allocation requires --admission"));
    for command in [
        "method-generate-status",
        "method-generate-export",
        "method-generate-prepare",
    ] {
        assert!(refused(&f.run(&[
            command,
            "--checkpoint",
            "generation.json",
            "--chain",
            "chain.json"
        ]))
        .contains("unknown argument"));
    }
    for flag in ["--stage", "--binding"] {
        assert!(refused(&f.run(&[
            "method-generate-step",
            "--checkpoint",
            "generation.json",
            flag,
            "other"
        ]))
        .contains("unknown argument"));
    }
    assert_eq!(f.snapshot(), before);
}

#[test]
fn omitting_chain_preserves_existing_prepared_and_pending_behavior() {
    for pending in [false, true] {
        let f = Files::new(pending);
        fs::remove_file(f.0.join("chain.json")).unwrap();
        fs::remove_file(f.0.join(COMPANION)).unwrap();
        let journal_before = fs::read(f.0.join("admission.json")).unwrap();
        let output = f.step(false);
        if pending {
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            let recovered: MethodSynthesisSession = f.read("generation.json");
            assert_eq!(recovered.phase(), MethodSynthesisPhase::NeedsReconciliation);
        } else {
            assert!(refused(&output).contains("GENESIS_LLM_PROVIDER=anthropic"));
        }
        assert_eq!(
            fs::read(f.0.join("admission.json")).unwrap(),
            journal_before
        );
    }
}
