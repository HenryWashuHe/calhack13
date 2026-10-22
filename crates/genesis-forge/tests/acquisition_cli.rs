//! CLI checks stay offline: provider credentials are stripped from every child.
use genesis_forge::AcquisitionSession;
use genesis_verification::OracleSuite;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const PUBLIC_HTML: &str = "ACQUISITION_EXAMPLE_HTML_NOT_STATUS_637a";
const ORACLE_SECRET: &str = "ACQUISITION_ORACLE_NEVER_EMITTED_98e4";
const MODEL: &str = "claude-sonnet-4-6";

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "genesis-acquisition-cli-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let fixture = Self { directory };
        let mut suite: Value = serde_json::from_slice(include_bytes!(
            "../../genesis-verification/examples/fixtures/trusted/suite.json"
        ))
        .unwrap();
        suite["scope"] = json!(ORACLE_SECRET);
        suite["cases"][0]["expected"]["venues"][0]["venue_id"] = json!(ORACLE_SECRET);
        let urls: Vec<_> = suite["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["source"]["url"].clone())
            .collect();
        let plan = json!({
            "request": {
                "operation_id": "acquisition-cli-initial",
                "candidate": {
                    "capability_id": "venue-reader",
                    "capability_version": "1.0.0",
                    "actor_name": "genesis-acquisition-cli-initial",
                    "source_version": "0.1"
                },
                "requirement": "Extract venue data from the public example HTML.",
                "allowed_source_urls": urls,
                "examples": [{"url": urls[0], "html": format!("<html>{PUBLIC_HTML}</html>")}]
            },
            "model": {"model": MODEL, "max_tokens": 1024},
            "repair": {
                "operation_id": "acquisition-cli-repair",
                "candidate": {
                    "capability_id": "venue-reader",
                    "capability_version": "1.0.1",
                    "actor_name": "genesis-acquisition-cli-repair",
                    "source_version": "0.1"
                }
            }
        });
        fixture.write_json("plan.json", &plan);
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
        serde_json::from_slice(&self.bytes(name)).unwrap()
    }

    fn bytes(&self, name: &str) -> Vec<u8> {
        fs::read(self.path(name)).unwrap()
    }

    fn args(&self, command: &str) -> Vec<OsString> {
        let mut args = vec![command.into()];
        if command == "acquire-prepare" {
            args.extend(["--plan".into(), self.path("plan.json").into_os_string()]);
        }
        for (flag, file) in [
            ("--suite", "suite.json"),
            ("--checkpoint", "acquisition.json"),
        ] {
            args.extend([flag.into(), self.path(file).into_os_string()]);
        }
        if command == "acquire-export-forge" {
            args.extend(["--output".into(), self.path("forge.json").into_os_string()]);
        }
        args
    }

    fn run(&self, command: &str) -> Output {
        invoke(&self.args(command))
    }

    fn prepare(&self) -> Value {
        success(&self.run("acquire-prepare"))
    }

    fn pending_generation(&self) {
        self.prepare();
        let mut session: AcquisitionSession =
            serde_json::from_slice(&self.bytes("acquisition.json")).unwrap();
        let suite: OracleSuite = serde_json::from_slice(&self.bytes("suite.json")).unwrap();
        session.begin_generation(&suite).unwrap();
        self.write_json("acquisition.json", &session);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
    command.env_remove("ANTHROPIC_API_KEY");
    command.env_remove("APIFY_TOKEN");
    command
}

fn invoke(args: &[OsString]) -> Output {
    command().args(args).output().unwrap()
}

fn redacted(output: &Output) {
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        for private in [PUBLIC_HTML, ORACLE_SECRET] {
            assert!(
                !text.contains(private),
                "private content in CLI output: {text}"
            );
        }
        assert!(!text.contains("panicked"), "CLI panicked: {text}");
    }
}

fn success(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    redacted(output);
    serde_json::from_slice(&output.stdout).unwrap()
}

fn error(output: &Output, expected: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    redacted(output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "expected {expected:?}, got {stderr}"
    );
}

#[test]
fn offline_prepare_and_status_are_private_and_read_only() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    assert_eq!(prepared["phase"], "generating");
    assert_eq!(prepared["generation_phase"], "prepared");
    assert!(prepared["artifact"].is_null());
    let before = fixture.bytes("acquisition.json");
    assert_eq!(success(&fixture.run("acquire-status")), prepared);
    assert_eq!(fixture.bytes("acquisition.json"), before);
    for field in [
        "request",
        "examples",
        "extractor_typescript",
        "suite",
        "expected",
    ] {
        assert!(prepared.get(field).is_none());
    }
    assert!(!fixture.path("acquisition.json.lock").exists());
    assert!(!fixture.path("forge.json").exists());
    assert_eq!(fs::read_dir(&fixture.directory).unwrap().count(), 3);
}

