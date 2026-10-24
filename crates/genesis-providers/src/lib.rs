//! # genesis-providers
//!
//! Provider adapter interfaces, a pinned Apify executor, and a development stub.
//!
//! Other adapters (Elastic Agent Builder, Fetch bridge, CREAO) plug in by
//! implementing these traits. The kernel never writes ownership from an
//! adapter: adapters return bindings and
//! receipts; the kernel decides.
//!
//! [`DevStub`] stands in for a missing provider during development. Every
//! binding it returns uses `Provider::DevStub` and a `devstub-` identifier so
//! the UI can label it. It must never be presented as a live integration.

use async_trait::async_trait;
use genesis_apify::{RunRequest, RunTicket};
use genesis_core::engine::RoleToProvision;
use genesis_core::ids::{ChangeId, OrgId};
use genesis_core::model::{AgentBinding, Provider};
use genesis_verification::{json_sha256, ExecutionEvidence};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

pub mod apify;
pub mod creao;
pub use apify::ApifyCapabilityExecutor;
pub use creao::{
    CreaoSpecialist, SpecialistExecutionReceipt, SpecialistProgress, SpecialistRuntime,
};
pub use genesis_creao::PreEffectGuard;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ProviderError {
    #[error("provider {provider:?} is not configured")]
    NotConfigured { provider: Provider },
    #[error("provider returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("provider request timed out")]
    Timeout,
    #[error("provider rejected the request: {0}")]
    Rejected(String),
    /// The provider positively reported that this exact run reached a
    /// terminal failure (for Apify: FAILED, TIMED-OUT or ABORTED after the
    /// run's identity and build were checked). Unlike `Rejected`, the
    /// external effect is known to have stopped. Only a provider adapter that
    /// read that terminal status may produce it.
    #[error("provider run reached a terminal failure: {0}")]
    RunFailed(String),
    /// The outcome is unknown (e.g. a dropped connection after submit).
    /// Callers must reconcile before retrying.
    #[error("outcome unknown: {0}")]
    Unknown(String),
}

/// What the kernel asks a provider to create for a new role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionRequest {
    pub org_id: OrgId,
    pub change_id: ChangeId,
    pub role: RoleToProvision,
}

impl ProvisionRequest {
    /// Digest of the configuration that will be sent to the provider.
    pub fn config_digest(&self) -> String {
        let cfg = serde_json::json!({
            "name": self.role.name,
            "charter": self.role.charter,
            "authority": self.role.authority,
            "provider": self.role.provider,
        });
        json_sha256(&cfg)
    }
}

/// Creates the provider-side implementation of a role.
#[async_trait]
pub trait RoleProvisioner: Send + Sync {
    fn provider(&self) -> Provider;

    /// Create the agent. Must be safe to reconcile: if the call times out the
    /// kernel may ask again with the same `change_id` and role key.
    async fn provision(&self, req: &ProvisionRequest) -> Result<AgentBinding, ProviderError>;

    /// Readiness check before activation (e.g. a CREAO bridge readiness run).
    async fn check_ready(&self, binding: &AgentBinding) -> Result<bool, ProviderError>;
}

/// Persist this submission receipt before polling. It does not verify output.
/// Deserialization is for trusted kernel storage, never a public proof of a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRunReceipt {
    pub provider: Provider,
    pub provider_refs: BTreeMap<String, String>,
    pub raw_status: String,
    #[serde(default)]
    pub ticket: Option<RunTicket>,
}

impl CapabilityRunReceipt {
    /// Check saved receipt consistency without making a provider request.
    /// This does not authenticate a receipt supplied by an untrusted caller.
    pub fn validate_for(&self, request: &RunRequest) -> Result<(), ProviderError> {
        let mismatch = || ProviderError::Rejected("capability run receipt mismatch".into());
        match self.provider {
            Provider::Apify => {
                let ticket = self.ticket.as_ref().ok_or_else(mismatch)?;
                ticket.validate_for(request).map_err(apify::map_error)?;
                for (key, expected) in [
                    ("actor_id", request.artifact.actor_id.as_str()),
                    ("build_id", request.artifact.build_id.as_str()),
                    ("build_number", request.artifact.build_number.as_str()),
                    ("run_id", ticket.run_id()),
                ] {
                    if self.provider_refs.get(key).map(String::as_str) != Some(expected) {
                        return Err(mismatch());
                    }
                }
                Ok(())
            }
            Provider::DevStub if self.ticket.is_none() => Ok(()),
            _ => Err(mismatch()),
        }
    }
}

