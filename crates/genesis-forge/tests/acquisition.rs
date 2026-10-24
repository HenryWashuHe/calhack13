//! Acquisition acceptance tests exercise the real HTTP adapters against local
//! provider fixtures. Cloud execution is simulated; generated source is never
//! executed on the host, and the independent oracle is never a model input.
use genesis_apify::{ApifyClient, RunLimits};
use genesis_forge::synthesis::{ModelConfig, SynthesisPrompt};
use genesis_forge::synthesis_http::AnthropicGenerator;
use genesis_forge::{
    generated_candidate, AcquisitionPhase, AcquisitionPlan, AcquisitionSession, ActionKind,
    CandidateSpec, ForgeSession, Phase, SourcePackage,
};
use genesis_verification::{decode_json, json_sha256, Decision, ExecutionEvidence, OracleSuite};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const MODEL: &str = "claude-sonnet-4-6";
const TOKEN: &str = "local-acquisition-provider-fixture";
const PRIVATE_ORACLE: &str = "ACQUISITION_PRIVATE_ORACLE_SCOPE_71c3";
const PUBLIC_HTML: &str = "<html>PUBLIC_ACQUISITION_EXAMPLE_3d9a</html>";
const INITIAL_PARSER: &str = "export function extract() { throw new Error('untrusted initial parser — never execute on host'); }\r\n";
const REPAIRED_PARSER: &str = "export function extract() { throw new Error('untrusted repaired parser — never execute on host'); }\r\n";

struct Reply {
    status: u16,
    body: Value,
}
impl Reply {
    fn json(body: Value) -> Self {
        Self { status: 200, body }
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
                    let mut buffer = [0; 4096];
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(read, 0, "provider request ended before its body");
                    bytes.extend_from_slice(&buffer[..read]);
                    assert!(bytes.len() < 4 * 1024 * 1024);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
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
                let body = reply.body.to_string();
                let response = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status, body.len(), body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }

    fn client(&self) -> ApifyClient {
        ApifyClient::for_local_testing(
            TOKEN,
            RunLimits {
                request_timeout: Duration::from_secs(1),
                deadline: Duration::from_secs(2),
                poll_interval: Duration::from_millis(1),
                ..RunLimits::default()
            },
            &self.base,
        )
        .unwrap()
    }

