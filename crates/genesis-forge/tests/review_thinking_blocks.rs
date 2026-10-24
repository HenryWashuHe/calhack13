//! Independent rejection checks: no filtering malformed blocks into executable source.
use genesis_core::development_method::ResourceUsage;
use genesis_forge::{
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
    text_generation::{TextFailure, TextOutcome, TextRejection},
    AnthropicGenerator,
};
use serde_json::{json, Value};
use std::time::Duration;
#[path = "common/text_generation_fixture.rs"]
mod http;
#[path = "common/method_plan.rs"]
mod plan;

fn text() -> Value {
    json!({"type":"text","text":"{\"source\":\"private-source\"}"})
}
fn thinking() -> Value {
    json!({"type":"thinking","thinking":"private-thought","signature":"private-signature"})
}
async fn rejected(body: Vec<u8>, metadata_known: bool) {
    let mut session = MethodSynthesisSession::new(plan::plan()).unwrap();
    let request = session.begin().unwrap();
    let (endpoint, server) = http::serve(body, 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    server.await.unwrap(); // Also refuses a second request to the loopback endpoint.
    assert_eq!(
        completion.outcome,
        TextOutcome::Failed {
            reason: TextFailure::Protocol
        }
    );
    assert!(completion.diagnostic.is_some());
    if metadata_known {
        assert_eq!(
            completion.usage.input_tokens,
            ResourceUsage::Measured { value: 100 }
        );
        assert_eq!(
            completion.usage.output_tokens,
            ResourceUsage::Measured { value: 32 }
        );
    } else {
        assert_eq!(
            completion.diagnostic.unwrap().rejection,
            TextRejection::InvalidJson
        );
        assert!(matches!(
            completion.usage.model_calls,
            ResourceUsage::Unknown { .. }
        ));
        assert!(matches!(
            completion.usage.output_tokens,
            ResourceUsage::Unknown { .. }
        ));
    }
    assert!(!serde_json::to_string(&completion)
        .unwrap()
        .contains("private-"));
    session.complete(completion).unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Failed);
    assert!(session.package().is_err());
    let before = serde_json::to_value(&session).unwrap();
    assert!(session.begin().is_err());
    assert!(!session.recover_interrupted().unwrap());
    assert_eq!(serde_json::to_value(session).unwrap(), before);
}

#[tokio::test]
async fn malformed_or_misordered_blocks_cannot_be_filtered_into_source() {
    for prefix in [
        json!({"type":"thinking","thinking":"private-thought"}),
        json!({"type":"thinking","signature":"private-signature"}),
        json!({"type":"thinking","thinking":null,"signature":"private-signature"}),
        json!({"type":"thinking","thinking":"","signature":null}),
        json!({"type":"thinking","thinking":"","signature":""}),
        json!({"type":"thinking","thinking":"","signature":"private-signature","text":"private-smuggled"}),
        json!({"type":"redacted_thinking"}),
        json!({"type":"redacted_thinking","data":null}),
        json!({"type":"redacted_thinking","data":""}),
        json!({"type":"redacted_thinking","data":"private-data","signature":"private-extra"}),
        json!({"type":"tool_use","id":"private-tool","name":"run","input":{}}),
        json!({"type":"unknown","text":"private-other"}),
    ] {
        let mut body = http::response("");
        body["content"] = json!([prefix, text()]);
        rejected(serde_json::to_vec(&body).unwrap(), true).await;
    }
    for content in [
        json!([text(), thinking()]),
        json!([text(), {"type":"redacted_thinking","data":"private-data"}]),
        json!([thinking(), text(), text()]),
        json!([thinking()]),
        json!([thinking(), {"type":"text","text":"private-source","signature":"private-extra"}]),
        json!([thinking(), text(), {"type":"tool_use","id":"private-tool","name":"run","input":{}}]),
    ] {
        let mut body = http::response("");
        body["content"] = content;
        rejected(serde_json::to_vec(&body).unwrap(), true).await;
    }
}

#[tokio::test]
async fn duplicate_thinking_keys_are_rejected_before_usage_is_trusted() {
    for block in [
        r#"{"type":"thinking","thinking":"","signature":"private-one","signature":"private-two"}"#,
        r#"{"type":"thinking","type":"redacted_thinking","thinking":"","signature":"private-one","data":"private-two"}"#,
    ] {
        let mut body = http::response("");
        body["content"] = json!(["replace-with-raw-block", text()]);
        let raw = serde_json::to_string(&body)
            .unwrap()
            .replace("\"replace-with-raw-block\"", block);
        rejected(raw.into_bytes(), false).await;
    }
}

#[test]
fn historical_failed_generation_is_not_reinterpreted_by_the_new_parser() {
    let saved = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/invention-lab/evidence/live-method-20261024/generation.json"
    ));
    let mut session: MethodSynthesisSession = serde_json::from_str(saved).unwrap();
    session.validate().unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Failed);
    assert!(session.package().is_err());
    assert!(session.begin().is_err());
    assert!(!session.recover_interrupted().unwrap());
    assert_eq!(
        serde_json::to_value(session).unwrap(),
        serde_json::from_str::<Value>(saved).unwrap()
    );
}
