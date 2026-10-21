//! In-process fake of the Elasticsearch REST subset GENESIS uses, for tests
//! (feature `fake`). It enforces `dynamic: strict` mappings, term filters and a
//! token-overlap stand-in for `multi_match`, so it checks request shape and
//! filtering, not Elasticsearch relevance. A document whose `title` contains
//! `REJECT` fails indexing.

use crate::{ElasticClient, ElasticConfig};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct FakeState {
    pub mappings: BTreeMap<String, Value>,
    pub docs: BTreeMap<String, BTreeMap<String, Value>>,
    pub auth: Vec<String>,
    /// Make every search fail with HTTP 500 (body contains a fake secret).
    pub fail_search: bool,
    /// Make every bulk request fail with HTTP 503.
    pub fail_bulk: bool,
}

pub type Shared = Arc<Mutex<FakeState>>;

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
    if f.fail_bulk {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                json!({"error": {"type": "unavailable_shards_exception", "reason": "token=supersecret"}}),
            ),
        );
    }
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

/// Start a fake on a random local port and return a client for it.
pub async fn start() -> (ElasticClient, Shared) {
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
