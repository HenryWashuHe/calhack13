//! Henry's Claude: independent CLI regressions for guards a mutation run found
//! untested (the current code is correct on each).
use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use std::{fs, path::PathBuf, process::Command};
#[path = "common/method_plan.rs"]
mod plan_fixture;

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "genesis-method-cli-review-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("plan.json"),
            serde_json::to_vec(&plan_fixture::plan()).unwrap(),
        )
        .unwrap();
        Self { directory }
    }
    fn checkpoint(&self) -> PathBuf {
        self.directory.join("generation.json")
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
        c.arg(name).arg("--checkpoint").arg(self.checkpoint());
        if name == "method-generate-prepare" {
            c.arg("--plan").arg(self.directory.join("plan.json"));
        }
        c
    }
    fn prepare(&self) {
        let out = self.command("method-generate-prepare").output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn read(&self) -> MethodSynthesisSession {
        serde_json::from_slice(&fs::read(self.checkpoint()).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn status_never_settles_a_pending_request() {
    let f = Fixture::new("status");
    f.prepare();
    let mut s = f.read();
    s.begin().unwrap();
    fs::write(f.checkpoint(), serde_json::to_vec(&s).unwrap()).unwrap();
    let before = fs::read(f.checkpoint()).unwrap();
    // Status is read-only: a pending intent stays pending (only step may
    // record it as interrupted), so status can't change what step does next.
    let out = f.command("method-generate-status").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read(f.checkpoint()).unwrap(), before);
    assert_eq!(f.read().phase(), MethodSynthesisPhase::Requesting);
}

#[test]
fn a_complete_configuration_for_another_provider_is_refused_before_any_intent() {
    let f = Fixture::new("provider");
    f.prepare();
    let before = fs::read(f.checkpoint()).unwrap();
    let out = f
        .command("method-generate-step")
        .env("GENESIS_LLM_PROVIDER", "openai")
        .env("GENESIS_LLM_MODEL", &plan_fixture::plan().settings.model)
        .env("GENESIS_LLM_API_KEY", "synthetic-other-provider-key")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(f.checkpoint()).unwrap(), before);
    assert_eq!(f.read().phase(), MethodSynthesisPhase::Prepared);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("GENESIS_LLM_PROVIDER=anthropic"),
        "{stderr}"
    );
    assert!(!stderr.contains("synthetic-other-provider-key"));
}