/// Executes a pinned capability artifact (Apify Actor build).
#[async_trait]
pub trait CapabilityExecutor: Send + Sync {
    fn provider(&self) -> Provider;

    async fn submit(&self, request: &RunRequest) -> Result<CapabilityRunReceipt, ProviderError>;

    /// Resume a previously persisted run using only GET requests. The immutable
    /// backend request and ticket must refer to the same artifact and input.
    /// Evidence still requires independent domain verification.
    async fn resume(
        &self,
        request: &RunRequest,
        ticket: &RunTicket,
    ) -> Result<ExecutionEvidence, ProviderError>;
}

/// Development stand-in for any provider.
#[derive(Debug, Clone, Default)]
pub struct DevStub {
    /// Artificial latency so the UI shows the PROVISIONING state.
    pub delay: Duration,
}

impl DevStub {
    pub fn with_delay(delay: Duration) -> Self {
        Self { delay }
    }
}

#[async_trait]
impl RoleProvisioner for DevStub {
    fn provider(&self) -> Provider {
        Provider::DevStub
    }

    async fn provision(&self, req: &ProvisionRequest) -> Result<AgentBinding, ProviderError> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        Ok(AgentBinding {
            provider: Provider::DevStub,
            provider_agent_id: format!(
                "devstub-{}-{}",
                format!("{:?}", req.role.provider).to_lowercase(),
                uuid::Uuid::new_v4().simple()
            ),
            config_digest: req.config_digest(),
            conversation_ref: None,
        })
    }

    async fn check_ready(&self, _binding: &AgentBinding) -> Result<bool, ProviderError> {
        Ok(true)
    }
}

#[async_trait]
impl CapabilityExecutor for DevStub {
    fn provider(&self) -> Provider {
        Provider::DevStub
    }

    async fn submit(&self, request: &RunRequest) -> Result<CapabilityRunReceipt, ProviderError> {
        let mut refs = BTreeMap::new();
        refs.insert("actor_id".into(), request.artifact.actor_id.clone());
        refs.insert("build_id".into(), request.artifact.build_id.clone());
        refs.insert("build_number".into(), request.artifact.build_number.clone());
        refs.insert(
            "run_id".into(),
            format!("devstub-run-{}", uuid::Uuid::new_v4().simple()),
        );
        Ok(CapabilityRunReceipt {
            provider: Provider::DevStub,
            provider_refs: refs,
            raw_status: "DEVSTUB_SUBMITTED".into(),
            ticket: None,
        })
    }

    async fn resume(
        &self,
        _request: &RunRequest,
        _ticket: &RunTicket,
    ) -> Result<ExecutionEvidence, ProviderError> {
        Err(ProviderError::NotConfigured {
            provider: Provider::DevStub,
        })
    }
}

/// A provisioner that always fails. Used to test partial-provisioning safety.
#[derive(Debug, Clone)]
pub struct FailingProvisioner {
    pub provider: Provider,
    pub error: ProviderError,
}

#[async_trait]
impl RoleProvisioner for FailingProvisioner {
    fn provider(&self) -> Provider {
        self.provider
    }
    async fn provision(&self, _req: &ProvisionRequest) -> Result<AgentBinding, ProviderError> {
        Err(self.error.clone())
    }
    async fn check_ready(&self, _binding: &AgentBinding) -> Result<bool, ProviderError> {
        Ok(false)
    }
}

/// Maps providers to adapters. Missing providers fall back to [`DevStub`] only
/// when `allow_dev_stub` is set.
#[derive(Clone, Default)]
pub struct ProviderSet {
    provisioners: HashMap<Provider, Arc<dyn RoleProvisioner>>,
    executor: Option<Arc<dyn CapabilityExecutor>>,
    specialists: HashMap<Provider, Arc<dyn SpecialistRuntime>>,
    dev_stub: Option<DevStub>,
}

