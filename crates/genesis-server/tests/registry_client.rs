//! Transport checks use loopback HTTP only; manifests are labeled test metadata.
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{Response, StatusCode};
use axum::Router;
use genesis_core::capability::{
    ActorArtifact, CapabilityManifest, CaseKind, CaseResult, VerificationReport,
};
use genesis_core::ids::{CapabilityId, OrgId};
use genesis_core::lifecycle::CapabilityState;
use genesis_server::error::ApiError;
use genesis_server::registry::{self, RegistryClient, RegistryState, SearchRequest};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const TOKEN: &str = "private-registry-token-canary";
const BODY_CANARY: &str = "private-provider-body-canary";
const LIMIT: usize = 1024 * 1024;

struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, task }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
struct Canned {
    status: StatusCode,
    body: String,
    chunked: bool,
    location: Option<String>,
    seen: Arc<Mutex<Vec<(String, String)>>>,
}
impl Canned {
    fn json(body: String) -> Self {
        Self {
            status: StatusCode::OK,
            body,
            chunked: false,
            location: None,
            seen: Arc::default(),
        }
    }
    async fn serve(&self) -> Server {
        Server::start(Router::new().fallback(canned).with_state(self.clone())).await
    }
}
async fn canned(State(state): State<Canned>, request: Request) -> Response<Body> {
    assert_eq!(
        request.headers()["authorization"],
        format!("Bearer {TOKEN}")
    );
    state
        .seen
        .lock()
        .unwrap()
        .push((request.method().to_string(), request.uri().to_string()));
    let body = if state.chunked {
        let chunks = state
            .body
            .into_bytes()
            .chunks(8192)
            .map(|chunk| Ok::<_, Infallible>(chunk.to_vec()))
            .collect::<Vec<_>>();
        Body::from_stream(futures::stream::iter(chunks))
    } else {
        Body::from(state.body)
    };
    let mut response = Response::builder().status(state.status);
    if let Some(location) = state.location {
        response = response.header("location", location);
    }
    response.body(body).unwrap()
}
fn search() -> SearchRequest {
    SearchRequest {
        query: "venue".into(),
        limit: 10,
        available_scopes: None,
    }
}
fn manifest() -> CapabilityManifest {
    let case = |name: &str, kind| CaseResult {
        name: name.into(),
        kind,
        passed: true,
        detail: String::new(),
    };
    CapabilityManifest {
        capability_id: CapabilityId::new("venue extractor/你好?x#%"),
        version: "1/β?tag#%".into(),
        publisher: OrgId::new("org-test"),
        source_digest: "a".repeat(64),
        intent: "Extract venue availability".into(),
        applicability_conditions: vec!["synthetic fixture only".into()],
        known_limitations: vec!["transport test, not live execution".into()],
        input_schema: json!({}),
        output_schema: json!({}),
        required_permissions: ["capability.venue_lookup".into()].into(),
        artifact: ActorArtifact {
            actor_id: "test-actor".into(),
            build_id: "test-build".into(),
            build_number: "0.1.1".into(),
        },
        role_template: None,
        responsibility_template: None,
        workflow_procedure: None,
        local_configuration_schema: Value::Null,
        acceptance_suite_ref: "test-suite".into(),
        verification: VerificationReport {
            passed: true,
            cases: vec![
                case("a", CaseKind::Success),
                case("b", CaseKind::Success),
                case("c", CaseKind::Unsupported),
            ],
            tested_environment: "loopback fixture".into(),
            limitations: vec![],
            oracle_ref: "test-oracle".into(),
            verified_at: chrono::Utc::now(),
        },
        provenance: vec![],
        state: CapabilityState::Approved,
        published_at: None,
    }
}
fn sanitized(error: &ApiError) {
    let serialized = format!("{:?} {}", error, serde_json::to_string(error).unwrap());
    for secret in [TOKEN, BODY_CANARY, "http://", "https://", "private-user"] {
        assert!(!serialized.contains(secret), "error disclosed {secret}");
    }
    assert!(error.details.is_null());
}

#[tokio::test]
async fn real_registry_roundtrip_preserves_unicode_encoded_segments_and_search_scopes() {
    let registry = RegistryState::open_in_memory(
        HashMap::from([(TOKEN.into(), OrgId::new("org-test"))]),
        vec![],
    )
    .unwrap();
    let server = Server::start(registry::router(registry)).await;
    let client = RegistryClient::new(format!("{}/", server.url), TOKEN);
    let original = manifest();
    let published = client.publish(&original).await.unwrap();
    assert_eq!(published.state, CapabilityState::Published);
    assert_eq!(client.publish(&original).await.unwrap(), published);
    assert_eq!(
        client
            .manifest(&original.capability_id, &original.version)
            .await
            .unwrap(),
        published
    );
    let mut request = search();
    request.limit = 100; // The existing inheritance driver requests this limit.
    let hits = client.search(&request).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].capability_id, original.capability_id);
    assert_eq!(hits[0].version, original.version);
    request.available_scopes = Some(vec![]);
    assert!(client.search(&request).await.unwrap().is_empty());
    let mut changed = original;
    changed.artifact.build_id = "different".into();
    let error = client.publish(&changed).await.unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert_eq!(error.error, "registry_error");
    sanitized(&error);
}

