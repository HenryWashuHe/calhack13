//! Durable execution of one trusted, pinned capability request. Collection is
//! transport evidence only: this module never grants semantic success or writes
//! a commitment. One organization process owns each database in the demo.

use crate::capability_authority::authorize_job;
use crate::store::{put_job, EventMeta, Store, StoreError, StoreResult};
use chrono::Utc;
use genesis_apify::RunRequest;
use genesis_core::engine;
use genesis_core::events::{DomainEvent, EventKind};
use genesis_core::ids::{InstallationId, JobId};
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{InstallationStatus, JobStatus, StateMachine};
use genesis_core::model::{OrgState, Provider};
use genesis_providers::{CapabilityRunReceipt, ProviderError, ProviderSet};
use genesis_verification::{decode_json, json_sha256, ExecutionEvidence, RunStatus};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum CapabilityRuntimeError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("invalid capability run: {0}")]
    Invalid(&'static str),
    #[error("the previous read failed; explicit retry_read is required")]
    RetryRequired,
    #[error("retry_read requires a previously failed read")]
    UnexpectedRetry,
    #[error("the previous submission has an unknown outcome; reconcile without resubmitting")]
    NeedsReconciliation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRunPhase {
    Prepared,
    Submitting,
    Running,
    Collecting,
    Collected,
    Failed,
    NeedsReconciliation,
    Rejected,
}

impl CapabilityRunPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Submitting => "submitting",
            Self::Running => "running",
            Self::Collecting => "collecting",
            Self::Collected => "collected",
            Self::Failed => "failed",
            Self::NeedsReconciliation => "needs_reconciliation",
            Self::Rejected => "rejected",
        }
    }
}

/// Trusted server-created intent. Local acceptance can execute a checking
/// installation before attachment, but does not itself accept that installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionPurpose {
    Invocation,
    LocalAcceptance {
        installation_id: InstallationId,
        suite_id: String,
    },
    AdoptionAcceptance {
        adoption_id: String,
        installation_id: InstallationId,
        suite_id: String,
    },
}

impl ExecutionPurpose {
    pub(crate) fn is_acceptance_for(&self, id: &InstallationId, suite: &str) -> bool {
        match self {
            Self::LocalAcceptance {
                installation_id,
                suite_id,
            }
            | Self::AdoptionAcceptance {
                installation_id,
                suite_id,
                ..
            } => installation_id == id && suite_id == suite,
            Self::Invocation => false,
        }
    }
}

/// Private backend record; deserialization does not authenticate external claims.
/// The fingerprint binds the complete immutable job envelope and trusted request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRunRecord {
    pub schema_version: u32,
    pub job_id: JobId,
    pub request: RunRequest,
    pub required_scope: Option<String>,
    pub required_permissions: BTreeSet<String>,
    pub purpose: ExecutionPurpose,
    pub request_sha256: String,
    pub phase: CapabilityRunPhase,
    pub receipt: Option<CapabilityRunReceipt>,
    pub evidence: Option<ExecutionEvidence>,
    pub failure: Option<ProviderError>,
    pub retryable_read: bool,
}

impl CapabilityRunRecord {
    pub fn new(
        job: &Job,
        request: RunRequest,
        required_scope: Option<String>,
    ) -> Result<Self, CapabilityRuntimeError> {
        Self::new_with_purpose(job, request, required_scope, ExecutionPurpose::Invocation)
    }

    pub fn new_for_acceptance(
        job: &Job,
        request: RunRequest,
        required_scope: Option<String>,
        installation_id: InstallationId,
        suite_id: String,
    ) -> Result<Self, CapabilityRuntimeError> {
        Self::new_with_purpose(
            job,
            request,
            required_scope,
            ExecutionPurpose::LocalAcceptance {
                installation_id,
                suite_id,
            },
        )
    }

