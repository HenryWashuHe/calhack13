use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use genesis_fetch::{
    client::{BridgeClient, ClientErrorKind, ClientLimits},
    discovery::{DiscoveryClient, SearchRequest},
    protocol::{address, Message, SendRequest, PROTOCOL_DIGEST},
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const TOKEN: &str = "local-test-token-never-a-real-key";
const A: &str = "agent1qdpstehd8x39n3jr0mas3adcy9d7rh4ss8wtw6euch0mq04tqu66kpfcu3q";
const B: &str = "agent1qgj8y2mswcc4jm275tsnq948fa7aqe8d9v0jd78h0nx9ak6v3fnxj6m6pkj";
const SEARCH_ID: &str = "cd8128ec-fb67-4223-801a-2db685358bfe";

#[derive(Clone)]
struct Reply {
    status: u16,
    body: String,
    delay: Duration,
    headers: Vec<(String, String)>,
}
impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            body: value.to_string(),
            delay: Duration::ZERO,
            headers: vec![],
        }
    }
}
#[derive(Clone)]
struct ServerState {
    reply: Arc<Mutex<Reply>>,
    identity_reply: Arc<Mutex<Option<Reply>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}
struct Server {
    base: String,
    state: ServerState,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(reply: Reply) -> Self {
        let state = ServerState {
            reply: Arc::new(Mutex::new(reply)),
            identity_reply: Default::default(),
            requests: Default::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = Router::new().fallback(handler).with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { base, state, task }
    }
    fn reply(&self, reply: Reply) {
        *self.state.reply.lock().unwrap() = reply;
    }
    fn identity_reply(&self, reply: Reply) {
        *self.state.identity_reply.lock().unwrap() = Some(reply);
    }
    fn requests(&self) -> Vec<Value> {
        self.state.requests.lock().unwrap().clone()
    }
    fn discovery(&self, limits: ClientLimits) -> DiscoveryClient {
        DiscoveryClient::for_local_testing(TOKEN, limits, &self.base).unwrap()
    }
    fn bridge(&self, limits: ClientLimits) -> BridgeClient {
        BridgeClient::new(&self.base, TOKEN, A, PROTOCOL_DIGEST, limits).unwrap()
    }
}
async fn handler(State(state): State<ServerState>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 1024 * 1024).await.unwrap();
    state.requests.lock().unwrap().push(json!({
        "method": parts.method.to_string(), "uri": parts.uri.to_string(),
        "authorization": parts.headers.get("authorization").and_then(|h| h.to_str().ok()),
        "content_type": parts.headers.get("content-type").and_then(|h| h.to_str().ok()),
        "body": if body.is_empty() { Value::Null } else { serde_json::from_slice::<Value>(&body).unwrap() },
    }));
    let reply = if parts.uri.path() == "/v1/identity" {
        state.identity_reply.lock().unwrap().clone()
    } else {
        None
    }
    .unwrap_or_else(|| state.reply.lock().unwrap().clone());
    tokio::time::sleep(reply.delay).await;
    let mut response = Response::builder().status(StatusCode::from_u16(reply.status).unwrap());
    for (key, value) in reply.headers {
        response = response.header(key, value);
    }
    response.body(Body::from(reply.body)).unwrap()
}
fn search() -> SearchRequest {
    SearchRequest {
        search_text: "venue accessibility specialist".into(),
        protocol_digest: PROTOCOL_DIGEST.into(),
        states: vec!["active".into()],
        agent_types: vec!["uagent".into()],
        offset: 0,
        limit: 10,
        search_id: None,
    }
}
fn page(agents: Vec<Value>) -> Value {
    json!({"offset":0,"limit":10,"num_hits":agents.len(),"total":agents.len(),"search_id":SEARCH_ID,"agents":agents})
}
fn advert(agent: &str, protocols: Value) -> Value {
    json!({"address":agent,"name":"Independent specialist","protocols":protocols,"endpoints":["https://untrusted.invalid/never-fetch"]})
}
fn message() -> Message {
    serde_json::from_value(json!({
        "schema_version":1,"message_id":"9d8138ec-fb67-4223-801a-2db685358bfe",
        "org_id":"org-a","mission_id":"mission","contract_id":"contract","correlation_id":"correlation",
        "expires_at":"2099-01-01T00:00:00Z",
        "body":{"kind":"recruit_request","requirement":"Find accessible venues","capabilities":["venue-review"],"lease_expires_at":"2099-01-01T00:00:00Z"}
    })).unwrap()
}
fn send() -> SendRequest {
    SendRequest {
        schema_version: 1,
        recipient: B.into(),
        message: message(),
    }
}
fn identity() -> Value {
    json!({"schema_version":1,"address":A,"protocol_digest":PROTOCOL_DIGEST})
}
fn receipt() -> Value {
    json!({"schema_version":1,"message_id":message().message_id,"bridge_address":A,"recipient":B,"state":"queued"})
}
fn inbox(items: Vec<Value>, next: u64) -> Value {
    json!({"schema_version":1,"address":A,"items":items,"next_cursor":next})
}
fn item(sequence: u64) -> Value {
    json!({"sequence":sequence,"sender":B,"message":message()})
}

