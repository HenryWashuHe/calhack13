use genesis_creao::{
    AccountLimiter, CreaoClient, CreateAgentRequest, ErrorKind, Limits, NativeRunStatus,
    OperationRequest, QuotaLimits, RunRequest, RunTicket, Stage, WriteAutonomy,
};
use genesis_verification::decode_json;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const TOKEN: &str = "cr_sk_synthetic_test_secret_never_live";
const AGENT: &str = "00000000-0000-4000-8000-000000000000";
const RUN: &str = "11111111-1111-4111-8111-111111111111";
const CONVERSATION: &str = "22222222-2222-4222-8222-222222222222";
const WORKSPACE: &str = "33333333-3333-4333-8333-333333333333";
const OTHER: &str = "44444444-4444-4444-8444-444444444444";

struct Reply {
    status: u16,
    body: String,
    headers: Vec<(&'static str, String)>,
    delay: Duration,
}
impl Reply {
    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            body: value.to_string(),
            headers: vec![],
            delay: Duration::ZERO,
        }
    }
}
struct Mock {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Mock {
    async fn serve(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let size = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + size {
                            break;
                        }
                    }
                    assert!(bytes.len() < 2 * 1024 * 1024);
                }
                received
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                tokio::time::sleep(reply.delay).await;
                let mut response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", reply.status, reply.body.len());
                for (name, value) in reply.headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&reply.body);
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    fn client(&self) -> CreaoClient {
        self.client_with(limits(), "test-account")
    }
    fn client_with(&self, limits: Limits, namespace: &str) -> CreaoClient {
        CreaoClient::for_local_testing(TOKEN, limits, limiter(namespace), &self.base).unwrap()
    }
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}
fn limiter(namespace: &str) -> AccountLimiter {
    AccountLimiter::in_memory(namespace, QuotaLimits::default()).unwrap()
}
fn limits() -> Limits {
    Limits {
        request_timeout: Duration::from_secs(2),
        ..Limits::default()
    }
}
fn create() -> OperationRequest {
    OperationRequest::CreateAgent(CreateAgentRequest {
        input: "Create a venue research specialist".into(),
        model: None,
        conversation_id: None,
    })
}
fn run() -> OperationRequest {
    OperationRequest::CreateRun(RunRequest {
        agent_id: AGENT.into(),
        input: json!({"venue":"A"}),
        model: Some("example/model".into()),
        conversation_id: Some(CONVERSATION.into()),
        workspace_id: Some(WORKSPACE.into()),
    })
}
fn response(status: &str, creation: bool) -> Value {
    let mut v = json!({"run_id":RUN,"status":status,"agent_id":AGENT,"conversation_id":CONVERSATION,"model":"example/model"});
    if creation {
        v["agent_id"] = Value::Null;
    } else {
        v["workspace_id"] = json!(WORKSPACE);
    }
    v
}
fn agent() -> Value {
    json!({"agent_id":AGENT,"name":"Venue specialist","description":null,"version":"0.1",
        "status":"active","write_autonomy":"full_auto","created_at":"2026-07-02T00:00:00Z",
        "updated_at":"2026-07-03T00:00:00Z","skill_content":"Research venues","config":{"name":"Venue specialist"}})
}
fn body(request: &str) -> Value {
    decode_json(request.split_once("\r\n\r\n").unwrap().1.as_bytes()).unwrap()
}

