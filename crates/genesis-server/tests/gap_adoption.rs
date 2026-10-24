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
    collections::BTreeMap,
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
    requests: Vec<(String, String)>,
    runs: BTreeMap<usize, usize>,
}
impl World {
    fn run(run: usize) -> Value {
        json!({"data":{"id":format!("b-run-{run}"),"actId":pin().actor_id,"buildId":pin().build_id,
            "status":"SUCCEEDED","defaultDatasetId":format!("b-dataset-{run}")}})
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
                if self.ambiguous_runs {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error":"PRIVATE_AMBIGUOUS_RUN"}),
                    );
                }
                (StatusCode::CREATED, Self::run(run))
            }
            ("GET", ["v2", "actor-runs", run]) => {
                let run: usize = run.strip_prefix("b-run-").unwrap().parse().unwrap();
                (StatusCode::OK, Self::run(run))
            }
            ("GET", ["v2", "datasets", dataset, "items"]) => {
                let run: usize = dataset.strip_prefix("b-dataset-").unwrap().parse().unwrap();
                (
                    StatusCode::OK,
                    json!([receipts()[self.runs[&run]].output.clone()]),
                )
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
    async fn start(ambiguous_runs: bool) -> Self {
        let world = Arc::new(Mutex::new(World {
            ambiguous_runs,
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
        let providers = Arc::new(Providers::start(o.ambiguous_runs).await);
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
async fn unknown_check_outcomes_need_an_operator_and_retries_are_bounded() {
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
    // Agents can't retry; an operator can, as a new attempt that keeps the
    // stopped adoption (and its unknown run) in history.
    let (status, _) = retry(&f, "agent").await;
    assert!(status.is_client_error(), "{status}");
    let (status, body) = retry(&f, "operator").await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(body["attempt"], 1);
    assert_eq!(body["previous_adoption_ids"], json!([first]));
    // The stopped adoption is never resubmitted or revived.
    assert_eq!(f.providers.runs(), posts);
    assert!(!f.driver_bound());
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
