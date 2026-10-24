use genesis_core::development_method::ResourceUsage;
use genesis_forge::{
    text_generation::{
        Sampling, TextGenerationRequest, TextModelSettings, TextOutcome, TextRejection,
        MAX_CONTENT_BLOCKS, MAX_TEXT_BYTES,
    },
    AnthropicGenerator,
};
use serde_json::{json, Value};
use std::time::Duration;
#[path = "common/text_generation_fixture.rs"]
mod fixture;
async fn call(content: Value) -> genesis_forge::text_generation::TextCompletion {
    let mut body = fixture::response("unused");
    body["content"] = content;
    let (endpoint, server) =
        fixture::serve(serde_json::to_vec(&body).unwrap(), 200, Duration::ZERO).await;
    let request = TextGenerationRequest {
        system: "Public fixture protocol".into(),
        user: "Develop source".into(),
        settings: TextModelSettings {
            model: "fixture-model-1".into(),
            max_tokens: 512,
            timeout_ms: 1000,
            sampling: Sampling::ProviderDefault,
            reasoning: None,
        },
    };
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    server.await.unwrap();
    completion
}
#[tokio::test]
async fn documented_thinking_prefixes_return_only_exact_final_text_and_full_usage() {
    let text = "{\"source\":\"# café\\r\\n\"}";
    for thinking in ["", "private-reasoning-sentinel"] {
        let c = call(json!([
            {"type":"thinking","thinking":thinking,"signature":"opaque-signature-sentinel"},
            {"type":"redacted_thinking","data":"encrypted-reasoning-sentinel"},
            {"type":"text","text":text}
        ]))
        .await;
        assert_eq!(c.outcome, TextOutcome::Returned { text: text.into() });
        assert!(c.diagnostic.is_none());
        assert_eq!(c.usage.model_calls, ResourceUsage::Measured { value: 1 });
        assert_eq!(c.usage.input_tokens, ResourceUsage::Measured { value: 100 });
        assert_eq!(c.usage.output_tokens, ResourceUsage::Measured { value: 32 });
        let saved = serde_json::to_string(&c).unwrap();
        for sentinel in [
            "private-reasoning-sentinel",
            "opaque-signature-sentinel",
            "encrypted-reasoning-sentinel",
        ] {
            assert!(!saved.contains(sentinel));
        }
    }
}
#[tokio::test]
async fn block_count_and_each_opaque_payload_are_bounded() {
    let thinking = json!({"type":"thinking","thinking":"","signature":"opaque"});
    let mut blocks = vec![thinking.clone(); MAX_CONTENT_BLOCKS - 1];
    blocks.push(json!({"type":"text","text":"source"}));
    assert!(matches!(
        call(json!(blocks)).await.outcome,
        TextOutcome::Returned { .. }
    ));
    blocks.insert(0, thinking);
    let c = call(json!(blocks)).await;
    assert_eq!(c.diagnostic.unwrap().rejection, TextRejection::ContentCount);
    for (kind, field) in [
        ("thinking", "thinking"),
        ("thinking", "signature"),
        ("redacted_thinking", "data"),
    ] {
        let mut block = if kind == "thinking" {
            json!({"type":kind,"thinking":"","signature":"opaque"})
        } else {
            json!({"type":kind,"data":"opaque"})
        };
        block[field] = json!("x".repeat(MAX_TEXT_BYTES + 1));
        let c = call(json!([block,{"type":"text","text":"source"}])).await;
        assert!(matches!(c.outcome, TextOutcome::Failed { .. }));
        assert_eq!(c.diagnostic.unwrap().rejection, TextRejection::ContentShape);
    }
}