    pub(crate) fn new_with_purpose(
        job: &Job,
        request: RunRequest,
        required_scope: Option<String>,
        purpose: ExecutionPurpose,
    ) -> Result<Self, CapabilityRuntimeError> {
        if job.status != JobStatus::Queued {
            return Err(CapabilityRuntimeError::Invalid("new job must be queued"));
        }
        request
            .validate()
            .map_err(|_| CapabilityRuntimeError::Invalid("request"))?;
        let mut record = Self {
            schema_version: 1,
            job_id: job.job_id.clone(),
            request,
            required_scope,
            required_permissions: BTreeSet::new(),
            purpose,
            request_sha256: String::new(),
            phase: CapabilityRunPhase::Prepared,
            receipt: None,
            evidence: None,
            failure: None,
            retryable_read: false,
        };
        record.request_sha256 = record.fingerprint(job);
        record.validate(job)?;
        Ok(record)
    }

    /// Called with permissions from the trusted manifest before inserting intent.
    pub fn with_required_permissions(
        mut self,
        job: &Job,
        permissions: BTreeSet<String>,
    ) -> Result<Self, CapabilityRuntimeError> {
        self.validate(job)?;
        if self.phase != CapabilityRunPhase::Prepared {
            return Err(CapabilityRuntimeError::Invalid(
                "permissions are immutable after preparation",
            ));
        }
        self.required_permissions = permissions;
        self.request_sha256 = self.fingerprint(job);
        self.validate(job)?;
        Ok(self)
    }

    fn fingerprint(&self, job: &Job) -> String {
        json_sha256(
            &json!({"schema_version":1,"request":self.request,"required_scope":self.required_scope,"required_permissions":self.required_permissions,"purpose":self.purpose,
            "job":{"job_id":job.job_id,"org_id":job.org_id,"mission_id":job.mission_id,
            "role_id":job.role_id,"responsibility_id":job.responsibility_id,"assignment_id":job.assignment_id,
            "epoch":job.epoch,"attempt_id":job.attempt_id,"attempt_no":job.attempt_no,"task":job.task,
            "input":job.input,"capability":job.capability,"provider":job.provider,
            "idempotency_key":job.idempotency_key,"deadline":job.deadline}}),
        )
    }

    /// Compare a retried HTTP submission while ignoring newly allocated job IDs.
    /// Call only after loading/validating this record and its persisted job.
    pub fn matches_submission(
        &self,
        stored_job: &Job,
        proposed_job: &Job,
        request: &RunRequest,
        scope: Option<&str>,
    ) -> bool {
        self.purpose == ExecutionPurpose::Invocation
            && self.validate(stored_job).is_ok()
            && request.validate().is_ok()
            && json_sha256(&json!(self.request)) == json_sha256(&json!(request))
            && self.required_scope.as_deref() == scope
            && stored_job.org_id == proposed_job.org_id
            && stored_job.role_id == proposed_job.role_id
            && stored_job.responsibility_id == proposed_job.responsibility_id
            && stored_job.task == proposed_job.task
            && stored_job.input == proposed_job.input
            && stored_job.capability == proposed_job.capability
            && stored_job.provider == proposed_job.provider
            && stored_job.deadline == proposed_job.deadline
            && stored_job.idempotency_key == proposed_job.idempotency_key
    }

