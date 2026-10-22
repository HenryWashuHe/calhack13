use genesis_apify::{ApifyClient, ErrorKind, RunLimits, RunRequest, RunTicket, Stage};
use genesis_verification::{
    decode_json, json_sha256, verify, ArtifactPin, ExecutionEvidence, OracleSuite, RunStatus,
    SourceSnapshot,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "synthetic-test-secret-never-live";

struct Reply {
    status: u16,
    body: String,
    headers: Vec<(&'static str, String)>,
    delay: Duration,
}
impl Reply {
    fn json(body: Value) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
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
impl Mock {
    async fn serve(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
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
                                if key.eq_ignore_ascii_case("content-length") {
                                    value.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
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
    fn client(&self, limits: RunLimits) -> ApifyClient {
        ApifyClient::for_local_testing(TOKEN, limits, &self.base).unwrap()
    }
    async fn finish(self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(2), self.task)
            .await
            .unwrap()
            .unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

fn limits() -> RunLimits {
    RunLimits {
        request_timeout: Duration::from_secs(1),
        poll_interval: Duration::from_millis(1),
        deadline: Duration::from_secs(2),
        ..RunLimits::default()
    }
}
fn request() -> RunRequest {
    RunRequest {
        case_id: "format-a".into(),
        artifact: ArtifactPin {
            capability_id: "venue-extractor".into(),
            capability_version: "1".into(),
            source_sha256: "a".repeat(64),
            actor_id: "actor1".into(),
            build_id: "build1".into(),
            build_number: "0.1.2".into(),
        },
        input: json!({"url": "https://fixtures.example/venue"}),
        source: SourceSnapshot {
            url: "https://fixtures.example/venue".into(),
            sha256: "b".repeat(64),
        },
    }
}
fn build() -> Value {
    json!({"data":{"id":"build1", "actId":"actor1", "buildNumber":"0.1.2", "status":"SUCCEEDED"}})
}
fn run(status: &str) -> Value {
    json!({"data":{"id":"run1", "actId":"actor1", "buildId":"build1", "status":status, "defaultDatasetId":"dataset1"}})
}
fn output() -> Value {
    json!({"status":"unsupported", "reason":"unsupported_format", "evidence":{"source_url":"https://fixtures.example/venue", "snapshot_sha256":"b".repeat(64), "locator":"body"}})
}

#[tokio::test]
async fn submits_exact_pin_and_builds_receipt_only_from_provider_metadata() {
    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("RUNNING")),
        Reply::json(run("SUCCEEDED")),
        Reply::json(json!([output()])),
    ])
    .await;
    let input = request();
    let receipt = mock.client(limits()).execute(&input).await.unwrap();
    assert_eq!(receipt.artifact, input.artifact);
    assert_eq!(receipt.input_sha256, json_sha256(&input.input));
    assert_eq!(receipt.source, input.source);
    assert_eq!(receipt.run_id, "run1");
    assert_eq!(receipt.dataset_id, "dataset1");
    assert_eq!(receipt.status, RunStatus::Succeeded);
    assert_eq!(receipt.output, output());
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("GET /v2/actor-builds/build1 "));
    assert!(requests[1].starts_with("POST /v2/actors/actor1/runs?build=0.1.2&"));
    for part in [
        "timeout=120",
        "memory=256",
        "maxTotalChargeUsd=1",
        "maxItems=2",
        "forcePermissionLevel=LIMITED_PERMISSIONS",
    ] {
        assert!(requests[1].contains(part));
    }
    assert!(requests[1].ends_with(&input.input.to_string()));
    assert!(requests[2].starts_with("GET /v2/actor-runs/run1 "));
    assert!(requests[3]
        .starts_with("GET /v2/datasets/dataset1/items?format=json&limit=2&offset=0&clean=false "));
    for request in &requests {
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {TOKEN}")));
        assert!(!request.lines().next().unwrap().contains(TOKEN));
    }
}

#[tokio::test]
async fn rejects_build_identity_or_status_before_submission() {
    for (field, value, kind) in [
        ("id", "different", ErrorKind::ProvenanceMismatch),
        ("actId", "different", ErrorKind::ProvenanceMismatch),
        ("buildNumber", "0.1.3", ErrorKind::ProvenanceMismatch),
        ("status", "RUNNING", ErrorKind::BuildNotSucceeded),
    ] {
        let mut body = build();
        body["data"][field] = json!(value);
        let mock = Mock::serve(vec![Reply::json(body)]).await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert_eq!(error.kind, kind);
        assert!(!error.submission_may_have_occurred);
        assert_eq!(mock.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn rejects_wrong_run_actor_or_build_before_fetching_dataset() {
    for field in ["actId", "buildId", "id", "defaultDatasetId"] {
        let mut body = run("SUCCEEDED");
        body["data"][field] = json!("wrong/id");
        let mock = Mock::serve(vec![Reply::json(build()), Reply::json(body)]).await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::ProvenanceMismatch);
        assert!(error.submission_may_have_occurred);
        assert_eq!(mock.finish().await.len(), 2);
    }
}

#[tokio::test]
async fn validates_every_poll_record_and_dataset_identity() {
    for field in ["id", "actId", "buildId", "defaultDatasetId"] {
        let mut body = run("SUCCEEDED");
        body["data"][field] = json!("different");
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("RUNNING")),
            Reply::json(body),
        ])
        .await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::ProvenanceMismatch);
        assert_eq!(error.run_id.as_deref(), Some("run1"));
        assert_eq!(mock.finish().await.len(), 3);
    }
}

