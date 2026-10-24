//! Durable, organization-bound CREAO operations. Native completion is an
//! observation, not permission to activate a role or accept a mission result.
//!
//! Use one active runtime per organization database. Clones share a step gate;
//! SQLite compare-and-swap also prevents concurrent submission claims. Call
//! `recover_interrupted` at startup before resuming work. The database contains
//! private requests and provider output and is not an importable authority token.

use crate::{
    ApiVersion, CreaoClient, Error, ErrorKind, NativeRunStatus, OperationRequest, PreEffectGuard,
    RunObservation, RunTicket,
};
use chrono::{DateTime, Utc};
use genesis_verification::{decode_json, json_sha256};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::OpenOptions,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("invalid CREAO operation or stored checkpoint")]
    Invalid,
    #[error("CREAO operation context does not match this runtime")]
    ContextMismatch,
    #[error("CREAO operation ID already belongs to a different request or context")]
    Conflict,
    #[error("CREAO operation was not found")]
    NotFound,
    #[error("CREAO checkpoint changed concurrently")]
    ConcurrentChange,
    #[error("CREAO runtime storage is unavailable")]
    Storage,
    #[error("CREAO operation needs reconciliation; it will not be resubmitted")]
    NeedsReconciliation,
    #[error("CREAO read failure requires an explicit retry")]
    RetryRequired,
    #[error("CREAO operation has no read failure to retry")]
    UnexpectedRetry,
    #[error("CREAO readiness challenge expired before submission")]
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationPurpose {
    Provision {
        change_id: String,
        config_digest: String,
    },
    Execution {
        job_id: String,
        assignment_id: String,
        epoch: u64,
    },
    /// A gateway readiness challenge is neither provisioning nor mission work.
    /// Expiration controls dispatch/receipt acceptance, not historical validity.
    Readiness {
        challenge_id: String,
        config_digest: String,
        expires_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OperationSpec {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "ApiVersion::is_v1")]
    pub api_version: ApiVersion,
    pub operation_id: String,
    pub org_id: String,
    pub role_id: String,
    pub account_namespace: String,
    pub purpose: OperationPurpose,
    pub request: OperationRequest,
}

impl OperationSpec {
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.schema_version != 1
            || [&self.operation_id, &self.org_id, &self.role_id]
                .iter()
                .any(|v| !identifier(v))
            || !crate::model::plain(&self.account_namespace, 256)
            || !self.account_namespace.is_ascii()
            || self
                .request
                .payload_for(self.api_version, crate::model::MAX_INPUT)
                .is_err()
        {
            return Err(RuntimeError::Invalid);
        }
        match (&self.purpose, &self.request) {
            (
                OperationPurpose::Provision {
                    change_id,
                    config_digest,
                },
                OperationRequest::CreateAgent(_),
            ) if identifier(change_id) && digest(config_digest) => Ok(()),
            (
                OperationPurpose::Execution {
                    job_id,
                    assignment_id,
                    ..
                },
                OperationRequest::CreateRun(_),
            ) if identifier(job_id) && identifier(assignment_id) => Ok(()),
            (
                OperationPurpose::Readiness {
                    challenge_id,
                    config_digest,
                    ..
                },
                OperationRequest::CreateRun(_),
            ) if identifier(challenge_id) && digest(config_digest) => Ok(()),
            _ => Err(RuntimeError::Invalid),
        }
    }

    pub fn fingerprint(&self) -> Result<String, RuntimeError> {
        self.validate()?;
        let request_sha256 = self
            .request
            .request_sha256_for(self.api_version)
            .map_err(|_| RuntimeError::Invalid)?;
        Ok(json_sha256(
            &json!({"schema_version": 1, "spec": self, "request_sha256": request_sha256}),
        ))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Prepared,
    Submitting,
    Waiting,
    Polling,
    NativeCompleted,
    Failed,
    NeedsReconciliation,
}