    pub fn validate(&self, job: &Job) -> Result<(), CapabilityRuntimeError> {
        let invalid = || CapabilityRuntimeError::Invalid("checkpoint or job binding");
        self.request.validate().map_err(|_| invalid())?;
        if self.schema_version != 1
            || self.job_id != job.job_id
            || job.provider != Some(Provider::Apify)
            || self.request.input != job.input
            || job.capability.as_ref().is_none_or(|cap| {
                cap.capability_id.as_str() != self.request.artifact.capability_id
                    || cap.version != self.request.artifact.capability_version
            })
            || self
                .required_scope
                .as_ref()
                .is_some_and(|s| !valid_label(s))
            || self.required_permissions.len() > 64
            || self.required_permissions.iter().any(|s| !valid_label(s))
            || matches!(&self.purpose, ExecutionPurpose::LocalAcceptance { installation_id, suite_id } | ExecutionPurpose::AdoptionAcceptance { installation_id, suite_id, .. }
                if !valid_label(installation_id.as_str()) || !valid_label(suite_id))
            || matches!(&self.purpose, ExecutionPurpose::AdoptionAcceptance { adoption_id, .. } if !valid_label(adoption_id))
            || self.request_sha256 != self.fingerprint(job)
        {
            return Err(invalid());
        }
        if let Some(receipt) = &self.receipt {
            if receipt.provider != Provider::Apify
                || receipt.raw_status.len() > 256
                || receipt.provider_refs.len() > 16
                || receipt
                    .provider_refs
                    .iter()
                    .any(|(key, value)| key.len() > 64 || value.len() > 256)
                || receipt.validate_for(&self.request).is_err()
            {
                return Err(invalid());
            }
        }
        if let Some(evidence) = &self.evidence {
            self.validate_evidence(evidence)?;
        }
        let shape = match self.phase {
            CapabilityRunPhase::Prepared | CapabilityRunPhase::Submitting => {
                self.receipt.is_none()
                    && self.evidence.is_none()
                    && self.failure.is_none()
                    && !self.retryable_read
            }
            CapabilityRunPhase::Running => {
                self.receipt.is_some()
                    && self.evidence.is_none()
                    && (self.failure.is_some() == self.retryable_read)
            }
            CapabilityRunPhase::Collecting => {
                self.receipt.is_some()
                    && self.evidence.is_none()
                    && self.failure.is_none()
                    && !self.retryable_read
            }
            CapabilityRunPhase::Collected => {
                self.receipt.is_some()
                    && self
                        .evidence
                        .as_ref()
                        .is_some_and(|evidence| job.result.as_ref() == Some(&evidence.output))
                    && self.failure.is_none()
                    && !self.retryable_read
            }
            CapabilityRunPhase::NeedsReconciliation => {
                self.receipt.is_none()
                    && self.evidence.is_none()
                    && self.failure.is_some()
                    && !self.retryable_read
            }
            CapabilityRunPhase::Failed | CapabilityRunPhase::Rejected => {
                self.evidence.is_none() && self.failure.is_some() && !self.retryable_read
            }
        };
        let projected = match self.phase {
            CapabilityRunPhase::Prepared => job.status == JobStatus::Queued,
            CapabilityRunPhase::Submitting => job.status == JobStatus::Submitting,
            CapabilityRunPhase::Running | CapabilityRunPhase::Collecting => {
                job.status == JobStatus::Running
            }
            // A separate trusted semantic verifier may later finalize this job.
            CapabilityRunPhase::Collected => matches!(
                job.status,
                JobStatus::Verifying
                    | JobStatus::Succeeded
                    | JobStatus::Failed
                    | JobStatus::ResultRejected
            ),
            CapabilityRunPhase::NeedsReconciliation => job.status == JobStatus::Unknown,
            CapabilityRunPhase::Failed => job.status == JobStatus::Failed,
            CapabilityRunPhase::Rejected => job.status == JobStatus::ResultRejected,
        };
        if !shape || !projected {
            return Err(invalid());
        }
        Ok(())
    }

    fn validate_evidence(
        &self,
        evidence: &ExecutionEvidence,
    ) -> Result<(), CapabilityRuntimeError> {
        let invalid = || CapabilityRuntimeError::Invalid("execution evidence binding");
        let ticket = self
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.ticket.as_ref())
            .ok_or_else(invalid)?;
        if evidence.case_id != self.request.case_id
            || evidence.artifact != self.request.artifact
            || evidence.source != self.request.source
            || evidence.input_sha256 != json_sha256(&self.request.input)
            || evidence.run_id != ticket.run_id()
            || evidence.status != RunStatus::Succeeded
            || evidence.dataset_id.is_empty()
            || evidence.dataset_id.len() > 256
            || !evidence
                .dataset_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !evidence.output.is_object()
            || !bounded_output(&evidence.output)
        {
            return Err(invalid());
        }
        let ticket_json = serde_json::to_value(ticket).map_err(|_| invalid())?;
        if ticket_json["dataset_id"]
            .as_str()
            .is_some_and(|expected| expected != evidence.dataset_id)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn bounded_output(value: &Value) -> bool {
    let mut stack = vec![(value, 0usize)];
    let mut count = 0usize;
    let mut bytes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        count += 1;
        if count > 50_000 || depth > 32 {
            return false;
        }
        match value {
            Value::String(s) => bytes = bytes.saturating_add(s.len()),
            Value::Array(items) => {
                if items.len() > 50_000usize.saturating_sub(count + stack.len()) {
                    return false;
                }
                stack.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Value::Object(items) => {
                if items.len() > 50_000usize.saturating_sub(count + stack.len()) {
                    return false;
                }
                for (key, item) in items {
                    bytes = bytes.saturating_add(key.len());
                    stack.push((item, depth + 1));
                }
            }
            _ => {}
        }
        if bytes > 1024 * 1024 {
            return false;
        }
    }
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 1024 * 1024)
}

