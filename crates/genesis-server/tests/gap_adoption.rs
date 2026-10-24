//! Henry's Claude: adopt-before-acquire for capability gaps. Real kernel
//! commits and SQLite, a real registry server, and real Rust Apify/model
//! clients against a loopback provider. Org A's publication is a fixture
//! manifest; B discovers, installs and verifies it with its own suite. No
//! source runs locally and no live provider is contacted.
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    Json, Router,
};
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::{
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap, CAPABILITY_GAP_SOURCE},
    ids::*,
    lifecycle::InstallationStatus,
    model::{CapabilityGapStatus, OrgState},
};
use genesis_forge::{AnthropicGenerator, ModelConfig, PublicExample};
use genesis_providers::{ApifyCapabilityExecutor, ProviderSet};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    capability_adoption,
    capability_adoption_store::{AdoptionAttention, AdoptionPhase},
    capability_evidence::EvidencePolicy,
    capability_gap_adoption::{
        self, GapAdoptionCode, GapAdoptionPolicy, GapAdoptionRule, GapAdoptionState, Step,
    },
    capability_gap_driver,
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    parse_publishers,
    registry::RegistryState,
    serve_org, spawn_registry,
    store::EventMeta,
    AcquisitionConfig, OrgConfig,
};
use genesis_verification::{decode_json, ArtifactPin, ExecutionEvidence, OracleSuite};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

const MODEL: &str = "claude-sonnet-4-6";
const KEY: &str = "gap-adoption-loopback-key";
const ORG: &str = "org-b";
const PERMISSION: &str = "capability.venue_lookup";
const REQUESTED: &str = "venue-reader";
const FOREIGN: &str = "shared-venue-reader";
const REQUIREMENT: &str = "Read venue capacity price availability for the independent organization";
const SOURCE: &str = "https://fixtures.genesis.example/venues/index.html";
/// A second ruled gap on the same requirement (the kernel allows three).
const SIBLING: &str = "venue-reader-two";

fn trusted_suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
/// B's own frozen suite: different case IDs and inputs from A's.
fn b_suite() -> OracleSuite {
    let mut s = trusted_suite();
    s.suite_id = "gap-adoption-suite-b".into();
    for (index, case) in s.cases.iter_mut().enumerate() {
        case.case_id = format!("b-case-{index}");
        case.input["attendees"] = json!(case.input["attendees"].as_u64().unwrap() + 7);
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
        capability_id: FOREIGN.into(),
        capability_version: "1.0.0".into(),
        source_sha256: "a".repeat(64),
        actor_id: "fixture-shared-actor".into(),
        build_id: "fixture-shared-build".into(),
        build_number: "1.0.1".into(),
    }
}
fn manifest(capability: &str, publisher: &str) -> Value {
    let artifact = pin();
    let mut evidence = receipts();
    for item in &mut evidence {
        item.artifact = artifact.clone();
    }
    let policy = EvidencePolicy::new(vec![trusted_suite()], vec![]).unwrap();
    let summary = policy
        .accept_suite(&trusted_suite().suite_id, &artifact, &evidence)
        .unwrap();
    json!({"capability_id":capability,"version":"1.0.0","publisher":publisher,
        "source_digest":artifact.source_sha256,"intent":REQUIREMENT,
        "applicability_conditions":["configured synthetic venue fixture inputs"],
        "known_limitations":["canned upstream fixture; fresh B evidence required"],
        "input_schema":{"type":"object"},"output_schema":{"type":"object"},
        "required_permissions":[PERMISSION],"artifact":{"actor_id":artifact.actor_id,
            "build_id":artifact.build_id,"build_number":artifact.build_number},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,
        "local_configuration_schema":{},"acceptance_suite_ref":trusted_suite().suite_id,
        "verification":summary.kernel_report,"provenance":["loopback-upstream-summary-only"],
        "state":"APPROVED","published_at":null})
}

#[derive(Default)]
struct World {
    ambiguous_runs: bool,
    /// Native runs remain RUNNING until explicitly changed by the fixture.
    running_runs: bool,
    /// Simulate revoked read access without changing a native run's status.
    deny_run_reads: bool,
    /// Next run submissions that are created but finish FAILED (definite).
    failing_runs: usize,
    failed: BTreeSet<usize>,
    /// Collected outputs disagree with B's suite (same domain, incompatible).
    wrong_outputs: bool,
    /// Next submissions whose POST response already reports the run FAILED
    /// (a definite terminal result, recorded with no receipt).
    failing_submits: usize,
    requests: Vec<(String, String)>,
    runs: BTreeMap<usize, usize>,
}
impl World {
    fn run(&self, run: usize) -> Value {
        json!({"data":{"id":format!("b-run-{run}"),"actId":pin().actor_id,"buildId":pin().build_id,
            "status":if self.running_runs { "RUNNING" } else { "SUCCEEDED" },
            "defaultDatasetId":format!("b-dataset-{run}")}})
    }
    fn reply(&mut self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.requests.push((method.into(), path.into()));
        let pin = pin();
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "messages"]) => (
                StatusCode::OK,
                json!({"id":"msg_gap_adoption","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":json!({"extractor_typescript":"export function extract() { throw new Error('never executed'); }"}).to_string()}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":10}}),
            ),
            ("POST", ["v2", "actors"]) => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"synthesis is not part of these fixtures"}),
            ),
            ("GET", ["v2", "actor-builds", build]) if *build == pin.build_id => (
                StatusCode::OK,
                json!({"data":{"id":pin.build_id,"actId":pin.actor_id,"buildNumber":pin.build_number,"status":"SUCCEEDED"}}),
            ),
            ("POST", ["v2", "actors", actor, "runs"]) if *actor == pin.actor_id => {
                let case = b_suite()
                    .cases
                    .iter()
                    .position(|case| case.input == body)
                    .expect("only B's own suite inputs may execute");
                let run = self.runs.len();
                self.runs.insert(run, case);
                if self.failing_runs > 0 {
                    self.failing_runs -= 1;
                    self.failed.insert(run);
                }
                if self.ambiguous_runs {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error":"PRIVATE_AMBIGUOUS_RUN"}),
                    );
                }
                if self.failing_submits > 0 {
                    self.failing_submits -= 1;
                    self.failed.insert(run);
                    let mut reply = self.run(run);
                    reply["data"]["status"] = json!("FAILED");
                    return (StatusCode::CREATED, reply);
                }
                (StatusCode::CREATED, self.run(run))
            }
            ("GET", ["v2", "actor-runs", run]) => {
                if self.deny_run_reads {
                    return (
                        StatusCode::UNAUTHORIZED,
                        json!({"error":"read access denied"}),
                    );
                }
                let run: usize = run.strip_prefix("b-run-").unwrap().parse().unwrap();
                let mut reply = self.run(run);
                if self.failed.contains(&run) {
                    reply["data"]["status"] = json!("FAILED");
                }
                (StatusCode::OK, reply)
            }
            ("GET", ["v2", "datasets", dataset, "items"]) => {
                let run: usize = dataset.strip_prefix("b-dataset-").unwrap().parse().unwrap();
                let mut output = receipts()[self.runs[&run]].output.clone();
                if self.wrong_outputs {
                    // Same shape, different facts: B's own suite must reject it.
                    if let Some(venue) = output["venues"].get_mut(0) {
                        venue["capacity"] = json!(venue["capacity"].as_u64().unwrap_or(0) + 7);
                    }
                }
                (StatusCode::OK, json!([output]))
            }
            _ => (
                StatusCode::NOT_FOUND,
                json!({"error":"unexpected loopback route"}),
            ),
        }
    }
}

struct Providers {
    world: Arc<Mutex<World>>,
    model: Arc<AnthropicGenerator>,
    apify: Arc<ApifyClient>,
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Providers {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Providers {
    async fn start(ambiguous_runs: bool, running_runs: bool) -> Self {
        let world = Arc::new(Mutex::new(World {
            ambiguous_runs,
            running_runs,
            ..World::default()
        }));
        let router = Router::new()
            .fallback(
                |State(world): State<Arc<Mutex<World>>>, request: Request| async move {
                    let method = request.method().to_string();
                    let path = request.uri().path().to_owned();
                    let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
                        .await
                        .unwrap();
                    let body = if bytes.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes).unwrap()
                    };
                    let (status, body) = world.lock().unwrap().reply(&method, &path, body);
                    (status, Json(body))
                },
            )
            .with_state(world.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let model = Arc::new(
            AnthropicGenerator::with_test_endpoint(&format!("{url}/v1/messages"), KEY).unwrap(),
        );
        let apify = Arc::new(ApifyClient::for_local_testing(KEY, limits(), &url).unwrap());
        Self {
            world,
            model,
            apify,
            url,
            task,
        }
    }
    fn count(&self, method: &str, prefix: &str) -> usize {
        self.world
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(m, p)| m == method && p.starts_with(prefix))
            .count()
    }
    fn model_calls(&self) -> usize {
        self.count("POST", "/v1/messages")
    }
    fn actor_creations(&self) -> usize {
        self.world
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(m, p)| m == "POST" && p == "/v2/actors")
            .count()
    }
    fn runs(&self) -> usize {
        self.count("POST", &format!("/v2/actors/{}/runs", pin().actor_id))
    }
    fn wrong_outputs(&self) {
        self.world.lock().unwrap().wrong_outputs = true;
    }
    fn fail_submits(&self, n: usize) {
        self.world.lock().unwrap().failing_submits = n;
    }
    fn fail_runs(&self, n: usize) {
        self.world.lock().unwrap().failing_runs = n;
    }
    fn deny_run_reads(&self, denied: bool) {
        self.world.lock().unwrap().deny_run_reads = denied;
    }
}
fn limits() -> RunLimits {
    RunLimits {
        request_timeout: Duration::from_secs(2),
        deadline: Duration::from_secs(5),
        poll_interval: Duration::from_millis(1),
        ..RunLimits::default()
    }
}
fn provider_set(p: &Providers) -> ProviderSet {
    let client = ApifyClient::for_local_testing(KEY, limits(), &p.url).unwrap();
    ProviderSet::new().with_executor(Arc::new(ApifyCapabilityExecutor::new(client)))
}

#[derive(Clone, Copy)]
struct Options {
    /// Org A's foreign publication is in the registry.
    foreign: bool,
    registry_up: bool,
    ambiguous_runs: bool,
    running_runs: bool,
    fallback: bool,
    adopt_first: bool,
    sibling: bool,
    /// Searches that fail before the registry answers.
    failed_searches: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            foreign: true,
            registry_up: true,
            ambiguous_runs: false,
            running_runs: false,
            fallback: true,
            adopt_first: true,
            sibling: false,
            failed_searches: 0,
        }
    }
}

fn gap_policy() -> GapAcquisitionPolicy {
    let suite = b_suite();
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: REQUESTED.into(),
            suite_id: suite.suite_id.clone(),
            actor_name_prefix: "gap-reader".into(),
            allowed_source_urls: suite.cases.iter().map(|c| c.source.url.clone()).collect(),
            examples: vec![PublicExample {
                url: suite.cases[0].source.url.clone(),
                html: "<html>FIXED_PUBLIC_EXAMPLE</html>".into(),
            }],
            repair: false,
        }],
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    config: OrgConfig,
    app: AppState,
    gap_id: CapabilityGapId,
    requirement_id: RequirementId,
    providers: Arc<Providers>,
    registry_searches: Arc<Mutex<usize>>,
    server: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(task) = self.server.take() {
            task.abort();
        }
    }
}