    async fn finish(self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.scope = PRIVATE_ORACLE.into();
    suite
}

fn outputs() -> Vec<Value> {
    let runs: Vec<ExecutionEvidence> = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap();
    runs.into_iter().map(|run| run.output).collect()
}

fn wrong_outputs() -> Vec<Value> {
    let mut values = outputs();
    values[0]["venues"][0]["capacity"] = json!(999);
    values
}

fn plan(suite: &OracleSuite, repair: bool) -> AcquisitionPlan {
    let urls = suite
        .cases
        .iter()
        .map(|case| case.source.url.clone())
        .collect::<Vec<_>>();
    serde_json::from_value(json!({
        "request": {
            "operation_id": "acquire-initial",
            "candidate": {
                "capability_id": "venue-extraction", "capability_version": "1.0.0",
                "actor_name": "genesis-acquire-initial", "source_version": "0.1"
            },
            "requirement": "Extract venue listings from the supplied public HTML.",
            "allowed_source_urls": urls,
            "examples": [{"url": urls[0], "html": PUBLIC_HTML}]
        },
        "model": {"model": MODEL, "max_tokens": 1024},
        "repair": repair.then(|| json!({
            "operation_id": "acquire-repair",
            "candidate": {
                "capability_id": "venue-extraction", "capability_version": "1.0.1",
                "actor_name": "genesis-acquire-repair", "source_version": "0.2"
            }
        }))
    }))
    .unwrap()
}

fn restore(session: &AcquisitionSession, suite: &OracleSuite) -> AcquisitionSession {
    let restored: AcquisitionSession = decode_json(&serde_json::to_vec(session).unwrap()).unwrap();
    restored.validate(suite).unwrap();
    restored
}

fn advance_local(session: &mut AcquisitionSession, suite: &OracleSuite) {
    for _ in 0..4 {
        if !session.advance_local(suite).unwrap() {
            return;
        }
        *session = restore(session, suite);
    }
    panic!("local acquisition transitions did not stabilize");
}

fn request_body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn user_prompt(request: &str) -> Value {
    let body = request_body(request);
    serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap()
}

async fn generation(
    session: &mut AcquisitionSession,
    suite: &OracleSuite,
    parser: &str,
    message_id: &str,
) -> String {
    let mock = Mock::serve(vec![Reply::json(json!({
        "id": message_id, "type": "message", "role": "assistant", "model": MODEL,
        "content": [{"type": "text", "text": json!({"extractor_typescript": parser}).to_string()}],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": {"input_tokens": 200, "output_tokens": 80}
    }))])
    .await;
    let generator =
        AnthropicGenerator::with_test_endpoint(&format!("{}/v1/messages", mock.base), TOKEN)
            .unwrap();
    let config: ModelConfig = session.generation().config().clone();
    let prompt: SynthesisPrompt = session.begin_generation(suite).unwrap();
    *session = restore(session, suite);
    let receipt = generator.generate(&prompt, &config).await;
    session.complete_generation(receipt, suite).unwrap();
    *session = restore(session, suite);
    assert_eq!(session.generation().extractor().unwrap(), parser);
    let mut requests = mock.finish().await;
    assert_eq!(requests.len(), 1);
    let request = requests.remove(0);
    assert!(!request.contains(PRIVATE_ORACLE));
    assert!(request.contains("PUBLIC_ACQUISITION_EXAMPLE_3d9a"));
    request
}

async fn prepared(repair: bool) -> (AcquisitionSession, OracleSuite, String) {
    let suite = suite();
    let mut session = AcquisitionSession::new(plan(&suite, repair), &suite).unwrap();
    let request = generation(&mut session, &suite, INITIAL_PARSER, "msg_initial").await;
    advance_local(&mut session, &suite);
    assert_eq!(session.phase(), AcquisitionPhase::Forging);
    assert_current_packaging(session.forge().unwrap().spec());
    (session, suite, request)
}

fn assert_current_packaging(spec: &CandidateSpec) {
    let contents = |name: &str| {
        &spec
            .source
            .files()
            .iter()
            .find(|file| file.name == name)
            .unwrap()
            .content
    };
    let config: Value = serde_json::from_str(contents(".actor/actor.json")).unwrap();
    assert_eq!(config["name"], spec.actor_name);
    assert_eq!(config["version"], spec.source_version);
    let provenance: Value = serde_json::from_str(contents("genesis-synthesis.json")).unwrap();
    assert_eq!(provenance["format"], "genesis-synthesis-v2");
    let scaffold = SourcePackage::new(
        spec.source
            .files()
            .iter()
            .filter(|file| file.name != "genesis-synthesis.json")
            .cloned()
            .collect(),
    )
    .unwrap();
    assert_eq!(
        provenance["scaffold_source_sha256"],
        scaffold.sha256().unwrap()
    );
}

fn snapshot(spec: &CandidateSpec) -> Value {
    json!({"versionNumber": spec.source_version, "sourceType": "SOURCE_FILES", "envVars": [], "applyEnvVarsToBuild": false,
        "sourceFiles": spec.source.files().iter().map(|file| json!({"name": file.name, "content": file.content, "format": "TEXT"})).collect::<Vec<_>>()})
}

fn actor(spec: &CandidateSpec, attempt: &str) -> Reply {
    Reply::json(
        json!({"data": {"id": format!("actor-{attempt}"), "name": spec.actor_name, "isPublic": false,
        "actorPermissionLevel": "LIMITED_PERMISSIONS", "versions": [snapshot(spec)]}}),
    )
}

fn build(spec: &CandidateSpec, attempt: &str, status: &str) -> Reply {
    Reply::json(
        json!({"data": {"id": format!("build-{attempt}"), "actId": format!("actor-{attempt}"),
        "buildNumber": format!("{}.1", spec.source_version), "status": status, "actVersion": snapshot(spec)}}),
    )
}

fn run(attempt: &str, index: usize) -> Reply {
    Reply::json(
        json!({"data": {"id": format!("run-{attempt}-{index}"), "actId": format!("actor-{attempt}"),
        "buildId": format!("build-{attempt}"), "status": "SUCCEEDED", "defaultDatasetId": format!("dataset-{attempt}-{index}")}}),
    )
}

fn pipeline(spec: &CandidateSpec, attempt: &str, outputs: Vec<Value>) -> Vec<Reply> {
    let mut replies = vec![
        actor(spec, attempt),
        Reply::json(json!({"data": snapshot(spec)})),
        build(spec, attempt, "RUNNING"),
        build(spec, attempt, "SUCCEEDED"),
    ];
    for (index, output) in outputs.into_iter().enumerate() {
        replies.extend([
            build(spec, attempt, "SUCCEEDED"),
            run(attempt, index),
            build(spec, attempt, "SUCCEEDED"),
            run(attempt, index),
            Reply::json(json!([output])),
        ]);
    }
    replies
}

async fn forge_step(
    session: &mut AcquisitionSession,
    suite: &OracleSuite,
    client: &ApifyClient,
) -> ActionKind {
    let action = session.prepare_forge(suite).unwrap().unwrap();
    let kind = action.kind();
    *session = restore(session, suite);
    let completion = action.execute(client).await;
    session.complete_forge(completion, suite).unwrap();
    *session = restore(session, suite);
    kind
}

async fn finish_forge(
    session: &mut AcquisitionSession,
    suite: &OracleSuite,
    attempt: &str,
    outputs: Vec<Value>,
) -> Vec<String> {
    let mock = Mock::serve(pipeline(session.forge().unwrap().spec(), attempt, outputs)).await;
    let client = mock.client();
    for _ in 0..3 + 2 * suite.cases.len() {
        forge_step(session, suite, &client).await;
    }
    mock.finish().await
}

fn assert_fresh_pipeline(requests: &[String], attempt: &str, total_cases: usize) {
    assert_eq!(requests.len(), 4 + 5 * total_cases);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST /v2/actors "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(
                |request| request.starts_with(&format!("POST /v2/actors/actor-{attempt}/builds?"))
            )
            .count(),
        1
    );
    assert_eq!(requests.iter().filter(|request| request.starts_with(&format!("POST /v2/actors/actor-{attempt}/runs?"))).count(), total_cases);
    assert!(requests
        .iter()
        .all(|request| !request.contains(PRIVATE_ORACLE)));
    assert!(
        requests.iter().all(|request| {
            !request.contains("PUBLIC_ACQUISITION_EXAMPLE_3d9a") && !request.contains("grand-hall")
        }),
        "Actor requests must not carry model examples or oracle answers"
    );
}

