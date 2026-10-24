//! Henry's Claude: the two-organization capability beat with no operator
//! calls on either side.
//!
//! Org A commits `RequestCapability`; its own workers synthesize, verify,
//! publish, install and accept a parser (#66 driver, #78 finalizer). Later Org
//! B commits its own `RequestCapability` for the same need under a different
//! name; its adopt-first coordinator finds A's publication, installs it and
//! accepts it only with B's own suite. Both organizations run real
//! `serve_org`, share one real registry, and talk to one loopback provider
//! that records which organization's key made each call. No source runs
//! locally and no live provider is contacted.
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
    capability_gap_adoption::{GapAdoptionPolicy, GapAdoptionRule, GapAdoptionState},
    capability_gap_finalizer::{GapFinalizationPolicy, GapFinalizationRule},
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    parse_publishers,
    registry::RegistryState,
    serve_org, spawn_registry,
    store::EventMeta,
    AcquisitionConfig, OrgConfig,
};
use genesis_verification::{decode_json, ExecutionEvidence, OracleSuite};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

const MODEL: &str = "claude-sonnet-4-6";
const KEY_A: &str = "two-org-loopback-key-a";
const KEY_B: &str = "two-org-loopback-key-b";
const PERMISSION: &str = "capability.venue_lookup";
const NEED: &str = "Read venue capacity price availability from the public portal";
const SOURCE: &str = "https://fixtures.genesis.example/venues/index.html";

fn a_suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
/// B's own suite: different case IDs and inputs; A never sees it.
fn b_suite() -> OracleSuite {
    let mut s = a_suite();
    s.suite_id = "two-org-suite-b".into();
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Org {
    A,
    B,
}

#[derive(Default)]
struct World {
    calls: Vec<(Org, String, String)>,
    actors: BTreeMap<String, Value>,
    runs: BTreeMap<String, (String, Value)>,
}
impl World {
    fn build(&self, actor: &str) -> Value {
        let source = &self.actors[actor]["versions"][0];
        let version = source["versionNumber"].as_str().unwrap();
        json!({"data":{"id":format!("build-{actor}"),"actId":actor,"buildNumber":format!("{version}.1"),"status":"SUCCEEDED","actVersion":source}})
    }
    fn run(&self, run: &str) -> Value {
        let actor = &self.runs[run].0;
        json!({"data":{"id":run,"actId":actor,"buildId":format!("build-{actor}"),"status":"SUCCEEDED","defaultDatasetId":format!("dataset-{run}")}})
    }
    fn reply(&mut self, org: Org, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.calls.push((org, method.into(), path.into()));
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "messages"]) => (
                StatusCode::OK,
                json!({"id":"msg_two_org","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":json!({"extractor_typescript":"export function extract() { throw new Error('loopback fixture, never executed locally'); }"}).to_string()}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":120,"output_tokens":80}}),
            ),
            ("POST", ["v2", "actors"]) => {
                let id = format!("actor-two-org-{}", self.actors.len());
                let mut response = body.clone();
                response["id"] = json!(id);
                self.actors.insert(id, body);
                (StatusCode::CREATED, json!({"data":response}))
            }
            ("GET", ["v2", "actors", actor, "versions", _]) => (
                StatusCode::OK,
                json!({"data":self.actors[*actor]["versions"][0]}),
            ),
            ("POST", ["v2", "actors", actor, "builds"]) => (StatusCode::CREATED, self.build(actor)),
            ("GET", ["v2", "actor-builds", build]) => (
                StatusCode::OK,
                self.build(build.strip_prefix("build-").unwrap()),
            ),
            ("POST", ["v2", "actors", actor, "runs"]) => {
                // Each organization may only execute its own suite's inputs.
                let suite = if org == Org::A { a_suite() } else { b_suite() };
                let case = suite
                    .cases
                    .iter()
                    .position(|case| case.input == body)
                    .unwrap_or_else(|| panic!("{org:?} ran an input outside its own suite"));
                let id = format!("run-two-org-{}", self.runs.len());
                self.runs.insert(
                    id.clone(),
                    ((*actor).into(), receipts()[case].output.clone()),
                );
                (StatusCode::CREATED, self.run(&id))
            }
            ("GET", ["v2", "actor-runs", run]) => (StatusCode::OK, self.run(run)),
            ("GET", ["v2", "datasets", dataset, "items"]) => (
                StatusCode::OK,
                json!([self.runs[dataset.strip_prefix("dataset-").unwrap()].1]),
            ),
            _ => panic!("unexpected loopback route {method} {path}"),
        }
    }
}

