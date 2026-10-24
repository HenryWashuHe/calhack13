use genesis_forge::synthesis::{ModelConfig, SynthesisError, SynthesisRequest, SynthesisSession};
use genesis_forge::synthesis_http::AnthropicGenerator;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const PRIVATE_HTML: &str = "SYNTHESIS_PRIVATE_EXAMPLE_HTML_9bd3";
const PRIVATE_ORACLE: &str = "SYNTHESIS_PRIVATE_ORACLE_4b2a";
const MODEL: &str = "claude-sonnet-4-6";
const PARSER: &str = "// GENERATED_PARSER_PRIVATE_BYTES — preserve whitespace\r\nthrow new Error('GENERATED_SOURCE_MUST_NOT_EXECUTE_LOCALLY');\nexport function extract() { return { status: 'unsupported' }; }\n";

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "genesis-synthesis-cli-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let fixture = Self { directory };
        let mut suite: Value = serde_json::from_slice(
            &fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../genesis-verification/examples/fixtures/trusted/suite.json"),
            )
            .unwrap(),
        )
        .unwrap();
        suite["cases"][0]["expected"]["venues"][0]["venue_id"] = json!(PRIVATE_ORACLE);
        let urls: Vec<_> = suite["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["source"]["url"].clone())
            .collect();
        let candidate = json!({
            "capability_id": "venue-reader",
            "capability_version": "1.0.0",
            "actor_name": "genesis-synthesis-cli-test",
            "source_version": "0.1"
        });
        let request = json!({
            "operation_id": "synthesis-cli-v1",
            "candidate": candidate,
            "requirement": "Extract the venue listings from the supplied public web examples.",
            "allowed_source_urls": urls,
            "examples": [{"url": urls[0], "html": format!("<html>{PRIVATE_HTML}</html>")}]
        });
        let mut metadata = candidate;
        metadata["operation_id"] = request["operation_id"].clone();
        fixture.write_json("request.json", &request);
        fixture.write_json("model.json", &json!({"model": MODEL, "max_tokens": 1024}));
        fixture.write_json("candidate.json", &metadata);
        fixture.write_json("suite.json", &suite);
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.path(name), bytes).unwrap();
    }

    fn write_json(&self, name: &str, value: &impl serde::Serialize) {
        self.write(name, serde_json::to_vec(value).unwrap());
    }

    fn read_json(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.path(name)).unwrap()).unwrap()
    }

    fn args(&self, command: &str) -> Vec<OsString> {
        let mut args = vec![command.into()];
        let paths: &[(&str, &str)] = match command {
            "generate-prepare" => &[
                ("--request", "request.json"),
                ("--model-config", "model.json"),
                ("--checkpoint", "generation.json"),
            ],
            "generate-status" | "generate-step" => &[("--checkpoint", "generation.json")],
            "prepare-generated" => &[
                ("--candidate", "candidate.json"),
                ("--generation", "generation.json"),
                ("--suite", "suite.json"),
                ("--checkpoint", "forge.json"),
            ],
            _ => panic!("unsupported test command"),
        };
        for (flag, file) in paths {
            args.extend([(*flag).into(), self.path(file).into_os_string()]);
        }
        args
    }

    fn run(&self, command: &str) -> Output {
        invoke(&self.args(command))
    }

    fn prepare(&self) {
        assert_success(&self.run("generate-prepare"));
    }

    fn session(&self) -> SynthesisSession {
        serde_json::from_slice(&fs::read(self.path("generation.json")).unwrap()).unwrap()
    }

    async fn generated(&self) {
        let request: SynthesisRequest =
            serde_json::from_slice(&fs::read(self.path("request.json")).unwrap()).unwrap();
        let config: ModelConfig =
            serde_json::from_slice(&fs::read(self.path("model.json")).unwrap()).unwrap();
        let mut session = SynthesisSession::new(request, config.clone()).unwrap();
        let prompt = session.begin().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let body_start = loop {
                let mut buffer = [0; 4096];
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "client disconnected before request headers");
                bytes.extend_from_slice(&buffer[..read]);
                assert!(bytes.len() < 1024 * 1024);
                if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&bytes[..body_start]);
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() - body_start < length {
                let mut buffer = [0; 4096];
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "client disconnected before request body");
                bytes.extend_from_slice(&buffer[..read]);
            }
            let sent: Value =
                serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
            assert_eq!(sent["model"], MODEL);
            assert!(String::from_utf8_lossy(&bytes).contains(PRIVATE_HTML));
            assert!(!String::from_utf8_lossy(&bytes).contains(PRIVATE_ORACLE));
            let body = serde_json::to_vec(&json!({
                "id": "msg_synthesis_cli_fixture",
                "type": "message",
                "role": "assistant",
                "model": MODEL,
                "content": [{"type": "text", "text": json!({"extractor_typescript": PARSER}).to_string()}],
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {"input_tokens": 200, "output_tokens": 80}
            })).unwrap();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let generator =
            AnthropicGenerator::with_test_endpoint(&endpoint, "local-fixture-key").unwrap();
        let receipt = generator.generate(&prompt, &config).await;
        session.complete(receipt).unwrap();
        server.await.unwrap();
        assert_eq!(session.extractor().unwrap(), PARSER);
        self.write_json("generation.json", &session);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
    // CLI tests cannot consume developer credentials or call either live provider.
    command.env_remove("ANTHROPIC_API_KEY");
    command.env_remove("APIFY_TOKEN");
    command
}

