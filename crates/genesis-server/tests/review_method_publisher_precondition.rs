//! Inert source transfer against a local registry; no method or provider runs.
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_server::registry::{self, methods::MethodPublication, RegistryClient, RegistryState};
use serde_json::json;
use tokio::{net::TcpListener, task::JoinHandle};

struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Server {
    async fn start() -> Self {
        let state = RegistryState::open_in_memory(
            [
                ("a-token".into(), OrgId::new("org-a")),
                ("unicode-token".into(), OrgId::new("org/研究?&=β")),
            ]
            .into(),
            vec!["reader".into()],
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, registry::router(state))
                .await
                .unwrap();
        });
        Self { url, task }
    }
    async fn candidates(&self) -> serde_json::Value {
        reqwest::Client::new()
            .post(format!("{}/registry/v1/methods/search", self.url))
            .bearer_auth("reader")
            .json(&json!({"query":"fixture"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn publication() -> MethodPublication {
    let exact = |id: &str| ExactContentRef {
        domain: "fixture".into(),
        schema: "inert/1".into(),
        id: id.into(),
        version: "1".into(),
        sha256: "a".repeat(64),
    };
    let package = MethodPackage::new(
        "inert-method".into(),
        "1".into(),
        exact("runtime"),
        exact("protocol"),
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
        intent: "publisher fixture".into(),
        applicability: vec![],
    }
}

#[tokio::test]
async fn mismatched_client_expectation_cannot_write_under_either_publisher() {
    let server = Server::start().await;
    let client = RegistryClient::new(&server.url, "a-token");
    assert!(client
        .publish_method(&OrgId::new("org-b"), &publication())
        .await
        .is_err());
    assert_eq!(
        server.candidates().await["candidates"],
        json!([]),
        "an expectation mismatch must be refused before the token owner's publication is inserted"
    );
    let own = client
        .publish_method(&OrgId::new("org-a"), &publication())
        .await
        .unwrap();
    assert_eq!(own.publisher, OrgId::new("org-a"));
    assert_eq!(
        client
            .publish_method(&OrgId::new("org-a"), &publication())
            .await
            .unwrap(),
        own
    );
    assert_eq!(
        server.candidates().await["candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn raw_query_mismatch_is_forbidden_and_leaves_no_write() {
    let server = Server::start().await;
    let response = reqwest::Client::new()
        .post(format!("{}/registry/v1/methods", server.url))
        .query(&[("expected_publisher", "org-b")])
        .bearer_auth("a-token")
        .json(&publication())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    assert_eq!(server.candidates().await["candidates"], json!([]));
}

#[tokio::test]
async fn publisher_query_round_trips_unicode_and_reserved_characters() {
    let server = Server::start().await;
    let client = RegistryClient::new(&server.url, "unicode-token");
    let expected = OrgId::new("org/研究?&=β");
    let offer = client
        .publish_method(&expected, &publication())
        .await
        .unwrap();
    assert_eq!(offer.publisher, expected);
}

#[tokio::test]
async fn legacy_bare_post_remains_token_qualified_and_reader_cannot_publish() {
    let server = Server::start().await;
    let http = reqwest::Client::new();
    let url = format!("{}/registry/v1/methods", server.url);
    let denied = http
        .post(&url)
        .query(&[("expected_publisher", "org-a")])
        .bearer_auth("reader")
        .json(&publication())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(server.candidates().await["candidates"], json!([]));
    let response = http
        .post(url)
        .bearer_auth("a-token")
        .json(&publication())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["publisher"],
        "org-a"
    );
}