impl ProviderSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable the development fallback for providers without an adapter.
    pub fn with_dev_stub(mut self, stub: DevStub) -> Self {
        self.dev_stub = Some(stub);
        self
    }

    pub fn with_provisioner(mut self, provider: Provider, p: Arc<dyn RoleProvisioner>) -> Self {
        self.provisioners.insert(provider, p);
        self
    }

    pub fn with_executor(mut self, e: Arc<dyn CapabilityExecutor>) -> Self {
        self.executor = Some(e);
        self
    }

    /// Split asynchronous specialists are distinct from ready role provisioners.
    /// There is deliberately no development fallback for this evidence path.
    pub fn with_specialist(mut self, specialist: Arc<dyn SpecialistRuntime>) -> Self {
        self.specialists.insert(specialist.provider(), specialist);
        self
    }

    pub fn specialist(
        &self,
        provider: Provider,
    ) -> Result<Arc<dyn SpecialistRuntime>, ProviderError> {
        self.specialists
            .get(&provider)
            .cloned()
            .ok_or(ProviderError::NotConfigured { provider })
    }

    /// Providers exposing durable asynchronous operations. These still require
    /// kernel orchestration/readiness; they are not synchronous provisioners.
    pub fn configured_specialists(&self) -> Vec<Provider> {
        let mut providers: Vec<_> = self.specialists.keys().copied().collect();
        providers.sort();
        providers
    }

    pub fn dev_stub_enabled(&self) -> bool {
        self.dev_stub.is_some()
    }

    pub fn provisioner(
        &self,
        provider: Provider,
    ) -> Result<Arc<dyn RoleProvisioner>, ProviderError> {
        if let Some(p) = self.provisioners.get(&provider) {
            return Ok(p.clone());
        }
        match &self.dev_stub {
            Some(stub) => Ok(Arc::new(stub.clone())),
            None => Err(ProviderError::NotConfigured { provider }),
        }
    }

    pub fn executor(&self) -> Result<Arc<dyn CapabilityExecutor>, ProviderError> {
        if let Some(e) = &self.executor {
            return Ok(e.clone());
        }
        match &self.dev_stub {
            Some(stub) => Ok(Arc::new(stub.clone())),
            None => Err(ProviderError::NotConfigured {
                provider: Provider::Apify,
            }),
        }
    }

    /// Registered provider identities, with development stubs explicitly labeled.
    pub fn configured(&self) -> Vec<Provider> {
        let mut v: Vec<_> = self.provisioners.keys().copied().collect();
        if let Some(executor) = &self.executor {
            v.push(executor.provider());
        }
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::ids::RoleId;
    use genesis_core::model::AuthorityGrant;

    fn req() -> ProvisionRequest {
        ProvisionRequest {
            org_id: OrgId::new("org_a"),
            change_id: ChangeId::new("chg_1"),
            role: RoleToProvision {
                key: "relocation".into(),
                role_id: RoleId::new("role_1"),
                name: "Relocation lead".into(),
                charter: "Relocate".into(),
                authority: AuthorityGrant::default(),
                provider: Provider::Elastic,
            },
        }
    }

    #[tokio::test]
    async fn dev_stub_bindings_are_labeled() {
        let b = DevStub::default().provision(&req()).await.unwrap();
        assert_eq!(b.provider, Provider::DevStub);
        assert!(b.provider_agent_id.starts_with("devstub-elastic-"));
        assert_eq!(b.config_digest, req().config_digest());
    }

    #[test]
    fn missing_provider_without_stub_is_not_configured() {
        let set = ProviderSet::new();
        assert!(matches!(
            set.provisioner(Provider::Creao),
            Err(ProviderError::NotConfigured {
                provider: Provider::Creao
            })
        ));
        let set = set.with_dev_stub(DevStub::default());
        assert_eq!(
            set.provisioner(Provider::Creao).unwrap().provider(),
            Provider::DevStub
        );
    }
}