async fn registry(foreign: bool, searches: Arc<Mutex<usize>>, fail_first: usize) -> String {
    let state = RegistryState::open_in_memory(
        parse_publishers("org-a:a-registry,org-b:b-registry"),
        vec![],
    )
    .unwrap();
    let addr = spawn_registry(state, "127.0.0.1:0").await.unwrap();
    let url = format!("http://{addr}");
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut publications = vec![("self-venue-reader", "org-b", "b-registry")];
    if foreign {
        publications.push((FOREIGN, "org-a", "a-registry"));
    }
    for (capability, publisher, token) in publications {
        let response = http
            .post(format!("{url}/registry/v1/manifests"))
            .bearer_auth(token)
            .json(&manifest(capability, publisher))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    // Count B's searches through a thin forwarding proxy.
    let target = url.clone();
    let router = Router::new().fallback(move |request: Request| {
        let target = target.clone();
        let searches = searches.clone();
        async move {
            let method = request.method().clone();
            let path = request.uri().path().to_owned();
            if path.ends_with("/search") {
                let mut n = searches.lock().unwrap();
                *n += 1;
                // The first `fail_first` searches hit an outage.
                if *n <= fail_first {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        [("content-type", "application/json")],
                        b"{\"error\":\"outage\"}".to_vec(),
                    );
                }
            }
            let auth = request.headers().get("authorization").cloned();
            let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
            let http = reqwest::Client::builder().no_proxy().build().unwrap();
            let mut forward = http
                .request(method, format!("{target}{path}"))
                .header("content-type", "application/json");
            if let Some(auth) = auth {
                forward = forward.header("authorization", auth);
            }
            if !bytes.is_empty() {
                forward = forward.body(bytes.to_vec());
            }
            let response = forward.send().await.unwrap();
            let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
            let body = response.bytes().await.unwrap();
            (
                status,
                [("content-type", "application/json")],
                body.to_vec(),
            )
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    proxy
}

impl Fixture {
    async fn new(o: Options) -> Self {
        let providers = Arc::new(Providers::start(o.ambiguous_runs, o.running_runs).await);
        let registry_searches = Arc::new(Mutex::new(0));
        let registry_url = if o.registry_up {
            registry(o.foreign, registry_searches.clone(), o.failed_searches).await
        } else {
            // Nothing listens here: every registry call fails.
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            drop(listener);
            url
        };
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev(ORG, "Adopt-first fixture");
        config.db = Some(dir.path().join("org.sqlite"));
        config.dev_stub = false;
        config.registry_url = Some(registry_url);
        config.registry_token = Some("b-registry".into());
        config.oracle_suites = vec![b_suite()];
        let mut acquisition = AcquisitionConfig::offline(AcquisitionPolicy {
            model: ModelConfig {
                model: MODEL.into(),
                max_tokens: 1024,
            },
            quotas: AcquisitionQuotas::default(),
            max_duration_seconds: 3600,
        });
        acquisition.model = Some(providers.model.clone());
        acquisition.apify = Some(providers.apify.clone());
        acquisition.automatic = true;
        config.acquisition = Some(acquisition);
        config.gap_acquisition_policy = Some(gap_policy());
        config.gap_adoption_policy = o.adopt_first.then(|| GapAdoptionPolicy {
            rules: vec![
                GapAdoptionRule {
                    capability_id: REQUESTED.into(),
                    suite_id: b_suite().suite_id,
                    synthesis_fallback: o.fallback,
                },
                GapAdoptionRule {
                    capability_id: SIBLING.into(),
                    suite_id: b_suite().suite_id,
                    synthesis_fallback: o.fallback,
                },
            ],
        });
        let app = build_org(&config, Some(provider_set(&providers))).unwrap();
        let (gap_id, requirement_id) = seed(&app, o.sibling).await;
        Self {
            _dir: dir,
            config,
            app,
            gap_id,
            requirement_id,
            providers,
            registry_searches,
            server: None,
        }
    }
    fn state(&self) -> OrgState {
        self.app.current_state().unwrap()
    }
    fn gap_status(&self) -> CapabilityGapStatus {
        self.state().capability_gaps[&self.gap_id].status.clone()
    }
    fn record(&self) -> Option<genesis_server::capability_gap_adoption::GapAdoptionRecord> {
        self.app.store.gap_adoption(&self.gap_id).unwrap()
    }
    fn adoptions(&self) -> Vec<genesis_server::capability_adoption_store::AdoptionRecord> {
        self.app.store.list_adoptions(0, 32).unwrap()
    }
    fn searches(&self) -> usize {
        *self.registry_searches.lock().unwrap()
    }
    fn driver_bound(&self) -> bool {
        self.app
            .store
            .gap_acquisition_binding(self.gap_id.as_str())
            .unwrap()
            .is_some()
    }
    fn reopen(&mut self) {
        self.stop();
        self.app = build_org(&self.config, Some(provider_set(&self.providers))).unwrap();
    }
    async fn serve(&mut self) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app = self.app.clone();
        self.server = Some(tokio::spawn(async move {
            serve_org(app, listener).await.unwrap()
        }));
    }
    fn stop(&mut self) {
        if let Some(task) = self.server.take() {
            task.abort();
        }
    }
    async fn wait_for(&self, what: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..400 {
            if done(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "timed out waiting for {what}: gap {:?}, record {:?}, adoptions {:?}",
            self.gap_status(),
            self.record(),
            self.adoptions()
                .iter()
                .map(|a| a.status())
                .collect::<Vec<_>>()
        );
    }
    async fn drain(&self) -> Option<Step> {
        capability_gap_adoption::drain_once(&self.app)
            .await
            .unwrap()
            .remove(&self.gap_id)
    }
    async fn adoption_step(&self) {
        let id = self.record().unwrap().adoption_id.unwrap();
        capability_adoption::step(&self.app, &id).await.unwrap();
    }
    /// Store commit seam: rewrite the live requirement's summary.
    fn change_requirement(&self) {
        let state = self.state();
        let mut next = state.clone();
        next.requirements
            .get_mut(&self.requirement_id)
            .unwrap()
            .summary = "Read something else entirely".into();
        next.revision += 1;
        self.app
            .store
            .commit_state(
                state.revision,
                &next,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
    fn gap_notes(&self) -> usize {
        self.state()
            .observations
            .values()
            .filter(|o| o.source == CAPABILITY_GAP_SOURCE)
            .count()
    }
    /// Store commit seam: a delegation rotated the grant (same assignment,
    /// next epoch), as a transfer would.
    fn rotate_epoch(&self) {
        let state = self.state();
        let gap = state.capability_gaps[&self.gap_id].clone();
        let mut next = state.clone();
        let assignment = state
            .active_assignment(&gap.responsibility_id)
            .unwrap()
            .assignment_id
            .clone();
        next.responsibilities
            .get_mut(&gap.responsibility_id)
            .unwrap()
            .epoch += 1;
        next.assignments.get_mut(&assignment).unwrap().epoch += 1;
        next.revision += 1;
        self.app
            .store
            .commit_state(
                state.revision,
                &next,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
    /// Another writer closes the gap through the ordinary kernel transition
    /// (e.g. a second process whose gap driver runs without the adoption
    /// policy and bound or refused it in the meantime).
    async fn close_gap(&self, to: CapabilityGapStatus) {
        let gap = self.gap_id.clone();
        self.app
            .mutate(EventMeta::default(), move |state| {
                engine::update_capability_gap_at(state, &gap, to, Utc::now())
                    .map(|outcome| (outcome, ()))
                    .map_err(|e| validation_error(vec![e]))
            })
            .await
            .unwrap();
    }
    /// A second SQLite connection to the organization's database, for
    /// storage-level injections the public API cannot express.
    fn db(&self) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(self.config.db.as_ref().unwrap()).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        conn
    }
    /// Rewrite this gap's stored record text (keeping it valid JSON).
    fn tamper_record(&self, from: &str, to: &str, adoption_id: Option<&str>) {
        let changed = self
            .db()
            .execute(
                "UPDATE capability_gap_adoptions SET record_json=replace(record_json, ?2, ?3), adoption_id=?4 WHERE gap_id=?1",
                rusqlite::params![self.gap_id.as_str(), from, to, adoption_id],
            )
            .unwrap();
        assert_eq!(changed, 1);
    }
    /// Run one adoption through discovery, its checks and finalization.
    async fn run_adoption(&self, id: &str) -> Result<(), genesis_server::error::ApiError> {
        capability_adoption::step(&self.app, id).await?; // select + install
        capability_adoption::step(&self.app, id).await?; // queue checks
        self.collect_checks(id).await;
        capability_adoption::step(&self.app, id).await.map(|_| ()) // finalize
    }
    async fn collect_checks(&self, id: &str) {
        for job in self.app.store.get_adoption(id).unwrap().job_ids() {
            for _ in 0..10 {
                let run = self.app.store.get_capability_run(job).unwrap();
                if run.phase == genesis_server::capability_runtime::CapabilityRunPhase::Collected {
                    break;
                }
                self.app.capability_runtime.step(job, false).await.unwrap();
            }
        }
    }
    fn operator_intent(
        &self,
        key: &str,
    ) -> genesis_server::capability_adoption_store::CreateAdoption {
        let state = self.state();
        let gap = &state.capability_gaps[&self.gap_id];
        genesis_server::capability_adoption_store::CreateAdoption {
            client_request_id: key.into(),
            mission_id: gap.mission_id.clone(),
            requirement_id: gap.requirement_id.clone(),
            role_id: gap.role_id.clone(),
            responsibility_id: gap.responsibility_id.clone(),
            suite_id: b_suite().suite_id,
            provider_access_acknowledged: true,
            deadline: None,
        }
    }
}

async fn seed(app: &AppState, sibling: bool) -> (CapabilityGapId, RequirementId) {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Find a venue", "roles":[{"key":"lead","name":"Lead","charter":"Own the missing parser",
            "authority":{"scopes":["capability.acquire",PERMISSION],"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Coordinate","owner":"lead"}]
    }))
    .unwrap();
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
                "public portal",
                "New venue layout",
                Some(SOURCE.into()),
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
                "Pass B's own extraction checks",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let role = state.roles.keys().next().unwrap().clone();
    let mut proposal = json!({
        "expected_org_revision":state.revision, "trigger_evidence_ids":[observation],
        "rationale":"The requirement needs a parser", "budget_limit_cents":0,
        "operations":[
            {"op":"AssignResponsibility", "target":{"kind":"from_requirement","key":"reader","requirement_id":requirement,
                "purpose":"Read venue listings","trigger":"Layout changes","termination":"Verified parser available"},
                "owner":{"kind":"existing","role_id":role}},
            {"op":"RequestCapability", "key":"parser", "requirement_id":requirement,
                "role":{"kind":"existing","role_id":role}, "responsibility":{"kind":"new","key":"reader"},
                "capability_id":REQUESTED, "purpose":"Read venue listings",
                "observed_sources":gap_policy().rules[0].allowed_source_urls}
        ]
    });
    if sibling {
        proposal["operations"].as_array_mut().unwrap().push(json!(
            {"op":"RequestCapability", "key":"sibling", "requirement_id":requirement,
                "role":{"kind":"existing","role_id":role}, "responsibility":{"kind":"new","key":"reader"},
                "capability_id":SIBLING, "purpose":"Read venue listings (second view)",
                "observed_sources":gap_policy().rules[0].allowed_source_urls}));
    }
    let proposal: OrgChangeProposal = serde_json::from_value(proposal).unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    let gap_id = allocation.gaps["parser"].clone();
    app.mutate(EventMeta::default(), |state| {
        engine::commit(
            state,
            &proposal,
            &allocation,
            &Default::default(),
            &engine::local_catalog(state),
            Utc::now(),
        )
        .map(|outcome| (outcome, ()))
        .map_err(validation_error)
    })
    .await
    .unwrap();
    (gap_id, requirement)
}

fn installed(status: &CapabilityGapStatus) -> Option<(&str, &str, &InstallationId)> {
    match status {
        CapabilityGapStatus::Installed {
            acquisition_id,
            capability,
            installation_id,
        } => Some((
            acquisition_id,
            capability.capability_id.as_str(),
            installation_id,
        )),
        _ => None,
    }
}

#[tokio::test]
async fn serve_org_adopts_a_foreign_capability_for_a_committed_gap_without_synthesis() {
    let mut f = Fixture::new(Options::default()).await;
    f.serve().await;
    f.wait_for("installed gap", |f| installed(&f.gap_status()).is_some())
        .await;

    let status = f.gap_status();
    let (link, capability, installation_id) = installed(&status).unwrap();
    let adoptions = f.adoptions();
    assert_eq!(adoptions.len(), 1, "exactly one adoption intent");
    let adoption = &adoptions[0];
    assert_eq!(adoption.phase(), AdoptionPhase::Accepted);
    assert_eq!(link, adoption.id());
    // A different, independently discovered capability ID with an exact pin.
    assert_eq!(capability, FOREIGN);
    assert_eq!(adoption.installation_id(), Some(installation_id));
    let state = f.state();
    let installation = &state.installations[installation_id];
    assert_eq!(installation.status, InstallationStatus::Accepted);
    assert_eq!(installation.capability.capability_id.as_str(), FOREIGN);
    assert_eq!(installation.capability.version, "1.0.0");
    assert_eq!(installation.source_digest, pin().source_sha256);
    let record = f.record().unwrap();
    assert_eq!(record.state, GapAdoptionState::Installed);
    assert_eq!(record.adoption_id.as_deref(), Some(adoption.id()));
    assert_eq!(record.installation_id.as_ref(), Some(installation_id));

    // Zero synthesis: no model call, no Actor creation, no gap acquisition.
    assert_eq!(f.providers.model_calls(), 0);
    assert_eq!(f.providers.actor_creations(), 0);
    assert!(!f.driver_bound());
    // Only B's own suite ran, once per case; one search; one planner note.
    assert_eq!(f.providers.runs(), b_suite().cases.len());
    assert_eq!(f.searches(), 1);
    assert_eq!(f.gap_notes(), 1);

    // Nothing more happens across passes or a restart.
    let revision = f.state().revision;
    tokio::time::sleep(Duration::from_millis(600)).await;
    f.reopen();
    f.serve().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(f.state().revision, revision);
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.providers.runs(), b_suite().cases.len());
    assert_eq!(f.providers.model_calls(), 0);
    assert_eq!(f.searches(), 1);
    let report = genesis_core::audit::audit(&f.state());
    assert!(report.is_empty(), "{report:?}");
}

#[tokio::test]
async fn restarts_and_concurrent_passes_keep_one_intent_and_one_adoption() {
    let mut f = Fixture::new(Options::default()).await;
    // Concurrent passes on two independently opened organizations (two
    // processes on one database) freeze one record, then dispatch one adoption.
    let other = build_org(&f.config, Some(provider_set(&f.providers))).unwrap();
    let gap_id = f.gap_id.clone();
    let pass = |app: &AppState| {
        let app = app.clone();
        let gap = gap_id.clone();
        async move {
            capability_gap_adoption::drain_once(&app)
                .await
                .unwrap()
                .remove(&gap)
        }
    };
    let (a, b, c) = tokio::join!(pass(&f.app), pass(&other), pass(&f.app));
    let steps = [a, b, c];
    assert!(steps.contains(&Some(Step::Frozen)), "{steps:?}");
    assert!(
        steps
            .iter()
            .filter(|s| **s == Some(Step::Dispatched))
            .count()
            <= 1,
        "{steps:?}"
    );
    // The driver leaves the deferred gap alone meanwhile.
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    capability_gap_driver::drain_once(&other).await.unwrap();
    assert!(!f.driver_bound());

    f.reopen();
    let (a, b, c) = tokio::join!(pass(&f.app), pass(&other), pass(&f.app));
    let steps = [a, b, c];
    assert!(
        steps
            .iter()
            .filter(|s| **s == Some(Step::Dispatched))
            .count()
            <= 1,
        "{steps:?}"
    );
    assert_eq!(f.adoptions().len(), 1);
    let adoption_id = f.record().unwrap().adoption_id.unwrap();
    drop(other);

    f.reopen();
    assert_eq!(f.drain().await, Some(Step::Waiting));
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());
    assert_eq!(f.record().unwrap().adoption_id.unwrap(), adoption_id);

    f.serve().await;
    f.wait_for("installed gap", |f| installed(&f.gap_status()).is_some())
        .await;
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.adoptions()[0].id(), adoption_id);
    assert_eq!(f.state().installations.len(), 1);
    assert_eq!(f.providers.runs(), b_suite().cases.len());
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn a_definitive_empty_search_releases_the_gap_to_synthesis_exactly_once() {
    let f = Fixture::new(Options {
        foreign: false,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    let adoption = &f.adoptions()[0];
    assert_eq!(adoption.phase(), AdoptionPhase::Attention);
    // Only B's own publication exists, and B excludes itself.
    assert_eq!(
        adoption.attention_code(),
        Some(AdoptionAttention::NoCandidate)
    );
    assert_eq!(f.searches(), 1);
    // Not yet released: the driver still defers.
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());

    assert_eq!(f.drain().await, Some(Step::Released));
    let record = f.record().unwrap();
    assert_eq!(record.state, GapAdoptionState::Released);
    assert_eq!(record.code, Some(GapAdoptionCode::NoCandidate));
    assert_eq!(f.providers.model_calls(), 0);

    let summary = capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(
        f.driver_bound(),
        "{summary:?} {:?}",
        capability_gap_driver::status(&f.app, &f.gap_id)
    );
    let acquiring = |s: &CapabilityGapStatus| match s {
        CapabilityGapStatus::Acquiring { acquisition_id } => Some(acquisition_id.clone()),
        _ => None,
    };
    let first = acquiring(&f.gap_status()).expect("released gap is acquiring");
    // Further passes of both workers: one release, one acquisition, one search.
    for _ in 0..3 {
        assert_eq!(f.drain().await, None);
        capability_gap_driver::drain_once(&f.app).await.unwrap();
    }
    assert_eq!(acquiring(&f.gap_status()), Some(first));
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.searches(), 1);
    assert_eq!(f.record().unwrap().state, GapAdoptionState::Released);
}

