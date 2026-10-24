//! Local CLI checks; all provider credentials are absent and no HTTP is used.
use genesis_creao::{CreateAgentRequest, OperationPurpose, OperationRequest, OperationSpec};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_genesis-creao"));
    command.env_remove("CREAO_ACCOUNT_API_KEY");
    command
}
fn spec() -> OperationSpec {
    OperationSpec {
        schema_version: 1,
        api_version: genesis_creao::ApiVersion::V1,
        operation_id: "cli-create".into(),
        org_id: "org_a".into(),
        role_id: "role_monitor".into(),
        account_namespace: "shared-demo-account".into(),
        purpose: OperationPurpose::Provision {
            change_id: "change_1".into(),
            config_digest: "a".repeat(64),
        },
        request: OperationRequest::CreateAgent(CreateAgentRequest {
            input: "Private role details must not appear in status output".into(),
            model: None,
            conversation_id: None,
        }),
    }
}
fn prepare(db: &Path, input: &Path) -> Output {
    command()
        .arg("prepare")
        .arg("--db")
        .arg(db)
        .args(["--org", "org_a", "--spec"])
        .arg(input)
        .output()
        .unwrap()
}
fn output(result: Output) -> Value {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
#[test]
fn prepare_and_status_are_offline_idempotent_and_do_not_disclose_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.sqlite");
    let input = dir.path().join("spec.json");
    std::fs::write(&input, serde_json::to_vec(&spec()).unwrap()).unwrap();
    let first = output(prepare(&db, &input));
    let second = output(prepare(&db, &input));
    assert_eq!(first, second);
    assert_eq!(first["phase"], "prepared");
    assert_eq!(first["readiness_verified"], false);
    assert_eq!(first["domain_verified"], false);
    assert!(first.get("request").is_none());
    let current = command()
        .args(["status", "--db"])
        .arg(&db)
        .args(["--org", "org_a", "--operation", "cli-create"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&current.stdout).contains("Private role details"));
    assert_eq!(output(current), first);
    let mut changed = spec();
    if let OperationRequest::CreateAgent(request) = &mut changed.request {
        request.input = "Different role".into();
    }
    std::fs::write(&input, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(!prepare(&db, &input).status.success());
}
#[test]
fn malformed_or_duplicate_spec_fails_before_creating_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.sqlite");
    let input = dir.path().join("bad.json");
    let encoded = serde_json::to_string(&spec()).unwrap();
    std::fs::write(&input, encoded.replacen("{", "{\"schema_version\":1,", 1)).unwrap();
    assert!(!prepare(&db, &input).status.success());
    assert!(!db.exists());
    std::fs::write(&input, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    assert!(!prepare(&db, &input).status.success());
    assert!(!db.exists());
}
#[test]
fn step_requires_environment_credentials_without_mutating_prepared_record() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.sqlite");
    let quota = dir.path().join("account.sqlite");
    let input = dir.path().join("spec.json");
    std::fs::write(&input, serde_json::to_vec(&spec()).unwrap()).unwrap();
    let first = output(prepare(&db, &input));
    let result = command()
        .args(["step", "--db"])
        .arg(&db)
        .args([
            "--org",
            "org_a",
            "--operation",
            "cli-create",
            "--account",
            "shared-demo-account",
            "--quota-db",
        ])
        .arg(&quota)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("CREAO_ACCOUNT_API_KEY"));
    assert!(!quota.exists());
    assert_eq!(
        output(
            command()
                .args(["status", "--db"])
                .arg(&db)
                .args(["--org", "org_a", "--operation", "cli-create"])
                .output()
                .unwrap()
        ),
        first
    );
}
#[test]
fn token_endpoint_and_blind_retry_flags_are_not_supported() {
    for flag in ["--token", "--api-key", "--base-url", "--force-retry"] {
        let result = command()
            .args(["step", flag, "forbidden"])
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
    let result = command().arg("--help").output().unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout)
        .contains("native completion is not organization authority"));
}
