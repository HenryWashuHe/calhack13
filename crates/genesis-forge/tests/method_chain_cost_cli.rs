use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStage, ChainWork},
    method_chain_costs::{assess_chain_costs, ChainStageEvidence},
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::MethodSynthesisSession,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
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
struct Files {
    root: PathBuf,
    expected: Value,
}
impl Files {
    fn new() -> Self {
        let mut f = Self {
            root: std::env::temp_dir().join(format!(
                "genesis-chain-cost-cli-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
            expected: Value::Null,
        };
        for dir in [
            "declaration",
            "reports",
            "evidence/generation",
            "evidence/reuse",
        ] {
            fs::create_dir_all(f.root.join(dir)).unwrap();
        }
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
        let mut admission = GenerationAdmission::new(roster).unwrap();
        let allocation =
            StageAllocation::prepare(limits.clone(), &admission, std::slice::from_ref(&prepared))
                .unwrap();
        admission.reserve(&prepared).unwrap();
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
        let mut reused = stage.clone();
        reused.limits.id = "reuse-b".into();
        reused.org_id = "org_b".into();
        reused.work = ChainWork::TrialOnly;
        let mut unused = reused.clone();
        unused.limits.id = "declined".into();
        let chain =
            ChainAllocation::new("chain".into(), grant(9000), vec![stage, reused, unused]).unwrap();
        let mut scope = fixture::scope(&[prepared], Some(&trial));
        scope.total_grant = grant(3000);
        let binding = chain.bind_generation("develop-a", &allocation).unwrap();
        let trial_binding = allocation.bind(&trial, &scope).unwrap();
        let rows: Vec<_> = trial
            .slots
            .iter()
            .map(|s| fixture::evidence(&trial, &s.slot_id))
            .collect();
        let mut other = trial.clone();
        other.id = "reuse-trial".into();
        for slot in &mut other.slots {
            slot.org_id = "org_b".into();
            slot.run_id = format!("reuse-{}", slot.slot_id);
            slot.attempt_id = slot.run_id.clone().into();
        }
        let mut other_scope = fixture::scope(&[], Some(&other));
        other_scope.total_grant = grant(3000);
        let other_binding = chain
            .bind_trial_only("reuse-b", &other, &other_scope)
            .unwrap();
        let other_rows: Vec<_> = other
            .slots
            .iter()
            .map(|s| fixture::evidence(&other, &s.slot_id))
            .collect();
        let snapshots = [generated];
        let expected = serde_json::to_value(
            assess_chain_costs(
                &chain,
                &[
                    ChainStageEvidence::Generation {
                        binding: &binding,
                        allocation: &allocation,
                        admission: &admission,
                        scope: &scope,
                        snapshots: &snapshots,
                        trial: Some((&trial_binding, &trial, &rows)),
                    },
                    ChainStageEvidence::TrialOnly {
                        binding: &other_binding,
                        scope: &other_scope,
                        trial: &other,
                        evidence: &other_rows,
                    },
                ],
            )
            .unwrap(),
        )
        .unwrap();
        f.save("declaration/chain.json", &chain);
        f.save("declaration/chain.json.stage-develop-a.json", &binding);
        f.save("declaration/chain.json.stage-reuse-b.json", &other_binding);
        f.save("evidence/generation/allocation.json", &allocation);
        f.save(
            "evidence/generation/allocation.json.trial-binding.json",
            &trial_binding,
        );
        f.save("evidence/generation/admission.json", &admission);
        f.save("evidence/generation/observed.json", &snapshots[0]);
        for (dir, scope, trial, rows) in [
            ("generation", &scope, &trial, &rows),
            ("reuse", &other_scope, &other, &other_rows),
        ] {
            f.save(&format!("evidence/{dir}/scope.json"), scope);
            f.save(&format!("evidence/{dir}/plan.json"), trial);
            f.save(&format!("evidence/{dir}/rows.json"),&rows.iter().map(|r|json!({"slot_id":r.slot_id,"execution":r.execution,"scoring":r.scoring})).collect::<Vec<_>>());
            f.save(&format!("evidence/{dir}/cost.json"),&json!({"schema":"genesis-method-cost-input/1","scope":"scope.json","generation_checkpoints":if dir == "generation" {vec!["observed.json"]} else {vec![]},"trial":{"plan":"plan.json","evidence":"rows.json"}}));
        }
        f.save("reports/input.json",&json!({"schema":"genesis-chain-cost-input/1","stages":[
            {"stage_id":"reuse-b","target":{"kind":"trial_only","cost_manifest":"../evidence/reuse/cost.json"}},
            {"stage_id":"develop-a","target":{"kind":"generation","cost_manifest":"../evidence/generation/cost.json","allocation":"../evidence/generation/allocation.json","admission":"../evidence/generation/admission.json"}}
        ]}));
        f.expected = expected;
        f
    }
    fn save(&self, name: &str, value: &impl Serialize) {
        fs::write(self.root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.root.join(name)).unwrap()).unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .current_dir(std::env::temp_dir())
            .args(args)
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GENESIS_LLM_API_KEY")
            .env("GENESIS_LLM_PROVIDER", "invalid-no-call")
            .output()
            .unwrap()
    }
    fn command(&self) -> Output {
        self.run(&[
            "method-chain-cost-assess",
            "--chain",
            self.root.join("declaration/chain.json").to_str().unwrap(),
            "--manifest",
            self.root.join("reports/input.json").to_str().unwrap(),
        ])
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(path: &Path, output: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(path).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    walk(&p, output);
                } else {
                    output.push((p.clone(), fs::read(p).unwrap()));
                }
            }
        }
        let mut output = vec![];
        walk(&self.root, &mut output);
        output.sort();
        output
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn ok(output: Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn refused(output: Output) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn explicitly_generation_only_stage_reports_known_costs_without_trial_companion() {
    let f = Files::new();
    // Construct a separate synthetic declaration, not a relabeling of real evidence.
    let mut allocation = f.read("evidence/generation/allocation.json");
    allocation["limits"]["max_trial_slots"] = 0.into();
    allocation["limits"]["development_grant"] = json!(grant(0));
    allocation["limits"]["scoring_grant"] = json!(grant(0));
    let allocation: StageAllocation = serde_json::from_value(allocation).unwrap();
    let mut chain = f.read("declaration/chain.json");
    chain["stages"].as_array_mut().unwrap().truncate(2);
    chain["stages"][0]["limits"] = json!(allocation.limits());
    let chain: ChainAllocation = serde_json::from_value(chain).unwrap();
    let binding = chain.bind_generation("develop-a", &allocation).unwrap();
    let trial = serde_json::from_value(f.read("evidence/reuse/plan.json")).unwrap();
    let scope = serde_json::from_value(f.read("evidence/reuse/scope.json")).unwrap();
    let reuse = chain.bind_trial_only("reuse-b", &trial, &scope).unwrap();
    f.save("declaration/chain.json", &chain);
    f.save("declaration/chain.json.stage-develop-a.json", &binding);
    f.save("declaration/chain.json.stage-reuse-b.json", &reuse);
    f.save("evidence/generation/allocation.json", &allocation);
    let mut scope = f.read("evidence/generation/scope.json");
    scope["trial"] = Value::Null;
    f.save("evidence/generation/scope.json", &scope);
    let mut manifest = f.read("evidence/generation/cost.json");
    manifest.as_object_mut().unwrap().remove("trial");
    f.save("evidence/generation/cost.json", &manifest);
    fs::remove_file(
        f.root
            .join("evidence/generation/allocation.json.trial-binding.json"),
    )
    .unwrap();
    let before = f.snapshot();
    let report = ok(f.command());
    assert_eq!(report["stages"][0]["unresolved_trial"], false);
    assert_eq!(report["stages"].as_array().unwrap().len(), 2);
    for (index, total) in report["combined"].as_array().unwrap().iter().enumerate() {
        assert_eq!(total["unknown"], false);
        assert_eq!(total["known_lower_bound"], if index == 4 { 4 } else { 5 });
    }
    assert_eq!(before, f.snapshot());
}

#[test]
fn nested_paths_match_library_and_legacy_stage_report_without_touching_locks() {
    let f = Files::new();
    for path in [
        "declaration/chain.json.lock",
        "declaration/chain.json.stage-develop-a.json.lock",
        "evidence/generation/observed.json.lock",
        "evidence/generation/allocation.json.lock",
        "evidence/generation/admission.json.lock",
    ] {
        fs::write(f.root.join(path), b"other-owner").unwrap();
    }
    let before = f.snapshot();
    let report = ok(f.command());
    assert_eq!(report, f.expected);
    assert_eq!(report["combined"][0]["known_lower_bound"], 9);
    assert_eq!(report["combined"][0]["unknown"], true);
    assert_eq!(report["stages"][0]["stage_id"], "develop-a");
    assert_eq!(report["stages"][2]["binding"], Value::Null);
    assert!(!report.to_string().contains("print(1)"));
    let legacy = ok(f.run(&[
        "method-cost-assess",
        "--manifest",
        f.root
            .join("evidence/generation/cost.json")
            .to_str()
            .unwrap(),
        "--admission",
        f.root
            .join("evidence/generation/admission.json")
            .to_str()
            .unwrap(),
        "--allocation",
        f.root
            .join("evidence/generation/allocation.json")
            .to_str()
            .unwrap(),
    ]));
    assert_eq!(legacy["costs"], report["stages"][0]["observed"]["report"]);
    assert_eq!(f.snapshot(), before);
}

#[test]
fn generation_only_pending_and_omitted_stages_stay_unknown_without_recovery() {
    let f = Files::new();
    let mut input = f.read("reports/input.json");
    input["stages"].as_array_mut().unwrap().remove(0);
    f.save("reports/input.json", &input);
    let mut cost = f.read("evidence/generation/cost.json");
    cost["trial"] = Value::Null;
    f.save("evidence/generation/cost.json", &cost);
    let mut scope = f.read("evidence/generation/scope.json");
    scope["trial"] = Value::Null;
    f.save("evidence/generation/scope.json", &scope);
    fs::remove_file(
        f.root
            .join("evidence/generation/allocation.json.trial-binding.json"),
    )
    .unwrap();
    let observed: MethodSynthesisSession =
        serde_json::from_value(f.read("evidence/generation/observed.json")).unwrap();
    let mut pending = MethodSynthesisSession::new(observed.plan().clone()).unwrap();
    pending.begin().unwrap();
    f.save("evidence/generation/observed.json", &pending);
    let before = f.snapshot();
    let report = ok(f.command());
    assert_eq!(report["stages"][0]["unresolved_trial"], true);
    assert_eq!(
        report["stages"][0]["observed"]["report"]["costs"]["rows"][0]["observation"]["phase"],
        "requesting"
    );
    assert!(report["combined"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["unknown"] == true));
    assert_eq!(f.snapshot(), before);
    input["stages"] = json!([]);
    f.save("reports/input.json", &input);
    let report = ok(f.command());
    assert_eq!(report["stages"].as_array().unwrap().len(), 3);
    assert!(report["combined"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["unknown"] == true && t["known_lower_bound"] == 0));
}

#[test]
fn named_missing_malformed_or_substituted_files_never_become_unknown_or_rebind() {
    for mutation in [
        "missing-cost",
        "missing-chain-binding",
        "missing-trial-binding",
        "bad-admission",
        "wrong-stage",
        "foreign-chain",
        "changed-scope",
    ] {
        let f = Files::new();
        match mutation {
            "missing-cost" => fs::remove_file(f.root.join("evidence/reuse/cost.json")).unwrap(),
            "missing-chain-binding" => {
                fs::remove_file(f.root.join("declaration/chain.json.stage-reuse-b.json")).unwrap()
            }
            "missing-trial-binding" => fs::remove_file(
                f.root
                    .join("evidence/generation/allocation.json.trial-binding.json"),
            )
            .unwrap(),
            "bad-admission" => fs::write(
                f.root.join("evidence/generation/admission.json"),
                b"{broken",
            )
            .unwrap(),
            "wrong-stage" => f.save(
                "declaration/chain.json.stage-reuse-b.json",
                &f.read("declaration/chain.json.stage-develop-a.json"),
            ),
            "foreign-chain" => {
                let mut v = f.read("declaration/chain.json");
                v["id"] = "foreign".into();
                f.save("declaration/chain.json", &v);
            }
            _ => {
                let mut v = f.read("evidence/generation/scope.json");
                v["id"] = "other-scope".into();
                f.save("evidence/generation/scope.json", &v);
            }
        }
        let before = f.snapshot();
        refused(f.command());
        assert_eq!(f.snapshot(), before, "{mutation}");
    }
}

#[test]
fn strict_manifests_reject_extra_generation_inputs_duplicate_stages_and_unknown_flags() {
    for mutation in [
        "schema",
        "duplicate",
        "undeclared",
        "extra",
        "empty-path",
        "extra-allocation",
        "unused-generation",
        "scope-generation",
        "nested-schema",
    ] {
        let f = Files::new();
        let mut input = f.read("reports/input.json");
        match mutation {
            "schema" => input["schema"] = "future".into(),
            "duplicate" => {
                let stage = input["stages"][0].clone();
                input["stages"].as_array_mut().unwrap().push(stage);
            }
            "undeclared" => input["stages"][0]["stage_id"] = "../escape".into(),
            "extra" => input["totals"] = json!({"model_calls":0}),
            "empty-path" => input["stages"][0]["target"]["cost_manifest"] = "".into(),
            "extra-allocation" => {
                input["stages"][0]["target"]["allocation"] =
                    "../evidence/generation/allocation.json".into()
            }
            "unused-generation" => {
                let mut v = f.read("evidence/reuse/cost.json");
                v["generation_checkpoints"] = json!(["../generation/observed.json"]);
                f.save("evidence/reuse/cost.json", &v);
            }
            "scope-generation" => {
                let mut v = f.read("evidence/reuse/scope.json");
                v["generations"] = f.read("evidence/generation/scope.json")["generations"].clone();
                f.save("evidence/reuse/scope.json", &v);
            }
            _ => {
                let mut v = f.read("evidence/reuse/cost.json");
                v["schema"] = "future".into();
                f.save("evidence/reuse/cost.json", &v);
            }
        }
        f.save("reports/input.json", &input);
        let before = f.snapshot();
        refused(f.command());
        assert_eq!(f.snapshot(), before, "{mutation}");
    }
    let f = Files::new();
    refused(f.run(&["method-chain-cost-assess", "--chain", "absent.json"]));
    refused(f.run(&[
        "method-chain-cost-assess",
        "--chain",
        "absent.json",
        "--manifest",
        "absent.json",
        "--retry-read",
    ]));
}