#[tokio::test]
async fn creates_agent_and_restores_ticket_for_get_only_completion() {
    let mut completed = response("completed", true);
    completed["result"] = json!({"agent_id":AGENT,"text":"Created","future_field":true});
    completed["future_metadata"] = json!({"version":2});
    let mock = Mock::serve(vec![
        Reply::json(202, response("pending", true)),
        Reply::json(200, completed.clone()),
    ])
    .await;
    let request = create();
    let ticket = mock.client().submit(&request).await.unwrap();
    assert_eq!(ticket.agent_id(), None);
    assert_eq!(ticket.run_id(), RUN);
    let restored: RunTicket = decode_json(&serde_json::to_vec(&ticket).unwrap()).unwrap();
    restored.validate_for(&request).unwrap();
    let observation = mock.client().poll(&request, &restored).await.unwrap();
    assert_eq!(observation.status, NativeRunStatus::Completed);
    assert_eq!(observation.agent_id.as_deref(), Some(AGENT));
    assert_eq!(observation.raw_response, completed);
    observation.validate_for(&request, &restored).unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /v1/agents HTTP/1.1\r\n"));
    assert_eq!(
        body(&requests[0]),
        json!({"input":"Create a venue research specialist"})
    );
    assert!(requests[1].starts_with(&format!("GET /v1/runs/{RUN} HTTP/1.1\r\n")));
    for request in requests {
        assert!(request.contains(&format!("authorization: Bearer {TOKEN}\r\n")));
        assert!(!request.lines().next().unwrap().contains(TOKEN));
    }
}

#[tokio::test]
async fn run_forwards_exact_input_suppresses_notifications_and_reuses_agent_for_next_event() {
    let mock = Mock::serve(vec![
        Reply::json(202, response("pending", false)),
        Reply::json(200, response("completed", false)),
        Reply::json(202, response("pending", false)),
    ])
    .await;
    let client = mock.client();
    let first = run();
    let ticket = client.submit(&first).await.unwrap();
    client.clone().poll(&first, &ticket).await.unwrap();
    let mut second = run();
    if let OperationRequest::CreateRun(r) = &mut second {
        r.input = json!({"venue":"B"});
    }
    client.submit(&second).await.unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("POST /v1/runs "));
    assert!(requests[1].starts_with("GET /v1/runs/"));
    assert!(requests[2].starts_with("POST /v1/runs "));
    let first_body = body(&requests[0]);
    assert_eq!(
        first_body,
        json!({"agent_id":AGENT,"input":{"venue":"A"},"model":"example/model","conversation_id":CONVERSATION,"workspace_id":WORKSPACE,"should_notify":false})
    );
    assert_eq!(body(&requests[2])["agent_id"], AGENT);
    assert_eq!(body(&requests[2])["input"], json!({"venue":"B"}));
}

#[tokio::test]
async fn mismatched_request_account_and_corrupt_ticket_rejected_before_network() {
    let mock = Mock::serve(vec![Reply::json(202, response("pending", false))]).await;
    let request = run();
    let ticket = mock.client().submit(&request).await.unwrap();
    let mut changed = request.clone();
    if let OperationRequest::CreateRun(r) = &mut changed {
        r.input = json!({"venue":"other"});
    }
    assert_eq!(
        mock.client()
            .poll(&changed, &ticket)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::TicketMismatch
    );
    assert_eq!(
        mock.client_with(limits(), "other-account")
            .poll(&request, &ticket)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::TicketMismatch
    );
    for (key, value) in [
        ("schema_version", json!(2)),
        ("run_id", json!("../../secret")),
        ("agent_id", json!(OTHER)),
        ("conversation_id", json!(OTHER)),
        ("request_sha256", json!("b".repeat(64))),
    ] {
        let mut raw = serde_json::to_value(&ticket).unwrap();
        raw[key] = value;
        let corrupt: RunTicket = serde_json::from_value(raw).unwrap();
        assert!(mock.client().poll(&request, &corrupt).await.is_err());
    }
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn poll_rejects_changed_run_agent_conversation_model_or_workspace() {
    for key in [
        "run_id",
        "agent_id",
        "conversation_id",
        "workspace_id",
        "model",
    ] {
        let mut corrupted = response("completed", false);
        corrupted[key] = if key == "model" {
            json!("other/model")
        } else {
            json!(OTHER)
        };
        let mock = Mock::serve(vec![
            Reply::json(202, response("pending", false)),
            Reply::json(200, corrupted),
        ])
        .await;
        let client = mock.client();
        let request = run();
        let ticket = client.submit(&request).await.unwrap();
        let error = client.poll(&request, &ticket).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::ProvenanceMismatch, "{key}");
        assert!(!error.post_may_have_run);
        assert_eq!(mock.requests().len(), 2);
    }
}