impl Store {
    pub fn init_capability_runs(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_runs (
            job_id TEXT PRIMARY KEY REFERENCES jobs(job_id), client_request_id TEXT NOT NULL UNIQUE,
            phase TEXT NOT NULL, record_json TEXT NOT NULL);",
        )?;
        Ok(())
    }

    pub fn get_capability_run(
        &self,
        job_id: &JobId,
    ) -> Result<CapabilityRunRecord, CapabilityRuntimeError> {
        let row: Option<(String, String)> = self
            .lock()
            .query_row(
                "SELECT phase, record_json FROM capability_runs WHERE job_id = ?1",
                params![job_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StoreError::from)?;
        let (phase, text) =
            row.ok_or_else(|| StoreError::NotFound(format!("capability run {job_id}")))?;
        if text.len() > 4 * 1024 * 1024 {
            return Err(CapabilityRuntimeError::Invalid("checkpoint size"));
        }
        let record: CapabilityRunRecord = decode_json(text.as_bytes())
            .map_err(|_| CapabilityRuntimeError::Invalid("checkpoint schema"))?;
        if record.job_id != *job_id || phase != record.phase.label() {
            return Err(CapabilityRuntimeError::Invalid("checkpoint index"));
        }
        record.validate(&self.get_job(job_id)?)?;
        Ok(record)
    }

    pub fn find_capability_run(
        &self,
        client_key: &str,
    ) -> Result<Option<CapabilityRunRecord>, CapabilityRuntimeError> {
        let id: Option<String> = self
            .lock()
            .query_row(
                "SELECT job_id FROM capability_runs WHERE client_request_id = ?1",
                params![client_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)?;
        id.map(|id| self.get_capability_run(&JobId::new(id)))
            .transpose()
    }

    pub fn recoverable_capability_runs(
        &self,
    ) -> Result<Vec<CapabilityRunRecord>, CapabilityRuntimeError> {
        let ids = {
            let conn = self.lock();
            let mut statement = conn.prepare("SELECT job_id FROM capability_runs WHERE phase IN ('prepared','submitting','running','collecting') ORDER BY job_id").map_err(StoreError::from)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(StoreError::from)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)?
        };
        ids.into_iter()
            .map(|id| self.get_capability_run(&JobId::new(id)))
            .collect()
    }
}

fn invalid_store(message: &str) -> StoreError {
    StoreError::Db(rusqlite::Error::InvalidParameterName(message.into()))
}