impl OperationPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Submitting => "submitting",
            Self::Waiting => "waiting",
            Self::Polling => "polling",
            Self::NativeCompleted => "native_completed",
            Self::Failed => "failed",
            Self::NeedsReconciliation => "needs_reconciliation",
        }
    }
}

/// Sanitized failure metadata; never stores a provider error body or credential.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "reason",
    content = "error",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RuntimeFailure {
    Provider(Error),
    InterruptedSubmission,
    UnknownStatus,
    NativeFailed,
    NativeCancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OperationRecord {
    pub spec: OperationSpec,
    pub fingerprint: String,
    pub phase: OperationPhase,
    pub ticket: Option<RunTicket>,
    pub observation: Option<RunObservation>,
    pub failure: Option<RuntimeFailure>,
    pub retryable_read: bool,
    pub next_poll_at: Option<DateTime<Utc>>,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OperationRecord {
    /// Validates provenance and state shape, not authenticity of an imported
    /// record. Only the trusted backend's own database may supply checkpoints.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.fingerprint != self.spec.fingerprint()?
            || self.updated_at < self.created_at
            || self.revision > i64::MAX as u64
        {
            return Err(RuntimeError::Invalid);
        }
        if let Some(ticket) = &self.ticket {
            ticket
                .validate_for_account(&self.spec.request, &self.spec.account_namespace)
                .map_err(|_| RuntimeError::Invalid)?;
        }
        if let Some(observation) = &self.observation {
            observation
                .validate_for(
                    &self.spec.request,
                    self.ticket.as_ref().ok_or(RuntimeError::Invalid)?,
                )
                .map_err(|_| RuntimeError::Invalid)?;
        }
        if self.next_poll_at.is_some_and(|at| at < self.updated_at)
            || self.retryable_read && self.phase != OperationPhase::Waiting
        {
            return Err(RuntimeError::Invalid);
        }
        let clean = self.failure.is_none() && !self.retryable_read;
        let nonterminal = self.observation.as_ref().is_none_or(|o| {
            matches!(
                o.status,
                NativeRunStatus::Pending | NativeRunStatus::Running
            )
        });
        let valid = match self.phase {
            OperationPhase::Prepared | OperationPhase::Submitting => {
                self.ticket.is_none()
                    && self.observation.is_none()
                    && clean
                    && self.next_poll_at.is_none()
            }
            OperationPhase::Waiting => {
                self.ticket.is_some()
                    && self.next_poll_at.is_some()
                    && nonterminal
                    && (clean
                        || self.retryable_read
                            && matches!(&self.failure, Some(RuntimeFailure::Provider(error)) if retryable(error) && !error.post_may_have_run))
            }
            OperationPhase::Polling => {
                self.ticket.is_some() && nonterminal && clean && self.next_poll_at.is_none()
            }
            OperationPhase::NativeCompleted => {
                self.ticket.is_some()
                    && clean
                    && self.next_poll_at.is_none()
                    && self
                        .observation
                        .as_ref()
                        .is_some_and(|o| o.status.is_completed())
            }
            OperationPhase::Failed => {
                !self.retryable_read
                    && self.next_poll_at.is_none()
                    && match &self.failure {
                        Some(RuntimeFailure::NativeFailed) => self
                            .observation
                            .as_ref()
                            .is_some_and(|o| o.status == NativeRunStatus::Failed),
                        Some(RuntimeFailure::NativeCancelled) => self
                            .observation
                            .as_ref()
                            .is_some_and(|o| o.status == NativeRunStatus::Cancelled),
                        Some(RuntimeFailure::Provider(error)) => {
                            !error.post_may_have_run && nonterminal
                        }
                        _ => false,
                    }
            }
            OperationPhase::NeedsReconciliation => {
                !self.retryable_read
                    && self.next_poll_at.is_none()
                    && match &self.failure {
                        Some(RuntimeFailure::InterruptedSubmission) => {
                            self.ticket.is_none() && self.observation.is_none()
                        }
                        Some(RuntimeFailure::Provider(error)) => {
                            error.post_may_have_run
                                && self.ticket.is_none()
                                && self.observation.is_none()
                        }
                        Some(RuntimeFailure::UnknownStatus) => self
                            .observation
                            .as_ref()
                            .is_some_and(|o| matches!(o.status, NativeRunStatus::Unknown(_))),
                        _ => false,
                    }
            }
        };
        if valid {
            Ok(())
        } else {
            Err(RuntimeError::Invalid)
        }
    }
}

#[derive(Clone)]
pub struct RuntimeStore {
    connection: Arc<Mutex<Connection>>,
    org_id: Arc<str>,
    gate: Arc<AsyncMutex<()>>,
}

impl RuntimeStore {
    /// The parent directory must be private and backend-controlled. New files
    /// are mode 0600 on Unix. Existing symlinks/nonregular files are rejected.
    pub fn open(path: impl AsRef<Path>, org_id: &str) -> Result<Self, RuntimeError> {
        if !identifier(org_id) {
            return Err(RuntimeError::Invalid);
        }
        let path = path.as_ref();
        match std::fs::symlink_metadata(path) {
            Ok(meta) if !meta.file_type().is_file() => return Err(RuntimeError::Storage),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options.open(path).map_err(|_| RuntimeError::Storage)?;
            }
            Err(_) => return Err(RuntimeError::Storage),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| RuntimeError::Storage)?;
        }
        Self::initialize(
            Connection::open(path).map_err(|_| RuntimeError::Storage)?,
            org_id,
        )
    }

    pub fn in_memory(org_id: &str) -> Result<Self, RuntimeError> {
        Self::initialize(
            Connection::open_in_memory().map_err(|_| RuntimeError::Storage)?,
            org_id,
        )
    }

    fn initialize(mut connection: Connection, org_id: &str) -> Result<Self, RuntimeError> {
        if !identifier(org_id) {
            return Err(RuntimeError::Invalid);
        }
        connection
            .busy_timeout(Duration::from_secs(1))
            .map_err(|_| RuntimeError::Storage)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS creao_runtime_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema_version INTEGER NOT NULL, org_id TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS creao_operations (operation_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, phase TEXT NOT NULL, revision INTEGER NOT NULL, record_json TEXT NOT NULL);")
            .map_err(|_| RuntimeError::Storage)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| RuntimeError::Storage)?;
        tx.execute(
            "INSERT OR IGNORE INTO creao_runtime_meta VALUES (1,1,?1)",
            [org_id],
        )
        .map_err(|_| RuntimeError::Storage)?;
        let binding: (i64, String) = tx
            .query_row(
                "SELECT schema_version,org_id FROM creao_runtime_meta WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| RuntimeError::Storage)?;
        if binding != (1, org_id.to_owned()) {
            return Err(RuntimeError::ContextMismatch);
        }
        tx.commit().map_err(|_| RuntimeError::Storage)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            org_id: Arc::from(org_id),
            gate: Arc::new(AsyncMutex::new(())),
        })
    }

    pub fn org_id(&self) -> &str {
        &self.org_id
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, RuntimeError> {
        self.connection.lock().map_err(|_| RuntimeError::Storage)
    }

    /// The key is immutable: idempotency binds the entire request and all
    /// organization, account, role, assignment, epoch and change context.
    pub fn prepare(&self, spec: OperationSpec) -> Result<OperationRecord, RuntimeError> {
        let fingerprint = spec.fingerprint()?;
        if spec.org_id != self.org_id() {
            return Err(RuntimeError::ContextMismatch);
        }
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| RuntimeError::Storage)?;
        if let Some(existing) = read_record(&tx, &spec.operation_id, self.org_id())? {
            if existing.fingerprint != fingerprint || existing.spec != spec {
                return Err(RuntimeError::Conflict);
            }
            return Ok(existing);
        }
        let now = Utc::now();
        let record = OperationRecord {
            spec,
            fingerprint,
            phase: OperationPhase::Prepared,
            ticket: None,
            observation: None,
            failure: None,
            retryable_read: false,
            next_poll_at: None,
            revision: 0,
            created_at: now,
            updated_at: now,
        };
        let encoded = encode(&record)?;
        tx.execute(
            "INSERT INTO creao_operations VALUES (?1,?2,?3,?4,?5)",
            params![
                record.spec.operation_id,
                record.fingerprint,
                record.phase.label(),
                record.revision as i64,
                encoded
            ],
        )
        .map_err(|_| RuntimeError::Storage)?;
        tx.commit().map_err(|_| RuntimeError::Storage)?;
        Ok(record)
    }

    pub fn get(&self, operation_id: &str) -> Result<OperationRecord, RuntimeError> {
        if !identifier(operation_id) {
            return Err(RuntimeError::Invalid);
        }
        read_record(&*self.lock()?, operation_id, self.org_id())?.ok_or(RuntimeError::NotFound)
    }

    /// Bounded listing for operational inspection; use `get` for a known ID.
    pub fn list(&self) -> Result<Vec<OperationRecord>, RuntimeError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT operation_id FROM creao_operations ORDER BY operation_id LIMIT 1000")
            .map_err(|_| RuntimeError::Storage)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| RuntimeError::Storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RuntimeError::Storage)?;
        ids.into_iter()
            .map(|id| read_record(&connection, &id, self.org_id())?.ok_or(RuntimeError::NotFound))
            .collect()
    }

    fn save(
        &self,
        expected: &OperationRecord,
        mut next: OperationRecord,
    ) -> Result<OperationRecord, RuntimeError> {
        if next.spec != expected.spec
            || next.fingerprint != expected.fingerprint
            || next.spec.org_id != self.org_id()
        {
            return Err(RuntimeError::Invalid);
        }
        next.revision = expected
            .revision
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or(RuntimeError::Invalid)?;
        let encoded = encode(&next)?;
        let changed = self.lock()?.execute("UPDATE creao_operations SET phase=?1,revision=?2,record_json=?3 WHERE operation_id=?4 AND fingerprint=?5 AND phase=?6 AND revision=?7 AND record_json=?8",
            params![next.phase.label(), next.revision as i64, encoded, expected.spec.operation_id, expected.fingerprint, expected.phase.label(), expected.revision as i64, encode(expected)?])
            .map_err(|_| RuntimeError::Storage)?;
        if changed != 1 {
            return Err(RuntimeError::ConcurrentChange);
        }
        Ok(next)
    }
}

