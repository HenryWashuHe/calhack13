//! Independent PR82 CLI boundary review; no provider calls or source execution.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_accounting::{CostScope, GenerationExpectation},
    method_admission::GenerationAdmission,
    method_roster::GenerationRoster,
    method_synthesis::MethodSynthesisSession,
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
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        for directory in [&self.0, &self.0.join("evidence")] {
            for item in fs::read_dir(directory).unwrap() {
                let path = item.unwrap().path();
                if path.is_file() {
                    out.push((path.clone(), fs::read(path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }
    fn run(&self, admission: Option<&str>) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        command.current_dir(&self.0).args([
            "method-cost-assess",
            "--manifest",
            "evidence/input.json",
        ]);
        if let Some(path) = admission {
            command.args(["--admission", path]);
        }
        command
            .env("GENESIS_LLM_PROVIDER", "invalid-must-not-be-consulted")
            .output()
            .unwrap()
    }
}

#[test]
fn journal_paths_are_bounded_read_only_and_invalid_guard_never_falls_back() {
    let files = Files(std::env::temp_dir().join(format!(
            "genesis-review-admitted-cost-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir_all(files.0.join("evidence")).unwrap();
    let session = MethodSynthesisSession::new(fixture::plan()).unwrap();
    let grant = ResourceGrant {
        model_calls: 1,
        input_tokens: 10,
        output_tokens: u64::from(session.plan().settings.max_tokens),
        wall_time_ms: 100,
        simulation_steps: 0,
    };
    let roster = GenerationRoster::freeze(
        "review".into(),
        session.plan().settings.clone(),
        grant.clone(),
        std::slice::from_ref(&session),
    )
    .unwrap();
    let admission = GenerationAdmission::new(roster).unwrap();
    let scope = CostScope {
        id: "review".into(),
        generations: vec![GenerationExpectation {
            org_id: session.plan().org_id.clone(),
            operation_id: session.plan().operation_id.clone(),
            plan_sha256: session.plan().sha256().unwrap(),
            request_sha256: session.request_sha256().into(),
            checkpoint_sha256: session.checkpoint_sha256().into(),
        }],
        trial: None,
        generated_links: vec![],
        total_grant: grant,
    };
    fs::write(
        files.0.join("admission.json"),
        serde_json::to_vec(&admission).unwrap(),
    )
    .unwrap();
    // Admission is a CLI argument relative to CWD, unlike paths inside a manifest.
    fs::write(files.0.join("evidence/admission.json"), b"invalid-neighbor").unwrap();
    fs::write(
        files.0.join("evidence/generation.json"),
        serde_json::to_vec(&session).unwrap(),
    )
    .unwrap();
    fs::write(
        files.0.join("evidence/scope.json"),
        serde_json::to_vec(&scope).unwrap(),
    )
    .unwrap();
    fs::write(files.0.join("evidence/input.json"), serde_json::to_vec(&json!({"schema":"genesis-method-cost-input/1", "scope":"scope.json", "generation_checkpoints":["generation.json"], "trial":null})).unwrap()).unwrap();
    fs::write(files.0.join("admission.json.lock"), b"other writer").unwrap();
    fs::write(
        files.0.join("evidence/generation.json.lock"),
        b"other writer",
    )
    .unwrap();
    let before = files.snapshot();
    let legacy = files.run(None);
    assert!(legacy.status.success(), "{legacy:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&legacy.stdout).unwrap()["schema"],
        "genesis-method-cost-report/1"
    );
    let guarded = files.run(Some("admission.json"));
    assert!(guarded.status.success(), "{guarded:?}");
    let report: Value = serde_json::from_slice(&guarded.stdout).unwrap();
    assert_eq!(report["schema"], "genesis-admitted-method-cost-report/1");
    assert_eq!(
        report["costs"]["rows"][0]["evidence_sha256"],
        session.checkpoint_sha256()
    );
    assert_eq!(report["costs"]["combined"][0]["unknown"], true);
    assert_eq!(files.snapshot(), before);

    for contents in [b"{".to_vec(), vec![b' '; 8 * 1024 * 1024 + 1]] {
        fs::write(files.0.join("bad.json"), &contents).unwrap();
        let before = files.snapshot();
        let output = files.run(Some("bad.json"));
        assert_eq!(output.status.code(), Some(2));
        assert!(
            output.stdout.is_empty(),
            "must not emit the valid legacy report"
        );
        if contents.len() > 8 * 1024 * 1024 {
            assert!(String::from_utf8_lossy(&output.stderr).contains("byte limit"));
        }
        assert_eq!(files.snapshot(), before);
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(files.0.join("admission.json"), files.0.join("linked.json"))
            .unwrap();
        let before = files.snapshot();
        let output = files.run(Some("linked.json"));
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("regular file"));
        assert_eq!(files.snapshot(), before);
    }
}
