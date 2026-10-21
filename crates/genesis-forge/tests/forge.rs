//! Independent state-machine acceptance tests. Cloud responses are simulated;
//! candidate extraction outputs and oracle answers are separate checked-in data.
use genesis_apify::{ApifyClient, RunLimits, RunRequest, SourceFile};
use genesis_forge::{
    ActionKind, CandidateSpec, ForgeSession, ForgeStateError, Phase, SourcePackage,
};
use genesis_verification::{
    decode_json, json_sha256, verify, Decision, ExecutionEvidence, OracleSuite,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "synthetic-forge-state-test-token";
const ORACLE_CANARY: &str = "private-oracle-scope-never-upload";

struct Reply {
    status: u16,
    body: String,
}
impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            body: value.to_string(),
        }
    }
    fn unavailable() -> Self {
        Self {
            status: 503,
            body: "{}".into(),
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
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() < 4 * 1024 * 1024);
                    if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + content_length {
                            break;
                        }
                    }
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.status, reply.body.len(), reply.body);
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    fn client(&self) -> ApifyClient {
        let limits = RunLimits {
            request_timeout: Duration::from_secs(1),
            deadline: Duration::from_secs(2),
            poll_interval: Duration::from_millis(1),
            ..RunLimits::default()
        };
        ApifyClient::for_local_testing(TOKEN, limits, &self.base).unwrap()
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

fn file(name: &str, content: &str) -> SourceFile {
    SourceFile {
        name: name.into(),
        content: content.into(),
    }
}
fn spec() -> CandidateSpec {
    CandidateSpec {
        operation_id: "forge-op-1".into(),
        capability_id: "venue-extraction".into(),
        capability_version: "1".into(),
        actor_name: "genesis-venue-1".into(),
        source_version: "0.1".into(),
        source: SourcePackage::new(vec![
            file("src/extract.ts", "export const extract = () => [];\n"),
            file("package-lock.json", "{\"lockfileVersion\":3}\n"),
            file("Dockerfile", "FROM node:22\n"),
        ])
        .unwrap(),
    }
}
fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
fn outputs() -> Vec<Value> {
    let runs: Vec<ExecutionEvidence> = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap();
    runs.into_iter().map(|run| run.output).collect()
}
fn snapshot(spec: &CandidateSpec) -> Value {
    json!({"versionNumber":spec.source_version,"sourceType":"SOURCE_FILES","envVars":[],"applyEnvVarsToBuild":false,
        "sourceFiles":spec.source.files().iter().map(|f|json!({"name":f.name,"content":f.content,"format":"TEXT"})).collect::<Vec<_>>()})
}
fn actor(spec: &CandidateSpec) -> Reply {
    Reply::json(
        json!({"data":{"id":"actor-1","name":spec.actor_name,"isPublic":false,
        "actorPermissionLevel":"LIMITED_PERMISSIONS","versions":[snapshot(spec)]}}),
    )
}
fn build(spec: &CandidateSpec, status: &str) -> Reply {
    Reply::json(
        json!({"data":{"id":"build-1","actId":"actor-1","buildNumber":"0.1.1","status":status,"actVersion":snapshot(spec)}}),
    )
}
fn run(index: usize, status: &str) -> Reply {
    Reply::json(
        json!({"data":{"id":format!("run-{index}"),"actId":"actor-1","buildId":"build-1",
        "status":status,"defaultDatasetId":format!("dataset-{index}")}}),
    )
}
fn pipeline(spec: &CandidateSpec, outputs: Vec<Value>) -> Vec<Reply> {
    let mut replies = vec![
        actor(spec),
        Reply::json(json!({"data":snapshot(spec)})),
        build(spec, "RUNNING"),
        build(spec, "SUCCEEDED"),
    ];
    for (index, output) in outputs.into_iter().enumerate() {
        replies.extend([
            build(spec, "SUCCEEDED"),
            run(index, "SUCCEEDED"),
            build(spec, "SUCCEEDED"),
            run(index, "SUCCEEDED"),
            Reply::json(json!([output])),
        ]);
    }
    replies
}
fn restore(session: &ForgeSession) -> ForgeSession {
    decode_json(&serde_json::to_vec(session).unwrap()).unwrap()
}
async fn advance(
    session: &mut ForgeSession,
    suite: &OracleSuite,
    client: &ApifyClient,
) -> ActionKind {
    let action = session.prepare_next(suite).unwrap().unwrap();
    let kind = action.kind();
    // Exercise the required pre-operation durable boundary on every action:
    // complete against a checkpoint deserialized after prepare, not the original.
    *session = restore(session);
    let completion = action.execute(client).await;
    session.complete(completion, suite).unwrap();
    kind
}
async fn complete_pipeline(
    spec: CandidateSpec,
    suite: &OracleSuite,
    outputs: Vec<Value>,
) -> (ForgeSession, Vec<String>) {
    let mock = Mock::serve(pipeline(&spec, outputs)).await;
    let client = mock.client();
    let mut session = ForgeSession::new(spec, suite).unwrap();
    for _ in 0..3 + 2 * suite.cases.len() {
        advance(&mut session, suite, &client).await;
    }
    assert!(session.prepare_next(suite).unwrap().is_none());
    (session, mock.finish().await)
}

#[test]
fn source_digest_covers_exact_bytes_names_and_all_scaffold_files() {
    let first = SourcePackage::new(vec![
        file("src/extract.ts", "abc"),
        file("package-lock.json", "{}"),
    ])
    .unwrap();
    let reordered = SourcePackage::new(vec![
        file("package-lock.json", "{}"),
        file("src/extract.ts", "abc"),
    ])
    .unwrap();
    assert_eq!(first.sha256().unwrap(), reordered.sha256().unwrap());
    let manifest = first.manifest().unwrap();
    let parser = manifest
        .files
        .iter()
        .find(|f| f.path == "src/extract.ts")
        .unwrap();
    assert_eq!(parser.byte_length, 3);
    assert_eq!(
        parser.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    for files in [
        vec![
            file("src/extract.ts", "abd"),
            file("package-lock.json", "{}"),
        ],
        vec![
            file("src/renamed.ts", "abc"),
            file("package-lock.json", "{}"),
        ],
        vec![
            file("src/extract.ts", "abc"),
            file("package-lock.json", "{\"changed\":true}"),
        ],
        vec![file("src/extract.ts", "abc")],
        vec![
            file("src/extract.ts", "abc\n"),
            file("package-lock.json", "{}"),
        ],
        vec![
            file("src/extract.ts", "abc\r\n"),
            file("package-lock.json", "{}"),
        ],
    ] {
        assert_ne!(
            first.sha256().unwrap(),
            SourcePackage::new(files).unwrap().sha256().unwrap()
        );
    }
    let utf8 = SourcePackage::new(vec![file("unicode.txt", "é")]).unwrap();
    assert_eq!(utf8.manifest().unwrap().files[0].byte_length, 2);
}

#[test]
fn source_paths_cannot_alias_escape_or_conflict() {
    for path in [
        "",
        "/absolute.ts",
        "../outside.ts",
        "src/../main.ts",
        "src//main.ts",
        "./main.ts",
        "src\\main.ts",
        "C:/main.ts",
        "src/main.ts/",
    ] {
        assert!(
            SourcePackage::new(vec![file(path, "x")]).is_err(),
            "accepted {path}"
        );
    }
    assert!(SourcePackage::new(vec![file("same.ts", "a"), file("same.ts", "b")]).is_err());
    assert!(
        SourcePackage::new(vec![file("src", "a"), file("src/main.ts", "b")]).is_err(),
        "file/directory collision accepted"
    );
    assert!(SourcePackage::new(vec![]).is_err());
}

#[test]
fn prepared_checkpoint_contains_no_oracle_answers_and_cannot_be_marked_verified() {
    let mut oracle = suite();
    oracle.scope = ORACLE_CANARY.into();
    let session = ForgeSession::new(spec(), &oracle).unwrap();
    let serialized = serde_json::to_string(&session).unwrap();
    assert!(!serialized.contains(ORACLE_CANARY));
    assert!(!serialized.contains("grand-hall"));
    assert!(session.verified(&oracle).is_err());
    let mut forged = serde_json::to_value(session).unwrap();
    forged["phase"] = json!("verified");
    let restored: ForgeSession = serde_json::from_value(forged).unwrap();
    assert_eq!(
        restored.validate(&oracle).unwrap_err(),
        ForgeStateError::CorruptCheckpoint
    );
    assert!(restored.verified(&oracle).is_err());
}

#[test]
fn crash_after_preparing_post_never_reoffers_post() {
    let oracle = suite();
    let mut session = ForgeSession::new(spec(), &oracle).unwrap();
    let action = session.prepare_next(&oracle).unwrap().unwrap();
    assert_eq!(action.kind(), ActionKind::CreateActor);
    assert!(action.sequence() > 0);
    drop(action); // The external outcome is unknowable after restart.
    let mut restored = restore(&session);
    assert!(matches!(
        restored.prepare_next(&oracle),
        Err(ForgeStateError::NeedsReconciliation)
    ));
    assert!(restored.artifact().is_none());
}

#[test]
fn altered_source_candidate_or_suite_invalidates_restored_work() {
    let oracle = suite();
    let session = ForgeSession::new(spec(), &oracle).unwrap();
    for pointer in [
        "/spec/operation_id",
        "/spec/capability_version",
        "/spec/source/files/0/content",
        "/spec/actor_name",
    ] {
        let mut altered = serde_json::to_value(&session).unwrap();
        *altered.pointer_mut(pointer).unwrap() = json!("changed");
        let restored: ForgeSession = serde_json::from_value(altered).unwrap();
        assert!(
            restored.validate(&oracle).is_err(),
            "accepted stale fingerprint for {pointer}"
        );
    }
    let mut changed_oracle = oracle;
    changed_oracle.cases[0].input["attendees"] = json!(121);
    assert_eq!(
        session.validate(&changed_oracle).unwrap_err(),
        ForgeStateError::SuiteChanged
    );
}

#[tokio::test]
async fn full_pipeline_produces_only_a_scoped_verified_candidate() {
    let mut oracle = suite();
    oracle.scope = ORACLE_CANARY.into();
    let candidate_spec = spec();
    let expected_digest = candidate_spec.source.sha256().unwrap();
    let (session, requests) = complete_pipeline(candidate_spec, &oracle, outputs()).await;
    assert_eq!(session.phase(), Phase::Verified);
    assert_eq!(session.completed_cases(), oracle.cases.len());
    let candidate = restore(&session).verified(&oracle).unwrap();
    assert_eq!(candidate.artifact.source_sha256, expected_digest);
    assert_eq!(candidate.artifact.build_id, "build-1");
    assert_eq!(candidate.verification.decision, Decision::Accepted);
    assert_eq!(candidate.verification.artifact, candidate.artifact);
    assert_eq!(
        candidate.verification.suite_sha256,
        json_sha256(&serde_json::to_value(&oracle).unwrap())
    );
    let serialized = serde_json::to_value(candidate).unwrap();
    for key in ["grant", "authorization", "published", "installed", "safe"] {
        assert!(serialized.get(key).is_none());
    }
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST /v2/actors "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST /v2/actors/actor-1/builds?"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST /v2/actors/actor-1/runs?"))
            .count(),
        3
    );
    for request in requests {
        assert!(
            !request.contains(ORACLE_CANARY),
            "oracle scope was sent to provider"
        );
    }
}

#[tokio::test]
async fn provider_success_with_wrong_fact_rejects_whole_candidate() {
    let oracle = suite();
    let mut actual = outputs();
    actual[0]["venues"][0]["capacity"] = json!(999);
    let (session, _) = complete_pipeline(spec(), &oracle, actual).await;
    assert_eq!(session.phase(), Phase::Rejected);
    assert_eq!(
        session.report(&oracle).unwrap().decision,
        Decision::Rejected
    );
    assert_eq!(
        session.verified(&oracle).unwrap_err(),
        ForgeStateError::NotVerified
    );
}

#[tokio::test]
async fn saved_accepted_label_does_not_authorize_mutated_receipts_or_pin() {
    let oracle = suite();
    let (session, _) = complete_pipeline(spec(), &oracle, outputs()).await;
    for (pointer, value) in [
        ("/receipts/0/output/venues/0/capacity", json!(999)),
        ("/receipts/0/artifact/build_id", json!("old-build")),
        ("/pin/source_sha256", json!("f".repeat(64))),
        ("/receipts/0/input_sha256", json!("f".repeat(64))),
        ("/receipts/0/status", json!("failed")),
    ] {
        let mut altered = serde_json::to_value(&session).unwrap();
        *altered.pointer_mut(pointer).unwrap() = value;
        let restored: ForgeSession = serde_json::from_value(altered).unwrap();
        assert!(
            restored.verified(&oracle).is_err(),
            "accepted tampered saved {pointer}"
        );
    }
}

#[tokio::test]
async fn terminal_phase_must_agree_with_recomputed_verification_decision() {
    let oracle = suite();
    let (session, _) = complete_pipeline(spec(), &oracle, outputs()).await;
    let mut altered = serde_json::to_value(&session).unwrap();
    altered["phase"] = json!("rejected");
    let restored: ForgeSession = serde_json::from_value(altered).unwrap();
    assert_eq!(
        restored.validate(&oracle).unwrap_err(),
        ForgeStateError::CorruptCheckpoint,
        "a rejected label with passing receipts is an inconsistent checkpoint"
    );
    assert!(restored.report(&oracle).is_err());
}

#[tokio::test]
async fn ambiguous_actor_create_remains_reconciliation_required() {
    let oracle = suite();
    let mock = Mock::serve(vec![Reply::unavailable()]).await;
    let mut session = ForgeSession::new(spec(), &oracle).unwrap();
    advance(&mut session, &oracle, &mock.client()).await;
    assert_eq!(session.phase(), Phase::NeedsReconciliation);
    assert!(session.failure().unwrap().ambiguous);
    assert!(matches!(
        restore(&session).prepare_next(&oracle),
        Err(ForgeStateError::NeedsReconciliation)
    ));
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn ambiguous_build_post_never_repeats_build_or_actor_creation() {
    let oracle = suite();
    let spec = spec();
    let mock = Mock::serve(vec![
        actor(&spec),
        Reply::json(json!({"data":snapshot(&spec)})),
        Reply::unavailable(),
    ])
    .await;
    let client = mock.client();
    let mut session = ForgeSession::new(spec, &oracle).unwrap();
    advance(&mut session, &oracle, &client).await;
    advance(&mut session, &oracle, &client).await;
    assert_eq!(session.phase(), Phase::NeedsReconciliation);
    assert_eq!(
        session.failure().unwrap().actor_id.as_deref(),
        Some("actor-1")
    );
    assert!(matches!(
        restore(&session).prepare_next(&oracle),
        Err(ForgeStateError::NeedsReconciliation)
    ));
    assert_eq!(mock.finish().await.len(), 3);
}

#[tokio::test]
async fn source_edit_after_actor_creation_prevents_build_dispatch() {
    let oracle = suite();
    let spec = spec();
    let mock = Mock::serve(vec![actor(&spec)]).await;
    let mut session = ForgeSession::new(spec, &oracle).unwrap();
    advance(&mut session, &oracle, &mock.client()).await;
    let mut altered = serde_json::to_value(session).unwrap();
    altered["spec"]["source"]["files"][0]["content"] = json!("changed after create");
    let mut restored: ForgeSession = serde_json::from_value(altered).unwrap();
    assert!(restored.prepare_next(&oracle).is_err());
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn retryable_build_read_can_resume_without_reposting() {
    let oracle = suite();
    let spec = spec();
    let mock = Mock::serve(vec![
        actor(&spec),
        Reply::json(json!({"data":snapshot(&spec)})),
        build(&spec, "RUNNING"),
        Reply::unavailable(),
        build(&spec, "SUCCEEDED"),
    ])
    .await;
    let client = mock.client();
    let mut session = ForgeSession::new(spec, &oracle).unwrap();
    advance(&mut session, &oracle, &client).await;
    advance(&mut session, &oracle, &client).await;
    advance(&mut session, &oracle, &client).await;
    assert_eq!(session.phase(), Phase::BuildSubmitted);
    assert!(session.failure().unwrap().retryable);
    session = restore(&session);
    let action = session.prepare_next(&oracle).unwrap().unwrap();
    assert_eq!(action.kind(), ActionKind::PollBuild);
    session
        .complete(action.execute(&client).await, &oracle)
        .unwrap();
    assert_eq!(session.phase(), Phase::Testing);
    let requests = mock.finish().await;
    assert_eq!(
        requests.iter().filter(|r| r.starts_with("POST ")).count(),
        2
    );
    assert!(requests[3].starts_with("GET /v2/actor-builds/build-1 "));
    assert!(requests[4].starts_with("GET /v2/actor-builds/build-1 "));
}

#[tokio::test]
async fn terminal_snapshot_mismatch_never_produces_an_artifact_pin() {
    let oracle = suite();
    let spec = spec();
    let mut wrong = snapshot(&spec);
    wrong["sourceFiles"][0] = json!({"name":"unexpected.ts"});
    let terminal = Reply::json(
        json!({"data":{"id":"build-1","actId":"actor-1","buildNumber":"0.1.1",
        "status":"SUCCEEDED","actVersion":wrong}}),
    );
    let mock = Mock::serve(vec![
        actor(&spec),
        Reply::json(json!({"data":snapshot(&spec)})),
        build(&spec, "RUNNING"),
        terminal,
    ])
    .await;
    let client = mock.client();
    let mut session = ForgeSession::new(spec, &oracle).unwrap();
    for _ in 0..3 {
        advance(&mut session, &oracle, &client).await;
    }
    assert_eq!(session.phase(), Phase::Failed);
    assert!(session.artifact().is_none());
    assert!(session.verified(&oracle).is_err());
    assert_eq!(mock.finish().await.len(), 4);
}

#[tokio::test]
async fn completion_from_another_candidate_cannot_advance_pending_state() {
    let oracle = suite();
    let first_spec = spec();
    let mock = Mock::serve(vec![actor(&first_spec)]).await;
    let mut first = ForgeSession::new(first_spec, &oracle).unwrap();
    let action = first.prepare_next(&oracle).unwrap().unwrap();
    let completion = action.execute(&mock.client()).await;
    let mut second_spec = spec();
    second_spec.operation_id = "different-operation".into();
    let mut second = ForgeSession::new(second_spec, &oracle).unwrap();
    let _second_action = second.prepare_next(&oracle).unwrap().unwrap();
    assert_eq!(
        second.complete(completion, &oracle).unwrap_err(),
        ForgeStateError::OutcomeMismatch
    );
    assert_eq!(second.phase(), Phase::Prepared);
    assert_eq!(mock.finish().await.len(), 1);
}

#[tokio::test]
async fn one_repair_requires_new_identity_source_and_fresh_full_evidence() {
    let oracle = suite();
    let mut wrong = outputs();
    wrong[0]["venues"][0]["capacity"] = json!(999);
    let (rejected, _) = complete_pipeline(spec(), &oracle, wrong).await;
    let mut replacement = spec();
    replacement.operation_id = "repair-op".into();
    replacement.actor_name = "genesis-venue-repair".into();
    replacement.capability_version = "2".into();
    replacement.source = SourcePackage::new(vec![file(
        "src/extract.ts",
        "export const extract = () => ['repaired'];\n",
    )])
    .unwrap();
    let repaired = rejected.repair(replacement.clone(), &oracle).unwrap();
    assert_eq!(repaired.phase(), Phase::Prepared);
    assert_eq!(repaired.completed_cases(), 0);
    assert!(repaired.artifact().is_none());
    let saved = serde_json::to_value(&repaired).unwrap();
    assert_eq!(saved["repair_count"], json!(1));
    assert_eq!(saved["parent_operation_id"], json!("forge-op-1"));
    for field in ["operation_id", "actor_name", "capability_version"] {
        let mut value = serde_json::to_value(&replacement).unwrap();
        value[field] = serde_json::to_value(rejected.spec()).unwrap()[field].clone();
        let invalid: CandidateSpec = serde_json::from_value(value).unwrap();
        assert_eq!(
            rejected.repair(invalid, &oracle).unwrap_err(),
            ForgeStateError::RepairNotAllowed
        );
    }
    let mut unchanged_source = replacement.clone();
    unchanged_source.source = rejected.spec().source.clone();
    assert_eq!(
        rejected.repair(unchanged_source, &oracle).unwrap_err(),
        ForgeStateError::RepairNotAllowed
    );
    let mut wrong = outputs();
    wrong[0]["venues"][0]["capacity"] = json!(999);
    let mock = Mock::serve(pipeline(&replacement, wrong)).await;
    let client = mock.client();
    let mut repaired = repaired;
    for _ in 0..3 + 2 * oracle.cases.len() {
        advance(&mut repaired, &oracle, &client).await;
    }
    assert_eq!(repaired.phase(), Phase::Rejected);
    replacement.operation_id = "forbidden-third-op".into();
    replacement.actor_name = "genesis-venue-third".into();
    replacement.capability_version = "3".into();
    replacement.source = SourcePackage::new(vec![file("src/extract.ts", "third source")]).unwrap();
    assert_eq!(
        repaired.repair(replacement, &oracle).unwrap_err(),
        ForgeStateError::RepairNotAllowed
    );
    mock.finish().await;
}

#[tokio::test]
async fn ambiguous_case_submission_cannot_advance_or_repeat_that_case() {
    let oracle = suite();
    let spec = spec();
    let mock = Mock::serve(vec![
        actor(&spec),
        Reply::json(json!({"data":snapshot(&spec)})),
        build(&spec, "RUNNING"),
        build(&spec, "SUCCEEDED"),
        build(&spec, "SUCCEEDED"),
        Reply::unavailable(),
    ])
    .await;
    let client = mock.client();
    let mut session = ForgeSession::new(spec, &oracle).unwrap();
    for _ in 0..4 {
        advance(&mut session, &oracle, &client).await;
    }
    assert_eq!(session.phase(), Phase::NeedsReconciliation);
    assert_eq!(session.completed_cases(), 0);
    assert!(matches!(
        restore(&session).prepare_next(&oracle),
        Err(ForgeStateError::NeedsReconciliation)
    ));
    assert_eq!(mock.finish().await.len(), 6);
}

#[tokio::test]
async fn pending_read_after_restart_collects_the_same_case_without_a_new_run() {
    let oracle = suite();
    let spec = spec();
    let mut replies = vec![
        actor(&spec),
        Reply::json(json!({"data":snapshot(&spec)})),
        build(&spec, "RUNNING"),
        build(&spec, "SUCCEEDED"),
        build(&spec, "SUCCEEDED"),
        run(0, "SUCCEEDED"),
        Reply::unavailable(),
    ];
    replies.extend([
        build(&spec, "SUCCEEDED"),
        run(0, "SUCCEEDED"),
        Reply::json(json!([outputs().remove(0)])),
    ]);
    let mock = Mock::serve(replies).await;
    let client = mock.client();
    let mut session = ForgeSession::new(spec, &oracle).unwrap();
    for _ in 0..4 {
        advance(&mut session, &oracle, &client).await;
    }
    let abandoned_read = session.prepare_next(&oracle).unwrap().unwrap();
    assert_eq!(abandoned_read.kind(), ActionKind::CollectCase);
    let sequence = abandoned_read.sequence();
    drop(abandoned_read);
    session = restore(&session);
    let resumed_read = session.prepare_next(&oracle).unwrap().unwrap();
    assert_eq!(resumed_read.sequence(), sequence);
    session
        .complete(resumed_read.execute(&client).await, &oracle)
        .unwrap();
    assert_eq!(session.phase(), Phase::Testing);
    assert_eq!(session.completed_cases(), 0);
    assert!(session.failure().unwrap().retryable);
    session = restore(&session);
    advance(&mut session, &oracle, &client).await;
    assert_eq!(session.completed_cases(), 1);
    assert_eq!(
        session.prepare_next(&oracle).unwrap().unwrap().kind(),
        ActionKind::SubmitCase
    );
    let requests = mock.finish().await;
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST /v2/actors/actor-1/runs?"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("GET /v2/actor-runs/run-0 "))
            .count(),
        1
    );
}

#[tokio::test]
async fn second_org_reuses_same_pin_with_new_inputs_and_no_source_or_build_posts() {
    let oracle_a = suite();
    let source_spec = spec();
    let (session_a, _) = complete_pipeline(source_spec.clone(), &oracle_a, outputs()).await;
    let approved = session_a.verified(&oracle_a).unwrap();
    let old_receipts: Vec<ExecutionEvidence> =
        serde_json::from_value(serde_json::to_value(&session_a).unwrap()["receipts"].clone())
            .unwrap();
    let mut oracle_b = oracle_a;
    oracle_b.suite_id = "organization-b-local-acceptance".into();
    for case in &mut oracle_b.cases {
        case.input["organization"] = json!("org-b");
        case.input["attendees"] = json!(125);
    }
    assert!(
        !verify(&oracle_b, &approved.artifact, &old_receipts)
            .unwrap()
            .is_accepted(),
        "A's runs cannot substitute for B's different input"
    );
    let mut replies = Vec::new();
    for (index, output) in outputs().into_iter().enumerate() {
        let metadata = json!({"data":{"id":format!("b-run-{index}"),"actId":"actor-1","buildId":"build-1",
            "status":"SUCCEEDED","defaultDatasetId":format!("b-dataset-{index}")}});
        replies.extend([
            build(&source_spec, "SUCCEEDED"),
            Reply::json(metadata),
            Reply::json(json!([output])),
        ]);
    }
    let mock = Mock::serve(replies).await;
    let client = mock.client();
    let mut observations = Vec::new();
    for case in &oracle_b.cases {
        observations.push(
            client
                .execute(&RunRequest {
                    case_id: case.case_id.clone(),
                    artifact: approved.artifact.clone(),
                    input: case.input.clone(),
                    source: case.source.clone(),
                })
                .await
                .unwrap(),
        );
    }
    let outcome = verify(&oracle_b, &approved.artifact, &observations).unwrap();
    assert!(outcome.is_accepted());
    assert_eq!(outcome.report().artifact, approved.artifact);
    assert_ne!(
        outcome.report().suite_sha256,
        approved.verification.suite_sha256
    );
    for (old, new) in old_receipts.iter().zip(&observations) {
        assert_ne!(old.input_sha256, new.input_sha256);
        assert_ne!(old.run_id, new.run_id);
    }
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 9);
    let posts: Vec<_> = requests.iter().filter(|r| r.starts_with("POST ")).collect();
    assert_eq!(posts.len(), 3);
    assert!(posts
        .iter()
        .all(|r| r.starts_with("POST /v2/actors/actor-1/runs?")));
    assert!(posts
        .iter()
        .all(|r| r.contains("\"organization\":\"org-b\"")));
}

#[test]
fn checkpoint_wire_shape_and_immutable_request_schema_are_strict() {
    let oracle = suite();
    let session = ForgeSession::new(spec(), &oracle).unwrap();
    for pointer in ["", "/spec", "/spec/source", "/spec/source/files/0"] {
        let mut value = serde_json::to_value(&session).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("approved".into(), json!(true));
        assert!(
            serde_json::from_value::<ForgeSession>(value).is_err(),
            "unknown field accepted at {pointer}"
        );
    }
    let mut value = serde_json::to_value(session).unwrap();
    value["schema_version"] = json!(999);
    let restored: ForgeSession = serde_json::from_value(value).unwrap();
    assert_eq!(
        restored.validate(&oracle).unwrap_err(),
        ForgeStateError::CorruptCheckpoint
    );
}

#[tokio::test]
async fn restored_terminal_state_rejects_each_invalid_provider_ticket_schema() {
    let oracle = suite();
    let (session, _) = complete_pipeline(spec(), &oracle, outputs()).await;
    for ticket in ["actor", "build"] {
        for schema in [0, 999] {
            let mut value = serde_json::to_value(&session).unwrap();
            value[ticket]["schema_version"] = json!(schema);
            let mut restored: ForgeSession = serde_json::from_value(value).unwrap();
            assert_eq!(
                restored.validate(&oracle).unwrap_err(),
                ForgeStateError::CorruptCheckpoint,
                "accepted {ticket} ticket schema {schema}"
            );
            assert!(restored.verified(&oracle).is_err());
            assert!(restored.prepare_next(&oracle).is_err());
        }
    }
}

#[tokio::test]
async fn mutually_consistent_build_pin_and_receipts_cannot_change_source_version() {
    let oracle = suite();
    let (session, _) = complete_pipeline(spec(), &oracle, outputs()).await;
    let mut value = serde_json::to_value(&session).unwrap();
    value["build"]["build_number"] = json!("0.2.1");
    value["pin"]["build_number"] = json!("0.2.1");
    for receipt in value["receipts"].as_array_mut().unwrap() {
        receipt["artifact"]["build_number"] = json!("0.2.1");
    }
    let restored: ForgeSession = serde_json::from_value(value).unwrap();
    assert_eq!(restored.spec().source_version, "0.1");
    assert_eq!(
        restored.validate(&oracle).unwrap_err(),
        ForgeStateError::CorruptCheckpoint
    );
    assert!(restored.report(&oracle).is_err());
    assert!(restored.verified(&oracle).is_err());
}

#[tokio::test]
async fn restored_run_ticket_must_validate_its_schema_and_exact_request_binding() {
    let oracle = suite();
    let candidate = spec();
    let replies = pipeline(&candidate, outputs())
        .into_iter()
        .take(6)
        .collect();
    let mock = Mock::serve(replies).await;
    let mut session = ForgeSession::new(candidate, &oracle).unwrap();
    for _ in 0..4 {
        advance(&mut session, &oracle, &mock.client()).await;
    }
    assert_eq!(session.phase(), Phase::Testing);
    session.validate(&oracle).unwrap();
    for (field, invalid) in [
        ("schema_version", json!(999)),
        ("request_sha256", json!("f".repeat(64))),
        ("request_sha256", json!("not-a-digest")),
    ] {
        let mut value = serde_json::to_value(&session).unwrap();
        assert!(value["run"].is_object());
        value["run"][field] = invalid;
        let mut restored: ForgeSession = serde_json::from_value(value).unwrap();
        let before = serde_json::to_vec(&restored).unwrap();
        assert_eq!(
            restored.validate(&oracle).unwrap_err(),
            ForgeStateError::CorruptCheckpoint,
            "accepted invalid run/{field}"
        );
        assert!(restored.prepare_next(&oracle).is_err());
        assert_eq!(serde_json::to_vec(&restored).unwrap(), before);
    }
    assert_eq!(mock.finish().await.len(), 6);
}

#[test]
fn source_version_is_bounded_before_parsing_components() {
    let mut candidate = spec();
    candidate.source_version = "99.99".into();
    candidate.validate().unwrap();
    for version in ["0.123".into(), "1.".repeat(512 * 1024)] {
        candidate.source_version = version;
        assert_eq!(
            candidate.validate().unwrap_err(),
            ForgeStateError::InvalidCandidate
        );
        assert!(ForgeSession::new(candidate.clone(), &suite()).is_err());
    }
}

fn context_replies(candidate: &CandidateSpec, actor_id: &str, build_id: &str) -> Vec<Reply> {
    let mut actor_response: Value = serde_json::from_str(&actor(candidate).body).unwrap();
    actor_response["data"]["id"] = json!(actor_id);
    let build_response = |status: &str| {
        let mut value: Value = serde_json::from_str(&build(candidate, status).body).unwrap();
        value["data"]["id"] = json!(build_id);
        value["data"]["actId"] = json!(actor_id);
        Reply::json(value)
    };
    vec![
        Reply::json(actor_response),
        Reply::json(json!({"data":snapshot(candidate)})),
        build_response("RUNNING"),
        build_response("SUCCEEDED"),
    ]
}

async fn reject_cross_provider_completions(
    first_actor: &str,
    second_actor: &str,
    completed_actions: usize,
    failed_reads: bool,
) {
    let oracle = suite();
    let candidate = spec();
    let make_replies = |actor_id, build_id| {
        let mut replies = context_replies(&candidate, actor_id, build_id);
        if completed_actions == 1 {
            replies.truncate(3);
        } else if failed_reads {
            replies[3] = Reply::unavailable();
        }
        replies
    };
    let first_mock = Mock::serve(make_replies(first_actor, "build-first")).await;
    let second_mock = Mock::serve(make_replies(second_actor, "build-second")).await;
    let mut first = ForgeSession::new(candidate.clone(), &oracle).unwrap();
    let mut second = ForgeSession::new(candidate, &oracle).unwrap();
    for _ in 0..completed_actions {
        advance(&mut first, &oracle, &first_mock.client()).await;
        advance(&mut second, &oracle, &second_mock.client()).await;
    }
    let first_action = first.prepare_next(&oracle).unwrap().unwrap();
    let second_action = second.prepare_next(&oracle).unwrap().unwrap();
    assert_eq!(first_action.kind(), second_action.kind());
    assert_eq!(first_action.sequence(), second_action.sequence());
    let first_before = serde_json::to_vec(&first).unwrap();
    let second_before = serde_json::to_vec(&second).unwrap();
    let first_completion = first_action.execute(&first_mock.client()).await;
    let second_completion = second_action.execute(&second_mock.client()).await;
    assert_eq!(
        second.complete(first_completion, &oracle).unwrap_err(),
        ForgeStateError::OutcomeMismatch
    );
    assert_eq!(serde_json::to_vec(&second).unwrap(), second_before);
    assert_eq!(
        first.complete(second_completion, &oracle).unwrap_err(),
        ForgeStateError::OutcomeMismatch
    );
    assert_eq!(serde_json::to_vec(&first).unwrap(), first_before);
    first.validate(&oracle).unwrap();
    second.validate(&oracle).unwrap();
    first_mock.finish().await;
    second_mock.finish().await;
}

#[tokio::test]
async fn same_candidate_and_sequence_cannot_swap_different_actor_completions() {
    reject_cross_provider_completions("actor-first", "actor-second", 1, false).await;
}

#[tokio::test]
async fn same_actor_candidate_and_sequence_cannot_swap_different_build_completions() {
    reject_cross_provider_completions("same-actor", "same-actor", 2, false).await;
}

#[tokio::test]
async fn a_failed_read_from_another_build_cannot_change_the_pending_checkpoint() {
    reject_cross_provider_completions("same-actor", "same-actor", 2, true).await;
}

#[tokio::test]
async fn same_candidate_and_sequence_cannot_swap_different_suite_completions() {
    for change_input in [false, true] {
        let oracle_first = suite();
        let mut oracle_second = suite();
        if change_input {
            // Same suite ID, different actual input still requires a different context.
            oracle_second.cases[0].input["attendees"] = json!(121);
        } else {
            oracle_second.suite_id = "another-independent-organization".into();
        }
        let candidate = spec();
        let first_mock = Mock::serve(vec![actor(&candidate)]).await;
        let second_mock = Mock::serve(vec![actor(&candidate)]).await;
        let mut first = ForgeSession::new(candidate.clone(), &oracle_first).unwrap();
        let mut second = ForgeSession::new(candidate, &oracle_second).unwrap();
        let first_action = first.prepare_next(&oracle_first).unwrap().unwrap();
        let second_action = second.prepare_next(&oracle_second).unwrap().unwrap();
        assert_eq!(first_action.sequence(), second_action.sequence());
        assert_eq!(first_action.kind(), ActionKind::CreateActor);
        let first_before = serde_json::to_vec(&first).unwrap();
        let second_before = serde_json::to_vec(&second).unwrap();
        let first_completion = first_action.execute(&first_mock.client()).await;
        let second_completion = second_action.execute(&second_mock.client()).await;
        assert_eq!(
            second
                .complete(first_completion, &oracle_second)
                .unwrap_err(),
            ForgeStateError::OutcomeMismatch
        );
        assert_eq!(serde_json::to_vec(&second).unwrap(), second_before);
        assert_eq!(
            first
                .complete(second_completion, &oracle_first)
                .unwrap_err(),
            ForgeStateError::OutcomeMismatch
        );
        assert_eq!(serde_json::to_vec(&first).unwrap(), first_before);
        first.validate(&oracle_first).unwrap();
        second.validate(&oracle_second).unwrap();
        first_mock.finish().await;
        second_mock.finish().await;
    }
}
