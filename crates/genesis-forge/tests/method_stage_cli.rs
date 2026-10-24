use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission, method_roster::GenerationRoster,
    method_stage::StageLimits, method_synthesis::MethodSynthesisSession,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "common/method_accounting_fixture.rs"]
mod fixture;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let f = Self(std::env::temp_dir().join(format!(
                "genesis-stage-cli-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )));
        fs::create_dir(&f.0).unwrap();
        let generated = fixture::generation("one", true);
        let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
        let trial = fixture::trial(&generated);
        let admission = GenerationAdmission::new(
            GenerationRoster::freeze(
                "stage".into(),
                prepared.plan().settings.clone(),
                ResourceGrant {
                    model_calls: 1,
                    input_tokens: 0,
                    output_tokens: 512,
                    wall_time_ms: 0,
                    simulation_steps: 0,
                },
                std::slice::from_ref(&prepared),
            )
            .unwrap(),
        )
        .unwrap();
        let limits = StageLimits {
            id: "stage-a".into(),
            max_trial_slots: 2,
            development_grant: trial.development_grant.clone(),
            scoring_grant: trial.scoring_grant.clone(),
            total_grant: ResourceGrant {
                model_calls: 5,
                input_tokens: 40,
                output_tokens: 552,
                wall_time_ms: 400,
                simulation_steps: 400,
            },
        };
        let mut scope = fixture::scope(std::slice::from_ref(&prepared), Some(&trial));
        scope.total_grant = limits.total_grant.clone();
        f.save("generation.json", &prepared);
        f.save("admission.json", &admission);
        f.save("trial.json", &trial);
        f.save("scope.json", &scope);
        f.save("stage-input.json",&json!({"schema":"genesis-stage-input/1","limits":limits,"prepared_checkpoints":["generation.json"]}));
        f.save("bind-input.json",&json!({"schema":"genesis-stage-trial-input/1","plan":"trial.json","scope":"scope.json"}));
        let e = fixture::evidence(&trial, "candidate");
        f.save(
            "evidence.json",
            &vec![json!({"slot_id":e.slot_id,"execution":e.execution,"scoring":e.scoring})],
        );
        f.save("cost-input.json",&json!({"schema":"genesis-method-cost-input/1","scope":"scope.json","generation_checkpoints":["generation.json"],"trial":{"plan":"trial.json","evidence":"evidence.json"}}));
        f
    }
    fn save(&self, name: &str, value: &impl Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.0.join(name)).unwrap()).unwrap()
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                let b = fs::read(&p).unwrap();
                (p, b)
            })
            .collect();
        out.sort();
        out
    }
    fn run(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        cmd.current_dir(&self.0).args(args);
        for k in [
            "GENESIS_LLM_PROVIDER",
            "GENESIS_LLM_MODEL",
            "GENESIS_LLM_API_KEY",
            "ANTHROPIC_API_KEY",
            "APIFY_TOKEN",
        ] {
            cmd.env_remove(k);
        }
        cmd.env("GENESIS_LLM_PROVIDER", "invalid-no-live-call")
            .output()
            .unwrap()
    }
    fn prepare(&self) -> Output {
        self.run(&[
            "method-stage-prepare",
            "--manifest",
            "stage-input.json",
            "--admission",
            "admission.json",
            "--allocation",
            "stage.json",
        ])
    }
    fn bind(&self) -> Output {
        self.run(&[
            "method-stage-bind",
            "--manifest",
            "bind-input.json",
            "--allocation",
            "stage.json",
        ])
    }
    fn assess(&self) -> Output {
        self.run(&[
            "method-cost-assess",
            "--manifest",
            "cost-input.json",
            "--admission",
            "admission.json",
            "--allocation",
            "stage.json",
        ])
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
fn rejected(o: Output) {
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
}

#[test]
fn prepare_bind_and_read_only_assessment_preserve_inputs_and_separate_grants() {
    let f = Files::new();
    let original = f.snapshot();
    ok(f.prepare());
    ok(f.bind());
    for (p, b) in original {
        assert_eq!(fs::read(p).unwrap(), b);
    }
    let before = f.snapshot();
    let report = ok(f.assess());
    assert_eq!(report["schema"], "genesis-stage-cost-report/1");
    assert_eq!(
        report["costs"]["costs"]["rows"].as_array().unwrap().len(),
        5
    );
    assert_eq!(report["costs"]["costs"]["combined"][0]["unknown"], true);
    assert_eq!(f.snapshot(), before);
    let mut journal: GenerationAdmission =
        serde_json::from_value(f.read("admission.json")).unwrap();
    let prepared: MethodSynthesisSession =
        serde_json::from_value(f.read("generation.json")).unwrap();
    journal.reserve(&prepared).unwrap();
    f.save("admission.json", &journal);
    f.save("generation.json", &fixture::generation("one", false));
    let report = ok(f.assess());
    assert_eq!(
        report["costs"]["generation"]["costs"]["combined"][1]["limit_exceeded"],
        true
    );
    assert_eq!(
        report["costs"]["costs"]["combined"][1]["limit_exceeded"],
        false
    );
}

#[test]
fn canonical_companion_refuses_replacement_and_missing_or_changed_binding_never_falls_back() {
    let f = Files::new();
    ok(f.prepare());
    let before = f.snapshot();
    rejected(f.assess());
    assert_eq!(f.snapshot(), before);
    ok(f.bind());
    let before = f.snapshot();
    rejected(f.prepare());
    rejected(f.bind());
    assert_eq!(f.snapshot(), before);
    let mut trial: genesis_core::development_trial::TrialPlan =
        serde_json::from_value(f.read("trial.json")).unwrap();
    trial.id = "other-valid".into();
    let mut scope = f.read("scope.json");
    scope["trial"]["plan"] = serde_json::to_value(trial.reference().unwrap()).unwrap();
    f.save("trial.json", &trial);
    f.save("scope.json", &scope);
    let before = f.snapshot();
    rejected(f.bind());
    rejected(f.assess());
    assert_eq!(f.snapshot(), before);
}

#[test]
fn occupied_journal_aliases_and_started_attempts_cannot_prepare_allocation() {
    let f = Files::new();
    fs::write(f.0.join("admission.json.lock"), b"other owner").unwrap();
    let before = f.snapshot();
    rejected(f.prepare());
    assert_eq!(f.snapshot(), before);
    fs::remove_file(f.0.join("admission.json.lock")).unwrap();
    let before = f.snapshot();
    rejected(f.run(&[
        "method-stage-prepare",
        "--manifest",
        "stage-input.json",
        "--admission",
        "admission.json",
        "--allocation",
        "admission.json",
    ]));
    assert_eq!(f.snapshot(), before);
    let mut prepared: MethodSynthesisSession =
        serde_json::from_value(f.read("generation.json")).unwrap();
    let mut journal: GenerationAdmission =
        serde_json::from_value(f.read("admission.json")).unwrap();
    journal.reserve(&prepared).unwrap();
    f.save("admission.json", &journal);
    let before = f.snapshot();
    rejected(f.prepare());
    assert_eq!(f.snapshot(), before);
    prepared.begin().unwrap();
    f.save("generation.json", &prepared);
    let before = f.snapshot();
    rejected(f.prepare());
    assert_eq!(f.snapshot(), before);
}

#[test]
fn generation_allocation_guard_runs_before_credentials_or_reservation() {
    let f = Files::new();
    ok(f.prepare());
    let before = f.snapshot();
    rejected(f.run(&[
        "method-generate-step",
        "--checkpoint",
        "generation.json",
        "--allocation",
        "stage.json",
    ]));
    rejected(f.run(&[
        "method-cost-assess",
        "--manifest",
        "cost-input.json",
        "--allocation",
        "stage.json",
    ]));
    assert_eq!(f.snapshot(), before);
    let mut foreign = f.read("stage.json");
    foreign["roster"]["id"] = "other-roster".into();
    f.save("foreign.json", &foreign);
    let before = f.snapshot();
    let result = f.run(&[
        "method-generate-step",
        "--checkpoint",
        "generation.json",
        "--admission",
        "admission.json",
        "--allocation",
        "foreign.json",
    ]);
    assert!(String::from_utf8_lossy(&result.stderr).contains("admission differs"));
    rejected(result);
    assert_eq!(f.snapshot(), before);
    let result = f.run(&[
        "method-generate-step",
        "--checkpoint",
        "generation.json",
        "--admission",
        "admission.json",
        "--allocation",
        "stage.json",
    ]);
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("requires GENESIS_LLM_PROVIDER=anthropic")
    );
    rejected(result);
    assert_eq!(f.snapshot(), before);
}