#[tokio::test]
async fn exact_legacy_package_restores_without_repackaging_or_replaying_pending_post() {
    let (session, suite, _) = prepared(false).await;
    let mut spec = session.forge().unwrap().spec().clone();
    let mut files = spec.source.files().to_vec();
    files
        .iter_mut()
        .find(|file| file.name == ".actor/actor.json")
        .unwrap()
        .content =
        include_str!("../../../actors/venue-extractor-template/.actor/actor.json").into();
    let scaffold = SourcePackage::new(
        files
            .iter()
            .filter(|file| file.name != "genesis-synthesis.json")
            .cloned()
            .collect(),
    )
    .unwrap();
    let provenance = files
        .iter_mut()
        .find(|file| file.name == "genesis-synthesis.json")
        .unwrap();
    let mut value: Value = serde_json::from_str(&provenance.content).unwrap();
    value["format"] = json!("genesis-synthesis-v1");
    value["scaffold_source_sha256"] = json!(scaffold.sha256().unwrap());
    provenance.content = serde_json::to_string_pretty(&value).unwrap();
    spec.source = SourcePackage::new(files).unwrap();
    let legacy_source = serde_json::to_vec(&spec.source).unwrap();
    let mut forge = ForgeSession::new(spec, &suite).unwrap();
    assert_eq!(
        forge.prepare_next(&suite).unwrap().unwrap().kind(),
        ActionKind::CreateActor
    );
    let mut wire = serde_json::to_value(&session).unwrap();
    wire["forge"] = serde_json::to_value(forge).unwrap();
    wire["checkpoint_sha256"] = json!(json_sha256(&json!([
        "genesis-acquisition-v1",
        wire["schema_version"],
        wire["plan_sha256"],
        wire["suite_sha256"],
        wire["generation"],
        wire["forge"],
        wire["original"],
        wire["unchanged_repair"],
        wire["phase"]
    ])));
    let mut restored: AcquisitionSession = serde_json::from_value(wire.clone()).unwrap();
    let before_validation = serde_json::to_vec(&restored).unwrap();
    restored.validate(&suite).unwrap();
    assert_eq!(
        serde_json::to_vec(&restore(&restored, &suite)).unwrap(),
        before_validation
    );
    assert!(restored.recover_interrupted(&suite).unwrap());
    assert_eq!(restored.phase(), AcquisitionPhase::NeedsReconciliation);
    assert!(restored.prepare_forge(&suite).is_err());
    assert_eq!(
        serde_json::to_vec(&restored.forge().unwrap().spec().source).unwrap(),
        legacy_source
    );
    let reconciled = serde_json::to_vec(&restored).unwrap();
    let mut restarted = restore(&restored, &suite);
    assert!(!restarted.recover_interrupted(&suite).unwrap());
    assert_eq!(serde_json::to_vec(&restarted).unwrap(), reconciled);

    // Old and new formats still reject a valid but foreign candidate identity,
    // even if a trusted-storage writer recomputes the outer checksum.
    for original in [
        wire["forge"].clone(),
        serde_json::to_value(session.forge().unwrap()).unwrap(),
    ] {
        let original: ForgeSession = serde_json::from_value(original).unwrap();
        for field in ["actor_name", "source_version"] {
            let mut spec = original.spec().clone();
            if field == "actor_name" {
                spec.actor_name = "substituted-actor".into();
            } else {
                spec.source_version = "0.9".into();
            }
            wire["forge"] = serde_json::to_value(ForgeSession::new(spec, &suite).unwrap()).unwrap();
            wire["checkpoint_sha256"] = json!(json_sha256(&json!([
                "genesis-acquisition-v1",
                wire["schema_version"],
                wire["plan_sha256"],
                wire["suite_sha256"],
                wire["generation"],
                wire["forge"],
                wire["original"],
                wire["unchanged_repair"],
                wire["phase"]
            ])));
            let foreign: AcquisitionSession = serde_json::from_value(wire.clone()).unwrap();
            assert!(foreign.validate(&suite).is_err(), "{field}");
        }
    }
}

