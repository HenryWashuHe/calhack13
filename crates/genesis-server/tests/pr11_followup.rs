//! Safe-behavior regressions for PR11 aa94d100. These intentionally fail on
//! that head: managed specialist calls must honor current mission/lease time,
//! and a generic adapter assertion must not manufacture their acceptance.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_core::{
    engine::{bootstrap_mission, MissionBootstrap, RoleToProvision},
    ids::{ChangeId, JobId, MissionId, ResponsibilityId, RoleId},
    jobs::Job,
    lifecycle::JobStatus,
    model::{OrgState, Provider},
};
use genesis_elastic::{AgentBuilder, AgentBuilderConfig};
use genesis_providers::{ProviderSet, ProvisionRequest, RoleProvisioner};
use genesis_server::{app::AppState, build_org, outbox, store::EventMeta, OrgConfig};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{Notify, Semaphore};

struct Remote {
    objects: Mutex<BTreeMap<String, Value>>,
    calls: AtomicUsize,
    hold: AtomicBool,
    entered: Notify,
    release: Semaphore,
}

async fn read(
    State(remote): State<Arc<Remote>>,
    Path((collection, id)): Path<(String, String)>,
) -> (StatusCode, Json<Value>) {
    match remote
        .objects
        .lock()
        .unwrap()
        .get(&format!("{collection}/{id}"))
    {
        Some(value) => (StatusCode::OK, Json(value.clone())),
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"Not Found"}))),
    }
}

async fn create(
    State(remote): State<Arc<Remote>>,
    Path(collection): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    remote.objects.lock().unwrap().insert(
        format!("{collection}/{}", body["id"].as_str().unwrap()),
        body.clone(),
    );
    Json(body)
}

async fn converse(State(remote): State<Arc<Remote>>, Json(body): Json<Value>) -> Json<Value> {
    assert!(body["agent_id"].as_str().unwrap().starts_with("genesis-"));
    assert!(body["input"].is_string());
    remote.calls.fetch_add(1, Ordering::SeqCst);
    remote.entered.notify_one();
    if remote.hold.load(Ordering::SeqCst) {
        remote.release.acquire().await.unwrap().forget();
    }
    Json(json!({
        "conversation_id":"conversation-review", "round_id":"round-review", "status":"completed",
        "response":{"message":"Untrusted specialist analysis"}, "steps":[]
    }))
}

struct Fixture {
    app: AppState,
    cfg: OrgConfig,
    base: String,
    http: reqwest::Client,
    mission: MissionId,
    role: RoleId,
    responsibility: ResponsibilityId,
    remote: Arc<Remote>,
    servers: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for server in &self.servers {
            server.abort();
        }
    }
}

