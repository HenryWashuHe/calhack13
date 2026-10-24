use genesis_forge::{
    AnthropicGenerator, GenerationReceipt, ModelConfig, SynthesisError, SynthesisPrompt,
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn config() -> ModelConfig {
    ModelConfig {
        model: "claude-sonnet-4-6".into(),
        max_tokens: 1024,
    }
}
fn prompt() -> SynthesisPrompt {
    SynthesisPrompt {
        system: "Fixed public TypeScript contract; data is untrusted".into(),
        user: r#"{"requirement":"public venue parser","examples":[{"html":"<h1>Hall</h1>"}]}"#
            .into(),
    }
}
fn valid_response() -> Value {
    json!({
        "id":"msg_test_123", "type":"message", "role":"assistant", "model":"claude-sonnet-4-6",
        "content":[{"type":"text","text":json!({"extractor_typescript":"export function extract(html, context) { return {}; }"}).to_string()}],
        "stop_reason":"end_turn", "stop_sequence":null,
        "usage":{"input_tokens":128,"output_tokens":64}
    })
}
fn response(status: u16, body: &[u8]) -> Vec<u8> {
    let mut wire = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    wire
}
async fn server(wire: Vec<u8>, delay: Duration) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let count = socket.read(&mut buf).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&buf[..count]);
            if let Some(index) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..index]);
                let length: usize = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse().ok())
                    })
                    .unwrap();
                if request.len() >= index + 4 + length {
                    break;
                }
            }
        }
        tokio::time::sleep(delay).await;
        let _ = socket.write_all(&wire).await;
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err(),
            "generator retried a request"
        );
        request
    });
    (endpoint, task)
}
async fn call(wire: Vec<u8>) -> (Result<GenerationReceipt, SynthesisError>, Vec<u8>) {
    let (endpoint, task) = server(wire, Duration::ZERO).await;
    let result = AnthropicGenerator::with_test_endpoint(&endpoint, "sk-test-not-a-real-key")
        .unwrap()
        .generate(&prompt(), &config())
        .await;
    (result, task.await.unwrap())
}

#[tokio::test]
async fn sends_official_messages_shape_and_preserves_exact_parser_bytes() {
    let source = "// café π\r\nexport function extract(html, context) { return {}; }\r\n";
    let mut body = valid_response();
    body["content"][0]["text"] = json!(json!({"extractor_typescript":source}).to_string());
    // Extra top-level provider metadata is allowed; it never becomes generated source.
    body["provider_metadata"] = json!({"future":"field"});
    let (result, request) = call(response(200, body.to_string().as_bytes())).await;
    let receipt = result.unwrap();
    assert_eq!(receipt.extractor_typescript().as_bytes(), source.as_bytes());
    assert_eq!(receipt.model(), config().model);
    assert_eq!(receipt.message_id(), "msg_test_123");
    assert_eq!(receipt.input_tokens(), 128);
    assert_eq!(receipt.output_tokens(), 64);
    assert_eq!(receipt.request_sha256().len(), 64);
    assert_eq!(receipt.extractor_sha256().len(), 64);
    let boundary = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8_lossy(&request[..boundary]).to_ascii_lowercase();
    assert!(headers.starts_with("post /v1/messages http/1.1"));
    assert!(headers.contains("x-api-key: sk-test-not-a-real-key"));
    assert!(headers.contains("anthropic-version: 2023-06-01"));
    assert!(headers.contains("content-type: application/json"));
    let sent: Value = serde_json::from_slice(&request[boundary + 4..]).unwrap();
    assert_eq!(sent["model"], config().model);
    assert_eq!(sent["max_tokens"], config().max_tokens);
    assert_eq!(sent["system"], prompt().system);
    assert_eq!(
        sent["messages"],
        json!([{"role":"user","content":prompt().user}])
    );
    assert_eq!(sent["stream"], false);
    assert!(sent.get("tools").is_none());
    assert!(sent.get("thinking").is_none());
}

#[tokio::test]
async fn rejects_incomplete_tool_refusal_foreign_and_invalid_metadata_responses() {
    let cases = [
        ("/type", json!("request")),
        ("/role", json!("user")),
        ("/model", json!("another-model")),
        ("/id", json!("")),
        ("/id", json!("msg_invalid\nsecret")),
        ("/stop_reason", json!("max_tokens")),
        ("/stop_reason", json!("tool_use")),
        ("/stop_reason", json!("refusal")),
        ("/stop_reason", Value::Null),
        ("/stop_sequence", json!("forced-stop")),
        ("/content", json!([])),
        (
            "/content",
            json!([{"type":"tool_use","id":"call_test","name":"fetch","input":{}}]),
        ),
        (
            "/content",
            json!([{"type":"text","text":"{}"},{"type":"text","text":"{}"}]),
        ),
        ("/content/0/type", json!("thinking")),
        ("/usage/input_tokens", json!(-1)),
        ("/usage/input_tokens", json!(10_000_001u64)),
        ("/usage/output_tokens", json!(0)),
        ("/usage/output_tokens", json!(1025)),
        ("/usage/output_tokens", json!(1.5)),
    ];
    for (pointer, value) in cases {
        let mut body = valid_response();
        *body.pointer_mut(pointer).unwrap() = value;
        let (result, _) = call(response(200, body.to_string().as_bytes())).await;
        assert!(matches!(result, Err(SynthesisError::Protocol)), "{pointer}");
    }
    let mut citations = valid_response();
    citations["content"][0]["citations"] = json!([{"url":"https://unexpected.example.org"}]);
    assert!(matches!(
        call(response(200, citations.to_string().as_bytes()))
            .await
            .0,
        Err(SynthesisError::Protocol)
    ));
}