#[tokio::test]
async fn provider_actor_config_normalization_is_identity_neutral_only_for_correct_packages() {
    for substitute_parser in [false, true] {
        let (mut session, suite, _) = prepared(false).await;
        let spec = session.forge().unwrap().spec();
        let mut response = actor(spec, "initial").body;
        let files = response["data"]["versions"][0]["sourceFiles"]
            .as_array_mut()
            .unwrap();
        for file in files {
            if file["name"] == ".actor/actor.json" {
                let mut config: Value =
                    serde_json::from_str(file["content"].as_str().unwrap()).unwrap();
                if config["name"] != spec.actor_name || config["version"] != spec.source_version {
                    config["name"] = json!(spec.actor_name);
                    config["version"] = json!(spec.source_version);
                    file["content"] = json!(serde_json::to_string_pretty(&config).unwrap());
                }
            }
            if substitute_parser && file["name"] == "src/extractor.ts" {
                file["content"] = json!("export const changed_by_provider = true;");
            }
        }
        let mock = Mock::serve(vec![Reply::json(response)]).await;
        forge_step(&mut session, &suite, &mock.client()).await;
        assert_eq!(
            session.phase(),
            if substitute_parser {
                AcquisitionPhase::NeedsReconciliation
            } else {
                AcquisitionPhase::Forging
            }
        );
        if substitute_parser {
            assert!(session.prepare_forge(&suite).is_err());
            assert!(session.verified(&suite).is_err());
        }
        let requests = mock.finish().await;
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /v2/actors "));
    }
}