#[derive(Clone)]
pub struct Runtime {
    client: CreaoClient,
    store: RuntimeStore,
    poll_interval: Duration,
}

impl Runtime {
    pub fn new(
        client: CreaoClient,
        store: RuntimeStore,
        poll_interval: Duration,
    ) -> Result<Self, RuntimeError> {
        if poll_interval.is_zero() || poll_interval > Duration::from_secs(3600) {
            return Err(RuntimeError::Invalid);
        }
        Ok(Self {
            client,
            store,
            poll_interval,
        })
    }

    pub fn org_id(&self) -> &str {
        self.store.org_id()
    }
    pub fn account_namespace(&self) -> &str {
        self.client.account_namespace()
    }
    pub fn api_version(&self) -> ApiVersion {
        self.client.api_version()
    }
    pub fn prepare(&self, spec: OperationSpec) -> Result<OperationRecord, RuntimeError> {
        if spec.account_namespace != self.account_namespace()
            || spec.api_version != self.api_version()
        {
            return Err(RuntimeError::ContextMismatch);
        }
        self.store.prepare(spec)
    }
    pub fn get(&self, operation_id: &str) -> Result<OperationRecord, RuntimeError> {
        let record = self.store.get(operation_id)?;
        self.validate_context(&record)?;
        Ok(record)
    }

