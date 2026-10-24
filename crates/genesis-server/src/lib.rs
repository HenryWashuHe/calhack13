//! # genesis-server
//!
//! The GENESIS organization kernel as an HTTP service.
//!
//! - One process per organization (`genesis org`), each with its own SQLite
//!   store, tokens and event log.
//! - One shared capability registry (`genesis registry`).
//! - `genesis dev` runs a registry plus Org A and Org B locally.

pub mod app;
pub mod audit;
pub mod auth;
pub mod capability_acquisition_bootstrap;
pub mod capability_acquisition_driver;
pub mod capability_acquisition_history;
pub mod capability_acquisition_runtime;
pub mod capability_acquisition_store;
pub mod capability_adoption;
pub mod capability_adoption_candidates;
pub mod capability_adoption_outcome;
pub mod capability_adoption_store;
pub mod capability_authority;
pub mod capability_evidence;
pub mod capability_finalization;
pub mod capability_gap_adoption;
pub mod capability_gap_driver;
pub mod capability_gap_finalizer;
pub mod capability_gap_policy;
pub mod capability_gap_store;
pub mod capability_gaps;
pub mod capability_invocation;
pub mod capability_runtime;
pub mod capability_scenario;
pub mod capability_store;
pub mod elastic_jobs;
pub mod error;
pub mod method_adoption;
pub mod outbox;
pub mod projection;
pub mod registry;
pub mod routes;
pub mod saga;
pub mod specialist;
pub mod specialist_jobs;
pub mod store;
pub mod venue_assessment_domain;
pub mod venue_assessments;
pub mod venue_monitors;
pub mod venue_recovery;
pub mod venue_recovery_domain;
pub mod venue_recovery_intake;
mod venue_recovery_sources;

/// Optional CREAO specialist registration from the backend environment.
pub mod creao_config;

/// Durable CREAO dispatch for gateway readiness challenges.
pub mod creao_readiness;

/// Readiness proven by backend-observed gateway receipts.
pub mod gateway_readiness;

pub mod method_development_intake;

use app::AppState;
use auth::Tokens;
use genesis_core::ids::OrgId;
use genesis_core::model::{Constitution, OrgState};
use genesis_providers::{DevStub, ProviderSet};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use store::Store;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};

/// Trusted backend acquisition configuration. Clients cannot be supplied by an
/// HTTP caller, and Debug never traverses credentials held by a provider client.
#[derive(Clone)]
pub struct AcquisitionConfig {
    pub policy: capability_acquisition_store::AcquisitionPolicy,
    /// Explicit trusted opt-in. Drives existing and new authorized intents.
    pub automatic: bool,
    pub model: Option<Arc<genesis_forge::AnthropicGenerator>>,
    pub apify: Option<Arc<genesis_apify::ApifyClient>>,
}

impl AcquisitionConfig {
    pub fn offline(policy: capability_acquisition_store::AcquisitionPolicy) -> Self {
        Self {
            policy,
            automatic: false,
            model: None,
            apify: None,
        }
    }
}

impl std::fmt::Debug for AcquisitionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcquisitionConfig")
            .field("policy", &self.policy)
            .field("automatic", &self.automatic)
            .field("model_configured", &self.model.is_some())
            .field("apify_configured", &self.apify.is_some())
            .finish()
    }
}

#[derive(Clone)]
pub struct OrgConfig {
    pub org_id: OrgId,
    pub name: String,
    /// SQLite path, or `None` for an in-memory store (tests).
    pub db: Option<PathBuf>,
    pub constitution: Constitution,
    pub tokens: Tokens,
    pub registry_url: Option<String>,
    pub registry_token: Option<String>,
    /// Fall back to the labeled development stub for unconfigured providers.
    pub dev_stub: bool,
    pub stub_delay: Duration,
    pub provision_timeout: Duration,
    /// Backend-owned oracle answers; never accepted from an HTTP client.
    pub oracle_suites: Vec<genesis_verification::OracleSuite>,
    /// Trusted local forge checkpoints, reverified on startup.
    pub forge_checkpoints: Vec<genesis_forge::ForgeSession>,
    /// Opt-in server-managed acquisition; provider credentials stay in clients.
    pub acquisition: Option<AcquisitionConfig>,
    /// Explicit trusted opt-in to creating acquisition intents for committed gaps.
    /// Includes public synthesis examples; never print its contents in Debug.
    pub gap_acquisition_policy: Option<capability_gap_policy::GapAcquisitionPolicy>,
    /// Explicit trusted opt-in to publishing, installing and locally accepting
    /// verified gap acquisitions (`acquired` → `installed`). Off by default.
    pub gap_finalization_policy: Option<capability_gap_finalizer::GapFinalizationPolicy>,
    /// Explicit trusted opt-in to adopting an existing foreign publication for
    /// a committed gap before any synthesis (`open` → `installed`). Off by default.
    pub gap_adoption_policy: Option<capability_gap_adoption::GapAdoptionPolicy>,
}

