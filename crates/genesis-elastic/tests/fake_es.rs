//! Client tests against an in-process fake of the Elasticsearch REST subset
//! GENESIS uses. The fake enforces `dynamic: strict` mappings, term filters
//! and a token-overlap stand-in for `multi_match`, so it checks request shape
//! and filtering, not Elasticsearch relevance.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, put};
use axum::{Json, Router};
use chrono::Utc;
use genesis_core::engine::{
    bootstrap_mission, record_observation, record_requirement, MissionBootstrap,
};
use genesis_core::ids::OrgId;
use genesis_core::model::{Constitution, OrgState};
use genesis_elastic::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Fake {
    mappings: BTreeMap<String, Value>,
    docs: BTreeMap<String, BTreeMap<String, Value>>,
    auth: Vec<String>,
    fail_search: bool,
}

type Shared = Arc<Mutex<Fake>>;

async fn create_index(
    State(s): State<Shared>,
    Path(index): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut f = s.lock().unwrap();
    f.auth.push(
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .into(),
    );
    if f.mappings.contains_key(&index) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": {"type": "resource_already_exists_exception", "reason": "exists"}, "status": 400}),
            ),
        );
    }
    f.mappings.insert(index, body["mappings"].clone());
    (StatusCode::OK, Json(json!({"acknowledged": true})))
}

async fn bulk(
    State(s): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let mut f = s.lock().unwrap();
    assert_eq!(headers.get("content-type").unwrap(), "application/x-ndjson");
    let text = String::from_utf8(body.to_vec()).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let mut items = Vec::new();
    for pair in lines.chunks(2) {
        let action: Value = serde_json::from_str(pair[0]).unwrap();
        let doc: Value = serde_json::from_str(pair[1]).unwrap();
        let index = action["index"]["_index"].as_str().unwrap().to_owned();
        let id = action["index"]["_id"].as_str().unwrap().to_owned();
        let props = f.mappings.get(&index).map(|m| m["properties"].clone());
        let error = match props {
            None => Some("index_not_found_exception"),
            Some(p) => doc
                .as_object()
                .unwrap()
                .keys()
                .any(|k| p.get(k).is_none())
                .then_some("strict_dynamic_mapping_exception"),
        };
        let error = error.or_else(|| {
            doc["title"]
                .as_str()
                .filter(|t| t.contains("REJECT"))
                .map(|_| "mapper_parsing_exception")
        });
        match error {
            Some(kind) => items.push(json!({"index": {"_id": id, "status": 400, "error": {"type": kind, "reason": "secret detail"}}})),
            None => {
                f.docs.entry(index).or_default().insert(id.clone(), doc);
                items.push(json!({"index": {"_id": id, "status": 201}}));
            }
        }
    }
    let errors = items.iter().any(|i| i["index"]["status"] != 201);
    (
        StatusCode::OK,
        Json(json!({"took": 1, "errors": errors, "items": items})),
    )
}

fn matches_filter(doc: &Value, filter: &Value) -> bool {
    if let Some(t) = filter.get("term") {
        let (k, v) = t.as_object().unwrap().iter().next().unwrap();
        return &doc[k] == v || doc[k].as_array().is_some_and(|a| a.contains(v));
    }
    if let Some(t) = filter.get("terms") {
        let (k, vs) = t.as_object().unwrap().iter().next().unwrap();
        return vs.as_array().unwrap().contains(&doc[k]);
    }
    panic!("unsupported filter {filter}");
}

fn field_text(doc: &Value, field: &str) -> String {
    let name = field.split('^').next().unwrap();
    match &doc[name] {
        Value::String(s) => s.to_lowercase(),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
        _ => String::new(),
    }
}

async fn search(
    State(s): State<Shared>,
    Path(index): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let f = s.lock().unwrap();
    if f.fail_search {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"error": {"type": "search_phase_execution_exception", "reason": "token=supersecret"}}),
            ),
        );
    }
    let Some(docs) = f.docs.get(&index) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"type": "index_not_found_exception"}})),
        );
    };
    let mm = &body["query"]["bool"]["must"][0]["multi_match"];
    let terms: Vec<String> = mm["query"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let fields: Vec<&str> = mm["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    let filters = body["query"]["bool"]["filter"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut hits: Vec<(f64, &String, &Value)> = docs
        .iter()
        .filter(|(_, d)| filters.iter().all(|flt| matches_filter(d, flt)))
        .map(|(id, d)| {
            let text: String = fields
                .iter()
                .map(|f| field_text(d, f))
                .collect::<Vec<_>>()
                .join(" ");
            let score = terms.iter().filter(|t| text.contains(t.as_str())).count() as f64;
            (score, id, d)
        })
        .filter(|(score, _, _)| *score > 0.0)
        .collect();
    hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(b.1)));
    let size = body["size"].as_u64().unwrap() as usize;
    let hits: Vec<Value> = hits
        .into_iter()
        .take(size)
        .map(|(score, id, d)| json!({"_index": index, "_id": id, "_score": score, "_source": d}))
        .collect();
    (StatusCode::OK, Json(json!({"hits": {"hits": hits}})))
}

async fn start() -> (ElasticClient, Shared) {
    let shared: Shared = Arc::default();
    let app = Router::new()
        .route("/_bulk", post(bulk))
        .route("/{index}", put(create_index))
        .route("/{index}/_search", post(search))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ElasticClient::new(ElasticConfig {
        elasticsearch_url: url,
        api_key: "dGVzdC1rZXk=".into(),
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    (client, shared)
}

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
