use genesis_verification::{json_sha256, ArtifactPin, ExecutionEvidence, OracleSuite};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-verification-cli-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated CLI test directory");
        Self(path)
    }

    fn write(&self, name: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, content).expect("write CLI test input");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/fixtures")
        .join(relative)
}

fn arguments(runs: &Path) -> Vec<OsString> {
    vec![
        "--suite".into(),
        fixture("trusted/suite.json").into_os_string(),
        "--artifact".into(),
        fixture("artifact.json").into_os_string(),
        "--runs".into(),
        runs.as_os_str().to_owned(),
    ]
}

fn invoke(args: &[OsString]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_genesis-verify"))
        .args(args)
        .output()
        .expect("execute built genesis-verify binary")
}

fn assert_controlled_error(output: &Output, message: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected invalid-input exit, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "invalid input emitted a report");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("genesis-verify:"), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!stderr.contains("stack overflow"), "{stderr}");
}

#[test]
fn shipped_example_is_accepted_and_emits_a_scoped_json_report() {
    let output = invoke(&arguments(&fixture("runs.json")));
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON acceptance report");
    assert_eq!(report["decision"], "accepted");
    assert_eq!(report["suite_id"], "offline-venue-suite-v1");
    assert!(report["scope"]
        .as_str()
        .unwrap()
        .contains("Offline synthetic"));
    assert!(report["issues"].as_array().unwrap().is_empty());
    let cases = report["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 3);
    assert!(cases
        .iter()
        .all(|case| case["issues"].as_array().unwrap().is_empty()));
    let expected_pin: Value =
        serde_json::from_slice(&fs::read(fixture("artifact.json")).unwrap()).unwrap();
    assert_eq!(report["artifact"], expected_pin);
}

#[test]
fn incorrect_output_is_rejected_at_the_actual_candidate_row() {
    let output = invoke(&arguments(&fixture("runs.rejected.json")));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON rejection report");
    assert_eq!(report["decision"], "rejected");
    let case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_id"] == "table-listings")
        .unwrap();
    // The oracle lists grand-hall first; the actual candidate lists it second.
    assert_eq!(
        case["issues"],
        json!([{
            "code": "value_mismatch",
            "path": "output/venues/1/capacity"
        }])
    );
}

#[test]
fn malformed_truncated_and_wrongly_typed_receipts_fail_without_a_report() {
    let temp = TempDir::new();
    let complete = fs::read(fixture("runs.json")).unwrap();
    let payloads: &[&[u8]] = &[
        b"this is not JSON",
        &complete[..complete.len() - 3],
        br#"{"not":"an array of execution receipts"}"#,
    ];
    for (index, payload) in payloads.iter().enumerate() {
        let path = temp.write(&format!("malformed-{index}.json"), payload);
        assert_controlled_error(&invoke(&arguments(&path)), "invalid JSON/schema");
    }
}

#[test]
fn invalid_trusted_suite_is_configuration_error_not_candidate_rejection() {
    let temp = TempDir::new();
    let mut suite: Value =
        serde_json::from_slice(&fs::read(fixture("trusted/suite.json")).unwrap()).unwrap();
    suite["cases"] = json!([]);
    let suite_path = temp.write("empty-suite.json", serde_json::to_vec(&suite).unwrap());
    let mut args = arguments(&fixture("runs.json"));
    args[1] = suite_path.into_os_string();
    assert_controlled_error(&invoke(&args), "suite");
}

#[test]
fn missing_file_fails_without_a_report() {
    let temp = TempDir::new();
    assert_controlled_error(
        &invoke(&arguments(&temp.0.join("absent.json"))),
        "cannot open",
    );
}

#[test]
fn duplicate_unknown_and_missing_flags_are_rejected() {
    let base = arguments(&fixture("runs.json"));
    let mut duplicate = base.clone();
    duplicate.extend(["--runs".into(), fixture("runs.json").into_os_string()]);
    assert_controlled_error(&invoke(&duplicate), "duplicate argument");

    let mut unknown = base.clone();
    unknown.push("--approve-anyway".into());
    assert_controlled_error(&invoke(&unknown), "unknown argument");

    assert_controlled_error(&invoke(&[]), "missing --suite");
    assert_controlled_error(&invoke(&base[..4]), "missing --runs");
    assert_controlled_error(&invoke(&base[..5]), "missing path for --runs");
    assert_controlled_error(
        &invoke(&["--suite".into(), "--runs".into()]),
        "missing path for --suite",
    );
}

#[test]
fn help_describes_the_trust_boundary_and_exits_successfully() {
    for flag in ["--help", "-h"] {
        let output = invoke(&[flag.into()]);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Usage: genesis-verify"));
        assert!(stdout.contains("Offline JSON receipts do not prove"));
    }
}

#[test]
fn exact_file_limit_is_accepted_and_one_extra_byte_is_rejected() {
    let temp = TempDir::new();
    // Whitespace is valid JSON padding, so the result exercises the byte limit,
    // not an unrelated syntax or output-schema failure.
    let mut bytes = fs::read(fixture("runs.json")).unwrap();
    bytes.resize(8 * 1024 * 1024, b' ');
    let exact = temp.write("exact-limit.json", &bytes);
    let accepted = invoke(&arguments(&exact));
    assert_eq!(
        accepted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    bytes.push(b' ');
    let excessive = temp.write("over-limit.json", &bytes);
    assert_controlled_error(
        &invoke(&arguments(&excessive)),
        "exceeds the 8 MiB input limit",
    );
}

#[test]
fn deeply_nested_actor_output_is_a_controlled_parse_error() {
    let temp = TempDir::new();
    let mut runs: Value = serde_json::from_slice(&fs::read(fixture("runs.json")).unwrap()).unwrap();
    runs[0]["output"] = json!("DEEP_JSON_PLACEHOLDER");
    // Build nesting as text to avoid making this test depend on the test process's
    // recursive Value serialization or destruction behavior.
    let nested = format!("{}null{}", "[".repeat(512), "]".repeat(512));
    let raw = serde_json::to_string(&runs)
        .unwrap()
        .replace("\"DEEP_JSON_PLACEHOLDER\"", &nested);
    let path = temp.write("deep.json", raw);
    assert_controlled_error(&invoke(&arguments(&path)), "invalid JSON/schema");
}

#[test]
fn duplicate_actor_output_keys_cannot_hide_an_incorrect_value() {
    let temp = TempDir::new();
    let original = fs::read_to_string(fixture("runs.json")).unwrap();
    assert_eq!(original.matches("\"capacity\": 180").count(), 1);
    let ambiguous = original.replacen(
        "\"capacity\": 180",
        "\"capacity\": 999, \"capacity\": 180",
        1,
    );
    let path = temp.write("duplicate-output-key.json", ambiguous);
    assert_controlled_error(&invoke(&arguments(&path)), "invalid JSON/schema");
}

#[test]
fn duplicate_oracle_input_keys_are_rejected_before_digesting() {
    let temp = TempDir::new();
    let original = fs::read_to_string(fixture("trusted/suite.json")).unwrap();
    assert_eq!(original.matches("\"attendees\": 120").count(), 1);
    let ambiguous = original.replacen(
        "\"attendees\": 120",
        "\"attendees\": 999, \"attendees\": 120",
        1,
    );
    let path = temp.write("duplicate-input-key.json", ambiguous);
    let mut args = arguments(&fixture("runs.json"));
    args[1] = path.into_os_string();
    assert_controlled_error(&invoke(&args), "invalid JSON/schema");
}

#[test]
fn shipped_receipts_bind_to_real_snapshot_bytes_and_canonical_inputs() {
    let suite: OracleSuite =
        serde_json::from_slice(&fs::read(fixture("trusted/suite.json")).unwrap()).unwrap();
    let pin: ArtifactPin =
        serde_json::from_slice(&fs::read(fixture("artifact.json")).unwrap()).unwrap();
    let runs: Vec<ExecutionEvidence> =
        serde_json::from_slice(&fs::read(fixture("runs.json")).unwrap()).unwrap();
    let source_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(fixture("synthetic-artifact.txt")).unwrap())
    );
    assert_eq!(pin.source_sha256, source_hash);
    assert_eq!(suite.cases.len(), 3);
    assert_eq!(runs.len(), suite.cases.len());

    // These bytes are checked independently of the Actor output or oracle values.
    for case in suite.cases {
        let file_name = case.source.url.rsplit('/').next().unwrap();
        let bytes = fs::read(fixture(&format!("portal/{file_name}"))).unwrap();
        assert_eq!(case.source.sha256, format!("{:x}", Sha256::digest(&bytes)));
        let receipt = runs.iter().find(|run| run.case_id == case.case_id).unwrap();
        assert_eq!(receipt.source, case.source);
        assert_eq!(receipt.artifact, pin);
        assert_eq!(receipt.input_sha256, json_sha256(&case.input));
    }
}
