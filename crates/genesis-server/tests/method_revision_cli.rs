//! CLI network selection -> existing pure revision preparation. Source is inert.
use axum::{http::Request, middleware::Next, response::Response, Router};
use genesis_core::ids::OrgId;
use genesis_forge::{
    method_revision::{MethodRevisionInput, MethodRevisionSpec},
    method_synthesis::MethodSynthesisPhase,
};
use genesis_server::registry::{methods::*, RegistryClient, RegistryState};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    process::{Command, Output},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::net::TcpListener;
#[path = "../../genesis-forge/tests/common/method_revision_fixture.rs"]
mod fixture;

struct Registry {
    url: String,
    task: tokio::task::JoinHandle<()>,
    fetches: Arc<AtomicUsize>,
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture() -> (Registry, MethodCandidate, MethodRevisionSpec) {
    let (spec, parent) = fixture::fixture();
    let state = RegistryState::open_in_memory(
        HashMap::from([("publisher-fixture".into(), OrgId::new("org-a"))]),
        vec!["receiver-fixture".into()],
    )
    .unwrap();
    let fetches = Arc::new(AtomicUsize::new(0));
    let counted = fetches.clone();
    let router: Router = genesis_server::registry::router(state).layer(axum::middleware::from_fn(
        move |request: Request<axum::body::Body>, next: Next| {
            let counted = counted.clone();
            async move {
                if request.uri().path().ends_with("/fetch") {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
                let response: Response = next.run(request).await;
                response
            }
        },
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let registry = Registry { url, task, fetches };
    let client = RegistryClient::new(&registry.url, "publisher-fixture");
    client
        .publish_method(
            &OrgId::new("org-a"),
            &MethodPublication {
                reference: spec.parent.clone(),
                package_json: String::from_utf8(parent).unwrap(),
                intent: "Development method".into(),
                applicability: vec!["inert fixture".into()],
            },
        )
        .await
        .unwrap();
    let result = client
        .search_methods(&MethodSearch {
            query: "Development".into(),
            limit: 1,
            runtime: None,
            protocol: None,
            exclude_publisher: Some(OrgId::new("org_b")),
        })
        .await
        .unwrap();
    (registry, result.candidates[0].clone(), spec)
}
async fn cli(
    registry: &Registry,
    candidate: &MethodCandidate,
    spec: &MethodRevisionSpec,
) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let selection = dir.path().join("selected.json");
    let plan = dir.path().join("revision.json");
    let original = serde_json::to_vec(spec).unwrap();
    std::fs::write(&selection, serde_json::to_vec(candidate).unwrap()).unwrap();
    std::fs::write(&plan, &original).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_genesis"));
    command
        .env_clear()
        .env("GENESIS_REGISTRY_URL", &registry.url)
        .env("GENESIS_REGISTRY_TOKEN", "receiver-fixture")
        .args(["method", "revision-input", "--input"])
        .arg(&selection)
        .arg("--plan")
        .arg(&plan);
    let result = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert_eq!(std::fs::read(plan).unwrap(), original);
    assert_eq!(
        std::fs::read(selection).unwrap(),
        serde_json::to_vec(candidate).unwrap()
    );
    result
}
fn refused(result: Output) {
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8(result.stderr).unwrap();
    for secret in ["publisher-fixture", "receiver-fixture", fixture::SOURCE] {
        assert!(!stderr.contains(secret));
    }
}

#[tokio::test]
async fn selected_source_becomes_a_receiver_owned_prepared_revision_with_selection_bound() {
    let (registry, selected, spec) = fixture().await;
    let result = cli(&registry, &selected, &spec).await;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(registry.fetches.load(Ordering::SeqCst), 1);
    let input: MethodRevisionInput = serde_json::from_slice(&result.stdout).unwrap();
    let mut session = input.prepare().unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Prepared);
    assert_eq!(session.plan().org_id, spec.generation.org_id);
    assert_eq!(session.plan().settings, spec.generation.settings);
    assert_eq!(session.plan().runtime, spec.generation.runtime);
    assert_eq!(input.spec.parent, spec.parent);
    let provenance = input.spec.generation.public_inputs.last().unwrap();
    let value: Value = serde_json::from_str(&provenance.text).unwrap();
    assert_eq!(value["candidate"], json!(selected));
    assert_eq!(value["candidate"]["publisher"], "org-a");
    assert_eq!(
        provenance.reference.sha256,
        genesis_core::digest::canonical_sha256(&value)
    );
    assert_ne!(
        session.request_sha256(),
        spec.prepare(input.parent_package_json.as_bytes())
            .unwrap()
            .request_sha256()
    );
    // Fixture completion only, never executes the source or calls a provider.
    fixture::complete(&mut session, fixture::SOURCE);
    input.observe(&session).unwrap();
    let mut changed = input.clone();
    changed.spec.generation.public_inputs.pop();
    assert!(changed.observe(&session).is_err());
    assert!(spec
        .observe(input.parent_package_json.as_bytes(), &session)
        .is_err());
}

#[tokio::test]
async fn local_mismatch_or_duplicate_provenance_is_rejected_before_http() {
    let (registry, selected, spec) = fixture().await;
    let mut wrong = spec.clone();
    wrong.parent.sha256 = "f".repeat(64);
    refused(cli(&registry, &selected, &wrong).await);
    let mut wrong = spec.clone();
    wrong.generation.runtime.version = "2".into();
    refused(cli(&registry, &selected, &wrong).await);
    let mut duplicate = spec.clone();
    let mut document = duplicate.generation.public_inputs[0].clone();
    document.reference.domain = "genesis.method_registry_selection".into();
    duplicate.generation.public_inputs.push(document);
    refused(cli(&registry, &selected, &duplicate).await);
    let mut too_many = spec.clone();
    too_many.generation.public_inputs = vec![spec.generation.public_inputs[0].clone(); 15];
    refused(cli(&registry, &selected, &too_many).await);
    assert_eq!(registry.fetches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changed_selected_metadata_cannot_enter_a_revision() {
    let (registry, mut selected, spec) = fixture().await;
    selected.intent = "different selection".into();
    refused(cli(&registry, &selected, &spec).await);
    assert_eq!(registry.fetches.load(Ordering::SeqCst), 1);
}

#[path = "common/review_method_revision_cli.rs"]
mod review;
