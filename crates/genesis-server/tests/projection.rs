//! Elastic projection and retrieval through the organization API, against the
//! in-process Elasticsearch fake.

use axum::http::StatusCode;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::time::Duration;

struct Org {
    base: String,
    cfg: OrgConfig,
    http: reqwest::Client,
}

impl Org {
    async fn call(
        &self,
        method: reqwest::Method,
        token: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = self
            .http
            .request(method, format!("{}/api/v1{path}", self.base))
            .bearer_auth(token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            reqwest::Method::POST,
            &self.cfg.tokens.operator,
            path,
            Some(body),
        )
        .await
    }
    async fn get(&self, token: &str, path: &str) -> (StatusCode, Value) {
        self.call(reqwest::Method::GET, token, path, None).await
    }
}

async fn start(elastic: bool) -> (Org, Option<genesis_elastic::fake::Shared>) {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let mut app = build_org(&cfg, None).unwrap();
    let mut fake = None;
    if elastic {
        let (client, shared) = genesis_elastic::fake::start().await;
        app = app.with_elastic(client);
        fake = Some(shared);
    }
    let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
    (
        Org {
            base: format!("http://{addr}"),
            cfg,
            http: reqwest::Client::new(),
        },
        fake,
    )
}

async fn seed(o: &Org) -> String {
    let (s, m) = o
        .post(
            "/missions",
            json!({
                "goal": "Run the campus event for 120 attendees",
                "roles": [{"key": "coordinator", "name": "Mission coordinator", "charter": "Owns the plan",
                           "authority": {"scopes": ["mission.coordinate"], "budget_cents": 0}}],
                "responsibilities": [{"purpose": "Coordinate the mission", "owner": "coordinator"}]
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{m}");
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let (s, obs) = o
        .post(
            &format!("/missions/{mission}/observations"),
            json!({"source": "venue-portal", "summary": "Main hall flooded on the event date"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{obs}");
    mission
}

async fn search_until(o: &Org, path: &str, want: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..100 {
        let (s, body) = o.get(&o.cfg.tokens.agent, path).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        if want(&body) {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("projection never satisfied {path}");
}

#[tokio::test]
async fn commits_are_projected_and_searchable() {
    let (o, fake) = start(true).await;
    let mission = seed(&o).await;
    let body = search_until(&o, "/search/evidence?q=flooded+hall", |b| {
        b["hits"].as_array().is_some_and(|h| !h.is_empty())
    })
    .await;
    let top = &body["hits"][0]["doc"];
    assert_eq!(top["kind"], "observation");
    assert_eq!(top["org_id"], "org_a");
    assert_eq!(top["mission_id"], mission);

    let (_, state) = o.get(&o.cfg.tokens.operator, "/organization").await;
    let (_, status) = o.get(&o.cfg.tokens.operator, "/projection").await;
    assert_eq!(status["configured"], true);
    assert_eq!(status["projected_revision"], state["revision"]);
    assert_eq!(status["last_error"], Value::Null);

    // Later commits are projected too; filters narrow by kind.
    let (s, _) = o
        .post(
            &format!("/missions/{mission}/requirements"),
            json!({"summary": "Relocate the event", "evidence": [top["doc_id"].as_str().unwrap().rsplit(':').next().unwrap()], "acceptance": "new venue reserved"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let body = search_until(&o, "/search/evidence?q=relocate&kinds=requirement", |b| {
        b["hits"].as_array().is_some_and(|h| h.len() == 1)
    })
    .await;
    assert_eq!(body["hits"][0]["doc"]["kind"], "requirement");
    assert!(fake
        .unwrap()
        .lock()
        .unwrap()
        .docs
        .contains_key("genesis-evidence-org_a"));
}

#[tokio::test]
async fn retrieval_requires_configuration_and_authority() {
    let (o, _) = start(false).await;
    let (s, body) = o.get(&o.cfg.tokens.agent, "/search/evidence?q=venue").await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "elastic_not_configured");
    let (_, status) = o.get(&o.cfg.tokens.operator, "/projection").await;
    assert_eq!(status["configured"], false);

    let (o, _) = start(true).await;
    let (s, _) = o
        .get(&o.cfg.tokens.adapter, "/search/evidence?q=venue")
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = o
        .get(&o.cfg.tokens.agent, "/search/evidence?q=venue&kinds=bogus")
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn elastic_outage_never_blocks_kernel_commits() {
    let (o, fake) = start(true).await;
    let fake = fake.unwrap();
    fake.lock().unwrap().fail_bulk = true;
    seed(&o).await; // commits succeed while Elastic is down
    let mut status = Value::Null;
    for _ in 0..100 {
        status = o.get(&o.cfg.tokens.operator, "/projection").await.1;
        if !status["last_error"].is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let err = status["last_error"].as_str().expect("failure is reported");
    assert!(err.contains("unavailable_shards_exception") && !err.contains("supersecret"));

    // Recovery: the projector retries and catches up.
    fake.lock().unwrap().fail_bulk = false;
    let body = search_until(&o, "/search/evidence?q=flooded", |b| {
        b["hits"].as_array().is_some_and(|h| !h.is_empty())
    })
    .await;
    assert_eq!(body["hits"][0]["doc"]["kind"], "observation");

    fake.lock().unwrap().fail_search = true;
    let (s, body) = o
        .get(&o.cfg.tokens.agent, "/search/evidence?q=flooded")
        .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(!body.to_string().contains("supersecret"));
}