#[tokio::test]
async fn rejects_failed_and_unknown_terminal_statuses() {
    for (status, expected) in [
        ("FAILED", ErrorKind::RunFailed),
        ("TIMED-OUT", ErrorKind::RunFailed),
        ("ABORTED", ErrorKind::RunFailed),
        ("COMPLETED", ErrorKind::UnknownRunStatus),
    ] {
        let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run(status))]).await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert_eq!(error.kind, expected);
        assert_eq!(error.run_id.as_deref(), Some("run1"));
        mock.finish().await;
    }
}

#[tokio::test]
async fn terminal_success_requires_dataset_id() {
    let mut body = run("SUCCEEDED");
    body["data"]
        .as_object_mut()
        .unwrap()
        .remove("defaultDatasetId");
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(body)]).await;
    assert_eq!(
        mock.client(limits())
            .execute(&request())
            .await
            .unwrap_err()
            .kind,
        ErrorKind::MalformedResponse
    );
    mock.finish().await;
}

#[tokio::test]
async fn rejects_empty_multiple_nonobject_or_incomplete_dataset_pages() {
    for body in [
        json!([]),
        json!([output(), output()]),
        json!([1]),
        json!({"data":[output()]}),
    ] {
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("SUCCEEDED")),
            Reply::json(body),
        ])
        .await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert!(matches!(
            error.kind,
            ErrorKind::InvalidDataset | ErrorKind::MalformedResponse
        ));
        mock.finish().await;
    }
    for (header, value) in [
        ("X-Apify-Pagination-Total", "100"),
        ("X-Apify-Pagination-Count", "0"),
        ("X-Apify-Pagination-Offset", "1"),
        ("X-Apify-Pagination-Total", "oops"),
    ] {
        let mut reply = Reply::json(json!([output()]));
        reply.headers.push((header, value.into()));
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("SUCCEEDED")),
            reply,
        ])
        .await;
        assert_eq!(
            mock.client(limits())
                .execute(&request())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::InvalidDataset
        );
        mock.finish().await;
    }
}

#[tokio::test]
async fn limits_output_bytes_and_sanitizes_provider_errors() {
    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("SUCCEEDED")),
        Reply::json(json!([{"value":"x".repeat(4096)}])),
    ])
    .await;
    let mut config = limits();
    config.max_response_bytes = 1024;
    let error = mock.client(config).execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ResponseTooLarge);
    mock.finish().await;

    let mut reply = Reply::json(json!({"error":{"message":TOKEN}}));
    reply.status = 401;
    let mock = Mock::serve(vec![reply]).await;
    let client = mock.client(limits());
    let error = client.execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::HttpStatus(401));
    assert!(!format!("{client:?} {error:?} {error}").contains(TOKEN));
    mock.finish().await;
}

