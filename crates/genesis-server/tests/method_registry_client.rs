//! Loopback transport and source-only revision preparation; fixture code is never executed.
use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    Router,
};
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    digest::{canonical_json, canonical_sha256},
    ids::OrgId,
};
use genesis_forge::{
    method_revision::MethodRevisionSpec,
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisPlan, PublicDocument},
    text_generation::{Sampling, TextModelSettings},
};
use genesis_server::{
    error::ApiError,
    registry::{self, methods::*, RegistryClient, RegistryState},
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, task::JoinHandle};

const TOKEN: &str = "synthetic-private-token";
const CANARY: &str = "private-response-canary";
struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        Self {
            url,
            task: tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(Clone)]
struct Canned {
    body: String,
    status: StatusCode,
    location: Option<String>,
    chunked: bool,
    seen: Arc<Mutex<Vec<String>>>,
}
impl Canned {
    fn new(body: String) -> Self {
        Self {
            body,
            status: StatusCode::OK,
            location: None,
            chunked: false,
            seen: Arc::default(),
        }
    }
    async fn serve(&self) -> Server {
        Server::start(Router::new().fallback(canned).with_state(self.clone())).await
    }
}
async fn canned(State(s): State<Canned>, request: Request) -> Response<Body> {
    assert_eq!(
        request.headers()["authorization"],
        format!("Bearer {TOKEN}")
    );
    assert_eq!(request.method(), "POST");
    s.seen.lock().unwrap().push(request.uri().to_string());
    let body = if s.chunked {
        let chunks = s
            .body
            .into_bytes()
            .chunks(8192)
            .map(|c| Ok::<_, Infallible>(c.to_vec()))
            .collect::<Vec<_>>();
        Body::from_stream(futures::stream::iter(chunks))
    } else {
        Body::from(s.body)
    };
    let mut response = Response::builder().status(s.status);
    if let Some(location) = s.location {
        response = response.header("location", location);
    }
    response.body(body).unwrap()
}
fn document(id: &str, value: Value) -> PublicDocument {
    PublicDocument {
        reference: ExactContentRef {
            domain: "fixture".into(),
            schema: "public-text/1".into(),
            id: id.into(),
            version: "1".into(),
            sha256: canonical_sha256(&value),
        },
        text: canonical_json(&value),
    }
}
fn protocol() -> PublicDocument {
    document("protocol", json!({"interface":"fixture-only"}))
}
fn package() -> MethodPackage {
    MethodPackage::new(
        "method-a".into(),
        "1".into(),
        document("runtime", json!({"runtime":"not executed"})).reference,
        protocol().reference,
        "method.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "method.py".into(),
            content: "# inert source fixture\n".into(),
        }],
    )
    .unwrap()
}
fn offer() -> MethodOffer {
    let p = package();
    MethodOffer {
        publisher: OrgId::new("org-a"),
        publication: MethodPublication {
            reference: p.reference().unwrap(),
            package_json: p.artifact_json().unwrap(),
            intent: "Develop TRANSPORT experiments".into(),
            applicability: vec!["public fixture tasks".into()],
        },
    }
}
fn candidate() -> MethodCandidate {
    let o = offer();
    let p = package();
    MethodCandidate {
        publisher: o.publisher,
        reference: o.publication.reference,
        intent: o.publication.intent,
        applicability: o.publication.applicability,
        runtime: p.runtime().clone(),
        protocol: p.protocol().clone(),
        dependencies: vec![],
    }
}
fn query() -> MethodSearch {
    MethodSearch {
        query: "transport".into(),
        limit: 2,
        runtime: Some(package().runtime().clone()),
        protocol: Some(package().protocol().clone()),
        exclude_publisher: Some(OrgId::new("org-b")),
    }
}
fn lookup() -> MethodLookup {
    let o = offer();
    MethodLookup {
        publisher: o.publisher,
        reference: o.publication.reference,
    }
}
fn sanitized(error: &ApiError) {
    let rendered = format!("{error:?} {}", serde_json::to_string(error).unwrap());
    for forbidden in [TOKEN, CANARY, "http://", "https://"] {
        assert!(!rendered.contains(forbidden));
    }
    assert!(error.details.is_null());
}
async fn operation(client: &RegistryClient, which: usize) -> Result<Value, ApiError> {
    match which {
        0 => client
            .publish_method(&offer().publisher, &offer().publication)
            .await
            .map(|v| json!(v)),
        1 => client.method_offer(&lookup()).await.map(|v| json!(v)),
        _ => client.search_methods(&query()).await.map(|v| json!(v)),
    }
}

