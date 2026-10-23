//! # genesis-server
//!
//! The GENESIS organization kernel as an HTTP service.
//!
//! - One process per organization (`genesis org`), each with its own SQLite
//!   store, tokens and event log.
//! - One shared capability registry (`genesis registry`).
//! - `genesis dev` runs a registry plus Org A and Org B locally.

pub mod app;
pub mod auth;
pub mod error;
pub mod outbox;
pub mod registry;
pub mod routes;
pub mod saga;
pub mod store;

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

#[derive(Debug, Clone)]
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
    Ok(AppState {
        org_id: cfg.org_id.clone(),
        name: cfg.name.clone(),
        store,
        providers,
        tokens: cfg.tokens.clone(),
        events: tx,
        kernel: Arc::new(Mutex::new(())),
        registry,
        provision_timeout: cfg.provision_timeout,
    })
}

/// Serve an organization on `listener`, with its outbox worker and saga
/// recovery. Returns when the server stops.
pub async fn serve_org(app: AppState, listener: TcpListener) -> anyhow::Result<()> {
    let resumed = saga::resume_pending(&app).await.unwrap_or(0);
    if resumed > 0 {
        tracing::info!(resumed, "resumed interrupted organization changes");
    }
    let worker = outbox::spawn(app.clone());
    let router = app.router();
    let res = axum::serve(listener, router).await;
    worker.abort();
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
