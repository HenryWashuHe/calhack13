//! Independent offline review of explicit reasoning identity. No model/source runs.
use genesis_forge::method_admission::GenerationAdmission;
use genesis_forge::method_roster::GenerationRoster;
use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use genesis_forge::text_generation::{ReasoningEffort, ReasoningPolicy, TextCompletion};
use serde_json::{json, Value};
use std::{fs, process::Command};

#[path = "common/method_plan.rs"]
mod plan_fixture;

fn session(effort: ReasoningEffort) -> MethodSynthesisSession {
    let mut plan = plan_fixture::plan();
    plan.settings.model = "claude-sonnet-5-5".into();
    plan.settings.reasoning = Some(ReasoningPolicy::Adaptive { effort });
    MethodSynthesisSession::new(plan).unwrap()
}

#[test]
fn foreign_effort_completion_cannot_settle_the_pending_request() {
    let mut high = session(ReasoningEffort::High);
    let high_request = high.begin().unwrap();
    let mut low = session(ReasoningEffort::Low);
    let low_request = low.begin().unwrap();
    let high_completion = TextCompletion::interrupted(&high_request).unwrap();
    let low_completion = TextCompletion::interrupted(&low_request).unwrap();
    let pending = serde_json::to_vec(&high).unwrap();

    // Either hash alone must reject substitution, not merely the settings copy.
    let mut wire_only = low_completion.clone();
    wire_only.request_sha256 = high_completion.request_sha256.clone();
    let mut request_only = low_completion.clone();
    request_only.wire_sha256 = high_completion.wire_sha256.clone();
    for foreign in [low_completion, wire_only, request_only] {
        assert!(high.complete(foreign).is_err());
        assert_eq!(serde_json::to_vec(&high).unwrap(), pending);
        assert_eq!(high.phase(), MethodSynthesisPhase::Requesting);
    }
    high.complete(high_completion).unwrap();
    assert_eq!(high.phase(), MethodSynthesisPhase::NeedsReconciliation);
    assert_eq!(
        high.plan().settings.reasoning,
        high_request.settings.reasoning
    );
    assert!(
        high.begin().is_err(),
        "an unknown outcome is never replayable"
    );
}

#[test]
fn retained_rosters_and_sealed_admissions_keep_historical_identity() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/invention-lab/evidence");
    for dir in [
        "live-method-20261024",
        "live-method-20261024-diagnostic",
        "live-method-20261024-generated",
        "live-method-20261024-b-revision",
    ] {
        let directory = root.join(dir);
        let bytes = fs::read(directory.join("roster.json")).unwrap();
        let original: Value = serde_json::from_slice(&bytes).unwrap();
        let roster: GenerationRoster = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_value(&roster).unwrap(), original, "{dir}");
        assert!(roster.settings().reasoning.is_none());
        let final_session: MethodSynthesisSession =
            serde_json::from_slice(&fs::read(directory.join("generation.json")).unwrap()).unwrap();
        assert_eq!(
            roster
                .assess(std::slice::from_ref(&final_session))
                .unwrap()
                .costs
                .rows
                .len(),
            1
        );

        let journal = fs::read(directory.join("admission.json")).unwrap();
        let admission: GenerationAdmission = serde_json::from_slice(&journal).unwrap();
        assert_eq!(
            serde_json::to_value(admission).unwrap(),
            serde_json::from_slice::<Value>(&journal).unwrap(),
            "{dir}"
        );

        // Retrofitting an explicit policy cannot upgrade the old declared attempt.
        let mut changed = original;
        changed["settings"]["reasoning"] = json!({"mode":"adaptive","effort":"high"});
        let explicit: GenerationRoster = serde_json::from_value(changed).unwrap();
        assert_ne!(explicit.sha256().unwrap(), roster.sha256().unwrap());
        assert!(explicit.assess(&[final_session]).is_err());
    }
}

#[test]
fn invalid_policy_is_refused_before_cli_creates_an_intent() {
    let directory = std::env::temp_dir().join(format!(
        "genesis-review-reasoning-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let checkpoint = directory.join("generation.json");
    let plan_path = directory.join("plan.json");
    let original = serde_json::to_value(session(ReasoningEffort::High).plan()).unwrap();
    for mutation in ["model", "effort", "extra"] {
        let mut plan = original.clone();
        match mutation {
            "model" => plan["settings"]["model"] = json!("claude-sonnet-4-6"),
            "effort" => plan["settings"]["reasoning"]["effort"] = json!("ultra"),
            _ => plan["settings"]["reasoning"]["budget_tokens"] = json!(256),
        }
        let bytes = serde_json::to_vec(&plan).unwrap();
        fs::write(&plan_path, &bytes).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
            .args(["method-generate-prepare", "--plan"])
            .arg(&plan_path)
            .arg("--checkpoint")
            .arg(&checkpoint)
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GENESIS_LLM_API_KEY")
            .env_remove("GENESIS_LLM_PROVIDER")
            .env_remove("GENESIS_LLM_MODEL")
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{mutation}");
        assert!(!checkpoint.exists(), "{mutation} left an intent");
        assert_eq!(fs::read(&plan_path).unwrap(), bytes);
    }
    fs::write(&plan_path, serde_json::to_vec(&original).unwrap()).unwrap();
    let valid = Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
        .args(["method-generate-prepare", "--plan"])
        .arg(&plan_path)
        .arg("--checkpoint")
        .arg(&checkpoint)
        .output()
        .unwrap();
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    let prepared: MethodSynthesisSession =
        serde_json::from_slice(&fs::read(checkpoint).unwrap()).unwrap();
    assert_eq!(prepared.phase(), MethodSynthesisPhase::Prepared);
    fs::remove_dir_all(directory).unwrap();
}