#[tokio::test]
async fn requirement_to_verified_candidate_survives_every_serialization_boundary() {
    let (mut session, suite, model_request) = prepared(true).await;
    assert_eq!(session.attempt(), 0);
    let source_sha256 = session.forge().unwrap().spec().source.sha256().unwrap();
    let requests = finish_forge(&mut session, &suite, "initial", outputs()).await;
    advance_local(&mut session, &suite);
    assert_eq!(session.phase(), AcquisitionPhase::Verified);
    let verified = restore(&session, &suite).verified(&suite).unwrap();
    assert_eq!(verified.operation_id, "acquire-initial");
    assert_eq!(verified.artifact.actor_id, "actor-initial");
    assert_eq!(verified.artifact.build_id, "build-initial");
    assert_eq!(verified.artifact.source_sha256, source_sha256);
    assert_eq!(verified.verification.decision, Decision::Accepted);
    assert_eq!(verified.verification.cases.len(), suite.cases.len());
    assert_eq!(
        session.forge().unwrap().completed_cases(),
        suite.cases.len()
    );
    assert!(verified.parent_operation_id.is_none());
    assert_fresh_pipeline(&requests, "initial", suite.cases.len());
    assert!(user_prompt(&model_request).get("repair").is_none());
    assert!(!model_request.contains("grand-hall"));
    assert!(!model_request.contains("table-listings"));
    let saved = serde_json::to_vec(&session).unwrap();
    assert!(!session.advance_local(&suite).unwrap());
    assert_eq!(serde_json::to_vec(&session).unwrap(), saved);
    assert!(session.begin_generation(&suite).is_err());
    assert!(session.prepare_forge(&suite).unwrap().is_none());
}

