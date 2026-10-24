//! Drives one checkpointed capability-acquisition operation at a time. The
//! process gate is shared across runtime clones, not a distributed lease: one
//! organization process must own a database. Database CAS and organization
//! revision checks also reject conflicting writes. Provider I/O never holds the
//! kernel lock; cancellation leaves its debited, durable intent for recovery.

use crate::capability_acquisition_store::{
    AcquisitionDebit, AcquisitionRecord, AcquisitionStoreError,
};
use crate::capability_evidence::EvidencePolicy;
use crate::store::{Store, StoreError};
use chrono::Utc;
use genesis_apify::ApifyClient;
use genesis_forge::{
    AcquisitionError, AcquisitionPhase, ActionCompletion, AnthropicGenerator, ForgeAction,
    GenerationReceipt, ModelConfig, SynthesisError, SynthesisPrompt, VerifiedCandidate,
};
use genesis_verification::OracleSuite;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum AcquisitionRuntimeError {
    #[error(transparent)]
    Store(#[from] AcquisitionStoreError),
    #[error(transparent)]
    Persistence(#[from] StoreError),
    #[error(transparent)]
    Acquire(#[from] AcquisitionError),
    #[error("the backend acquisition oracle is not configured")]
    UnknownSuite,
    #[error("the acquisition provider is not configured: {0}")]
    Unconfigured(&'static str),
    #[error("the previous read failed; explicit retry_read is required")]
    RetryRequired,
    #[error("retry_read requires a previously failed read")]
    UnexpectedRetry,
    #[error("the acquisition is permanently fenced")]
    Fenced,
    #[error("the acquisition checkpoint changed during provider execution")]
    ConcurrentChange,
    #[error("the acquisition changed after it was scheduled")]
    StaleSchedule,
}

enum Effect {
    Generate {
        client: Arc<AnthropicGenerator>,
        prompt: SynthesisPrompt,
        config: ModelConfig,
    },
    Forge {
        client: Arc<ApifyClient>,
        action: Box<ForgeAction>,
    },
}
enum Completion {
    Generated(Box<Result<GenerationReceipt, SynthesisError>>),
    Forged(Box<ActionCompletion>),
}

#[derive(Clone)]
pub struct AcquisitionRuntime {
    store: Store,
    evidence: Arc<EvidencePolicy>,
    kernel: Arc<Mutex<()>>,
    gate: Arc<Mutex<()>>,
    model: Option<Arc<AnthropicGenerator>>,
    apify: Option<Arc<ApifyClient>>,
}

impl AcquisitionRuntime {
    pub fn new(
        store: Store,
        evidence: Arc<EvidencePolicy>,
        kernel: Arc<Mutex<()>>,
        gate: Arc<Mutex<()>>,
        model: Option<Arc<AnthropicGenerator>>,
        apify: Option<Arc<ApifyClient>>,
    ) -> Self {
        Self {
            store,
            evidence,
            kernel,
            gate,
            model,
            apify,
        }
    }

    fn suite(&self, suite_id: &str) -> Result<&OracleSuite, AcquisitionRuntimeError> {
        self.evidence
            .suite(suite_id)
            .ok_or(AcquisitionRuntimeError::UnknownSuite)
    }

    /// Backend-only lookup. HTTP handlers must emit a redacted status, never
    /// serialize this private checkpoint or its source, examples, and outputs.
    pub fn status(&self, id: &str) -> Result<AcquisitionRecord, AcquisitionRuntimeError> {
        let suite_id = self.store.acquisition_suite_id(id)?;
        Ok(self.store.get_acquisition(id, self.suite(&suite_id)?)?)
    }

    /// One local transition or provider operation. Intent and quota consumption
    /// commit atomically before the provider can observe the request. Every
    /// entry reconciles a prior interrupted POST without replaying it.
    pub async fn step(
        &self,
        id: &str,
        retry_read: bool,
    ) -> Result<AcquisitionRecord, AcquisitionRuntimeError> {
        self.step_inner(id, retry_read, None).await
    }

    /// Advance only the version selected by an automatic driver. Check inside
    /// the shared gate and kernel lock, before even local recovery or fencing,
    /// so waiting behind a manual step cannot advance a newly changed record.
    pub async fn step_if_current(
        &self,
        id: &str,
        expected_version: u64,
    ) -> Result<AcquisitionRecord, AcquisitionRuntimeError> {
        self.step_inner(id, false, Some(expected_version)).await
    }

    /// Reconcile saved intent and current authority without executing any model,
    /// Actor, build, or saved-ticket read. Interrupted POSTs become ambiguous;
    /// interrupted GETs keep their existing ticket and pending action unchanged.
    /// Failed reads retain their explicit retry gate. Useful for expiring paused
    /// work without silently retrying the effect that caused it to pause.
    pub async fn reconcile_if_current(
        &self,
        id: &str,
        expected_version: u64,
    ) -> Result<AcquisitionRecord, AcquisitionRuntimeError> {
        let _driver = self.gate.lock().await;
        let _kernel = self.kernel.lock().await;
        let mut record = self.status(id)?;
        if record.version() != expected_version {
            return Err(AcquisitionRuntimeError::StaleSchedule);
        }
        let suite = self.suite(record.suite_id())?;
        let mut session = record.session().clone();
        let recovered = session.recover_interrupted(suite)?;
        if recovered {
            record = record.with_session(session)?;
        }
        let state = self.store.current_state()?;
        let fenced = fence_if_unauthorized(&mut record, &state)?;
        if recovered || fenced {
            record = self.store.save_acquisition(
                record.version(),
                state.revision,
                &record,
                suite,
                None,
                Utc::now(),
            )?;
        }
        Ok(record)
    }

    async fn step_inner(
        &self,
        id: &str,
        retry_read: bool,
        expected_version: Option<u64>,
    ) -> Result<AcquisitionRecord, AcquisitionRuntimeError> {
        let _driver = self.gate.lock().await;
        let mut record;
        let effect;
        {
            let _kernel = self.kernel.lock().await;
            record = self.status(id)?;
            if expected_version.is_some_and(|version| record.version() != version) {
                return Err(AcquisitionRuntimeError::StaleSchedule);
            }
            let suite = self.suite(record.suite_id())?;
            let state = self.store.current_state()?;
            let mut session = record.session().clone();
            let recovered = session.recover_interrupted(suite)?;
            if recovered {
                record = record.with_session(session)?;
            }
            let fenced = fence_if_unauthorized(&mut record, &state)?;
            if recovered || fenced {
                return Ok(self.store.save_acquisition(
                    record.version(),
                    state.revision,
                    &record,
                    suite,
                    None,
                    Utc::now(),
                )?);
            }
            if record.is_fenced() {
                return Ok(record);
            }
            let failed_read = retryable_read(&record);
            if retry_read && !failed_read {
                return Err(AcquisitionRuntimeError::UnexpectedRetry);
            }
            if failed_read && !retry_read {
                return Err(AcquisitionRuntimeError::RetryRequired);
            }

            let mut session = record.session().clone();
            if session.advance_local(suite)? {
                record = record.with_session(session)?;
                return Ok(self.store.save_acquisition(
                    record.version(),
                    state.revision,
                    &record,
                    suite,
                    None,
                    Utc::now(),
                )?);
            }
            let debit;
            match session.phase() {
                AcquisitionPhase::Generating => {
                    let client = self
                        .model
                        .clone()
                        .ok_or(AcquisitionRuntimeError::Unconfigured("anthropic"))?;
                    let config = session.generation().config().clone();
                    let prompt = session.begin_generation(suite)?;
                    effect = Effect::Generate {
                        client,
                        prompt,
                        config,
                    };
                    debit = AcquisitionDebit::ModelCall;
                }
                AcquisitionPhase::Forging => {
                    let client = self
                        .apify
                        .clone()
                        .ok_or(AcquisitionRuntimeError::Unconfigured("apify"))?;
                    let action = session
                        .prepare_forge(suite)?
                        .ok_or(AcquisitionRuntimeError::ConcurrentChange)?;
                    debit = AcquisitionDebit::from(action.kind());
                    effect = Effect::Forge {
                        client,
                        action: Box::new(action),
                    };
                }
                AcquisitionPhase::Verified
                | AcquisitionPhase::Rejected
                | AcquisitionPhase::Failed
                | AcquisitionPhase::NeedsReconciliation => return Ok(record),
            }
            record = record.with_session(session)?;
            record = self.store.save_acquisition(
                record.version(),
                state.revision,
                &record,
                suite,
                Some(debit),
                Utc::now(),
            )?;
        }

        let completion = match effect {
            Effect::Generate {
                client,
                prompt,
                config,
            } => Completion::Generated(Box::new(client.generate(&prompt, &config).await)),
            Effect::Forge { client, action } => {
                Completion::Forged(Box::new((*action).execute(&client).await))
            }
        };

        let _kernel = self.kernel.lock().await;
        let suite = self.suite(record.suite_id())?;
        let saved = self.store.get_acquisition(id, suite)?;
        if saved.version() != record.version() || saved.binding() != record.binding() {
            return Err(AcquisitionRuntimeError::ConcurrentChange);
        }
        let state = self.store.current_state()?;
        let mut session = record.session().clone();
        match completion {
            Completion::Generated(result) => session.complete_generation(*result, suite)?,
            Completion::Forged(result) => session.complete_forge(*result, suite)?,
        }
        // Preserve received source, Actor/build/run IDs, and evidence even when
        // the grant expired or was revoked while I/O was in flight. Fencing the
        // outer record prevents later authority restoration from reviving it.
        record = record.with_session(session)?;
        fence_if_unauthorized(&mut record, &state)?;
        Ok(self.store.save_acquisition(
            record.version(),
            state.revision,
            &record,
            suite,
            None,
            Utc::now(),
        )?)
    }

    /// Startup recovery is entirely local. It also permanently fences work
    /// whose captured organization grant is no longer valid.
    pub async fn recover_interrupted(&self) -> Result<usize, AcquisitionRuntimeError> {
        let _driver = self.gate.lock().await;
        let _kernel = self.kernel.lock().await;
        let mut recovered = 0;
        for id in self.store.acquisition_ids()? {
            let mut record = self.status(&id)?;
            let suite = self.suite(record.suite_id())?;
            let mut session = record.session().clone();
            let changed = session.recover_interrupted(suite)?;
            if changed {
                record = record.with_session(session)?;
            }
            let state = self.store.current_state()?;
            let fenced = fence_if_unauthorized(&mut record, &state)?;
            if changed || fenced {
                self.store.save_acquisition(
                    record.version(),
                    state.revision,
                    &record,
                    suite,
                    None,
                    Utc::now(),
                )?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    /// Returns a proof-bearing private record only after fresh grant checks.
    /// Publication and installation still require their own kernel policy.
    pub async fn verified_record(
        &self,
        id: &str,
    ) -> Result<AcquisitionRecord, AcquisitionRuntimeError> {
        let _driver = self.gate.lock().await;
        let _kernel = self.kernel.lock().await;
        let mut record = self.status(id)?;
        let suite = self.suite(record.suite_id())?;
        let state = self.store.current_state()?;
        if fence_if_unauthorized(&mut record, &state)? {
            self.store.save_acquisition(
                record.version(),
                state.revision,
                &record,
                suite,
                None,
                Utc::now(),
            )?;
        }
        if record.is_fenced() {
            return Err(AcquisitionRuntimeError::Fenced);
        }
        record.session().verified(suite)?;
        Ok(record)
    }

    pub async fn verified(&self, id: &str) -> Result<VerifiedCandidate, AcquisitionRuntimeError> {
        let record = self.verified_record(id).await?;
        Ok(record.session().verified(self.suite(record.suite_id())?)?)
    }
}

fn retryable_read(record: &AcquisitionRecord) -> bool {
    record.session().phase() == AcquisitionPhase::Forging
        && record.session().forge().is_some_and(|forge| {
            forge
                .pending_action()
                .is_some_and(|kind| !kind.is_mutating())
                && forge.failure().is_some_and(|failure| failure.retryable)
        })
}

fn fence_if_unauthorized(
    record: &mut AcquisitionRecord,
    state: &genesis_core::model::OrgState,
) -> Result<bool, AcquisitionRuntimeError> {
    if record.is_fenced() {
        return Ok(false);
    }
    match record.authorize(state, Utc::now()) {
        Ok(()) => Ok(false),
        Err(AcquisitionStoreError::Authority(reason)) => {
            record.fence(reason);
            Ok(true)
        }
        Err(error) => Err(error.into()),
    }
}