#[tokio::test]
async fn bounds_normalized_evidence_when_small_exponent_json_expands() {
    let marker = "PRIVATE_DATASET_KEY_MUST_NOT_APPEAR";
    let wire = format!("[{{\"{marker}\":[{}]}}]", vec!["1e9"; 100].join(","));
    assert!(wire.len() < 1024);
    let decoded: Vec<Value> = decode_json(wire.as_bytes()).unwrap();
    assert!(serde_json::to_vec(&decoded[0]).unwrap().len() > 1024);
    for resume in [false, true] {
        let mut replies = vec![Reply::json(build()), Reply::json(run("SUCCEEDED"))];
        if resume {
            replies.extend([Reply::json(build()), Reply::json(run("SUCCEEDED"))]);
        }
        replies.push(Reply {
            status: 200,
            body: wire.clone(),
            headers: vec![],
            delay: Duration::ZERO,
        });
        let mock = Mock::serve(replies).await;
        let mut config = limits();
        config.max_response_bytes = 1024;
        let client = mock.client(config);
        let error = if resume {
            let ticket = client.submit(&request()).await.unwrap();
            let restored: RunTicket =
                serde_json::from_slice(&serde_json::to_vec(&ticket).unwrap()).unwrap();
            client.resume(&request(), &restored).await.unwrap_err()
        } else {
            client.execute(&request()).await.unwrap_err()
        };
        assert_eq!(error.kind, ErrorKind::ResponseTooLarge);
        assert_eq!(error.stage, Stage::Dataset);
        assert_eq!(error.run_id.as_deref(), Some("run1"));
        assert!(error.submission_may_have_occurred);
        let diagnostic = format!("{client:?} {error:?} {error}");
        assert!(!diagnostic.contains(TOKEN));
        assert!(!diagnostic.contains(marker));
        assert!(!diagnostic.contains(&wire));
        let requests = mock.finish().await;
        assert_eq!(requests.len(), if resume { 5 } else { 3 });
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("GET /v2/datasets/dataset1/items?"))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn normalized_evidence_limit_includes_receipt_metadata_and_accepts_exact_boundary() {
    let input = request();
    let expected = ExecutionEvidence {
        case_id: input.case_id.clone(),
        artifact: input.artifact.clone(),
        input_sha256: json_sha256(&input.input),
        source: input.source.clone(),
        run_id: "run1".into(),
        dataset_id: "dataset1".into(),
        status: RunStatus::Succeeded,
        output: output(),
    };
    let evidence_bytes = serde_json::to_vec(&expected).unwrap().len();
    assert!(serde_json::to_vec(&json!([output()])).unwrap().len() < evidence_bytes - 1);
    for limit in [evidence_bytes, evidence_bytes - 1] {
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("SUCCEEDED")),
            Reply::json(json!([output()])),
        ])
        .await;
        let mut config = limits();
        config.max_response_bytes = limit;
        let result = mock.client(config).execute(&input).await;
        if limit == evidence_bytes {
            assert_eq!(
                serde_json::to_value(result.unwrap()).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind, ErrorKind::ResponseTooLarge);
            assert_eq!(error.stage, Stage::Dataset);
            assert_eq!(error.run_id.as_deref(), Some("run1"));
        }
        assert_eq!(mock.finish().await.len(), 3);
    }
}

#[tokio::test]
async fn submission_failure_is_ambiguous_and_is_never_retried() {
    let mut reply = Reply::json(json!({"error":TOKEN}));
    reply.status = 503;
    let mock = Mock::serve(vec![Reply::json(build()), reply]).await;
    let error = mock.client(limits()).execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::HttpStatus(503));
    assert_eq!(error.stage, Stage::Submission);
    assert!(error.submission_may_have_occurred);
    assert_eq!(error.run_id, None);
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn deadline_includes_poll_sleep_and_preserves_recovery_id() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let mut config = limits();
    config.deadline = Duration::from_millis(200);
    config.poll_interval = Duration::from_millis(500);
    let error = mock.client(config).execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
    assert_eq!(error.run_id.as_deref(), Some("run1"));
    assert!(error.submission_may_have_occurred);
    mock.finish().await;
}