#[tokio::test]
async fn semantic_rejection_repairs_once_with_coarse_feedback_and_all_fresh_evidence() {
    let (mut session, suite, _) = prepared(true).await;
    let first_source = session.forge().unwrap().spec().source.sha256().unwrap();
    let first_requests = finish_forge(&mut session, &suite, "initial", wrong_outputs()).await;
    assert_eq!(session.phase(), AcquisitionPhase::Rejected);
    assert!(session.verified(&suite).is_err());
    advance_local(&mut session, &suite);
    assert_eq!(session.phase(), AcquisitionPhase::Generating);
    assert_eq!(session.attempt(), 1);
    assert!(session.forge().is_none());
    let model_request = generation(&mut session, &suite, REPAIRED_PARSER, "msg_repair").await;
    let public = user_prompt(&model_request);
    assert_eq!(public["request"]["operation_id"], "acquire-repair");
    assert_eq!(public["request"]["examples"][0]["html"], PUBLIC_HTML);
    let original_public = serde_json::to_value(plan(&suite, true).request).unwrap();
    for field in ["requirement", "examples", "allowed_source_urls"] {
        assert_eq!(public["request"][field], original_public[field]);
    }
    assert_eq!(public.as_object().unwrap().len(), 2);
    assert_eq!(
        public["repair"]["previous_extractor_typescript"],
        INITIAL_PARSER
    );
    assert_eq!(public["repair"]["feedback"], json!(["value_mismatch"]));
    assert_eq!(public["repair"].as_object().unwrap().len(), 2);
    for withheld in [
        PRIVATE_ORACLE,
        "table-listings",
        "grand-hall",
        "output/",
        "999",
        "180",
        "75000",
        "offline-venue-suite-v1",
        "dataset-initial",
        "build-initial",
    ] {
        assert!(
            !public.to_string().contains(withheld),
            "repair prompt leaked {withheld}"
        );
    }
    for case in &suite.cases {
        assert!(!public.to_string().contains(&case.source.sha256));
    }
    advance_local(&mut session, &suite);
    assert_eq!(session.phase(), AcquisitionPhase::Forging);
    assert_ne!(
        session.forge().unwrap().spec().source.sha256().unwrap(),
        first_source
    );
    assert_current_packaging(session.forge().unwrap().spec());
    assert_eq!(session.forge().unwrap().completed_cases(), 0);
    assert!(session.forge().unwrap().artifact().is_none());
    let second_requests = finish_forge(&mut session, &suite, "repair", outputs()).await;
    advance_local(&mut session, &suite);
    let verified = session.verified(&suite).unwrap();
    assert_eq!(verified.operation_id, "acquire-repair");
    assert_eq!(
        verified.parent_operation_id.as_deref(),
        Some("acquire-initial")
    );
    assert_eq!(verified.artifact.actor_id, "actor-repair");
    assert_eq!(verified.artifact.build_id, "build-repair");
    assert!(verified.verification.cases.iter().all(|case| case
        .run_id
        .as_ref()
        .unwrap()
        .starts_with("run-repair-")));
    assert_fresh_pipeline(&first_requests, "initial", suite.cases.len());
    assert_fresh_pipeline(&second_requests, "repair", suite.cases.len());
    assert!(session.begin_generation(&suite).is_err());
}

#[tokio::test]
async fn second_semantic_rejection_is_terminal_instead_of_opening_a_third_attempt() {
    let (mut session, suite, _) = prepared(true).await;
    finish_forge(&mut session, &suite, "initial", wrong_outputs()).await;
    advance_local(&mut session, &suite);
    generation(&mut session, &suite, REPAIRED_PARSER, "msg_repair").await;
    advance_local(&mut session, &suite);
    finish_forge(&mut session, &suite, "repair", wrong_outputs()).await;
    advance_local(&mut session, &suite);
    assert_eq!(session.phase(), AcquisitionPhase::Rejected);
    assert_eq!(session.attempt(), 1);
    assert!(session.verified(&suite).is_err());
    assert!(session.begin_generation(&suite).is_err());
    assert!(session.prepare_forge(&suite).unwrap().is_none());
    let saved = serde_json::to_vec(&session).unwrap();
    assert!(!session.advance_local(&suite).unwrap());
    assert_eq!(serde_json::to_vec(&session).unwrap(), saved);
}

#[tokio::test]
async fn fresh_model_metadata_cannot_turn_an_unchanged_parser_into_a_repair() {
    let (mut session, suite, _) = prepared(true).await;
    let first_source = session.forge().unwrap().spec().source.sha256().unwrap();
    finish_forge(&mut session, &suite, "initial", wrong_outputs()).await;
    advance_local(&mut session, &suite);
    generation(
        &mut session,
        &suite,
        INITIAL_PARSER,
        "msg_fresh_receipt_same_source",
    )
    .await;
    // Provenance and candidate identities differ, but the actual parser does not.
    assert_ne!(
        generated_candidate(session.generation())
            .unwrap()
            .source
            .sha256()
            .unwrap(),
        first_source
    );
    advance_local(&mut session, &suite);
    assert_eq!(session.phase(), AcquisitionPhase::Rejected);
    assert_eq!(session.attempt(), 1);
    assert!(session.forge().is_none());
    assert!(session.prepare_forge(&suite).unwrap().is_none());
    assert!(session.begin_generation(&suite).is_err());
    assert!(session.verified(&suite).is_err());
    let saved = serde_json::to_string(&session).unwrap();
    assert!(
        saved.contains("actor-initial"),
        "initial rejected evidence must remain in the checkpoint"
    );
}

