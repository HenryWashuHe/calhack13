//! Henry's Claude: a watchable LOOPBACK rehearsal of the no-operator capability
//! beat, for practicing the demo with the console and without any provider key.
//!
//! ```sh
//! cargo run -p genesis-server --example rehearse_autonomy
//! (cd apps/web && npm run dev)     # default console endpoints match
//! ```
//!
//! Starts a registry (:7100), Org A (:7101) and Org B (:7102) with the
//! console's default dev tokens, plus a slowed loopback provider that stands
//! in for Anthropic and Apify. Org A commits `RequestCapability`; its own
//! workers synthesize, verify, publish, install and accept a parser. Then Org
//! B commits its own request under another name and adopts A's version after
//! its own suite accepts it.
//!
//! EVERYTHING EXTERNAL HERE IS A FIXTURE: no model writes code, no Actor is
//! built and no portal is read. Never present this as a live run. The point is
//! to rehearse timing and narration on the real kernel, workers and console.
//! Set `REHEARSAL_DELAY_MS` (default 700) to pace provider replies.
//! Set `REHEARSAL_PLANNER=1` to seed only the missions and observations and
//! let a real `genesis-planner run` (live model) make both requests.
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
    engine::{self, MissionBootstrap},
    ids::*,
    model::CapabilityGapStatus,
};
use genesis_forge::{AnthropicGenerator, ModelConfig, PublicExample};
use genesis_providers::{ApifyCapabilityExecutor, ProviderSet};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas},
    capability_gap_adoption::{GapAdoptionPolicy, GapAdoptionRule},
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

const MODEL: &str = "rehearsal-fixture-model";
const KEY_A: &str = "rehearsal-key-a";
const KEY_B: &str = "rehearsal-key-b";
const PERMISSION: &str = "capability.venue_lookup";
const NEED: &str = "Read venue capacity price availability from the public portal";
const SOURCE: &str = "https://fixtures.genesis.example/venues/index.html";

fn a_suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
fn b_suite() -> OracleSuite {
    let mut s = a_suite();
    s.suite_id = "rehearsal-suite-b".into();
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

#[derive(Default)]
struct World {
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
    fn reply(&mut self, org: &str, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        let (status, reply) = match (method, parts.as_slice()) {
            ("POST", ["v1", "messages"]) => (
                StatusCode::OK,
                json!({"id":"msg_rehearsal","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":json!({"extractor_typescript":"export function extract() { throw new Error('rehearsal fixture, never executed'); }"}).to_string()}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}),
            ),
            ("POST", ["v2", "actors"]) => {
                let id = format!("fixture-actor-{}", self.actors.len());
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
                let suite = if org == "A" { a_suite() } else { b_suite() };
                let Some(case) = suite.cases.iter().position(|c| c.input == body) else {
                    return (
                        StatusCode::BAD_REQUEST,
                        json!({"error":"input outside this organization's suite"}),
                    );
                };
                let id = format!("fixture-run-{}", self.runs.len());
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
            _ => (
                StatusCode::NOT_FOUND,
                json!({"error":"unknown fixture route"}),
            ),
        };
        println!(
            "  [fixture provider] org {org}: {method} {path} -> {}",
            status.as_u16()
        );
        (status, reply)
    }
}

async fn provider(delay: Duration) -> String {
    let world = Arc::new(Mutex::new(World::default()));
    let router = Router::new()
        .fallback(
            move |State(world): State<Arc<Mutex<World>>>, request: Request| async move {
                let method = request.method().to_string();
                let path = request.uri().path().to_owned();
                let key = request
                    .headers()
                    .get("x-api-key")
                    .or_else(|| request.headers().get("authorization"))
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .trim_start_matches("Bearer ")
                    .to_owned();
                let org = if key == KEY_A { "A" } else { "B" };
                let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
                    .await
                    .unwrap_or_default();
                let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                tokio::time::sleep(delay).await;
                let (status, body) = world.lock().unwrap().reply(org, &method, &path, body);
                (status, Json(body))
            },
        )
        .with_state(world);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

fn limits() -> RunLimits {
    RunLimits {
        request_timeout: Duration::from_secs(10),
        deadline: Duration::from_secs(30),
        poll_interval: Duration::from_millis(200),
        ..RunLimits::default()
    }
}

fn gap_policy(suite: &OracleSuite, capability: &str) -> GapAcquisitionPolicy {
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: capability.into(),
            suite_id: suite.suite_id.clone(),
            actor_name_prefix: "gap-reader".into(),
            allowed_source_urls: suite.cases.iter().map(|c| c.source.url.clone()).collect(),
            examples: vec![PublicExample {
                url: suite.cases[0].source.url.clone(),
                html: "<html>REHEARSAL_PUBLIC_EXAMPLE</html>".into(),
            }],
            repair: false,
        }],
    }
}

