use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub fn response(text: &str) -> Value {
    json!({"id":"msg_method_1","type":"message","role":"assistant","model":"fixture-model-1",
    "content":[{"type":"text","text":text}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":100,"output_tokens":32}})
}
#[allow(dead_code)] // The CLI persistence probe uses serve_observed directly.
pub async fn serve(
    body: Vec<u8>,
    status: u16,
    delay: Duration,
) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    serve_observed(body, status, delay, || {}).await
}
pub async fn serve_observed(
    body: Vec<u8>,
    status: u16,
    delay: Duration,
    observer: impl FnOnce() + Send + 'static,
) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![];
        loop {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(boundary) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                let len = String::from_utf8_lossy(&bytes[..boundary])
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap();
                if bytes.len() >= boundary + 4 + len {
                    break;
                }
            }
        }
        observer();
        tokio::time::sleep(delay).await;
        let wire=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
        let _ = socket.write_all(wire.as_bytes()).await;
        let _ = socket.write_all(&body).await;
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err(),
            "automatic retry"
        );
        bytes
    });
    (endpoint, task)
}
