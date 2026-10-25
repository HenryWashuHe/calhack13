//! Authenticated loopback source transfer only; no method is executed.
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_server::registry::{
    self,
    methods::{MethodPublication, MAX_METHOD_PACKAGE_BYTES},
    RegistryState,
};
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
fn lookup(p: &MethodPublication, publisher: &str) -> Value {
    json!({"publisher":publisher,"reference":p.reference})
}
fn query() -> Value {
    json!({"query":"terrain","limit":10})
}

#[tokio::test]
async fn authenticated_publication_is_immutable_publisher_qualified_and_durable() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("registry.db");
    let server = Server::start(&db).await;
    let p = publication("1/β", "# inert fixture\n");
    for token in ["unknown", "reader"] {
        assert_eq!(server.call("methods", token, json!(p)).await.0, 401);
    }
    let (status, saved) = server.call("methods", "a-token", json!(p)).await;
    assert_eq!(status, 201);
    assert_eq!(saved["publisher"], "a");
    assert_eq!(saved["publication"], json!(p));
    assert_eq!(
        server.call("methods", "a-token", json!(p)).await,
        (200, saved.clone())
    );
    let mut changed = p.clone();
    changed.intent.push_str(" edited");
    assert_eq!(
        server.call("methods", "a-token", json!(changed)).await.0,
        409
    );
    let other = publication("1/β", "# different inert bytes\n");
    assert_eq!(server.call("methods", "a-token", json!(other)).await.0, 409);
    assert_eq!(server.call("methods", "b-token", json!(other)).await.0, 201);
    for token in ["reader", "a-token", "b-token"] {
        assert_eq!(
            server.call("methods/fetch", token, lookup(&p, "a")).await,
            (200, saved.clone())
        );
    }
    assert_eq!(
        server
            .call("methods/fetch", "reader", lookup(&p, "b"))
            .await
            .0,
        409
    );
    assert_eq!(
        server
            .call("methods/fetch", "reader", lookup(&p, "absent"))
            .await
            .0,
        404
    );
    assert_eq!(
        server
            .call("methods/fetch", "unknown", lookup(&p, "a"))
            .await
            .0,
        401
    );
    assert_eq!(
        server.call("methods/search", "unknown", query()).await.0,
        401
    );
    drop(server);
    let reopened = Server::start(&db).await;
    assert_eq!(
        reopened
            .call("methods/fetch", "reader", lookup(&p, "a"))
            .await,
        (200, saved)
    );
    let next = publication("2", "# new version\n");
    assert_eq!(
        reopened.call("methods", "a-token", json!(next)).await.0,
        201
    );
    assert_eq!(
        reopened
            .call("methods/fetch", "reader", lookup(&p, "a"))
            .await
            .1["publication"],
        json!(p)
    );
}

#[tokio::test]
async fn search_is_source_free_exact_filtered_bounded_and_never_implies_absence() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Server::start(&tmp.path().join("db")).await;
    for version in ["1", "2", "3"] {
        assert_eq!(
            server
                .call(
                    "methods",
                    "a-token",
                    json!(publication(version, "# secret fixture source\n"))
                )
                .await
                .0,
            201
        );
    }
    assert_eq!(
        server
            .call("methods", "b-token", json!(publication("1", "# source\n")))
            .await
            .0,
        201
    );
    let mut q = query();
    q["limit"] = json!(2);
    let (status, found) = server.call("methods/search", "reader", q).await;
    assert_eq!(status, 200);
    assert_eq!(found["truncated"], true);
    assert_eq!(found["candidates"].as_array().unwrap().len(), 2);
    assert!(!found.to_string().contains("package_json"));
    assert!(!found.to_string().contains("secret fixture"));
    assert_eq!(found["candidates"][0]["reference"]["version"], "1");
    let mut q = query();
    q["runtime"] = json!(exact("runtime"));
    q["protocol"] = json!(exact("protocol"));
    q["exclude_publisher"] = json!("a");
    let found = server.call("methods/search", "reader", q.clone()).await.1;
    assert_eq!(found["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(found["candidates"][0]["publisher"], "b");
    assert_eq!(found["truncated"], false);
    q["runtime"]["sha256"] = json!("b".repeat(64));
    assert_eq!(
        server.call("methods/search", "reader", q).await.1["candidates"],
        json!([])
    );
    for query in ["TERRAIN", "100%"] {
        assert_eq!(
            server
                .call("methods/search", "reader", json!({"query":query}))
                .await
                .1["candidates"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
    assert_eq!(
        server
            .call("methods/search", "reader", json!({"query":"_"}))
            .await
            .1["candidates"],
        json!([])
    );
    for bad in [
        json!({"query":""}),
        json!({"query":"terrain","limit":0}),
        json!({"query":"terrain","limit":51}),
        json!({"query":"terrain","accepted":true}),
    ] {
        assert!(server.call("methods/search", "reader", bad).await.0 >= 400);
    }
}

#[tokio::test]
async fn malformed_changed_noncanonical_and_overbound_source_cannot_publish() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Server::start(&tmp.path().join("db")).await;
    let p = publication("1", "# inert\n");
    let mut wrong_ref = json!(p);
    wrong_ref["reference"]["id"] = json!("foreign");
    let mut wrong_hash = json!(p);
    wrong_hash["reference"]["sha256"] = json!("f".repeat(64));
    let mut wrong_bytes = json!(p);
    wrong_bytes["package_json"] = json!(format!("{} ", p.package_json));
    let mut unknown = json!(p);
    unknown["publisher"] = json!("b");
    let mut unbounded = json!(p);
    unbounded["package_json"] = json!("x".repeat(MAX_METHOD_PACKAGE_BYTES + 1));
    let mut metadata = json!(p);
    metadata["applicability"] = json!(vec!["bound"; 17]);
    for bad in [
        wrong_ref,
        wrong_hash,
        wrong_bytes,
        unknown,
        unbounded,
        metadata,
    ] {
        assert!(server.call("methods", "a-token", bad).await.0 >= 400);
        assert_eq!(
            server.call("methods/search", "reader", query()).await.1["candidates"],
            json!([])
        );
    }
    assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 201);
    let mut wrong_schema = lookup(&p, "a");
    wrong_schema["reference"]["schema"] = json!("other/1");
    assert_eq!(
        server.call("methods/fetch", "reader", wrong_schema).await.0,
        409
    );
}

#[tokio::test]
async fn separate_registry_connections_cannot_replace_a_concurrent_publication() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("db");
    let a = Server::start(&db).await;
    let b = Server::start(&db).await;
    let p = publication("1", "# first\n");
    let other = publication("1", "# second\n");
    let (x, y) = tokio::join!(
        a.call("methods", "a-token", json!(p)),
        b.call("methods", "a-token", json!(other))
    );
    let mut statuses = [x.0, y.0];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    let winning = if x.0 == 201 { p } else { other };
    assert_eq!(
        a.call("methods/fetch", "reader", lookup(&winning, "a"))
            .await
            .1["publication"],
        json!(winning)
    );
}
