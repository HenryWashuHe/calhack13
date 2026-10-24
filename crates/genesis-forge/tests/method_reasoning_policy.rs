use genesis_core::development_method::ResourceGrant;
use genesis_forge::method_roster::GenerationRoster;
use genesis_forge::method_synthesis::MethodSynthesisSession;
use genesis_forge::text_generation::{
    ReasoningEffort, ReasoningPolicy, TextGenerationRequest, TextModelSettings, TextOutcome,
};
use genesis_forge::AnthropicGenerator;
use serde_json::{json, Value};
use std::time::Duration;
#[path = "common/text_generation_fixture.rs"]
mod fixture;
#[path = "common/method_plan.rs"]
mod plan_fixture;

fn request(effort: ReasoningEffort) -> TextGenerationRequest {
    let mut plan = plan_fixture::plan();
    plan.settings.model = "claude-sonnet-5-5".into();
    plan.settings.reasoning = Some(ReasoningPolicy::Adaptive { effort });
    plan.request().unwrap()
}

#[test]
fn explicit_effort_changes_submitted_bytes_and_request_identity() {
    let mut identities = std::collections::BTreeSet::new();
    for (effort, label) in [
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::Xhigh, "xhigh"),
        (ReasoningEffort::Max, "max"),
    ] {
        let request = request(effort);
        let wire: Value = serde_json::from_slice(&request.wire_bytes().unwrap()).unwrap();
        assert_eq!(wire["thinking"], json!({"type":"adaptive"}));
        assert_eq!(wire["output_config"], json!({"effort":label}));
        for unsupported in ["tools", "temperature", "top_p", "top_k"] {
            assert!(wire.get(unsupported).is_none());
        }
        assert!(identities.insert(request.sha256().unwrap()));
        let mut legacy = request.clone();
        legacy.settings.reasoning = None;
        assert_ne!(legacy.sha256().unwrap(), request.sha256().unwrap());
    }
}

#[test]
fn policy_is_closed_and_unsupported_models_cannot_silently_drop_it() {
    let settings = request(ReasoningEffort::High).settings;
    for invalid in [
        json!({"mode":"disabled","effort":"high"}),
        json!({"mode":"adaptive"}),
        json!({"mode":"adaptive","effort":"ultra"}),
        json!({"mode":"adaptive","effort":1}),
        json!({"mode":"adaptive","effort":"high","temperature":0}),
        json!({"mode":"adaptive","effort":"high","budget_tokens":100}),
    ] {
        let mut value = serde_json::to_value(&settings).unwrap();
        value["reasoning"] = invalid;
        assert!(serde_json::from_value::<TextModelSettings>(value).is_err());
    }
    let mut changed = request(ReasoningEffort::High);
    changed.settings.model = "claude-sonnet-4-6".into();
    assert!(changed.validate().is_err());
    assert!(changed.wire_bytes().is_err());
    assert!(changed.sha256().is_err());
}

#[test]
fn explicit_policy_is_frozen_by_checkpoint_and_roster() {
    let mut plan = plan_fixture::plan();
    plan.settings.model = "claude-sonnet-5-5".into();
    plan.settings.reasoning = Some(ReasoningPolicy::Adaptive {
        effort: ReasoningEffort::High,
    });
    let session = MethodSynthesisSession::new(plan.clone()).unwrap();
    let settings = plan.settings.clone();
    let grant = ResourceGrant {
        model_calls: 2,
        input_tokens: 2000,
        output_tokens: 1024,
        wall_time_ms: 2000,
        simulation_steps: 0,
    };
    let roster = GenerationRoster::freeze(
        "reasoning".into(),
        settings.clone(),
        grant.clone(),
        std::slice::from_ref(&session),
    )
    .unwrap();
    let restored: MethodSynthesisSession =
        serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.plan().settings, settings);
    let mut corrupt = serde_json::to_value(&session).unwrap();
    corrupt["plan"]["settings"]["reasoning"]["effort"] = json!("low");
    assert!(serde_json::from_value::<MethodSynthesisSession>(corrupt).is_err());
    plan.operation_id = "different_effort".into();
    plan.settings.reasoning = Some(ReasoningPolicy::Adaptive {
        effort: ReasoningEffort::Low,
    });
    let low = MethodSynthesisSession::new(plan).unwrap();
    assert!(
        GenerationRoster::freeze("mixed".into(), settings, grant, &[session, low.clone()]).is_err()
    );
    assert!(roster.assess(&[low]).is_err());
}

#[test]
fn all_four_saved_attempts_keep_their_checkpoint_and_request_identities() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/invention-lab/evidence");
    for dir in [
        "live-method-20261024",
        "live-method-20261024-diagnostic",
        "live-method-20261024-generated",
        "live-method-20261024-b-revision",
    ] {
        for file in ["prepared-generation.json", "generation.json"] {
            let bytes = std::fs::read(root.join(dir).join(file)).unwrap();
            let original: Value = serde_json::from_slice(&bytes).unwrap();
            let session: MethodSynthesisSession = serde_json::from_slice(&bytes).unwrap();
            session.validate().unwrap();
            assert_eq!(
                serde_json::to_value(&session).unwrap(),
                original,
                "{dir}/{file}"
            );
            assert!(session.plan().settings.reasoning.is_none());
        }
    }
}

#[tokio::test]
async fn loopback_provider_receives_the_frozen_explicit_policy() {
    let request = request(ReasoningEffort::High);
    let mut response = fixture::response("one bounded response");
    response["model"] = json!("claude-sonnet-5-5");
    let (endpoint, server) =
        fixture::serve(serde_json::to_vec(&response).unwrap(), 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    let sent = server.await.unwrap();
    let split = sent.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&sent[split..], request.wire_bytes().unwrap());
    assert_eq!(completion.request_sha256, request.sha256().unwrap());
    assert!(matches!(completion.outcome, TextOutcome::Returned { .. }));
}