#[tokio::test]
async fn submission_timeout_is_ambiguous_and_not_retried() {
    let mut reply = Reply::json(run("SUCCEEDED"));
    reply.delay = Duration::from_millis(500);
    let mock = Mock::serve(vec![Reply::json(build()), reply]).await;
    let mut config = limits();
    config.deadline = Duration::from_millis(200);
    let error = mock.client(config).execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
    assert_eq!(error.stage, Stage::Submission);
    assert!(error.submission_may_have_occurred);
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn refuses_redirect_without_forwarding_token() {
    let mut reply = Reply::json(json!({}));
    reply.status = 302;
    reply
        .headers
        .push(("Location", "https://attacker.invalid/collect".into()));
    let mock = Mock::serve(vec![reply]).await;
    assert_eq!(
        mock.client(limits())
            .execute(&request())
            .await
            .unwrap_err()
            .kind,
        ErrorKind::HttpStatus(302)
    );
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn malformed_provider_json_fails_without_echoing_content() {
    let reply = Reply {
        body: format!("{{{TOKEN}"),
        ..Reply::json(json!({}))
    };
    let mock = Mock::serve(vec![reply]).await;
    let error = mock.client(limits()).execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::MalformedResponse);
    assert!(!format!("{error:?}").contains(TOKEN));
    mock.finish().await;
}

#[tokio::test]
async fn invalid_input_is_rejected_before_network() {
    let client = ApifyClient::for_local_testing(TOKEN, limits(), "http://127.0.0.1:1").unwrap();
    let mut invalid = Vec::new();
    let mut value = request();
    value.artifact.build_number = "latest".into();
    invalid.push(value);
    let mut value = request();
    value.artifact.actor_id = "../run".into();
    invalid.push(value);
    let mut value = request();
    value.source.sha256 = "B".repeat(64);
    invalid.push(value);
    let mut value = request();
    value.source.url = "https://user:password@example.com".into();
    invalid.push(value);
    let mut value = request();
    value.input = json!([]);
    invalid.push(value);
    let mut value = request();
    value.input = json!({"data":"x".repeat(300 * 1024)});
    invalid.push(value);
    let mut nested = json!(0);
    for _ in 0..40 {
        nested = json!({"nested":nested});
    }
    let mut value = request();
    value.input = nested;
    invalid.push(value);
    for request in invalid {
        let error = client.execute(&request).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(!error.submission_may_have_occurred);
    }
}

#[tokio::test]
async fn duplicate_json_keys_in_metadata_and_output_are_rejected() {
    let bad_build = Reply {
        body: r#"{"data":{"id":"build1","actId":"evil","actId":"actor1","buildNumber":"0.1.2","status":"SUCCEEDED"}}"#.into(),
        ..Reply::json(json!({}))
    };
    let mock = Mock::serve(vec![bad_build]).await;
    let error = mock.client(limits()).execute(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::MalformedResponse);
    assert!(!error.submission_may_have_occurred);
    mock.finish().await;

    let duplicate_output = Reply {
        body: r#"[{"status":"supported","venues":[{"capacity":1,"capacity":500}]}]"#.into(),
        ..Reply::json(json!({}))
    };
    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("SUCCEEDED")),
        duplicate_output,
    ])
    .await;
    assert_eq!(
        mock.client(limits())
            .execute(&request())
            .await
            .unwrap_err()
            .kind,
        ErrorKind::MalformedResponse
    );
    mock.finish().await;
}

#[test]
fn credentials_endpoints_and_limits_are_validated_without_exposing_token() {
    for url in [
        "http://example.com",
        "https://127.0.0.1",
        "http://127.0.0.1/path",
        "http://127.0.0.1?token=secret",
        "http://user:pass@127.0.0.1",
        "http://127.0.0.1#fragment",
    ] {
        assert_eq!(
            ApifyClient::for_local_testing(TOKEN, limits(), url)
                .unwrap_err()
                .kind,
            ErrorKind::InvalidConfiguration
        );
    }
    for token in ["", "bad\r\nheader", "contains space"] {
        assert_eq!(
            ApifyClient::new(token, limits()).unwrap_err().kind,
            ErrorKind::InvalidConfiguration
        );
    }
    let mut invalid_limits = Vec::new();
    let mut config = limits();
    config.poll_interval = Duration::ZERO;
    invalid_limits.push(config);
    let mut config = limits();
    config.memory_mbytes = 129;
    invalid_limits.push(config);
    let mut config = limits();
    config.max_total_charge_usd = f64::NAN;
    invalid_limits.push(config);
    let mut config = limits();
    config.deadline = Duration::MAX;
    invalid_limits.push(config);
    for config in invalid_limits {
        assert_eq!(
            ApifyClient::new(TOKEN, config).unwrap_err().kind,
            ErrorKind::InvalidConfiguration
        );
    }
}