#[tokio::test]
async fn redirects_never_forward_credentials_or_repeat_any_operation() {
    let target = Canned::json("[]".into());
    let target_server = target.serve().await;
    let mut redirect = Canned::json(BODY_CANARY.into());
    redirect.status = StatusCode::TEMPORARY_REDIRECT;
    redirect.location = Some(format!("{}/redirected", target_server.url));
    let server = redirect.serve().await;
    let client = RegistryClient::new(&server.url, TOKEN);
    let item = manifest();
    for error in [
        client.search(&search()).await.unwrap_err(),
        client
            .manifest(&item.capability_id, &item.version)
            .await
            .unwrap_err(),
        client.publish(&item).await.unwrap_err(),
    ] {
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.error, "registry_redirect");
        sanitized(&error);
    }
    assert_eq!(redirect.seen.lock().unwrap().len(), 3);
    assert!(target.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn declared_and_chunked_oversize_are_bounded_before_json_decoding() {
    for chunked in [false, true] {
        let mut response = Canned::json(format!("[]{}", " ".repeat(LIMIT)));
        response.chunked = chunked;
        let server = response.serve().await;
        let error = RegistryClient::new(&server.url, TOKEN)
            .search(&search())
            .await
            .unwrap_err();
        assert_eq!(error.error, "registry_response_too_large");
        sanitized(&error);
        assert_eq!(response.seen.lock().unwrap().len(), 1);
    }
    let response = Canned::json(format!("[]{}", " ".repeat(LIMIT - 2)));
    let server = response.serve().await;
    assert!(RegistryClient::new(&server.url, TOKEN)
        .search(&search())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn malformed_and_nested_duplicate_json_never_become_manifests() {
    let item = manifest();
    let valid = serde_json::to_string(&item).unwrap();
    let duplicate = valid.replacen(
        "\"input_schema\":{}",
        &format!("\"input_schema\":{{\"{BODY_CANARY}\":1,\"{BODY_CANARY}\":2}}"),
        1,
    );
    assert_ne!(duplicate, valid);
    for body in [
        format!("{valid} {BODY_CANARY}"),
        duplicate,
        json!({"unexpected": BODY_CANARY}).to_string(),
    ] {
        let response = Canned::json(body);
        let server = response.serve().await;
        let client = RegistryClient::new(&server.url, TOKEN);
        let error = client
            .manifest(&item.capability_id, &item.version)
            .await
            .unwrap_err();
        assert_eq!(error.error, "registry_invalid_response");
        sanitized(&error);
    }
}

#[tokio::test]
async fn remote_failures_are_static_and_post_is_not_retried() {
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::NOT_FOUND,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let mut response = Canned::json(json!({"error":BODY_CANARY,"message":TOKEN,"details":{"private-user":"https://secret.example"}}).to_string());
        response.status = status;
        let server = response.serve().await;
        let error = RegistryClient::new(&server.url, TOKEN)
            .publish(&manifest())
            .await
            .unwrap_err();
        assert_eq!(error.status, status);
        assert_eq!(error.error, "registry_error");
        sanitized(&error);
        assert_eq!(response.seen.lock().unwrap().len(), 1);
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = RegistryClient::new(url, TOKEN)
        .search(&search())
        .await
        .unwrap_err();
    assert_eq!(error.error, "registry_unreachable");
    sanitized(&error);
}

#[tokio::test]
async fn malformed_origins_credentials_and_request_bounds_fail_before_network() {
    let response = Canned::json("[]".into());
    let server = response.serve().await;
    let authority = server.url.trim_start_matches("http://");
    for origin in [
        "not a URL".into(),
        "file:///tmp/registry".into(),
        format!("{}/prefix", server.url),
        format!("{}/..", server.url),
        format!("{}?secret={TOKEN}", server.url),
        format!("{}#secret", server.url),
        format!(" {}/", server.url),
        format!("http://private-user:{TOKEN}@{authority}"),
        format!("http://@{authority}"),
    ] {
        let error = RegistryClient::new(origin, TOKEN)
            .search(&search())
            .await
            .unwrap_err();
        assert_eq!(error.error, "registry_configuration");
        sanitized(&error);
    }
    for token in ["", "invalid\nprivate-registry-token-canary"] {
        let error = RegistryClient::new(&server.url, token)
            .search(&search())
            .await
            .unwrap_err();
        assert_eq!(error.error, "registry_configuration");
        sanitized(&error);
    }
    let client = RegistryClient::new(&server.url, TOKEN);
    for request in [
        SearchRequest {
            limit: 0,
            ..search()
        },
        SearchRequest {
            limit: 101,
            ..search()
        },
        SearchRequest {
            query: "x".repeat(4097),
            ..search()
        },
        SearchRequest {
            available_scopes: Some(vec!["x".into(); 129]),
            ..search()
        },
        SearchRequest {
            available_scopes: Some(vec![String::new()]),
            ..search()
        },
        SearchRequest {
            available_scopes: Some(vec!["x".repeat(257)]),
            ..search()
        },
    ] {
        let error = client.search(&request).await.unwrap_err();
        assert_eq!(error.error, "registry_invalid_request");
        sanitized(&error);
    }
    for (id, version) in [("", "1"), ("..", "1"), ("valid", "."), ("valid", "")] {
        let error = client
            .manifest(&CapabilityId::new(id), version)
            .await
            .unwrap_err();
        assert_eq!(error.error, "registry_invalid_request");
        sanitized(&error);
    }
    assert!(response.seen.lock().unwrap().is_empty());
}