#[tokio::test]
async fn interrupted_forge_posts_require_reconciliation_and_never_trigger_repair() {
    for (completed, expected) in [
        (0, ActionKind::CreateActor),
        (1, ActionKind::StartBuild),
        (3, ActionKind::SubmitCase),
    ] {
        let (mut session, suite, _) = prepared(true).await;
        if completed != 0 {
            let spec = session.forge().unwrap().spec();
            let mut replies = vec![actor(spec, "initial")];
            if completed > 1 {
                replies.extend([
                    Reply::json(json!({"data": snapshot(spec)})),
                    build(spec, "initial", "RUNNING"),
                    build(spec, "initial", "SUCCEEDED"),
                ]);
            }
            let mock = Mock::serve(replies).await;
            let client = mock.client();
            for _ in 0..completed {
                forge_step(&mut session, &suite, &client).await;
            }
            mock.finish().await;
        }
        let abandoned = session.prepare_forge(&suite).unwrap().unwrap();
        assert_eq!(abandoned.kind(), expected);
        assert!(expected.is_mutating());
        drop(abandoned);
        session = restore(&session, &suite);
        assert!(session.recover_interrupted(&suite).unwrap());
        advance_local(&mut session, &suite);
        assert_eq!(session.phase(), AcquisitionPhase::NeedsReconciliation);
        assert_eq!(session.attempt(), 0);
        assert!(session.begin_generation(&suite).is_err());
        let result = session.prepare_forge(&suite);
        assert!(result.is_err() || result.unwrap().is_none());
        assert!(session.verified(&suite).is_err());
        let saved = serde_json::to_vec(&session).unwrap();
        assert!(!session.recover_interrupted(&suite).unwrap());
        assert_eq!(serde_json::to_vec(&session).unwrap(), saved);
    }
}

#[tokio::test]
async fn interrupted_collection_reuses_the_same_pin_run_and_sequence() {
    let (mut session, suite, _) = prepared(true).await;
    let spec = session.forge().unwrap().spec();
    let mut replies = vec![
        actor(spec, "initial"),
        Reply::json(json!({"data": snapshot(spec)})),
        build(spec, "initial", "RUNNING"),
        build(spec, "initial", "SUCCEEDED"),
        build(spec, "initial", "SUCCEEDED"),
        run("initial", 0),
    ];
    replies.extend([
        build(spec, "initial", "SUCCEEDED"),
        run("initial", 0),
        Reply::json(json!([outputs().remove(0)])),
    ]);
    let mock = Mock::serve(replies).await;
    let client = mock.client();
    for _ in 0..4 {
        forge_step(&mut session, &suite, &client).await;
    }
    let pinned = session.forge().unwrap().artifact().unwrap().clone();
    let abandoned = session.prepare_forge(&suite).unwrap().unwrap();
    assert_eq!(abandoned.kind(), ActionKind::CollectCase);
    let sequence = abandoned.sequence();
    drop(abandoned);
    session = restore(&session, &suite);
    assert!(!session.recover_interrupted(&suite).unwrap());
    let resumed = session.prepare_forge(&suite).unwrap().unwrap();
    assert_eq!(resumed.kind(), ActionKind::CollectCase);
    assert_eq!(resumed.sequence(), sequence);
    session
        .complete_forge(resumed.execute(&client).await, &suite)
        .unwrap();
    session = restore(&session, &suite);
    assert_eq!(session.forge().unwrap().artifact(), Some(&pinned));
    assert_eq!(session.forge().unwrap().completed_cases(), 1);
    assert_eq!(session.forge().unwrap().phase(), Phase::Testing);
    let requests = mock.finish().await;
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST /v2/actors/actor-initial/runs?"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("GET /v2/actor-runs/run-initial-0 "))
            .count(),
        1
    );
    assert!(requests
        .iter()
        .all(|request| !request.contains("build-repair")));
}

