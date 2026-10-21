use genesis_forge::ForgeSession;
use genesis_verification::OracleSuite;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "genesis-forge-cli-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let fixture = Self { directory };
        fixture.write("candidate.json", serde_json::to_vec(&json!({
            "operation_id":"offline-candidate-v1", "capability_id":"venue-reader", "capability_version":"1.0.0",
            "actor_name":"genesis-cli-offline-test", "source_version":"0.1"
        })).unwrap());
        fixture.write(
            "extractor.ts",
            fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../actors/venue-extractor-template/src/extractor.ts"),
            )
            .unwrap(),
        );
        fixture.write(
            "suite.json",
            fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../genesis-verification/examples/fixtures/trusted/suite.json"),
            )
            .unwrap(),
        );
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.path(name), bytes).unwrap();
    }
    fn args(&self, command: &str) -> Vec<OsString> {
        let mut args = vec![
            command.into(),
            "--suite".into(),
            self.path("suite.json").into_os_string(),
            "--checkpoint".into(),
            self.path("session.json").into_os_string(),
        ];
        if command == "prepare" {
            args.extend([
                "--candidate".into(),
                self.path("candidate.json").into_os_string(),
                "--extractor".into(),
                self.path("extractor.ts").into_os_string(),
            ]);
        }
        args
    }
    fn run(&self, command: &str) -> Output {
        invoke(&self.args(command))
    }
    fn prepare(&self) {
        assert_success(&self.run("prepare"));
    }
    fn session(&self) -> Value {
        serde_json::from_slice(&fs::read(self.path("session.json")).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn command() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
    // Tests must never use a developer's real provider credentials.
    cmd.env_remove("APIFY_TOKEN");
    cmd
}
fn invoke(args: &[OsString]) -> Output {
    command().args(args).output().unwrap()
}
fn assert_success(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
fn assert_error(output: &Output, message: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn prepares_offline_and_status_revalidates_without_exposing_source_or_oracle() {
    let fixture = Fixture::new();
    let mut suite: Value =
        serde_json::from_slice(&fs::read(fixture.path("suite.json")).unwrap()).unwrap();
    suite["cases"][0]["expected"]["venues"][0]["venue_id"] = json!("ORACLE_PRIVATE_SENTINEL");
    fixture.write("suite.json", serde_json::to_vec(&suite).unwrap());
    let prepared = assert_success(&fixture.run("prepare"));
    assert_eq!(prepared["phase"], "prepared");
    assert_eq!(prepared["completed_cases"], 0);
    assert_eq!(prepared["total_cases"], 3);
    assert!(prepared["pending_action"].is_null());
    let status = assert_success(&fixture.run("status"));
    assert_eq!(prepared, status);
    assert!(status.get("source").is_none());
    let checkpoint = fs::read_to_string(fixture.path("session.json")).unwrap();
    assert!(!checkpoint.contains("ORACLE_PRIVATE_SENTINEL"));
    assert!(!checkpoint.contains("APIFY_TOKEN="));
    let session = fixture.session();
    let files = session["spec"]["source"]["files"].as_array().unwrap();
    assert!(files.iter().any(|file| file["name"] == "src/extractor.ts"));
    assert!(files
        .iter()
        .any(|file| file["name"] == "allowed-sources.json"));
    assert!(files
        .iter()
        .all(|file| !file["name"].as_str().unwrap().contains("suite")));
    assert!(!fixture.path("session.json.lock").exists());
}

#[test]
fn prepare_cannot_overwrite_an_existing_checkpoint() {
    let fixture = Fixture::new();
    fixture.write("session.json", b"existing checkpoint must survive");
    assert_error(&fixture.run("prepare"), "already exists");
    assert_eq!(
        fs::read(fixture.path("session.json")).unwrap(),
        b"existing checkpoint must survive"
    );
    assert!(!fixture.path("session.json.lock").exists());
}

#[test]
fn lock_collision_stops_work_and_preserves_stale_lock_for_review() {
    let fixture = Fixture::new();
    fixture.write("session.json.lock", b"pid=999999\n");
    assert_error(&fixture.run("prepare"), "lock already exists");
    assert_eq!(
        fs::read(fixture.path("session.json.lock")).unwrap(),
        b"pid=999999\n"
    );
    assert!(!fixture.path("session.json").exists());
}

#[test]
fn missing_or_invalid_token_never_marks_a_submission_pending() {
    let fixture = Fixture::new();
    fixture.prepare();
    let original = fs::read(fixture.path("session.json")).unwrap();
    assert_error(&fixture.run("step"), "APIFY_TOKEN is required");
    assert_eq!(fs::read(fixture.path("session.json")).unwrap(), original);
    let secret = "MUST_NOT_APPEAR invalid token";
    let output = command()
        .args(fixture.args("step"))
        .env("APIFY_TOKEN", secret)
        .output()
        .unwrap();
    assert_error(&output, "InvalidConfiguration");
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    assert_eq!(fs::read(fixture.path("session.json")).unwrap(), original);
    assert!(!fixture.path("session.json.lock").exists());
}

#[test]
fn pending_post_after_restart_is_persisted_as_reconciliation_without_credentials() {
    let fixture = Fixture::new();
    fixture.prepare();
    let suite: OracleSuite =
        serde_json::from_slice(&fs::read(fixture.path("suite.json")).unwrap()).unwrap();
    let mut session: ForgeSession =
        serde_json::from_slice(&fs::read(fixture.path("session.json")).unwrap()).unwrap();
    let action = session.prepare_next(&suite).unwrap().unwrap();
    assert!(action.kind().is_mutating());
    // Simulate a crash after the caller persisted intent but before it could
    // persist a completion. Never execute the provider action in this test.
    drop(action);
    fixture.write("session.json", serde_json::to_vec(&session).unwrap());
    let result = fixture.run("step");
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let status: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["phase"], "needs_reconciliation");
    assert_eq!(status["pending_action"], "create_actor");
    assert_eq!(status["failure"]["ambiguous"], true);
    assert!(String::from_utf8_lossy(&result.stderr).contains("no request was sent"));
    let saved = fs::read(fixture.path("session.json")).unwrap();
    assert_eq!(fixture.run("step").status.code(), Some(1));
    assert_eq!(fs::read(fixture.path("session.json")).unwrap(), saved);
}

#[test]
fn export_requires_verified_artifact_and_status_rejects_changed_suite_or_corruption() {
    let fixture = Fixture::new();
    fixture.prepare();
    assert_error(&fixture.run("export"), "NotVerified");
    let mut suite: Value =
        serde_json::from_slice(&fs::read(fixture.path("suite.json")).unwrap()).unwrap();
    suite["scope"] = json!("changed trusted scope");
    fixture.write("suite.json", serde_json::to_vec(&suite).unwrap());
    assert_error(&fixture.run("status"), "SuiteChanged");
    let other = Fixture::new();
    other.prepare();
    let mut checkpoint = other.session();
    checkpoint["phase"] = json!("verified");
    other.write("session.json", serde_json::to_vec(&checkpoint).unwrap());
    assert_error(&other.run("status"), "CorruptCheckpoint");
}

#[test]
fn duplicate_keys_and_unexpected_source_metadata_are_rejected() {
    let fixture = Fixture::new();
    let metadata = fs::read_to_string(fixture.path("candidate.json")).unwrap();
    let duplicate = metadata.replacen(
        "\"operation_id\":",
        "\"operation_id\":\"hidden\",\"operation_id\":",
        1,
    );
    fixture.write("candidate.json", duplicate);
    assert_error(&fixture.run("prepare"), "invalid JSON/schema");
    assert!(!fixture.path("session.json").exists());
    let mut candidate: Value = serde_json::from_str(&metadata).unwrap();
    candidate["source"] = json!({"files": []});
    fixture.write("candidate.json", serde_json::to_vec(&candidate).unwrap());
    assert_error(&fixture.run("prepare"), "invalid JSON/schema");
    fixture.write("candidate.json", metadata);
    let suite = fs::read_to_string(fixture.path("suite.json"))
        .unwrap()
        .replacen(
            "\"attendees\": 120",
            "\"attendees\": 999, \"attendees\": 120",
            1,
        );
    fixture.write("suite.json", suite);
    assert_error(&fixture.run("prepare"), "invalid JSON/schema");
}

#[test]
fn deep_truncated_and_oversized_json_fail_before_any_checkpoint_is_created() {
    let fixture = Fixture::new();
    for payload in [
        b"{\"operation_id\":".to_vec(),
        format!("{}null{}", "[".repeat(256), "]".repeat(256)).into_bytes(),
    ] {
        fixture.write("candidate.json", payload);
        assert_error(&fixture.run("prepare"), "invalid JSON/schema");
        assert!(!fixture.path("session.json").exists());
    }
    fixture.write("candidate.json", vec![b' '; 8 * 1024 * 1024 + 1]);
    assert_error(&fixture.run("prepare"), "byte limit");
    assert!(!fixture.path("session.json").exists());
}

#[test]
fn extractor_bytes_are_bounded_and_must_be_utf8() {
    let fixture = Fixture::new();
    fixture.write("extractor.ts", vec![b'x'; 128 * 1024 + 1]);
    assert_error(&fixture.run("prepare"), "byte limit");
    fixture.write("extractor.ts", [0xff, 0xfe]);
    assert_error(&fixture.run("prepare"), "UTF-8");
    assert!(!fixture.path("session.json").exists());
}

#[test]
fn arguments_do_not_accept_tokens_endpoints_or_accidental_retries() {
    let fixture = Fixture::new();
    assert_error(&invoke(&[]), "Usage:");
    assert_error(&invoke(&["destroy".into()]), "unknown command");
    let mut args = fixture.args("prepare");
    args.push("--token".into());
    args.push("not-a-token".into());
    assert_error(&invoke(&args), "unknown argument");
    let mut args = fixture.args("step");
    args.push("--base-url".into());
    args.push("http://127.0.0.1:1".into());
    assert_error(&invoke(&args), "unknown argument");
    let mut args = fixture.args("prepare");
    args.extend([
        "--suite".into(),
        fixture.path("suite.json").into_os_string(),
    ]);
    assert_error(&invoke(&args), "duplicate --suite");
    assert_error(
        &invoke(&["status".into(), "--suite".into()]),
        "missing path",
    );
    assert_error(&invoke(&["status".into()]), "missing --suite");
    fixture.prepare();
    let mut args = fixture.args("step");
    args.push("--retry-read".into());
    assert_error(&invoke(&args), "requires a prior retryable read failure");
    let help = invoke(&["--help".into()]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("no publication or installation"));
}

#[cfg(unix)]
#[test]
fn checkpoint_permissions_are_private_and_symlink_inputs_are_refused() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = Fixture::new();
    fixture.prepare();
    assert_eq!(
        fs::metadata(fixture.path("session.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::rename(fixture.path("session.json"), fixture.path("original.json")).unwrap();
    symlink(fixture.path("original.json"), fixture.path("session.json")).unwrap();
    assert_error(&fixture.run("status"), "not a symlink");
    assert_error(&fixture.run("prepare"), "already exists");
}
