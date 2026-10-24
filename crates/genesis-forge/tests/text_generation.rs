use genesis_core::development_method::ResourceUsage;
use genesis_forge::{
    text_generation::{
        GenerationTransport, Sampling, TextGenerationRequest, TextModelSettings, TextOutcome,
        TextRejection, TextStopReason, MAX_TEXT_BYTES,
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
            reasoning: None,
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
    assert!(c.diagnostic.is_none());
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
        assert_eq!(c.diagnostic.unwrap().rejection, TextRejection::InvalidJson);
        assert_eq!(c.diagnostic.unwrap().stop_reason, None);
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
    assert!(c.diagnostic.is_none());
}

#[tokio::test]
async fn diagnostics_name_first_rejection_without_changing_acceptance_or_usage() {
    for (pointer, value, rejection) in [
        (
            "/model",
            json!("foreign-model"),
            TextRejection::ModelMismatch,
        ),
        (
            "/stop_reason",
            json!("max_tokens"),
            TextRejection::StopReason,
        ),
        (
            "/stop_sequence",
            json!("private-stop"),
            TextRejection::StopSequence,
        ),
        (
            "/usage/output_tokens",
            json!(513),
            TextRejection::OutputLimit,
        ),
        ("/content", json!({}), TextRejection::ContentShape),
        ("/content", json!([]), TextRejection::ContentCount),
        (
            "/content/0/type",
            json!("unknown"),
            TextRejection::ContentType,
        ),
        (
            "/content/0/text",
            json!("x".repeat(MAX_TEXT_BYTES + 1)),
            TextRejection::TextLimit,
        ),
        (
            "/content/0/citations",
            json!([{"private":"citation"}]),
            TextRejection::Citations,
        ),
    ] {
        let mut body = response("rejected-content");
        if pointer.ends_with("citations") {
            body["content"][0]["citations"] = value;
        } else {
            *body.pointer_mut(pointer).unwrap() = value;
        }
        let c = call(serde_json::to_vec(&body).unwrap()).await;
        assert!(matches!(c.outcome, TextOutcome::Failed { .. }));
        assert_eq!(c.diagnostic.unwrap().rejection, rejection);
        assert_eq!(c.usage.input_tokens, ResourceUsage::Measured { value: 100 });
        let stored = serde_json::to_string(&c).unwrap();
        assert!(!stored.contains("rejected-content"));
        assert!(!stored.contains("private-stop"));
        assert!(!stored.contains("\"private\""));
    }
    let mut body = response("text");
    body["model"] = json!("foreign-model");
    body["stop_reason"] = json!("max_tokens");
    body["content"] = json!([]);
    let c = call(serde_json::to_vec(&body).unwrap()).await;
    assert_eq!(
        c.diagnostic.unwrap().rejection,
        TextRejection::ModelMismatch
    );
    assert_eq!(
        c.diagnostic.unwrap().stop_reason,
        Some(TextStopReason::MaxTokens)
    );
}

#[tokio::test]
async fn output_token_count_does_not_infer_the_stop_reason() {
    let mut body = response("complete");
    body["usage"]["output_tokens"] = json!(512);
    let c = call(serde_json::to_vec(&body).unwrap()).await;
    assert!(matches!(c.outcome, TextOutcome::Returned { .. }));
    assert!(c.diagnostic.is_none());
    body["stop_reason"] = json!("max_tokens");
    body["usage"]["output_tokens"] = json!(32);
    let c = call(serde_json::to_vec(&body).unwrap()).await;
    assert!(matches!(c.outcome, TextOutcome::Failed { .. }));
    assert_eq!(
        c.diagnostic.unwrap().stop_reason,
        Some(TextStopReason::MaxTokens)
    );
}

#[tokio::test]
async fn malformed_envelopes_and_http_errors_do_not_invent_stop_or_usage() {
    for (field, value) in [("role", json!("user")), ("stop_reason", json!(42))] {
        let mut body = response("private-text");
        body[field] = value;
        let c = call(serde_json::to_vec(&body).unwrap()).await;
        assert_eq!(
            c.diagnostic.unwrap().rejection,
            TextRejection::InvalidEnvelope
        );
        assert_eq!(c.diagnostic.unwrap().stop_reason, None);
        assert!(matches!(
            c.usage.output_tokens,
            ResourceUsage::Unknown { .. }
        ));
    }
    let (endpoint, task) = serve(b"private-error".to_vec(), 401, Duration::ZERO).await;
    let c = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request())
        .await
        .unwrap();
    task.await.unwrap();
    assert!(c.diagnostic.is_none());
    assert!(!serde_json::to_string(&c).unwrap().contains("private-error"));
}
