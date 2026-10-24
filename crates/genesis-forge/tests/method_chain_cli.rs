use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStage, ChainWork},
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::MethodSynthesisSession,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[allow(dead_code)]
#[path = "common/method_accounting_fixture.rs"]
mod fixture;
static NEXT: AtomicU64 = AtomicU64::new(0);
fn grant(n: u64) -> ResourceGrant {
    ResourceGrant {
        model_calls: n,
        input_tokens: n,
        output_tokens: n,
        wall_time_ms: n,
        simulation_steps: n,
    }
}
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let f = Self(std::env::temp_dir().join(format!(
                "genesis-chain-cli-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        fs::create_dir(&f.0).unwrap();
        fs::create_dir(f.0.join("inputs")).unwrap();
        let generated = fixture::generation("one", true);
        let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
        let mut trial = fixture::trial(&generated);
        for slot in &mut trial.slots {
            slot.org_id = "org_a".into();
        }
        let limits = StageLimits {
            id: "develop-a".into(),
            max_trial_slots: 2,
            development_grant: trial.development_grant.clone(),
            scoring_grant: trial.scoring_grant.clone(),
            total_grant: grant(3000),
        };
        let roster = GenerationRoster::freeze(
            "roster".into(),
            prepared.plan().settings.clone(),
            grant(2000),
            std::slice::from_ref(&prepared),
        )
        .unwrap();
        let allocation = StageAllocation::prepare(
            limits.clone(),
            &GenerationAdmission::new(roster).unwrap(),
            std::slice::from_ref(&prepared),
        )
        .unwrap();
        let stage = ChainStage {
            org_id: "org_a".into(),
            limits,
            trial_model_settings: trial.model_settings.clone(),
            work: ChainWork::Generation {
                settings: prepared.plan().settings.clone(),
                max_attempts: 1,
                grant: grant(2000),
            },
        };
        let mut returned = stage.clone();
        returned.limits.id = "return-a".into();
        returned.work = ChainWork::TrialOnly;
        let chain =
            ChainAllocation::new("chain".into(), grant(6000), vec![stage, returned]).unwrap();
        let mut scope = fixture::scope(&[prepared], Some(&trial));
        scope.total_grant = grant(3000);
        f.save("inputs/chain-input.json", &chain);
        f.save("inputs/allocation.json", &allocation);
        f.save(
            "inputs/allocation.json.trial-binding.json",
            &allocation.bind(&trial, &scope).unwrap(),
        );
        f.save("inputs/scope.json", &scope);
        f.save("inputs/trial.json", &trial);
        scope.generations.clear();
        f.save("inputs/return-scope.json", &scope);
        f.save("inputs/generation.json", &json!({"schema":"genesis-chain-stage-input/1", "stage_id":"develop-a", "target":{"kind":"generation", "allocation":"allocation.json"}}));
        f.save("inputs/trial.json.input", &json!({"schema":"genesis-chain-stage-input/1", "stage_id":"develop-a", "target":{"kind":"generation", "allocation":"allocation.json", "trial":{"plan":"trial.json", "scope":"scope.json"}}}));
        f.save("inputs/return.json", &json!({"schema":"genesis-chain-stage-input/1", "stage_id":"return-a", "target":{"kind":"trial_only", "plan":"trial.json", "scope":"return-scope.json"}}));
        f
    }
    fn save(&self, name: &str, value: &impl Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.0.join(name)).unwrap()).unwrap()
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = vec![];
        for directory in [&self.0, &self.0.join("inputs")] {
            for entry in fs::read_dir(directory).unwrap() {
                let p = entry.unwrap().path();
                if p.is_file() {
                    files.push((p.clone(), fs::read(p).unwrap()));
                }
            }
        }
        files.sort();
        files
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(&self.0)
            .args(args)
            .env_remove("GENESIS_LLM_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env("GENESIS_LLM_PROVIDER", "invalid-no-call")
            .output()
            .unwrap()
    }
    fn command(&self, command: &str, manifest: &str) -> Output {
        self.run(&[command, "--manifest", manifest, "--chain", "chain.json"])
    }
    fn prepare(&self) {
        ok(self.command("method-chain-prepare", "inputs/chain-input.json"));
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn ok(o: Output) -> Value {
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}
fn refused(o: Output) {
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
}

#[test]
fn exact_generation_and_return_bindings_are_create_only_and_checks_are_read_only() {
    let f = Files::new();
    let initial = f.snapshot();
    f.prepare();
    let generation = ok(f.command("method-chain-bind", "inputs/generation.json"));
    let returned = ok(f.command("method-chain-bind", "inputs/return.json"));
    assert_eq!(returned["target"]["kind"], "trial_only");
    for (path, bytes) in initial {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    let before = f.snapshot();
    assert_eq!(
        ok(f.command("method-chain-check", "inputs/generation.json")),
        generation
    );
    assert_eq!(
        ok(f.command("method-chain-check", "inputs/trial.json.input")),
        generation
    );
    assert_eq!(
        ok(f.command("method-chain-check", "inputs/return.json")),
        returned
    );
    refused(f.command("method-chain-prepare", "inputs/chain-input.json"));
    refused(f.command("method-chain-bind", "inputs/generation.json"));
    refused(f.command("method-chain-bind", "inputs/return.json"));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn missing_and_substituted_retained_records_never_fall_back_or_rebind() {
    let f = Files::new();
    f.prepare();
    let before = f.snapshot();
    refused(f.command("method-chain-check", "inputs/return.json"));
    assert_eq!(f.snapshot(), before);
    ok(f.command("method-chain-bind", "inputs/return.json"));
    let mut trial: genesis_core::development_trial::TrialPlan =
        serde_json::from_value(f.read("inputs/trial.json")).unwrap();
    trial.id = "another-valid-trial".into();
    let mut scope = f.read("inputs/return-scope.json");
    scope["trial"]["plan"] = serde_json::to_value(trial.reference().unwrap()).unwrap();
    f.save("inputs/trial.json", &trial);
    f.save("inputs/return-scope.json", &scope);
    let before = f.snapshot();
    refused(f.command("method-chain-check", "inputs/return.json"));
    refused(f.command("method-chain-bind", "inputs/return.json"));
    assert_eq!(f.snapshot(), before);
    // A changed chain with valid structural ceilings still cannot reuse the binding.
    let f = Files::new();
    f.prepare();
    ok(f.command("method-chain-bind", "inputs/generation.json"));
    let mut chain = f.read("chain.json");
    chain["id"] = "different-chain".into();
    f.save("chain.json", &chain);
    let before = f.snapshot();
    refused(f.command("method-chain-check", "inputs/generation.json"));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn malformed_inputs_and_wrong_stage_kinds_leave_no_binding_or_input_changes() {
    for mutation in ["schema", "stage", "path", "kind", "unknown"] {
        let f = Files::new();
        f.prepare();
        let mut input = f.read("inputs/generation.json");
        match mutation {
            "schema" => input["schema"] = "future".into(),
            "stage" => input["stage_id"] = "../escape".into(),
            "path" => input["target"]["allocation"] = "".into(),
            "kind" => input["stage_id"] = "return-a".into(),
            _ => input["target"]["extra"] = true.into(),
        }
        f.save("inputs/generation.json", &input);
        let before = f.snapshot();
        refused(f.command("method-chain-bind", "inputs/generation.json"));
        assert_eq!(f.snapshot(), before, "{mutation}");
    }
    let f = Files::new();
    let mut chain = f.read("inputs/chain-input.json");
    chain["total_grant"]["input_tokens"] = 5999.into();
    f.save("inputs/chain-input.json", &chain);
    let before = f.snapshot();
    refused(f.command("method-chain-prepare", "inputs/chain-input.json"));
    refused(f.run(&["method-chain-check", "--chain", "chain.json"]));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn existing_chain_locks_block_writes_but_do_not_mutate_read_only_checks() {
    let f = Files::new();
    f.prepare();
    ok(f.command("method-chain-bind", "inputs/generation.json"));
    for path in ["chain.json.lock", "chain.json.stage-develop-a.json.lock"] {
        fs::write(f.0.join(path), b"other-owner").unwrap();
    }
    let before = f.snapshot();
    refused(f.command("method-chain-bind", "inputs/return.json"));
    ok(f.command("method-chain-check", "inputs/generation.json"));
    assert_eq!(f.snapshot(), before);
    let f = Files::new();
    f.prepare();
    fs::write(
        f.0.join("chain.json.stage-return-a.json.lock"),
        b"stage-owner",
    )
    .unwrap();
    let before = f.snapshot();
    refused(f.command("method-chain-bind", "inputs/return.json"));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn generated_trial_requires_exact_separately_retained_stage_binding() {
    let f = Files::new();
    f.prepare();
    ok(f.command("method-chain-bind", "inputs/generation.json"));
    let mut scope = f.read("inputs/scope.json");
    scope["id"] = "changed-scope".into();
    f.save("inputs/scope.json", &scope);
    let before = f.snapshot();
    refused(f.command("method-chain-check", "inputs/trial.json.input"));
    assert_eq!(f.snapshot(), before);
    fs::remove_file(f.0.join("inputs/allocation.json.trial-binding.json")).unwrap();
    let before = f.snapshot();
    refused(f.command("method-chain-check", "inputs/trial.json.input"));
    assert_eq!(f.snapshot(), before);
    // Without a trial request this is only an allocation check, explicitly.
    ok(f.command("method-chain-check", "inputs/generation.json"));
}
