//! Independent pressure probes for the exact method registry (#132/#133/#141/#142/#150/#151).
//! Each probe asserts a documented guarantee from
//! docs/invention-lab/contracts/method-registry.md; a failing probe is a defect.
//! Loopback only. No source is executed and no model is called.
use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    Router,
};
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_forge::method_revision::{MethodRevisionInput, MethodRevisionSpec};
use genesis_server::registry::{self, methods::*, RegistryClient, RegistryState};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    process::{Command, Output},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
#[path = "../../genesis-forge/tests/common/method_revision_fixture.rs"]
#[allow(dead_code)]
mod fixture;

struct Server {
    url: String,
    addr: std::net::SocketAddr,
    task: JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        Self {
            url: format!("http://{addr}"),
            addr,
            task: tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
        }
    }
    async fn registry(state: RegistryState) -> Self {
        Self::start(registry::router(state)).await
    }
    async fn call(&self, route: &str, token: &str, value: Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("{}/registry/v1/{route}", self.url))
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
    /// Exact request bytes, so no client library normalizes the header.
    async fn raw_status(&self, path: &str, authorization: Option<&str>, body: &str) -> u16 {
        let mut stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        let auth = authorization
            .map(|v| format!("Authorization: {v}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: probe\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let text = String::from_utf8_lossy(&response);
        text.split_whitespace().nth(1).unwrap().parse().unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn state(path: Option<&std::path::Path>) -> RegistryState {
    let publishers = HashMap::from([
        ("a-token".to_string(), OrgId::new("org-a")),
        ("b-token".to_string(), OrgId::new("org-b")),
    ]);
    match path {
        Some(path) => RegistryState::open(path, publishers, vec!["reader".into()]).unwrap(),
        None => RegistryState::open_in_memory(publishers, vec!["reader".into()]).unwrap(),
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
fn package(id: &str, version: &str, content: &str) -> MethodPackage {
    MethodPackage::new(
        id.into(),
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
    .unwrap()
}
fn publication(
    id: &str,
    version: &str,
    content: &str,
    intent: &str,
    applicability: &[&str],
) -> MethodPublication {
    let p = package(id, version, content);
    MethodPublication {
        reference: p.reference().unwrap(),
        package_json: p.artifact_json().unwrap(),
        intent: intent.into(),
        applicability: applicability.iter().map(|s| s.to_string()).collect(),
    }
}
fn search(query: &str) -> Value {
    json!({ "query": query, "limit": 50 })
}
fn count(found: &(u16, Value)) -> usize {
    assert_eq!(found.0, 200, "search must succeed: {found:?}");
    found.1["candidates"].as_array().unwrap().len()
}

// ---------------------------------------------------------------------------
// Auth: "Reader tokens cannot publish"; an absent/empty credential is not a token.
// ---------------------------------------------------------------------------

/// `genesis registry --publishers "org-a:a-token,org-b:"` (a trailing empty or
/// whitespace token) must not configure an empty credential. Readers already
/// filter empty entries; publishers are parsed by `parse_publishers`.
#[test]
fn probe_publisher_parser_never_registers_an_empty_token() {
    for spec in ["org-a:a-token,org-b:", "org-a:a-token, org-b:  "] {
        let publishers = genesis_server::parse_publishers(spec);
        assert!(
            !publishers.contains_key(""),
            "{spec:?} registered an empty publisher token for {:?}",
            publishers.get("")
        );
    }
}

/// End to end: with the registry configured from that publisher string, a
/// request carrying an empty bearer credential must be unauthenticated.
#[tokio::test]
async fn probe_empty_bearer_cannot_publish_or_read_when_publisher_spec_has_empty_token() {
    let publishers = genesis_server::parse_publishers("org-a:a-token,org-b:");
    let server = Server::registry(RegistryState::open_in_memory(publishers, vec![]).unwrap()).await;
    let p = publication("probe-auth", "1", "# inert\n", "auth probe", &[]);
    let body = serde_json::to_string(&p).unwrap();
    let mut statuses = Vec::new();
    for header in [None, Some("Bearer"), Some("Bearer "), Some("Bearer \t")] {
        statuses.push((
            header,
            server
                .raw_status("/registry/v1/methods", header, &body)
                .await,
            server
                .raw_status("/registry/v1/methods/search", header, r#"{"query":"auth"}"#)
                .await,
        ));
    }
    for (header, publish, search) in &statuses {
        assert_eq!(
            (*publish, *search),
            (401, 401),
            "empty credential {header:?} was accepted: {statuses:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Search: "case-folded substring of public intent/applicability".
// ---------------------------------------------------------------------------

/// A query that occurs literally (same case) inside the public intent must
/// match a case-folded substring search. Rust `str::to_lowercase` applies the
/// context-sensitive Greek final-sigma rule to query and stored text
/// separately, so the same `Σ` lowercases to `ς` in a short query but `σ`
/// inside a longer word.
#[tokio::test]
async fn probe_search_matches_exact_literal_substring_with_sigma() {
    let server = Server::registry(state(None)).await;
    let p = publication(
        "probe-sigma",
        "1",
        "# inert\n",
        "ΟΔΟΣΑ terrain survey",
        &["public fixture"],
    );
    assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 201);
    // Control: ordinary ASCII and full-word queries match.
    assert_eq!(
        count(
            &server
                .call("methods/search", "reader", search("TERRAIN"))
                .await
        ),
        1
    );
    assert_eq!(
        count(
            &server
                .call("methods/search", "reader", search("ΟΔΟΣΑ"))
                .await
        ),
        1
    );
    // "ΟΔΟΣ" is a literal, same-case substring of the intent.
    assert!(p.intent.contains("ΟΔΟΣ"));
    let found = server
        .call("methods/search", "reader", search("ΟΔΟΣ"))
        .await;
    assert_eq!(
        count(&found),
        1,
        "literal substring 'ΟΔΟΣ' of intent 'ΟΔΟΣΑ terrain survey' was not found: {found:?}"
    );
}

/// The documented match is a substring of the public intent or applicability.
/// A query that is a substring of neither field (only of their space-joined
/// concatenation) should not produce a candidate. Interpretive: see report.
#[tokio::test]
async fn probe_search_does_not_match_across_the_intent_applicability_boundary() {
    let server = Server::registry(state(None)).await;
    let p = publication("probe-join", "1", "# inert\n", "alpha", &["beta"]);
    assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 201);
    assert_eq!(
        count(
            &server
                .call("methods/search", "reader", search("alpha"))
                .await
        ),
        1
    );
    assert_eq!(
        count(
            &server
                .call("methods/search", "reader", search("beta"))
                .await
        ),
        1
    );
    let found = server
        .call("methods/search", "reader", search("alpha beta"))
        .await;
    assert_eq!(
        count(&found),
        0,
        "'alpha beta' is a substring of neither intent 'alpha' nor applicability 'beta': {found:?}"
    );
}

// ---------------------------------------------------------------------------
// Exactness and immutability.
// ---------------------------------------------------------------------------

/// Identity is exact bytes: NFC and NFD spellings of an ID are distinct
/// publications, and a lookup never resolves to the other normalization.
#[tokio::test]
async fn probe_exact_fetch_never_normalizes_unicode_identity() {
    let server = Server::registry(state(None)).await;
    let nfc = publication("caf\u{e9}", "1", "# nfc\n", "normalization probe", &[]);
    let nfd = publication("cafe\u{301}", "1", "# nfd\n", "normalization probe", &[]);
    assert_eq!(server.call("methods", "a-token", json!(nfc)).await.0, 201);
    let lookup = |p: &MethodPublication| json!({"publisher":"org-a","reference":p.reference});
    assert_eq!(
        server.call("methods/fetch", "reader", lookup(&nfd)).await.0,
        404
    );
    assert_eq!(server.call("methods", "a-token", json!(nfd)).await.0, 201);
    for p in [&nfc, &nfd] {
        let (status, offer) = server.call("methods/fetch", "reader", lookup(p)).await;
        assert_eq!(status, 200);
        assert_eq!(offer["publication"], json!(p));
    }
}

/// Many concurrent publishes of one identity across two connections to one
/// database: exactly one 201; identical retries 200 with the winner; changed
/// bytes 409; never a server error or an overwrite.
#[tokio::test]
async fn probe_concurrent_same_identity_publishes_have_one_winner_and_no_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("registry.db");
    let servers = [
        Server::registry(state(Some(&db))).await,
        Server::registry(state(Some(&db))).await,
    ];
    let first = publication("probe-race", "1", "# first\n", "race probe", &[]);
    let second = publication("probe-race", "1", "# second\n", "race probe", &[]);
    let calls = (0..24).map(|i| {
        let server = &servers[i % 2];
        let p = if i % 3 == 0 { &second } else { &first };
        async move { (p.clone(), server.call("methods", "a-token", json!(p)).await) }
    });
    let results = futures::future::join_all(calls).await;
    let winners: Vec<_> = results.iter().filter(|(_, r)| r.0 == 201).collect();
    assert_eq!(
        winners.len(),
        1,
        "{:?}",
        results.iter().map(|(_, r)| r.0).collect::<Vec<_>>()
    );
    let winner = winners[0].0.clone();
    for (p, (status, body)) in &results {
        match *status {
            201 => {}
            200 => {
                assert_eq!(p, &winner);
                assert_eq!(body["publication"], json!(winner));
            }
            409 => assert_ne!(p, &winner),
            other => panic!("unexpected status {other}: {body}"),
        }
    }
    let (status, offer) = servers[1]
        .call(
            "methods/fetch",
            "reader",
            json!({"publisher":"org-a","reference":winner.reference}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(offer["publication"], json!(winner));
}

// ---------------------------------------------------------------------------
// Bounds: every response the server can legitimately produce must fit the
// client's 1 MiB decoder ("Search candidates are bounded to 8 KiB").
// ---------------------------------------------------------------------------

/// Fifty maximal (8 KiB-class) candidates plus one extra must decode through
/// the typed client with coherent truncation.
#[tokio::test]
async fn probe_maximal_search_page_fits_client_limit() {
    let server = Server::registry(state(None)).await;
    let intent = format!("bounds {}", "i".repeat(1024 - 7));
    let applicability: Vec<String> = (0..12)
        .map(|i| format!("{i:02}{}", "a".repeat(254)))
        .collect();
    let refs: Vec<&str> = applicability.iter().map(String::as_str).collect();
    for version in 0..51 {
        let p = publication(
            "probe-bounds",
            &format!("{version:03}"),
            "# inert\n",
            &intent,
            &refs,
        );
        let (status, body) = server.call("methods", "a-token", json!(p)).await;
        assert_eq!(status, 201, "{body}");
    }
    let client = RegistryClient::new(&server.url, "reader");
    let results = client
        .search_methods(&MethodSearch {
            query: "bounds".into(),
            limit: 50,
            runtime: None,
            protocol: None,
            exclude_publisher: None,
        })
        .await
        .unwrap();
    assert_eq!(results.candidates.len(), 50);
    assert!(results.truncated);
}

/// The largest accepted package whose content is all JSON-escaped characters
/// roughly doubles in the offer encoding; the typed client must still fetch it.
#[tokio::test]
async fn probe_maximal_escaped_package_fetches_through_client() {
    let server = Server::registry(state(None)).await;
    // Find the largest quote-only content that still fits the 128 KiB transport limit.
    let (mut low, mut high) = (1usize, MAX_METHOD_PACKAGE_BYTES);
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let p = package("probe-escape", "1", &"\"".repeat(mid));
        if p.artifact_json().unwrap().len() <= MAX_METHOD_PACKAGE_BYTES {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let p = publication("probe-escape", "1", &"\"".repeat(low), "escape bound", &[]);
    assert!(p.package_json.len() > MAX_METHOD_PACKAGE_BYTES - 64);
    assert_eq!(server.call("methods", "a-token", json!(p)).await.0, 201);
    let client = RegistryClient::new(&server.url, "reader");
    let offer = client
        .method_offer(&MethodLookup {
            publisher: OrgId::new("org-a"),
            reference: p.reference.clone(),
        })
        .await
        .unwrap();
    assert_eq!(offer.publication, p);
}

// ---------------------------------------------------------------------------
// Revision bridge: the bundle must carry exactly the selected metadata and
// the selected exact bytes, whatever the registry (or a proxy) returns.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Substitute {
    offer: Value,
}
async fn substitute(State(s): State<Substitute>, request: Request) -> Response<Body> {
    let status = if request.uri().path().ends_with("/methods/fetch") {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(s.offer.to_string()))
        .unwrap()
}

fn selected(spec: &MethodRevisionSpec) -> MethodCandidate {
    MethodCandidate {
        publisher: OrgId::new("org-a"),
        reference: spec.parent.clone(),
        intent: "Development method".into(),
        applicability: vec!["inert fixture".into()],
        runtime: spec.generation.runtime.clone(),
        protocol: spec.generation.protocol.reference.clone(),
        dependencies: spec.generation.dependencies.clone(),
    }
}
fn true_offer(candidate: &MethodCandidate, parent: &[u8]) -> Value {
    json!({
        "publisher": candidate.publisher,
        "publication": {
            "reference": candidate.reference,
            "package_json": String::from_utf8(parent.to_vec()).unwrap(),
            "intent": candidate.intent,
            "applicability": candidate.applicability,
        }
    })
}
async fn revision_cli(url: &str, candidate: &MethodCandidate, spec: &MethodRevisionSpec) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let selection = dir.path().join("selected.json");
    let plan = dir.path().join("revision.json");
    std::fs::write(&selection, serde_json::to_vec(candidate).unwrap()).unwrap();
    std::fs::write(&plan, serde_json::to_vec(spec).unwrap()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_genesis"));
    command
        .env_clear()
        .env("GENESIS_REGISTRY_URL", url)
        .env("GENESIS_REGISTRY_TOKEN", "receiver-probe")
        .args(["method", "revision-input", "--input"])
        .arg(&selection)
        .arg("--plan")
        .arg(&plan);
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

/// A lying registry/proxy cannot change the selected metadata, publisher or
/// exact parent bytes that land in the receiver's revision bundle.
#[tokio::test]
async fn probe_revision_bundle_cannot_carry_substituted_registry_data() {
    let (spec, parent) = fixture::fixture();
    let candidate = selected(&spec);
    let honest = true_offer(&candidate, &parent);

    // Control: an honest response produces a bundle binding exactly the selection.
    let server = Server::start(Router::new().fallback(substitute).with_state(Substitute {
        offer: honest.clone(),
    }))
    .await;
    let out = revision_cli(&server.url, &candidate, &spec).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let input: MethodRevisionInput = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(input.parent_package_json.as_bytes(), parent.as_slice());
    let selection: Value =
        serde_json::from_str(&input.spec.generation.public_inputs.last().unwrap().text).unwrap();
    assert_eq!(selection["candidate"], json!(candidate));
    drop(server);

    // A different valid package for another publisher's same id/version.
    let other = MethodPackage::new(
        spec.parent.id.clone(),
        spec.parent.version.clone(),
        spec.generation.runtime.clone(),
        spec.generation.protocol.reference.clone(),
        "parent.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "parent.py".into(),
            content: "# substituted parent\n".into(),
        }],
    )
    .unwrap();
    let mut variants = Vec::new();
    let mut v = honest.clone();
    v["publication"]["applicability"] = json!(["substituted applicability"]);
    variants.push(("changed applicability", v));
    let mut v = honest.clone();
    v["publisher"] = json!("org-z");
    variants.push(("changed publisher", v));
    let mut v = honest.clone();
    v["publication"]["reference"] = json!(other.reference().unwrap());
    v["publication"]["package_json"] = json!(other.artifact_json().unwrap());
    variants.push(("substituted valid package", v));
    let mut v = honest.clone();
    v["publication"]["package_json"] = json!(other.artifact_json().unwrap());
    variants.push(("substituted bytes under selected reference", v));
    let mut v = honest.clone();
    v["publication"]["package_json"] = json!(String::from_utf8(parent.clone())
        .unwrap()
        .replace("\":", "\" :"));
    variants.push(("noncanonical selected bytes", v));
    for (name, offer) in variants {
        let server = Server::start(
            Router::new()
                .fallback(substitute)
                .with_state(Substitute { offer }),
        )
        .await;
        let out = revision_cli(&server.url, &candidate, &spec).await;
        assert!(
            !out.status.success() && out.stdout.is_empty(),
            "{name}: substituted registry data entered a revision bundle"
        );
    }
}
