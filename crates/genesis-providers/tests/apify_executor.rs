//! Native loopback tests for the provider boundary; no live Apify credentials.
use genesis_apify::{ApifyClient, RunLimits, RunRequest};
use genesis_core::engine::RoleToProvision;
use genesis_core::ids::{ChangeId, OrgId, RoleId};
use genesis_core::model::{AuthorityGrant, Provider};
use genesis_providers::{
    ApifyCapabilityExecutor, CapabilityExecutor, CapabilityRunReceipt, DevStub, ProviderError,
    ProviderSet, ProvisionRequest,
};
use genesis_verification::{json_sha256, ArtifactPin, RunStatus, SourceSnapshot};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "provider-test-secret-never-live";

struct Reply {
    status: u16,
    body: String,
    delay: Duration,
}

impl Reply {
    fn json(body: Value) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            delay: Duration::ZERO,
        }
    }

    fn error(status: u16) -> Self {
        Self {
            status,
            body: format!("provider echoed {TOKEN} and private request body"),
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
                    if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
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
                    assert!(bytes.len() < 1024 * 1024);
                }
                received
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                tokio::time::sleep(reply.delay).await;
                let response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.status, reply.body.len(), reply.body);
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }

    fn executor(&self) -> ApifyCapabilityExecutor {
        self.executor_with(limits())
    }

    fn executor_with(&self, limits: RunLimits) -> ApifyCapabilityExecutor {
        ApifyCapabilityExecutor::new(
            ApifyClient::for_local_testing(TOKEN, limits, &self.base).unwrap(),
        )
    }

    async fn finish(self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(3), self.task)
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
        input: json!({"url": "https://fixtures.example/venue", "requested_capacity": 120}),
        source: SourceSnapshot {
            url: "https://fixtures.example/venue".into(),
            sha256: "b".repeat(64),
        },
    }
}

fn build() -> Value {
    json!({"data": {"id": "build1", "actId": "actor1", "buildNumber": "0.1.2", "status": "SUCCEEDED"}})
}

fn run(status: &str) -> Value {
    json!({"data": {"id": "run1", "actId": "actor1", "buildId": "build1", "status": status, "defaultDatasetId": "dataset1"}})
}

fn output() -> Value {
    json!({"status": "unsupported", "reason": "unsupported_format", "evidence": {"source_url": "https://fixtures.example/venue", "snapshot_sha256": "b".repeat(64), "locator": "body"}})
}

#[tokio::test]
async fn persists_exact_run_and_resumes_without_post() {
    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("RUNNING")),
        Reply::json(build()),
        Reply::json(run("SUCCEEDED")),
        Reply::json(json!([output()])),
    ])
    .await;
    let input = request();
    let submitted = mock.executor().submit(&input).await.unwrap();
    assert_eq!(
        mock.requests.lock().unwrap().len(),
        2,
        "submit must not poll before the kernel persists a ticket"
    );
    assert_eq!(submitted.provider, Provider::Apify);
    assert_eq!(submitted.raw_status, "APIFY_SUBMITTED");
    assert_eq!(submitted.provider_refs["build_id"], input.artifact.build_id);
    submitted.validate_for(&input).unwrap();
    let stored = serde_json::to_vec(&submitted).unwrap();
    let recovered: CapabilityRunReceipt = serde_json::from_slice(&stored).unwrap();
    recovered.validate_for(&input).unwrap();
    // A fresh executor emulates a process restart using only the saved ticket.
    let evidence = mock
        .executor()
        .resume(&input, recovered.ticket.as_ref().unwrap())
        .await
        .unwrap();
    assert_eq!(evidence.artifact, input.artifact);
    assert_eq!(evidence.case_id, input.case_id);
    assert_eq!(evidence.source, input.source);
    assert_eq!(evidence.input_sha256, json_sha256(&input.input));
    assert_eq!(evidence.run_id, "run1");
    assert_eq!(evidence.dataset_id, "dataset1");
    assert_eq!(evidence.status, RunStatus::Succeeded);
    assert_eq!(evidence.output, output());
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 5);
    assert!(requests[0].starts_with("GET /v2/actor-builds/build1 "));
    assert!(requests[1].starts_with("POST /v2/actors/actor1/runs?build=0.1.2&"));
    assert!(requests[1].ends_with(&input.input.to_string()));
    for request in &requests[2..] {
        assert!(request.starts_with("GET "));
    }
    for request in requests {
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {TOKEN}")));
        assert!(!request.lines().next().unwrap().contains(TOKEN));
    }
}

#[tokio::test]
async fn mismatched_pin_is_rejected_before_post() {
    let mut metadata = build();
    metadata["data"]["buildNumber"] = json!("0.1.3");
    let mock = Mock::serve(vec![Reply::json(metadata)]).await;
    let error = mock.executor().submit(&request()).await.unwrap_err();
    assert!(matches!(error, ProviderError::Rejected(_)));
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET "));
}