struct Provider {
    world: Arc<Mutex<World>>,
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Provider {
    async fn start() -> Self {
        let world = Arc::new(Mutex::new(World::default()));
        let router = Router::new()
            .fallback(
                |State(world): State<Arc<Mutex<World>>>, request: Request| async move {
                    let method = request.method().to_string();
                    let path = request.uri().path().to_owned();
                    let key = request
                        .headers()
                        .get("x-api-key")
                        .map(|v| v.to_str().unwrap().to_owned())
                        .or_else(|| {
                            request.headers().get("authorization").map(|v| {
                                v.to_str().unwrap().trim_start_matches("Bearer ").to_owned()
                            })
                        })
                        .expect("authenticated provider call");
                    let org = match key.as_str() {
                        KEY_A => Org::A,
                        KEY_B => Org::B,
                        other => panic!("unknown provider key {other}"),
                    };
                    let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
                        .await
                        .unwrap();
                    let body = if bytes.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes).unwrap()
                    };
                    let (status, body) = world.lock().unwrap().reply(org, &method, &path, body);
                    (status, Json(body))
                },
            )
            .with_state(world.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { world, url, task }
    }
    fn count(&self, org: Org, method: &str, pred: impl Fn(&str) -> bool) -> usize {
        self.world
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(o, m, p)| *o == org && m == method && pred(p))
            .count()
    }
    fn model_calls(&self, org: Org) -> usize {
        self.count(org, "POST", |p| p == "/v1/messages")
    }
    fn actor_creations(&self, org: Org) -> usize {
        self.count(org, "POST", |p| p == "/v2/actors")
    }
    fn runs(&self, org: Org) -> usize {
        self.count(org, "POST", |p| p.ends_with("/runs"))
    }
    fn apify(&self, key: &str) -> ApifyClient {
        ApifyClient::for_local_testing(key, limits(), &self.url).unwrap()
    }
}
fn limits() -> RunLimits {
    RunLimits {
        request_timeout: Duration::from_secs(5),
        deadline: Duration::from_secs(8),
        poll_interval: Duration::from_millis(1),
        ..RunLimits::default()
    }
}

fn gap_policy(suite: &OracleSuite) -> GapAcquisitionPolicy {
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: "venue-reader".into(),
            suite_id: suite.suite_id.clone(),
            actor_name_prefix: "gap-reader".into(),
            allowed_source_urls: suite.cases.iter().map(|c| c.source.url.clone()).collect(),
            examples: vec![PublicExample {
                url: suite.cases[0].source.url.clone(),
                html: "<html>FIXED_PUBLIC_GAP_EXAMPLE</html>".into(),
            }],
            repair: false,
        }],
    }
}
/// B names the same need differently; adoption may still satisfy it.
fn b_gap_policy() -> GapAcquisitionPolicy {
    let mut p = gap_policy(&b_suite());
    p.rules[0].capability_id = "capacity-checker".into();
    p
}

