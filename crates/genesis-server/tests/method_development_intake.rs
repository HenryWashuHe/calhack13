//! Pure registry fixtures and local HTTP/SQLite; no method source is executed.
use axum::{extract::State, routing::post, Json, Router};
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::OrgId,
};
use genesis_server::{app::AppState, build_org, registry::methods::*, routes, OrgConfig};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{net::TcpListener, task::JoinHandle};

static NEXT: AtomicUsize = AtomicUsize::new(0);

async fn serve(router: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (
        url,
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}
struct Host {
    cfg: OrgConfig,
    app: AppState,
    url: String,
    task: JoinHandle<()>,
    registry: JoinHandle<()>,
    requests: Arc<AtomicUsize>,
    body: Value,
    mission: String,
    dir: PathBuf,
}
impl Drop for Host {
    fn drop(&mut self) {
        self.task.abort();
        self.registry.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Host {
    async fn new() -> Self {
        let r = |id: &str| ExactContentRef {
            domain: "fixture".into(),
            schema: "inert/1".into(),
            id: id.into(),
            version: "1".into(),
            sha256: "a".repeat(64),
        };
        let package = MethodPackage::new(
            "inert".into(),
            "1".into(),
            r("runtime"),
            r("protocol"),
            "method.py".into(),
            vec![],
            vec![MethodSourceFile {
                path: "method.py".into(),
                content: "# SOURCE_CANARY_NEVER_RUN\n".into(),
            }],
        )
        .unwrap();
        let candidate = MethodCandidate {
            publisher: OrgId::new("publisher"),
            reference: package.reference().unwrap(),
            runtime: r("runtime"),
            protocol: r("protocol"),
            dependencies: vec![],
            intent: "development".into(),
            applicability: vec![],
        };
        let offer = MethodOffer {
            publisher: candidate.publisher.clone(),
            publication: MethodPublication {
                reference: candidate.reference.clone(),
                package_json: package.artifact_json().unwrap(),
                intent: candidate.intent.clone(),
                applicability: vec![],
            },
        };
        let requests = Arc::new(AtomicUsize::new(0));
        let (registry_url, registry) = serve(
            Router::new()
                .route(
                    "/registry/v1/methods/fetch",
                    post(
                        |State((offer, count)): State<(MethodOffer, Arc<AtomicUsize>)>| async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            Json(offer)
                        },
                    ),
                )
                .with_state((offer, requests.clone())),
        )
        .await;
        let dir = std::env::temp_dir().join(format!(
            "genesis-method-intake-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut cfg = OrgConfig::dev("receiver", "Receiver");
        cfg.db = Some(dir.join("org.sqlite"));
        cfg.registry_url = Some(registry_url);
        cfg.registry_token = Some("synthetic-token".into());
        let app = build_org(&cfg, None).unwrap();
        let (url, task) = serve(routes::router(app.clone())).await;
        let mission: Value = reqwest::Client::new()
            .post(format!("{url}/api/v1/missions"))
            .bearer_auth(&cfg.tokens.operator)
            .json(&json!({"goal":"Develop transferable methods","roles":[],"responsibilities":[]}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        Self {
            cfg,
            app,
            url,
            task,
            registry,
            requests,
            body: json!({"client_request_id":"receiver-need-1",
            "need":"Develop methods for continuing transport research", "acceptance":"Independent local evaluations remain recorded", "selection":candidate}),
            mission: mission["mission_id"].as_str().unwrap().into(),
            dir,
        }
    }
    fn path(&self) -> String {
        format!(
            "{}/api/v1/missions/{}/method-development",
            self.url, self.mission
        )
    }
    async fn post(&self, body: &Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(self.path())
            .bearer_auth(&self.cfg.tokens.agent)
            .json(body)
            .send()
            .await
            .unwrap()
    }
    async fn restart(&mut self) {
        self.task.abort();
        self.app = build_org(&self.cfg, None).unwrap();
        (self.url, self.task) = serve(routes::router(self.app.clone())).await;
    }
}

#[tokio::test]
async fn exact_selection_creates_one_source_free_observation_and_replays_after_restart() {
    let mut h = Host::new().await;
    let response = h.post(&h.body).await;
    assert_eq!(response.status(), 201);
    let record: Value = response.json().await.unwrap();
    assert_eq!(record["request"], h.body);
    assert_eq!(record["org_id"], "receiver");
    let state = h.app.current_state().unwrap();
    assert_eq!(state.observations.len(), 1);
    assert!(state.requirements.is_empty() && state.roles.is_empty());
    let observation = state.observations.values().next().unwrap();
    assert_eq!(
        observation.observation_id.as_str(),
        record["observation_id"].as_str().unwrap()
    );
    assert!(observation
        .evidence_ref
        .as_ref()
        .unwrap()
        .ends_with(record["request_sha256"].as_str().unwrap()));
    assert!(!serde_json::to_string(&state)
        .unwrap()
        .contains("SOURCE_CANARY"));
    assert!(!record.to_string().contains("SOURCE_CANARY"));
    h.restart().await;
    h.registry.abort(); // Replay requires no new fetch, even after restart.
    let replay = h.post(&h.body).await;
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.json::<Value>().await.unwrap(), record);
    let fetched: Value = reqwest::Client::new()
        .get(format!("{}/receiver-need-1", h.path()))
        .bearer_auth(&h.cfg.tokens.agent)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched, record);
    assert_eq!(h.requests.load(Ordering::SeqCst), 1);
    assert_eq!(h.app.current_state().unwrap().observations.len(), 1);
}

#[tokio::test]
async fn concurrent_replay_is_one_event_and_conflicting_body_is_not_refetched() {
    let h = Host::new().await;
    let (a, b) = tokio::join!(h.post(&h.body), h.post(&h.body));
    let statuses = [a.status().as_u16(), b.status().as_u16()];
    assert!(statuses.contains(&201) && statuses.contains(&200));
    assert_eq!(
        a.json::<Value>().await.unwrap(),
        b.json::<Value>().await.unwrap()
    );
    assert_eq!(h.app.current_state().unwrap().observations.len(), 1);
    let events = h.app.store.events_after(0, 100).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e.kind,
                genesis_core::events::EventKind::ObservationRecorded { .. }
            ))
            .count(),
        1
    );
    let count = h.requests.load(Ordering::SeqCst);
    let mut changed = h.body.clone();
    changed["acceptance"] = "different".into();
    assert_eq!(h.post(&changed).await.status(), 409);
    assert_eq!(h.requests.load(Ordering::SeqCst), count);
}

#[tokio::test]
async fn mismatched_registry_selection_never_becomes_an_observation() {
    let h = Host::new().await;
    let mut body = h.body.clone();
    body["selection"]["intent"] = "substituted".into();
    assert_eq!(h.post(&body).await.status(), 502);
    assert!(h.app.current_state().unwrap().observations.is_empty());
    assert_eq!(h.post(&h.body).await.status(), 201); // Failed fetch did not consume request ID.
}

#[tokio::test]
async fn invalid_unauthorized_and_foreign_mission_requests_have_no_fetch_or_state_effect() {
    let h = Host::new().await;
    let mut empty = h.body.clone();
    empty["need"] = "  ".into();
    assert_eq!(h.post(&empty).await.status(), 400);
    let mut extra = h.body.clone();
    extra["accepted"] = true.into();
    assert_eq!(h.post(&extra).await.status(), 422);
    let mut large = h.body.clone();
    large["need"] = "x".repeat(17000).into();
    assert_eq!(h.post(&large).await.status(), 413);
    let http = reqwest::Client::new();
    assert_eq!(
        http.post(h.path())
            .bearer_auth(&h.cfg.tokens.adapter)
            .json(&h.body)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        http.post(format!(
            "{}/api/v1/missions/foreign/method-development",
            h.url
        ))
        .bearer_auth(&h.cfg.tokens.agent)
        .json(&h.body)
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    assert_eq!(h.requests.load(Ordering::SeqCst), 0);
    assert!(h.app.current_state().unwrap().observations.is_empty());
}

#[tokio::test]
async fn failed_request_insert_rolls_back_observation_and_event() {
    let h = Host::new().await;
    let connection = rusqlite::Connection::open(h.cfg.db.as_ref().unwrap()).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_intake BEFORE INSERT ON method_development_requests BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    let before = serde_json::to_value(h.app.current_state().unwrap()).unwrap();
    let events = h.app.store.events_after(0, 100).unwrap().len();
    assert_eq!(h.post(&h.body).await.status(), 500);
    assert_eq!(
        serde_json::to_value(h.app.current_state().unwrap()).unwrap(),
        before
    );
    assert_eq!(h.app.store.events_after(0, 100).unwrap().len(), events);
    connection
        .execute_batch("DROP TRIGGER fail_intake;")
        .unwrap();
    assert_eq!(h.post(&h.body).await.status(), 201);
}

#[tokio::test]
async fn closed_or_expired_mission_cannot_create_work() {
    for expired in [false, true] {
        let h = Host::new().await;
        h.app
            .mutate(genesis_server::store::EventMeta::default(), |state| {
                let mut state = state.clone();
                let mission = state.missions.values_mut().next().unwrap();
                if expired {
                    mission.deadline = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
                } else {
                    mission.status = genesis_core::lifecycle::MissionStatus::Failed;
                }
                state.revision += 1;
                Ok((
                    genesis_core::engine::Outcome {
                        state,
                        effects: vec![],
                    },
                    (),
                ))
            })
            .await
            .unwrap();
        assert_eq!(h.post(&h.body).await.status(), 409);
        assert_eq!(h.requests.load(Ordering::SeqCst), 0);
        assert!(h.app.current_state().unwrap().observations.is_empty());
    }
}
