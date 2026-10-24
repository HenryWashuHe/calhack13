//! Client tests against the in-process Elasticsearch fake (`genesis_elastic::fake`).

use chrono::Utc;
use genesis_core::engine::{
    bootstrap_mission, record_observation, record_requirement, MissionBootstrap,
};
use genesis_core::ids::OrgId;
use genesis_core::model::{Constitution, OrgState};
use genesis_elastic::fake::start;
use genesis_elastic::*;
use serde_json::json;
use std::time::Duration;

fn org(id: &str, goal: &str, observation: &str, requirement: &str) -> OrgState {
    let state = OrgState::new(OrgId::new(id), id, Constitution::default());
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": goal,
        "roles": [{"key":"lead","name":"Program lead","charter":"Run the program end to end",
                   "authority":{"scopes":[],"budget_cents":0}}],
        "responsibilities": [{"purpose":"Coordinate the program","owner":"lead"}]
    }))
    .unwrap();
    let (out, mission) = bootstrap_mission(&state, &spec, Utc::now()).unwrap();
    let (out, obs) = record_observation(
        &out.state,
        &mission,
        "venue-portal",
        observation,
        None,
        Utc::now(),
    )
    .unwrap();
    let (out, _) = record_requirement(&out.state, &mission, requirement, &[obs], "Done").unwrap();
    out.state
}

#[tokio::test]
async fn projects_and_searches_an_organizations_evidence() {
    let (client, fake) = start().await;
    let a = org(
        "org_a",
        "Run the spring hackathon",
        "Main hall flooded; venue unavailable",
        "Relocate the event to a new venue",
    );
    client.ensure_indices("org_a").await.unwrap();
    client.ensure_indices("org_a").await.unwrap(); // idempotent
    let docs = org_docs(&a);
    assert_eq!(
        client.index_evidence("org_a", &docs).await.unwrap(),
        docs.len()
    );
    // Re-projection overwrites by deterministic ID.
    client.index_evidence("org_a", &docs).await.unwrap();
    assert_eq!(
        fake.lock().unwrap().docs["genesis-evidence-org_a"].len(),
        docs.len()
    );

    let hits = client
        .search_evidence("org_a", "venue flooded", &EvidenceFilter::default(), 5)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].doc.kind, DocKind::Observation);

    let only_req = EvidenceFilter {
        kinds: vec![DocKind::Requirement],
        mission_id: None,
    };
    let hits = client
        .search_evidence("org_a", "venue", &only_req, 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].doc.title, "Relocate the event to a new venue");
    assert!(hits[0].doc.refs.contains_key("evidence_0"));

    assert!(fake
        .lock()
        .unwrap()
        .auth
        .iter()
        .all(|a| a == "ApiKey dGVzdC1rZXk="));
}

#[tokio::test]
async fn organizations_cannot_read_each_others_evidence() {
    let (client, _) = start().await;
    let a = org(
        "org_a",
        "Run the spring hackathon",
        "Main hall flooded",
        "Relocate the venue",
    );
    let b = org(
        "org_b",
        "Run a robotics fair",
        "Sponsor withdrew",
        "Find a replacement venue sponsor",
    );
    for (id, st) in [("org_a", &a), ("org_b", &b)] {
        client.ensure_indices(id).await.unwrap();
        client.index_evidence(id, &org_docs(st)).await.unwrap();
    }
    let hits = client
        .search_evidence("org_b", "venue", &EvidenceFilter::default(), 10)
        .await
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.doc.org_id == "org_b"));
    // Writing another org's documents into an index is refused client-side.
    let err = client
        .index_evidence("org_b", &org_docs(&a))
        .await
        .unwrap_err();
    assert!(matches!(err, ElasticError::Config(_)));
}

#[tokio::test]
async fn capability_search_filters_state_and_permissions() {
    let (client, _) = start().await;
    client.ensure_indices("org_b").await.unwrap();
    let cap = |id: &str, intent: &str, perms: &[&str], state: &str| CapabilityDoc {
        doc_id: format!("{id}@1.0.0"),
        capability_id: id.into(),
        version: "1.0.0".into(),
        publisher: "org_a".into(),
        intent: intent.into(),
        applicability_conditions: vec!["Public venue listing pages".into()],
        known_limitations: vec![],
        required_permissions: perms.iter().map(|p| p.to_string()).collect(),
        source_digest: "sha256:abc".into(),
        state: state.into(),
    };
    client
        .index_capabilities(&[
            cap(
                "venue-extractor",
                "Extract venue capacity and availability",
                &["web.read"],
                "PUBLISHED",
            ),
            cap(
                "venue-booker",
                "Book a venue and pay the deposit",
                &["payments.write"],
                "PUBLISHED",
            ),
            cap("venue-draft", "Extract venue data (draft)", &[], "TESTING"),
        ])
        .await
        .unwrap();
    let all = client.search_capabilities("venue", None, 10).await.unwrap();
    let ids: Vec<_> = all.iter().map(|h| h.doc.capability_id.as_str()).collect();
    assert!(ids.contains(&"venue-extractor") && ids.contains(&"venue-booker"));
    assert!(
        !ids.contains(&"venue-draft"),
        "unpublished capabilities are not candidates"
    );

    let scopes = vec!["web.read".to_owned()];
    let allowed = client
        .search_capabilities("venue", Some(&scopes), 10)
        .await
        .unwrap();
    assert_eq!(allowed.len(), 1);
    assert_eq!(allowed[0].doc.capability_id, "venue-extractor");
}

#[tokio::test]
async fn partial_bulk_failures_and_http_errors_are_sanitized() {
    let (client, fake) = start().await;
    client.ensure_indices("org_a").await.unwrap();
    let mut docs = org_docs(&org("org_a", "Goal", "Fine observation", "Need"));
    docs[0].title = "REJECT me".into();
    let err = client.index_evidence("org_a", &docs).await.unwrap_err();
    match &err {
        ElasticError::Partial { failed } => {
            assert_eq!(failed.len(), 1);
            assert_eq!(failed[0].1, "mapper_parsing_exception");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!err.to_string().contains("secret"));

    fake.lock().unwrap().fail_search = true;
    let err = client
        .search_evidence("org_a", "x", &EvidenceFilter::default(), 3)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ElasticError::Http {
            operation: "search",
            status: 500,
            kind: "search_phase_execution_exception".into()
        }
    );
    assert!(!err.to_string().contains("supersecret"));
}

#[tokio::test]
async fn search_before_any_projection_returns_nothing() {
    let (client, _) = start().await;
    let hits = client
        .search_evidence("org_new", "anything", &EvidenceFilter::default(), 3)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn unreachable_cluster_is_a_transport_error() {
    let client = ElasticClient::new(ElasticConfig {
        elasticsearch_url: "http://127.0.0.1:9".into(),
        api_key: "k".into(),
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(2),
    })
    .unwrap();
    let err = client.ensure_indices("org_a").await.unwrap_err();
    assert!(matches!(
        err,
        ElasticError::Transport {
            operation: "create_index",
            ..
        }
    ));
}
