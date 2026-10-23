//! Shared application state and the organization router.

use crate::auth::Tokens;
use crate::error::{ApiError, ApiResult};
use crate::registry::RegistryClient;
use crate::store::{EventMeta, NewOutbox, Store, StoreResult};
use axum::Router;
use genesis_core::engine::Outcome;
use genesis_core::events::DomainEvent;
use genesis_core::ids::{ChangeId, OrgId};
use genesis_core::model::OrgState;
use genesis_providers::ProviderSet;
use rusqlite::Transaction;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, OwnedMutexGuard};

/// Serializes drivers of the same change within one AppState and its clones.
/// This is not a cross-process/database lease. A provider still must reconcile
/// a cancelled or ambiguous creation request before retrying native effects.
#[derive(Clone, Default)]
pub(crate) struct SagaGates {
    gates: Arc<StdMutex<HashMap<ChangeId, Weak<Mutex<()>>>>>,
    /// Changes with a live background driver (running or backing off).
    drivers: Arc<StdMutex<std::collections::HashSet<ChangeId>>>,
}

/// Held by a background driver; releases its claim when dropped.
pub(crate) struct DriverClaim {
    drivers: Arc<StdMutex<std::collections::HashSet<ChangeId>>>,
    change_id: ChangeId,
}

impl Drop for DriverClaim {
    fn drop(&mut self) {
        if let Ok(mut d) = self.drivers.lock() {
            d.remove(&self.change_id);
        }
    }
}

impl SagaGates {
    /// Claim the single background driver slot for a change, or `None` if
    /// another driver already holds it.
    pub(crate) fn claim_driver(&self, change_id: &ChangeId) -> Option<DriverClaim> {
        let mut d = self.drivers.lock().ok()?;
        d.insert(change_id.clone()).then(|| DriverClaim {
            drivers: self.drivers.clone(),
            change_id: change_id.clone(),
        })
    }

    pub(crate) async fn acquire(&self, change_id: &ChangeId) -> ApiResult<OwnedMutexGuard<()>> {
        let gate = {
            let mut gates = self
                .gates
                .lock()
                .map_err(|_| ApiError::internal("change execution gate unavailable"))?;
            // Waiters retain a strong Arc before awaiting, so a live gate is
            // never removed/replaced. Completed IDs do not accumulate forever.
            gates.retain(|_, gate| gate.strong_count() > 0);
            match gates.get(change_id).and_then(Weak::upgrade) {
                Some(gate) => gate,
                None => {
                    let gate = Arc::new(Mutex::new(()));
                    gates.insert(change_id.clone(), Arc::downgrade(&gate));
                    gate
                }
            }
        };
        // Never await while holding the registry's synchronous mutex.
        Ok(gate.lock_owned().await)
    }
}

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
    pub(crate) saga_gates: SagaGates,
    pub registry: Option<RegistryClient>,
    /// Elastic projection and retrieval; disabled unless configured.
    pub projection: crate::projection::Projection,
    /// Agent Builder client used to deliver jobs to Elastic specialists.
    pub elastic_agents: Option<Arc<genesis_elastic::AgentBuilder>>,
    pub provision_timeout: Duration,
    /// Readiness verifiers for asynchronously created agents, by provider.
    pub readiness: crate::specialist::ReadinessRegistry,
    pub evidence_policy: Arc<crate::capability_evidence::EvidencePolicy>,
    pub capability_runtime: Arc<crate::capability_runtime::CapabilityRuntime>,
    pub acquisition_policy: Option<Arc<crate::capability_acquisition_store::AcquisitionPolicy>>,
    pub automatic_acquisition: bool,
    pub gap_acquisition_policy: Option<Arc<crate::capability_gap_policy::GapAcquisitionPolicy>>,
    pub gap_finalization_policy:
        Option<Arc<crate::capability_gap_finalizer::GapFinalizationPolicy>>,
    pub gap_adoption_policy: Option<Arc<crate::capability_gap_adoption::GapAdoptionPolicy>>,
    pub acquisition_runtime: Arc<crate::capability_acquisition_runtime::AcquisitionRuntime>,
    pub(crate) adoption_gate: Arc<Mutex<()>>,
}

/// Result of a kernel mutation.
pub struct Mutation<T> {
    pub state: OrgState,
    pub events: Vec<DomainEvent>,
    pub value: T,
}

impl AppState {
    /// Provision Elastic roles as Agent Builder specialists and deliver their
    /// jobs as converse turns.
    pub fn with_elastic_agents(mut self, agents: genesis_elastic::AgentBuilder) -> Self {
        let agents = Arc::new(agents);
        self.providers = self
            .providers
            .clone()
            .with_provisioner(genesis_core::model::Provider::Elastic, agents.clone());
        self.elastic_agents = Some(agents);
        self
    }

    /// Enable Elastic projection and retrieval for this organization.
    pub fn with_elastic(mut self, client: genesis_elastic::ElasticClient) -> Self {
        self.projection = crate::projection::Projection::new(client);
        self
    }

    pub fn broadcast(&self, events: &[DomainEvent]) {
        for e in events {
            // No subscribers is fine.
            let _ = self.events.send(e.clone());
        }
    }

    pub fn current_state(&self) -> ApiResult<OrgState> {
        Ok(self.store.current_state()?)
    }

    /// Register the readiness check for agents created asynchronously by
    /// `provider` (see [`crate::specialist`]).
    pub fn with_readiness_verifier(
        mut self,
        provider: genesis_core::model::Provider,
        verifier: Arc<dyn crate::specialist::ReadinessVerifier>,
    ) -> Self {
        self.readiness = self.readiness.clone().with(provider, verifier);
        self
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