#[tokio::test]
async fn creation_requires_completed_agent_and_rejects_conflicting_ids() {
    for completed in [response("completed", true), {
        let mut value = response("completed", true);
        value["agent_id"] = json!(AGENT);
        value["result"] = json!({"agent_id":OTHER});
        value
    }] {
        let mock = Mock::serve(vec![
            Reply::json(202, response("pending", true)),
            Reply::json(200, completed),
        ])
        .await;
        let ticket = mock.client().submit(&create()).await.unwrap();
        assert_eq!(
            mock.client()
                .poll(&create(), &ticket)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::ProvenanceMismatch
        );
    }
}

#[tokio::test]
async fn refined_ticket_freezes_identities_first_observed_during_polling() {
    let mut pending = response("pending", true);
    pending["conversation_id"] = Value::Null;
    pending["model"] = Value::Null;
    let mut first = response("running", true);
    first["agent_id"] = json!(AGENT);
    let mut replacement = response("completed", true);
    replacement["agent_id"] = json!(OTHER);
    let mock = Mock::serve(vec![
        Reply::json(202, pending),
        Reply::json(200, first),
        Reply::json(200, replacement),
    ])
    .await;
    let client = mock.client();
    let request = create();
    let ticket = client.submit(&request).await.unwrap();
    let observed = client.poll(&request, &ticket).await.unwrap();
    let refined = ticket.refine(&request, &observed).unwrap();
    assert_eq!(refined.agent_id(), Some(AGENT));
    assert_eq!(refined.conversation_id(), Some(CONVERSATION));
    let restored: RunTicket = decode_json(&serde_json::to_vec(&refined).unwrap()).unwrap();
    assert_eq!(
        client.poll(&request, &restored).await.unwrap_err().kind,
        ErrorKind::ProvenanceMismatch
    );
    assert_eq!(mock.requests().len(), 3);
}

#[tokio::test]
async fn native_failure_cancellation_and_unknown_status_are_not_success() {
    for status in ["failed", "cancelled", "new-provider-status"] {
        let mock = Mock::serve(vec![
            Reply::json(202, response("pending", false)),
            Reply::json(200, response(status, false)),
        ])
        .await;
        let ticket = mock.client().submit(&run()).await.unwrap();
        let observed = mock.client().poll(&run(), &ticket).await.unwrap();
        assert!(!observed.status.is_completed());
        assert_eq!(observed.status.as_str(), status);
        assert_eq!(
            observed.status.is_terminal(),
            status != "new-provider-status"
        );
    }
}

#[tokio::test]
async fn raw_duplicate_keys_and_missing_required_metadata_reject() {
    let mut duplicate = Reply::json(202, response("pending", false));
    duplicate.body = duplicate.body.replacen(
        "\"status\":\"pending\"",
        "\"status\":\"pending\",\"status\":\"completed\"",
        1,
    );
    let mock = Mock::serve(vec![duplicate]).await;
    let error = mock.client().submit(&run()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::MalformedResponse);
    assert!(error.post_may_have_run);
    for key in ["run_id", "agent_id", "conversation_id", "model", "status"] {
        let mut raw = response("pending", false);
        raw.as_object_mut().unwrap().remove(key);
        let mock = Mock::serve(vec![Reply::json(202, raw)]).await;
        assert_eq!(
            mock.client().submit(&run()).await.unwrap_err().kind,
            ErrorKind::MalformedResponse,
            "{key}"
        );
    }
}

