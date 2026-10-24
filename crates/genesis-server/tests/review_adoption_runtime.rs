//! Review regressions for guards in the durable adoption runtime that the
//! integration suite in `capability_adoption.rs` does not distinguish. They use
//! the real registry and organization HTTP services, the real Rust Apify client
//! and canned loopback replies. The organization is stepped manually so each
//! intermediate state is observable; nothing here claims live provider access.
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use chrono::{Duration as ChronoDuration, Utc};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::change::{KeyAllocation, OrgChangeProposal};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::ids::{InstallationId, JobId, RoleId};
use genesis_core::lifecycle::InstallationStatus;
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::app::AppState;
use genesis_server::capability_adoption;
use genesis_server::capability_adoption_store::{
    AdoptionAttention, AdoptionError, AdoptionFence, AdoptionPhase,
};
use genesis_server::capability_evidence::EvidencePolicy;
use genesis_server::capability_runtime::CapabilityRunPhase;
use genesis_server::error::validation_error;
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, parse_publishers, OrgConfig};
use genesis_verification::{decode_json, ArtifactPin, ExecutionEvidence, OracleSuite};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

const BASE: &str = "/capability-adoptions";
const KEY: &str = "PRIVATE_REVIEW_ADOPTION_KEY_7c1e0";
const REQUIREMENT: &str = "Read venue capacity price availability for the independent organization";
const CAPABILITY: &str = "shared-venue-reader";
const SEARCH: &str = "/registry/v1/search";

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

fn suite(independent: bool) -> OracleSuite {
    let mut s: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    if independent {
        s.suite_id = "independent-adoption-suite-b".into();
        s.scope = "PRIVATE_REVIEW_ADOPTION_ORACLE".into();
        for (index, case) in s.cases.iter_mut().enumerate() {
            case.case_id = format!("b-case-{index}");
            case.input["attendees"] = json!(case.input["attendees"].as_u64().unwrap() + 7);
        }
    }
    s
}
fn receipts() -> Vec<ExecutionEvidence> {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap()
}
fn pin() -> ArtifactPin {
    ArtifactPin {
        capability_id: CAPABILITY.into(),
        capability_version: "1.0.0".into(),
        source_sha256: "a".repeat(64),
        actor_id: "fixture-shared-actor".into(),
        build_id: "fixture-shared-build".into(),
        build_number: "1.0.1".into(),
    }
}
fn manifest(capability: &str, publisher: &str, permission: &str) -> Value {
    let artifact = pin();
    let mut evidence = receipts();
    for item in &mut evidence {
        item.artifact = artifact.clone();
    }
    let policy = EvidencePolicy::new(vec![suite(false)], vec![]).unwrap();
    let summary = policy
        .accept_suite(&suite(false).suite_id, &artifact, &evidence)
        .unwrap();
    json!({"capability_id":capability,"version":"1.0.0","publisher":publisher,
        "source_digest":artifact.source_sha256,"intent":REQUIREMENT,
        "applicability_conditions":["configured synthetic venue fixture inputs"],
        "known_limitations":["canned upstream fixture; fresh B evidence required"],
        "input_schema":{"type":"object"},"output_schema":{"type":"object"},
        "required_permissions":[permission],"artifact":{"actor_id":artifact.actor_id,
            "build_id":artifact.build_id,"build_number":artifact.build_number},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,
        "local_configuration_schema":{},"acceptance_suite_ref":suite(false).suite_id,
        "verification":summary.kernel_report,"provenance":["loopback-upstream-summary-only"],
        "state":"APPROVED","published_at":null})
}

