//! Independent transport regressions against server source 2039c5a4; inert bytes only.
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_server::registry::{
    self,
    methods::{MethodCandidate, MethodPublication, MAX_METHOD_PACKAGE_BYTES},
    RegistryState,
};
use serde_json::{json, Value};
use tokio::{net::TcpListener, task::JoinHandle};

struct Server(String, JoinHandle<()>);
impl Server {
    async fn start() -> Self {
        let state = RegistryState::open_in_memory(
            [("publisher-token".into(), OrgId::new("publisher"))].into(),
            vec![],
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/registry/v1/methods",
            listener.local_addr().unwrap()
        );
        Self(
            url,
            tokio::spawn(async move {
                axum::serve(listener, registry::router(state))
                    .await
                    .unwrap()
            }),
        )
    }
    async fn raw(&self, route: &str, body: String) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("{}{route}", self.0))
            .bearer_auth("publisher-token")
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, route: &str, body: Value) -> (u16, Value) {
        self.raw(route, body.to_string()).await
    }
    async fn fetch(&self, publication: &MethodPublication) -> (u16, Value) {
        self.post(
            "/fetch",
            json!({"publisher":"publisher","reference":publication.reference}),
        )
        .await
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.1.abort();
    }
}
fn exact(id: String) -> ExactContentRef {
    ExactContentRef {
        domain: "genesis".into(),
        schema: "inert/1".into(),
        id,
        version: "1".into(),
        sha256: "a".repeat(64),
    }
}
fn package(content: String, dependencies: Vec<ExactContentRef>) -> MethodPackage {
    MethodPackage::new(
        "boundary-method".into(),
        "1".into(),
        exact("runtime".into()),
        exact("protocol".into()),
        "main.py".into(),
        dependencies,
        vec![MethodSourceFile {
            path: "main.py".into(),
            content,
        }],
    )
    .unwrap()
}
fn publication(package: &MethodPackage) -> MethodPublication {
    MethodPublication {
        reference: package.reference().unwrap(),
        package_json: package.artifact_json().unwrap(),
        intent: "inert boundary".into(),
        applicability: vec![],
    }
}
fn candidate_size(p: &MethodPublication, package: &MethodPackage) -> usize {
    serde_json::to_vec(&MethodCandidate {
        publisher: OrgId::new("publisher"),
        reference: p.reference.clone(),
        intent: p.intent.clone(),
        applicability: p.applicability.clone(),
        runtime: package.runtime().clone(),
        protocol: package.protocol().clone(),
        dependencies: package.dependencies().to_vec(),
    })
    .unwrap()
    .len()
}

#[tokio::test]
async fn exact_package_transport_limit_roundtrips_but_one_more_byte_never_persists() {
    let server = Server::start().await;
    let overhead = package(String::new(), vec![])
        .artifact_json()
        .unwrap()
        .len();
    let allowed = publication(&package(
        "x".repeat(MAX_METHOD_PACKAGE_BYTES - overhead),
        vec![],
    ));
    let excess = publication(&package(
        "x".repeat(MAX_METHOD_PACKAGE_BYTES - overhead + 1),
        vec![],
    ));
    assert_eq!(allowed.package_json.len(), MAX_METHOD_PACKAGE_BYTES);
    assert_eq!(excess.package_json.len(), MAX_METHOD_PACKAGE_BYTES + 1);
    assert_eq!(server.post("", json!(excess)).await.0, 400);
    assert_eq!(server.fetch(&excess).await.0, 404);
    assert_eq!(server.post("", json!(allowed)).await.0, 201);
    let (status, fetched) = server.fetch(&allowed).await;
    assert_eq!(status, 200);
    assert_eq!(fetched["publication"], json!(allowed));
    assert_eq!(server.fetch(&excess).await.0, 409);
}

#[tokio::test]
async fn dependency_and_serialized_candidate_limits_reject_valid_core_packages_atomically() {
    let server = Server::start().await;
    let dependencies: Vec<_> = (0..17).map(|i| exact(format!("dependency-{i}"))).collect();
    let excess = publication(&package("# inert".into(), dependencies.clone()));
    assert_eq!(server.post("", json!(excess)).await.0, 400);
    assert_eq!(server.fetch(&excess).await.0, 404);
    let allowed = publication(&package("# inert".into(), dependencies[..16].to_vec()));
    assert_eq!(server.post("", json!(allowed)).await.0, 201);

    let server = Server::start().await;
    let dependencies = (0..16)
        .map(|i| exact(format!("{i}-{}", "x".repeat(200))))
        .collect();
    let source = package("# inert".into(), dependencies);
    let mut allowed = publication(&source);
    // Construct the precise serialized candidate boundary, independent of inner-source size.
    let mut found = false;
    for count in 0..=16 {
        allowed.intent = "i".into();
        allowed.applicability = vec!["a".repeat(256); count];
        let base = candidate_size(&allowed, &source);
        if (7169..=8192).contains(&base) {
            allowed.intent = "i".repeat(1 + 8192 - base);
            if allowed.intent.len() < 1024 {
                found = true;
                break;
            }
        }
    }
    assert!(found);
    assert_eq!(candidate_size(&allowed, &source), 8192);
    assert!(allowed.package_json.len() < MAX_METHOD_PACKAGE_BYTES);
    let mut excess = allowed.clone();
    excess.intent.push('i');
    assert!(excess.intent.len() <= 1024);
    assert_eq!(candidate_size(&excess, &source), 8193);
    assert_eq!(server.post("", json!(excess)).await.0, 400);
    assert_eq!(server.fetch(&excess).await.0, 404);
    assert_eq!(server.post("", json!(allowed)).await.0, 201);
    let (status, result) = server.post("/search", json!({"query":"i"})).await;
    assert_eq!(status, 200);
    assert_eq!(result["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(
        serde_json::to_vec(&result["candidates"][0]).unwrap().len(),
        8192
    );
    assert_eq!(
        server.fetch(&allowed).await.1["publication"],
        json!(allowed)
    );
}

#[tokio::test]
async fn duplicate_transport_fields_and_stale_package_metadata_leave_no_publication() {
    let server = Server::start().await;
    let original = publication(&package("# inert".into(), vec![exact("dep".into())]));
    let encoded = serde_json::to_string(&original).unwrap();
    for duplicate in [
        format!("{{\"intent\":\"inert boundary\",{}", &encoded[1..]),
        encoded.replacen(
            "\"sha256\":",
            &format!("\"sha256\":\"{}\",\"sha256\":", original.reference.sha256),
            1,
        ),
    ] {
        assert_eq!(server.raw("", duplicate).await.0, 422);
        assert_eq!(server.fetch(&original).await.0, 404);
    }
    for field in ["protocol", "dependencies"] {
        let mut altered = original.clone();
        let mut envelope: Value = serde_json::from_str(&altered.package_json).unwrap();
        let identity = if field == "dependencies" {
            &mut envelope["package"][field][0]
        } else {
            &mut envelope["package"][field]
        };
        identity["sha256"] = json!("b".repeat(64));
        altered.package_json = genesis_core::digest::canonical_json(&envelope);
        assert_eq!(server.post("", json!(altered)).await.0, 400);
        assert_eq!(server.fetch(&original).await.0, 404);
    }
    assert_eq!(server.post("", json!(original)).await.0, 201);
    assert_eq!(
        server.fetch(&original).await.1["publication"],
        json!(original)
    );
}