#[tokio::test]
async fn provider_token_echo_is_rejected_even_in_unknown_fields_or_escaped_json() {
    for escaped in [false, true] {
        let mut raw = response("pending", false);
        raw["unrecognized_secret"] = json!({"nested":[TOKEN]});
        let mut reply = Reply::json(202, raw);
        if escaped {
            reply.body = reply.body.replace(
                TOKEN,
                &TOKEN
                    .chars()
                    .map(|c| format!("\\u{:04x}", c as u32))
                    .collect::<String>(),
            );
        }
        let mock = Mock::serve(vec![reply]).await;
        let client = mock.client();
        let error = client.submit(&run()).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::CredentialEcho);
        assert!(!format!("{error:?} {error} {client:?}").contains(TOKEN));
    }
}

#[tokio::test]
async fn error_body_never_leaks_and_ambiguous_post_never_retries() {
    for status in [400, 500, 503] {
        let mock = Mock::serve(vec![Reply::json(
            status,
            json!({"error":{"message":TOKEN}}),
        )])
        .await;
        let error = mock.client().submit(&create()).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::HttpStatus);
        assert_eq!(error.http_status, Some(status));
        assert!(error.post_may_have_run);
        assert!(!format!("{error:?} {error}").contains(TOKEN));
        assert_eq!(mock.requests().len(), 1);
    }
}