fn invoke(args: &[OsString]) -> Output {
    command().args(args).output().unwrap()
}

fn assert_redacted(output: &Output) {
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        for secret in [
            PRIVATE_HTML,
            PRIVATE_ORACLE,
            "GENERATED_PARSER_PRIVATE_BYTES",
        ] {
            assert!(!text.contains(secret), "private content exposed: {text}");
        }
    }
}

fn assert_success(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_redacted(output);
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_error(output: &Output, expected: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert_redacted(output);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(expected), "{error}");
    assert!(!error.contains("panicked"), "{error}");
}

#[test]
fn offline_preparation_and_read_only_status_do_not_expose_examples_or_source() {
    let fixture = Fixture::new();
    let prepared = assert_success(&fixture.run("generate-prepare"));
    assert_eq!(prepared["phase"], "prepared");
    let bytes = fs::read(fixture.path("generation.json")).unwrap();
    assert_eq!(assert_success(&fixture.run("generate-status")), prepared);
    assert_eq!(fs::read(fixture.path("generation.json")).unwrap(), bytes);
    assert!(prepared.get("examples").is_none());
    assert!(prepared.get("extractor_typescript").is_none());
    assert!(!fixture.path("generation.json.lock").exists());
    assert!(!fixture.path("forge.json").exists());
}

#[test]
fn generation_creation_never_overwrites_and_respects_existing_locks() {
    let fixture = Fixture::new();
    fixture.write("generation.json", b"existing checkpoint must survive");
    assert_error(&fixture.run("generate-prepare"), "already exists");
    assert_eq!(
        fs::read(fixture.path("generation.json")).unwrap(),
        b"existing checkpoint must survive"
    );
    assert!(!fixture.path("generation.json.lock").exists());
    fs::remove_file(fixture.path("generation.json")).unwrap();
    fixture.write("generation.json.lock", b"pid=999999\n");
    assert_error(&fixture.run("generate-prepare"), "lock already exists");
    assert_eq!(
        fs::read(fixture.path("generation.json.lock")).unwrap(),
        b"pid=999999\n"
    );
    assert!(!fixture.path("generation.json").exists());
}

#[test]
fn strict_request_and_model_json_fail_without_creating_a_checkpoint() {
    let fixture = Fixture::new();
    let request = fs::read(fixture.path("request.json")).unwrap();
    for invalid in [
        "{\"operation_id\":".to_string(),
        String::from_utf8(request.clone()).unwrap().replacen(
            "\"operation_id\":",
            "\"operation_id\":\"hidden\",\"operation_id\":",
            1,
        ),
        format!("{}null{}", "[".repeat(256), "]".repeat(256)),
    ] {
        fixture.write("request.json", invalid);
        assert_error(&fixture.run("generate-prepare"), "invalid JSON/schema");
        assert!(!fixture.path("generation.json").exists());
    }
    fixture.write("request.json", request);
    fixture.write(
        "model.json",
        format!("{{\"model\":\"{MODEL}\",\"max_tokens\":1,\"max_tokens\":1024}}"),
    );
    assert_error(&fixture.run("generate-prepare"), "invalid JSON/schema");
    assert!(!fixture.path("generation.json").exists());
    fixture.write_json("model.json", &json!({"model": MODEL, "max_tokens": 1024}));
    let mut request = fixture.read_json("request.json");
    request["examples"][0]["oracle"] = json!(PRIVATE_ORACLE);
    fixture.write_json("request.json", &request);
    assert_error(&fixture.run("generate-prepare"), "invalid JSON/schema");
    assert!(!fixture.path("generation.json").exists());
}