    fn validate_context(&self, record: &OperationRecord) -> Result<(), RuntimeError> {
        record.validate()?;
        if record.spec.account_namespace != self.account_namespace()
            || record.spec.org_id != self.org_id()
            || record.spec.api_version != self.api_version()
        {
            return Err(RuntimeError::ContextMismatch);
        }
        Ok(())
    }

    /// Performs at most one POST or GET. Due-time waits return the unchanged
    /// record, and transient read failures require an explicit `retry_read`.
    pub async fn step(
        &self,
        operation_id: &str,
        retry_read: bool,
    ) -> Result<OperationRecord, RuntimeError> {
        let _gate = self.store.gate.lock().await;
        self.step_locked(operation_id, retry_read, None).await
    }

    /// Recheck live backend authority after both runtime and account queues.
    /// While waiting for the guard, the operation retains its previous state.
    /// Its durable effect claim is saved only after authorization, before HTTP.
    /// Cancellation after that claim remains conservatively ambiguous for POST;
    /// the caller must independently fence any result after authority changes.
    pub async fn step_guarded(
        &self,
        operation_id: &str,
        retry_read: bool,
        guard: PreEffectGuard,
    ) -> Result<OperationRecord, RuntimeError> {
        let _gate = self.store.gate.lock().await;
        self.step_locked(operation_id, retry_read, Some(guard))
            .await
    }

