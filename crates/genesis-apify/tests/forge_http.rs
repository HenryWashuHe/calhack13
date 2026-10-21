use genesis_apify::{
    ActorDefinition, ActorTicket, ApifyClient, BuildTicket, ForgeErrorKind, ForgeStage, RunLimits,
    SourceFile,
};
use genesis_verification::decode_json;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "synthetic-forge-secret-never-live";
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
                let reply = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",reply.status,reply.body.len(),reply.body);
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    fn client(&self) -> ApifyClient {
        ApifyClient::for_local_testing(TOKEN, limits(), &self.base).unwrap()
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
        deadline: Duration::from_secs(2),
        request_timeout: Duration::from_secs(1),
        poll_interval: Duration::from_millis(1),
        ..RunLimits::default()
    }
}
fn definition() -> ActorDefinition {
    ActorDefinition {
        name: "genesis-venue-v1".into(),
        version_number: "0.1".into(),
        source_files: vec![
            SourceFile {
                name: "src/main.ts".into(),
                content: "console.log('synthetic');".into(),
            },
            SourceFile {
                name: ".actor/actor.json".into(),
                content: r#"{"actorSpecification":1,"name":"genesis-venue-v1","version":"0.1"}"#
                    .into(),
            },
        ],
    }
}
fn version() -> Value {
    json!({"versionNumber":"0.1","sourceType":"SOURCE_FILES","sourceFiles":definition().source_files.iter().map(|file| json!({"name":file.name,"content":file.content,"format":"TEXT"})).collect::<Vec<_>>(),"envVars":[],"applyEnvVarsToBuild":false})
}
fn actor() -> Value {
    json!({"data":{"id":"actor1","name":"genesis-venue-v1","isPublic":false,"actorPermissionLevel":"LIMITED_PERMISSIONS","versions":[version()]}})
}
fn build(status: &str) -> Value {
    json!({"data":{"id":"build1","actId":"actor1","buildNumber":"0.1.1","status":status,"actVersion":version()}})
}
async fn tickets() -> (ActorTicket, BuildTicket) {
    let mock = Mock::serve(vec![
        Reply::json(actor()),
        Reply::json(json!({"data":version()})),
        Reply::json(build("RUNNING")),
    ])
    .await;
    let client = mock.client();
    let actor = client.create_actor(&definition()).await.unwrap();
    let build = client.start_build(&definition(), &actor).await.unwrap();
    mock.finish().await;
    (actor, build)
}

#[tokio::test]
async fn creates_inline_actor_and_builds_exact_version_with_resumable_tickets() {
    let mock = Mock::serve(vec![
        Reply::json(actor()),
        Reply::json(json!({"data":version()})),
        Reply::json(build("RUNNING")),
    ])
    .await;
    let client = mock.client();
    let actor = client.create_actor(&definition()).await.unwrap();
    assert_eq!(actor.actor_id(), "actor1");
    assert_eq!(actor.version_number(), "0.1");
    assert_eq!(
        actor.definition_sha256(),
        definition().definition_sha256().unwrap()
    );
    let actor: ActorTicket = decode_json(&serde_json::to_vec(&actor).unwrap()).unwrap();
    let ticket = client.start_build(&definition(), &actor).await.unwrap();
    assert_eq!(ticket.build_id(), "build1");
    assert_eq!(ticket.build_number(), "0.1.1");
    assert_eq!(ticket.definition_sha256(), actor.definition_sha256());
    let ticket: BuildTicket = decode_json(&serde_json::to_vec(&ticket).unwrap()).unwrap();
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("POST /v2/actors "));
    let sent: Value = serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(sent["isPublic"], false);
    assert_eq!(sent["actorPermissionLevel"], "LIMITED_PERMISSIONS");
    assert_eq!(sent["versions"][0]["sourceType"], "SOURCE_FILES");
    assert_eq!(sent["versions"][0]["applyEnvVarsToBuild"], false);
    assert_eq!(sent["versions"][0]["envVars"], json!([]));
    assert_eq!(sent["versions"][0]["buildTag"], Value::Null);
    assert_eq!(
        sent["versions"][0]["sourceFiles"][0]["name"],
        ".actor/actor.json"
    );
    assert!(requests[1].starts_with("GET /v2/actors/actor1/versions/0.1 "));
    assert!(requests[2].starts_with("POST /v2/actors/actor1/builds?version=0.1&useCache=false&betaPackages=false&waitForFinish=0 "));
    for request in &requests {
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {TOKEN}")));
        assert!(!request.lines().next().unwrap().contains(TOKEN));
    }
    let mock = Mock::serve(vec![
        Reply::json(build("RUNNING")),
        Reply::json(build("SUCCEEDED")),
    ])
    .await;
    let built = mock
        .client()
        .resume_build(&definition(), &actor, &ticket)
        .await
        .unwrap();
    assert_eq!(built.actor_id(), "actor1");
    assert_eq!(built.build_id(), "build1");
    assert_eq!(built.build_number(), "0.1.1");
    assert_eq!(built.version_number(), "0.1");
    assert_eq!(built.definition_sha256(), actor.definition_sha256());
    assert!(mock
        .finish()
        .await
        .iter()
        .all(|request| request.starts_with("GET /v2/actor-builds/build1 ")));
}