#[test]
fn invalid_requests_and_oversized_input_do_not_create_checkpoints_or_echo_html() {
    let fixture = Fixture::new();
    let original = fixture.read_json("request.json");
    for url in [
        "http://example.com/",
        "https://example.com/#fragment",
        "https://user:password@example.com/",
    ] {
        let mut request = original.clone();
        request["allowed_source_urls"][0] = json!(url);
        fixture.write_json("request.json", &request);
        assert_error(&fixture.run("generate-prepare"), "public contract");
        assert!(!fixture.path("generation.json").exists());
    }
    fixture.write("request.json", vec![b' '; 8 * 1024 * 1024 + 1]);
    assert_error(&fixture.run("generate-prepare"), "byte limit");
    assert!(!fixture.path("generation.json").exists());
}

#[test]
fn missing_or_invalid_api_key_leaves_prepared_intent_unchanged() {
    let fixture = Fixture::new();
    fixture.prepare();
    let before = fs::read(fixture.path("generation.json")).unwrap();
    assert_error(&fixture.run("generate-step"), "ANTHROPIC_API_KEY");
    assert_eq!(fs::read(fixture.path("generation.json")).unwrap(), before);
    let secret = "SYNTHESIS_KEY_MUST_NEVER_APPEAR\ninvalid";
    let output = command()
        .args(fixture.args("generate-step"))
        .env("ANTHROPIC_API_KEY", secret)
        .output()
        .unwrap();
    assert_error(&output, "public contract");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHESIS_KEY_MUST_NEVER_APPEAR"));
    assert_eq!(fs::read(fixture.path("generation.json")).unwrap(), before);
    assert!(!fixture.path("generation.json.lock").exists());
}

#[test]
fn pending_generation_recovers_without_a_key_and_is_never_replayed() {
    let fixture = Fixture::new();
    fixture.prepare();
    let mut session = fixture.session();
    let _prompt = session.begin().unwrap();
    // Persist the pre-request intent only, simulating termination during POST.
    fixture.write_json("generation.json", &session);
    let result = fixture.run("generate-step");
    assert_eq!(result.status.code(), Some(1));
    assert_redacted(&result);
    let status: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["phase"], "needs_reconciliation");
    assert!(String::from_utf8_lossy(&result.stderr).contains("no request was sent"));
    let saved = fs::read(fixture.path("generation.json")).unwrap();
    assert_eq!(fixture.run("generate-step").status.code(), Some(1));
    assert_eq!(fs::read(fixture.path("generation.json")).unwrap(), saved);
    assert_eq!(
        assert_success(&fixture.run("generate-status"))["phase"],
        "needs_reconciliation"
    );
    assert_error(&fixture.run("prepare-generated"), "InvalidCandidate");
    assert!(!fixture.path("forge.json").exists());
}

#[test]
fn failed_generation_is_terminal_and_reports_only_a_sanitized_failure() {
    let fixture = Fixture::new();
    fixture.prepare();
    let mut session = fixture.session();
    let _prompt = session.begin().unwrap();
    session
        .complete(Err(SynthesisError::Http { status: 429 }))
        .unwrap();
    fixture.write_json("generation.json", &session);
    let before = fs::read(fixture.path("generation.json")).unwrap();
    let output = fixture.run("generate-step");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert_redacted(&output);
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["phase"], "failed");
    assert_eq!(status["failure"], json!({"kind": "http", "status": 429}));
    assert_eq!(assert_success(&fixture.run("generate-status")), status);
    assert_eq!(fs::read(fixture.path("generation.json")).unwrap(), before);
    assert_error(&fixture.run("prepare-generated"), "InvalidCandidate");
    assert!(!fixture.path("forge.json").exists());
}

#[test]
fn generation_cli_refuses_credentials_endpoint_overrides_and_retry_flags() {
    let fixture = Fixture::new();
    for (flag, value) in [
        ("--api-key", "not-a-key"),
        ("--base-url", "http://127.0.0.1:1"),
        ("--endpoint", "http://127.0.0.1:1"),
        ("--retry-read", ""),
        ("--retry", ""),
    ] {
        let mut args = fixture.args("generate-step");
        args.push(flag.into());
        if !value.is_empty() {
            args.push(value.into());
        }
        assert_error(&invoke(&args), "unknown argument");
    }
    let mut args = fixture.args("generate-prepare");
    args.extend([
        "--request".into(),
        fixture.path("request.json").into_os_string(),
    ]);
    assert_error(&invoke(&args), "duplicate --request");
    assert!(!fixture.path("generation.json").exists());
}

