//! Henry's Claude: how model HTTP responses are turned into text or errors.
//! Real clients against one-shot loopback servers.
use genesis_planner::llm::{
    AnthropicClient, LlmClient, LlmError, LlmRequest, Message, OpenAiCompatClient,
};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

/// Serve one canned response; returns the base URL.
fn serve_once(status: &str, extra_headers: &str, body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    );
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let head = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .map(|v| v.trim().parse().unwrap())
            .unwrap_or(0);
        while bytes.len() < header_end + length {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
        stream.write_all(response.as_bytes()).unwrap();
    });
    url
}

fn request() -> LlmRequest {
    LlmRequest {
        system: "Return JSON".into(),
        messages: vec![Message::user("evidence")],
        max_tokens: 64,
    }
}

async fn anthropic(status: &str, extra: &str, body: serde_json::Value) -> Result<String, LlmError> {
    let url = serve_once(status, extra, body.to_string());
    AnthropicClient::new("k", "m", Some(url))
        .complete(&request())
        .await
}

async fn error_kind_for(kind: &str) -> String {
    match anthropic(
        "429 Too Many Requests",
        "",
        json!({"type":"error","error":{"type":kind,"message":"provider text"}}),
    )
    .await
    {
        Err(LlmError::Http { status: 429, kind }) => kind,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn redirects_are_reported_as_not_followed() {
    let extra = "Location: http://127.0.0.1:9/elsewhere\r\n";
    let body = json!({"error":{"type":"moved"}});
    match anthropic("307 Temporary Redirect", extra, body.clone()).await {
        Err(LlmError::Http { status, kind }) => {
            assert_eq!(status, 307);
            assert_eq!(kind, "redirect_not_followed");
        }
        other => panic!("{other:?}"),
    }
    let url = serve_once("302 Found", extra, body.to_string());
    match OpenAiCompatClient::new("k", "m", url, "test")
        .complete(&request())
        .await
    {
        Err(LlmError::Http { status, kind }) => {
            assert_eq!(status, 302);
            assert_eq!(kind, "redirect_not_followed");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn non_success_status_is_an_http_error_even_with_a_reply_shaped_body() {
    // A 5xx carrying content must not be accepted as a model reply, and must
    // stay an `Http` error so the retry wrapper can classify it.
    match anthropic(
        "503 Service Unavailable",
        "",
        json!({"content":[{"type":"text","text":"{\"verdict\":\"change\"}"}],
               "error":{"type":"overloaded_error"}}),
    )
    .await
    {
        Err(LlmError::Http { status, kind }) => {
            assert_eq!(status, 503);
            assert_eq!(kind, "overloaded_error");
        }
        other => panic!("{other:?}"),
    }
    let url = serve_once(
        "400 Bad Request",
        "",
        json!({"choices":[{"message":{"content":"{}"}}],"error":{"code":"bad_request"}})
            .to_string(),
    );
    match OpenAiCompatClient::new("k", "m", url, "test")
        .complete(&request())
        .await
    {
        Err(LlmError::Http { status, kind }) => {
            assert_eq!(status, 400);
            assert_eq!(kind, "bad_request");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn provider_error_kind_is_kept_only_when_identifier_like() {
    assert_eq!(error_kind_for("rate_limit_error").await, "rate_limit_error");
    assert_eq!(error_kind_for("a-b.c_9").await, "a-b.c_9");
    let at_limit = "k".repeat(48);
    assert_eq!(error_kind_for(&at_limit).await, at_limit);
    assert_eq!(error_kind_for(&"k".repeat(49)).await, "unknown");
    assert_eq!(error_kind_for("has space").await, "unknown");
    assert_eq!(error_kind_for("key=secret").await, "unknown");
    assert_eq!(error_kind_for("").await, "unknown");
}

#[tokio::test]
async fn only_text_blocks_form_the_reply() {
    let reply = anthropic(
        "200 OK",
        "",
        json!({"content":[
            {"type":"thinking","thinking":"private","text":"NOT-A-REPLY"},
            {"type":"text","text":"{\"a\":"},
            {"type":"tool_use","text":"ALSO-NOT"},
            {"type":"text","text":"1}"}]}),
    )
    .await
    .unwrap();
    assert_eq!(reply, "{\"a\":1}");
}

#[tokio::test]
async fn a_reply_without_text_is_malformed() {
    for content in [
        json!([]),
        json!([{"type":"text","text":""}]),
        json!([{"type":"thinking","thinking":"only thoughts"}]),
    ] {
        match anthropic("200 OK", "", json!({ "content": content })).await {
            Err(LlmError::Malformed(m)) => assert_eq!(m, "no text content"),
            other => panic!("{content}: {other:?}"),
        }
    }
}