#[tokio::test]
async fn an_empty_search_without_fallback_stays_at_attention() {
    let f = Fixture::new(Options {
        foreign: false,
        fallback: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    // Definite: the gap is refused (waking the planner), never synthesized.
    assert_eq!(
        f.drain().await,
        Some(Step::Refused(GapAdoptionCode::NoCandidate))
    );
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());
    assert!(matches!(
        f.gap_status(),
        CapabilityGapStatus::Refused { acquisition_id: Some(ref id), .. }
            if *id == f.adoptions()[0].id()
    ));
    assert_eq!(f.record().unwrap().state, GapAdoptionState::Refused);
    assert_eq!(f.gap_notes(), 1);
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn an_unreachable_registry_keeps_the_adoption_and_never_falls_back() {
    let f = Fixture::new(Options {
        registry_up: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    let adoption_id = f.record().unwrap().adoption_id.unwrap();
    f.adoption_step().await;
    assert_eq!(
        f.adoptions()[0].attention_code(),
        Some(AdoptionAttention::RegistryUnavailable)
    );
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Registry))
    );
    for _ in 0..2 {
        f.drain().await;
        capability_gap_driver::drain_once(&f.app).await.unwrap();
    }
    let record = f.record().unwrap();
    assert_eq!(record.state, GapAdoptionState::Attention);
    assert_eq!(record.adoption_id.as_deref(), Some(adoption_id.as_str()));
    assert!(!f.driver_bound());
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn an_unknown_check_outcome_keeps_the_adoption_and_never_falls_back() {
    let mut f = Fixture::new(Options {
        ambiguous_runs: true,
        ..Options::default()
    })
    .await;
    f.serve().await;
    f.wait_for("checks attention", |f| {
        f.record()
            .is_some_and(|r| r.state == GapAdoptionState::Attention)
    })
    .await;
    let record = f.record().unwrap();
    assert_eq!(record.code, Some(GapAdoptionCode::Checks));
    assert!(record.adoption_id.is_some());
    let posts = f.providers.runs();
    assert!(posts >= 1);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    // Not resubmitted, not refused, not synthesized.
    assert_eq!(f.providers.runs(), posts);
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert!(!f.driver_bound());
    assert_eq!(f.providers.model_calls(), 0);
    assert_eq!(f.adoptions().len(), 1);
}

#[tokio::test]
async fn a_changed_requirement_before_dispatch_creates_no_adoption() {
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    f.change_requirement();
    assert_eq!(
        f.drain().await,
        Some(Step::Refused(GapAdoptionCode::Authority))
    );
    assert!(matches!(
        f.gap_status(),
        CapabilityGapStatus::Refused {
            acquisition_id: None,
            ..
        }
    ));
    assert!(f.adoptions().is_empty());
    assert_eq!(f.searches(), 0);
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn a_changed_requirement_during_checks_cannot_install_the_gap() {
    let mut f = Fixture::new(Options::default()).await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await; // discovery: selected and installed locally
    assert_eq!(f.adoptions()[0].phase(), AdoptionPhase::Selected);
    f.adoption_step().await; // checks queued
    assert_eq!(f.adoptions()[0].phase(), AdoptionPhase::Checking);
    f.change_requirement();
    f.serve().await;
    f.wait_for("authority refusal", |f| {
        f.record()
            .is_some_and(|r| r.state == GapAdoptionState::Refused)
    })
    .await;
    assert_eq!(f.record().unwrap().code, Some(GapAdoptionCode::Authority));
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_ne!(f.adoptions()[0].phase(), AdoptionPhase::Accepted);
    assert!(matches!(
        f.gap_status(),
        CapabilityGapStatus::Refused { .. }
    ));
    assert!(!f.driver_bound());
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn an_ordinary_accepted_adoption_never_installs_an_unowned_gap() {
    // Adopt-first off: an operator's adoption for the same requirement, even
    // when accepted, does not touch the gap.
    let mut f = Fixture::new(Options {
        adopt_first: false,
        ..Options::default()
    })
    .await;
    let state = f.state();
    let gap = state.capability_gaps[&f.gap_id].clone();
    let intent = genesis_server::capability_adoption_store::CreateAdoption {
        client_request_id: "operator-adoption".into(),
        mission_id: gap.mission_id.clone(),
        requirement_id: gap.requirement_id.clone(),
        role_id: gap.role_id.clone(),
        responsibility_id: gap.responsibility_id.clone(),
        suite_id: b_suite().suite_id,
        provider_access_acknowledged: true,
        deadline: None,
    };
    let (adoption, _) = f
        .app
        .store
        .create_adoption(&intent, &b_suite(), Utc::now())
        .unwrap();
    // Keep the driver out of it: this test is only about the link.
    f.config.gap_acquisition_policy = None;
    f.config.acquisition = None;
    f.reopen();
    f.serve().await;
    f.wait_for("accepted ordinary adoption", |f| {
        f.app.store.get_adoption(adoption.id()).unwrap().phase() == AdoptionPhase::Accepted
    })
    .await;
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert!(f.record().is_none());
    assert_eq!(f.gap_notes(), 0);
}

#[tokio::test]
async fn a_record_pointing_at_a_foreign_adoption_is_not_linked() {
    // A stored intent whose adoption ID names an adoption for another key and
    // requirement grant is rejected as invalid, not trusted by its prefix.
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    let state = f.state();
    let gap = state.capability_gaps[&f.gap_id].clone();
    let other = genesis_server::capability_adoption_store::CreateAdoption {
        client_request_id: format!("gap-adopt:{}", f.gap_id),
        mission_id: gap.mission_id.clone(),
        requirement_id: gap.requirement_id.clone(),
        role_id: gap.role_id.clone(),
        responsibility_id: gap.responsibility_id.clone(),
        suite_id: b_suite().suite_id,
        provider_access_acknowledged: true,
        // A different intent under the coordinator's key.
        deadline: Some(Utc::now() + chrono::Duration::minutes(5)),
    };
    f.app
        .store
        .create_adoption(&other, &b_suite(), Utc::now())
        .unwrap();
    // The coordinator's own intent now conflicts with the squatted key.
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::InvalidRecord))
    );
    let record = f.record().unwrap();
    assert!(record.adoption_id.is_none());
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn a_requirement_changed_before_freeze_refuses_the_gap_like_the_driver() {
    // Review finding: without a record the gap used to stay open forever.
    for adopt_first in [false, true] {
        let f = Fixture::new(Options {
            adopt_first,
            ..Options::default()
        })
        .await;
        f.change_requirement();
        for _ in 0..3 {
            capability_gap_adoption::drain_once(&f.app).await.unwrap();
            capability_gap_driver::drain_once(&f.app).await.unwrap();
        }
        assert!(
            matches!(f.gap_status(), CapabilityGapStatus::Refused { .. }),
            "adopt_first={adopt_first}: {:?}",
            f.gap_status()
        );
        assert_eq!(f.gap_notes(), 1, "adopt_first={adopt_first}");
        assert!(f.adoptions().is_empty());
        assert!(f.record().is_none());
    }
}

#[tokio::test]
async fn removing_the_policy_after_freeze_still_finishes_the_frozen_intent() {
    // Review finding: a frozen record used to sit active forever once the
    // coordinator stopped running with the policy.
    let mut f = Fixture::new(Options {
        foreign: false,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.config.gap_adoption_policy = None;
    f.reopen();
    f.serve().await;
    // The frozen fallback choice wins: released once, then the driver binds.
    f.wait_for("release and driver binding", |f| {
        f.record()
            .is_some_and(|r| r.state == GapAdoptionState::Released)
            && f.driver_bound()
    })
    .await;
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.searches(), 1);
}

#[tokio::test]
async fn a_sibling_gap_waits_for_the_requirement_slot_instead_of_stranding() {
    // Review finding: the adoption store allows one active adoption per
    // requirement grant, and the second gap used to stop at terminal attention
    // with no adoption. It now waits and dispatches when the slot frees.
    let mut f = Fixture::new(Options {
        sibling: true,
        ..Options::default()
    })
    .await;
    let gaps: Vec<_> = f.state().capability_gaps.keys().cloned().collect();
    assert_eq!(gaps.len(), 2);
    let first = capability_gap_adoption::drain_once(&f.app).await.unwrap();
    assert!(first.values().all(|s| *s == Step::Frozen), "{first:?}");
    let second = capability_gap_adoption::drain_once(&f.app).await.unwrap();
    assert_eq!(
        second.values().filter(|s| **s == Step::Dispatched).count(),
        1,
        "{second:?}"
    );
    assert_eq!(
        second.values().filter(|s| **s == Step::Waiting).count(),
        1,
        "{second:?}"
    );
    let waiting = gaps
        .iter()
        .find(|g| {
            f.app
                .store
                .gap_adoption(g)
                .unwrap()
                .unwrap()
                .adoption_id
                .is_none()
        })
        .unwrap()
        .clone();
    let record = f.app.store.gap_adoption(&waiting).unwrap().unwrap();
    assert_eq!(record.state, GapAdoptionState::Active);
    assert_eq!(record.code, Some(GapAdoptionCode::Conflict));
    f.serve().await;
    // Both gaps reach a decision with their own adoption; neither is
    // synthesized or left without an adoption.
    f.wait_for("both gaps decided", |f| {
        gaps.iter().all(|g| {
            f.app
                .store
                .gap_adoption(g)
                .unwrap()
                .is_some_and(|r| r.adoption_id.is_some() && r.state != GapAdoptionState::Active)
        })
    })
    .await;
    assert_eq!(f.adoptions().len(), 2);
    let state = f.state();
    let (done, other): (Vec<_>, Vec<_>) = gaps
        .iter()
        .partition(|g| installed(&state.capability_gaps[*g].status).is_some());
    assert_eq!(done.len(), 1);
    // The sibling's own adoption selects the capability the first gap already
    // installed; that installation conflict is reported, with its adoption
    // kept, rather than synthesized or silently dropped.
    let sibling = f.app.store.gap_adoption(other[0]).unwrap().unwrap();
    assert_eq!(sibling.state, GapAdoptionState::Attention);
    assert_eq!(sibling.code, Some(GapAdoptionCode::Conflict));
    assert_eq!(
        state.capability_gaps[other[0]].status,
        CapabilityGapStatus::Open
    );
    for g in &gaps {
        assert!(f
            .app
            .store
            .gap_acquisition_binding(g.as_str())
            .unwrap()
            .is_none());
    }
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn an_acceptance_before_the_adoption_id_is_saved_still_links_the_gap() {
    // Review finding: create_adoption and saving its ID are two writes. Model
    // a crash between them: the adoption exists under the coordinator's exact
    // key and intent, the record has no ID, and the adoption runs to
    // acceptance before any coordinator pass. The acceptance must still link.
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    let record = f.record().unwrap();
    let intent = genesis_server::capability_adoption_store::CreateAdoption {
        client_request_id: record.client_request_id(),
        mission_id: record.mission_id.clone(),
        requirement_id: record.requirement_id.clone(),
        role_id: record.role_id.clone(),
        responsibility_id: record.responsibility_id.clone(),
        suite_id: record.suite_id.clone(),
        provider_access_acknowledged: true,
        deadline: None,
    };
    let (adoption, _) = f
        .app
        .store
        .create_adoption(&intent, &b_suite(), Utc::now())
        .unwrap();
    let id = adoption.id().to_owned();
    capability_adoption::step(&f.app, &id).await.unwrap(); // select + install
    capability_adoption::step(&f.app, &id).await.unwrap(); // queue checks
    for job in f.app.store.get_adoption(&id).unwrap().job_ids() {
        for _ in 0..10 {
            let run = f.app.store.get_capability_run(job).unwrap();
            if run.phase == genesis_server::capability_runtime::CapabilityRunPhase::Collected {
                break;
            }
            f.app.capability_runtime.step(job, false).await.unwrap();
        }
    }
    capability_adoption::step(&f.app, &id).await.unwrap(); // accept
    assert_eq!(
        f.app.store.get_adoption(&id).unwrap().phase(),
        AdoptionPhase::Accepted
    );
    let (link, _, _) = installed(&f.gap_status())
        .map(|(a, b, c)| (a.to_owned(), b, c.clone()))
        .expect("linked in the acceptance transaction");
    assert_eq!(link, id);
    let record = f.record().unwrap();
    assert_eq!(record.state, GapAdoptionState::Installed);
    assert_eq!(record.adoption_id.as_deref(), Some(id.as_str()));
    assert_eq!(f.drain().await, None);
}

async fn retry(f: &Fixture, principal: &str) -> (reqwest::StatusCode, Value) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = f.app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!(
            "{url}/api/v1/capability-gaps/{}/adoption/retry",
            f.gap_id
        ))
        .bearer_auth(format!("{ORG}-{principal}"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.json().await.unwrap_or(Value::Null);
    server.abort();
    (status, body)
}

#[tokio::test]
async fn a_registry_outage_can_be_retried_once_by_an_operator_and_then_adopts() {
    let mut f = Fixture::new(Options {
        failed_searches: 1,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    let first = f.record().unwrap().adoption_id.unwrap();
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Registry))
    );
    // Only operators may retry.
    let (status, _) = retry(&f, "agent").await;
    assert!(status.is_client_error(), "{status}");
    let (status, body) = retry(&f, "operator").await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(body["attempt"], 1);
    assert_eq!(body["previous_adoption_ids"], json!([first]));
    assert_eq!(body["state"], "active");
    // A second retry while active is refused; nothing was synthesized.
    let (status, _) = retry(&f, "operator").await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    assert!(!f.driver_bound());
    f.serve().await;
    f.wait_for("installed after retry", |f| {
        installed(&f.gap_status()).is_some()
    })
    .await;
    let record = f.record().unwrap();
    assert_ne!(record.adoption_id.as_deref(), Some(first.as_str()));
    assert_eq!(f.adoptions().len(), 2);
    assert_eq!(f.searches(), 2);
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn unknown_check_outcomes_block_retry_definite_ones_allow_it_and_retries_are_bounded() {
    let mut f = Fixture::new(Options {
        ambiguous_runs: true,
        ..Options::default()
    })
    .await;
    f.serve().await;
    f.wait_for("checks attention", |f| {
        f.record()
            .is_some_and(|r| r.state == GapAdoptionState::Attention)
    })
    .await;
    f.stop();
    let first = f.record().unwrap().adoption_id.unwrap();
    let posts = f.providers.runs();
    // Nothing retries on its own.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(f.providers.runs(), posts);
    assert_eq!(f.record().unwrap().attempt, 0);
    // Agents can't retry. Nor can an operator while a native check has an
    // unknown outcome: a fresh adoption would submit it again (Codex, #86).
    let (status, _) = retry(&f, "agent").await;
    assert!(status.is_client_error(), "{status}");
    let (status, body) = retry(&f, "operator").await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "adoption_checks_unresolved", "{body}");
    let (_, status_body) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(status_body["retryable"], false, "{status_body}");
    let record = f.record().unwrap();
    assert_eq!(record.attempt, 0);
    assert_eq!(record.adoption_id.as_deref(), Some(first.as_str()));
    assert_eq!(record.state, GapAdoptionState::Attention);
    // The stopped adoption is never resubmitted or revived.
    assert_eq!(f.providers.runs(), posts);
    assert!(!f.driver_bound());
    assert_eq!(f.providers.model_calls(), 0);

    // A definite failure recorded without a receipt (the submission response
    // itself reports the run FAILED) is settled: the operator may retry, and
    // the fresh adoption's checks run and install.
    let mut f = Fixture::new(Options::default()).await;
    f.providers.fail_submits(usize::MAX);
    f.serve().await;
    f.wait_for("checks attention", |f| {
        f.record()
            .is_some_and(|r| r.state == GapAdoptionState::Attention)
    })
    .await;
    f.stop();
    assert_eq!(f.record().unwrap().code, Some(GapAdoptionCode::Checks));
    let first = f.record().unwrap().adoption_id.unwrap();
    let terminal: Value = reqwest::get(format!("{}/v2/actor-runs/b-run-0", f.providers.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        terminal["data"]["status"], "FAILED",
        "the retry positive control must retain terminal provider evidence"
    );
    f.providers.fail_submits(0);
    let (status, body) = retry(&f, "operator").await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(body["attempt"], 1);
    assert_eq!(body["previous_adoption_ids"], json!([first]));
    f.serve().await;
    f.wait_for("installed after retry", |f| {
        installed(&f.gap_status()).is_some()
    })
    .await;
    assert_eq!(f.adoptions().len(), 2);
    assert_eq!(f.providers.model_calls(), 0);

    // Registry outages: two retries, then no more.
    let f = Fixture::new(Options {
        failed_searches: 3,
        ..Options::default()
    })
    .await;
    f.drain().await;
    for attempt in 0..=genesis_server::capability_gap_adoption::MAX_RETRIES {
        f.drain().await;
        f.adoption_step().await;
        assert_eq!(
            f.drain().await,
            Some(Step::Attention(GapAdoptionCode::Registry)),
            "attempt {attempt}"
        );
        let (status, _) = retry(&f, "operator").await;
        let expected = if attempt < genesis_server::capability_gap_adoption::MAX_RETRIES {
            reqwest::StatusCode::OK
        } else {
            reqwest::StatusCode::CONFLICT
        };
        assert_eq!(status, expected, "attempt {attempt}");
    }
    assert_eq!(f.adoptions().len(), 3);
    assert!(!f.driver_bound());
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
}

async fn adoption_status(f: &Fixture, gap: &str) -> (reqwest::StatusCode, Value) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = f.app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{url}/api/v1/capability-gaps/{gap}/adoption"))
        .bearer_auth(format!("{ORG}-operator"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.json().await.unwrap_or(Value::Null);
    server.abort();
    (status, body)
}

#[tokio::test]
async fn a_link_whose_record_changed_underneath_fails_the_whole_acceptance() {
    // Compare-and-swap: the acceptance transaction links from the record it
    // read. If another writer changes that record before the link is saved,
    // the stale link must conflict and fail the acceptance whole; it must not
    // commit the gap installed while the record write silently matched no row.
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    let id = f.record().unwrap().adoption_id.unwrap();
    capability_adoption::step(&f.app, &id).await.unwrap(); // select + install
    capability_adoption::step(&f.app, &id).await.unwrap(); // queue checks
    f.collect_checks(&id).await;
    let before = f.record().unwrap();
    // Inject a concurrent writer between the link's read and its save: the
    // adoption's own phase write in the acceptance transaction (which runs
    // just before the link is saved) also rewrites the gap record.
    f.db()
        .execute_batch(
            "CREATE TRIGGER concurrent_gap_writer AFTER UPDATE OF phase ON capability_adoptions
             WHEN NEW.phase <> OLD.phase
             BEGIN
               UPDATE capability_gap_adoptions
                  SET record_json = replace(record_json, '\"code\":null', '\"code\":\"conflict\"');
             END;",
        )
        .unwrap();
    assert!(capability_adoption::step(&f.app, &id).await.is_err());
    assert_eq!(
        f.app.store.get_adoption(&id).unwrap().phase(),
        AdoptionPhase::Checking
    );
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert_eq!(
        f.record().unwrap(),
        before,
        "the whole transaction rolled back"
    );
    assert_eq!(f.gap_notes(), 0);

    // Once the conflict clears, the retried acceptance links atomically.
    f.db()
        .execute_batch("DROP TRIGGER concurrent_gap_writer;")
        .unwrap();
    capability_adoption::step(&f.app, &id).await.unwrap();
    assert_eq!(
        f.app.store.get_adoption(&id).unwrap().phase(),
        AdoptionPhase::Accepted
    );
    assert_eq!(installed(&f.gap_status()).unwrap().0, id);
    assert_eq!(f.record().unwrap().state, GapAdoptionState::Installed);
}

#[tokio::test]
async fn lost_authority_is_refused_before_the_adoption_fences_itself() {
    // The coordinator mirrors the adoption's own fence early: the very next
    // pass refuses the gap, even before the adoption has stepped again.
    let f = Fixture::new(Options::default()).await;
    f.drain().await;
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    let id = f.record().unwrap().adoption_id.unwrap();
    f.change_requirement();
    assert_eq!(
        f.drain().await,
        Some(Step::Refused(GapAdoptionCode::Authority))
    );
    assert_eq!(
        f.app.store.get_adoption(&id).unwrap().phase(),
        AdoptionPhase::Discovering,
        "refused before the adoption fenced itself"
    );
    assert!(matches!(
        f.gap_status(),
        CapabilityGapStatus::Refused { acquisition_id: Some(ref link), .. } if *link == id
    ));
    assert_eq!(f.record().unwrap().state, GapAdoptionState::Refused);
    assert_eq!(f.searches(), 0);
    assert_eq!(f.gap_notes(), 1);
}

#[tokio::test]
async fn a_changed_suite_stops_dispatch_at_configuration() {
    // The suite frozen with the intent changed (same ID, different cases)
    // before dispatch: no adoption is created under the new suite.
    let mut f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    let mut suite = b_suite();
    suite.cases[0].input["attendees"] =
        json!(suite.cases[0].input["attendees"].as_u64().unwrap() + 1);
    f.config.oracle_suites = vec![suite];
    f.reopen();
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Configuration))
    );
    assert!(f.adoptions().is_empty());
    assert!(f.record().unwrap().adoption_id.is_none());
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert_eq!(f.searches(), 0);
}

#[tokio::test]
async fn an_adoption_under_the_key_from_an_earlier_grant_is_not_adopted() {
    // The adoption store replays an identical intent by key regardless of the
    // grant it was created under. An adoption squatting the coordinator's key
    // from before a delegation rotated the epoch replays to the coordinator,
    // which must not record it as its own.
    let f = Fixture::new(Options::default()).await;
    let squatter = f.operator_intent(&format!("gap-adopt:{}", f.gap_id));
    let (squatted, _) = f
        .app
        .store
        .create_adoption(&squatter, &b_suite(), Utc::now())
        .unwrap();
    let old_epoch = squatted.binding().grant.epoch;
    f.rotate_epoch();
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.record().unwrap().epoch, old_epoch + 1);
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::InvalidRecord))
    );
    let record = f.record().unwrap();
    assert!(record.adoption_id.is_none());
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn a_record_rewritten_to_name_another_adoption_is_invalid() {
    // Storage-level tamper: the stored ID names an operator's adoption for the
    // same requirement. Its progress must not drive the gap.
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    let (operator, _) = f
        .app
        .store
        .create_adoption(
            &f.operator_intent("operator-adoption"),
            &b_suite(),
            Utc::now(),
        )
        .unwrap();
    // The operator's adoption holds the requirement slot.
    assert_eq!(f.drain().await, Some(Step::Waiting));
    f.tamper_record(
        "\"adoption_id\":null",
        &format!("\"adoption_id\":\"{}\"", operator.id()),
        Some(operator.id()),
    );
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::InvalidRecord))
    );
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert_eq!(
        f.app.store.get_adoption(operator.id()).unwrap().phase(),
        AdoptionPhase::Discovering
    );
}