#[tokio::test]
async fn post_timeout_is_ambiguous_and_get_timeout_is_read_only() {
    let mut delayed = Reply::json(202, response("pending", false));
    delayed.delay = Duration::from_secs(1);
    let mock = Mock::serve(vec![delayed]).await;
    let short = Limits {
        request_timeout: Duration::from_millis(30),
        ..limits()
    };
    let error = mock
        .client_with(short.clone(), "test-account")
        .submit(&run())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
    assert!(error.post_may_have_run);
    assert_eq!(mock.requests().len(), 1);
    let mut delayed = Reply::json(200, response("completed", false));
    delayed.delay = Duration::from_secs(1);
    let mock = Mock::serve(vec![Reply::json(202, response("pending", false)), delayed]).await;
    let client = mock.client_with(short, "test-account");
    let ticket = client.submit(&run()).await.unwrap();
    let error = client.poll(&run(), &ticket).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
    assert!(!error.post_may_have_run);
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn rate_limit_cooldown_applies_to_clones_and_bounds_quota_wait() {
    let mut reply = Reply::json(429, json!({"error":{"message":TOKEN}}));
    reply.headers.push(("Retry-After", "60".into()));
    let mock = Mock::serve(vec![reply]).await;
    let client = mock.client_with(
        Limits {
            request_timeout: Duration::from_millis(30),
            ..limits()
        },
        "test-account",
    );
    let error = client.submit(&create()).await.unwrap_err();
    assert_eq!(error.http_status, Some(429));
    assert_eq!(error.retry_after_secs, Some(60));
    assert!(error.post_may_have_run);
    let error = client.clone().submit(&run()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
    assert!(!error.post_may_have_run);
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn redirects_are_not_followed_or_given_credentials() {
    let mut reply = Reply::json(307, json!({}));
    reply
        .headers
        .push(("Location", "http://127.0.0.1:1/steal".into()));
    let mock = Mock::serve(vec![reply]).await;
    let error = mock.client().submit(&create()).await.unwrap_err();
    assert_eq!(error.http_status, Some(307));
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn bounds_request_size_structure_and_response_before_accepting() {
    let mock = Mock::serve(vec![]).await;
    let mut request = run();
    if let OperationRequest::CreateRun(r) = &mut request {
        r.input = json!("x".repeat(256 * 1024));
    }
    assert_eq!(
        mock.client().submit(&request).await.unwrap_err().kind,
        ErrorKind::InvalidRequest
    );
    if let OperationRequest::CreateRun(r) = &mut request {
        r.input = json!(vec![0; 10001]);
    }
    assert_eq!(
        mock.client().submit(&request).await.unwrap_err().kind,
        ErrorKind::InvalidRequest
    );
    if let OperationRequest::CreateRun(r) = &mut request {
        r.input = Value::Null;
        for _ in 0..66 {
            r.input = json!([r.input]);
        }
    }
    assert_eq!(
        mock.client().submit(&request).await.unwrap_err().kind,
        ErrorKind::InvalidRequest
    );
    assert!(mock.requests().is_empty());
    let mock = Mock::serve(vec![Reply::json(202, json!({"huge":"x".repeat(1024)}))]).await;
    let client = mock.client_with(
        Limits {
            max_response_bytes: 512,
            ..limits()
        },
        "test-account",
    );
    let error = client.submit(&create()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ResponseTooLarge);
    assert!(error.post_may_have_run);
}

#[test]
fn strict_request_serialization_and_configuration_preflight() {
    let invalid = [
        "http://localhost/v1",
        "http://example.com/v1",
        "http://127.0.0.1/v1?token=bad",
        "http://127.0.0.1/v2",
        "http://user@127.0.0.1/v1",
        "https://127.0.0.1/v1",
    ];
    for base in invalid {
        assert!(CreaoClient::for_local_testing(TOKEN, limits(), limiter("test"), base).is_err());
    }
    for token in [
        "",
        "cr_sk_",
        "wrong_prefix",
        "cr_sk_secret\n",
        "cr_sk_secret with space",
    ] {
        let error = CreaoClient::new(token, limits(), limiter("test")).unwrap_err();
        assert_eq!(error.stage, Stage::Configuration);
        if !token.is_empty() {
            assert!(!format!("{error:?}").contains(token));
        }
    }
    let mut request = serde_json::to_value(run()).unwrap();
    request["request"]["webhook_url"] = json!("https://example.com/hook");
    assert!(serde_json::from_value::<OperationRequest>(request).is_err());
    let mut create = create();
    if let OperationRequest::CreateAgent(r) = &mut create {
        r.model = Some("x".repeat(101));
    }
    assert!(create.validate().is_err());
    assert_eq!(
        run().request_sha256().unwrap(),
        run().request_sha256().unwrap()
    );
}

#[tokio::test]
async fn get_agent_preserves_config_and_distinguishes_guarded_write_policy() {
    for (value, expected) in [
        ("full_auto", WriteAutonomy::FullAuto),
        ("approval_required", WriteAutonomy::ApprovalRequired),
        ("blocked", WriteAutonomy::Blocked),
        ("future-policy", WriteAutonomy::Unknown),
    ] {
        let mut raw = agent();
        raw["write_autonomy"] = json!(value);
        let mock = Mock::serve(vec![Reply::json(200, raw.clone())]).await;
        let observed = mock.client().get_agent(AGENT).await.unwrap();
        assert_eq!(observed.agent_id, AGENT);
        assert_eq!(observed.write_autonomy, expected);
        assert_eq!(observed.raw_response, raw);
        assert!(mock.requests()[0].starts_with(&format!("GET /v1/agents/{AGENT} HTTP/1.1\r\n")));
    }
}

#[tokio::test]
async fn get_agent_rejects_wrong_identity_or_missing_documented_required_fields() {
    let mut wrong = agent();
    wrong["agent_id"] = json!(OTHER);
    let mock = Mock::serve(vec![Reply::json(200, wrong)]).await;
    assert_eq!(
        mock.client().get_agent(AGENT).await.unwrap_err().kind,
        ErrorKind::ProvenanceMismatch
    );
    assert_eq!(
        mock.client()
            .get_agent("../../wrong")
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequest
    );
    for key in [
        "agent_id",
        "name",
        "description",
        "version",
        "status",
        "write_autonomy",
        "created_at",
        "updated_at",
    ] {
        let mut raw = agent();
        raw.as_object_mut().unwrap().remove(key);
        let mock = Mock::serve(vec![Reply::json(200, raw)]).await;
        assert_eq!(
            mock.client().get_agent(AGENT).await.unwrap_err().kind,
            ErrorKind::MalformedResponse,
            "{key}"
        );
    }
}
