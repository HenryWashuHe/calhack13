use genesis_forge::{
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
    AnthropicGenerator,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
#[path = "common/text_generation_fixture.rs"]
mod http_fixture;
#[path = "common/method_plan.rs"]
mod plan_fixture;
struct Fixture {
    directory: PathBuf,
}
// Parallel tests can observe the same clock tick, even through `as_nanos()`.
static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "genesis-method-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("plan.json"),
            serde_json::to_vec(&plan_fixture::plan()).unwrap(),
        )
        .unwrap();
        Self { directory }
    }
    fn command(&self, name: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        for key in [
            "GENESIS_LLM_PROVIDER",
            "GENESIS_LLM_MODEL",
            "GENESIS_LLM_API_KEY",
            "ANTHROPIC_API_KEY",
            "APIFY_TOKEN",
        ] {
            c.env_remove(key);
        }
        c.arg(name)
            .arg("--checkpoint")
            .arg(self.directory.join("generation.json"));
        if name == "method-generate-prepare" {
            c.arg("--plan").arg(self.directory.join("plan.json"));
        }
        c
    }
    fn run(&self, name: &str) -> Output {
        self.command(name).output().unwrap()
    }
    fn read(&self) -> MethodSynthesisSession {
        serde_json::from_slice(&fs::read(self.directory.join("generation.json")).unwrap()).unwrap()
    }
    fn save(&self, s: &MethodSynthesisSession) {
        fs::write(
            self.directory.join("generation.json"),
            serde_json::to_vec(s).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
#[test]
fn prepare_is_offline_exclusive_and_config_failure_leaves_it_prepared() {
    let f = Fixture::new();
    let prepared = f.run("method-generate-prepare");
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let original = fs::read(f.directory.join("generation.json")).unwrap();
    assert!(!f.run("method-generate-prepare").status.success());
    assert_eq!(
        fs::read(f.directory.join("generation.json")).unwrap(),
        original
    );
    let missing = f.run("method-generate-step");
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(f.read().phase(), MethodSynthesisPhase::Prepared);
    let mismatch = f
        .command("method-generate-step")
        .env("GENESIS_LLM_PROVIDER", "anthropic")
        .env("GENESIS_LLM_MODEL", "different-model")
        .env("GENESIS_LLM_API_KEY", "synthetic-key")
        .output()
        .unwrap();
    assert_eq!(mismatch.status.code(), Some(2));
    assert_eq!(f.read().phase(), MethodSynthesisPhase::Prepared);
    assert!(!String::from_utf8_lossy(&mismatch.stderr).contains("synthetic-key"));
}
#[test]
fn interrupted_request_recovers_before_credentials_and_cannot_restart() {
    let f = Fixture::new();
    let prepared = f.run("method-generate-prepare");
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let mut s = f.read();
    s.begin().unwrap();
    f.save(&s);
    let out = f
        .command("method-generate-step")
        .env("GENESIS_LLM_PROVIDER", "unsupported")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(f.read().phase(), MethodSynthesisPhase::NeedsReconciliation);
    assert_eq!(f.run("method-generate-step").status.code(), Some(1));
    assert!(!f.run("method-generate-export").status.success());
}
#[tokio::test]
async fn export_is_exact_package_bytes_and_status_never_prints_source() {
    let f = Fixture::new();
    let prepared = f.run("method-generate-prepare");
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let mut s = f.read();
    let request = s.begin().unwrap();
    let source = "raise Exception('PRIVATE_GENERATED_SOURCE_DO_NOT_EXECUTE')\r\n";
    let (endpoint, server) = http_fixture::serve(
        serde_json::to_vec(&http_fixture::response(
            &json!({"source":source}).to_string(),
        ))
        .unwrap(),
        200,
        Duration::ZERO,
    )
    .await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    s.complete(completion).unwrap();
    server.await.unwrap();
    f.save(&s);
    let out = f.run("method-generate-export");
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        s.package().unwrap().artifact_json().unwrap().as_bytes()
    );
    assert!(!out.stdout.ends_with(b"\n"));
    for name in ["method-generate-status", "method-generate-step"] {
        let out = f.run(name);
        assert!(out.status.success());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE_GENERATED_SOURCE"));
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(status["phase"], "generated");
    }
}