#[tokio::test]
async fn a_stopped_record_is_not_stepped_again() {
    let f = Fixture::new(Options {
        registry_up: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Registry))
    );
    let stopped = f.record().unwrap();
    for _ in 0..2 {
        assert_eq!(f.drain().await, Some(Step::Idle));
    }
    assert_eq!(f.record().unwrap(), stopped, "not rewritten");

    // A released record, too, is left alone while its gap is still open.
    let f = Fixture::new(Options {
        foreign: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    assert_eq!(f.drain().await, Some(Step::Released));
    let released = f.record().unwrap();
    assert_eq!(f.drain().await, Some(Step::Idle));
    assert_eq!(f.record().unwrap(), released);
}

#[tokio::test]
async fn a_gap_closed_by_another_writer_is_never_frozen_or_deferred() {
    let f = Fixture::new(Options::default()).await;
    f.close_gap(CapabilityGapStatus::Refused {
        reason: "closed elsewhere".into(),
        acquisition_id: None,
    })
    .await;
    let gap = f.state().capability_gaps[&f.gap_id].clone();
    assert!(!capability_gap_adoption::defers_driver(&f.app, &gap));
    assert_eq!(f.drain().await, None, "only open gaps are listed");
    assert_eq!(
        capability_gap_adoption::step(&f.app, &f.gap_id)
            .await
            .unwrap(),
        Step::Idle
    );
    assert!(f.record().is_none());
}

#[tokio::test]
async fn a_frozen_gap_bound_elsewhere_before_dispatch_creates_no_adoption() {
    // A driver in another process bound the gap between freeze and dispatch.
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    f.close_gap(CapabilityGapStatus::Acquiring {
        acquisition_id: "acquisition-elsewhere".into(),
    })
    .await;
    let gap = f.state().capability_gaps[&f.gap_id].clone();
    // The driver must keep progressing the gap it bound.
    assert!(!capability_gap_adoption::defers_driver(&f.app, &gap));
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::InvalidRecord))
    );
    assert!(f.adoptions().is_empty());
    assert_eq!(f.searches(), 0);
    assert!(matches!(
        f.gap_status(),
        CapabilityGapStatus::Acquiring { .. }
    ));
}

