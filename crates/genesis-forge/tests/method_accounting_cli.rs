use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, path::PathBuf, process::Command};
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

struct Files(PathBuf);
// Parallel tests can observe the same clock tick, even through `as_nanos()`.
static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-cost-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn save(&self, name: &str, value: &impl Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn manifest(&self, checkpoints: Value, trial: Value) {
        self.save(
            "input.json",
            &json!({
                "schema":"genesis-method-cost-input/1", "scope":"scope.json",
                "generation_checkpoints":checkpoints,"trial":trial
            }),
        );
    }
    fn run(&self) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        // Deliberately use a different cwd: manifest-relative paths must still work.
        cmd.current_dir(std::env::temp_dir());
        for key in [
            "GENESIS_LLM_PROVIDER",
            "GENESIS_LLM_MODEL",
            "GENESIS_LLM_API_KEY",
            "ANTHROPIC_API_KEY",
            "APIFY_TOKEN",
        ] {
            cmd.env_remove(key);
        }
        cmd.args(["method-cost-assess", "--manifest"])
            .arg(self.0.join("input.json"))
            .output()
            .unwrap()
    }
    fn report(&self) -> Value {
        let result = self.run();
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
fn saved_evidence_reports_overrun_without_modifying_files_or_exposing_source() {
    let f = Files::new();
    let g = fixture::generation("used", true);
    let p = fixture::trial(&g);
    let mut scope = fixture::scope(std::slice::from_ref(&g), Some(&p));
    scope.total_grant.model_calls = 0;
    f.save("scope.json", &scope);
    f.save("generation.json", &g);
    f.save("plan.json", &p);
    f.save(
        "evidence.json",
        &["candidate", "control"]
            .into_iter()
            .map(|slot| {
                let e = fixture::evidence(&p, slot);
                json!({"slot_id":e.slot_id,"execution":e.execution,"scoring":e.scoring})
            })
            .collect::<Vec<_>>(),
    );
    f.manifest(
        json!(["generation.json"]),
        json!({"plan":"plan.json","evidence":"evidence.json"}),
    );
    let snapshot = || {
        let mut entries = fs::read_dir(&f.0)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.clone(), fs::read(p).unwrap())
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    };
    let before = snapshot();
    let report = f.report();
    assert_eq!(report["scope_sha256"], scope.sha256().unwrap());
    assert_eq!(report["combined"][0]["known_lower_bound"], 5);
    assert_eq!(report["combined"][0]["limit_exceeded"], true);
    assert_eq!(
        report["rows"][0]["observation"]["transport"],
        "loopback_fixture"
    );
    assert!(!report.to_string().contains("print(1)"));
    assert_eq!(snapshot(), before);
}

#[test]
fn omitted_evidence_is_unknown_but_missing_or_substituted_supplied_files_are_errors() {
    let f = Files::new();
    let g = fixture::generation("declared", true);
    let p = fixture::trial(&g);
    f.save(
        "scope.json",
        &fixture::scope(std::slice::from_ref(&g), Some(&p)),
    );
    f.manifest(json!([]), Value::Null);
    let report = f.report();
    assert_eq!(report["rows"].as_array().unwrap().len(), 5);
    assert_eq!(report["combined"][0]["unknown"], true);
    assert_eq!(report["combined"][0]["known_lower_bound"], 0);
    f.manifest(json!(["absent.json"]), Value::Null);
    let missing = f.run();
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    f.save("foreign.json", &fixture::generation("foreign", true));
    f.manifest(json!(["foreign.json"]), Value::Null);
    let foreign = f.run();
    assert_eq!(foreign.status.code(), Some(2));
    assert!(foreign.stdout.is_empty());
}

#[test]
fn pending_generation_is_observed_without_recovery_or_network() {
    let f = Files::new();
    let mut pending =
        MethodSynthesisSession::new(fixture::generation("pending", true).plan().clone()).unwrap();
    pending.begin().unwrap();
    f.save(
        "scope.json",
        &fixture::scope(std::slice::from_ref(&pending), None),
    );
    f.save("pending.json", &pending);
    f.manifest(json!(["pending.json"]), Value::Null);
    let report = f.report();
    assert_eq!(report["rows"][0]["observation"]["phase"], "requesting");
    assert_eq!(report["rows"][0]["observation"]["transport"], Value::Null);
    assert_eq!(report["combined"][0]["unknown"], true);
    let after: MethodSynthesisSession =
        serde_json::from_slice(&fs::read(f.0.join("pending.json")).unwrap()).unwrap();
    assert_eq!(after.phase(), MethodSynthesisPhase::Requesting);
    assert_eq!(after.checkpoint_sha256(), pending.checkpoint_sha256());
}
