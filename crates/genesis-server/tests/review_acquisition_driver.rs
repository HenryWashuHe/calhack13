//! Review tests for the automatic acquisition scheduler's local-only reconcile
//! pass (`capability_acquisition_driver::drain_once`). A paused schedule
//! (quota) is reconciled locally once due: if the acquisition did not change,
//! its attention code must survive; if reconciliation advanced the record, the
//! stale code must not follow the new version.
//!
//! Real kernel commits and SQLite. Providers are a loopback endpoint that only
//! counts requests; no provider request is expected in either test. The due
//! time is moved into the past with one SQL write, the same clock shim used by
//! `tests/acquisition_history.rs`, instead of sleeping five seconds.
use axum::{extract::State, http::StatusCode, Router};
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::{
    change::{KeyAllocation, OrgChangeProposal},
    engine::{self, MissionBootstrap},
    ids::*,
    lifecycle::RoleState,
    model::{CapabilityGap, OrgState},
};
use genesis_forge::{AnthropicGenerator, ModelConfig, PublicExample};
use genesis_server::{
    app::AppState,
    build_org,
    capability_acquisition_driver::{self, AutomationState, DrainSummary, DriverErrorCode},
    capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas, AcquisitionRecord},
    capability_gap_driver,
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    error::validation_error,
    store::EventMeta,
    AcquisitionConfig, OrgConfig,
};
use genesis_verification::{decode_json, OracleSuite};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

const MODEL: &str = "claude-sonnet-4-6";
const KEY: &str = "review-acquisition-driver-loopback-key";
const PAST: &str = "2000-01-01T00:00:00.000000000Z";

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}

fn gap_policy() -> GapAcquisitionPolicy {
    let suite = suite();
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: "venue-reader".into(),
            suite_id: suite.suite_id.clone(),
            actor_name_prefix: "review-reader".into(),
            allowed_source_urls: suite
                .cases
                .iter()
                .map(|case| case.source.url.clone())
                .collect(),
            examples: vec![PublicExample {
                url: suite.cases[0].source.url.clone(),
                html: "<html>REVIEW_PUBLIC_EXAMPLE</html>".into(),
            }],
            repair: false,
        }],
    }
}

