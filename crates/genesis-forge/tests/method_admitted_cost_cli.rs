use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission, method_roster::GenerationRoster,
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
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new(reserved: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-admitted-cost-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let f = Self(path);
        let generated = fixture::generation("one", true);
        let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
        let mut admission = GenerationAdmission::new(
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
        if reserved {
            admission.reserve(&prepared).unwrap();
        }
        let plan = fixture::trial(&generated);
        f.save("admission.json", &admission);
        f.save("generation.json", &generated);
        f.save("prepared.json", &prepared);
        f.save("scope.json", &fixture::scope(&[prepared], Some(&plan)));
        f.save("plan.json", &plan);
        f.save(
            "evidence.json",
            &vec![fixture::evidence(&plan, "candidate")]
                .into_iter()
                .map(|e| json!({"slot_id":e.slot_id,"execution":e.execution,"scoring":e.scoring}))
                .collect::<Vec<_>>(),
        );
        f.manifest(json!(["generation.json"]));
        f
    }
    fn save(&self, name: &str, value: &impl Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn manifest(&self, checkpoints: Value) {
        self.save("input.json", &json!({"schema":"genesis-method-cost-input/1", "scope":"scope.json", "generation_checkpoints":checkpoints, "trial":{"plan":"plan.json","evidence":"evidence.json"}}));
    }
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut result: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                let bytes = fs::read(&p).unwrap();
                (p, bytes)
            })
            .collect();
        result.sort();
        result
    }
    fn run(&self, extra: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        cmd.current_dir(std::env::temp_dir())
            .args(["method-cost-assess", "--manifest"])
            .arg(self.0.join("input.json"));
        for key in [
            "GENESIS_LLM_PROVIDER",
            "GENESIS_LLM_MODEL",
            "GENESIS_LLM_API_KEY",
            "ANTHROPIC_API_KEY",
            "APIFY_TOKEN",
        ] {
            cmd.env_remove(key);
        }
        if extra.is_empty() {
            cmd.arg("--admission").arg(self.0.join("admission.json"));
        } else {
            cmd.args(extra);
        }
        cmd.output().unwrap()
    }
    fn report(&self) -> Value {
        let result = self.run(&[]);
        assert_eq!(
            result.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).unwrap()
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn admitted_cli_binds_saved_history_and_preserves_inputs_with_unknown_and_overrun() {
    let f = Files::new(true);
    let before = f.snapshot();
    let report = f.report();
    assert_eq!(report["schema"], "genesis-admitted-method-cost-report/1");
    assert_eq!(report["admission"]["reserved_attempts"], 1);
    assert_eq!(report["costs"]["rows"].as_array().unwrap().len(), 5);
    assert_eq!(report["costs"]["combined"][0]["known_lower_bound"], 3);
    assert_eq!(report["costs"]["combined"][0]["unknown"], true);
    assert_eq!(
        report["generation"]["costs"]["combined"][1]["limit_exceeded"],
        true
    );
    assert_eq!(report["costs"]["combined"][1]["limit_exceeded"], false);
    assert_ne!(
        report["declared_scope_sha256"],
        report["costs"]["scope_sha256"]
    );
    assert!(!report.to_string().contains("print(1)"));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn optional_guard_refuses_unreserved_advanced_and_named_missing_evidence_without_writes() {
    let f = Files::new(false);
    let before = f.snapshot();
    let result = f.run(&[]);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(f.snapshot(), before);
    f.manifest(json!(["prepared.json"]));
    assert_eq!(f.report()["admission"]["reserved_attempts"], 0);
    f.manifest(json!([]));
    assert_eq!(
        f.report()["costs"]["rows"][0]["observation"]["kind"],
        "missing"
    );
    f.manifest(json!(["absent.json"]));
    let before = f.snapshot();
    let result = f.run(&[]);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(f.snapshot(), before);
}

#[test]
fn admission_flag_is_optional_for_legacy_exact_snapshot_assessment() {
    let f = Files::new(false);
    let generated = fixture::generation("one", true);
    f.save(
        "scope.json",
        &fixture::scope(
            std::slice::from_ref(&generated),
            Some(&fixture::trial(&generated)),
        ),
    );
    // A legacy cost scope pins results, not the original admission roster.
    let output = Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
        .args(["method-cost-assess", "--manifest"])
        .arg(f.0.join("input.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["schema"],
        "genesis-method-cost-report/1"
    );
    let before = f.snapshot();
    let guarded = f.run(&[]);
    assert_eq!(guarded.status.code(), Some(2));
    assert!(guarded.stdout.is_empty());
    assert_eq!(f.snapshot(), before);
}

#[test]
fn invalid_journal_and_argument_shapes_emit_no_success_report() {
    let f = Files::new(true);
    for args in [
        vec!["--admission"],
        vec!["--admission", "missing.json"],
        vec!["--admission", "one", "--admission", "two"],
    ] {
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    f.save("admission.json", &json!({"schema_version": 999}));
    let before = f.snapshot();
    let output = f.run(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(f.snapshot(), before);
}