#[test]
fn preparation_never_overwrites_and_every_command_honors_the_checkpoint_lock() {
    let fixture = Fixture::new();
    fixture.write("acquisition.json", b"existing checkpoint");
    error(&fixture.run("acquire-prepare"), "already exists");
    assert_eq!(fixture.bytes("acquisition.json"), b"existing checkpoint");
    fs::remove_file(fixture.path("acquisition.json")).unwrap();
    fixture.prepare();
    let before = fixture.bytes("acquisition.json");
    fixture.write("acquisition.json.lock", b"pid=999999\n");
    for action in [
        "acquire-prepare",
        "acquire-status",
        "acquire-step",
        "acquire-export",
    ] {
        error(&fixture.run(action), "lock already exists");
        assert_eq!(fixture.bytes("acquisition.json"), before);
        assert_eq!(fixture.bytes("acquisition.json.lock"), b"pid=999999\n");
    }
}

#[test]
fn strict_plan_and_suite_json_fail_before_a_checkpoint_is_created() {
    let fixture = Fixture::new();
    let plan = String::from_utf8(fixture.bytes("plan.json")).unwrap();
    for invalid in [
        "{\"request\":".to_string(),
        plan.replacen("\"max_tokens\":", "\"max_tokens\":1,\"max_tokens\":", 1),
        format!("{}null{}", "[".repeat(256), "]".repeat(256)),
    ] {
        fixture.write("plan.json", invalid);
        error(&fixture.run("acquire-prepare"), "invalid JSON/schema");
        assert!(!fixture.path("acquisition.json").exists());
        assert!(!fixture.path("acquisition.json.lock").exists());
    }
    fixture.write("plan.json", plan);
    let mut unknown = fixture.read_json("plan.json");
    unknown["oracle"] = json!(ORACLE_SECRET);
    fixture.write_json("plan.json", &unknown);
    error(&fixture.run("acquire-prepare"), "invalid JSON/schema");
    fixture.write("plan.json", vec![b' '; 8 * 1024 * 1024 + 1]);
    error(&fixture.run("acquire-prepare"), "byte limit");
    assert!(!fixture.path("acquisition.json").exists());

    let fresh = Fixture::new();
    let suite = String::from_utf8(fresh.bytes("suite.json")).unwrap();
    fresh.write(
        "suite.json",
        suite.replacen(
            "\"schema_version\":",
            "\"schema_version\":1,\"schema_version\":",
            1,
        ),
    );
    error(&fresh.run("acquire-prepare"), "invalid JSON/schema");
    assert!(!fresh.path("acquisition.json").exists());
}

#[test]
fn model_repair_identity_and_oracle_coverage_are_validated_offline() {
    let fixture = Fixture::new();
    let original = fixture.read_json("plan.json");
    let invalid_fields = [
        ("/model/model", json!("invalid model name")),
        ("/model/max_tokens", json!(0)),
        ("/model/max_tokens", json!(16_385)),
        (
            "/repair/operation_id",
            original["request"]["operation_id"].clone(),
        ),
        (
            "/repair/candidate/actor_name",
            original["request"]["candidate"]["actor_name"].clone(),
        ),
        (
            "/repair/candidate/capability_version",
            original["request"]["candidate"]["capability_version"].clone(),
        ),
        (
            "/repair/candidate/capability_id",
            json!("another-capability"),
        ),
        ("/repair/candidate/source_version", json!("invalid")),
    ];
    for (path, value) in invalid_fields {
        let mut plan = original.clone();
        *plan.pointer_mut(path).unwrap() = value;
        fixture.write_json("plan.json", &plan);
        error(&fixture.run("acquire-prepare"), "genesis-forge:");
        assert!(
            !fixture.path("acquisition.json").exists(),
            "accepted invalid {path}"
        );
    }
    let mut uncovered = original.clone();
    uncovered["request"]["allowed_source_urls"]
        .as_array_mut()
        .unwrap()
        .pop();
    fixture.write_json("plan.json", &uncovered);
    error(&fixture.run("acquire-prepare"), "genesis-forge:");
    assert!(!fixture.path("acquisition.json").exists());

    let mut no_repair = original;
    no_repair["repair"] = Value::Null;
    fixture.write_json("plan.json", &no_repair);
    fixture.prepare();
}