impl std::fmt::Debug for OrgConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrgConfig")
            .field("org_id", &self.org_id)
            .field("name", &self.name)
            .field("db", &self.db)
            .field("dev_stub", &self.dev_stub)
            .field("oracle_suite_count", &self.oracle_suites.len())
            .field("forge_checkpoint_count", &self.forge_checkpoints.len())
            .field("acquisition", &self.acquisition)
            .field(
                "gap_acquisition_configured",
                &self.gap_acquisition_policy.is_some(),
            )
            .field(
                "gap_finalization_configured",
                &self.gap_finalization_policy.is_some(),
            )
            .field(
                "gap_adoption_configured",
                &self.gap_adoption_policy.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl OrgConfig {
    pub fn dev(org_id: &str, name: &str) -> Self {
        Self {
            org_id: OrgId::new(org_id),
            name: name.to_owned(),
            db: None,
            constitution: demo_constitution(),
            tokens: Tokens {
                operator: format!("{org_id}-operator"),
                agent: format!("{org_id}-agent"),
                adapter: format!("{org_id}-adapter"),
            },
            registry_url: None,
            registry_token: None,
            dev_stub: true,
            stub_delay: Duration::ZERO,
            provision_timeout: Duration::from_secs(60),
            oracle_suites: Vec::new(),
            forge_checkpoints: Vec::new(),
            acquisition: None,
            gap_acquisition_policy: None,
            gap_finalization_policy: None,
            gap_adoption_policy: None,
        }
    }
}

/// Scopes for the event-organization demo. Real deployments load a
/// constitution file instead (`--constitution`).
pub fn demo_constitution() -> Constitution {
    Constitution {
        scopes: [
            "mission.coordinate",
            "program.schedule",
            "attendees.inform",
            "ledger.reserve_venue",
            "contract.logistics",
            "capability.venue_lookup",
            "capability.acquire",
            "monitor.venue",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        budget_cents: 1_000_000,
        max_active_roles: 10,
        max_active_contracts: 3,
    }
}

/// Open the store, initialize revision 0 if needed, and build the app state.
pub fn build_org(cfg: &OrgConfig, providers: Option<ProviderSet>) -> anyhow::Result<AppState> {
    if let Some(policy) = &cfg.gap_adoption_policy {
        policy.validate()?;
        anyhow::ensure!(
            cfg.registry_url.is_some(),
            "gap adoption requires a capability registry"
        );
        for rule in &policy.rules {
            anyhow::ensure!(
                cfg.oracle_suites
                    .iter()
                    .any(|s| s.suite_id == rule.suite_id),
                "gap adoption rule {} names an unknown suite",
                rule.capability_id
            );
        }
    }
    if let Some(policy) = &cfg.gap_finalization_policy {
        policy.validate()?;
        anyhow::ensure!(
            cfg.gap_acquisition_policy.is_some() && cfg.registry_url.is_some(),
            "gap finalization requires gap acquisition and a capability registry"
        );
    }
    if let Some(policy) = &cfg.gap_acquisition_policy {
        policy.validate()?;
        anyhow::ensure!(
            cfg.acquisition
                .as_ref()
                .is_some_and(|config| config.automatic),
            "gap acquisition requires automatic acquisition and a trusted acquisition policy"
        );
    }
    if cfg.acquisition.as_ref().is_some_and(|config| {
        config.automatic && (config.model.is_none() || config.apify.is_none())
    }) {
        anyhow::bail!("automatic acquisition requires configured model and Apify clients");
    }
    let store = match &cfg.db {
        Some(path) => Store::open(path, cfg.org_id.clone())?,
        None => Store::open_in_memory(cfg.org_id.clone())?,
    };
    let initial = OrgState::new(
        cfg.org_id.clone(),
        cfg.name.clone(),
        cfg.constitution.clone(),
    );
    let (state, _) = store.init(&initial)?;
    if state.org_id != cfg.org_id {
        anyhow::bail!(
            "database belongs to {}, not {}; use a separate database per organization",
            state.org_id,
            cfg.org_id
        );
    }
    let providers = providers.unwrap_or_else(|| {
        let p = ProviderSet::new();
        if cfg.dev_stub {
            p.with_dev_stub(DevStub::with_delay(cfg.stub_delay))
        } else {
            p
        }
    });
    let registry = match (&cfg.registry_url, &cfg.registry_token) {
        (Some(url), Some(token)) => Some(registry::RegistryClient::new(url.clone(), token.clone())),
        _ => None,
    };
    let (tx, _) = broadcast::channel(1024);
    let kernel = Arc::new(Mutex::new(()));
    store.init_capability_runs()?;
    store.init_method_development()?;
    store.init_capability_pins()?;
    store.init_capability_adoptions()?;
    store.init_capability_invocations()?;
    store.init_venue_assessments()?;
    store.init_venue_monitors()?;
    store.init_venue_recoveries()?;
    store.init_venue_recovery_intake()?;
    store.init_capability_finalization()?;
    store.init_gap_acquisitions()?;
    store.init_gap_driver()?;
    store.init_gap_finalizer()?;
    store.init_gap_adoption()?;
    if let Some(config) = &cfg.acquisition {
        store.init_acquisitions(&config.policy)?;
        store.init_acquisition_driver()?;
    }
    let evidence_policy = Arc::new(capability_evidence::EvidencePolicy::new(
        cfg.oracle_suites.clone(),
        cfg.forge_checkpoints.clone(),
    )?);
    // Read-only acquisition discovery survives disabled provider clients. Legacy
    // records require trusted suite validation before a baseline can be published.
    store.init_acquisition_history()?;
    if cfg.acquisition.is_some() {
        capability_acquisition_bootstrap::bootstrap(&store, &evidence_policy)?;
    }
    let capability_runtime = Arc::new(
        capability_runtime::CapabilityRuntime::new(
            store.clone(),
            providers.clone(),
            kernel.clone(),
            Arc::new(Mutex::new(())),
        )
        .with_events(tx.clone())
        .with_adoption_policy(evidence_policy.clone()),
    );
    let acquisition_runtime = Arc::new(capability_acquisition_runtime::AcquisitionRuntime::new(
        store.clone(),
        evidence_policy.clone(),
        kernel.clone(),
        Arc::new(Mutex::new(())),
        cfg.acquisition.as_ref().and_then(|c| c.model.clone()),
        cfg.acquisition.as_ref().and_then(|c| c.apify.clone()),
    ));
    Ok(AppState {
        saga_gates: Default::default(),
        org_id: cfg.org_id.clone(),
        name: cfg.name.clone(),
        store,
        providers,
        tokens: cfg.tokens.clone(),
        events: tx,
        kernel,
        evidence_policy,
        capability_runtime,
        acquisition_policy: cfg.acquisition.as_ref().map(|c| Arc::new(c.policy.clone())),
        automatic_acquisition: cfg.acquisition.as_ref().is_some_and(|c| c.automatic),
        gap_acquisition_policy: cfg.gap_acquisition_policy.clone().map(Arc::new),
        gap_finalization_policy: cfg.gap_finalization_policy.clone().map(Arc::new),
        gap_adoption_policy: cfg.gap_adoption_policy.clone().map(Arc::new),
        acquisition_runtime,
        adoption_gate: Arc::new(Mutex::new(())),
        registry,
        projection: projection::Projection::disabled(),
        elastic_agents: None,
        fetch_recovery: None,
        automatic_venue_recovery: false,
        provision_timeout: cfg.provision_timeout,
        readiness: specialist::ReadinessRegistry::default(),
    })
}

/// Enable Elastic integrations configured in the environment:
/// projection/retrieval when `ELASTICSEARCH_URL` and `ELASTIC_DATA_API_KEY`
/// are set, and Agent Builder specialists for `provider: elastic` roles when
/// `ELASTIC_KIBANA_URL` and `ELASTIC_MANAGEMENT_API_KEY` are set. Otherwise
/// returns `app` unchanged.
pub fn attach_elastic_from_env(mut app: AppState) -> anyhow::Result<AppState> {
    if let Some(cfg) = genesis_elastic::AgentBuilderConfig::from_env() {
        let builder = genesis_elastic::AgentBuilder::new(cfg)
            .map_err(|e| anyhow::anyhow!("agent builder configuration: {e}"))?;
        tracing::info!(org = %app.org_id, "elastic agent builder provisioning enabled");
        app = app.with_elastic_agents(builder);
    }
    if let Some(cfg) = genesis_elastic::ElasticConfig::from_env() {
        let client = genesis_elastic::ElasticClient::new(cfg)?;
        tracing::info!(org = %app.org_id, index = %client.evidence_index(app.org_id.as_str()), "elastic projection enabled");
        app = app.with_elastic(client);
    }
    Ok(app)
}

/// Serve an organization on `listener`, with its outbox worker and saga
/// recovery. Returns when the server stops.
pub async fn serve_org(app: AppState, listener: TcpListener) -> anyhow::Result<()> {
    // Registered here because CREAO startup recovery is asynchronous.
    let app = creao_config::attach_creao_from_env(app).await?;
    let app = venue_recovery::attach_from_env(app)?;
    let app = venue_recovery_intake::attach_from_env(app)?;
    let resumed = saga::resume_pending(&app).await.unwrap_or(0);
    if resumed > 0 {
        tracing::info!(resumed, "resumed interrupted organization changes");
    }
    app.capability_runtime.recover_interrupted().await?;
    capability_finalization::recover(&app).await?;
    if app.acquisition_policy.is_some() {
        app.acquisition_runtime.recover_interrupted().await?;
    }
    let _capability_worker = AbortWorker(capability_worker(app.clone()));
    let _finalization_worker = AbortWorker(capability_finalization::spawn(app.clone()));
    let _venue_assessment_worker = AbortWorker(venue_assessments::spawn(app.clone()));
    let _venue_monitor_worker = AbortWorker(venue_monitors::spawn(app.clone()));
    let _venue_recovery_worker = AbortWorker(venue_recovery::spawn(app.clone()));
    let _venue_recovery_intake = AbortWorker(venue_recovery_intake::spawn(app.clone()));
    let _adoption_worker = AbortWorker(capability_adoption::spawn(app.clone()));
    let _acquisition_worker = app
        .automatic_acquisition
        .then(|| AbortWorker(capability_acquisition_driver::spawn(app.clone())));
    let _gap_worker = app
        .gap_acquisition_policy
        .is_some()
        .then(|| AbortWorker(capability_gap_driver::spawn(app.clone())));
    let _gap_finalizer = app
        .gap_finalization_policy
        .is_some()
        .then(|| AbortWorker(capability_gap_finalizer::spawn(app.clone())));
    // Always runs: frozen adopt-first intents finish even if the policy is
    // later removed (the driver keeps deferring their gaps until they do).
    let _gap_adoption = AbortWorker(capability_gap_adoption::spawn(app.clone()));
    let _specialist_worker =
        specialist::spawn(app.clone(), Duration::from_millis(500)).map(AbortWorker);
    let _outbox_worker = AbortWorker(outbox::spawn(app.clone()));
    let _saga_sweeper = AbortWorker(saga::spawn_sweeper(app.clone(), Duration::from_secs(5)));
    let _projector = projection::spawn(app.clone()).map(AbortWorker);
    let router = app.router();
    let res = axum::serve(listener, router).await;
    res.map_err(Into::into)
}

/// Spawn an organization server in the background (used by `dev` and tests).
pub async fn spawn_org(app: AppState, addr: &str) -> anyhow::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(e) = serve_org(app, listener).await {
            tracing::error!(error = %e, "organization server stopped");
        }
    });
    Ok(local)
}

