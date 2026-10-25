//! Can the planner's real HTTP client reach the provider from this host?
//! Sends one Messages request with a deliberately invalid key, so no
//! credential is used and nothing is billed: reaching the provider shows up
//! as HTTP 401, while a TLS or proxy failure shows up as a transport error.
//! Ignored by default because it needs the network.
use genesis_planner::llm::{AnthropicClient, LlmClient, LlmError, LlmRequest, Message};

#[tokio::test]
#[ignore = "needs outbound network"]
async fn the_planner_client_reaches_the_provider_and_is_refused_for_the_fake_key() {
    let client = AnthropicClient::new("not-a-key", "claude-sonnet-5-5", None);
    let req = LlmRequest {
        system: "x".into(),
        messages: vec![Message::user("x")],
        max_tokens: 1,
    };
    match client.complete(&req).await {
        Err(LlmError::Http { status: 401, .. }) => {}
        other => panic!("expected the provider's 401, got {other:?}"),
    }
}