fn config(
    dir: &std::path::Path,
    org: &str,
    token: &str,
    suite: OracleSuite,
    url: &str,
    key: &str,
) -> OrgConfig {
    let mut config = OrgConfig::dev(org, org);
    config.db = Some(dir.join(format!("{org}.sqlite")));
    config.dev_stub = false;
    config.registry_url = Some("http://127.0.0.1:7100".into());
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
        AnthropicGenerator::with_test_endpoint(&format!("{url}/v1/messages"), key).unwrap(),
    ));
    acquisition.apify = Some(Arc::new(
        ApifyClient::for_local_testing(key, limits(), url).unwrap(),
    ));
    acquisition.automatic = true;
    config.acquisition = Some(acquisition);
    config
}

/// A mission and the portal observation that starts the beat.
async fn seed(app: &AppState, goal: &str) -> (MissionId, ObservationId) {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": goal, "roles":[{"key":"venue","name":"Venue operations","charter":"Owns venue lookup",
            "authority":{"scopes":["capability.acquire",PERMISSION],"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Keep a venue booked","owner":"venue"}]
    }))
    .unwrap();
    let mission = app
        .mutate(EventMeta::default(), |s| {
            engine::bootstrap_mission(s, &spec, Utc::now()).map_err(validation_error)
        })
        .await
        .unwrap()
        .value;
    let observation = app
        .mutate(EventMeta::default(), |s| {
            engine::record_observation(
                s,
                &mission,
                "public portal",
                "The venue portal changed layout; nothing here can read it",
                Some(SOURCE.into()),
                Utc::now(),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap()
        .value;
    (mission, observation)
}

/// What a planner would commit: a mission whose change requests a capability.
async fn request(app: &AppState, capability: &str, goal: &str) -> CapabilityGapId {
    let (mission, observation) = seed(app, goal).await;
    let requirement = app
        .mutate(EventMeta::default(), |s| {
            engine::record_requirement(
                s,
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
    let sources: Vec<String> = a_suite()
        .cases
        .iter()
        .map(|c| c.source.url.clone())
        .collect();
    let proposal: OrgChangeProposal = serde_json::from_value(json!({
        "expected_org_revision":state.revision, "trigger_evidence_ids":[observation],
        "rationale":"The venue team needs a reader for the changed portal", "budget_limit_cents":0,
        "operations":[
            {"op":"AssignResponsibility", "target":{"kind":"from_requirement","key":"reader","requirement_id":requirement,
                "purpose":"Read venue listings","trigger":"Layout changes","termination":"Verified parser available"},
                "owner":{"kind":"existing","role_id":role}},
            {"op":"RequestCapability", "key":"parser", "requirement_id":requirement,
                "role":{"kind":"existing","role_id":role}, "responsibility":{"kind":"new","key":"reader"},
                "capability_id":capability, "purpose":"Read venue listings", "observed_sources":sources}
        ]
    }))
    .unwrap();
    let allocation = KeyAllocation::allocate(&proposal);
    let gap = allocation.gaps["parser"].clone();
    app.mutate(EventMeta::default(), |s| {
        engine::commit(
            s,
            &proposal,
            &allocation,
            &Default::default(),
            &engine::local_catalog(s),
            Utc::now(),
        )
        .map(|o| (o, ()))
        .map_err(validation_error)
    })
    .await
    .unwrap();
    gap
}

/// The first gap a planner opens in this organization.
async fn first_gap(app: &AppState) -> CapabilityGapId {
    loop {
        if let Some(gap) = app
            .current_state()
            .unwrap()
            .capability_gaps
            .values()
            .min_by_key(|g| g.opened_in_revision)
        {
            println!(
                "planner requested {} for {:?}",
                gap.capability_id, gap.observed_sources
            );
            return gap.gap_id.clone();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

async fn follow(app: &AppState, gap: &CapabilityGapId, org: &str) {
    let mut last = String::new();
    loop {
        let status = app.current_state().unwrap().capability_gaps[gap]
            .status
            .clone();
        let label = match &status {
            CapabilityGapStatus::Open => "REQUESTED".to_owned(),
            CapabilityGapStatus::Acquiring { acquisition_id } => {
                format!("ACQUIRING ({acquisition_id})")
            }
            CapabilityGapStatus::Acquired {
                capability,
                publication_ref,
                ..
            } => format!(
                "VERIFIED {}@{}{}",
                capability.capability_id,
                capability.version,
                publication_ref
                    .as_ref()
                    .map(|p| format!(", published {p}"))
                    .unwrap_or_default()
            ),
            CapabilityGapStatus::Installed {
                capability,
                acquisition_id,
                ..
            } => format!(
                "INSTALLED {}@{} via {acquisition_id}",
                capability.capability_id, capability.version
            ),
            CapabilityGapStatus::Refused { reason, .. } => format!("REFUSED: {reason}"),
        };
        // Adopt-first works while the kernel gap is still open; show it too.
        let adoption = app
            .store
            .gap_adoption(gap)
            .ok()
            .flatten()
            .map(|r| match (&r.adoption_id, r.state) {
                (Some(id), s) => format!(" · adopt first: {s:?} ({id})"),
                (None, s) => format!(" · adopt first: {s:?}"),
            })
            .unwrap_or_default();
        let label = format!("{label}{adoption}");
        if label != last {
            println!("{org} gap {gap}: {label}");
            last = label;
        }
        if matches!(
            status,
            CapabilityGapStatus::Installed { .. } | CapabilityGapStatus::Refused { .. }
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let delay = Duration::from_millis(
        std::env::var("REHEARSAL_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(700),
    );
    println!("LOOPBACK FIXTURE REHEARSAL: no live provider is contacted.");
    let url = provider(delay).await;
    spawn_registry(
        RegistryState::open_in_memory(
            parse_publishers("org_a:a-registry,org_b:b-registry"),
            vec![],
        )?,
        "127.0.0.1:7100",
    )
    .await?;
    let dir = tempfile::tempdir()?;

    let mut a_config = config(dir.path(), "org_a", "a-registry", a_suite(), &url, KEY_A);
    a_config.gap_acquisition_policy = Some(gap_policy(&a_suite(), "venue-reader"));
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
    let a = build_org(
        &a_config,
        Some(
            ProviderSet::new().with_executor(Arc::new(ApifyCapabilityExecutor::new(
                ApifyClient::for_local_testing(KEY_A, limits(), &url)?,
            ))),
        ),
    )?;
    let mut b_config = config(dir.path(), "org_b", "b-registry", b_suite(), &url, KEY_B);
    b_config.gap_acquisition_policy = Some(gap_policy(&b_suite(), "capacity-checker"));
    b_config.gap_adoption_policy = Some(GapAdoptionPolicy {
        rules: vec![GapAdoptionRule {
            capability_id: "capacity-checker".into(),
            suite_id: b_suite().suite_id,
            synthesis_fallback: false,
        }],
    });
    let b = build_org(
        &b_config,
        Some(
            ProviderSet::new().with_executor(Arc::new(ApifyCapabilityExecutor::new(
                ApifyClient::for_local_testing(KEY_B, limits(), &url)?,
            ))),
        ),
    )?;
    for (app, port) in [(a.clone(), 7101), (b.clone(), 7102)] {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
    }
    println!("Org A http://127.0.0.1:7101  Org B http://127.0.0.1:7102  (console defaults)");
    println!("Starting in 5s; open the console now.");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // REHEARSAL_PLANNER=1: only the mission and the portal observation are
    // seeded; a real `genesis-planner run` (with GENESIS_LLM_* set) must
    // propose the requirement and RequestCapability itself. That checks the
    // live model picks a name, sources and role the policies accept.
    let planner = std::env::var("REHEARSAL_PLANNER").is_ok_and(|v| v == "1");
    let a_goal = "Run the campus hack night for 120 attendees";
    let b_goal = "Book a venue for the 150-person robotics meetup";
    let a_gap = if planner {
        println!("\nOrg A: portal observation recorded. Run in another terminal:");
        println!("  genesis-planner --org-url http://127.0.0.1:7101 --token org_a-agent run");
        seed(&a, a_goal).await;
        first_gap(&a).await
    } else {
        println!("\nOrg A commits RequestCapability for venue-reader.");
        request(&a, "venue-reader", a_goal).await
    };
    follow(&a, &a_gap, "Org A").await;

    let b_gap = if planner {
        println!("\nOrg B: portal observation recorded. Run in another terminal:");
        println!("  genesis-planner --org-url http://127.0.0.1:7102 --token org_b-agent run");
        seed(&b, b_goal).await;
        first_gap(&b).await
    } else {
        println!("\nOrg B, a different organization, commits its own request as capacity-checker.");
        request(&b, "capacity-checker", b_goal).await
    };
    follow(&b, &b_gap, "Org B").await;

    println!("\nDone. Servers keep running for the console; Ctrl-C to stop.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