/// Spawn the shared registry in the background.
pub async fn spawn_registry(
    state: registry::RegistryState,
    addr: &str,
) -> anyhow::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let router = registry::router(state).layer(
        tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any),
    );
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!(error = %e, "registry stopped");
        }
    });
    Ok(local)
}

/// Parse `org_a:token,org_b:token` into a publisher map.
pub fn parse_publishers(spec: &str) -> HashMap<String, OrgId> {
    spec.split(',')
        .filter_map(|pair| {
            let (org, token) = pair.split_once(':')?;
            Some((token.trim().to_owned(), OrgId::new(org.trim())))
        })
        .collect()
}

/// One bounded provider operation at a time for this organization's demo process.
/// Failed reads require explicit retry_read; ambiguous submissions never replay.
fn capability_worker(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            interval.tick().await;
            let records = match app.store.recoverable_capability_runs() {
                Ok(records) => records,
                Err(error) => {
                    tracing::error!(%error, "capability recovery scan failed");
                    break;
                }
            };
            for record in records {
                if record.failure.is_none()
                    && matches!(
                        record.phase,
                        capability_runtime::CapabilityRunPhase::Prepared
                            | capability_runtime::CapabilityRunPhase::Running
                    )
                {
                    if let Err(error) = app.capability_runtime.step(&record.job_id, false).await {
                        tracing::warn!(job_id=%record.job_id, %error, "capability step requires attention");
                    }
                }
            }
        }
    })
}

/// Cancellation of the server future must cancel workers as well. Dropping a
/// bare JoinHandle detaches it and could let external effects outlive the server.
struct AbortWorker(tokio::task::JoinHandle<()>);
impl Drop for AbortWorker {
    fn drop(&mut self) {
        self.0.abort();
    }
}