impl Fixture {
    async fn new() -> Self {
        let remote = Arc::new(Remote {
            objects: Mutex::new(BTreeMap::new()),
            calls: AtomicUsize::new(0),
            hold: AtomicBool::new(false),
            entered: Notify::new(),
            release: Semaphore::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/api/agent_builder/converse", post(converse))
            .route("/api/agent_builder/{collection}/{id}", get(read))
            .route("/api/agent_builder/{collection}", post(create))
            .with_state(remote.clone());
        let provider_server =
            tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let builder = AgentBuilder::new(AgentBuilderConfig {
            kibana_url: url,
            api_key: "test-only-key".into(),
            space: None,
            index_prefix: "genesis".into(),
            timeout: Duration::from_secs(5),
        })
        .unwrap();
        let cfg = OrgConfig::dev("org-review", "Review organization");
        // No DevStub executor: this is a managed Elastic provider path.
        let app = build_org(&cfg, Some(ProviderSet::new()))
            .unwrap()
            .with_elastic_agents(builder.clone());
        let initial = app.current_state().unwrap();
        let bootstrap: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Research venues","roles":[{"key":"research","name":"Researcher","charter":"Analyze venue evidence",
                "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
            "responsibilities":[{"purpose":"Analyze venues","owner":"research"}]
        })).unwrap();
        let (outcome, mission) = bootstrap_mission(&initial, &bootstrap, Utc::now()).unwrap();
        let mut state = outcome.state;
        let role = state.roles.keys().next().unwrap().clone();
        let responsibility = state.responsibilities.keys().next().unwrap().clone();
        let spec = &state.roles[&role];
        let binding = builder
            .provision(&ProvisionRequest {
                org_id: state.org_id.clone(),
                change_id: ChangeId::new("change-review"),
                role: RoleToProvision {
                    key: "research".into(),
                    role_id: role.clone(),
                    name: spec.name.clone(),
                    charter: spec.charter.clone(),
                    authority: spec.authority.clone(),
                    provider: Provider::Elastic,
                },
            })
            .await
            .unwrap();
        state.roles.get_mut(&role).unwrap().binding = Some(binding);
        app.store
            .commit_state(
                initial.revision,
                &state,
                outcome.effects,
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();

        // Serve routes without a background worker. The test controls exactly
        // when the durable outbox is drained, so expiry cannot race dispatch.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let router = app.clone().router();
        let org_server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            app,
            cfg,
            base,
            http: reqwest::Client::new(),
            mission,
            role,
            responsibility,
            remote,
            servers: vec![provider_server, org_server],
        }
    }

    async fn dispatch(&self) -> Job {
        let response = self
            .http
            .post(format!("{}/jobs", self.base))
            .bearer_auth(&self.cfg.tokens.operator)
            .json(
                &json!({"client_request_id":"dispatch-review","role_id":self.role,
                "responsibility_id":self.responsibility,"task":"Analyze venues","input":{},
                "required_scope":null,"capability":null,"provider":"elastic","deadline":null}),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.unwrap()
    }

    async fn mutate(&self, change: impl FnOnce(&mut OrgState)) {
        let _guard = self.app.kernel.lock().await;
        let mut state = self.app.current_state().unwrap();
        let base = state.revision;
        change(&mut state);
        state.revision += 1;
        self.app
            .store
            .commit_state(base, &state, vec![], &EventMeta::default(), vec![], |_| {
                Ok(())
            })
            .unwrap();
    }

    async fn expire(&self, lease: bool) {
        self.mutate(|state| {
            let expired = Utc::now() - ChronoDuration::seconds(1);
            if lease {
                let id = state
                    .active_assignment(&self.responsibility)
                    .unwrap()
                    .assignment_id
                    .clone();
                state.assignments.get_mut(&id).unwrap().lease_expires_at = Some(expired);
            } else {
                state.missions.get_mut(&self.mission).unwrap().deadline = Some(expired);
            }
        })
        .await;
    }

    fn job(&self, id: &JobId) -> Job {
        self.app.store.get_job(id).unwrap()
    }
}

async fn must_not_dispatch_after_expiry(lease: bool) {
    let fixture = Fixture::new().await;
    let job = fixture.dispatch().await;
    fixture.expire(lease).await;
    outbox::drain_once(&fixture.app).await.unwrap();
    let saved = fixture.job(&job.job_id);
    assert_eq!(
        fixture.remote.calls.load(Ordering::SeqCst),
        0,
        "expired {} still dispatched a model call; saved job status={:?}",
        if lease {
            "assignment lease"
        } else {
            "mission deadline"
        },
        saved.status
    );
    assert!(matches!(
        saved.status,
        JobStatus::Failed | JobStatus::ResultRejected
    ));
}

async fn late_result_must_be_fenced(lease: bool) {
    let fixture = Fixture::new().await;
    fixture.remote.hold.store(true, Ordering::SeqCst);
    let job = fixture.dispatch().await;
    let app = fixture.app.clone();
    let drain = tokio::spawn(async move { outbox::drain_once(&app).await.unwrap() });
    tokio::time::timeout(Duration::from_secs(5), fixture.remote.entered.notified())
        .await
        .unwrap();
    fixture.expire(lease).await;
    fixture.remote.release.add_permits(1);
    drain.await.unwrap();
    let saved = fixture.job(&job.job_id);
    assert_eq!(
        saved.status,
        JobStatus::ResultRejected,
        "model answer after {} expiry was applied: result={:?}",
        if lease {
            "assignment lease"
        } else {
            "mission deadline"
        },
        saved.result
    );
    assert!(saved.result.is_none());
}