#[tokio::test]
async fn discovery_posts_exact_authenticated_protocol_filter_and_normalizes_both_forms() {
    assert!(address(A) && address(B));
    let advertised = vec![
        advert(A, json!([PROTOCOL_DIGEST])),
        advert(
            B,
            json!([{"name":"GENESIS","version":"1.0","digest":PROTOCOL_DIGEST}]),
        ),
    ];
    let server = Server::start(Reply::json(page(advertised))).await;
    let result = server
        .discovery(ClientLimits::default())
        .search(&search())
        .await
        .unwrap();
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(result.candidates[0].protocol_digests, vec![PROTOCOL_DIGEST]);
    assert_eq!(
        result.candidates[1].advertised_metadata["endpoints"][0],
        "https://untrusted.invalid/never-fetch"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["method"], "POST");
    assert_eq!(requests[0]["uri"], "/v1/search/agents");
    assert_eq!(requests[0]["authorization"], format!("Bearer {TOKEN}"));
    assert_eq!(requests[0]["content_type"], "application/json");
    assert_eq!(
        requests[0]["body"],
        json!({"search_text":"venue accessibility specialist","filters":{"state":["active"],"agent_type":["uagent"],"protocol_digest":[PROTOCOL_DIGEST]},"sort":"relevancy","direction":"desc","offset":0,"limit":10})
    );
}

#[tokio::test]
async fn advertisements_without_matching_protocol_or_valid_address_are_not_candidates() {
    let agents = vec![
        advert(A, json!(["proto:bad"])),
        advert("not-an-agent", json!([PROTOCOL_DIGEST])),
        advert(B, json!([false,{"digest":PROTOCOL_DIGEST}])),
    ];
    let server = Server::start(Reply::json(page(agents))).await;
    let result = server
        .discovery(ClientLimits::default())
        .search(&search())
        .await
        .unwrap();
    assert_eq!(result.rejected_candidates, 2);
    assert_eq!(result.num_hits, 3);
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].address, B);
}

#[tokio::test]
async fn discovery_validates_paging_session_and_result_counts() {
    let server = Server::start(Reply::json(page(vec![]))).await;
    let client = server.discovery(ClientLimits::default());
    let mut req = search();
    req.search_id = Some(SEARCH_ID.into());
    assert!(client.search(&req).await.is_ok());
    for (key, value) in [
        ("offset", json!(1)),
        ("limit", json!(9)),
        ("num_hits", json!(1)),
        ("search_id", json!("1d8138ec-fb67-4223-801a-2db685358bfe")),
    ] {
        let mut bad = page(vec![]);
        bad[key] = value;
        server.reply(Reply::json(bad));
        let err = client.search(&req).await.unwrap_err();
        assert_eq!(err.kind, ClientErrorKind::MalformedResponse);
    }
    server.reply(Reply::json(page(vec![
        advert(A, json!([PROTOCOL_DIGEST])),
        advert(A, json!([PROTOCOL_DIGEST])),
    ])));
    assert_eq!(
        client.search(&req).await.unwrap_err().kind,
        ClientErrorKind::MalformedResponse
    );
}