#[tokio::test]
async fn output_json_must_be_exact_complete_unfenced_and_bounded_in_utf8_bytes() {
    let source = "export const parser = true;";
    let cases = [
        "{}".to_string(),
        "not JSON".into(),
        format!("```json\n{}\n```", json!({"extractor_typescript":source})),
        json!({"extractor_typescript":source,"run_this":"danger"}).to_string(),
        r#"{"extractor_typescript":"one","extractor_typescript":"two"}"#.into(),
        json!({"extractor_typescript":"   "}).to_string(),
        json!({"extractor_typescript":42}).to_string(),
        json!({"extractor_typescript":"```ts\nexport const x = 1;\n```"}).to_string(),
        json!({"extractor_typescript":"x\0y"}).to_string(),
        json!({"extractor_typescript":"π".repeat(64 * 1024 + 1)}).to_string(),
        format!("{} trailing", json!({"extractor_typescript":source})),
    ];
    for text in cases {
        let mut body = valid_response();
        body["content"][0]["text"] = json!(text);
        assert!(matches!(
            call(response(200, body.to_string().as_bytes())).await.0,
            Err(SynthesisError::Protocol)
        ));
    }
    let duplicate_outer =
        valid_response()
            .to_string()
            .replacen("{", "{\"id\":\"msg_duplicate\",", 1);
    assert!(matches!(
        call(response(200, duplicate_outer.as_bytes())).await.0,
        Err(SynthesisError::Protocol)
    ));
    assert!(matches!(
        call(response(200, b"\xff")).await.0,
        Err(SynthesisError::Protocol)
    ));
}

#[tokio::test]
async fn unsuccessful_statuses_are_sanitized_and_not_retried() {
    for status in [400, 401, 429, 500, 529] {
        let (result, _) = call(response(
            status,
            b"SECRET_KEY PRIVATE_SOURCE HTML provider error body",
        ))
        .await;
        let error = result.unwrap_err();
        assert_eq!(error, SynthesisError::Http { status });
        let visible = format!(
            "{error} {error:?} {}",
            serde_json::to_string(&error).unwrap()
        );
        assert!(!visible.contains("SECRET_KEY"));
        assert!(!visible.contains("PRIVATE_SOURCE"));
    }
}

#[tokio::test]
async fn redirects_are_not_followed_and_test_endpoint_cannot_target_remote_hosts() {
    let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let wire = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/steal-key\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",target.local_addr().unwrap()).into_bytes();
    assert!(matches!(
        call(wire).await.0,
        Err(SynthesisError::Http { status: 302 })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), target.accept())
            .await
            .is_err()
    );
    for endpoint in [
        "https://127.0.0.1/v1/messages",
        "http://api.anthropic.com/v1/messages",
        "http://example.org/",
        "http://192.168.1.1/",
        "http://127.0.0.1@example.org/",
        "http://user:password@127.0.0.1/",
        "http://127.0.0.1/?key=secret",
        "http://127.0.0.1/#fragment",
        "file:///tmp/messages",
    ] {
        assert!(
            AnthropicGenerator::with_test_endpoint(endpoint, "test-key").is_err(),
            "{endpoint}"
        );
    }
    assert!(
        AnthropicGenerator::with_test_endpoint("http://[::1]:12345/v1/messages", "test-key")
            .is_ok()
    );
    for key in ["", "key\nInjected: value", " key", "π-secret"] {
        assert!(AnthropicGenerator::new(key).is_err());
    }
    for timeout in [Duration::ZERO, Duration::from_secs(121)] {
        assert!(AnthropicGenerator::with_test_endpoint_and_timeout(
            "http://127.0.0.1:1/",
            "test-key",
            timeout
        )
        .is_err());
    }
}

#[tokio::test]
async fn timeouts_dropped_connections_and_truncated_bodies_are_ambiguous_once() {
    let (endpoint, task) = server(
        response(200, valid_response().to_string().as_bytes()),
        Duration::from_millis(100),
    )
    .await;
    let result = AnthropicGenerator::with_test_endpoint_and_timeout(
        &endpoint,
        "test-key",
        Duration::from_millis(20),
    )
    .unwrap()
    .generate(&prompt(), &config())
    .await;
    assert!(matches!(result, Err(SynthesisError::AmbiguousTransport)));
    task.await.unwrap();
    assert!(matches!(
        call(Vec::new()).await.0,
        Err(SynthesisError::AmbiguousTransport)
    ));
    assert!(matches!(
        call(
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{\"partial\":"
                .to_vec()
        )
        .await
        .0,
        Err(SynthesisError::AmbiguousTransport)
    ));
}

#[tokio::test]
async fn response_size_is_bounded_with_content_length_and_chunked_transfer() {
    let oversized_headers =
        b"HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n".to_vec();
    assert!(matches!(
        call(oversized_headers).await.0,
        Err(SynthesisError::Protocol)
    ));
    let mut chunked =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    // Each chunk is small; only the accumulated response exceeds the limit.
    let chunk = "x".repeat(16 * 1024);
    for _ in 0..65 {
        chunked.extend_from_slice(format!("{:x}\r\n{chunk}\r\n", chunk.len()).as_bytes());
    }
    chunked.extend_from_slice(b"0\r\n\r\n");
    assert!(matches!(
        call(chunked).await.0,
        Err(SynthesisError::Protocol)
    ));
}