#[tokio::test]
async fn a_definite_outcome_for_a_gap_closed_elsewhere_stops_at_attention() {
    let f = Fixture::new(Options {
        foreign: false,
        fallback: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    let closed = CapabilityGapStatus::Refused {
        reason: "closed elsewhere".into(),
        acquisition_id: None,
    };
    f.close_gap(closed.clone()).await;
    let notes = f.gap_notes();
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::NoCandidate))
    );
    let record = f.record().unwrap();
    assert_eq!(record.state, GapAdoptionState::Attention);
    assert!(record.adoption_id.is_some());
    // The other writer's decision stands; no second refusal or note.
    assert_eq!(f.gap_status(), closed);
    assert_eq!(f.gap_notes(), notes);
}

#[tokio::test]
async fn a_retry_is_refused_once_the_gap_is_closed() {
    let f = Fixture::new(Options {
        failed_searches: 1,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Registry))
    );
    f.close_gap(CapabilityGapStatus::Refused {
        reason: "closed elsewhere".into(),
        acquisition_id: None,
    })
    .await;
    let stopped = f.record().unwrap();
    let (status, body) = retry(&f, "operator").await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{body}");
    assert_eq!(f.record().unwrap(), stopped);
    assert_eq!(f.drain().await, None);
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.searches(), 1);

    // Status of an unknown gap is not found, not an empty record.
    let (status, _) = adoption_status(&f, "no-such-gap").await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    let (status, body) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body["code"], "registry");
}

#[tokio::test]
async fn a_database_without_the_adoption_table_has_no_records() {
    // Adopt-first off. Without a policy the pass returns nothing (it does not
    // even take the kernel lock), and a database whose adoption table is
    // missing reads as having no records everywhere, including inside an
    // ordinary adoption's acceptance transaction.
    let f = Fixture::new(Options {
        adopt_first: false,
        ..Options::default()
    })
    .await;
    assert!(capability_gap_adoption::drain_once(&f.app)
        .await
        .unwrap()
        .is_empty());
    let (adoption, _) = f
        .app
        .store
        .create_adoption(
            &f.operator_intent("operator-adoption"),
            &b_suite(),
            Utc::now(),
        )
        .unwrap();
    f.db()
        .execute_batch("DROP TABLE capability_gap_adoptions;")
        .unwrap();
    assert!(f.record().is_none());
    assert!(capability_gap_adoption::drain_once(&f.app)
        .await
        .unwrap()
        .is_empty());
    let (status, body) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(body["state"], Value::Null);
    f.run_adoption(adoption.id()).await.unwrap();
    assert_eq!(
        f.app.store.get_adoption(adoption.id()).unwrap().phase(),
        AdoptionPhase::Accepted
    );
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
}

#[tokio::test]
async fn stored_records_that_disagree_with_their_row_or_gap_are_not_trusted() {
    // A record stored under this gap's row naming another gap is unreadable,
    // and the driver defers it rather than binding.
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    f.tamper_record(
        &format!("\"gap_id\":\"{}\"", f.gap_id),
        "\"gap_id\":\"some-other-gap\"",
        None,
    );
    assert!(f.app.store.gap_adoption(&f.gap_id).is_err());
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(!f.driver_bound());

    // A linked gap whose record was rewritten back to active stays linked:
    // the pass recognizes its own installation and does nothing.
    let f = Fixture::new(Options::default()).await;
    f.drain().await;
    f.drain().await;
    let id = f.record().unwrap().adoption_id.unwrap();
    f.run_adoption(&id).await.unwrap();
    assert_eq!(installed(&f.gap_status()).unwrap().0, id);
    f.tamper_record("\"state\":\"installed\"", "\"state\":\"active\"", Some(&id));
    let rewritten = f.record().unwrap();
    assert_eq!(f.drain().await, Some(Step::Idle));
    assert_eq!(f.record().unwrap(), rewritten);
    assert_eq!(installed(&f.gap_status()).unwrap().0, id);
}

