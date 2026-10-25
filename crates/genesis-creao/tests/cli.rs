//! Local CLI checks; all provider credentials are absent and no HTTP is used.
use genesis_creao::{CreateAgentRequest, OperationPurpose, OperationRequest, OperationSpec};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_genesis-creao"));
    for name in [
        "CREAO_ACCOUNT_API_KEY",
        "CREAO_V2_API_KEY",
        "CREAO_API_VERSION",
        "CREAO_TRIGGER_API_KEY",
        "CREAO_TRIGGER_APP_ID",
        "CREAO_TRIGGER_RUNS_URL",
    ] {
        command.env_remove(name);
    }
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
    assert_eq!(first["api_version"], "v1");
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
fn v2_prepared_and_status_summaries_identify_the_saved_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.sqlite");
    let input = dir.path().join("spec.json");
    let mut hosted = spec();
    hosted.api_version = genesis_creao::ApiVersion::V2;
    std::fs::write(&input, serde_json::to_vec(&hosted).unwrap()).unwrap();
    assert_eq!(output(prepare(&db, &input))["api_version"], "v2");
    let status = output(
        command()
            .args(["status", "--db"])
            .arg(&db)
            .args(["--org", "org_a", "--operation", "cli-create"])
            .output()
            .unwrap(),
    );
    assert_eq!(status["api_version"], "v2");
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
    assert!(String::from_utf8_lossy(&result.stderr).contains("CREAO_TRIGGER_API_KEY"));
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
fn blank_api_version_defaults_to_trigger_for_offline_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.sqlite");
    let quota = dir.path().join("account.sqlite");
    let input = dir.path().join("spec.json");
    std::fs::write(&input, serde_json::to_vec(&trigger_spec()).unwrap()).unwrap();
    output(prepare(&db, &input));
    for version in ["", "  "] {
        let recovered = output(
            command()
                .args(["recover", "--db"])
                .arg(&db)
                .args([
                    "--org",
                    "org_a",
                    "--account",
                    "shared-demo-account",
                    "--quota-db",
                ])
                .arg(&quota)
                .env("CREAO_TRIGGER_API_KEY", "capi_synthetic_test_only")
                .env("CREAO_TRIGGER_APP_ID", APP)
                .env("CREAO_API_VERSION", version)
                .output()
                .unwrap(),
        );
        assert_eq!(recovered["recovered"], 0);
        assert_eq!(recovered["provider_requests"], 0);
    }
}
#[test]
fn legacy_agent_reconciliation_is_disabled_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.sqlite");
    let quota = dir.path().join("account.sqlite");
    let input = dir.path().join("spec.json");
    std::fs::write(&input, serde_json::to_vec(&spec()).unwrap()).unwrap();
    let prepared = output(prepare(&db, &input));
    let result = command()
        .args(["reconcile-agent", "--db"])
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
        .args(["--agent-id", "00000000-0000-4000-8000-000000000001"])
        .env("CREAO_ACCOUNT_API_KEY", "cr_sk_synthetic_test_only")
        .env("CREAO_API_VERSION", "v1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("Developer agent reconciliation is disabled"));
    let current = command()
        .args(["status", "--db"])
        .arg(&db)
        .args(["--org", "org_a", "--operation", "cli-create"])
        .output()
        .unwrap();
    assert_eq!(output(current), prepared);
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

const APP: &str = "00000000-0000-4000-8000-000000000001";
fn trigger_spec() -> OperationSpec {
    let mut value = spec();
    value.api_version = genesis_creao::ApiVersion::Trigger;
    value.request = OperationRequest::CreateRun(genesis_creao::RunRequest {
        agent_id: APP.into(),
        input: serde_json::json!({"task":"bootstrap", "role":"test-only"}),
        model: None,
        conversation_id: None,
        workspace_id: None,
    });
    value
}

#[test]
fn legacy_execution_is_disabled_and_preserves_its_offline_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("old.sqlite");
    let quota = dir.path().join("quota.sqlite");
    let input = dir.path().join("old.json");
    let mut legacy = spec();
    legacy.api_version = genesis_creao::ApiVersion::V2;
    std::fs::write(&input, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let before = output(prepare(&db, &input));
    for version in ["v2", "trigger"] {
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
            .env("CREAO_API_VERSION", version)
            .env("CREAO_TRIGGER_API_KEY", "capi_synthetic_test_only")
            .env("CREAO_TRIGGER_APP_ID", APP)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("capi_synthetic_test_only"));
        assert_eq!(
            output(
                command()
                    .args(["status", "--db"])
                    .arg(&db)
                    .args(["--org", "org_a", "--operation", "cli-create"])
                    .output()
                    .unwrap()
            ),
            before
        );
    }
}

#[test]
fn trigger_prepared_intent_is_explicit_and_bad_url_never_advances_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("trigger.sqlite");
    let quota = dir.path().join("quota.sqlite");
    let input = dir.path().join("trigger.json");
    std::fs::write(&input, serde_json::to_vec(&trigger_spec()).unwrap()).unwrap();
    let before = output(prepare(&db, &input));
    assert_eq!(before["api_version"], "trigger");
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
        .env("CREAO_TRIGGER_API_KEY", "capi_synthetic_test_only")
        .env("CREAO_TRIGGER_APP_ID", APP)
        .env(
            "CREAO_TRIGGER_RUNS_URL",
            "https://PRIVATE_CANARY@invalid.example/runs",
        )
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("PRIVATE_CANARY"));
    assert_eq!(
        output(
            command()
                .args(["status", "--db"])
                .arg(&db)
                .args(["--org", "org_a", "--operation", "cli-create"])
                .output()
                .unwrap()
        ),
        before
    );
}