#[tokio::test]
async fn invalid_search_filter_or_missing_protocol_fails_before_network() {
    let server = Server::start(Reply::json(page(vec![]))).await;
    let client = server.discovery(ClientLimits::default());
    for n in 0..6 {
        let mut req = search();
        match n {
            0 => req.protocol_digest.clear(),
            1 => req.limit = 51,
            2 => req.states = vec!["live".into()],
            3 => req.agent_types = vec!["uagent".into(), "uagent".into()],
            4 => req.search_id = Some("bad".into()),
            _ => req.search_text = TOKEN.into(),
        }
        let err = client.search(&req).await.unwrap_err();
        assert_eq!(err.kind, ClientErrorKind::InvalidRequest);
        assert!(!err.post_may_have_run);
    }
    assert!(server.requests().is_empty());
}

#[test]
fn endpoint_configuration_rejects_remote_dns_credentials_and_invalid_bounds() {
    let limits = ClientLimits::default();
    for base in [
        "http://localhost:8000/v1",
        "http://192.168.0.1:8000/v1",
        "https://127.0.0.1/v1",
        "http://127.0.0.1/v1?x=1",
        "http://user@127.0.0.1/v1",
        "http://127.0.0.1/v1/",
        "http://127.0.0.1/v1#x",
    ] {
        assert!(DiscoveryClient::for_local_testing(TOKEN, limits, base).is_err());
        assert!(BridgeClient::new(base, TOKEN, A, PROTOCOL_DIGEST, limits).is_err());
    }
    assert!(DiscoveryClient::new("bad\nheader", limits).is_err());
    assert!(DiscoveryClient::new(
        TOKEN,
        ClientLimits {
            timeout: Duration::ZERO,
            ..limits
        }
    )
    .is_err());
    assert!(DiscoveryClient::new(
        TOKEN,
        ClientLimits {
            max_response_bytes: 1024 * 1024 + 1,
            ..limits
        }
    )
    .is_err());
    assert!(BridgeClient::new(
        "http://127.0.0.1/v1",
        TOKEN,
        "agent1fake",
        PROTOCOL_DIGEST,
        limits
    )
    .is_err());
}

#[tokio::test]
async fn bridge_identity_and_receipts_are_bound_to_expected_peer_and_message() {
    let server = Server::start(Reply::json(identity())).await;
    let client = server.bridge(ClientLimits::default());
    assert_eq!(client.identity().await.unwrap().address, A);
    server.identity_reply(Reply::json(identity()));
    let mut accepted = Reply::json(receipt());
    accepted.status = 202;
    server.reply(accepted.clone());
    assert_eq!(client.send(&send()).await.unwrap().state, "queued");
    for (key, value) in [
        ("bridge_address", json!(B)),
        ("recipient", json!(A)),
        ("state", json!("completed")),
        ("message_id", json!(SEARCH_ID)),
        ("schema_version", json!(2)),
    ] {
        let mut bad = receipt();
        bad[key] = value;
        let mut reply = Reply::json(bad);
        reply.status = 202;
        server.reply(reply);
        let err = client.send(&send()).await.unwrap_err();
        assert_eq!(err.kind, ClientErrorKind::ReceiptMismatch);
        assert!(err.post_may_have_run);
    }
    let request = &server.requests()[2];
    assert_eq!(request["body"], serde_json::to_value(send()).unwrap());
    assert_eq!(request["uri"], "/v1/messages");
}

#[tokio::test]
async fn wrong_bridge_identity_prevents_disclosing_or_enqueuing_message() {
    let mut wrong = identity();
    wrong["address"] = json!(B);
    let server = Server::start(Reply::json(wrong)).await;
    let err = server
        .bridge(ClientLimits::default())
        .send(&send())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ClientErrorKind::IdentityMismatch);
    assert!(!err.post_may_have_run);
    assert_eq!(server.requests().len(), 1);
    assert_eq!(server.requests()[0]["method"], "GET");
    assert_eq!(server.requests()[0]["body"], Value::Null);
}

