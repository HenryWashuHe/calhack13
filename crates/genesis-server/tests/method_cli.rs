//! Real CLI + loopback HTTP, with inert source. No provider or method execution.
use axum::{extract::State, http::StatusCode, Router};
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_server::registry::{self, methods::*, RegistryState};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Output},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{net::TcpListener, task::JoinHandle};

const TOKEN: &str = "synthetic-cli-token";
const CANARY: &str = "private-response-or-input-canary";
struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        Self {
            url: format!("http://{}", listener.local_addr().unwrap()),
            task: tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
        }
    }
    async fn registry() -> Self {
        let state = RegistryState::open_in_memory(
            HashMap::from([(TOKEN.into(), OrgId::new("org-a"))]),
            vec!["b-reader".into()],
        )
        .unwrap();
        Self::start(registry::router(state)).await
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn reference(id: &str) -> ExactContentRef {
    ExactContentRef {
        domain: "fixture".into(),
        schema: "test/1".into(),
        id: id.into(),
        version: "1".into(),
        sha256: "a".repeat(64),
    }
}
fn publication() -> MethodPublication {
    let package = MethodPackage::new(
        "inert-method".into(),
        "1".into(),
        reference("runtime"),
        reference("protocol"),
        "method.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "method.py".into(),
            content: "# never executed\n".into(),
        }],
    )
    .unwrap();
    MethodPublication {
        reference: package.reference().unwrap(),
        package_json: package.artifact_json().unwrap(),
        intent: "Develop transport designs".into(),
        applicability: vec!["fixture tasks".into()],
    }
}
async fn cli(url: &str, token: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_genesis"));
    cmd.env_clear()
        .env("GENESIS_REGISTRY_URL", url)
        .args(["method"]);
    if let Some(token) = token {
        cmd.env("GENESIS_REGISTRY_TOKEN", token);
    }
    cmd.args(args);
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}
fn save(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn success(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains(TOKEN));
    serde_json::from_slice(&out.stdout).unwrap()
}
fn failure(out: Output) -> String {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    for secret in [TOKEN, CANARY] {
        assert!(!error.contains(secret), "{error}");
    }
    error
}

#[tokio::test]
async fn separate_cli_processes_publish_search_and_fetch_selected_exact_bytes() {
    let server = Server::registry().await;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    let p = input.to_str().unwrap();
    let publication = publication();
    save(&input, &publication);
    let offer = success(
        cli(
            &server.url,
            Some(TOKEN),
            &["publish", "--expected-publisher", "org-a", "--input", p],
        )
        .await,
    );
    assert_eq!(offer["publication"], json!(publication));
    failure(
        cli(
            &server.url,
            Some("b-reader"),
            &["publish", "--expected-publisher", "org-b", "--input", p],
        )
        .await,
    );
    save(
        &input,
        &json!({"query":"transport","exclude_publisher":"org-b"}),
    );
    let results = success(cli(&server.url, Some("b-reader"), &["search", "--input", p]).await);
    assert_eq!(results["candidates"].as_array().unwrap().len(), 1);
    let mut selected = results["candidates"][0].clone();
    assert_eq!(selected["publisher"], "org-a");
    save(&input, &selected);
    let before = std::fs::read(&input).unwrap();
    let fetched = success(cli(&server.url, Some("b-reader"), &["fetch", "--input", p]).await);
    assert_eq!(fetched, offer);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    selected["intent"] = json!("substituted selection");
    save(&input, &selected);
    assert!(
        failure(cli(&server.url, Some("b-reader"), &["fetch", "--input", p]).await)
            .contains("registry_invalid_response")
    );
}

async fn rejected(State(seen): State<Arc<AtomicUsize>>) -> (StatusCode, &'static str) {
    seen.fetch_add(1, Ordering::SeqCst);
    (StatusCode::INTERNAL_SERVER_ERROR, CANARY)
}

#[tokio::test]
async fn malformed_inputs_and_credentials_never_reach_the_network() {
    let seen = Arc::new(AtomicUsize::new(0));
    let server = Server::start(Router::new().fallback(rejected).with_state(seen.clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    let p = input.to_str().unwrap();
    for raw in [
        format!(r#"{{"query":"{CANARY}","query":"again"}}"#),
        format!(r#"{{"query":"ok","extra":"{CANARY}"}}"#),
        format!(r#"{{"query":"{}"}}"#, "x".repeat(1024 * 1024)),
    ] {
        std::fs::write(&input, raw).unwrap();
        assert!(
            failure(cli(&server.url, Some(TOKEN), &["search", "--input", p]).await)
                .contains("method input must")
        );
    }
    save(&input, &json!({"query":"ok"}));
    assert!(
        failure(cli(&server.url, None, &["search", "--input", p]).await)
            .contains("GENESIS_REGISTRY_TOKEN is required")
    );
    assert!(failure(
        cli(
            "https://example.invalid/private-path",
            Some(TOKEN),
            &["search", "--input", p]
        )
        .await
    )
    .contains("registry_configuration"));
    save(&input, &json!({"query":""}));
    assert!(
        failure(cli(&server.url, Some(TOKEN), &["search", "--input", p]).await)
            .contains("registry_invalid_request")
    );
    assert_eq!(seen.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_publication_is_one_request_with_no_remote_body_or_success_output() {
    let seen = Arc::new(AtomicUsize::new(0));
    let server = Server::start(Router::new().fallback(rejected).with_state(seen.clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    save(&input, &publication());
    assert!(failure(
        cli(
            &server.url,
            Some(TOKEN),
            &[
                "publish",
                "--expected-publisher",
                "org-a",
                "--input",
                input.to_str().unwrap(),
            ]
        )
        .await
    )
    .contains("registry_error"));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn publisher_expectation_failure_can_follow_a_committed_remote_write() {
    let server = Server::registry().await;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    let p = input.to_str().unwrap();
    save(&input, &publication());
    assert!(failure(
        cli(
            &server.url,
            Some(TOKEN),
            &[
                "publish",
                "--expected-publisher",
                "wrong-account",
                "--input",
                p,
            ]
        )
        .await
    )
    .contains("registry_invalid_response"));
    save(&input, &json!({"query":"transport"}));
    let results = success(cli(&server.url, Some("b-reader"), &["search", "--input", p]).await);
    assert_eq!(results["candidates"][0]["publisher"], "org-a");
}