#[tokio::test]
async fn generated_prepare_binds_saved_candidate_actor_identity() {
    let fixture = Fixture::new();
    let mut request = fixture.read_json("request.json");
    request["candidate"]["source_version"] = json!("0.2");
    let mut metadata = fixture.read_json("candidate.json");
    metadata["source_version"] = json!("0.2");
    fixture.write_json("request.json", &request);
    fixture.write_json("candidate.json", &metadata);
    fixture.generated().await;
    let generation_before = fs::read(fixture.path("generation.json")).unwrap();
    assert_success(&fixture.run("prepare-generated"));
    let forge = fixture.read_json("forge.json");
    let config = forge["spec"]["source"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["name"] == ".actor/actor.json")
        .unwrap();
    let config: Value = serde_json::from_str(config["content"].as_str().unwrap()).unwrap();
    assert_eq!(config["name"], request["candidate"]["actor_name"]);
    assert_eq!(config["version"], request["candidate"]["source_version"]);
    assert_eq!(
        fs::read(fixture.path("generation.json")).unwrap(),
        generation_before
    );
}

#[tokio::test]
async fn generated_handoff_preserves_exact_parser_and_attaches_only_provenance() {
    let fixture = Fixture::new();
    // The suite may use a subset, but it cannot replace the explicit source
    // allowlist fixed before the model call.
    let mut request = fixture.read_json("request.json");
    request["allowed_source_urls"]
        .as_array_mut()
        .unwrap()
        .push(json!(
            "https://fixtures.genesis.example/venues/additional.html"
        ));
    fixture.write_json("request.json", &request);
    fixture.generated().await;
    let generation_before = fs::read(fixture.path("generation.json")).unwrap();
    assert_eq!(
        assert_success(&fixture.run("generate-status"))["phase"],
        "generated"
    );
    assert_eq!(
        assert_success(&fixture.run("generate-step"))["phase"],
        "generated"
    );
    assert_eq!(
        fs::read(fixture.path("generation.json")).unwrap(),
        generation_before
    );
    let status = assert_success(&fixture.run("prepare-generated"));
    assert_eq!(status["phase"], "prepared");
    assert_eq!(status["total_cases"], 3);
    let forge = fixture.read_json("forge.json");
    let files = forge["spec"]["source"]["files"].as_array().unwrap();
    let file = |name: &str| files.iter().find(|entry| entry["name"] == name).unwrap();
    assert_eq!(
        file("src/extractor.ts")["content"]
            .as_str()
            .unwrap()
            .as_bytes(),
        PARSER.as_bytes()
    );
    for name in [
        "package-lock.json",
        "src/runtime.ts",
        "src/contract.ts",
        "Dockerfile",
    ] {
        assert_eq!(
            file(name)["content"].as_str().unwrap().as_bytes(),
            fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../actors/venue-extractor-template")
                    .join(name)
            )
            .unwrap()
        );
    }
    let provenance: Value =
        serde_json::from_str(file("genesis-synthesis.json")["content"].as_str().unwrap()).unwrap();
    assert_eq!(provenance["receipt"]["model"], MODEL);
    assert_eq!(
        provenance["receipt"]["message_id"],
        "msg_synthesis_cli_fixture"
    );
    assert_eq!(
        provenance["receipt"]["extractor_sha256"],
        format!("{:x}", Sha256::digest(PARSER.as_bytes()))
    );
    assert_eq!(
        provenance["receipt"]["request_sha256"],
        fixture.session().receipt().unwrap().request_sha256()
    );
    assert_eq!(provenance["receipt"]["input_tokens"], 200);
    assert_eq!(provenance["receipt"]["output_tokens"], 80);
    for key in [
        "request",
        "requirement",
        "examples",
        "html",
        "extractor_typescript",
    ] {
        assert!(provenance.get(key).is_none());
        assert!(provenance["receipt"].get(key).is_none());
    }
    let allowed: Value =
        serde_json::from_str(file("allowed-sources.json")["content"].as_str().unwrap()).unwrap();
    let mut expected_urls = fixture.read_json("request.json")["allowed_source_urls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|url| url.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    expected_urls.sort();
    assert_eq!(allowed["urls"], json!(expected_urls));
    let package = serde_json::to_string(files).unwrap();
    assert!(!package.contains(PRIVATE_HTML));
    assert!(!package.contains(PRIVATE_ORACLE));
    assert!(files
        .iter()
        .all(|entry| !entry["name"].as_str().unwrap().contains("suite")));
    assert_eq!(
        fs::read(fixture.path("generation.json")).unwrap(),
        generation_before
    );
    let forge_before = fs::read(fixture.path("forge.json")).unwrap();
    assert_error(&fixture.run("prepare-generated"), "already exists");
    assert_eq!(fs::read(fixture.path("forge.json")).unwrap(), forge_before);
}