#[tokio::test]
async fn submit_persists_ticket_and_resume_never_posts() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let ticket = mock.client(limits()).submit(&request()).await.unwrap();
    assert_eq!(ticket.run_id(), "run1");
    assert_eq!(mock.finish().await.len(), 2);
    let stored = serde_json::to_vec(&ticket).unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains(TOKEN));
    let restored: RunTicket = decode_json(&stored).unwrap();

    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("RUNNING")),
        Reply::json(run("SUCCEEDED")),
        Reply::json(json!([output()])),
    ])
    .await;
    let receipt = mock
        .client(limits())
        .resume(&request(), &restored)
        .await
        .unwrap();
    assert_eq!(receipt.run_id, "run1");
    assert_eq!(receipt.output, output());
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
}

#[tokio::test]
async fn resume_binds_complete_request_and_rejects_invalid_tickets_before_network() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let ticket = mock.client(limits()).submit(&request()).await.unwrap();
    mock.finish().await;
    let client = ApifyClient::for_local_testing(TOKEN, limits(), "http://127.0.0.1:1").unwrap();
    let mut changes = Vec::new();
    let mut changed = request();
    changed.case_id = "other-case".into();
    changes.push(changed);
    let mut changed = request();
    changed.input["url"] = json!("https://other.example");
    changes.push(changed);
    let mut changed = request();
    changed.source.sha256 = "c".repeat(64);
    changes.push(changed);
    let mut changed = request();
    changed.source.url = "https://other.example".into();
    changes.push(changed);
    let mut changed = request();
    changed.artifact.capability_version = "2".into();
    changes.push(changed);
    let mut changed = request();
    changed.artifact.source_sha256 = "d".repeat(64);
    changes.push(changed);
    let mut changed = request();
    changed.artifact.build_id = "build2".into();
    changes.push(changed);
    for changed in changes {
        let error = client.resume(&changed, &ticket).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::TicketMismatch);
        assert!(!error.submission_may_have_occurred);
    }
    for (key, value) in [
        ("run_id", json!("../escape")),
        ("schema_version", json!(2)),
        ("request_sha256", json!("c".repeat(64))),
        ("dataset_id", json!("../escape")),
        ("run_id", json!(TOKEN)),
    ] {
        let mut stored = serde_json::to_value(&ticket).unwrap();
        stored[key] = value;
        let corrupt: RunTicket = serde_json::from_value(stored).unwrap();
        let error = client.resume(&request(), &corrupt).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::TicketMismatch);
        assert!(!format!("{error:?}").contains(TOKEN));
    }
}

#[tokio::test]
async fn resume_rechecks_build_run_and_prior_dataset_identity() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let ticket = mock.client(limits()).submit(&request()).await.unwrap();
    mock.finish().await;
    for field in ["id", "actId", "buildId", "defaultDatasetId"] {
        let mut modified = run("SUCCEEDED");
        modified["data"][field] = json!("other");
        let mock = Mock::serve(vec![Reply::json(build()), Reply::json(modified)]).await;
        let error = mock
            .client(limits())
            .resume(&request(), &ticket)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::ProvenanceMismatch);
        assert_eq!(error.run_id.as_deref(), Some("run1"));
        assert!(error.submission_may_have_occurred);
        assert!(mock
            .finish()
            .await
            .iter()
            .all(|request| request.starts_with("GET ")));
    }
    let mut changed = build();
    changed["data"]["buildNumber"] = json!("0.1.3");
    let mock = Mock::serve(vec![Reply::json(changed)]).await;
    assert_eq!(
        mock.client(limits())
            .resume(&request(), &ticket)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::ProvenanceMismatch
    );
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn provider_cannot_echo_credentials_into_recovery_ids_or_tickets() {
    for field in ["id", "defaultDatasetId"] {
        for echoed in [TOKEN.to_string(), format!("prefix-{TOKEN}-suffix")] {
            let mut body = run("SUCCEEDED");
            body["data"][field] = json!(echoed);
            let mock = Mock::serve(vec![Reply::json(build()), Reply::json(body)]).await;
            let error = mock.client(limits()).submit(&request()).await.unwrap_err();
            assert_eq!(error.kind, ErrorKind::MalformedResponse);
            assert_eq!(error.run_id, None);
            assert!(!format!("{error:?} {error}").contains(TOKEN));
            assert!(error.submission_may_have_occurred);
            mock.finish().await;
        }
    }
}