#[tokio::test]
async fn review_empty_search_cannot_release_a_gap_after_public_same_owner_delegation() {
    let f = Fixture::new(Options {
        foreign: false,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    assert_eq!(
        f.adoptions()[0].attention_code(),
        Some(AdoptionAttention::NoCandidate)
    );
    let frozen = f.record().unwrap();
    let state = f.state();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = f.app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{url}/api/v1/org-changes"))
        .bearer_auth(format!("{ORG}-operator"))
        .json(&json!({
            "client_request_id":"review-adoption-fallback-public-delegation",
            "proposal":{
                "expected_org_revision":state.revision,
                "trigger_evidence_ids":state.capability_gaps[&f.gap_id].triggering_evidence,
                "rationale":"Make the accountable role the explicit executor under a fresh grant",
                "budget_limit_cents":0,
                "operations":[{"op":"DelegateTask",
                    "responsibility":{"kind":"existing","responsibility_id":frozen.responsibility_id},
                    "executor":{"kind":"role","role":{"kind":"existing","role_id":frozen.role_id}},
                    "task":"Continue work under the new explicit executor grant"}]
            }
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    server.abort();
    assert!(status.is_success(), "{status}: {body}");
    let state = f.state();
    let grant = state.active_assignment(&frozen.responsibility_id).unwrap();
    assert_ne!(grant.assignment_id, frozen.assignment_id);
    assert!(grant.epoch > frozen.epoch);
    // The search belonged to the revoked grant. A new owner's grant cannot
    // inherit its permission to synthesize, even when it is the same role.
    let outcome = f.drain().await;
    capability_gap_driver::drain_once(&f.app).await.unwrap();
    assert!(
        !f.driver_bound(),
        "stale empty-search result handed the gap to synthesis under a new grant: {outcome:?}, {:?}",
        f.record()
    );
    assert_eq!(outcome, Some(Step::Refused(GapAdoptionCode::Authority)));
    assert_eq!(f.providers.model_calls(), 0);
    assert_eq!(f.adoptions().len(), 1);
}

#[tokio::test]
async fn review_unknown_native_check_cannot_be_replayed_through_a_fresh_adoption() {
    use genesis_server::capability_runtime::CapabilityRunPhase;
    let f = Fixture::new(Options {
        ambiguous_runs: true,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    f.adoption_step().await;
    let first = f.adoptions()[0].clone();
    assert_eq!(first.phase(), AdoptionPhase::Checking);
    let old_job = first.job_ids()[0].clone();
    // The provider creates a real loopback run but responds 503; the native
    // POST therefore has an unknown outcome, retained for reconciliation.
    for _ in 0..10 {
        let _ = f.app.capability_runtime.step(&old_job, false).await;
        if f.app.store.get_capability_run(&old_job).unwrap().phase
            == CapabilityRunPhase::NeedsReconciliation
        {
            break;
        }
    }
    assert_eq!(
        f.app.store.get_capability_run(&old_job).unwrap().phase,
        CapabilityRunPhase::NeedsReconciliation
    );
    let posts = f.providers.runs();
    assert_eq!(posts, 1);
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Checks))
    );
    let (status, body) = retry(&f, "operator").await;
    if status == reqwest::StatusCode::CONFLICT {
        assert_eq!(f.record().unwrap().adoption_id.as_deref(), Some(first.id()));
        assert_eq!(f.adoptions().len(), 1);
        assert_eq!(f.providers.runs(), posts);
        return;
    }
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    let second_id = f.record().unwrap().adoption_id.unwrap();
    assert_ne!(second_id, first.id());
    f.adoption_step().await;
    f.adoption_step().await;
    let second = f.app.store.get_adoption(&second_id).unwrap();
    assert_eq!(second.phase(), AdoptionPhase::Checking);
    let new_job = second.job_ids()[0].clone();
    assert_ne!(new_job, old_job);
    for _ in 0..10 {
        let _ = f.app.capability_runtime.step(&new_job, false).await;
        if f.app.store.get_capability_run(&new_job).unwrap().phase
            == CapabilityRunPhase::NeedsReconciliation
        {
            break;
        }
    }
    assert_eq!(
        f.app.store.get_capability_run(&old_job).unwrap().phase,
        CapabilityRunPhase::NeedsReconciliation,
        "the original native effect is still unresolved"
    );
    assert_eq!(
        f.providers.runs(),
        posts,
        "new adoption {second_id} submitted the same native check again while {} still has an unknown outcome",
        first.id()
    );
}

#[tokio::test]
async fn review_configuration_retry_cannot_replay_an_unknown_native_check() {
    use genesis_server::capability_runtime::CapabilityRunPhase;
    let mut f = Fixture::new(Options {
        ambiguous_runs: true,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    f.adoption_step().await;
    let first = f.adoptions()[0].clone();
    assert_eq!(first.phase(), AdoptionPhase::Checking);
    let old_job = first.job_ids()[0].clone();
    // The provider creates a real loopback run but responds 503; the native
    // POST therefore has an unknown outcome, retained for reconciliation.
    for _ in 0..10 {
        let _ = f.app.capability_runtime.step(&old_job, false).await;
        if f.app.store.get_capability_run(&old_job).unwrap().phase
            == CapabilityRunPhase::NeedsReconciliation
        {
            break;
        }
    }
    assert_eq!(
        f.app.store.get_capability_run(&old_job).unwrap().phase,
        CapabilityRunPhase::NeedsReconciliation
    );
    let posts = f.providers.runs();
    assert_eq!(posts, 1);
    // Ordinary restart configuration drift is observed before the adoption
    // inspects its child run. Restoring configuration cannot erase that run.
    let mut suite = b_suite();
    suite.cases[0].input["attendees"] =
        json!(suite.cases[0].input["attendees"].as_u64().unwrap() + 1);
    f.config.oracle_suites = vec![suite];
    f.reopen();
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Configuration))
    );
    f.config.oracle_suites = vec![b_suite()];
    f.reopen();
    let (status, body) = retry(&f, "operator").await;
    if status == reqwest::StatusCode::CONFLICT {
        assert_eq!(f.record().unwrap().adoption_id.as_deref(), Some(first.id()));
        assert_eq!(f.adoptions().len(), 1);
        assert_eq!(f.providers.runs(), posts);
        return;
    }
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    let second_id = f.record().unwrap().adoption_id.unwrap();
    assert_ne!(second_id, first.id());
    f.adoption_step().await;
    f.adoption_step().await;
    let second = f.app.store.get_adoption(&second_id).unwrap();
    assert_eq!(second.phase(), AdoptionPhase::Checking);
    let new_job = second.job_ids()[0].clone();
    assert_ne!(new_job, old_job);
    for _ in 0..10 {
        let _ = f.app.capability_runtime.step(&new_job, false).await;
        if f.app.store.get_capability_run(&new_job).unwrap().phase
            == CapabilityRunPhase::NeedsReconciliation
        {
            break;
        }
    }
    assert_eq!(
        f.app.store.get_capability_run(&old_job).unwrap().phase,
        CapabilityRunPhase::NeedsReconciliation,
        "the original native effect is still unresolved"
    );
    assert_eq!(
        f.providers.runs(),
        posts,
        "new adoption {second_id} submitted the same native check again while {} still has an unknown outcome",
        first.id()
    );
}

#[tokio::test]
async fn review_rejected_local_check_does_not_settle_a_still_running_native_effect() {
    use genesis_server::capability_runtime::CapabilityRunPhase;
    let mut f = Fixture::new(Options {
        running_runs: true,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    f.adoption_step().await;
    let first = f.adoptions()[0].clone();
    assert_eq!(first.phase(), AdoptionPhase::Checking);
    let old_job = first.job_ids()[0].clone();
    let submitted = f
        .app
        .capability_runtime
        .step(&old_job, false)
        .await
        .unwrap();
    assert_eq!(submitted.phase, CapabilityRunPhase::Running);
    assert!(submitted.receipt.is_some());
    let posts = f.providers.runs();
    assert_eq!(posts, 1);

    // An ordinary restart with suite drift stops the adoption while the
    // provider's accepted run is still executing. Other checks are Prepared.
    let mut suite = b_suite();
    suite.cases[0].input["attendees"] =
        json!(suite.cases[0].input["attendees"].as_u64().unwrap() + 1);
    f.config.oracle_suites = vec![suite];
    f.reopen();
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Configuration))
    );
    let (_, before) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(before["retryable"], false, "{before}");

    // The normal capability worker also steps Running records. Since their
    // parent has stopped, this rejects the local result without canceling or
    // collecting the native run. Rejection therefore is not reconciliation.
    let rejected = f
        .app
        .capability_runtime
        .step(&old_job, false)
        .await
        .unwrap();
    assert_eq!(rejected.phase, CapabilityRunPhase::Rejected);
    assert_eq!(
        serde_json::to_value(&rejected.receipt).unwrap(),
        serde_json::to_value(&submitted.receipt).unwrap()
    );
    assert_eq!(f.providers.count("GET", "/v2/actor-runs/"), 0);
    assert_eq!(f.providers.runs(), posts);

    f.config.oracle_suites = vec![b_suite()];
    f.reopen();
    let (status_code, status_body) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(status_code, reqwest::StatusCode::OK, "{status_body}");
    let (retry_code, retry_body) = retry(&f, "operator").await;
    if retry_code == reqwest::StatusCode::OK {
        // Exercise an incorrectly allowed retry so the regression measures
        // the duplicate native POST, not only an optimistic status flag.
        assert_eq!(f.drain().await, Some(Step::Dispatched));
        let second_id = f.record().unwrap().adoption_id.unwrap();
        assert_ne!(second_id, first.id());
        f.adoption_step().await;
        f.adoption_step().await;
        let second = f.app.store.get_adoption(&second_id).unwrap();
        let new_job = second.job_ids()[0].clone();
        let replay = f
            .app
            .capability_runtime
            .step(&new_job, false)
            .await
            .unwrap();
        assert_eq!(replay.phase, CapabilityRunPhase::Running);
    }

    // The original native effect really remains RUNNING, independently of
    // the terminal label on its local checkpoint.
    let native: Value = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{}/v2/actor-runs/b-run-0", f.providers.url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(native["data"]["status"], "RUNNING");
    assert_eq!(
        f.providers.runs(),
        posts,
        "retry submitted a duplicate while the first provider run is still RUNNING; status={status_body}, retry={retry_code}: {retry_body}"
    );
    assert_eq!(status_body["retryable"], false, "{status_body}");
    assert_eq!(retry_code, reqwest::StatusCode::CONFLICT, "{retry_body}");
    assert_eq!(retry_body["error"], "adoption_checks_unresolved");
    let record = f.record().unwrap();
    assert_eq!(record.adoption_id.as_deref(), Some(first.id()));
    assert_eq!(record.attempt, 0);
    assert_eq!(record.state, GapAdoptionState::Attention);
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.providers.model_calls(), 0);
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn a_never_submitted_sibling_of_a_failed_check_settles_and_is_never_submitted_later() {
    // Henry's Claude: one check run FAILED at submission; its siblings were
    // prepared but never submitted. The stopped adoption can no longer
    // authorize them, so they settle the retry, and stepping them afterwards
    // rejects them locally with no POST.
    use genesis_server::capability_runtime::CapabilityRunPhase;
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    f.adoption_step().await;
    let first = f.adoptions()[0].clone();
    assert_eq!(first.phase(), AdoptionPhase::Checking);
    let jobs = first.job_ids().to_vec();
    assert!(jobs.len() > 1, "need siblings: {jobs:?}");
    f.providers.fail_submits(1);
    for _ in 0..10 {
        let _ = f.app.capability_runtime.step(&jobs[0], false).await;
        if f.app.store.get_capability_run(&jobs[0]).unwrap().phase == CapabilityRunPhase::Failed {
            break;
        }
    }
    assert_eq!(
        f.app.store.get_capability_run(&jobs[0]).unwrap().phase,
        CapabilityRunPhase::Failed
    );
    for job in &jobs[1..] {
        assert_eq!(
            f.app.store.get_capability_run(job).unwrap().phase,
            CapabilityRunPhase::Prepared
        );
    }
    let posts = f.providers.runs();
    assert_eq!(posts, 1);
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Checks))
    );
    let (_, status) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(status["retryable"], true, "{status}");
    let (code, body) = retry(&f, "operator").await;
    assert_eq!(code, reqwest::StatusCode::OK, "{body}");
    // The stopped adoption's prepared siblings are never submitted.
    for job in &jobs[1..] {
        let _ = f.app.capability_runtime.step(job, false).await;
        assert_eq!(
            f.app.store.get_capability_run(job).unwrap().phase,
            CapabilityRunPhase::Rejected
        );
    }
    assert_eq!(f.providers.runs(), posts);
}

