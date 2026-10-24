//! Asynchronous role provisioning (the "pending/readiness hook").
//!
//! Some providers (CREAO) create agents asynchronously through durable
//! operations ([`SpecialistRuntime`]). For such a role the saga:
//!
//! 1. prepares (or finds) the role's operation from its stable
//!    organization/change/role identity and records it on the change;
//! 2. advances it at most once per pass, outside the kernel lock, and persists
//!    the observed state. Pending and ambiguous operations stay visible on a
//!    `PROVISIONING` change instead of failing it; the organization (including
//!    existing responsibility ownership) is untouched until commit;
//! 3. on native completion, persists the created agent binding, then asks the
//!    provider's [`ReadinessVerifier`] for backend-observed readiness evidence;
//! 4. only after readiness passes lets the change commit the role.
//!
//! Without a registered verifier a created agent waits in
//! `awaiting_readiness`: provider completion or model text is never treated as
//! readiness. Created IDs are retained if readiness fails.

use crate::app::AppState;
use crate::error::ApiResult;
use async_trait::async_trait;
use chrono::Utc;
use genesis_core::change::{OrgChangeRecord, ProviderOperation, ProviderOperationState as S};
use genesis_core::engine::RoleToProvision;
use genesis_core::events::EventKind;
use genesis_core::lifecycle::ChangeStatus;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{ProviderError, ProvisionRequest, SpecialistProgress, SpecialistRuntime};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Result of a readiness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// Proven by a backend-observed receipt; `evidence_ref` points to it.
    Ready { evidence_ref: String },
    /// The check is still running; ask again later.
    Pending,
    /// The agent exists but is not usable.
    NotReady(String),
}

/// Proves that a created agent can serve its role, from evidence the backend
/// observes itself (e.g. a bridge/gateway receipt), never from model output.
#[async_trait]
pub trait ReadinessVerifier: Send + Sync {
    async fn verify(
        &self,
        role: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError>;
}

/// Readiness verifiers by provider.
#[derive(Clone, Default)]
pub struct ReadinessRegistry {
    verifiers: HashMap<Provider, Arc<dyn ReadinessVerifier>>,
}

impl ReadinessRegistry {
    pub fn with(mut self, provider: Provider, verifier: Arc<dyn ReadinessVerifier>) -> Self {
        self.verifiers.insert(provider, verifier);
        self
    }

    pub fn get(&self, provider: Provider) -> Option<Arc<dyn ReadinessVerifier>> {
        self.verifiers.get(&provider).cloned()
    }
}

/// Outcome of one provisioning pass for an asynchronous role.
pub(crate) enum Drive {
    /// Created and proven ready; the binding is on the record.
    Ready,
    /// Not finished; the change stays in `PROVISIONING`.
    Waiting,
    /// Terminal failure with a sanitized reason.
    Failed(String),
}

fn state_name(state: S) -> &'static str {
    match state {
        S::Pending => "pending",
        S::AwaitingReadiness => "awaiting_readiness",
        S::Ready => "ready",
        S::NeedsReconciliation => "needs_reconciliation",
        S::Failed => "failed",
    }
}

/// Record a new state for the role's operation and persist it. Emits a
/// progress event only when state, run or attention changed.
fn update(
    app: &AppState,
    record: &mut OrgChangeRecord,
    role_key: &str,
    f: impl FnOnce(&mut ProviderOperation),
    extra: Vec<EventKind>,
) -> ApiResult<()> {
    let op = record
        .operations
        .get_mut(role_key)
        .expect("operation recorded before update");
    let before = (op.state, op.run_id.clone(), op.attention.clone());
    f(op);
    let changed = before != (op.state, op.run_id.clone(), op.attention.clone());
    op.updated_at = Utc::now();
    let mut effects = extra;
    if changed {
        tracing::info!(change = %record.change_id, role = role_key, state = state_name(op.state), "provider operation progress");
        effects.insert(
            0,
            EventKind::ProvisioningProgress {
                change_id: record.change_id.clone(),
                role_key: role_key.to_owned(),
                provider: op.provider,
                operation_id: op.operation_id.clone(),
                state: op.state,
                run_id: op.run_id.clone(),
                attention: op.attention.clone(),
            },
        );
    }
    record.updated_at = Utc::now();
    crate::saga::save_record(app, record, effects)
}