#[tokio::test]
async fn pre_submission_http_error_is_sanitized() {
    let mock = Mock::serve(vec![Reply::error(403)]).await;
    let error = mock.executor().submit(&request()).await.unwrap_err();
    assert!(matches!(error, ProviderError::Http { status: 403, .. }));
    let encoded = serde_json::to_string(&error).unwrap();
    assert!(!encoded.contains(TOKEN));
    assert!(!error.to_string().contains("private request body"));
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn post_http_failure_is_unknown_and_never_retried() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::error(503)]).await;
    let error = mock.executor().submit(&request()).await.unwrap_err();
    assert!(matches!(error, ProviderError::Unknown(_)));
    assert!(!format!("{error:?}").contains(TOKEN));
    assert!(!serde_json::to_string(&error)
        .unwrap()
        .contains("private request body"));
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn malformed_post_metadata_does_not_become_a_receipt_or_leak_token() {
    let mut metadata = run("RUNNING");
    metadata["data"]["id"] = json!(TOKEN);
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(metadata)]).await;
    let executor = mock.executor();
    assert!(!format!("{executor:?}").contains(TOKEN));
    let error = executor.submit(&request()).await.unwrap_err();
    assert!(matches!(error, ProviderError::Unknown(_)));
    assert!(!format!("{error:?}").contains(TOKEN));
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn lost_post_response_is_unknown_and_never_retried() {
    let mut delayed = Reply::json(run("RUNNING"));
    delayed.delay = Duration::from_millis(300);
    let mock = Mock::serve(vec![Reply::json(build()), delayed]).await;
    let configured = RunLimits {
        request_timeout: Duration::from_millis(100),
        ..limits()
    };
    let error = mock
        .executor_with(configured)
        .submit(&request())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Unknown(_)));
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn terminal_failed_run_is_rejected_instead_of_retried() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("FAILED"))]).await;
    assert!(matches!(
        mock.executor().submit(&request()).await,
        Err(ProviderError::Rejected(_))
    ));
    assert_eq!(mock.finish().await.len(), 2);

    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("RUNNING")),
        Reply::json(build()),
        Reply::json(run("FAILED")),
    ])
    .await;
    let executor = mock.executor();
    let input = request();
    let receipt = executor.submit(&input).await.unwrap();
    assert!(matches!(
        executor
            .resume(&input, receipt.ticket.as_ref().unwrap())
            .await,
        Err(ProviderError::Rejected(_))
    ));
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    assert!(requests[2..]
        .iter()
        .all(|request| request.starts_with("GET ")));
}

#[tokio::test]
async fn resume_distinguishes_retryable_http_from_terminal_output_failure() {
    for status in [404, 429, 503] {
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("RUNNING")),
            Reply::error(status),
        ])
        .await;
        let executor = mock.executor();
        let input = request();
        let receipt = executor.submit(&input).await.unwrap();
        let error = executor
            .resume(&input, receipt.ticket.as_ref().unwrap())
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::Http { status: actual, .. } if actual == status));
        assert!(!serde_json::to_string(&error).unwrap().contains(TOKEN));
        assert_eq!(mock.finish().await.len(), 3);
    }
    let mock = Mock::serve(vec![
        Reply::json(build()),
        Reply::json(run("RUNNING")),
        Reply::json(build()),
        Reply::json(run("SUCCEEDED")),
        Reply::json(json!([42])),
    ])
    .await;
    let executor = mock.executor();
    let input = request();
    let receipt = executor.submit(&input).await.unwrap();
    assert!(matches!(
        executor
            .resume(&input, receipt.ticket.as_ref().unwrap())
            .await,
        Err(ProviderError::Rejected(_))
    ));
    assert_eq!(mock.finish().await.len(), 5);
}

#[tokio::test]
async fn resume_rejects_wrong_provenance_and_keeps_unknown_status_unresolved() {
    for (mut metadata, terminal) in [(run("RUNNING"), true), (run("UNRECOGNIZED_STATUS"), false)] {
        if terminal {
            metadata["data"]["buildId"] = json!("different-build");
        }
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("RUNNING")),
            Reply::json(build()),
            Reply::json(metadata),
        ])
        .await;
        let executor = mock.executor();
        let input = request();
        let receipt = executor.submit(&input).await.unwrap();
        let error = executor
            .resume(&input, receipt.ticket.as_ref().unwrap())
            .await
            .unwrap_err();
        assert_eq!(matches!(error, ProviderError::Rejected(_)), terminal);
        assert_eq!(matches!(error, ProviderError::Unknown(_)), !terminal);
        assert_eq!(mock.finish().await.len(), 4);
    }
}