#[tokio::test]
async fn review_failed_check_read_does_not_settle_a_still_running_native_effect() {
    use genesis_server::capability_runtime::CapabilityRunPhase;
    let f = Fixture::new(Options {
        running_runs: true,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    f.adoption_step().await;
    let first = f.adoptions()[0].clone();
    assert_eq!(first.phase(), AdoptionPhase::Checking);
    let old_job = first.job_ids()[0].clone();
    let submitted = f
        .app
        .capability_runtime
        .step(&old_job, false)
        .await
        .unwrap();
    assert_eq!(submitted.phase, CapabilityRunPhase::Running);
    assert!(submitted.receipt.is_some());
    let posts = f.providers.runs();
    assert_eq!(posts, 1);

    // The provider accepts the run, then refuses a read of that saved ticket.
    // HTTP 401 does not say whether the already-created native run has ended.
    f.providers.deny_run_reads(true);
    let failed = f
        .app
        .capability_runtime
        .step(&old_job, false)
        .await
        .unwrap();
    assert_eq!(failed.phase, CapabilityRunPhase::Failed);
    assert!(matches!(
        failed.failure,
        Some(genesis_providers::ProviderError::Http { status: 401, .. })
    ));
    assert_eq!(
        serde_json::to_value(&failed.receipt).unwrap(),
        serde_json::to_value(&submitted.receipt).unwrap()
    );
    assert_eq!(f.providers.count("GET", "/v2/actor-runs/"), 1);
    assert_eq!(f.providers.runs(), posts);
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Checks))
    );

    // Restoring read access does not establish that the original run ended.
    // A fresh submission still needs evidence of that run's terminal outcome.
    f.providers.deny_run_reads(false);
    let (status_code, status_body) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(status_code, reqwest::StatusCode::OK, "{status_body}");
    let (retry_code, retry_body) = retry(&f, "operator").await;
    if retry_code == reqwest::StatusCode::OK {
        assert_eq!(f.drain().await, Some(Step::Dispatched));
        let second_id = f.record().unwrap().adoption_id.unwrap();
        assert_ne!(second_id, first.id());
        f.adoption_step().await;
        f.adoption_step().await;
        let second = f.app.store.get_adoption(&second_id).unwrap();
        let new_job = second.job_ids()[0].clone();
        let replay = f
            .app
            .capability_runtime
            .step(&new_job, false)
            .await
            .unwrap();
        assert_eq!(replay.phase, CapabilityRunPhase::Running);
    }
    let native: Value = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{}/v2/actor-runs/b-run-0", f.providers.url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(native["data"]["status"], "RUNNING");
    assert_eq!(
        f.providers.runs(),
        posts,
        "retry submitted a duplicate after a failed read while the first provider run is still RUNNING; status={status_body}, retry={retry_code}: {retry_body}"
    );
    assert_eq!(status_body["retryable"], false, "{status_body}");
    assert_eq!(retry_code, reqwest::StatusCode::CONFLICT, "{retry_body}");
    assert_eq!(retry_body["error"], "adoption_checks_unresolved");
    let record = f.record().unwrap();
    assert_eq!(record.adoption_id.as_deref(), Some(first.id()));
    assert_eq!(record.attempt, 0);
    assert_eq!(record.state, GapAdoptionState::Attention);
    assert_eq!(f.adoptions().len(), 1);
    assert_eq!(f.providers.model_calls(), 0);
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn a_check_failed_after_submission_still_blocks_retry_without_terminal_evidence() {
    // Henry's Claude: POST accepted the run, then a status read reported it
    // FAILED. The local record keeps its receipt and a sanitized failure that
    // can't be told apart from a failed read (Codex, d84eb44), so retry stays
    // refused rather than risk a second POST beside a live run. Recovering this
    // case needs the runtime to record terminal provider evidence.
    use genesis_server::capability_runtime::CapabilityRunPhase;
    let f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    f.adoption_step().await;
    let first = f.adoptions()[0].clone();
    let job = first.job_ids()[0].clone();
    f.providers.fail_runs(1);
    for _ in 0..10 {
        let _ = f.app.capability_runtime.step(&job, false).await;
        if f.app.store.get_capability_run(&job).unwrap().phase == CapabilityRunPhase::Failed {
            break;
        }
    }
    let run = f.app.store.get_capability_run(&job).unwrap();
    assert_eq!(run.phase, CapabilityRunPhase::Failed);
    assert!(run.receipt.is_some(), "the POST was accepted");
    f.adoption_step().await;
    assert_eq!(
        f.drain().await,
        Some(Step::Attention(GapAdoptionCode::Checks))
    );
    let posts = f.providers.runs();
    let (code, body) = retry(&f, "operator").await;
    assert_eq!(code, reqwest::StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "adoption_checks_unresolved", "{body}");
    assert_eq!(f.record().unwrap().adoption_id.as_deref(), Some(first.id()));
    assert_eq!(f.providers.runs(), posts);
}

// ---------------------------------------------------------------------------
// Henry's Claude: why each registry candidate was or wasn't used.
// ---------------------------------------------------------------------------

fn decisions(
    f: &Fixture,
) -> Vec<genesis_server::capability_adoption_candidates::CandidateDecision> {
    let id = f
        .record()
        .and_then(|r| r.adoption_id)
        .expect("an adoption was dispatched");
    f.app.store.adoption_candidates(&id).unwrap()
}

fn kinds(f: &Fixture) -> Vec<(String, &'static str)> {
    decisions(f)
        .iter()
        .map(|d| (d.publisher.to_string(), d.decision.as_str()))
        .collect()
}