fn org_config(
    dir: &tempfile::TempDir,
    org: &str,
    registry: &str,
    token: &str,
    suite: OracleSuite,
    provider: &Provider,
    key: &str,
) -> OrgConfig {
    let mut config = OrgConfig::dev(org, org);
    config.db = Some(dir.path().join(format!("{org}.sqlite")));
    config.dev_stub = false;
    config.registry_url = Some(registry.into());
    config.registry_token = Some(token.into());
    config.oracle_suites = vec![suite];
    let mut acquisition = AcquisitionConfig::offline(AcquisitionPolicy {
        model: ModelConfig {
            model: MODEL.into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 3600,
    });
    acquisition.model = Some(Arc::new(
        AnthropicGenerator::with_test_endpoint(&format!("{}/v1/messages", provider.url), key)
            .unwrap(),
    ));
    acquisition.apify = Some(Arc::new(provider.apify(key)));
    acquisition.automatic = true;
    config.acquisition = Some(acquisition);
    config
}

fn providers(provider: &Provider, key: &str) -> ProviderSet {
    ProviderSet::new().with_executor(Arc::new(ApifyCapabilityExecutor::new(provider.apify(key))))
}

/// Bootstraps a mission whose committed change requests `capability_id`.
async fn request_capability(
    app: &AppState,
    capability_id: &str,
    sources: Vec<String>,
) -> CapabilityGapId {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Find a venue", "roles":[{"key":"lead","name":"Lead","charter":"Own venue lookup",
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
                NEED,
                std::slice::from_ref(&observation),
                "Pass this organization's own extraction checks",
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    let state = app.current_state().unwrap();
    let role = state.roles.keys().next().unwrap().clone();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision":state.revision, "trigger_evidence_ids":[observation],
        "rationale":"The requirement needs a parser", "budget_limit_cents":0,
        "operations":[
            {"op":"AssignResponsibility", "target":{"kind":"from_requirement","key":"reader","requirement_id":requirement,
                "purpose":"Read venue listings","trigger":"Layout changes","termination":"Verified parser available"},
                "owner":{"kind":"existing","role_id":role}},
            {"op":"RequestCapability", "key":"parser", "requirement_id":requirement,
                "role":{"kind":"existing","role_id":role}, "responsibility":{"kind":"new","key":"reader"},
                "capability_id":capability_id, "purpose":"Read venue listings", "observed_sources":sources}
        ]
    }))
    .unwrap();
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
    gap_id
}

async fn serve(app: &AppState) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app = app.clone();
    tokio::spawn(async move { serve_org(app, listener).await.unwrap() })
}