#[test]
fn prepared_generation_cannot_be_handed_to_the_forge() {
    let fixture = Fixture::new();
    fixture.prepare();
    assert_error(&fixture.run("prepare-generated"), "InvalidCandidate");
    assert!(!fixture.path("forge.json").exists());
}

#[test]
fn handoff_respects_both_generation_and_forge_checkpoint_locks() {
    let fixture = Fixture::new();
    fixture.prepare();
    let generation = fs::read(fixture.path("generation.json")).unwrap();
    for lock in ["generation.json.lock", "forge.json.lock"] {
        fixture.write(lock, b"pid=999999\n");
        assert_error(&fixture.run("prepare-generated"), "lock already exists");
        assert!(!fixture.path("forge.json").exists());
        assert_eq!(
            fs::read(fixture.path("generation.json")).unwrap(),
            generation
        );
        assert_eq!(fs::read(fixture.path(lock)).unwrap(), b"pid=999999\n");
        fs::remove_file(fixture.path(lock)).unwrap();
    }
}

#[tokio::test]
async fn every_candidate_metadata_field_is_bound_to_the_generation_request() {
    let fixture = Fixture::new();
    fixture.generated().await;
    let original = fixture.read_json("candidate.json");
    for (field, value) in [
        ("operation_id", "another-operation"),
        ("capability_id", "another-capability"),
        ("capability_version", "2.0.0"),
        ("actor_name", "another-actor"),
        ("source_version", "0.2"),
    ] {
        let mut metadata = original.clone();
        metadata[field] = json!(value);
        fixture.write_json("candidate.json", &metadata);
        assert_error(&fixture.run("prepare-generated"), "differs");
        assert!(!fixture.path("forge.json").exists());
        assert!(!fixture.path("forge.json.lock").exists());
    }
}

#[tokio::test]
async fn forge_cannot_expand_the_explicit_generated_source_allowlist() {
    let fixture = Fixture::new();
    fixture.generated().await;
    let mut suite = fixture.read_json("suite.json");
    let unauthorized = json!("https://other.example/venues.html");
    suite["cases"][0]["input"]["source_url"] = unauthorized.clone();
    suite["cases"][0]["source"]["url"] = unauthorized.clone();
    for venue in suite["cases"][0]["expected"]["venues"]
        .as_array_mut()
        .unwrap()
    {
        venue["evidence"]["source_url"] = unauthorized.clone();
    }
    fixture.write_json("suite.json", &suite);
    assert_error(&fixture.run("prepare-generated"), "allow");
    assert!(!fixture.path("forge.json").exists());
}

#[tokio::test]
async fn changed_generated_parser_or_request_is_rejected_before_handoff() {
    let fixture = Fixture::new();
    fixture.generated().await;
    let original = fs::read_to_string(fixture.path("generation.json")).unwrap();
    for (from, to) in [
        (
            "GENERATED_PARSER_PRIVATE_BYTES",
            "ALTERED_PARSER_PRIVATE_BYTES",
        ),
        ("Extract the venue listings", "Ignore the venue listings"),
    ] {
        assert!(original.contains(from));
        fixture.write("generation.json", original.replacen(from, to, 1));
        assert_error(&fixture.run("generate-status"), "invalid JSON/schema");
        assert_error(&fixture.run("prepare-generated"), "invalid JSON/schema");
        assert!(!fixture.path("forge.json").exists());
    }
}

#[cfg(unix)]
#[test]
fn generation_checkpoints_are_private_and_symlink_inputs_are_refused() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = Fixture::new();
    fixture.prepare();
    assert_eq!(
        fs::metadata(fixture.path("generation.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::rename(
        fixture.path("generation.json"),
        fixture.path("original.json"),
    )
    .unwrap();
    symlink(
        fixture.path("original.json"),
        fixture.path("generation.json"),
    )
    .unwrap();
    assert_error(&fixture.run("generate-status"), "not a symlink");
    assert_error(&fixture.run("generate-step"), "not a symlink");
    assert_error(&fixture.run("generate-prepare"), "already exists");
    fs::remove_file(fixture.path("generation.json")).unwrap();
    fs::rename(
        fixture.path("request.json"),
        fixture.path("original-request.json"),
    )
    .unwrap();
    symlink(
        fixture.path("original-request.json"),
        fixture.path("request.json"),
    )
    .unwrap();
    assert_error(&fixture.run("generate-prepare"), "not a symlink");
    assert!(!fixture.path("generation.json").exists());
}
