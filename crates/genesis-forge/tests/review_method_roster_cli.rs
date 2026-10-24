//! Independent boundary checks of CLI 2c80890; no providers or method execution.
use genesis_forge::method_synthesis::MethodSynthesisSession;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};
#[path = "common/method_revision_fixture.rs"]
mod fixture;

struct Files(PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Files {
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn run(&self, command: &str, manifest: &str, output: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .args([command, "--manifest"])
            .arg(self.0.join(manifest))
            .arg("--roster")
            .arg(self.0.join(output))
            .env(
                "GENESIS_LLM_PROVIDER",
                "invalid-provider-must-not-be-consulted",
            )
            .output()
            .unwrap()
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

#[test]
fn invalid_settings_aliasing_output_and_substituted_results_are_read_only_errors() {
    let files = Files(std::env::temp_dir().join(format!(
        "genesis-review-roster-cli-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    fs::create_dir(&files.0).unwrap();
    let (spec, parent) = fixture::fixture();
    let initial = spec.prepare(&parent).unwrap();
    files.write("generation.json", &initial);
    let declaration = json!({
        "schema":"genesis-generation-roster-input/1", "id":"review",
        "settings":initial.plan().settings, "prepared_checkpoints":["generation.json"],
        "total_grant":{"model_calls":1,"input_tokens":100,"output_tokens":16384,"wall_time_ms":1000,"simulation_steps":0}
    });
    let mut wrong_settings = declaration.clone();
    wrong_settings["settings"]["model"] = json!("different-model");
    files.write("declaration.json", &wrong_settings);
    let before = files.snapshot();
    let output = files.run("method-roster-prepare", "declaration.json", "roster.json");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(files.snapshot(), before);

    files.write("declaration.json", &declaration);
    let before = files.snapshot();
    let output = files.run(
        "method-roster-prepare",
        "declaration.json",
        "generation.json",
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(files.snapshot(), before);
    assert!(files
        .run("method-roster-prepare", "declaration.json", "roster.json")
        .status
        .success());

    let mut different_plan = initial.plan().clone();
    different_plan
        .instruction
        .push_str(" A different operation under the same IDs.");
    let mut substituted = MethodSynthesisSession::new(different_plan).unwrap();
    fixture::complete(&mut substituted, fixture::SOURCE);
    files.write("generation.json", &substituted);
    files.write("snapshots.json", &json!({"schema":"genesis-generation-roster-snapshots/1", "generation_checkpoints":["generation.json"]}));
    let before = files.snapshot();
    let output = files.run("method-roster-assess", "snapshots.json", "roster.json");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(files.snapshot(), before);

    let mut exact = initial;
    fixture::complete(&mut exact, fixture::SOURCE);
    files.write("generation.json", &exact);
    let before = files.snapshot();
    let output = files.run("method-roster-assess", "snapshots.json", "roster.json");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["costs"]["rows"][0]["evidence_sha256"],
        exact.checkpoint_sha256()
    );
    assert_eq!(files.snapshot(), before);
}