#[tokio::test]
async fn an_adopted_candidate_records_why_and_the_local_acceptance_that_decided_it() {
    let mut f = Fixture::new(Options::default()).await;
    f.serve().await;
    f.wait_for("installed", |f| installed(&f.gap_status()).is_some())
        .await;
    f.stop();
    let ds = decisions(&f);
    let adopted: Vec<_> = ds
        .iter()
        .filter(|d| d.decision.as_str() == "adopted")
        .collect();
    assert_eq!(adopted.len(), 1, "{ds:#?}");
    let adopted = adopted[0];
    let status = f.gap_status();
    let (id, _, installation) = installed(&status).unwrap();
    assert_eq!(adopted.capability_id.as_str(), FOREIGN);
    assert_eq!(adopted.version, "1.0.0");
    assert_ne!(adopted.publisher.as_str(), ORG);
    let acceptance = f.state().installations[installation]
        .acceptance_report_ref
        .clone()
        .unwrap();
    assert_eq!(adopted.acceptance_ref.as_deref(), Some(acceptance.as_str()));
    assert!(adopted.reason.contains("accepted"));
    // B's own publication is never inherited: skipped as its own when it
    // ranks first, otherwise never evaluated after the selection.
    for d in ds.iter().filter(|d| d.publisher.as_str() == ORG) {
        assert!(
            matches!(d.decision.as_str(), "own_publication" | "not_evaluated"),
            "{d:#?}"
        );
    }
    for d in ds.iter().filter(|d| d.rank > adopted.rank) {
        assert_eq!(d.decision.as_str(), "not_evaluated", "{d:#?}");
    }
    assert_eq!(f.record().unwrap().adoption_id.as_deref(), Some(id));
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn an_incompatible_candidate_is_rejected_with_a_reason_and_not_regenerated() {
    // Same domain, different facts: B's own suite rejects A's capability.
    let mut f = Fixture::new(Options::default()).await;
    f.providers.wrong_outputs();
    f.serve().await;
    f.wait_for("refused or attention", |f| {
        f.record().is_some_and(|r| {
            matches!(
                r.state,
                GapAdoptionState::Refused | GapAdoptionState::Attention
            )
        })
    })
    .await;
    f.stop();
    let ds = decisions(&f);
    let rejected: Vec<_> = ds
        .iter()
        .filter(|d| d.decision.as_str() == "not_applicable")
        .collect();
    assert_eq!(rejected.len(), 1, "{ds:#?}");
    let rejected = rejected[0];
    assert!(
        rejected.reason.contains("acceptance suite rejected it")
            && rejected.reason.contains("cases failed"),
        "{}",
        rejected.reason
    );
    assert!(rejected
        .acceptance_ref
        .as_deref()
        .is_some_and(|r| r.starts_with("adoption-rejection:")));
    // No expected (oracle) value leaks into the reason.
    let expected = receipts()[0].output["venues"][0]["capacity"].to_string();
    assert!(!rejected.reason.contains(&expected), "{}", rejected.reason);
    assert!(installed(&f.gap_status()).is_none());
    assert_eq!(
        f.providers.model_calls(),
        0,
        "a rejection is not regeneration"
    );
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn an_empty_search_records_each_skipped_candidate_and_no_selection() {
    let f = Fixture::new(Options {
        foreign: false,
        fallback: false,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    let k = kinds(&f);
    assert!(
        !k.is_empty(),
        "B's own publication was returned by the search"
    );
    assert!(k.iter().all(|(_, d)| *d == "own_publication"), "{k:?}");
    assert_eq!(
        f.adoptions()[0].attention_code(),
        Some(AdoptionAttention::NoCandidate)
    );
}

#[tokio::test]
async fn an_unreachable_registry_records_no_decision_and_stays_unknown() {
    let f = Fixture::new(Options {
        registry_up: false,
        ..Options::default()
    })
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    assert!(decisions(&f).is_empty(), "nothing was evaluated");
    assert_eq!(
        f.adoptions()[0].attention_code(),
        Some(AdoptionAttention::RegistryUnavailable)
    );
    assert!(!f.driver_bound(), "an unknown search is never an empty one");
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn candidate_decisions_survive_restart_and_repeated_passes_unchanged() {
    let mut f = Fixture::new(Options::default()).await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    let before = decisions(&f);
    assert!(before.iter().any(|d| d.decision.as_str() == "selected"));
    f.reopen();
    f.adoption_step().await;
    f.drain().await;
    let after = decisions(&f);
    // The selected row may only have been concluded; nothing else changes.
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(
            (b.rank, &b.capability_id, &b.version),
            (a.rank, &a.capability_id, &a.version)
        );
        if b.decision.as_str() != "selected" {
            assert_eq!(b, a);
        }
    }
}

#[tokio::test]
async fn candidate_decisions_are_operator_only_and_carry_no_private_text() {
    let mut f = Fixture::new(Options::default()).await;
    f.serve().await;
    f.wait_for("installed", |f| installed(&f.gap_status()).is_some())
        .await;
    f.stop();
    let id = f.record().unwrap().adoption_id.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = f.app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let get = |token: String| {
        http.get(format!("{url}/api/v1/capability-adoptions/{id}/candidates"))
            .bearer_auth(token)
            .send()
    };
    let agent = get(format!("{ORG}-agent")).await.unwrap();
    assert!(agent.status().is_client_error(), "{}", agent.status());
    let operator = get(format!("{ORG}-operator")).await.unwrap();
    assert_eq!(operator.status(), reqwest::StatusCode::OK);
    let body: Value = operator.json().await.unwrap();
    server.abort();
    assert_eq!(body["adoption_id"], json!(id));
    let items = body["items"].as_array().unwrap();
    assert!(items.iter().any(|d| d["decision"] == "adopted"), "{body}");
    let text = body.to_string();
    for case in &b_suite().cases {
        let expected = serde_json::to_string(&case.expected).unwrap();
        assert!(!text.contains(&expected), "oracle leaked: {text}");
    }
    assert!(!text.contains("\"intent\""), "{text}");
}

// Independent PR116 checks: logging must stay readable for refused answers and
// must not change first-eligible selection merely by examining later ranks.
async fn review_registry_answer(f: &mut Fixture, candidates: Value) -> tokio::task::JoinHandle<()> {
    let router = Router::new().fallback(move |request: Request| {
        let candidates = candidates.clone();
        async move {
            if request.uri().path().ends_with("/search") {
                Json(candidates)
            } else {
                Json(manifest(FOREIGN, "org-a"))
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    f.app.registry = Some(genesis_server::registry::RegistryClient::new(
        url,
        "fixture-reader",
    ));
    task
}

fn review_search_candidate(score: f64) -> Value {
    let m = manifest(FOREIGN, "org-a");
    json!({"capability_id":m["capability_id"],"version":m["version"],
        "publisher":m["publisher"],"source_digest":m["source_digest"],
        "intent":m["intent"],"applicability_conditions":m["applicability_conditions"],
        "required_permissions":m["required_permissions"],"score":score})
}

#[tokio::test]
async fn review_candidate_log_preserves_first_eligible_selection() {
    let mut f = Fixture::new(Options::default()).await;
    let registry = review_registry_answer(
        &mut f,
        json!([review_search_candidate(2.0), review_search_candidate(0.0)]),
    )
    .await;
    f.drain().await;
    f.drain().await;
    let id = f.record().unwrap().adoption_id.unwrap();
    let record = capability_adoption::step(&f.app, &id).await.unwrap();
    registry.abort();
    assert_eq!(
        record.phase(),
        AdoptionPhase::Selected,
        "observability-only logging must preserve selection before an unevaluated later candidate"
    );
}

#[tokio::test]
async fn review_overflow_score_refusal_keeps_candidate_log_readable() {
    let mut f = Fixture::new(Options::default()).await;
    // This is a valid finite JSON number, but it exceeds f32's range.
    let registry = review_registry_answer(&mut f, json!([review_search_candidate(1e39)])).await;
    f.drain().await;
    f.drain().await;
    let id = f.record().unwrap().adoption_id.unwrap();
    let record = capability_adoption::step(&f.app, &id).await.unwrap();
    registry.abort();
    assert_eq!(record.phase(), AdoptionPhase::Attention);
    let recorded = f.app.store.adoption_candidates(&id);
    assert!(
        recorded.is_ok(),
        "a refused registry score must not persist an unreadable decision: {recorded:?}"
    );
}

#[tokio::test]
async fn review_zero_score_refusal_is_readable_control() {
    let mut f = Fixture::new(Options::default()).await;
    let registry = review_registry_answer(&mut f, json!([review_search_candidate(0.0)])).await;
    f.drain().await;
    f.drain().await;
    let id = f.record().unwrap().adoption_id.unwrap();
    let record = capability_adoption::step(&f.app, &id).await.unwrap();
    registry.abort();
    assert_eq!(record.phase(), AdoptionPhase::Attention);
    assert_eq!(
        record.attention_code(),
        Some(AdoptionAttention::InvalidCandidate)
    );
    let recorded = f.app.store.adoption_candidates(&id).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].decision.as_str(), "invalid_response");
}

#[tokio::test]
async fn a_valid_first_candidate_is_adopted_and_a_later_overflowing_score_stays_readable() {
    // Henry's Claude, for Codex's #116 review: rank 0 is eligible and is
    // selected before rank 1's out-of-range score is ever examined. The
    // unevaluated row keeps no score (JSON can't hold infinity), so the log
    // stays readable through local acceptance and its conclusion.
    let mut f = Fixture::new(Options::default()).await;
    let registry = review_registry_answer(
        &mut f,
        json!([review_search_candidate(2.0), review_search_candidate(1e39)]),
    )
    .await;
    assert_eq!(f.drain().await, Some(Step::Frozen));
    assert_eq!(f.drain().await, Some(Step::Dispatched));
    f.adoption_step().await;
    assert_eq!(f.adoptions()[0].phase(), AdoptionPhase::Selected);
    let selected = decisions(&f);
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].decision.as_str(), "selected");
    assert_eq!(selected[0].score, Some(2.0));
    assert_eq!(selected[1].decision.as_str(), "not_evaluated");
    assert_eq!(selected[1].score, None);
    f.serve().await;
    f.wait_for("installed", |f| installed(&f.gap_status()).is_some())
        .await;
    f.stop();
    registry.abort();
    let done = decisions(&f);
    assert_eq!(done[0].decision.as_str(), "adopted");
    assert!(done[0].acceptance_ref.is_some());
    assert_eq!(done[1], selected[1]);
}

#[tokio::test]
async fn an_already_installed_version_records_an_installation_conflict_without_checks() {
    // Henry's Claude, for Codex's #116 review: a separate intent that selects
    // a version this organization already accepted stops at
    // `installation_conflict`, with the reason committed alongside the stop.
    // It never claims to have run acceptance checks.
    let mut f = Fixture::new(Options::default()).await;
    f.serve().await;
    f.wait_for("installed", |f| installed(&f.gap_status()).is_some())
        .await;
    f.stop();
    let posts = f.providers.runs();
    let gap = f.record().unwrap();
    let suite = f.app.evidence_policy.suite(&gap.suite_id).unwrap();
    let (second, created) = f
        .app
        .store
        .create_adoption(
            &genesis_server::capability_adoption_store::CreateAdoption {
                client_request_id: "operator-second-intent".into(),
                mission_id: gap.mission_id.clone(),
                requirement_id: gap.requirement_id.clone(),
                role_id: gap.role_id.clone(),
                responsibility_id: gap.responsibility_id.clone(),
                suite_id: gap.suite_id.clone(),
                provider_access_acknowledged: true,
                deadline: None,
            },
            suite,
            Utc::now(),
        )
        .unwrap();
    assert!(created);
    let stopped = capability_adoption::step(&f.app, second.id())
        .await
        .unwrap();
    assert_eq!(stopped.phase(), AdoptionPhase::Attention);
    assert_eq!(
        stopped.attention_code(),
        Some(AdoptionAttention::InstallationConflict)
    );
    let rows = f.app.store.adoption_candidates(second.id()).unwrap();
    let conflict: Vec<_> = rows
        .iter()
        .filter(|d| d.decision.as_str() == "installation_conflict")
        .collect();
    assert_eq!(conflict.len(), 1, "{rows:?}");
    assert!(conflict[0].acceptance_ref.is_none());
    assert!(conflict[0]
        .reason
        .contains("without running acceptance checks"));
    assert!(rows.iter().all(|d| !matches!(
        d.decision.as_str(),
        "selected" | "adopted" | "not_applicable"
    )));
    // Repeating the step changes nothing and submits nothing.
    capability_adoption::step(&f.app, second.id())
        .await
        .unwrap();
    assert_eq!(f.app.store.adoption_candidates(second.id()).unwrap(), rows);
    assert_eq!(f.providers.runs(), posts);
    assert_eq!(f.state().installations.len(), 1);
}

// Henry's Claude: what each gap adoption concluded. One test per acceptance
// scenario; the classifier is read-only, so each also checks nothing fell
// back to synthesis.
fn outcome(f: &Fixture) -> genesis_server::capability_adoption_outcome::OutcomeView {
    let record = f.record().expect("a gap adoption record");
    genesis_server::capability_gap_adoption::outcome(&f.app, &record)
        .unwrap()
        .expect("a readable decision log")
}

async fn settle(f: &mut Fixture) {
    f.serve().await;
    f.wait_for("a conclusion", |f| {
        f.record()
            .is_some_and(|r| r.state != GapAdoptionState::Active)
    })
    .await;
    f.stop();
}

#[tokio::test]
async fn outcome_compatible_candidate_is_adopted_with_its_exact_version_and_local_acceptance() {
    let mut f = Fixture::new(Options::default()).await;
    settle(&mut f).await;
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "adopted", "{o:?}");
    let c = o.candidate.expect("the adopted candidate");
    assert_eq!(c.decision.as_str(), "adopted");
    assert_eq!(c.publisher.as_str(), "org-a");
    assert_eq!(c.version, manifest(FOREIGN, "org-a")["version"]);
    assert!(c.acceptance_ref.is_some());
    // The same view on the status route.
    let (code, body) = adoption_status(&f, f.gap_id.as_str()).await;
    assert_eq!(code, reqwest::StatusCode::OK);
    assert_eq!(body["outcome"]["outcome"], "adopted", "{body}");
    assert_eq!(
        body["outcome"]["candidate"]["capability_id"],
        json!(FOREIGN)
    );
    assert!(body["outcome"].get("score").is_none());
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn outcome_same_domain_but_incompatible_is_not_applicable_with_the_local_reason() {
    let mut f = Fixture::new(Options::default()).await;
    f.providers.wrong_outputs();
    settle(&mut f).await;
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "not_applicable", "{o:?}");
    assert!(o.reason.contains("cases failed"), "{}", o.reason);
    let expected = receipts()[0].output["venues"][0]["capacity"].to_string();
    assert!(!o.reason.contains(&expected));
    assert!(o
        .candidate
        .as_ref()
        .and_then(|c| c.acceptance_ref.as_deref())
        .is_some_and(|r| r.starts_with("adoption-rejection:")));
    assert!(installed(&f.gap_status()).is_none());
    assert_eq!(f.providers.model_calls(), 0);
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn outcome_nothing_eligible_is_no_candidate_and_names_why_without_a_candidate() {
    let f = Fixture::new(Options {
        foreign: false,
        fallback: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    f.drain().await;
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "no_candidate", "{o:?}");
    assert!(o.candidate.is_none());
    assert!(o.considered >= 1);
    assert!(
        o.reason.contains("published by this organization"),
        "{}",
        o.reason
    );
    // The unrelated or own tool is never executed.
    assert_eq!(f.providers.runs(), 0);
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn outcome_unreachable_registry_is_unknown_and_never_no_candidate() {
    let f = Fixture::new(Options {
        registry_up: false,
        ..Options::default()
    })
    .await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    f.drain().await;
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "unknown", "{o:?}");
    assert_eq!(o.considered, 0);
    assert!(o.reason.contains("couldn't be searched"), "{}", o.reason);
    assert!(!f.driver_bound());
    assert_eq!(f.gap_status(), CapabilityGapStatus::Open);
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn outcome_unknown_check_is_unknown_and_a_confirmed_failed_check_is_insufficient_evidence() {
    let mut f = Fixture::new(Options {
        ambiguous_runs: true,
        ..Options::default()
    })
    .await;
    settle(&mut f).await;
    assert_eq!(f.record().unwrap().code, Some(GapAdoptionCode::Checks));
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "unknown", "{o:?}");
    assert!(o.reason.contains("unknown or unresolved"), "{}", o.reason);
    // The candidate was chosen, but no verdict: still `selected`.
    assert_eq!(
        o.candidate.as_ref().map(|c| c.decision.as_str()),
        Some("selected")
    );

    let mut f = Fixture::new(Options::default()).await;
    f.providers.fail_submits(usize::MAX);
    settle(&mut f).await;
    assert_eq!(f.record().unwrap().code, Some(GapAdoptionCode::Checks));
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "insufficient_evidence", "{o:?}");
    assert!(!f.driver_bound());
    assert_eq!(f.providers.model_calls(), 0);
}

#[tokio::test]
async fn outcome_an_unusable_score_is_insufficient_evidence_not_empty() {
    let mut f = Fixture::new(Options::default()).await;
    let registry = review_registry_answer(&mut f, json!([review_search_candidate(0.0)])).await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    f.drain().await;
    registry.abort();
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "insufficient_evidence", "{o:?}");
    assert!(o.reason.contains("unusable score"), "{}", o.reason);
    assert!(!f.driver_bound());
}

#[tokio::test]
async fn outcome_changed_authority_is_stopped_and_old_evidence_names_no_verdict() {
    let mut f = Fixture::new(Options::default()).await;
    f.drain().await;
    f.drain().await;
    f.adoption_step().await;
    f.adoption_step().await;
    f.change_requirement();
    settle(&mut f).await;
    assert_eq!(f.record().unwrap().code, Some(GapAdoptionCode::Authority));
    let o = outcome(&f);
    assert_eq!(o.outcome.as_str(), "stopped", "{o:?}");
    assert!(o.reason.contains("can't authorize"), "{}", o.reason);
    assert!(o
        .candidate
        .as_ref()
        .is_none_or(|c| c.decision.as_str() != "adopted"));
}

#[tokio::test]
async fn outcome_is_stable_across_restart() {
    let mut f = Fixture::new(Options::default()).await;
    f.providers.wrong_outputs();
    settle(&mut f).await;
    let before = outcome(&f);
    f.reopen();
    f.drain().await;
    assert_eq!(outcome(&f), before);
}