#[test]
fn invalid_oracle_and_forged_terminal_label_cannot_establish_acceptance() {
    let fixture = Fixture::new();
    let original_suite = fixture.read_json("suite.json");
    let mut incomplete = original_suite.clone();
    incomplete["cases"].as_array_mut().unwrap().truncate(1);
    fixture.write_json("suite.json", &incomplete);
    error(&fixture.run("acquire-prepare"), "genesis-forge:");
    assert!(!fixture.path("acquisition.json").exists());
    fixture.write_json("suite.json", &original_suite);
    fixture.prepare();
    let mut forged = fixture.read_json("acquisition.json");
    forged["phase"] = json!("verified");
    fixture.write_json("acquisition.json", &forged);
    let before = fixture.bytes("acquisition.json");
    for action in ["acquire-status", "acquire-export", "acquire-export-forge"] {
        error(&fixture.run(action), "genesis-forge:");
        assert_eq!(fixture.bytes("acquisition.json"), before);
    }
    assert!(!fixture.path("forge.json").exists());
}

#[test]
fn missing_or_invalid_credentials_do_not_consume_generation_intent() {
    let fixture = Fixture::new();
    fixture.prepare();
    let before = fixture.bytes("acquisition.json");
    error(&fixture.run("acquire-step"), "ANTHROPIC_API_KEY");
    assert_eq!(fixture.bytes("acquisition.json"), before);
    let mut args = fixture.args("acquire-run");
    args.extend(["--max-steps".into(), "64".into()]);
    error(&invoke(&args), "ANTHROPIC_API_KEY");
    assert_eq!(fixture.bytes("acquisition.json"), before);
    let secret = "ACQUISITION_API_KEY_MUST_NOT_APPEAR\ninvalid";
    let output = command()
        .args(fixture.args("acquire-step"))
        .env("ANTHROPIC_API_KEY", secret)
        .output()
        .unwrap();
    error(&output, "genesis-forge:");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("ACQUISITION_API_KEY_MUST_NOT_APPEAR")
    );
    assert_eq!(fixture.bytes("acquisition.json"), before);
    assert!(!fixture.path("acquisition.json.lock").exists());
}

#[test]
fn interrupted_generation_recovers_without_credentials_and_never_replays() {
    for action in ["acquire-step", "acquire-run"] {
        let fixture = Fixture::new();
        fixture.pending_generation();
        let pending = fixture.bytes("acquisition.json");
        assert_eq!(
            success(&fixture.run("acquire-status"))["generation_phase"],
            "requesting"
        );
        assert_eq!(fixture.bytes("acquisition.json"), pending);
        let mut args = fixture.args(action);
        if action == "acquire-run" {
            args.extend(["--max-steps".into(), "64".into()]);
        }
        let recovered = invoke(&args);
        assert_eq!(recovered.status.code(), Some(1));
        redacted(&recovered);
        assert!(recovered.stderr.is_empty());
        let status: Value = serde_json::from_slice(&recovered.stdout).unwrap();
        assert_eq!(status["phase"], "needs_reconciliation");
        assert_eq!(status["generation_phase"], "needs_reconciliation");
        if action == "acquire-run" {
            assert_eq!(recovered.stdout.iter().filter(|b| **b == b'\n').count(), 1);
        }
        let saved = fixture.bytes("acquisition.json");
        assert_ne!(saved, pending);
        let repeated = invoke(&args);
        assert_eq!(repeated.status.code(), Some(1));
        redacted(&repeated);
        assert!(repeated.stderr.is_empty());
        assert_eq!(fixture.bytes("acquisition.json"), saved);
    }
}

#[test]
fn run_requires_an_explicit_bounded_step_count_before_touching_state() {
    let fixture = Fixture::new();
    fixture.prepare();
    let before = fixture.bytes("acquisition.json");
    error(&fixture.run("acquire-run"), "requires --max-steps");
    for count in ["0", "65", "-1", "4294967296", "1.0", "invalid"] {
        let mut args = fixture.args("acquire-run");
        args.extend(["--max-steps".into(), count.into()]);
        error(&invoke(&args), "integer between 1 and 64");
        assert_eq!(fixture.bytes("acquisition.json"), before);
    }
    let mut missing = fixture.args("acquire-run");
    missing.push("--max-steps".into());
    error(&invoke(&missing), "integer between 1 and 64");
    let mut duplicate = fixture.args("acquire-run");
    duplicate.extend([
        "--max-steps".into(),
        "1".into(),
        "--max-steps".into(),
        "2".into(),
    ]);
    error(&invoke(&duplicate), "duplicate --max-steps");
    for count in ["1", "64"] {
        let mut args = fixture.args("acquire-run");
        args.extend(["--max-steps".into(), count.into()]);
        error(&invoke(&args), "ANTHROPIC_API_KEY");
        assert_eq!(fixture.bytes("acquisition.json"), before);
    }
}