#[tokio::test]
async fn unknown_provider_outcome_and_wrong_build_identity_do_not_open_repair() {
    for ambiguous in [true, false] {
        let (mut session, suite, _) = prepared(true).await;
        let spec = session.forge().unwrap().spec();
        let replies = if ambiguous {
            vec![Reply {
                status: 503,
                body: json!({"private": "provider response is not a repair instruction"}),
            }]
        } else {
            let mut bad_build = build(spec, "initial", "SUCCEEDED");
            bad_build.body["data"]["id"] = json!("wrong-build");
            vec![
                actor(spec, "initial"),
                Reply::json(json!({"data": snapshot(spec)})),
                build(spec, "initial", "RUNNING"),
                bad_build,
            ]
        };
        let mock = Mock::serve(replies).await;
        let client = mock.client();
        for _ in 0..if ambiguous { 1 } else { 3 } {
            forge_step(&mut session, &suite, &client).await;
        }
        advance_local(&mut session, &suite);
        assert_eq!(
            session.phase(),
            if ambiguous {
                AcquisitionPhase::NeedsReconciliation
            } else {
                AcquisitionPhase::Failed
            }
        );
        assert_eq!(session.attempt(), 0);
        assert!(session.begin_generation(&suite).is_err());
        assert!(session.verified(&suite).is_err());
        mock.finish().await;
    }
}

#[tokio::test]
async fn changed_oracle_and_foreign_linked_generation_or_forge_are_rejected() {
    let (session, suite, _) = prepared(true).await;
    let mut changed_suite = suite.clone();
    changed_suite.scope.push_str(" changed");
    assert!(session.validate(&changed_suite).is_err());
    let mut changed = restore(&session, &suite);
    let before = serde_json::to_vec(&changed).unwrap();
    assert!(changed.advance_local(&changed_suite).is_err());
    assert_eq!(serde_json::to_vec(&changed).unwrap(), before);

    let mut other = AcquisitionSession::new(plan(&suite, true), &suite).unwrap();
    generation(&mut other, &suite, REPAIRED_PARSER, "msg_foreign").await;
    advance_local(&mut other, &suite);
    let original = serde_json::to_value(&session).unwrap();
    let foreign = serde_json::to_value(&other).unwrap();
    for field in ["generation", "forge"] {
        let mut altered = original.clone();
        assert!(altered.get(field).is_some(), "missing linked field {field}");
        altered[field] = foreign[field].clone();
        // The local digest is not a signature. Recompute it to demonstrate that
        // actual cross-checkpoint source binding rejects individually valid but
        // unrelated components, rather than relying only on stale hashes.
        let envelope = json!([
            "genesis-acquisition-v1",
            altered["schema_version"],
            altered["plan_sha256"],
            altered["suite_sha256"],
            altered["generation"],
            altered["forge"],
            altered["original"],
            altered["unchanged_repair"],
            altered["phase"]
        ]);
        altered["checkpoint_sha256"] = json!(json_sha256(&envelope));
        let mut restored: AcquisitionSession = serde_json::from_value(altered).unwrap();
        assert!(
            restored.validate(&suite).is_err(),
            "foreign {field} accepted"
        );
        assert!(restored.advance_local(&suite).is_err());
        assert!(restored.prepare_forge(&suite).is_err());
        assert!(restored.verified(&suite).is_err());
    }
}