    /// Atomically submit only a still-prepared operation. Concurrent callers
    /// return its saved state; they never poll or replay an existing submission.
    /// This lets a readiness probe own dispatch without taking over collection.
    pub async fn submit_prepared(
        &self,
        operation_id: &str,
    ) -> Result<OperationRecord, RuntimeError> {
        let _gate = self.store.gate.lock().await;
        let record = self.get(operation_id)?;
        if record.phase != OperationPhase::Prepared {
            return Ok(record);
        }
        self.step_locked(operation_id, false, None).await
    }

    async fn step_locked(
        &self,
        operation_id: &str,
        retry_read: bool,
        guard: Option<PreEffectGuard>,
    ) -> Result<OperationRecord, RuntimeError> {
        let record = self.get(operation_id)?;
        match record.phase {
            OperationPhase::Submitting | OperationPhase::NeedsReconciliation => {
                return Err(RuntimeError::NeedsReconciliation)
            }
            OperationPhase::Polling => return Err(RuntimeError::ConcurrentChange),
            OperationPhase::NativeCompleted | OperationPhase::Failed => return Ok(record),
            _ => {}
        }
        if record.phase == OperationPhase::Prepared
            && matches!(&record.spec.purpose, OperationPurpose::Readiness { expires_at, .. } if *expires_at <= Utc::now())
        {
            return Err(RuntimeError::Expired);
        }
        if record.retryable_read && !retry_read {
            return Err(RuntimeError::RetryRequired);
        }
        if retry_read && !record.retryable_read {
            return Err(RuntimeError::UnexpectedRetry);
        }
        if record.next_poll_at.is_some_and(|at| at > Utc::now()) {
            return Ok(record);
        }
        let submitting = record.phase == OperationPhase::Prepared;
        let mut claimed = record.clone();
        claimed.phase = if submitting {
            OperationPhase::Submitting
        } else {
            OperationPhase::Polling
        };
        claimed.updated_at = Utc::now().max(record.updated_at);
        claimed.next_poll_at = None;
        claimed.failure = None;
        claimed.retryable_read = false;
        // Legacy calls claim before entering the client. Guarded calls defer
        // this same FULL-synchronous write until AFTER quota admission and live
        // authorization. Cancellation while queued therefore leaves Prepared
        // (or Waiting), not an invented uncertain submission.
        let deferred_claim = Arc::new(Mutex::new(None));
        let (claimed, effect_guard) = if let Some(guard) = guard {
            let store = self.store.clone();
            let before = record.clone();
            let claim = claimed;
            let result = deferred_claim.clone();
            let effect_guard: PreEffectGuard = Arc::new(move || {
                let guard = guard.clone();
                let store = store.clone();
                let before = before.clone();
                let claim = claim.clone();
                let result = result.clone();
                Box::pin(async move {
                    if !guard().await {
                        return false;
                    }
                    let mut claim = claim;
                    claim.updated_at = Utc::now().max(before.updated_at);
                    let saved = store.save(&before, claim);
                    let allowed = saved.is_ok();
                    let Ok(mut result) = result.lock() else {
                        return false;
                    };
                    *result = Some(saved);
                    allowed
                })
            });
            (None, Some(effect_guard))
        } else {
            (Some(self.store.save(&record, claimed)?), None)
        };
        let submission = if submitting {
            Some(
                self.client
                    .submit_with_guard(&record.spec.request, effect_guard.as_ref())
                    .await,
            )
        } else {
            None
        };
        let poll = if !submitting {
            let ticket = record.ticket.as_ref().ok_or(RuntimeError::Invalid)?;
            Some(
                self.client
                    .poll_with_guard(&record.spec.request, ticket, effect_guard.as_ref())
                    .await,
            )
        } else {
            None
        };
        let claimed = if let Some(claimed) = claimed {
            claimed
        } else {
            let saved = deferred_claim
                .lock()
                .map_err(|_| RuntimeError::Storage)?
                .take();
            match saved {
                Some(saved) => saved?,
                // The client failed before authorization or the guard denied
                // it. Persist a definite local failure from the unchanged
                // record, never an unknown external outcome.
                None => record.clone(),
            }
        };
        let mut completed = claimed.clone();
        // Also clear an old read failure when no new claim was needed.
        completed.next_poll_at = None;
        completed.retryable_read = false;
        completed.failure = None;
        if let Some(submission) = submission {
            match submission {
                Ok(ticket) => {
                    ticket
                        .validate_for_account(&claimed.spec.request, self.account_namespace())
                        .map_err(|_| RuntimeError::Invalid)?;
                    completed.ticket = Some(ticket);
                    completed.phase = OperationPhase::Waiting;
                }
                Err(error) => {
                    completed.phase = if error.post_may_have_run {
                        OperationPhase::NeedsReconciliation
                    } else {
                        OperationPhase::Failed
                    };
                    completed.failure = Some(RuntimeFailure::Provider(error));
                }
            }
        } else if let Some(poll) = poll {
            let ticket = claimed.ticket.as_ref().ok_or(RuntimeError::Invalid)?;
            match poll {
                Ok(observation) => {
                    observation
                        .validate_for(&claimed.spec.request, ticket)
                        .map_err(|_| RuntimeError::Invalid)?;
                    // A nullable identity may be learned later, but cannot
                    // change after any successful observation or restart.
                    completed.ticket = Some(
                        ticket
                            .refine(&claimed.spec.request, &observation)
                            .map_err(|_| RuntimeError::Invalid)?,
                    );
                    match observation.status {
                        NativeRunStatus::Pending | NativeRunStatus::Running => {
                            completed.phase = OperationPhase::Waiting
                        }
                        NativeRunStatus::Completed => {
                            completed.phase = OperationPhase::NativeCompleted
                        }
                        NativeRunStatus::Failed => {
                            completed.phase = OperationPhase::Failed;
                            completed.failure = Some(RuntimeFailure::NativeFailed);
                        }
                        NativeRunStatus::Cancelled => {
                            completed.phase = OperationPhase::Failed;
                            completed.failure = Some(RuntimeFailure::NativeCancelled);
                        }
                        NativeRunStatus::Unknown(_) => {
                            completed.phase = OperationPhase::NeedsReconciliation;
                            completed.failure = Some(RuntimeFailure::UnknownStatus);
                        }
                    }
                    completed.observation = Some(observation);
                }
                Err(error) => {
                    completed.retryable_read = retryable(&error);
                    completed.phase = if completed.retryable_read {
                        OperationPhase::Waiting
                    } else {
                        OperationPhase::Failed
                    };
                    completed.failure = Some(RuntimeFailure::Provider(error));
                }
            }
        }
        completed.updated_at = Utc::now().max(claimed.updated_at);
        if completed.phase == OperationPhase::Waiting {
            let delay = match &completed.failure {
                Some(RuntimeFailure::Provider(error)) => {
                    Duration::from_secs(error.retry_after_secs.unwrap_or(0).min(86400))
                        .max(self.poll_interval)
                }
                _ => self.poll_interval,
            };
            completed.next_poll_at = Some(after(completed.updated_at, delay)?);
        }
        // If this commit fails, the saved intent remains. Recovery will never
        // turn an uncertain submission back into a new POST.
        self.store.save(&claimed, completed)
    }