/// Insert the job, idempotency key and capability request in the caller's event
/// transaction. A conflicting key must be resolved before constructing a new job.
pub fn insert_in_tx(
    tx: &Transaction<'_>,
    client_key: &str,
    job: &Job,
    record: &CapabilityRunRecord,
) -> StoreResult<()> {
    record
        .validate(job)
        .map_err(|_| invalid_store("invalid capability record"))?;
    if client_key.is_empty()
        || client_key.len() > 256
        || client_key.trim() != client_key
        || record.phase != CapabilityRunPhase::Prepared
        || job.status != JobStatus::Queued
    {
        return Err(invalid_store("invalid capability insertion"));
    }
    tx.execute("INSERT INTO jobs (job_id,client_request_id,status,job_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![job.job_id.as_str(), client_key, format!("{:?}",job.status), serde_json::to_string(job)?, job.created_at.to_rfc3339(), job.updated_at.to_rfc3339()])?;
    tx.execute("INSERT INTO capability_runs (job_id,client_request_id,phase,record_json) VALUES (?1,?2,?3,?4)",
        params![job.job_id.as_str(),client_key,record.phase.label(),serde_json::to_string(record)?])?;
    Ok(())
}

#[derive(Clone)]
pub struct CapabilityRuntime {
    store: Store,
    providers: ProviderSet,
    kernel: Arc<Mutex<()>>,
    gate: Arc<Mutex<()>>,
    events: Option<broadcast::Sender<DomainEvent>>,
    adoption_policy: Option<Arc<crate::capability_evidence::EvidencePolicy>>,
}

impl CapabilityRuntime {
    pub fn new(
        store: Store,
        providers: ProviderSet,
        kernel: Arc<Mutex<()>>,
        gate: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            store,
            providers,
            kernel,
            gate,
            events: None,
            adoption_policy: None,
        }
    }

    pub(crate) fn with_adoption_policy(
        mut self,
        policy: Arc<crate::capability_evidence::EvidencePolicy>,
    ) -> Self {
        self.adoption_policy = Some(policy);
        self
    }

    pub fn with_events(mut self, events: broadcast::Sender<DomainEvent>) -> Self {
        self.events = Some(events);
        self
    }

    /// One provider operation, with durable intent and authorization checks on
    /// both sides. Kernel state is unlocked during the provider await.
    pub async fn step(
        &self,
        job_id: &JobId,
        retry_read: bool,
    ) -> Result<CapabilityRunRecord, CapabilityRuntimeError> {
        let _single_step = self.gate.lock().await;
        let mut record;
        let executor;
        {
            let _kernel = self.kernel.lock().await;
            record = self.store.get_capability_run(job_id)?;
            let mut job = self.store.get_job(job_id)?;
            if record.phase == CapabilityRunPhase::Submitting
                || record.phase == CapabilityRunPhase::NeedsReconciliation
            {
                return Err(CapabilityRuntimeError::NeedsReconciliation);
            }
            if !matches!(
                record.phase,
                CapabilityRunPhase::Prepared | CapabilityRunPhase::Running
            ) {
                return Ok(record);
            }
            if record.retryable_read && !retry_read {
                return Err(CapabilityRuntimeError::RetryRequired);
            }
            if retry_read && !record.retryable_read {
                return Err(CapabilityRuntimeError::UnexpectedRetry);
            }
            let before = record.phase;
            if !authorized(
                &self.store,
                self.adoption_policy.as_deref(),
                &self.store.current_state()?,
                &job,
                &record,
            ) {
                reject(&mut record);
                self.persist(before, &record, &mut job)?;
                return Ok(record);
            }
            // A paused gap finalizer: no provider call, phase unchanged.
            if paused(&self.store, &self.store.current_state()?, &record) {
                return Ok(record);
            }
            executor = match self.providers.executor() {
                Ok(executor) if executor.provider() == Provider::Apify => executor,
                _ => {
                    record.phase = CapabilityRunPhase::Failed;
                    record.failure = Some(ProviderError::NotConfigured {
                        provider: Provider::Apify,
                    });
                    record.retryable_read = false;
                    self.persist(before, &record, &mut job)?;
                    return Ok(record);
                }
            };
            record.phase = if before == CapabilityRunPhase::Prepared {
                CapabilityRunPhase::Submitting
            } else {
                CapabilityRunPhase::Collecting
            };
            record.failure = None;
            record.retryable_read = false;
            self.persist(before, &record, &mut job)?;
        }

        enum Completed {
            Submission(Result<CapabilityRunReceipt, ProviderError>),
            Collection(Result<ExecutionEvidence, ProviderError>),
        }
        let completion = if record.phase == CapabilityRunPhase::Submitting {
            Completed::Submission(executor.submit(&record.request).await)
        } else {
            let ticket = record
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.ticket.as_ref())
                .ok_or(CapabilityRuntimeError::Invalid("missing saved ticket"))?;
            Completed::Collection(executor.resume(&record.request, ticket).await)
        };

        let _kernel = self.kernel.lock().await;
        let saved = self.store.get_capability_run(job_id)?;
        if saved.phase != record.phase || saved.request_sha256 != record.request_sha256 {
            return Err(CapabilityRuntimeError::Invalid(
                "concurrent checkpoint change",
            ));
        }
        let mut job = self.store.get_job(job_id)?;
        let before = record.phase;
        let still_authorized = authorized(
            &self.store,
            self.adoption_policy.as_deref(),
            &self.store.current_state()?,
            &job,
            &record,
        );
        match completion {
            Completed::Submission(Ok(receipt)) => {
                if receipt.provider != Provider::Apify
                    || receipt.validate_for(&record.request).is_err()
                {
                    record.phase = CapabilityRunPhase::NeedsReconciliation;
                    record.failure = Some(ProviderError::Unknown(
                        "invalid submission receipt; reconcile existing run".into(),
                    ));
                } else {
                    record.receipt = Some(receipt);
                    record.phase = CapabilityRunPhase::Running;
                }
            }
            Completed::Submission(Err(error)) => {
                record.phase = if matches!(error, ProviderError::Unknown(_)) {
                    CapabilityRunPhase::NeedsReconciliation
                } else {
                    CapabilityRunPhase::Failed
                };
                record.failure = Some(sanitize(error));
            }
            Completed::Collection(Ok(evidence)) => {
                if record.validate_evidence(&evidence).is_err() {
                    record.phase = CapabilityRunPhase::Failed;
                    record.failure = Some(ProviderError::Rejected(
                        "execution evidence does not match the saved request and ticket".into(),
                    ));
                } else {
                    record.evidence = Some(evidence);
                    record.phase = CapabilityRunPhase::Collected;
                }
            }
            Completed::Collection(Err(error)) => {
                record.retryable_read = retryable(&error);
                record.phase = if record.retryable_read {
                    CapabilityRunPhase::Running
                } else {
                    CapabilityRunPhase::Failed
                };
                record.failure = Some(sanitize(error));
            }
        }
        if !still_authorized {
            reject(&mut record);
        }
        self.persist(before, &record, &mut job)?;
        Ok(record)
    }

    /// Startup recovery does not contact providers. A saved POST intent without
    /// a ticket is ambiguous; a saved GET intent can safely retain its ticket.
    pub async fn recover_interrupted(&self) -> Result<usize, CapabilityRuntimeError> {
        let _single_step = self.gate.lock().await;
        let _kernel = self.kernel.lock().await;
        let mut changed = 0;
        for mut record in self.store.recoverable_capability_runs()? {
            let before = record.phase;
            match before {
                CapabilityRunPhase::Submitting => {
                    record.phase = CapabilityRunPhase::NeedsReconciliation;
                    record.failure = Some(ProviderError::Unknown(
                        "submission interrupted; reconcile without resubmitting".into(),
                    ));
                }
                CapabilityRunPhase::Collecting => {
                    record.phase = CapabilityRunPhase::Running;
                }
                _ => continue,
            }
            let mut job = self.store.get_job(&record.job_id)?;
            self.persist(before, &record, &mut job)?;
            changed += 1;
        }
        Ok(changed)
    }

    fn persist(
        &self,
        expected: CapabilityRunPhase,
        record: &CapabilityRunRecord,
        job: &mut Job,
    ) -> Result<(), CapabilityRuntimeError> {
        let mut events = Vec::new();
        let path: &[JobStatus] = match record.phase {
            CapabilityRunPhase::Prepared => &[],
            CapabilityRunPhase::Submitting => &[JobStatus::Submitting],
            CapabilityRunPhase::Running | CapabilityRunPhase::Collecting => {
                &[JobStatus::Submitting, JobStatus::Running]
            }
            CapabilityRunPhase::Collected => &[
                JobStatus::Submitting,
                JobStatus::Running,
                JobStatus::Verifying,
            ],
            CapabilityRunPhase::NeedsReconciliation => &[JobStatus::Submitting, JobStatus::Unknown],
            CapabilityRunPhase::Failed => &[JobStatus::Failed],
            CapabilityRunPhase::Rejected => &[
                JobStatus::Submitting,
                JobStatus::Running,
                JobStatus::ResultRejected,
            ],
        };
        for next in path {
            if job.status != *next && job.status.can_transition_to(*next) {
                let from = job
                    .set_status(*next, Utc::now())
                    .map_err(|_| CapabilityRuntimeError::Invalid("job transition"))?;
                events.push(EventKind::JobStatusChanged {
                    job_id: job.job_id.clone(),
                    from,
                    to: *next,
                });
            }
        }
        if let Some(receipt) = &record.receipt {
            job.provider_refs = receipt.provider_refs.clone();
            job.raw_status = Some("APIFY_SUBMITTED".into());
        }
        if let Some(evidence) = &record.evidence {
            job.result = Some(evidence.output.clone());
            job.raw_status = Some("APIFY_COLLECTED_UNVERIFIED".into());
        }
        if record.phase == CapabilityRunPhase::Rejected {
            job.rejection_reason = Some("capability invocation is no longer authorized".into());
            events.push(EventKind::JobResultRejected {
                job_id: job.job_id.clone(),
                reason: job.rejection_reason.clone().unwrap(),
            });
        }
        job.updated_at = Utc::now();
        record.validate(job)?;
        let encoded = serde_json::to_string(record).map_err(StoreError::from)?;
        let committed = self.store.append_events(events, &EventMeta::mission(job.mission_id.clone()).caused_by(job.job_id.to_string()), |tx| {
            let count = tx.execute("UPDATE capability_runs SET phase=?2,record_json=?3 WHERE job_id=?1 AND phase=?4",
                params![record.job_id.as_str(), record.phase.label(), encoded, expected.label()])?;
            if count != 1 { return Err(invalid_store("capability checkpoint changed")); }
            put_job(tx, job)
        })?;
        if let Some(sender) = &self.events {
            for event in committed {
                let _ = sender.send(event);
            }
        }
        Ok(())
    }
}