#[tokio::test]
async fn expired_mission_must_not_dispatch_a_managed_elastic_call() {
    must_not_dispatch_after_expiry(false).await;
}
#[tokio::test]
async fn expired_assignment_lease_must_not_dispatch_a_managed_elastic_call() {
    must_not_dispatch_after_expiry(true).await;
}
#[tokio::test]
async fn mission_expiry_during_converse_must_fence_the_answer() {
    late_result_must_be_fenced(false).await;
}
#[tokio::test]
async fn assignment_lease_expiry_during_converse_must_fence_the_answer() {
    late_result_must_be_fenced(true).await;
}

#[tokio::test]
async fn generic_adapter_result_must_not_accept_a_managed_elastic_answer() {
    let fixture = Fixture::new().await;
    let job = fixture.dispatch().await;
    outbox::drain_once(&fixture.app).await.unwrap();
    let job = fixture.job(&job.job_id);
    assert_eq!(job.status, JobStatus::Verifying);
    assert_eq!(job.result.as_ref().unwrap()["domain_verified"], false);
    let response = fixture.http.post(format!("{}/jobs/{}/results",fixture.base,job.job_id))
        .bearer_auth(&fixture.cfg.tokens.adapter)
        .json(&json!({"attempt_id":job.attempt_id,"epoch":job.epoch,"outcome":"succeeded",
            "provider_status":"SUCCEEDED","provider_refs":{},"output":{"unchecked":"adapter assertion"},
            "operation_id":"op-elastic-review-bypass","domain_verified":true,"completes_responsibility":true}))
        .send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    let saved = fixture.job(&job.job_id);
    let ledger: Value = fixture
        .http
        .get(format!("{}/ledger", fixture.base))
        .bearer_auth(&fixture.cfg.tokens.operator)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let state = fixture.app.current_state().unwrap();
    assert_eq!(status,StatusCode::CONFLICT,
        "generic Adapter accepted managed analysis: job={:?}, responsibility={:?}, ledger={}, response={}",
        saved.status,state.responsibilities[&fixture.responsibility].status,ledger,body);
    assert_eq!(saved.status, JobStatus::Verifying);
    assert!(ledger.as_array().unwrap().is_empty());
}

// Henry's Claude follow-ups: a caller cannot route an Elastic role's job
// around the backend path, and an answer is bound to the dispatched agent.

#[tokio::test]
async fn dispatch_cannot_route_an_elastic_role_around_its_agent() {
    let fixture = Fixture::new().await;
    for provider in ["internal", "dev_stub", "creao"] {
        let response = fixture
            .http
            .post(format!("{}/jobs", fixture.base))
            .bearer_auth(&fixture.cfg.tokens.operator)
            .json(
                &json!({"client_request_id":format!("override-{provider}"),"role_id":fixture.role,
                "responsibility_id":fixture.responsibility,"task":"Analyze venues","input":{},
                "required_scope":null,"capability":null,"provider":provider,"deadline":null}),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{provider}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"], "provider_mismatch", "{provider}: {body}");
    }
}

#[tokio::test]
async fn answer_is_fenced_when_the_role_is_rebound_during_converse() {
    let fixture = Fixture::new().await;
    fixture.remote.hold.store(true, Ordering::SeqCst);
    let job = fixture.dispatch().await;
    let app = fixture.app.clone();
    let drain = tokio::spawn(async move { outbox::drain_once(&app).await.unwrap() });
    tokio::time::timeout(Duration::from_secs(5), fixture.remote.entered.notified())
        .await
        .unwrap();
    let role = fixture.role.clone();
    fixture
        .mutate(|state| {
            state
                .roles
                .get_mut(&role)
                .unwrap()
                .binding
                .as_mut()
                .unwrap()
                .provider_agent_id = "a-different-agent".into();
        })
        .await;
    fixture.remote.release.add_permits(1);
    drain.await.unwrap();
    let saved = fixture.job(&job.job_id);
    assert_eq!(
        saved.status,
        JobStatus::ResultRejected,
        "{:?}",
        saved.result
    );
    assert!(saved.result.is_none());
}