#[tokio::test]
async fn source_reordering_is_equivalent_but_changed_definition_rejects_before_network() {
    let (actor, ticket) = tickets().await;
    let client = ApifyClient::for_local_testing(TOKEN, limits(), "http://127.0.0.1:1").unwrap();
    let mut mutations = Vec::new();
    let mut changed = definition();
    changed.source_files[0].content.push_str("different");
    mutations.push(changed);
    let mut changed = definition();
    changed.name = "other-name".into();
    mutations.push(changed);
    let mut changed = definition();
    changed.version_number = "0.2".into();
    mutations.push(changed);
    for changed in mutations {
        let error = client.start_build(&changed, &actor).await.unwrap_err();
        assert_eq!(error.kind, ForgeErrorKind::TicketMismatch);
        assert!(!error.submission_may_have_occurred);
        assert_eq!(
            client
                .resume_build(&changed, &actor, &ticket)
                .await
                .unwrap_err()
                .kind,
            ForgeErrorKind::TicketMismatch
        );
    }
    let mut reversed = definition();
    reversed.source_files.reverse();
    let mut response = build("SUCCEEDED");
    response["data"]["actVersion"]["sourceFiles"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let mock = Mock::serve(vec![Reply::json(response)]).await;
    mock.client()
        .resume_build(&reversed, &actor, &ticket)
        .await
        .unwrap();
    mock.finish().await;
}

#[tokio::test]
async fn rejects_invalid_definitions_before_network() {
    let client = ApifyClient::for_local_testing(TOKEN, limits(), "http://127.0.0.1:1").unwrap();
    let mut invalid = Vec::new();
    for name in ["../escape", "UpperCase", "a b", "-leading", "trailing-"] {
        let mut d = definition();
        d.name = name.into();
        invalid.push(d);
    }
    for version in ["latest", "1", "100.0", "01.0", "0.01", "0.1.1"] {
        let mut d = definition();
        d.version_number = version.into();
        invalid.push(d);
    }
    for path in [
        "../evil",
        "/absolute",
        "src//main.ts",
        "src/./main.ts",
        "src/../main.ts",
        "C:\\x",
        "src/\\x",
    ] {
        let mut d = definition();
        d.source_files[0].name = path.into();
        invalid.push(d);
    }
    let mut d = definition();
    d.source_files.clear();
    invalid.push(d);
    let mut d = definition();
    d.source_files.push(d.source_files[0].clone());
    invalid.push(d);
    let mut d = definition();
    d.source_files.push(SourceFile {
        name: "src".into(),
        content: "file-dir collision".into(),
    });
    invalid.push(d);
    let mut d = definition();
    d.source_files[0].content = "x".repeat(300 * 1024);
    invalid.push(d);
    let mut d = definition();
    d.source_files[0].content = "\u{0000}".repeat(100 * 1024);
    invalid.push(d);
    let mut d = definition();
    d.source_files[0].content = TOKEN.into();
    invalid.push(d);
    for definition in invalid {
        assert_eq!(
            client.create_actor(&definition).await.unwrap_err().kind,
            ForgeErrorKind::InvalidDefinition
        );
    }
}

#[tokio::test]
async fn create_response_mismatches_are_ambiguous_and_never_silently_recreated() {
    let mut bodies = Vec::new();
    let mut body = actor();
    body["data"]["name"] = json!("other");
    bodies.push(body);
    let mut body = actor();
    body["data"]["isPublic"] = json!(true);
    bodies.push(body);
    let mut body = actor();
    body["data"]["actorPermissionLevel"] = json!("FULL_PERMISSIONS");
    bodies.push(body);
    let mut body = actor();
    body["data"]["versions"][0]["versionNumber"] = json!("0.2");
    bodies.push(body);
    let mut body = actor();
    body["data"]["versions"][0]["sourceFiles"][0]["content"] = json!("wrong");
    bodies.push(body);
    for body in bodies {
        let mock = Mock::serve(vec![Reply::json(body)]).await;
        let error = mock.client().create_actor(&definition()).await.unwrap_err();
        assert_eq!(error.kind, ForgeErrorKind::ProvenanceMismatch);
        assert_eq!(error.actor_id.as_deref(), Some("actor1"));
        assert!(error.submission_may_have_occurred);
        assert_eq!(mock.finish().await.len(), 1);
    }
}

fn source_mutations() -> Vec<Value> {
    let mut cases = Vec::new();
    let mut v = version();
    v["versionNumber"] = json!("0.2");
    cases.push(v);
    let mut v = version();
    v["sourceType"] = json!("GIT_REPO");
    cases.push(v);
    let mut v = version();
    v["sourceFiles"][0]["content"] = json!("wrong");
    cases.push(v);
    let mut v = version();
    v["sourceFiles"][0]["format"] = json!("BASE64");
    cases.push(v);
    let mut v = version();
    v["sourceFiles"][0] = json!({"name":"unknown.ts"});
    cases.push(v);
    let mut v = version();
    v["sourceFiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("content");
    cases.push(v);
    let mut v = version();
    v["sourceFiles"][1] = v["sourceFiles"][0].clone();
    cases.push(v);
    let mut v = version();
    v["sourceFiles"].as_array_mut().unwrap().pop();
    cases.push(v);
    let mut v = version();
    v.as_object_mut().unwrap().remove("sourceFiles");
    cases.push(v);
    let mut v = version();
    v["envVars"] = json!([{"name":"CREDENTIAL","value":"secret"}]);
    cases.push(v);
    let mut v = version();
    v["applyEnvVarsToBuild"] = json!(true);
    cases.push(v);
    cases
}

#[tokio::test]
async fn source_preflight_rejects_drift_before_build_submission() {
    let (actor, _) = tickets().await;
    for version in source_mutations() {
        let mock = Mock::serve(vec![Reply::json(json!({"data":version}))]).await;
        let error = mock
            .client()
            .start_build(&definition(), &actor)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ForgeErrorKind::ProvenanceMismatch);
        assert_eq!(error.stage, ForgeStage::ReadVersion);
        assert!(!error.submission_may_have_occurred);
        assert_eq!(mock.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn terminal_build_requires_matching_captured_source_not_mutable_version() {
    let (actor, ticket) = tickets().await;
    let mut builds: Vec<_> = source_mutations()
        .into_iter()
        .map(|source| {
            let mut b = build("SUCCEEDED");
            b["data"]["actVersion"] = source;
            b
        })
        .collect();
    let mut missing = build("SUCCEEDED");
    missing["data"]
        .as_object_mut()
        .unwrap()
        .remove("actVersion");
    builds.push(missing);
    for body in builds {
        let mock = Mock::serve(vec![Reply::json(body)]).await;
        let error = mock
            .client()
            .resume_build(&definition(), &actor, &ticket)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ForgeErrorKind::ProvenanceMismatch);
        assert_eq!(error.build_id.as_deref(), Some("build1"));
        assert_eq!(mock.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn rejects_wrong_actor_build_number_id_and_failed_or_unknown_status() {
    let (actor, ticket) = tickets().await;
    for field in ["id", "actId", "buildNumber"] {
        let mut body = build("SUCCEEDED");
        body["data"][field] = json!("wrong");
        let mock = Mock::serve(vec![Reply::json(body)]).await;
        assert_eq!(
            mock.client()
                .resume_build(&definition(), &actor, &ticket)
                .await
                .unwrap_err()
                .kind,
            ForgeErrorKind::ProvenanceMismatch
        );
        mock.finish().await;
    }
    for (status, kind) in [
        ("FAILED", ForgeErrorKind::BuildFailed),
        ("ABORTED", ForgeErrorKind::BuildFailed),
        ("TIMED-OUT", ForgeErrorKind::BuildFailed),
        ("COMPLETE", ForgeErrorKind::UnknownBuildStatus),
    ] {
        let mock = Mock::serve(vec![Reply::json(build(status))]).await;
        assert_eq!(
            mock.client()
                .resume_build(&definition(), &actor, &ticket)
                .await
                .unwrap_err()
                .kind,
            kind
        );
        mock.finish().await;
    }
}

#[tokio::test]
async fn create_and_build_submission_errors_are_ambiguous_with_safe_recovery_ids() {
    let mut reply = Reply::json(json!({"error":TOKEN}));
    reply.status = 503;
    let mock = Mock::serve(vec![reply]).await;
    let error = mock.client().create_actor(&definition()).await.unwrap_err();
    assert_eq!(error.kind, ForgeErrorKind::HttpStatus(503));
    assert!(error.submission_may_have_occurred);
    assert!(!format!("{error:?}").contains(TOKEN));
    assert_eq!(mock.finish().await.len(), 1);
    let (actor, _) = tickets().await;
    let mut reply = Reply::json(json!({"error":TOKEN}));
    reply.status = 503;
    let mock = Mock::serve(vec![Reply::json(json!({"data":version()})), reply]).await;
    let error = mock
        .client()
        .start_build(&definition(), &actor)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ForgeErrorKind::HttpStatus(503));
    assert_eq!(error.actor_id.as_deref(), Some("actor1"));
    assert!(error.submission_may_have_occurred);
    assert_eq!(mock.finish().await.len(), 2);
}

#[tokio::test]
async fn echoed_credentials_never_become_actor_or_build_recovery_ids() {
    let mut body = actor();
    body["data"]["id"] = json!(TOKEN);
    let mock = Mock::serve(vec![Reply::json(body)]).await;
    let error = mock.client().create_actor(&definition()).await.unwrap_err();
    assert_eq!(error.actor_id, None);
    assert!(!format!("{error:?}").contains(TOKEN));
    mock.finish().await;
    let (actor, _) = tickets().await;
    let mut body = build("RUNNING");
    body["data"]["id"] = json!(format!("prefix-{TOKEN}"));
    let mock = Mock::serve(vec![
        Reply::json(json!({"data":version()})),
        Reply::json(body),
    ])
    .await;
    let error = mock
        .client()
        .start_build(&definition(), &actor)
        .await
        .unwrap_err();
    assert_eq!(error.build_id, None);
    assert!(!format!("{error:?}").contains(TOKEN));
    mock.finish().await;
}

#[tokio::test]
async fn duplicate_keys_and_oversized_responses_are_rejected() {
    let reply = Reply {
        body: r#"{"data":{"id":"evil","id":"actor1"}}"#.into(),
        ..Reply::json(json!({}))
    };
    let mock = Mock::serve(vec![reply]).await;
    assert_eq!(
        mock.client()
            .create_actor(&definition())
            .await
            .unwrap_err()
            .kind,
        ForgeErrorKind::MalformedResponse
    );
    mock.finish().await;
    let mock = Mock::serve(vec![Reply::json(json!({"data":"x".repeat(4096)}))]).await;
    let mut config = limits();
    config.max_response_bytes = 1024;
    let client = ApifyClient::for_local_testing(TOKEN, config, &mock.base).unwrap();
    assert_eq!(
        client.create_actor(&definition()).await.unwrap_err().kind,
        ForgeErrorKind::ResponseTooLarge
    );
    mock.finish().await;
}

#[tokio::test]
async fn deadlines_preserve_ambiguity_and_known_build_can_resume_later() {
    let mut reply = Reply::json(actor());
    reply.delay = Duration::from_millis(500);
    let mock = Mock::serve(vec![reply]).await;
    let mut config = limits();
    config.deadline = Duration::from_millis(200);
    let client = ApifyClient::for_local_testing(TOKEN, config, &mock.base).unwrap();
    let error = client.create_actor(&definition()).await.unwrap_err();
    assert_eq!(error.kind, ForgeErrorKind::DeadlineExceeded);
    assert!(error.submission_may_have_occurred);
    assert_eq!(mock.finish().await.len(), 1);
    let (actor, ticket) = tickets().await;
    let mock = Mock::serve(vec![Reply::json(build("RUNNING"))]).await;
    let mut config = limits();
    config.deadline = Duration::from_millis(200);
    config.poll_interval = Duration::from_millis(500);
    let client = ApifyClient::for_local_testing(TOKEN, config, &mock.base).unwrap();
    let error = client
        .resume_build(&definition(), &actor, &ticket)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ForgeErrorKind::DeadlineExceeded);
    assert_eq!(error.build_id.as_deref(), Some("build1"));
    mock.finish().await;
    let mock = Mock::serve(vec![Reply::json(build("SUCCEEDED"))]).await;
    mock.client()
        .resume_build(&definition(), &actor, &ticket)
        .await
        .unwrap();
    assert_eq!(mock.finish().await.len(), 1);
}

#[test]
fn local_definition_preflight_is_bounded_and_uses_the_ticket_fingerprint() {
    let original = definition();
    original.validate().unwrap();
    let digest = original.definition_sha256().unwrap();
    let mut reordered = original.clone();
    reordered.source_files.reverse();
    assert_eq!(reordered.definition_sha256().unwrap(), digest);
    let mut changed = original.clone();
    changed.source_files[0].content.push('!');
    assert_ne!(changed.definition_sha256().unwrap(), digest);
    let mut invalid = original.clone();
    invalid.source_files[0].content = "x".repeat(8 * 1024 * 1024);
    assert_eq!(
        invalid.validate().unwrap_err().kind,
        ForgeErrorKind::InvalidDefinition
    );
    let mut invalid = original;
    invalid.source_files[0].name = "../escape".into();
    assert_eq!(
        invalid.definition_sha256().unwrap_err().kind,
        ForgeErrorKind::InvalidDefinition
    );
}

#[tokio::test]
async fn tampered_tickets_fail_before_network() {
    let (actor, ticket) = tickets().await;
    let client = ApifyClient::for_local_testing(TOKEN, limits(), "http://127.0.0.1:1").unwrap();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("actor_id", json!("../escape")),
        ("version_number", json!("0.2")),
        ("definition_sha256", json!("a".repeat(64))),
        ("actor_id", json!(TOKEN)),
    ] {
        let mut saved = serde_json::to_value(&actor).unwrap();
        saved[field] = value;
        let invalid: ActorTicket = serde_json::from_value(saved).unwrap();
        let error = client
            .start_build(&definition(), &invalid)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ForgeErrorKind::TicketMismatch);
        assert!(!format!("{error:?}").contains(TOKEN));
    }
    for (field, value) in [
        ("schema_version", json!(2)),
        ("actor_id", json!("other")),
        ("build_id", json!("../escape")),
        ("build_number", json!("latest")),
        ("version_number", json!("0.2")),
        ("definition_sha256", json!("a".repeat(64))),
        ("build_id", json!(TOKEN)),
    ] {
        let mut saved = serde_json::to_value(&ticket).unwrap();
        saved[field] = value;
        let invalid: BuildTicket = serde_json::from_value(saved).unwrap();
        let error = client
            .resume_build(&definition(), &actor, &invalid)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ForgeErrorKind::TicketMismatch);
        assert!(!format!("{error:?}").contains(TOKEN));
    }
}

#[tokio::test]
async fn public_ticket_preflight_rejects_schema_ids_and_foreign_build_version() {
    let (actor, ticket) = tickets().await;
    actor.validate_for(&definition()).unwrap();
    ticket.validate_for(&definition(), &actor).unwrap();
    for (field, value) in [
        ("schema_version", json!(999)),
        ("actor_id", json!("")),
        ("actor_id", json!("../outside")),
        ("version_number", json!("0.2")),
        ("definition_sha256", json!("a".repeat(64))),
    ] {
        let mut saved = serde_json::to_value(&actor).unwrap();
        saved[field] = value;
        let invalid: ActorTicket = serde_json::from_value(saved).unwrap();
        assert_eq!(
            invalid.validate_for(&definition()).unwrap_err().kind,
            ForgeErrorKind::TicketMismatch
        );
        assert_eq!(
            ticket
                .validate_for(&definition(), &invalid)
                .unwrap_err()
                .kind,
            ForgeErrorKind::TicketMismatch
        );
    }
    for (field, value) in [
        ("schema_version", json!(999)),
        ("actor_id", json!("other")),
        ("build_id", json!("")),
        ("build_id", json!("../outside")),
        ("build_number", json!("0.2.1")),
        ("build_number", json!("0.1.0")),
        ("build_number", json!("0.1.100000")),
        ("version_number", json!("0.2")),
        ("definition_sha256", json!("a".repeat(64))),
    ] {
        let mut saved = serde_json::to_value(&ticket).unwrap();
        saved[field] = value;
        let invalid: BuildTicket = serde_json::from_value(saved).unwrap();
        assert_eq!(
            invalid
                .validate_for(&definition(), &actor)
                .unwrap_err()
                .kind,
            ForgeErrorKind::TicketMismatch
        );
    }
    let mut changed = definition();
    changed.source_files[0].content.push('!');
    assert_eq!(
        actor.validate_for(&changed).unwrap_err().kind,
        ForgeErrorKind::TicketMismatch
    );
}
