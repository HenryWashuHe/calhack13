use genesis_core::development_method::ResourceUsage;
use genesis_forge::{
    text_generation::{
        GenerationTransport, Sampling, TextGenerationRequest, TextModelSettings, TextOutcome,
    },
    AnthropicGenerator,
};
use serde_json::{json, Value};
use std::time::Duration;
#[path = "common/text_generation_fixture.rs"]
mod fixture;
use fixture::{response, serve};
fn request() -> TextGenerationRequest {
    TextGenerationRequest {
        system: "Fixed fixture protocol".into(),
        user: "Untrusted public task".into(),
        settings: TextModelSettings {
            model: "fixture-model-1".into(),
            max_tokens: 512,
            timeout_ms: 1000,
            sampling: Sampling::ProviderDefault,
        },
    }
}
async fn call(body: Vec<u8>) -> genesis_forge::text_generation::TextCompletion {
    let (endpoint, task) = serve(body, 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request())
        .await
        .unwrap();
    task.await.unwrap();
    completion
}
#[tokio::test]
async fn exact_wire_settings_and_text_bytes_are_retained() {
    let text = "// café\r\nsource bytes\r\n";
    let r = request();
    let (endpoint, task) = serve(
        serde_json::to_vec(&response(text)).unwrap(),
        200,
        Duration::ZERO,
    )
    .await;
    let c = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&r)
        .await
        .unwrap();
    let wire = task.await.unwrap();
    let boundary = wire.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    assert_eq!(&wire[boundary + 4..], r.wire_bytes().unwrap());
    assert_eq!(c.request_sha256, r.sha256().unwrap());
    assert_eq!(c.transport, GenerationTransport::LoopbackFixture);
    assert_eq!(c.outcome, TextOutcome::Returned { text: text.into() });
    let sent: Value = serde_json::from_slice(&wire[boundary + 4..]).unwrap();
    assert_eq!(sent["model"], "fixture-model-1");
    assert_eq!(sent["max_tokens"], 512);
    assert_eq!(sent["stream"], false);
    for key in ["tools", "temperature", "thinking", "top_p", "top_k"] {
        assert!(sent.get(key).is_none());
    }
    assert!(String::from_utf8_lossy(&wire[..boundary]).contains("anthropic-version: 2023-06-01"));
}
#[tokio::test]
async fn missing_usage_is_unknown_not_zero() {
    let mut body = response("text");
    body.as_object_mut().unwrap().remove("usage");
    let c = call(serde_json::to_vec(&body).unwrap()).await;
    assert!(matches!(c.outcome, TextOutcome::Returned { .. }));
    assert!(matches!(
        c.usage.input_tokens,
        ResourceUsage::Unknown { .. }
    ));
    assert!(matches!(
        c.usage.output_tokens,
        ResourceUsage::Unknown { .. }
    ));
}
#[tokio::test]
async fn truncation_tool_and_foreign_model_fail_but_retain_reported_usage() {
    for (field, value) in [
        ("model", json!("wrong-model")),
        ("stop_reason", json!("max_tokens")),
        ("stop_reason", json!("tool_use")),
        (
            "content",
            json!([{"type":"tool_use","name":"run","input":{}}]),
        ),
    ] {
        let mut body = response("text");
        body[field] = value;
        let c = call(serde_json::to_vec(&body).unwrap()).await;
        assert!(matches!(c.outcome, TextOutcome::Failed { .. }));
        assert_eq!(c.usage.output_tokens, ResourceUsage::Measured { value: 32 });
    }
}
#[tokio::test]
async fn duplicate_content_and_malformed_json_are_not_normalized_into_valid_output() {
    for body in [br#"{"id":"msg_test","type":"message","role":"assistant","model":"fixture-model-1","content":[{"type":"text","text":"bad","text":"good"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":2}}"#.to_vec(),b"{malformed".to_vec()] {
        let c = call(body).await;
        assert!(matches!(c.outcome, TextOutcome::Failed { .. }));
        assert!(matches!(c.usage.output_tokens, ResourceUsage::Unknown { .. }));
    }
}
#[tokio::test]
async fn timeout_stays_unknown_and_never_retries() {
    let mut r = request();
    r.settings.timeout_ms = 10;
    let (endpoint, task) = serve(
        serde_json::to_vec(&response("text")).unwrap(),
        200,
        Duration::from_millis(50),
    )
    .await;
    let c = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&r)
        .await
        .unwrap();
    task.await.unwrap();
    assert!(matches!(c.outcome, TextOutcome::Unknown { .. }));
    assert!(matches!(c.usage.model_calls, ResourceUsage::Unknown { .. }));
}
