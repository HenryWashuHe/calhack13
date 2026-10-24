//! Independent diagnostics review: retained #89 records and closed output only.
use genesis_forge::{
    method_admission::GenerationAdmission, method_roster::GenerationRoster,
    method_synthesis::MethodSynthesisSession, text_generation::TextCompletion, AnthropicGenerator,
};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Output},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[path = "common/text_generation_fixture.rs"]
mod http;
macro_rules! evidence {
    ($file:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/invention-lab/evidence/live-method-20261024/",
            $file
        ))
    };
}
const PREPARED: &str = evidence!("prepared-generation.json");
const FAILED: &str = evidence!("generation.json");
fn cli(saved: &str, command: &str) -> (Output, String) {
    let directory = std::env::temp_dir().join(format!(
        "genesis-review-diagnostics-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("generation.json");
    fs::write(&path, saved).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
        .arg(command)
        .arg("--checkpoint")
        .arg(&path)
        .env("GENESIS_LLM_PROVIDER", "invalid-must-not-be-read")
        .env_remove("GENESIS_LLM_MODEL")
        .env_remove("GENESIS_LLM_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .unwrap();
    let after = fs::read_to_string(path).unwrap();
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
    (output, after)
}
#[test]
fn original_bundle_keeps_identities_accounting_and_terminal_no_replay() {
    for saved in [PREPARED, FAILED] {
        let session: MethodSynthesisSession = serde_json::from_str(saved).unwrap();
        session.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&session).unwrap(),
            serde_json::from_str::<Value>(saved).unwrap()
        );
    }
    let failed: MethodSynthesisSession = serde_json::from_str(FAILED).unwrap();
    let admission: GenerationAdmission = serde_json::from_str(evidence!("admission.json")).unwrap();
    let roster: GenerationRoster = serde_json::from_str(evidence!("roster.json")).unwrap();
    assert_eq!(
        serde_json::to_value(&admission).unwrap(),
        serde_json::from_str::<Value>(evidence!("admission.json")).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&roster).unwrap(),
        serde_json::from_str::<Value>(evidence!("roster.json")).unwrap()
    );
    assert!(admission.is_reserved(&failed).unwrap());
    assert_eq!(admission.summary().unwrap().remaining_attempts, 0);
    let report = roster.assess(&[failed]).unwrap();
    assert_eq!(report.costs.combined[0].known_lower_bound, 1);
    assert_eq!(report.costs.combined[1].known_lower_bound, 3971);
    assert_eq!(report.costs.combined[2].known_lower_bound, 8192);
    let (output, after) = cli(FAILED, "method-generate-step");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(after, FAILED);
    assert_eq!(output.stdout, evidence!("final-status.json").as_bytes());
}
#[test]
fn diagnostic_is_digest_bound_and_cannot_turn_into_another_outcome() {
    let mut pending: MethodSynthesisSession = serde_json::from_str(PREPARED).unwrap();
    pending.begin().unwrap();
    let mut completion = serde_json::from_str::<Value>(FAILED).unwrap()["completion"].clone();
    completion["diagnostic"] = json!({"rejection":"stop_reason","stop_reason":"max_tokens"});
    let mut failed = pending.clone();
    failed
        .complete(serde_json::from_value::<TextCompletion>(completion.clone()).unwrap())
        .unwrap();
    let saved = serde_json::to_value(&failed).unwrap();
    let restored: MethodSynthesisSession = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(restored.checkpoint_sha256(), failed.checkpoint_sha256());
    let mut tampered = saved;
    tampered["completion"]["diagnostic"]["stop_reason"] = json!("end_turn");
    assert!(serde_json::from_value::<MethodSynthesisSession>(tampered).is_err());
    for outcome in [
        json!({"kind":"unknown","reason":"interrupted"}),
        json!({"kind":"failed","reason":"http"}),
        json!({"kind":"returned","text":"{}"}),
    ] {
        completion["outcome"] = outcome;
        assert!(pending
            .clone()
            .complete(serde_json::from_value(completion.clone()).unwrap())
            .is_err());
    }
}
#[tokio::test]
async fn unknown_provider_strings_are_not_retained_in_checkpoint_or_cli_status() {
    let mut pending: MethodSynthesisSession = serde_json::from_str(PREPARED).unwrap();
    let request = pending.begin().unwrap();
    let mut response = http::response("private-provider-body");
    response["model"] = json!(request.settings.model);
    response["stop_reason"] = json!("private-unknown-stop");
    response["stop_sequence"] = json!("private-stop-sequence");
    let (endpoint, server) =
        http::serve(serde_json::to_vec(&response).unwrap(), 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    server.await.unwrap();
    pending.complete(completion).unwrap();
    let saved = serde_json::to_string(&pending).unwrap();
    let restored: MethodSynthesisSession = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.checkpoint_sha256(), pending.checkpoint_sha256());
    let (output, after) = cli(&saved, "method-generate-status");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(after, saved);
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        status["diagnostic"],
        json!({"rejection":"stop_reason","stop_reason":"other"})
    );
    for secret in [
        "private-provider-body",
        "private-unknown-stop",
        "private-stop-sequence",
    ] {
        assert!(!saved.contains(secret));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    }
}
