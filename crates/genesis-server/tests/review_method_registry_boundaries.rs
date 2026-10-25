//! Independent boundary regressions for the method registry (a mutation pass
//! found these unpinned). Loopback only; no method is executed.
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_server::registry::{self, methods::MethodPublication, RegistryState};
use serde_json::{json, Value};
use tokio::{net::TcpListener, task::JoinHandle};

struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Server {
    async fn start(path: &std::path::Path) -> Self {
        let state = RegistryState::open(
            path,
            [
                ("a-token".into(), OrgId::new("a")),
                ("b-token".into(), OrgId::new("b")),
            ]
            .into(),
            vec!["reader".into()],
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/registry/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, registry::router(state))
                .await
                .unwrap()
        });
        Self { url, task }
    }
    async fn call(&self, route: &str, token: &str, value: Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("{}/{}", self.url, route))
            .bearer_auth(token)
            .json(&value)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn exact(id: &str) -> ExactContentRef {
    ExactContentRef {
        domain: "genesis".into(),
        schema: "fixture/1".into(),
        id: id.into(),
        version: "1".into(),
        sha256: "a".repeat(64),
    }
}
fn publication(version: &str, content: &str) -> MethodPublication {
    let package = MethodPackage::new(
        "search/你好?%".into(),
        version.into(),
        exact("runtime"),
        exact("protocol"),
        "main.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "main.py".into(),
            content: content.into(),
        }],
    )
    .unwrap();
    MethodPublication {
        reference: package.reference().unwrap(),
        package_json: package.artifact_json().unwrap(),
        intent: "Explore rough terrain".into(),
        applicability: vec!["100% fixture; no execution".into()],
    }
}

#[tokio::test]
async fn search_reports_truncation_only_when_more_matches_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Server::start(&tmp.path().join("registry.db")).await;
    for v in ["1", "2", "3"] {
        let p = publication(v, &format!("# inert fixture {v}\n"));
        assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 201);
    }
    let (status, exact_fit) = server
        .call(
            "methods/search",
            "reader",
            json!({"query":"terrain","limit":3}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(exact_fit["candidates"].as_array().unwrap().len(), 3);
    assert_eq!(exact_fit["truncated"], false, "{exact_fit}");
    let (_, short) = server
        .call(
            "methods/search",
            "reader",
            json!({"query":"terrain","limit":2}),
        )
        .await;
    assert_eq!(short["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(short["truncated"], true, "{short}");
}

#[tokio::test]
async fn sixteen_applicability_entries_publish_and_seventeen_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Server::start(&tmp.path().join("registry.db")).await;
    let mut p = publication("1", "# inert fixture\n");
    p.applicability = (0..17).map(|i| format!("claim {i}")).collect();
    assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 400);
    p.applicability.pop();
    assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 201);
}

#[tokio::test]
async fn fetch_refuses_malformed_lookups_before_lookup() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Server::start(&tmp.path().join("registry.db")).await;
    let p = publication("1", "# inert fixture\n");
    let mut reference = json!(p.reference);
    // A 256-byte id is well-formed (absent, 404); 257 bytes is malformed (400).
    reference["id"] = json!("x".repeat(256));
    let ok = json!({"publisher":"a","reference":reference.clone()});
    assert_eq!(server.call("methods/fetch", "reader", ok).await.0, 404);
    reference["id"] = json!("x".repeat(257));
    let long = json!({"publisher":"a","reference":reference});
    assert_eq!(server.call("methods/fetch", "reader", long).await.0, 400);
    let padded = json!({"publisher":" a","reference":p.reference});
    assert_eq!(server.call("methods/fetch", "reader", padded).await.0, 400);
}