async fn until_installed(app: &AppState, gap: &CapabilityGapId, what: &str) -> OrgState {
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let state = app.current_state().unwrap();
            match &state.capability_gaps[gap].status {
                CapabilityGapStatus::Installed { .. } => return state,
                CapabilityGapStatus::Refused { reason, .. } => panic!("{what} refused: {reason}"),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await;
    result.unwrap_or_else(|_| panic!("{what} gap was not installed"))
}

#[tokio::test]
async fn b_inherits_a_capability_that_a_synthesized_with_no_operator_calls() {
    let provider = Provider::start().await;
    let registry = spawn_registry(
        RegistryState::open_in_memory(
            parse_publishers("org-a:a-registry,org-b:b-registry"),
            vec![],
        )
        .unwrap(),
        "127.0.0.1:0",
    )
    .await
    .unwrap();
    let registry = format!("http://{registry}");
    let dir = tempfile::tempdir().unwrap();

    // Org A: synthesize, verify, publish and install its own parser.
    let mut a_config = org_config(
        &dir,
        "org-a",
        &registry,
        "a-registry",
        a_suite(),
        &provider,
        KEY_A,
    );
    a_config.gap_acquisition_policy = Some(gap_policy(&a_suite()));
    a_config.gap_finalization_policy = Some(GapFinalizationPolicy {
        rules: vec![GapFinalizationRule {
            capability_id: CapabilityId::new("venue-reader"),
            suite_id: a_suite().suite_id,
            intent: NEED.into(),
            applicability_conditions: vec!["Public venue portal pages".into()],
            known_limitations: vec!["Lookup only; no reservation".into()],
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object"}),
            required_permissions: [PERMISSION.to_owned()].into(),
            responsibility_template: Some("Keep verified venue lookup available".into()),
            workflow_procedure: None,
            local_configuration_schema: json!({}),
        }],
    });
    let a = build_org(&a_config, Some(providers(&provider, KEY_A))).unwrap();
    let a_gap = request_capability(
        &a,
        "venue-reader",
        gap_policy(&a_suite()).rules[0].allowed_source_urls.clone(),
    )
    .await;
    let a_server = serve(&a).await;
    let a_state = until_installed(&a, &a_gap, "org A").await;
    let CapabilityGapStatus::Installed {
        capability: a_capability,
        ..
    } = a_state.capability_gaps[&a_gap].status.clone()
    else {
        unreachable!()
    };
    assert_eq!(provider.model_calls(Org::A), 1);
    assert_eq!(provider.actor_creations(Org::A), 1);
    let a_runs = provider.runs(Org::A);

    // Org B: its own mission, its own name for the need, its own suite.
    let mut b_config = org_config(
        &dir,
        "org-b",
        &registry,
        "b-registry",
        b_suite(),
        &provider,
        KEY_B,
    );
    b_config.gap_acquisition_policy = Some(b_gap_policy());
    b_config.gap_adoption_policy = Some(GapAdoptionPolicy {
        rules: vec![GapAdoptionRule {
            capability_id: "capacity-checker".into(),
            suite_id: b_suite().suite_id,
            synthesis_fallback: true,
        }],
    });
    let b = build_org(&b_config, Some(providers(&provider, KEY_B))).unwrap();
    let b_gap = request_capability(
        &b,
        "capacity-checker",
        b_gap_policy().rules[0].allowed_source_urls.clone(),
    )
    .await;
    let b_server = serve(&b).await;
    let b_state = until_installed(&b, &b_gap, "org B").await;

    // B's gap names A's exact published pin and an installation B accepted.
    let CapabilityGapStatus::Installed {
        capability,
        installation_id,
        acquisition_id,
    } = b_state.capability_gaps[&b_gap].status.clone()
    else {
        unreachable!()
    };
    assert_eq!(capability, a_capability, "B inherited A's exact version");
    let installation = &b_state.installations[&installation_id];
    assert_eq!(installation.status, InstallationStatus::Accepted);
    let pinned = b.store.capability_manifest(&installation_id).unwrap();
    assert_eq!(pinned.publisher.as_str(), "org-a");
    let record = b.store.gap_adoption(&b_gap).unwrap().unwrap();
    assert_eq!(record.state, GapAdoptionState::Installed);
    assert_eq!(record.adoption_id.as_deref(), Some(acquisition_id.as_str()));
    let adoption = b.store.get_adoption(&acquisition_id).unwrap();
    assert_eq!(adoption.job_ids().len(), b_suite().cases.len());

    // B never synthesized: no model call, no Actor, no gap acquisition. Its
    // only provider work was its own suite against A's build.
    assert_eq!(provider.model_calls(Org::B), 0);
    assert_eq!(provider.actor_creations(Org::B), 0);
    assert_eq!(provider.runs(Org::B), b_suite().cases.len());
    assert!(b
        .store
        .gap_acquisition_binding(b_gap.as_str())
        .unwrap()
        .is_none());
    // A did nothing more for B.
    assert_eq!(provider.runs(Org::A), a_runs);
    assert_eq!(provider.model_calls(Org::A), 1);
    // Each kernel woke its planner once for its own gap.
    for state in [&a.current_state().unwrap(), &b_state] {
        let notes = state
            .observations
            .values()
            .filter(|o| o.source == CAPABILITY_GAP_SOURCE)
            .count();
        assert_eq!(notes, 1);
        assert!(genesis_core::audit::audit(state).is_empty());
    }
    a_server.abort();
    b_server.abort();
}
