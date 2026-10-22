//! Independent HTTP regressions for acceptance idempotency and immutable pins.
//! Fixtures are trusted test setup; no Actor submission or cloud call occurs.
use axum::Router;
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::capability::CapabilityManifest;
use genesis_core::engine::{self, MissionBootstrap};
use genesis_providers::{ApifyCapabilityExecutor, ProviderSet};
use genesis_server::capability_store;
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, parse_publishers, OrgConfig};
use genesis_verification::{decode_json, OracleSuite};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
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

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}

fn manifest() -> CapabilityManifest {
    serde_json::from_value(json!({
        "capability_id":"redteam-fixture-capability","version":"1.0.0","publisher":"org_a",
        "source_digest":"a".repeat(64),"intent":"Read configured venue fixtures",
        "input_schema":{"type":"object"},"output_schema":{"type":"object"},
        "required_permissions":["capability.venue_lookup"],
        "artifact":{"actor_id":"fixture-actor","build_id":"fixture-build","build_number":"0.0.1"},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,
        "acceptance_suite_ref":suite().suite_id,"verification":{
            "passed":true,"cases":[
                {"name":"a","kind":"success","passed":true},
                {"name":"b","kind":"success","passed":true},
                {"name":"c","kind":"unsupported","passed":true}],
            "tested_environment":"synthetic test setup, not live provider evidence",
            "oracle_ref":"trusted-test-fixture","verified_at":"2026-10-21T00:00:00Z"},
        "state":"APPROVED","published_at":null,
    }))
    .unwrap()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
        .unwrap()
}

async fn json_response(response: reqwest::Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = response.text().await.unwrap();
    let body = serde_json::from_str(&body).unwrap_or(Value::String(body));
    (status, body)
}

#[tokio::test]
async fn acceptance_client_key_binds_entire_batch_even_with_disjoint_case_ids() {
    let first = suite();
    let mut second = first.clone();
    second.suite_id = "second-suite-with-disjoint-case-ids".into();
    for case in &mut second.cases {
        case.case_id = format!("second-{}", case.case_id);
    }
    let mut config = OrgConfig::dev("org_a", "Review organization");
    config.dev_stub = false;
    config.oracle_suites = vec![first.clone(), second.clone()];
    // These tests only queue jobs via app.router(), without serve_org's worker.
    // The literal loopback client prevents any accidental external connection.
    let executor = ApifyCapabilityExecutor::new(
        ApifyClient::for_local_testing(
            "synthetic-review-token-never-live",
            RunLimits::default(),
            "http://127.0.0.1:1",
        )
        .unwrap(),
    );
    let app = build_org(
        &config,
        Some(ProviderSet::new().with_executor(Arc::new(executor))),
    )
    .unwrap();
    let initial = app.current_state().unwrap();
    let bootstrap: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Review acceptance batching",
        "roles":[{"key":"owner","name":"Owner","charter":"Own fixture acceptance",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}}],
        "responsibilities":[{"purpose":"Accept fixture capability","owner":"owner"}],
    }))
    .unwrap();
    let (bootstrapped, _) = engine::bootstrap_mission(&initial, &bootstrap, Utc::now()).unwrap();
    app.store
        .commit_state(
            initial.revision,
            &bootstrapped.state,
            bootstrapped.effects,
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    let role = bootstrapped.state.roles.keys().next().unwrap().clone();
    let responsibility = bootstrapped
        .state
        .responsibilities
        .keys()
        .next()
        .unwrap()
        .clone();
    let manifest = manifest();
    let (installed, installation) =
        engine::begin_installation(&bootstrapped.state, &manifest, json!({}), Utc::now()).unwrap();
    app.store
        .commit_state(
            bootstrapped.state.revision,
            &installed.state,
            installed.effects,
            &EventMeta::default(),
            vec![],
            |tx| capability_store::insert_pin(tx, &installation, &manifest),
        )
        .unwrap();
    let observer = app.clone();
    let server = Server::start(app.router()).await;
    let http = client();
    let url = format!(
        "{}/api/v1/capability-installations/{installation}/checks",
        server.url
    );
    let mut body = json!({"client_request_id":"one-logical-batch","role_id":role,
        "responsibility_id":responsibility,"suite_id":first.suite_id});
    let (status, created) = json_response(
        http.post(&url)
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    assert_eq!(
        created["job_ids"].as_array().unwrap().len(),
        first.cases.len()
    );
    assert_eq!(observer.store.list_jobs().unwrap().len(), first.cases.len());

    let (status, replay) = json_response(
        http.post(&url)
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["job_ids"], created["job_ids"]);
    assert_eq!(observer.store.list_jobs().unwrap().len(), first.cases.len());

    body["suite_id"] = json!(second.suite_id);
    let (status, conflict) = json_response(
        http.post(&url)
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "same key must not create another paid batch: {conflict}"
    );
    assert_eq!(conflict["error"], "idempotency_conflict");
    assert_eq!(observer.store.list_jobs().unwrap().len(), first.cases.len());
}

#[tokio::test]
async fn registry_rejects_changed_build_under_same_version_and_source_digest() {
    let state =
        RegistryState::open_in_memory(parse_publishers("org_a:review-registry-token"), vec![])
            .unwrap();
    let server = Server::start(registry::router(state)).await;
    let http = client();
    let url = format!("{}/registry/v1/manifests", server.url);
    let original = manifest();
    let (status, created) = json_response(
        http.post(&url)
            .bearer_auth("review-registry-token")
            .json(&original)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (status, replay) = json_response(
        http.post(&url)
            .bearer_auth("review-registry-token")
            .json(&original)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(created, replay);

    let mut substituted = original.clone();
    substituted.artifact.build_id = "substituted-build".into();
    assert_eq!(substituted.source_digest, original.source_digest);
    let (status, conflict) = json_response(
        http.post(&url)
            .bearer_auth("review-registry-token")
            .json(&substituted)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"], "immutable_version");

    let (_, stored) = json_response(
        http.get(format!(
            "{}/registry/v1/capabilities/{}/versions/{}",
            server.url, original.capability_id, original.version
        ))
        .bearer_auth("review-registry-token")
        .send()
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(stored["artifact"]["build_id"], original.artifact.build_id);
}
