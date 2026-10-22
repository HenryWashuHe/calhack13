//! `POST /missions/{id}/requirements` with `client_request_id` is idempotent:
//! a lost response can be resent without creating a second requirement.

use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};

struct Org {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl Org {
    async fn start(db: Option<std::path::PathBuf>) -> Self {
        let mut cfg = OrgConfig::dev("org_a", "Org A");
        cfg.db = db;
        let token = cfg.tokens.operator.clone();
        let addr = spawn_org(build_org(&cfg, None).unwrap(), "127.0.0.1:0")
            .await
            .unwrap();
        Self {
            base: format!("http://{addr}"),
            token,
            http: reqwest::Client::new(),
        }
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}/api/v1{path}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let s = r.status().as_u16();
        (s, r.json().await.unwrap_or(Value::Null))
    }
    async fn requirements(&self) -> usize {
        self.http
            .get(format!("{}/api/v1/organization", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["requirements"]
            .as_object()
            .unwrap()
            .len()
    }
    /// A mission with one observation; returns (mission_id, observation_id).
    async fn seed(&self) -> (String, String) {
        let (_, m) = self
            .post("/missions", json!({"goal": "Run the event", "roles": [{"key":"lead","name":"Lead","charter":"Own work",
                "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
                "responsibilities": [{"purpose":"Coordinate work","owner":"lead"}]}))
            .await;
        let mission = m["mission_id"].as_str().unwrap().to_owned();
        let (_, o) = self
            .post(
                &format!("/missions/{mission}/observations"),
                json!({"source":"fixture","summary":"Main hall closed"}),
            )
            .await;
        (mission, o["observation_id"].as_str().unwrap().to_owned())
    }
}

fn body(obs: &str, key: Option<&str>, summary: &str) -> Value {
    let mut b = json!({"summary": summary, "evidence": [obs], "acceptance": "A venue is booked"});
    if let Some(k) = key {
        b["client_request_id"] = json!(k);
    }
    b
}

#[tokio::test]
async fn same_key_and_body_returns_the_same_requirement() {
    let org = Org::start(None).await;
    let (mission, obs) = org.seed().await;
    let path = format!("/missions/{mission}/requirements");
    let (s1, first) = org.post(&path, body(&obs, Some("k1"), "Relocate")).await;
    let (s2, again) = org.post(&path, body(&obs, Some("k1"), "Relocate")).await;
    assert_eq!(s1, 201, "{first}");
    assert_eq!(s2, 200, "{again}");
    assert_eq!(again["requirement_id"], first["requirement_id"]);
    assert_eq!(again["duplicate"], true);
    assert_eq!(org.requirements().await, 1);
}

#[tokio::test]
async fn same_key_with_a_different_body_is_a_conflict() {
    let org = Org::start(None).await;
    let (mission, obs) = org.seed().await;
    let path = format!("/missions/{mission}/requirements");
    assert_eq!(
        org.post(&path, body(&obs, Some("k1"), "Relocate")).await.0,
        201
    );
    let (s, err) = org
        .post(&path, body(&obs, Some("k1"), "Something else"))
        .await;
    assert_eq!(s, 409, "{err}");
    assert_eq!(err["error"], "idempotency_conflict");
    assert_eq!(org.requirements().await, 1);
}

#[tokio::test]
async fn requests_without_a_key_keep_creating_requirements() {
    let org = Org::start(None).await;
    let (mission, obs) = org.seed().await;
    let path = format!("/missions/{mission}/requirements");
    assert_eq!(org.post(&path, body(&obs, None, "Relocate")).await.0, 201);
    assert_eq!(org.post(&path, body(&obs, None, "Relocate")).await.0, 201);
    assert_eq!(org.requirements().await, 2);
    let (s, _) = org.post(&path, body(&obs, Some(" "), "Relocate")).await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn the_key_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let (mission, obs, id) = {
        let org = Org::start(Some(db.clone())).await;
        let (mission, obs) = org.seed().await;
        let (_, r) = org
            .post(
                &format!("/missions/{mission}/requirements"),
                body(&obs, Some("k1"), "Relocate"),
            )
            .await;
        (mission, obs, r["requirement_id"].clone())
    };
    let reopened = Org::start(Some(db)).await;
    let (s, r) = reopened
        .post(
            &format!("/missions/{mission}/requirements"),
            body(&obs, Some("k1"), "Relocate"),
        )
        .await;
    assert_eq!(s, 200, "{r}");
    assert_eq!(r["requirement_id"], id);
    assert_eq!(reopened.requirements().await, 1);
}