#[test]
fn endpoint_credentials_and_unearned_retry_flags_cannot_bypass_saved_policy() {
    let fixture = Fixture::new();
    fixture.prepare();
    let before = fixture.bytes("acquisition.json");
    for (flag, value) in [
        ("--endpoint", "http://127.0.0.1:1"),
        ("--base-url", "http://127.0.0.1:1"),
        ("--api-key", "not-a-key"),
        ("--model", "another-model"),
        ("--retry", ""),
        ("--max-steps", "1"),
    ] {
        let mut args = fixture.args("acquire-step");
        args.push(flag.into());
        if !value.is_empty() {
            args.push(value.into());
        }
        error(&invoke(&args), "unknown argument");
    }
    for action in ["acquire-step", "acquire-run"] {
        let mut args = fixture.args(action);
        if action == "acquire-run" {
            args.extend(["--max-steps".into(), "1".into()]);
        }
        args.push("--retry-read".into());
        error(&invoke(&args), "requires a prior retryable read failure");
        args.push("--retry-read".into());
        error(&invoke(&args), "duplicate --retry-read");
    }
    assert_eq!(fixture.bytes("acquisition.json"), before);
}

#[test]
fn unverified_acquisitions_cannot_export_artifacts_or_replace_output_files() {
    let fixture = Fixture::new();
    fixture.prepare();
    let before = fixture.bytes("acquisition.json");
    error(&fixture.run("acquire-export"), "genesis-forge:");
    error(&fixture.run("acquire-export-forge"), "genesis-forge:");
    assert!(!fixture.path("forge.json").exists());
    assert!(!fixture.path("forge.json.lock").exists());
    fixture.write("forge.json", b"existing verified proof must survive");
    error(&fixture.run("acquire-export-forge"), "genesis-forge:");
    assert_eq!(
        fixture.bytes("forge.json"),
        b"existing verified proof must survive"
    );
    assert_eq!(fixture.bytes("acquisition.json"), before);
}

#[test]
fn changed_oracle_and_embedded_plan_are_rejected_without_mutation() {
    let fixture = Fixture::new();
    fixture.prepare();
    let original = fixture.bytes("acquisition.json");
    let suite_before = fixture.bytes("suite.json");
    let mut suite = fixture.read_json("suite.json");
    suite["scope"] = json!("Changed oracle scope");
    fixture.write_json("suite.json", &suite);
    for action in ["acquire-status", "acquire-step", "acquire-export"] {
        error(&fixture.run(action), "genesis-forge:");
        assert_eq!(fixture.bytes("acquisition.json"), original);
    }
    fixture.write("suite.json", suite_before);
    let encoded = String::from_utf8(original.clone()).unwrap();
    for (from, to) in [
        ("acquisition-cli-initial", "acquisition-cli-changed"),
        ("Extract venue data", "Fabricate venue data"),
        (MODEL, "another-model"),
        ("acquisition-cli-repair", "acquisition-cli-other-repair"),
    ] {
        assert!(encoded.contains(from));
        fixture.write("acquisition.json", encoded.replacen(from, to, 1));
        let changed = fixture.bytes("acquisition.json");
        error(&fixture.run("acquire-status"), "genesis-forge:");
        error(&fixture.run("acquire-step"), "genesis-forge:");
        assert_eq!(fixture.bytes("acquisition.json"), changed);
    }
    fixture.write("acquisition.json", original);
    success(&fixture.run("acquire-status"));
}

#[cfg(unix)]
#[test]
fn checkpoints_are_private_and_symlink_reads_fail_closed() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = Fixture::new();
    fixture.prepare();
    assert_eq!(
        fs::metadata(fixture.path("acquisition.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::rename(
        fixture.path("acquisition.json"),
        fixture.path("original.json"),
    )
    .unwrap();
    symlink(
        fixture.path("original.json"),
        fixture.path("acquisition.json"),
    )
    .unwrap();
    for action in ["acquire-status", "acquire-step", "acquire-export"] {
        error(&fixture.run(action), "not a symlink");
    }
    error(&fixture.run("acquire-prepare"), "already exists");
    fs::remove_file(fixture.path("acquisition.json")).unwrap();
    fs::rename(
        fixture.path("plan.json"),
        fixture.path("original-plan.json"),
    )
    .unwrap();
    symlink(
        fixture.path("original-plan.json"),
        fixture.path("plan.json"),
    )
    .unwrap();
    error(&fixture.run("acquire-prepare"), "not a symlink");
    assert!(!fixture.path("acquisition.json").exists());
}
