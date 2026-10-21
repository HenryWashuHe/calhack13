//! Shared application state and the organization router.

use crate::auth::Tokens;
use crate::error::{ApiError, ApiResult};
use crate::registry::RegistryClient;
use crate::store::{EventMeta, NewOutbox, Store, StoreResult};
use axum::Router;
use genesis_core::engine::Outcome;
use genesis_core::events::DomainEvent;
use genesis_core::ids::OrgId;
use genesis_core::model::OrgState;
use genesis_providers::ProviderSet;
use rusqlite::Transaction;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub org_id: OrgId,
    pub name: String,
    pub store: Store,
    pub providers: ProviderSet,
    pub tokens: Tokens,
    pub events: broadcast::Sender<DomainEvent>,
    /// Serializes read-modify-write cycles on the organization state.
    pub kernel: Arc<Mutex<()>>,
    pub registry: Option<RegistryClient>,
    pub provision_timeout: Duration,
    pub evidence_policy: Arc<crate::capability_evidence::EvidencePolicy>,
    pub capability_runtime: Arc<crate::capability_runtime::CapabilityRuntime>,
}

/// Result of a kernel mutation.
pub struct Mutation<T> {
    pub state: OrgState,
    pub events: Vec<DomainEvent>,
    pub value: T,
}

impl AppState {
    pub fn broadcast(&self, events: &[DomainEvent]) {
        for e in events {
            // No subscribers is fine.
            let _ = self.events.send(e.clone());
        }
    }

    pub fn current_state(&self) -> ApiResult<OrgState> {
        Ok(self.store.current_state()?)
    }

    /// Run a pure engine step under the kernel lock and persist its outcome.
    pub async fn mutate<T, F>(&self, meta: EventMeta, f: F) -> ApiResult<Mutation<T>>
    where
        F: FnOnce(&OrgState) -> Result<(Outcome, T), ApiError>,
    {
        self.mutate_with(
            meta,
            |s| f(s).map(|(o, v)| (o, v, Vec::new())),
            |_, _| Ok(()),
        )
        .await
    }

    /// Like [`mutate`](Self::mutate), with outbox items and extra writes that
    /// commit in the same transaction.
    pub async fn mutate_with<T, F, X>(
        &self,
        meta: EventMeta,
        f: F,
        extra: X,
    ) -> ApiResult<Mutation<T>>
    where
        F: FnOnce(&OrgState) -> Result<(Outcome, T, Vec<NewOutbox>), ApiError>,
        X: FnOnce(&Transaction<'_>, &T) -> StoreResult<()>,
    {
        let _guard = self.kernel.lock().await;
        let state = self.store.current_state()?;
        let (outcome, value, outbox) = f(&state)?;
        let events = self.store.commit_state(
            state.revision,
            &outcome.state,
            outcome.effects,
            &meta,
            outbox,
            |tx| extra(tx, &value),
        )?;
        self.broadcast(&events);
        Ok(Mutation {
            state: outcome.state,
            events,
            value,
        })
    }

    pub fn router(self) -> Router {
        crate::routes::router(self)
    }
}