#[tokio::test]
async fn input_bounds_include_keys_escaped_strings_nodes_and_source_url() {
    let client = ApifyClient::for_local_testing(TOKEN, limits(), "http://127.0.0.1:1").unwrap();
    let mut cases = Vec::new();
    let mut value = request();
    value.input = json!({"rows": vec![json!(null);10000]});
    cases.push(value);
    let mut value = request();
    value.input = json!({"\"".repeat(200 * 1024):0});
    cases.push(value);
    let mut value = request();
    value.input = json!({"text":"\u{0000}".repeat(100 * 1024)});
    cases.push(value);
    let mut value = request();
    value.source.url = format!("https://example.com/{}", "a".repeat(1024));
    cases.push(value);
    let mut value = request();
    value.source.url = "https://example.com/\tfoo".into();
    cases.push(value);
    for request in cases {
        assert_eq!(
            client.execute(&request).await.unwrap_err().kind,
            ErrorKind::InvalidRequest
        );
    }
}

#[tokio::test]
async fn full_provider_receipts_pass_verifier_but_successful_wrong_output_does_not() {
    let suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    let artifact: ArtifactPin = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/artifact.json"
    ))
    .unwrap();
    let runs: Vec<ExecutionEvidence> = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap();
    for corrupt_output in [false, true] {
        let mut receipts = Vec::new();
        for (index, case) in suite.cases.iter().enumerate() {
            let fixture = runs.iter().find(|run| run.case_id == case.case_id).unwrap();
            let provider_build = json!({"data":{"id":artifact.build_id, "actId":artifact.actor_id, "buildNumber":artifact.build_number, "status":"SUCCEEDED"}});
            let provider_run = json!({"data":{"id":fixture.run_id, "actId":artifact.actor_id, "buildId":artifact.build_id, "status":"SUCCEEDED", "defaultDatasetId":fixture.dataset_id}});
            let mut output = fixture.output.clone();
            if corrupt_output && index == 0 {
                output["venues"][0]["capacity"] = json!(99999);
            }
            let mock = Mock::serve(vec![
                Reply::json(provider_build),
                Reply::json(provider_run),
                Reply::json(json!([output])),
            ])
            .await;
            let request = RunRequest {
                case_id: case.case_id.clone(),
                artifact: artifact.clone(),
                input: case.input.clone(),
                source: case.source.clone(),
            };
            receipts.push(mock.client(limits()).execute(&request).await.unwrap());
            assert_eq!(mock.finish().await.len(), 3);
        }
        let outcome = verify(&suite, &artifact, &receipts).unwrap();
        assert_eq!(outcome.is_accepted(), !corrupt_output);
        assert!(receipts
            .iter()
            .all(|receipt| receipt.status == RunStatus::Succeeded));
    }
}

#[tokio::test]
async fn public_run_ticket_preflight_rejects_schema_ids_and_request_substitution() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let ticket = mock.client(limits()).submit(&request()).await.unwrap();
    mock.finish().await;
    ticket.validate_for(&request()).unwrap();
    for (field, value) in [
        ("schema_version", json!(999)),
        ("run_id", json!("")),
        ("run_id", json!("../outside")),
        ("dataset_id", json!("../outside")),
        ("request_sha256", json!("a".repeat(64))),
    ] {
        let mut saved = serde_json::to_value(&ticket).unwrap();
        saved[field] = value;
        let invalid: RunTicket = serde_json::from_value(saved).unwrap();
        assert_eq!(
            invalid.validate_for(&request()).unwrap_err().kind,
            ErrorKind::TicketMismatch
        );
    }
    let mut changed = request();
    changed.input["url"] = json!("https://different.example");
    assert_eq!(
        ticket.validate_for(&changed).unwrap_err().kind,
        ErrorKind::TicketMismatch
    );
    let mut changed = request();
    changed.artifact.source_sha256 = "c".repeat(64);
    assert_eq!(
        ticket.validate_for(&changed).unwrap_err().kind,
        ErrorKind::TicketMismatch
    );
    let mut invalid = request();
    invalid.input = json!({"oversized":"x".repeat(8*1024*1024)});
    assert_eq!(
        ticket.validate_for(&invalid).unwrap_err().kind,
        ErrorKind::InvalidRequest
    );
}