#[tokio::test]
async fn saved_ticket_rejects_changed_context_without_network() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let input = request();
    let executor = mock.executor();
    let receipt = executor.submit(&input).await.unwrap();
    let ticket = receipt.ticket.as_ref().unwrap();
    for changed in [
        RunRequest {
            case_id: "other-case".into(),
            ..input.clone()
        },
        RunRequest {
            input: json!({"url": "https://fixtures.example/different"}),
            ..input.clone()
        },
        RunRequest {
            source: SourceSnapshot {
                sha256: "c".repeat(64),
                ..input.source.clone()
            },
            ..input.clone()
        },
        RunRequest {
            artifact: ArtifactPin {
                build_id: "different-build".into(),
                ..input.artifact.clone()
            },
            ..input.clone()
        },
    ] {
        assert!(receipt.validate_for(&changed).is_err());
        assert!(matches!(
            executor.resume(&changed, ticket).await,
            Err(ProviderError::Rejected(_))
        ));
    }
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn receipt_requires_consistent_refs_and_ticket() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let input = request();
    let receipt = mock.executor().submit(&input).await.unwrap();
    for key in ["actor_id", "build_id", "build_number", "run_id"] {
        let mut changed = receipt.clone();
        changed.provider_refs.insert(key.into(), "wrong".into());
        assert!(changed.validate_for(&input).is_err());
    }
    let mut legacy = serde_json::to_value(&receipt).unwrap();
    legacy.as_object_mut().unwrap().remove("ticket");
    let legacy: CapabilityRunReceipt = serde_json::from_value(legacy).unwrap();
    assert!(legacy.ticket.is_none());
    assert!(legacy.validate_for(&input).is_err());
    let mut relabeled = receipt;
    relabeled.provider = Provider::DevStub;
    assert!(relabeled.validate_for(&input).is_err());
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn development_stub_cannot_return_trusted_execution_evidence() {
    let mock = Mock::serve(vec![Reply::json(build()), Reply::json(run("RUNNING"))]).await;
    let input = request();
    let real = mock.executor().submit(&input).await.unwrap();
    let stub = DevStub::default();
    let synthetic = stub.submit(&input).await.unwrap();
    assert_eq!(synthetic.provider, Provider::DevStub);
    assert_eq!(synthetic.raw_status, "DEVSTUB_SUBMITTED");
    assert!(synthetic.provider_refs["run_id"].starts_with("devstub-run-"));
    assert!(synthetic.ticket.is_none());
    synthetic.validate_for(&input).unwrap();
    assert!(matches!(
        stub.resume(&input, real.ticket.as_ref().unwrap()).await,
        Err(ProviderError::NotConfigured {
            provider: Provider::DevStub
        })
    ));
    assert_eq!(mock.finish().await.len(), 2);
}

#[test]
fn provisioning_configuration_digest_has_stable_canonical_bytes() {
    let req = ProvisionRequest {
        org_id: OrgId::new("org_a"),
        change_id: ChangeId::new("chg_1"),
        role: RoleToProvision {
            key: "relocation".into(),
            role_id: RoleId::new("role_1"),
            name: "Relocation lead".into(),
            charter: "Relocate".into(),
            authority: AuthorityGrant::default(),
            provider: Provider::Elastic,
        },
    };
    // Golden SHA-256 of independently sorted JSON, stable with preserve_order enabled.
    assert_eq!(
        req.config_digest(),
        "e11bf9a6ab34fd6e1db4cd1b91224e5d022f6718611aa33e162372da754c0db7"
    );
}

#[tokio::test]
async fn configured_providers_include_executor_identity_once() {
    let mock = Mock::serve(vec![]).await;
    let executor = Arc::new(mock.executor());
    assert_eq!(executor.provider(), Provider::Apify);
    let providers = ProviderSet::new().with_executor(executor);
    assert_eq!(providers.configured(), vec![Provider::Apify]);

    let stub = Arc::new(DevStub::default());
    let providers = ProviderSet::new()
        .with_provisioner(Provider::DevStub, stub.clone())
        .with_executor(stub);
    assert_eq!(providers.configured(), vec![Provider::DevStub]);
    // A permitted fallback alone does not falsely advertise a live provider.
    assert!(ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .configured()
        .is_empty());
    assert!(mock.finish().await.is_empty());
}

#[test]
fn every_provider_error_roundtrips_through_json() {
    for error in [
        ProviderError::NotConfigured {
            provider: Provider::Apify,
        },
        ProviderError::Http {
            status: 403,
            body: "Apify Build: HttpStatus(403)".into(),
        },
        ProviderError::Timeout,
        ProviderError::Rejected("Apify Configuration: InvalidRequest".into()),
        ProviderError::Unknown("Apify Submission: Transport".into()),
    ] {
        let encoded = serde_json::to_value(&error).unwrap();
        let restored: ProviderError = serde_json::from_value(encoded).unwrap();
        assert_eq!(restored, error);
    }
    assert_eq!(
        serde_json::to_value(ProviderError::Unknown("submission unresolved".into())).unwrap(),
        json!({"kind": "unknown", "detail": "submission unresolved"})
    );
}

#[test]
fn durable_run_request_roundtrips_and_validates_before_submission() {
    let input = request();
    let restored: RunRequest =
        serde_json::from_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.artifact, input.artifact);
    assert_eq!(restored.source, input.source);
    assert_eq!(restored.input, input.input);
    let mut unknown = serde_json::to_value(&input).unwrap();
    unknown["untrusted_success"] = json!(true);
    assert!(serde_json::from_value::<RunRequest>(unknown).is_err());
    let mut oversized = input;
    oversized.input = json!({"payload": "x".repeat(256 * 1024)});
    assert!(oversized.validate().is_err());
}