    /// Local-only startup recovery. Interrupted GETs retain their saved ticket;
    /// interrupted POSTs require external reconciliation and cannot be replayed.
    pub async fn recover_interrupted(&self) -> Result<usize, RuntimeError> {
        let _gate = self.store.gate.lock().await;
        let ids = {
            let connection = self.store.lock()?;
            let mut statement = connection.prepare("SELECT operation_id FROM creao_operations WHERE phase IN ('submitting','polling') ORDER BY operation_id").map_err(|_| RuntimeError::Storage)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|_| RuntimeError::Storage)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|_| RuntimeError::Storage)?
        };
        let mut changed = 0;
        for id in ids {
            let record = self.get(&id)?;
            let mut recovered = record.clone();
            recovered.updated_at = Utc::now().max(record.updated_at);
            match record.phase {
                OperationPhase::Submitting => {
                    recovered.phase = OperationPhase::NeedsReconciliation;
                    recovered.failure = Some(RuntimeFailure::InterruptedSubmission);
                }
                OperationPhase::Polling => {
                    recovered.phase = OperationPhase::Waiting;
                    recovered.next_poll_at = Some(after(recovered.updated_at, self.poll_interval)?);
                }
                _ => continue,
            }
            self.store.save(&record, recovered)?;
            changed += 1;
        }
        Ok(changed)
    }
}

