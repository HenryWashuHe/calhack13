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

/// State of the fake Kibana Agent Builder API.
#[derive(Default)]
pub struct FakeKibanaState {
    pub tools: BTreeMap<String, Value>,
    pub agents: BTreeMap<String, Value>,
    /// Number of POST requests per collection ("tools", "agents", "converse").
    pub posts: BTreeMap<String, usize>,
    /// Every request path, including any `/s/{space}` prefix.
    pub paths: Vec<String>,
    /// Requests that were writes but lacked `kbn-xsrf: true`.
    pub missing_xsrf: usize,
    pub auth: Vec<String>,
    /// Store the next created agent but answer 409, as if a concurrent or
    /// earlier request had created it.
    pub conflict_after_create: bool,
    /// Answer converse with a pending human-in-the-loop prompt.
    pub awaiting_prompt: bool,
}

pub type KibanaShared = Arc<Mutex<FakeKibanaState>>;

fn kibana_record(s: &KibanaShared, method: &str, path: &str, headers: &HeaderMap) {
    let mut f = s.lock().unwrap();
    f.paths.push(format!("{method} {path}"));
    f.auth.push(
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .into(),
    );
    if method != "GET" && headers.get("kbn-xsrf").and_then(|v| v.to_str().ok()) != Some("true") {
        f.missing_xsrf += 1;
    }
}

fn not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"statusCode": 404, "error": "Not Found", "message": "secret-ish detail"})),
    )
}

async fn kibana_get(
    State(s): State<KibanaShared>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    kibana_record(&s, "GET", uri.path(), &headers);
    let (collection, id) = (params["collection"].clone(), params["id"].clone());
    let f = s.lock().unwrap();
    let map = match collection.as_str() {
        "tools" => &f.tools,
        "agents" => &f.agents,
        _ => return not_found(),
    };
    match map.get(&id) {
        Some(v) => (StatusCode::OK, Json(v.clone())),
        None => not_found(),
    }
}

async fn kibana_post(
    State(s): State<KibanaShared>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    kibana_record(&s, "POST", uri.path(), &headers);
    let collection = params["collection"].clone();
    let mut f = s.lock().unwrap();
    *f.posts.entry(collection.clone()).or_default() += 1;
    if collection == "converse" {
        let agent = body["agent_id"].as_str().unwrap_or("").to_owned();
        let Some(a) = f.agents.get(&agent) else {
            return not_found();
        };
        if f.awaiting_prompt {
            return (
                StatusCode::OK,
                Json(
                    json!({"conversation_id": "conv-1", "round_id": "round-1", "status": "awaiting_prompt",
                            "response": {"prompts": [{"type": "confirmation", "id": "p1"}]}}),
                ),
            );
        }
        let tool = a["configuration"]["tools"][0]["tool_ids"][0].clone();
        let conv = body["conversation_id"]
            .as_str()
            .unwrap_or("conv-1")
            .to_owned();
        return (
            StatusCode::OK,
            Json(json!({
                "conversation_id": conv, "round_id": "round-1", "status": "completed",
                "steps": [{"type": "reasoning", "reasoning": "search"},
                          {"type": "tool_call", "tool_id": tool, "params": {}, "results": []}],
                "response": {"message": format!("answer to: {}", body["input"].as_str().unwrap_or(""))}
            })),
        );
    }
    let id = body["id"].as_str().unwrap_or("").to_owned();
    let conflict = f.conflict_after_create && collection == "agents";
    let map = match collection.as_str() {
        "tools" => &mut f.tools,
        "agents" => &mut f.agents,
        _ => return not_found(),
    };
    if map.contains_key(&id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"statusCode": 409, "error": "Conflict", "message": "exists"})),
        );
    }
    let mut stored = body.clone();
    if collection == "agents" {
        stored["type"] = json!("chat");
    }
    stored["readonly"] = json!(false);
    map.insert(id, stored.clone());
    if conflict {
        f.conflict_after_create = false;
        return (
            StatusCode::CONFLICT,
            Json(json!({"statusCode": 409, "error": "Conflict", "message": "exists"})),
        );
    }
    (StatusCode::OK, Json(stored))
}

/// Start a fake Kibana Agent Builder API; returns its base URL.
pub async fn start_kibana() -> (String, KibanaShared) {
    let shared: KibanaShared = Arc::default();
    let api = Router::new()
        .route("/api/agent_builder/{collection}", post(kibana_post))
        .route(
            "/api/agent_builder/{collection}/{id}",
            axum::routing::get(kibana_get),
        );
    let app = Router::new()
        .merge(api.clone())
        .nest("/s/{space}", api)
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, shared)
}