/// Advance once, retrying a failed *read* once (GETs are safe; a POST is
/// never replayed by the runtime). Each call is bounded by the timeout.
async fn advance(
    runtime: &dyn SpecialistRuntime,
    operation_id: &str,
    timeout: Duration,
) -> Result<SpecialistProgress, String> {
    let once = |retry| async move {
        tokio::time::timeout(timeout, runtime.advance(operation_id, retry))
            .await
            .map_err(|_| {
                "provider call timed out; will resume from the saved operation".to_owned()
            })?
            .map_err(|e| e.to_string())
    };
    match once(false).await {
        Ok(p) => Ok(p),
        Err(_) => once(true).await,
    }
}

/// One provisioning pass for a role whose provider is asynchronous.
pub(crate) async fn drive_role(
    app: &AppState,
    record: &mut OrgChangeRecord,
    role: &RoleToProvision,
    runtime: Arc<dyn SpecialistRuntime>,
) -> ApiResult<Drive> {
    let key = role.key.clone();
    // 1. Prepare or find the operation.
    if !record.operations.contains_key(&key) {
        let req = ProvisionRequest {
            org_id: app.org_id.clone(),
            change_id: record.change_id.clone(),
            role: role.clone(),
        };
        let prepared = match runtime.prepare_role(&req) {
            Ok(p) => p,
            Err(e) => {
                return Ok(Drive::Failed(format!(
                    "could not prepare provider operation: {e}"
                )))
            }
        };
        record.operations.insert(
            key.clone(),
            ProviderOperation {
                provider: role.provider,
                operation_id: prepared.operation_id().to_owned(),
                state: S::Failed, // replaced by the update below
                run_id: None,
                native_status: None,
                attention: None,
                readiness_evidence: None,
                updated_at: Utc::now(),
            },
        );
        update(app, record, &key, |op| op.state = S::Pending, vec![])?;
    }
    let op = record.operations[&key].clone();

    // 2. Advance the native operation until the agent exists.
    if !record.provisioned.contains_key(&key) {
        match op.state {
            S::NeedsReconciliation => return Ok(Drive::Waiting), // operator action required
            S::Failed => return Ok(Drive::Failed("provider operation failed".into())),
            _ => {}
        }
        let progress =
            match advance(runtime.as_ref(), &op.operation_id, app.provision_timeout).await {
                Ok(p) => p,
                Err(reason) => {
                    update(app, record, &key, |o| o.attention = Some(reason), vec![])?;
                    return Ok(Drive::Waiting);
                }
            };
        match progress {
            SpecialistProgress::Pending {
                run_id,
                native_status,
                ..
            } => {
                update(
                    app,
                    record,
                    &key,
                    |o| {
                        o.state = S::Pending;
                        o.run_id = run_id.or(o.run_id.take());
                        o.native_status = native_status;
                        o.attention = None;
                    },
                    vec![],
                )?;
                return Ok(Drive::Waiting);
            }
            SpecialistProgress::NeedsReconciliation { run_id, .. } => {
                update(
                    app,
                    record,
                    &key,
                    |o| {
                        o.state = S::NeedsReconciliation;
                        o.run_id = run_id.or(o.run_id.take());
                        o.attention = Some(
                            "provider outcome unknown; reconcile the saved operation before retrying".into(),
                        );
                    },
                    vec![],
                )?;
                return Ok(Drive::Waiting);
            }
            SpecialistProgress::Failed { run_id, .. } => {
                update(
                    app,
                    record,
                    &key,
                    |o| {
                        o.state = S::Failed;
                        o.run_id = run_id.or(o.run_id.take());
                        o.attention = None;
                    },
                    vec![],
                )?;
                let o = &record.operations[&key];
                return Ok(Drive::Failed(format!(
                    "provider operation {} failed (run {})",
                    o.operation_id,
                    o.run_id.as_deref().unwrap_or("none")
                )));
            }
            SpecialistProgress::Collected { .. } => {
                return Ok(Drive::Failed(
                    "role operation returned an execution result".into(),
                ))
            }
            SpecialistProgress::Created {
                run_id,
                provider_agent_id,
                config_digest,
                ..
            } => {
                // 3. Persist the created identity before anything else.
                let binding = AgentBinding {
                    provider: role.provider,
                    provider_agent_id: provider_agent_id.clone(),
                    config_digest,
                    conversation_ref: None,
                };
                record.provisioned.insert(key.clone(), binding);
                let ev = EventKind::RoleProvisioned {
                    change_id: record.change_id.clone(),
                    role_key: key.clone(),
                    role_id: role.role_id.clone(),
                    provider: role.provider,
                    provider_agent_id,
                    dev_stub: false,
                };
                update(
                    app,
                    record,
                    &key,
                    |o| {
                        o.state = S::AwaitingReadiness;
                        o.run_id = Some(run_id);
                        o.native_status = Some("completed".into());
                        o.attention = None;
                    },
                    vec![ev],
                )?;
            }
        }
    }

    // 4. Readiness, proven by the provider's verifier.
    if record.operations[&key].state == S::Ready {
        return Ok(Drive::Ready);
    }
    let Some(verifier) = app.readiness.get(role.provider) else {
        update(
            app,
            record,
            &key,
            |o| {
                o.state = S::AwaitingReadiness;
                o.attention = Some(format!(
                    "agent created; no readiness verifier is configured for {:?}, so the role stays inactive",
                    role.provider
                ));
            },
            vec![],
        )?;
        return Ok(Drive::Waiting);
    };
    let binding = record.provisioned[&key].clone();
    let verdict = tokio::time::timeout(app.provision_timeout, verifier.verify(role, &binding))
        .await
        .map_err(|_| ProviderError::Timeout)
        .and_then(|r| r);
    match verdict {
        Ok(Readiness::Ready { evidence_ref }) => {
            record.ready.insert(key.clone());
            update(
                app,
                record,
                &key,
                |o| {
                    o.state = S::Ready;
                    o.readiness_evidence = Some(evidence_ref);
                    o.attention = None;
                },
                vec![],
            )?;
            Ok(Drive::Ready)
        }
        Ok(Readiness::Pending) => {
            update(app, record, &key, |o| o.attention = None, vec![])?;
            Ok(Drive::Waiting)
        }
        Ok(Readiness::NotReady(reason)) => Ok(Drive::Failed(format!("readiness: {reason}"))),
        Err(e) => {
            update(
                app,
                record,
                &key,
                |o| o.attention = Some(format!("readiness check: {e}")),
                vec![],
            )?;
            Ok(Drive::Waiting)
        }
    }
}

/// Changes with asynchronous operations that can still make progress.
fn advanceable(record: &OrgChangeRecord) -> bool {
    record.status == ChangeStatus::Provisioning
        && record
            .operations
            .values()
            .any(|o| matches!(o.state, S::Pending | S::AwaitingReadiness))
}

/// Re-run sagas with pending asynchronous operations and advance running
/// specialist jobs (see [`crate::specialist_jobs`]). The provider runtime
/// itself honors poll due times, so frequent passes do not mean frequent
/// provider calls. Returns `None` when no asynchronous provider is configured.
pub fn spawn(app: AppState, every: Duration) -> Option<tokio::task::JoinHandle<()>> {
    if app.providers.configured_specialists().is_empty() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        loop {
            tick.tick().await;
            let changes = match app.store.list_changes() {
                Ok(c) => c,
                Err(error) => {
                    tracing::warn!(%error, "specialist poller could not list changes");
                    continue;
                }
            };
            for c in changes.into_iter().filter(advanceable) {
                if let Err(e) = crate::saga::run(&app, &c.change_id).await {
                    tracing::warn!(change = %c.change_id, error = %e.message, "specialist provisioning pass failed");
                }
            }
            crate::specialist_jobs::advance_jobs(&app).await;
        }
    }))
}