#[tokio::test]
async fn a_publishes_b_discovers_exact_source_and_prepares_its_own_revision() {
    let registry = RegistryState::open_in_memory(
        HashMap::from([
            ("a-token".into(), OrgId::new("org-a")),
            ("b-token".into(), OrgId::new("org-b")),
        ]),
        vec![],
    )
    .unwrap();
    let server = Server::start(registry::router(registry)).await;
    let a = RegistryClient::new(&server.url, "a-token");
    let b = RegistryClient::new(&server.url, "b-token");
    let original = offer();
    assert_eq!(
        a.publish_method(&original.publisher, &original.publication)
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        a.publish_method(&original.publisher, &original.publication)
            .await
            .unwrap(),
        original
    );
    // Equal method identity under a distinct publisher does not alias A's offer.
    b.publish_method(&OrgId::new("org-b"), &original.publication)
        .await
        .unwrap();
    let found = b.search_methods(&query()).await.unwrap();
    assert_eq!(found.candidates, vec![candidate()]);
    assert!(!found.truncated);
    let fetched = b
        .fetch_method_candidate(&found.candidates[0])
        .await
        .unwrap();
    assert_eq!(fetched, original);
    let revision = MethodRevisionSpec {
        parent: fetched.publication.reference.clone(),
        generation: MethodSynthesisPlan {
            operation_id: "b-distinct-task-revision".into(),
            org_id: "org-b".into(),
            target_id: "b-method".into(),
            target_version: "1".into(),
            runtime: package().runtime().clone(),
            protocol: protocol(),
            entrypoint: "method.py".into(),
            dependencies: vec![],
            public_task: document("b-task", json!({"task":"rough transport development"})),
            public_inputs: vec![],
            instruction: "Adapt the parent to the new public task.".into(),
            settings: TextModelSettings {
                model: "fixture-model-1".into(),
                max_tokens: 512,
                timeout_ms: 1000,
                sampling: Sampling::ProviderDefault,
                reasoning: None,
            },
        },
    };
    let prepared = revision
        .prepare(fetched.publication.package_json.as_bytes())
        .unwrap();
    assert_eq!(prepared.phase(), MethodSynthesisPhase::Prepared);
    assert!(prepared.completion().is_none());
    assert_eq!(prepared.plan().org_id, "org-b");
    assert_eq!(
        prepared.plan().public_inputs[0].reference,
        original.publication.reference
    );
    assert_eq!(
        prepared.plan().public_inputs[0].text,
        original.publication.package_json
    );
    let mut changed = original.publication.clone();
    changed.intent.push('!');
    assert_eq!(
        a.publish_method(&original.publisher, &changed)
            .await
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn publication_and_fetch_reject_substituted_identity_and_noncanonical_source() {
    let original = json!(offer());
    let mut variants = vec![];
    for field in ["domain", "schema", "id", "version", "sha256"] {
        let mut v = original.clone();
        v["publication"]["reference"][field] = json!(if field == "sha256" {
            "b".repeat(64)
        } else {
            "foreign".into()
        });
        variants.push(v);
    }
    let mut v = original.clone();
    v["publisher"] = json!("org-other");
    variants.push(v);
    let mut v = original.clone();
    v["publication"]["package_json"] = json!(format!("{}\n", offer().publication.package_json));
    variants.push(v);
    let mut v = original.clone();
    v["publication"]["package_json"] =
        json!(offer().publication.package_json.replace("inert", "other"));
    variants.push(v);
    for value in variants {
        let canned = Canned::new(value.to_string());
        let server = canned.serve().await;
        let client = RegistryClient::new(&server.url, TOKEN);
        for which in [0, 1] {
            let error = operation(&client, which).await.unwrap_err();
            assert_eq!(error.error, "registry_invalid_response");
            sanitized(&error);
        }
        assert_eq!(canned.seen.lock().unwrap().len(), 2);
    }
    let mut changed = original;
    changed["publication"]["intent"] = json!("changed public intent");
    let canned = Canned::new(changed.to_string());
    let server = canned.serve().await;
    assert_eq!(
        operation(&RegistryClient::new(server.url.clone(), TOKEN), 0)
            .await
            .unwrap_err()
            .error,
        "registry_invalid_response"
    );
}

#[tokio::test]
async fn search_rejects_inconsistent_bounds_filters_and_duplicate_logical_identities() {
    let c = json!(candidate());
    let mut variants = vec![
        json!({"candidates":[],"truncated":true}),
        json!({"candidates":[c.clone()],"truncated":true}),
        json!({"candidates":[c.clone(),c.clone(),c.clone()],"truncated":false}),
        json!({"candidates":[c.clone(),c.clone()],"truncated":false}),
    ];
    let mut conflict = c.clone();
    conflict["reference"]["sha256"] = json!("b".repeat(64));
    variants.push(json!({"candidates":[c.clone(),conflict],"truncated":false}));
    for field in ["runtime", "protocol"] {
        let mut v = c.clone();
        v[field]["id"] = json!("different");
        variants.push(json!({"candidates":[v],"truncated":false}));
    }
    for (field, value) in [
        ("publisher", json!("org-b")),
        ("intent", json!("unrelated")),
        ("unexpected", json!(true)),
        ("dependencies", json!(vec![package().runtime().clone(); 2])),
    ] {
        let mut v = c.clone();
        v[field] = value;
        variants.push(json!({"candidates":[v],"truncated":false}));
    }
    let mut earlier = c.clone();
    earlier["publisher"] = json!("org-0");
    variants.push(json!({"candidates":[c.clone(),earlier],"truncated":false}));
    for value in variants {
        let canned = Canned::new(value.to_string());
        let server = canned.serve().await;
        let error = RegistryClient::new(&server.url, TOKEN)
            .search_methods(&query())
            .await
            .unwrap_err();
        assert_eq!(error.error, "registry_invalid_response");
        sanitized(&error);
        assert_eq!(
            *canned.seen.lock().unwrap(),
            vec!["/registry/v1/methods/search"]
        );
    }
    // Truncation at a full page is coherent; identical IDs under different publishers are valid.
    let mut other = c.clone();
    other["publisher"] = json!("org-c");
    for value in [
        json!({"candidates":[c,other],"truncated":true}),
        json!({"candidates":[],"truncated":false}),
    ] {
        let canned = Canned::new(value.to_string());
        let server = canned.serve().await;
        RegistryClient::new(server.url.clone(), TOKEN)
            .search_methods(&query())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn method_operations_keep_transport_limits_no_redirects_no_retries_and_sanitized_errors() {
    let destination = Canned::new("{}".into());
    let dest = destination.serve().await;
    for mode in 0..6 {
        let mut canned = Canned::new(CANARY.into());
        let expected = match mode {
            0 => {
                canned.status = StatusCode::TEMPORARY_REDIRECT;
                canned.location = Some(format!("{}/?{TOKEN}", dest.url));
                "registry_redirect"
            }
            1 => {
                canned.status = StatusCode::SERVICE_UNAVAILABLE;
                "registry_error"
            }
            2 | 3 => {
                canned.body = "x".repeat(1024 * 1024 + 1);
                canned.chunked = mode == 3;
                "registry_response_too_large"
            }
            4 => {
                canned.body = serde_json::to_string(&offer()).unwrap().replacen(
                    "\"sha256\":",
                    &format!("\"sha256\":\"{}\",\"sha256\":", "a".repeat(64)),
                    1,
                );
                "registry_invalid_response"
            }
            _ => {
                canned.body = "not JSON".into();
                "registry_invalid_response"
            }
        };
        let server = canned.serve().await;
        let client = RegistryClient::new(&server.url, TOKEN);
        for which in 0..3 {
            let error = operation(&client, which).await.unwrap_err();
            assert_eq!(error.error, expected);
            sanitized(&error);
        }
        assert_eq!(
            *canned.seen.lock().unwrap(),
            vec![
                "/registry/v1/methods?expected_publisher=org-a",
                "/registry/v1/methods/fetch",
                "/registry/v1/methods/search"
            ]
        );
    }
    assert!(destination.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_local_requests_are_refused_before_network() {
    let canned = Canned::new("{}".into());
    let server = canned.serve().await;
    let client = RegistryClient::new(&server.url, TOKEN);
    let mut publication = offer().publication;
    publication.package_json.push('\n');
    assert_eq!(
        client
            .publish_method(&OrgId::new("org-a"), &publication)
            .await
            .unwrap_err()
            .error,
        "registry_invalid_request"
    );
    assert_eq!(
        client
            .publish_method(&OrgId::new(" "), &offer().publication)
            .await
            .unwrap_err()
            .error,
        "registry_invalid_request"
    );
    let mut oversized = offer().publication;
    oversized.package_json = "x".repeat(MAX_METHOD_PACKAGE_BYTES + 1);
    assert_eq!(
        client
            .publish_method(&OrgId::new("org-a"), &oversized)
            .await
            .unwrap_err()
            .error,
        "registry_invalid_request"
    );
    let mut bad_lookup = lookup();
    bad_lookup.reference.sha256 = "bad".into();
    assert_eq!(
        client.method_offer(&bad_lookup).await.unwrap_err().error,
        "registry_invalid_request"
    );
    let mut bad_query = query();
    bad_query.limit = 51;
    assert_eq!(
        client.search_methods(&bad_query).await.unwrap_err().error,
        "registry_invalid_request"
    );
    bad_query.limit = 1;
    bad_query.query = " ".into();
    assert_eq!(
        client.search_methods(&bad_query).await.unwrap_err().error,
        "registry_invalid_request"
    );
    let mut bad_candidate = candidate();
    bad_candidate.dependencies = vec![package().runtime().clone(); 2];
    assert_eq!(
        client
            .fetch_method_candidate(&bad_candidate)
            .await
            .unwrap_err()
            .error,
        "registry_invalid_request"
    );
    bad_candidate.dependencies[1].sha256 = "b".repeat(64);
    assert_eq!(
        client
            .fetch_method_candidate(&bad_candidate)
            .await
            .unwrap_err()
            .error,
        "registry_invalid_request"
    );
    for which in 0..3 {
        let invalid_origin = RegistryClient::new(format!("{}/foreign", server.url), TOKEN);
        assert_eq!(
            operation(&invalid_origin, which).await.unwrap_err().error,
            "registry_configuration"
        );
    }
    assert!(canned.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn selected_search_metadata_must_match_the_fetched_canonical_package() {
    let canned = Canned::new(serde_json::to_string(&offer()).unwrap());
    let server = canned.serve().await;
    let client = RegistryClient::new(&server.url, TOKEN);
    let original = candidate();
    assert_eq!(
        client.fetch_method_candidate(&original).await.unwrap(),
        offer()
    );
    let mut variants = vec![];
    let mut c = original.clone();
    c.intent.push('!');
    variants.push(c);
    let mut c = original.clone();
    c.applicability.push("other applicability".into());
    variants.push(c);
    let mut c = original.clone();
    c.runtime.id.push('x');
    variants.push(c);
    let mut c = original.clone();
    c.protocol.id.push('x');
    variants.push(c);
    let mut c = original.clone();
    c.dependencies.push(package().runtime().clone());
    variants.push(c);
    for candidate in variants {
        let error = client.fetch_method_candidate(&candidate).await.unwrap_err();
        assert_eq!(error.error, "registry_invalid_response");
        sanitized(&error);
    }
    assert_eq!(canned.seen.lock().unwrap().len(), 6);
}
