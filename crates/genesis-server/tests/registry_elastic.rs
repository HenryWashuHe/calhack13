//! Registry capability search through Elastic (in-process fake): published
//! manifests are projected, hits are resolved against canonical manifests,
//! and search falls back to keyword ranking when Elastic fails.

use genesis_core::capability::{
    ActorArtifact, CapabilityManifest, CaseKind, CaseResult, VerificationReport,
};
use genesis_core::ids::{CapabilityId, OrgId};
use genesis_core::lifecycle::CapabilityState;
use genesis_server::registry::RegistryState;
use genesis_server::spawn_registry;
use serde_json::{json, Value};
use std::collections::HashMap;

fn manifest(id: &str, intent: &str, permissions: &[&str]) -> CapabilityManifest {
    let case = |n: &str, k: CaseKind| CaseResult {
        name: n.into(),
        kind: k,
        passed: true,
        detail: String::new(),
    };
    CapabilityManifest {
        capability_id: CapabilityId::new(id),
        version: "1.0.0".into(),
        publisher: OrgId::new("org_a"),
        source_digest: genesis_core::capability::sha256_hex(id.as_bytes()),
        intent: intent.into(),
        applicability_conditions: vec!["public listing pages".into()],
        known_limitations: vec![],
        input_schema: json!({"type": "object"}),
        output_schema: json!({"type": "array"}),
        required_permissions: permissions.iter().map(|p| p.to_string()).collect(),
        artifact: ActorArtifact {
            actor_id: format!("actor-{id}"),
            build_id: "build-1".into(),
            build_number: "0.0.1".into(),
        },
        role_template: None,
        responsibility_template: None,
        workflow_procedure: None,
        local_configuration_schema: Value::Null,
        acceptance_suite_ref: "suite://v1".into(),
        verification: VerificationReport {
            passed: true,
            cases: vec![
                case("variant-a", CaseKind::Success),
                case("variant-b", CaseKind::Success),
                case("missing-data", CaseKind::Unsupported),
            ],
            tested_environment: "fixture".into(),
            limitations: vec![],
            oracle_ref: "oracle://v1".into(),
            verified_at: chrono::Utc::now(),
        },
        provenance: vec![],
        state: CapabilityState::Approved,
        published_at: None,
    }
}

async fn registry(elastic: Option<genesis_elastic::ElasticClient>) -> (String, reqwest::Client) {
    let publishers = HashMap::from([("pub-a".to_owned(), OrgId::new("org_a"))]);
    let mut reg = RegistryState::open_in_memory(publishers, vec!["reader".into()]).unwrap();
    if let Some(es) = elastic {
        reg = reg.with_elastic(es);
    }
    let addr = spawn_registry(reg, "127.0.0.1:0").await.unwrap();
    (format!("http://{addr}/registry/v1"), reqwest::Client::new())
}

async fn publish(base: &str, http: &reqwest::Client, m: &CapabilityManifest) {
    let r = http
        .post(format!("{base}/manifests"))
        .bearer_auth("pub-a")
        .json(m)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());
}

async fn search(base: &str, http: &reqwest::Client, body: Value) -> Vec<Value> {
    http.post(format!("{base}/search"))
        .bearer_auth("reader")
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn published_capabilities_are_found_through_elastic() {
    let (es, fake) = genesis_elastic::fake::start().await;
    let index = es.capability_index();
    let (base, http) = registry(Some(es)).await;
    publish(
        &base,
        &http,
        &manifest(
            "venue_extractor",
            "Extract venue capacity and availability",
            &["web.read"],
        ),
    )
    .await;
    publish(
        &base,
        &http,
        &manifest(
            "venue_booker",
            "Book a venue and pay a deposit",
            &["payments.write"],
        ),
    )
    .await;
    assert_eq!(
        fake.lock().unwrap().docs[&index].len(),
        2,
        "both manifests projected"
    );

    let hits = search(&base, &http, json!({"query": "venue capacity", "limit": 5})).await;
    assert_eq!(hits[0]["capability_id"], "venue_extractor");
    assert_eq!(
        hits[0]["source_digest"],
        genesis_core::capability::sha256_hex(b"venue_extractor")
    );

    let scoped = search(
        &base,
        &http,
        json!({"query": "venue", "available_scopes": ["web.read"]}),
    )
    .await;
    let ids: Vec<_> = scoped
        .iter()
        .map(|h| h["capability_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["venue_extractor"]);
}

#[tokio::test]
async fn index_entries_without_a_canonical_manifest_are_never_candidates() {
    let (es, fake) = genesis_elastic::fake::start().await;
    let (base, http) = registry(Some(es.clone())).await;
    publish(
        &base,
        &http,
        &manifest("venue_extractor", "Extract venue capacity", &[]),
    )
    .await;
    // Someone with index write access plants a document.
    let mut planted = genesis_elastic::capability_doc(&manifest(
        "venue_planted",
        "Extract venue capacity fast",
        &[],
    ));
    planted.state = "PUBLISHED".into();
    es.index_capabilities(&[planted]).await.unwrap();
    assert_eq!(fake.lock().unwrap().docs[&es.capability_index()].len(), 2);
    let hits = search(&base, &http, json!({"query": "venue capacity"})).await;
    let ids: Vec<_> = hits
        .iter()
        .map(|h| h["capability_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["venue_extractor"]);
}

#[tokio::test]
async fn elastic_outage_falls_back_to_keyword_search_and_resyncs() {
    let (es, fake) = genesis_elastic::fake::start().await;
    let index = es.capability_index();
    fake.lock().unwrap().fail_bulk = true;
    let (base, http) = registry(Some(es)).await;
    // Publication succeeds even though the projection fails.
    publish(
        &base,
        &http,
        &manifest("venue_extractor", "Extract venue capacity", &[]),
    )
    .await;
    assert!(!fake.lock().unwrap().docs.contains_key(&index));
    // Search still answers (keyword ranking).
    let hits = search(&base, &http, json!({"query": "venue capacity"})).await;
    assert_eq!(hits[0]["capability_id"], "venue_extractor");
    // Once Elastic recovers, the next search re-projects the store.
    fake.lock().unwrap().fail_bulk = false;
    let hits = search(&base, &http, json!({"query": "venue capacity"})).await;
    assert_eq!(hits[0]["capability_id"], "venue_extractor");
    assert_eq!(fake.lock().unwrap().docs[&index].len(), 1);
}

#[tokio::test]
async fn without_elastic_search_uses_keyword_ranking() {
    let (base, http) = registry(None).await;
    publish(
        &base,
        &http,
        &manifest("venue_extractor", "Extract venue capacity", &[]),
    )
    .await;
    let hits = search(&base, &http, json!({"query": "venue capacity"})).await;
    assert_eq!(hits.len(), 1);
}