fn read_record(
    connection: &Connection,
    id: &str,
    org_id: &str,
) -> Result<Option<OperationRecord>, RuntimeError> {
    let row: Option<(String, String, i64, Option<String>)> = connection.query_row(
        "SELECT fingerprint,phase,revision,CASE WHEN length(CAST(record_json AS BLOB)) <= ?2 THEN record_json ELSE NULL END FROM creao_operations WHERE operation_id=?1",
        params![id, MAX_RECORD_BYTES as i64], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .optional().map_err(|_| RuntimeError::Storage)?;
    let Some((fingerprint, phase, revision, encoded)) = row else {
        return Ok(None);
    };
    let encoded = encoded.ok_or(RuntimeError::Invalid)?;
    let record: OperationRecord =
        decode_json(encoded.as_bytes()).map_err(|_| RuntimeError::Invalid)?;
    record.validate()?;
    if record.spec.operation_id != id
        || record.spec.org_id != org_id
        || record.fingerprint != fingerprint
        || record.phase.label() != phase
        || revision < 0
        || record.revision != revision as u64
    {
        return Err(RuntimeError::Invalid);
    }
    Ok(Some(record))
}

fn encode(record: &OperationRecord) -> Result<String, RuntimeError> {
    record.validate()?;
    let encoded = serde_json::to_string(record).map_err(|_| RuntimeError::Invalid)?;
    if encoded.len() > MAX_RECORD_BYTES {
        return Err(RuntimeError::Invalid);
    }
    Ok(encoded)
}

fn identifier(value: &str) -> bool {
    crate::model::plain(value, 256)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}
fn after(now: DateTime<Utc>, delay: Duration) -> Result<DateTime<Utc>, RuntimeError> {
    now.checked_add_signed(chrono::Duration::from_std(delay).map_err(|_| RuntimeError::Invalid)?)
        .ok_or(RuntimeError::Invalid)
}
fn retryable(error: &Error) -> bool {
    !error.post_may_have_run
        && matches!(
            error.kind,
            ErrorKind::Transport | ErrorKind::DeadlineExceeded | ErrorKind::LimiterUnavailable
        )
        || !error.post_may_have_run
            && error.kind == ErrorKind::HttpStatus
            && error
                .http_status
                .is_some_and(|status| status == 429 || (500..=599).contains(&status))
}