/// Loopback provider that refuses everything and counts each request.
struct CountingProvider {
    calls: Arc<AtomicUsize>,
    model: Arc<AnthropicGenerator>,
    apify: Arc<ApifyClient>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for CountingProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl CountingProvider {
    async fn start() -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .fallback(|State(calls): State<Arc<AtomicUsize>>| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::INTERNAL_SERVER_ERROR
            })
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let model = Arc::new(
            AnthropicGenerator::with_test_endpoint(&format!("{url}/v1/messages"), KEY).unwrap(),
        );
        let apify = Arc::new(
            ApifyClient::for_local_testing(
                KEY,
                RunLimits {
                    request_timeout: Duration::from_secs(5),
                    deadline: Duration::from_secs(8),
                    poll_interval: Duration::from_millis(1),
                    ..RunLimits::default()
                },
                &url,
            )
            .unwrap(),
        );
        Self {
            calls,
            model,
            apify,
            task,
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    config: OrgConfig,
    app: AppState,
    gap_id: CapabilityGapId,
}
impl Fixture {
    /// Commits a genuine gap, lets the gap driver create its one acquisition
    /// intent, then exhausts the model-call quota so the first automatic step
    /// pauses with `quota` before any provider request.
    async fn paused_on_quota(org: &str, providers: &CountingProvider) -> (Self, AcquisitionRecord) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev(org, "Acquisition driver review fixture");
        config.db = Some(dir.path().join("org.sqlite"));
        config
            .constitution
            .scopes
            .insert("capability.acquire".into());
        config.oracle_suites = vec![suite()];
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
        config.acquisition = Some(acquisition);
        let mut app = build_org(&config, None).unwrap();
        app.gap_acquisition_policy = Some(Arc::new(gap_policy()));
        let spec: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Find a venue", "roles":[{"key":"lead","name":"Lead","charter":"Own the missing parser",
                "authority":{"scopes":["capability.acquire"],"budget_cents":0}}],
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
                    Some("https://fixtures.genesis.example/venues/index.html".into()),
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
                    "Read public venue listings",
                    std::slice::from_ref(&observation),
                    "Pass independent extraction checks",
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
                    "capability_id":"venue-reader", "purpose":"Read venue listings", "observed_sources":gap_policy().rules[0].allowed_source_urls}
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
        let mut f = Self {
            _dir: dir,
            config,
            app,
            gap_id,
        };
        capability_gap_driver::drain_once(&f.app).await.unwrap();
        let original = f.record();
        let limit = f
            .config
            .acquisition
            .as_ref()
            .unwrap()
            .policy
            .quotas
            .model_calls;
        f.sql()
            .execute(
                "UPDATE acquisition_counters SET model_calls=?1 WHERE singleton=1",
                [limit],
            )
            .unwrap();
        f.app.automatic_acquisition = true;
        let paused = capability_acquisition_driver::drain_once(&f.app)
            .await
            .unwrap();
        assert_eq!(
            paused,
            DrainSummary {
                serviced: 1,
                advanced: 0,
                attention: 1,
                stale: 0
            }
        );
        let status = capability_acquisition_driver::status(&f.app, original.id()).unwrap();
        assert_eq!(status.state, AutomationState::Attention);
        assert_eq!(status.error_code, Some(DriverErrorCode::Quota));
        assert_eq!(f.record().version(), original.version());
        (f, original)
    }

    fn record(&self) -> AcquisitionRecord {
        let ids = self.app.store.acquisition_ids().unwrap();
        assert_eq!(ids.len(), 1);
        self.app.acquisition_runtime.status(&ids[0]).unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.config.db.as_ref().unwrap()).unwrap()
    }
    fn make_reconcile_due(&self, id: &str) {
        assert_eq!(
            self.sql()
                .execute(
                    "UPDATE acquisition_automation SET next_check_at=?1 WHERE acquisition_id=?2",
                    [PAST, id],
                )
                .unwrap(),
            1
        );
    }
    fn saved_schedule(&self, id: &str) -> (i64, Option<String>, Option<String>) {
        self.sql()
            .query_row(
                "SELECT observed_version,error_code,next_check_at FROM acquisition_automation WHERE acquisition_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }
    async fn change(&self, edit: impl FnOnce(&mut OrgState, &CapabilityGap)) {
        let _guard = self.app.kernel.lock().await;
        let old = self.app.current_state().unwrap();
        let gap = old.capability_gaps[&self.gap_id].clone();
        let mut next = old.clone();
        edit(&mut next, &gap);
        next.revision += 1;
        self.app
            .store
            .commit_state(
                old.revision,
                &next,
                vec![],
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
    }
}

#[tokio::test]
async fn unchanged_local_reconcile_keeps_the_paused_attention_code() {
    let providers = CountingProvider::start().await;
    let (f, original) = Fixture::paused_on_quota("review-reconcile-unchanged", &providers).await;
    let counts = f.app.store.acquisition_counters().unwrap();
    f.make_reconcile_due(original.id());

    // Due paused work is serviced with a local-only reconcile. Nothing about
    // the acquisition changes, so the pause and its code must be retained.
    let summary = capability_acquisition_driver::drain_once(&f.app)
        .await
        .unwrap();
    assert_eq!(
        summary,
        DrainSummary {
            serviced: 1,
            advanced: 0,
            attention: 0,
            stale: 0
        }
    );
    assert_eq!(f.record().version(), original.version());
    let status = capability_acquisition_driver::status(&f.app, original.id()).unwrap();
    assert_eq!(status.state, AutomationState::Attention);
    assert_eq!(status.error_code, Some(DriverErrorCode::Quota));
    assert_eq!(status.observed_version, Some(original.version()));
    assert!(status.next_attempt_at.is_none());
    assert!(status.next_reconcile_at.is_some_and(|due| due > Utc::now()));
    let (observed, code, next) = f.saved_schedule(original.id());
    assert_eq!(observed, original.version() as i64);
    assert_eq!(code.as_deref(), Some("quota"));
    assert!(next.is_some_and(|due| due.as_str() > PAST));

    // The retained pause keeps the next pass local as well: still no effect.
    f.make_reconcile_due(original.id());
    capability_acquisition_driver::drain_once(&f.app)
        .await
        .unwrap();
    let status = capability_acquisition_driver::status(&f.app, original.id()).unwrap();
    assert_eq!(status.error_code, Some(DriverErrorCode::Quota));
    assert_eq!(f.app.store.acquisition_counters().unwrap(), counts);
    assert_eq!(providers.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_reconcile_that_advances_the_record_does_not_carry_the_stale_code() {
    let providers = CountingProvider::start().await;
    let (f, original) = Fixture::paused_on_quota("review-reconcile-advanced", &providers).await;
    let counts = f.app.store.acquisition_counters().unwrap();
    // Revoke the captured grant. The local reconcile fences the acquisition,
    // which saves a new checkpoint version without any provider effect.
    f.change(|state, gap| state.roles.get_mut(&gap.role_id).unwrap().state = RoleState::Draining)
        .await;
    f.make_reconcile_due(original.id());

    let summary = capability_acquisition_driver::drain_once(&f.app)
        .await
        .unwrap();
    assert_eq!(
        summary,
        DrainSummary {
            serviced: 1,
            advanced: 1,
            attention: 0,
            stale: 0
        }
    );
    let fenced = f.record();
    assert!(fenced.is_fenced());
    assert_eq!(fenced.version(), original.version() + 1);

    // The quota code described the old version. The schedule saved against the
    // new version must be clean. Status alone cannot show this (an inactive
    // acquisition never reports a code), so read the persisted schedule too.
    let status = capability_acquisition_driver::status(&f.app, original.id()).unwrap();
    assert_eq!(status.state, AutomationState::Inactive);
    assert_eq!(status.observed_version, Some(fenced.version()));
    assert_eq!(status.current_version, fenced.version());
    assert_eq!(status.error_code, None);
    let (observed, code, next) = f.saved_schedule(original.id());
    assert_eq!(observed, fenced.version() as i64);
    assert_eq!(code, None, "stale quota code followed the new version");
    assert!(next.is_some());
    let catalog_attention: Option<String> = f
        .sql()
        .query_row(
            "SELECT attention_code FROM acquisition_catalog WHERE id=?1",
            [original.id()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(catalog_attention, None);
    assert_eq!(
        f.app.store.acquisition_counters().unwrap().model_calls,
        counts.model_calls
    );
    assert_eq!(providers.calls.load(Ordering::SeqCst), 0);
}