/// How the loopback registry answers ranked search. Manifest reads always reach
/// the real registry, so a selected candidate is still canonical.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Search {
    Pass,
    /// Repeat the genuine foreign candidate this many times.
    Count(usize),
    /// Replace every returned candidate score.
    Score(f64),
    Fail,
}
struct RegistryControl {
    mode: Search,
    /// Hold search requests (after logging them) until cleared.
    hang: bool,
    seen: Vec<String>,
}
async fn registry_layer(
    State(control): State<Arc<Mutex<RegistryControl>>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    control.lock().unwrap().seen.push(path.clone());
    if path != SEARCH {
        return next.run(request).await;
    }
    loop {
        let hang = control.lock().unwrap().hang;
        if !hang {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mode = control.lock().unwrap().mode;
    if mode == Search::Fail {
        return (StatusCode::SERVICE_UNAVAILABLE, "fixture registry outage").into_response();
    }
    let (parts, body) = next.run(request).await.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    if parts.status != StatusCode::OK || mode == Search::Pass {
        return Response::from_parts(parts, Body::from(bytes));
    }
    let mut candidates: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
    match mode {
        Search::Count(count) => {
            let foreign = candidates
                .iter()
                .find(|c| c["capability_id"] == CAPABILITY)
                .cloned()
                .expect("genuine foreign candidate");
            candidates = vec![foreign; count];
        }
        Search::Score(score) => {
            for candidate in &mut candidates {
                candidate["score"] = json!(score);
            }
        }
        Search::Pass | Search::Fail => unreachable!(),
    }
    Json(candidates).into_response()
}

#[derive(Clone, Copy)]
enum Behavior {
    Good,
    Wrong,
}
struct World {
    behavior: Behavior,
    posts: usize,
    runs: BTreeMap<usize, usize>,
}
/// Accepts submissions as RUNNING (a submission never claims completion);
/// a later read of the same run reports SUCCEEDED with its dataset.
async fn apify(
    State(world): State<Arc<Mutex<World>>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    assert_eq!(request.headers()["authorization"], format!("Bearer {KEY}"));
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let mut world = world.lock().unwrap();
    let pin = pin();
    if method == "GET" && path == format!("/v2/actor-builds/{}", pin.build_id) {
        return (
            StatusCode::OK,
            Json(json!({"data":{"id":pin.build_id,"actId":pin.actor_id,
            "buildNumber":pin.build_number,"status":"SUCCEEDED"}})),
        );
    }
    if method == "POST" && path == format!("/v2/actors/{}/runs", pin.actor_id) {
        let index = suite(true)
            .cases
            .iter()
            .position(|case| case.input == body)
            .expect("only B's own configured input may execute");
        world.posts += 1;
        let run = world.runs.len();
        world.runs.insert(run, index);
        return (StatusCode::CREATED, Json(run_reply(run, "RUNNING")));
    }
    if method == "GET" {
        if let Some(run) = path
            .strip_prefix("/v2/actor-runs/b-run-")
            .and_then(|s| s.parse::<usize>().ok())
        {
            if world.runs.contains_key(&run) {
                return (StatusCode::OK, Json(run_reply(run, "SUCCEEDED")));
            }
        }
        if let Some(run) = path
            .strip_prefix("/v2/datasets/b-dataset-")
            .and_then(|s| s.strip_suffix("/items"))
            .and_then(|s| s.parse::<usize>().ok())
        {
            if let Some(case) = world.runs.get(&run) {
                let mut output = receipts()[*case].output.clone();
                if matches!(world.behavior, Behavior::Wrong) && *case == 0 {
                    output["venues"][0]["capacity"] = json!(999);
                }
                return (StatusCode::OK, Json(json!([output])));
            }
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error":"unexpected loopback request"})),
    )
}
fn run_reply(run: usize, status: &str) -> Value {
    json!({"data":{"id":format!("b-run-{run}"),"actId":pin().actor_id,"buildId":pin().build_id,
        "status":status,"defaultDatasetId":format!("b-dataset-{run}")}})
}

struct Fixture {
    _dir: tempfile::TempDir,
    _registry: Server,
    provider: Server,
    registry: Arc<Mutex<RegistryControl>>,
    world: Arc<Mutex<World>>,
    server: Server,
    app: AppState,
    config: OrgConfig,
    http: reqwest::Client,
    body: Value,
}
impl Fixture {
    async fn new(behavior: Behavior) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .unwrap();
        let control = Arc::new(Mutex::new(RegistryControl {
            mode: Search::Pass,
            hang: false,
            seen: vec![],
        }));
        let registry = Server::start(
            registry::router(
                RegistryState::open_in_memory(
                    parse_publishers("org-a:a-registry,org-b:b-registry"),
                    vec![],
                )
                .unwrap(),
            )
            .layer(middleware::from_fn_with_state(
                control.clone(),
                registry_layer,
            )),
        )
        .await;
        for (capability, publisher, permission, token) in [
            (CAPABILITY, "org-a", "capability.venue_lookup", "a-registry"),
            (
                "self-venue-reader",
                "org-b",
                "capability.venue_lookup",
                "b-registry",
            ),
            (
                "forbidden-venue-reader",
                "org-a",
                "capability.private",
                "a-registry",
            ),
        ] {
            let response = http
                .post(format!("{}/registry/v1/manifests", registry.url))
                .bearer_auth(token)
                .json(&manifest(capability, publisher, permission))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        control.lock().unwrap().seen.clear();
        let world = Arc::new(Mutex::new(World {
            behavior,
            posts: 0,
            runs: BTreeMap::new(),
        }));
        let provider = Server::start(Router::new().fallback(apify).with_state(world.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev("org-b", "Independent adopter");
        config.db = Some(dir.path().join("b.sqlite"));
        config.registry_url = Some(registry.url.clone());
        config.registry_token = Some("b-registry".into());
        config.oracle_suites = vec![suite(true)];
        let app = build_org(&config, Some(providers(&provider.url))).unwrap();
        let body = seed(&app).await;
        let server = Server::start(app.clone().router()).await;
        Self {
            _dir: dir,
            _registry: registry,
            provider,
            registry: control,
            world,
            server,
            app,
            config,
            http,
            body,
        }
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .http
            .post(format!("{}/api/v1{path}", self.server.url))
            .bearer_auth("org-b-operator")
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
    async fn get(&self, path: &str) -> Value {
        let response = self
            .http
            .get(format!("{}/api/v1{path}", self.server.url))
            .bearer_auth("org-b-operator")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.json().await.unwrap()
    }
    async fn create_as(&self, client_request_id: &str) -> String {
        let mut body = self.body.clone();
        body["client_request_id"] = json!(client_request_id);
        let (status, created) = self.post(BASE, body).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        created["id"].as_str().unwrap().to_owned()
    }
    async fn create(&self) -> String {
        self.create_as("review-adoption").await
    }
    async fn step(&self, id: &str) -> Value {
        let (status, body) = self.post(&format!("{BASE}/{id}/step"), json!({})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
    async fn step_child(&self, job: &str) -> Value {
        let (status, body) = self
            .post(&format!("/capability-runs/{job}/step"), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
    fn jobs(&self, id: &str) -> Vec<String> {
        self.app
            .store
            .get_adoption(id)
            .unwrap()
            .job_ids()
            .iter()
            .map(|job| job.to_string())
            .collect()
    }
    /// Discovery, then the atomic check batch, with no provider effect yet.
    async fn to_checking(&self, id: &str) -> InstallationId {
        let posts = self.posts();
        let selected = self.step(id).await;
        assert_eq!(selected["phase"], "selected", "{selected}");
        let checking = self.step(id).await;
        assert_eq!(checking["phase"], "checking", "{checking}");
        assert_eq!(
            self.posts(),
            posts,
            "stages delegate effects to run workers"
        );
        InstallationId::new(checking["installation_id"].as_str().unwrap())
    }
    /// Submit (provider replies RUNNING), then collect every child run.
    async fn collect_all(&self, id: &str) {
        for job in self.jobs(id) {
            assert_eq!(self.step_child(&job).await["phase"], "running");
            assert_eq!(self.step_child(&job).await["phase"], "collected");
        }
    }
    fn posts(&self) -> usize {
        self.world.lock().unwrap().posts
    }
    fn searches(&self) -> usize {
        self.registry_requests(SEARCH)
    }
    fn registry_requests(&self, prefix: &str) -> usize {
        self.registry
            .lock()
            .unwrap()
            .seen
            .iter()
            .filter(|path| path.starts_with(prefix))
            .count()
    }
    fn installation(&self, id: &InstallationId) -> genesis_core::model::Installation {
        self.app.current_state().unwrap().installations[id].clone()
    }
    async fn restart(&mut self) {
        self.server.task.abort();
        self.app = build_org(&self.config, Some(providers(&self.provider.url))).unwrap();
        self.server = Server::start(self.app.clone().router()).await;
    }
    async fn edit_state(&self, edit: impl FnOnce(&mut genesis_core::model::OrgState)) {
        self.app
            .mutate(EventMeta::default(), |state| {
                let mut next = state.clone();
                edit(&mut next);
                next.revision += 1;
                Ok((
                    engine::Outcome {
                        state: next,
                        effects: vec![],
                    },
                    (),
                ))
            })
            .await
            .unwrap();
    }
}
fn providers(url: &str) -> ProviderSet {
    let client = ApifyClient::for_local_testing(
        KEY,
        RunLimits {
            request_timeout: Duration::from_secs(2),
            deadline: Duration::from_secs(5),
            poll_interval: Duration::from_millis(1),
            ..RunLimits::default()
        },
        url,
    )
    .unwrap();
    ProviderSet::new()
        .with_dev_stub(DevStub::default())
        .with_executor(Arc::new(ApifyCapabilityExecutor::new(client)))
}
async fn seed(app: &AppState) -> Value {
    let spec: MissionBootstrap = serde_json::from_value(json!({"goal":"Independently inherit venue lookup",
        "roles":[{"key":"verifier","name":"Verifier","charter":"Independently validate inherited work",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Coordinate local acceptance","owner":"verifier"}]})).unwrap();
    let mission = app
        .mutate(EventMeta::default(), |state| {
            engine::bootstrap_mission(state, &spec, Utc::now()).map_err(validation_error)
        })
        .await
        .unwrap()
        .value;
    let observation = app
        .mutate(EventMeta::default(), |state| {
            engine::record_observation(
                state,
                &mission,
                "operator",
                "Missing venue capability",
                None,
                Utc::now(),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let requirement = app
        .mutate(EventMeta::default(), |state| {
            engine::record_requirement(
                state,
                &mission,
                REQUIREMENT,
                std::slice::from_ref(&observation),
                "Own oracle independently accepts venue output",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let role = state.roles.keys().next().unwrap().clone();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({"expected_org_revision":state.revision,
        "trigger_evidence_ids":[observation],"rationale":"Assign independent requirement","budget_limit_cents":0,
        "operations":[{"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"adoption",
            "requirement_id":requirement,"purpose":"Adopt venue capability","trigger":"Missing capability",
            "termination":"Locally accepted capability"},"owner":{"kind":"existing","role_id":role}}]})).unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    let responsibility = allocation.responsibilities["adoption"].clone();
    app.mutate(EventMeta::default(), |state| {
        Ok((
            engine::commit(
                state,
                &proposal,
                &allocation,
                &Default::default(),
                &engine::local_catalog(state),
                Utc::now(),
            )
            .map_err(validation_error)?,
            (),
        ))
    })
    .await
    .unwrap();
    json!({"client_request_id":"review-adoption","mission_id":mission,"requirement_id":requirement,
        "role_id":role,"responsibility_id":responsibility,"suite_id":suite(true).suite_id,
        "provider_access_acknowledged":true,"deadline":(Utc::now()+ChronoDuration::minutes(15)).to_rfc3339()})
}

fn assert_waiting(status: &Value) {
    assert_eq!(status["phase"], "checking", "{status}");
    for field in [
        "acceptance_ref",
        "rejection_ref",
        "attention",
        "fence_reason",
    ] {
        assert!(status[field].is_null(), "{field}: {status}");
    }
    assert!(status["next_attempt_at"].is_string(), "{status}");
}

/// capability_adoption.rs `finalize`: `if pending`. Until every child run is
/// Collected, the intent keeps waiting in Checking and neither accepts nor
/// rejects, whether the children are still Prepared, provider-RUNNING, or only
/// partly collected. The verdict follows the final collection.
#[tokio::test]
async fn checking_waits_for_every_child_run_to_be_collected_before_any_verdict() {
    let f = Fixture::new(Behavior::Good).await;
    let id = f.create().await;
    let installation = f.to_checking(&id).await;
    let jobs = f.jobs(&id);
    assert_eq!(jobs.len(), 3);
    let unaccepted = |f: &Fixture| {
        assert_eq!(
            f.installation(&installation).status,
            InstallationStatus::Checking
        );
        assert!(f
            .app
            .store
            .capability_acceptance(&installation)
            .unwrap()
            .is_none());
    };

    let mut version = f.get(&format!("{BASE}/{id}")).await["version"]
        .as_u64()
        .unwrap();
    let mut wait = |status: Value| {
        assert_waiting(&status);
        let next = status["version"].as_u64().unwrap();
        assert!(next > version, "a wait is a durable reschedule: {status}");
        version = next;
    };

    // Batch prepared, nothing submitted.
    wait(f.step(&id).await);
    unaccepted(&f);
    // Every run submitted; the provider reported RUNNING.
    for job in &jobs {
        assert_eq!(f.step_child(job).await["phase"], "running");
    }
    wait(f.step(&id).await);
    unaccepted(&f);
    // Two of three collected.
    for job in &jobs[..2] {
        assert_eq!(f.step_child(job).await["phase"], "collected");
    }
    wait(f.step(&id).await);
    unaccepted(&f);
    assert_eq!(
        f.app
            .store
            .get_capability_run(&JobId::new(&jobs[2]))
            .unwrap()
            .phase,
        CapabilityRunPhase::Running
    );

    assert_eq!(f.step_child(&jobs[2]).await["phase"], "collected");
    let accepted = f.step(&id).await;
    assert_eq!(accepted["phase"], "accepted", "{accepted}");
    assert_eq!(
        f.installation(&installation).status,
        InstallationStatus::Accepted
    );
    let proof = f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .unwrap();
    assert_eq!(
        proof
            .job_ids
            .iter()
            .map(|j| j.to_string())
            .collect::<Vec<_>>(),
        jobs
    );
    assert_eq!(f.posts(), 3);
}

/// capability_adoption.rs `finalize`: `!authorized_job(...)` and `stop`:
/// `installation.status != Checking`. Every child is collected with correct
/// output, but the installation the checks were bound to is no longer
/// Checking (closed outside this intent). Finalize must stop at ChecksFailed
/// without accepting, and stopping must leave the externally closed
/// installation and its reference untouched instead of closing it again.
#[tokio::test]
async fn collected_checks_for_an_installation_closed_elsewhere_stop_without_acceptance() {
    let f = Fixture::new(Behavior::Good).await;
    let id = f.create().await;
    let installation = f.to_checking(&id).await;
    f.collect_all(&id).await;
    let target = installation.clone();
    f.edit_state(|state| {
        let closed = state.installations.get_mut(&target).unwrap();
        closed.status = InstallationStatus::Rejected;
        closed.acceptance_report_ref = Some("external-close".into());
    })
    .await;

    let stopped = f.step(&id).await;
    assert_eq!(stopped["phase"], "attention", "{stopped}");
    assert_eq!(stopped["attention"], "checks_failed", "{stopped}");
    assert!(stopped["acceptance_ref"].is_null());
    assert!(stopped["rejection_ref"].is_null());
    let closed = f.installation(&installation);
    assert_eq!(closed.status, InstallationStatus::Rejected);
    assert_eq!(
        closed.acceptance_report_ref.as_deref(),
        Some("external-close")
    );
    assert!(f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .is_none());
    assert_eq!(f.step(&id).await, stopped, "terminal replay is inert");
    assert_eq!(f.posts(), 3);
}

/// Authority lost after every check was collected (the role's scope removed)
/// is caught by `reconcile` before `finalize` runs: the intent is fenced and
/// its installation closed as stopped, with no acceptance. This documents why
/// role/scope/requirement changes do not reach the `authorized_job` guard.
#[tokio::test]
async fn scope_removed_after_collection_fences_instead_of_accepting() {
    let f = Fixture::new(Behavior::Good).await;
    let id = f.create().await;
    let installation = f.to_checking(&id).await;
    f.collect_all(&id).await;
    let role = RoleId::new(f.body["role_id"].as_str().unwrap());
    f.edit_state(|state| {
        state
            .roles
            .get_mut(&role)
            .unwrap()
            .authority
            .scopes
            .remove("capability.venue_lookup");
    })
    .await;
    let fenced = f.step(&id).await;
    assert_eq!(fenced["phase"], "fenced", "{fenced}");
    assert_eq!(fenced["fence_reason"], "authority_changed", "{fenced}");
    assert!(fenced["acceptance_ref"].is_null());
    let closed = f.installation(&installation);
    assert_eq!(closed.status, InstallationStatus::Rejected);
    assert_eq!(
        closed.acceptance_report_ref,
        Some(format!("adoption-stopped:{id}:{}", fenced["version"]))
    );
    assert!(f
        .app
        .store
        .capability_acceptance(&installation)
        .unwrap()
        .is_none());
}

/// capability_adoption.rs `step`: `discovery_attempts() >= 3`. Discovery claims
/// interrupted after the registry request was sent still count. After exactly
/// three claims the next step stops at RegistryUnavailable without a fourth
/// search; the second and third claims still search.
#[tokio::test]
async fn interrupted_discovery_claims_stop_after_exactly_three_searches() {
    let f = Fixture::new(Behavior::Good).await;
    let id = f.create().await;
    f.registry.lock().unwrap().hang = true;
    for attempt in 1..=3u32 {
        let interrupted = tokio::time::timeout(
            Duration::from_millis(200),
            capability_adoption::step(&f.app, &id),
        )
        .await;
        assert!(
            interrupted.is_err(),
            "discovery must still await the registry"
        );
        assert_eq!(f.searches(), attempt as usize, "claim {attempt} searched");
        let record = f.app.store.get_adoption(&id).unwrap();
        assert_eq!(record.phase(), AdoptionPhase::Discovering);
        assert_eq!(record.discovery_attempts(), attempt);
    }
    f.registry.lock().unwrap().hang = false;
    let stopped = capability_adoption::step(&f.app, &id).await.unwrap();
    assert_eq!(stopped.phase(), AdoptionPhase::Attention);
    assert_eq!(
        stopped.attention_code(),
        Some(AdoptionAttention::RegistryUnavailable)
    );
    assert_eq!(stopped.discovery_attempts(), 3);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(f.searches(), 3, "no fourth registry search");
    assert!(f.app.current_state().unwrap().installations.is_empty());
}

/// capability_adoption.rs `discover`: `candidates.len() > 5` and the
/// finite/positive score guard. More than the requested five candidates, or any
/// zero, negative or non-finite score, is an invalid registry answer refused
/// before any manifest read or installation. Exactly five is accepted.
#[tokio::test]
async fn registry_answers_with_too_many_candidates_or_unusable_scores_are_refused() {
    let f = Fixture::new(Behavior::Good).await;
    for (index, mode) in [
        Search::Count(6),
        Search::Score(0.0),
        Search::Score(-0.5),
        // Overflows f32 when decoded: an infinite score.
        Search::Score(1e39),
    ]
    .into_iter()
    .enumerate()
    {
        f.registry.lock().unwrap().mode = mode;
        let id = f.create_as(&format!("invalid-candidates-{index}")).await;
        let manifests = f.registry_requests("/registry/v1/capabilities/");
        let refused = f.step(&id).await;
        assert_eq!(refused["phase"], "attention", "{mode:?}: {refused}");
        assert_eq!(
            refused["attention"], "invalid_candidate",
            "{mode:?}: {refused}"
        );
        assert!(refused["installation_id"].is_null());
        assert_eq!(
            f.registry_requests("/registry/v1/capabilities/"),
            manifests,
            "{mode:?}: no manifest read for an invalid answer"
        );
    }
    assert_eq!(f.searches(), 4);
    assert!(f.app.current_state().unwrap().installations.is_empty());

    f.registry.lock().unwrap().mode = Search::Count(5);
    let id = f.create_as("five-candidates").await;
    let selected = f.step(&id).await;
    assert_eq!(selected["phase"], "selected", "{selected}");
    assert_eq!(selected["selection"]["capability_id"], CAPABILITY);
}

/// capability_adoption.rs `reconcile`: `!valid || configured(app).is_err()`.
/// A trusted suite changed before discovery pauses the intent at
/// ConfigurationChanged without any registry read.
#[tokio::test]
async fn changed_trusted_suite_pauses_discovery_before_any_registry_read() {
    let mut f = Fixture::new(Behavior::Good).await;
    let id = f.create().await;
    f.config.oracle_suites[0].scope.push_str(" changed policy");
    f.restart().await;
    let paused = f.step(&id).await;
    assert_eq!(paused["phase"], "attention", "{paused}");
    assert_eq!(paused["attention"], "configuration_changed", "{paused}");
    assert_eq!(paused["discovery_attempts"], 0);
    assert_eq!(f.searches(), 0);
    assert!(f.app.current_state().unwrap().installations.is_empty());
}

/// capability_adoption.rs `step`: `current.version() != record.version()`.
/// If the durable record changes while discovery awaits the registry (here a
/// direct store write, as another process sharing the database could make),
/// the stale step must refuse with a conflict instead of selecting from it.
#[tokio::test]
async fn record_changed_during_discovery_refuses_the_stale_selection() {
    let f = Fixture::new(Behavior::Good).await;
    let id = f.create().await;
    f.registry.lock().unwrap().hang = true;
    let app = f.app.clone();
    let stepping = id.clone();
    let task = tokio::spawn(async move { capability_adoption::step(&app, &stepping).await });
    for _ in 0..200 {
        if f.searches() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(f.searches(), 1);
    let claimed = f.app.store.get_adoption(&id).unwrap();
    assert_eq!(claimed.discovery_attempts(), 1);
    let now = Utc::now();
    let rescheduled = claimed
        .wait_until(now + ChronoDuration::seconds(1), now)
        .unwrap()
        .for_revision(f.app.current_state().unwrap().revision)
        .unwrap();
    f.app.store.save_adoption(&claimed, &rescheduled).unwrap();
    f.registry.lock().unwrap().hang = false;

    let refused = task
        .await
        .unwrap()
        .expect_err("stale discovery must not commit");
    assert_eq!(refused.status.as_u16(), 409);
    assert_eq!(refused.error, "adoption_conflict");
    let current = f.app.store.get_adoption(&id).unwrap();
    assert_eq!(current.version(), rescheduled.version());
    assert_eq!(current.phase(), AdoptionPhase::Discovering);
    assert!(current.installation_id().is_none());
    assert!(f.app.current_state().unwrap().installations.is_empty());
}

/// capability_adoption_store.rs `fence` precondition and the save-time
/// transition table: semantic verdicts (Accepted, Rejected) and Fenced records
/// cannot be fenced, while an Attention record can be fenced and saved.
/// Also pins the 256-byte domain-ID boundary on the request key.
#[tokio::test]
async fn verdicts_cannot_be_fenced_but_attention_can_be_fenced_and_saved() {
    let f = Fixture::new(Behavior::Wrong).await;
    let fence = |id: &str| {
        f.app
            .store
            .get_adoption(id)
            .unwrap()
            .fence(AdoptionFence::AuthorityChanged, Utc::now())
    };

    let rejected = f.create_as("verdict-rejected").await;
    f.to_checking(&rejected).await;
    f.collect_all(&rejected).await;
    assert_eq!(f.step(&rejected).await["phase"], "rejected");
    assert!(matches!(
        fence(&rejected),
        Err(AdoptionError::InvalidRecord)
    ));

    f.world.lock().unwrap().behavior = Behavior::Good;
    let accepted = f.create_as("verdict-accepted").await;
    f.to_checking(&accepted).await;
    f.collect_all(&accepted).await;
    assert_eq!(f.step(&accepted).await["phase"], "accepted");
    assert!(matches!(
        fence(&accepted),
        Err(AdoptionError::InvalidRecord)
    ));

    f.registry.lock().unwrap().mode = Search::Fail;
    let longest = "k".repeat(256);
    let (status, _) = f
        .post(BASE, {
            let mut body = f.body.clone();
            body["client_request_id"] = json!("k".repeat(257));
            body
        })
        .await;
    assert!(status.is_client_error(), "{status}");
    let paused = f.create_as(&longest).await;
    let attention = f.step(&paused).await;
    assert_eq!(
        attention["attention"], "registry_unavailable",
        "{attention}"
    );
    let record = f.app.store.get_adoption(&paused).unwrap();
    let next = record
        .fence(AdoptionFence::AuthorityChanged, Utc::now())
        .unwrap()
        .for_revision(f.app.current_state().unwrap().revision)
        .unwrap();
    f.app.store.save_adoption(&record, &next).unwrap();
    let fenced = f.get(&format!("{BASE}/{paused}")).await;
    assert_eq!(fenced["phase"], "fenced", "{fenced}");
    assert!(fenced["attention"].is_null());
    assert!(matches!(fence(&paused), Err(AdoptionError::InvalidRecord)));
}