/// A local-acceptance run whose owning gap finalizer is paused at attention.
/// Results already in flight are still recorded; only new calls wait.
fn paused(store: &Store, state: &OrgState, record: &CapabilityRunRecord) -> bool {
    matches!(&record.purpose, ExecutionPurpose::LocalAcceptance { installation_id, .. }
        if crate::capability_gap_finalizer::run_fence(store, state, installation_id)
            == crate::capability_gap_finalizer::RunFence::Pause)
}

fn authorized(
    store: &Store,
    adoption_policy: Option<&crate::capability_evidence::EvidencePolicy>,
    state: &OrgState,
    job: &Job,
    record: &CapabilityRunRecord,
) -> bool {
    let now = Utc::now();
    if authorize_job(state, job, record.required_scope.as_deref(), now).is_err()
        || record
            .required_permissions
            .iter()
            .any(|scope| authorize_job(state, job, Some(scope), now).is_err())
    {
        return false;
    }
    let Some(capability) = &job.capability else {
        return false;
    };
    match &record.purpose {
        ExecutionPurpose::Invocation => {
            engine::authorize_capability(state, &job.role_id, capability).is_ok()
        }
        ExecutionPurpose::AdoptionAcceptance { .. } => adoption_policy.is_some_and(|policy| {
            crate::capability_adoption::authorized_job(store, policy, state, job, record)
        }),
        ExecutionPurpose::LocalAcceptance {
            installation_id, ..
        } => {
            state
                .installations
                .get(installation_id)
                .is_some_and(|installation| {
                    installation.status == InstallationStatus::Checking
                        && installation.capability == *capability
                        && installation.source_digest == record.request.artifact.source_sha256
                })
                // A gap finalizer's checks also need its frozen grant and live
                // requirement, before submission and before applying a result.
                && crate::capability_gap_finalizer::run_fence(store, state, installation_id)
                    != crate::capability_gap_finalizer::RunFence::Revoke
        }
    }
}