#[tokio::test]
async fn identity_preflight_and_send_share_one_overall_deadline() {
    let mut reply = Reply::json(receipt());
    reply.status = 202;
    reply.delay = Duration::from_millis(50);
    let server = Server::start(reply).await;
    server.identity_reply(Reply {
        delay: Duration::from_millis(50),
        ..Reply::json(identity())
    });
    let client = server.bridge(ClientLimits {
        timeout: Duration::from_millis(80),
        ..ClientLimits::default()
    });
    let err = client.send(&send()).await.unwrap_err();
    assert_eq!(err.kind, ClientErrorKind::Timeout);
    assert!(err.post_may_have_run);
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn bridge_rejects_identity_mismatch_and_extra_wire_fields() {
    let server = Server::start(Reply::json(identity())).await;
    let client = server.bridge(ClientLimits::default());
    for (key, value) in [
        ("address", json!(B)),
        ("schema_version", json!(2)),
        ("protocol_digest", json!("proto:wrong")),
    ] {
        let mut bad = identity();
        bad[key] = value;
        server.reply(Reply::json(bad));
        assert_eq!(
            client.identity().await.unwrap_err().kind,
            ClientErrorKind::IdentityMismatch
        );
    }
    let mut bad = identity();
    bad["untrusted_sender"] = json!(B);
    server.reply(Reply::json(bad));
    assert_eq!(
        client.identity().await.unwrap_err().kind,
        ClientErrorKind::MalformedResponse
    );
}

#[tokio::test]
async fn inbox_preserves_authenticated_sender_separately_from_payload() {
    let server = Server::start(Reply::json(inbox(vec![item(4), item(5)], 5))).await;
    let result = server
        .bridge(ClientLimits::default())
        .inbox(3, 2)
        .await
        .unwrap();
    assert_eq!(result.items[0].sender, B);
    assert_eq!(result.items[0].message.org_id, "org-a");
    assert_eq!(result.next_cursor, 5);
    assert_eq!(server.requests()[0]["uri"], "/v1/inbox?after=3&limit=2");
    server.reply(Reply::json(inbox(vec![], 5)));
    assert_eq!(
        server
            .bridge(ClientLimits::default())
            .inbox(5, 2)
            .await
            .unwrap()
            .next_cursor,
        5
    );
}

#[tokio::test]
async fn inbox_rejects_reordered_pages_wrong_cursor_and_invalid_messages() {
    let server = Server::start(Reply::json(inbox(vec![], 0))).await;
    let client = server.bridge(ClientLimits::default());
    let mut invalid_sender = item(2);
    invalid_sender["sender"] = json!("forged");
    let mut invalid_message = item(2);
    invalid_message["message"]["org_id"] = json!("");
    for bad in [
        inbox(vec![item(3), item(2)], 2),
        inbox(vec![item(1)], 1),
        inbox(vec![item(2)], 3),
        inbox(vec![], 2),
        inbox(vec![invalid_sender], 2),
        inbox(vec![invalid_message], 2),
    ] {
        server.reply(Reply::json(bad));
        assert_eq!(
            client.inbox(1, 2).await.unwrap_err().kind,
            ClientErrorKind::MalformedResponse
        );
    }
    server.reply(Reply::json(inbox(vec![item(2), item(3)], 3)));
    assert_eq!(
        client.inbox(1, 1).await.unwrap_err().kind,
        ClientErrorKind::MalformedResponse
    );
}

#[tokio::test]
async fn invalid_bridge_request_is_rejected_before_network() {
    let server = Server::start(Reply::json(identity())).await;
    let client = server.bridge(ClientLimits::default());
    let mut request = send();
    request.message.org_id = String::new();
    assert_eq!(
        client.send(&request).await.unwrap_err().kind,
        ClientErrorKind::InvalidRequest
    );
    let mut request = send();
    request.recipient = "forged".into();
    assert_eq!(
        client.send(&request).await.unwrap_err().kind,
        ClientErrorKind::InvalidRequest
    );
    assert_eq!(
        client.inbox(0, 51).await.unwrap_err().kind,
        ClientErrorKind::InvalidRequest
    );
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn redirects_and_rate_errors_are_not_retried_and_body_is_not_exposed() {
    let server = Server::start(Reply::json(page(vec![]))).await;
    let client = server.discovery(ClientLimits::default());
    for status in [307, 429, 503] {
        server.reply(Reply {
            status,
            body: format!("private {TOKEN}"),
            delay: Duration::ZERO,
            headers: vec![
                ("location".into(), format!("{}/redirected", server.base)),
                ("retry-after".into(), "17".into()),
            ],
        });
        let err = client.search(&search()).await.unwrap_err();
        assert_eq!(err.kind, ClientErrorKind::HttpStatus);
        assert_eq!(err.http_status, Some(status));
        assert_eq!(err.retry_after_secs, Some(17));
        assert!(err.post_may_have_run);
        assert!(!format!("{err:?} {err}").contains(TOKEN));
    }
    assert_eq!(server.requests().len(), 3);
    assert!(!format!("{client:?}").contains(TOKEN));
}

#[tokio::test]
async fn every_operation_has_a_deadline_and_does_not_retry_post() {
    let server = Server::start(Reply {
        delay: Duration::from_secs(1),
        ..Reply::json(page(vec![]))
    })
    .await;
    let limits = ClientLimits {
        timeout: Duration::from_millis(30),
        ..ClientLimits::default()
    };
    let err = server
        .discovery(limits)
        .search(&search())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ClientErrorKind::Timeout);
    assert!(err.post_may_have_run);
    let err = server.bridge(limits).identity().await.unwrap_err();
    assert_eq!(err.kind, ClientErrorKind::Timeout);
    assert!(!err.post_may_have_run);
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn bounded_response_and_duplicate_json_keys_fail_closed() {
    let server = Server::start(Reply::json(json!({"padding":"x".repeat(2048)}))).await;
    let client = server.discovery(ClientLimits {
        max_response_bytes: 1024,
        ..ClientLimits::default()
    });
    assert_eq!(
        client.search(&search()).await.unwrap_err().kind,
        ClientErrorKind::ResponseTooLarge
    );
    server.reply(Reply {
        body: "{\"offset\":0,\"offset\":1}".into(),
        ..Reply::json(json!({}))
    });
    assert_eq!(
        client.search(&search()).await.unwrap_err().kind,
        ClientErrorKind::MalformedResponse
    );
    server.reply(Reply {
        body: "{\"value\":NaN}".into(),
        ..Reply::json(json!({}))
    });
    assert_eq!(
        client.search(&search()).await.unwrap_err().kind,
        ClientErrorKind::MalformedResponse
    );
}

#[tokio::test]
async fn credential_echo_is_rejected_even_in_escaped_ignored_metadata() {
    let server = Server::start(Reply::json(page(vec![]))).await;
    let client = server.discovery(ClientLimits::default());
    for encoded in [
        TOKEN.into(),
        TOKEN
            .chars()
            .map(|c| format!("\\u{:04x}", c as u32))
            .collect::<String>(),
    ] {
        let body = page(vec![]).to_string().trim_end_matches('}').to_owned()
            + ",\"ignored\":\""
            + &encoded
            + "\"}";
        server.reply(Reply {
            body,
            ..Reply::json(json!({}))
        });
        let err = client.search(&search()).await.unwrap_err();
        assert_eq!(err.kind, ClientErrorKind::CredentialEcho);
        assert!(!format!("{err:?}").contains(TOKEN));
    }
}

#[tokio::test]
async fn chunked_response_is_bounded_without_content_length() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        for _ in 0..3 {
            if stream
                .write_all(format!("200\r\n{}\r\n", "x".repeat(512)).as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
    });
    let client = DiscoveryClient::for_local_testing(
        TOKEN,
        ClientLimits {
            max_response_bytes: 1024,
            ..ClientLimits::default()
        },
        &base,
    )
    .unwrap();
    assert_eq!(
        client.search(&search()).await.unwrap_err().kind,
        ClientErrorKind::ResponseTooLarge
    );
    task.await.unwrap();
}