fn reject(record: &mut CapabilityRunRecord) {
    record.phase = CapabilityRunPhase::Rejected;
    record.evidence = None;
    record.failure = Some(ProviderError::Rejected(
        "capability invocation is no longer authorized".into(),
    ));
    record.retryable_read = false;
}

fn retryable(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Timeout | ProviderError::Unknown(_))
        || matches!(error, ProviderError::Http { status, .. } if *status == 408 || *status == 429 || *status >= 500)
}

fn sanitize(error: ProviderError) -> ProviderError {
    match error {
        ProviderError::NotConfigured { provider } => ProviderError::NotConfigured { provider },
        ProviderError::Http { status, .. } => ProviderError::Http {
            status,
            body: "capability provider HTTP failure".into(),
        },
        ProviderError::Timeout => ProviderError::Timeout,
        ProviderError::Rejected(_) => {
            ProviderError::Rejected("capability provider rejected the operation".into())
        }
        // Kept distinct: a confirmed terminal run is the only provider
        // failure after submission that proves the external effect stopped.
        ProviderError::RunFailed(_) => {
            ProviderError::RunFailed("capability provider run reached a terminal failure".into())
        }
        ProviderError::Unknown(_) => ProviderError::Unknown(
            "capability provider outcome requires reconciliation or a saved-ticket read".into(),
        ),
    }
}
