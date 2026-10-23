//! SQLite persistence for one organization instance.
//!
//! SQLite is the authoritative store (TECH_SPEC §3). Each committed state is
//! written as a snapshot row keyed by revision, together with its events and
//! outbox items, in one transaction. A write whose base revision is no longer
//! current fails with [`StoreError::RevisionConflict`].

use chrono::{DateTime, Utc};
use genesis_core::change::OrgChangeRecord;
use genesis_core::events::{DomainEvent, EventKind, EVENT_SCHEMA_VERSION};
use genesis_core::ids::*;
use genesis_core::jobs::Job;
use genesis_core::model::OrgState;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("revision conflict: expected {expected}, store is at {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("store not initialized")]
    Uninitialized,
    #[error("commit refused, it would break kernel invariants: {0}")]
    InvariantViolation(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Metadata attached to events written by one operation.
#[derive(Debug, Clone, Default)]
pub struct EventMeta {
    pub mission_id: Option<MissionId>,
    pub correlation_id: Option<CorrelationId>,
    pub causation_id: Option<String>,
}

impl EventMeta {
    pub fn mission(mission_id: MissionId) -> Self {
        Self {
            mission_id: Some(mission_id),
            ..Self::default()
        }
    }
    pub fn caused_by(mut self, cause: impl Into<String>) -> Self {
        self.causation_id = Some(cause.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OutboxItem {
    pub id: i64,
    pub destination: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub next_attempt_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// An outbox item to enqueue inside a state transaction.
#[derive(Debug, Clone)]
pub struct NewOutbox {
    pub destination: String,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    org_id: OrgId,
    /// File behind `conn`, so read-only work (the history audit) can open
    /// its own connection without waiting on the shared one.
    path: Option<std::path::PathBuf>,
    /// Refuse any commit whose state or transition breaks a
    /// [`genesis_core::audit`] invariant. Opt-in: `GENESIS_AUDIT_COMMITS=1`.
    audit_commits: bool,
}

const MIGRATIONS: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS org_snapshots (
    revision    INTEGER PRIMARY KEY,
    state_json  TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    seq             INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id        TEXT NOT NULL UNIQUE,
    schema_version  INTEGER NOT NULL,
    org_id          TEXT NOT NULL,
    mission_id      TEXT,
    correlation_id  TEXT,
    causation_id    TEXT,
    revision        INTEGER NOT NULL,
    event_type      TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    created_at      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS org_changes (
    change_id          TEXT PRIMARY KEY,
    client_request_id  TEXT NOT NULL UNIQUE,
    status             TEXT NOT NULL,
    record_json        TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS jobs (
    job_id             TEXT PRIMARY KEY,
    client_request_id  TEXT NOT NULL UNIQUE,
    status             TEXT NOT NULL,
    job_json           TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);
-- Simulated commitment ledger. One row per logical operation.
CREATE TABLE IF NOT EXISTS commitments (
    operation_id  TEXT PRIMARY KEY,
    job_id        TEXT NOT NULL,
    payload_json  TEXT NOT NULL,
    created_at    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS outbox (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    destination      TEXT NOT NULL,
    kind             TEXT NOT NULL,
    payload_json     TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    attempts         INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    next_attempt_at  TEXT NOT NULL,
    created_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS outbox_pending ON outbox(status, next_attempt_at);
-- Idempotency identities for requirement creation. Written in the same
-- transaction as the snapshot that adds the requirement.
CREATE TABLE IF NOT EXISTS requirement_requests (
    client_request_id  TEXT PRIMARY KEY,
    requirement_id     TEXT NOT NULL,
    request_sha256     TEXT NOT NULL,
    created_at         TEXT NOT NULL
);
"#;

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl Store {
    pub fn open(path: impl AsRef<Path>, org_id: OrgId) -> StoreResult<Self> {
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn = Connection::open(path.as_ref())?;
        let mut store = Self::from_conn(conn, org_id)?;
        store.path = Some(path.as_ref().to_path_buf());
        Ok(store)
    }

    pub fn open_in_memory(org_id: OrgId) -> StoreResult<Self> {
        Self::from_conn(Connection::open_in_memory()?, org_id)
    }

    fn from_conn(conn: Connection, org_id: OrgId) -> StoreResult<Self> {
        // Wait briefly for another connection's lock (an operator tool, a
        // second process during a restart) instead of failing the write.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(MIGRATIONS)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            org_id,
            path: None,
            audit_commits: std::env::var("GENESIS_AUDIT_COMMITS").is_ok_and(|v| v == "1"),
        })
    }

    /// Turn commit-time invariant auditing on or off for this handle (and its
    /// clones made afterwards).
    pub fn with_commit_audit(mut self, on: bool) -> Self {
        self.audit_commits = on;
        self
    }

    /// The organization this store belongs to.
    pub fn org_id(&self) -> &OrgId {
        &self.org_id
    }

    /// The database file, or `None` for an in-memory store.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Write revision 0 if the store is empty. Returns the current state.
    pub fn init(&self, initial: &OrgState) -> StoreResult<(OrgState, Vec<DomainEvent>)> {
        let mut conn = self.lock();
        let existing: Option<String> = conn
            .query_row(
                "SELECT state_json FROM org_snapshots ORDER BY revision DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(json) = existing {
            return Ok((serde_json::from_str(&json)?, vec![]));
        }
        let tx = conn.transaction()?;
        insert_snapshot(&tx, initial)?;
        let events = insert_events(
            &tx,
            &self.org_id,
            initial.revision,
            &EventMeta::default(),
            vec![EventKind::OrganizationInitialized {
                name: initial.name.clone(),
            }],
        )?;
        tx.commit()?;
        Ok((initial.clone(), events))
    }

    pub fn current_state(&self) -> StoreResult<OrgState> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT state_json FROM org_snapshots ORDER BY revision DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match json {
            Some(j) => Ok(serde_json::from_str(&j)?),
            None => Err(StoreError::Uninitialized),
        }
    }

    pub fn state_at(&self, revision: u64) -> StoreResult<OrgState> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT state_json FROM org_snapshots WHERE revision = ?1",
                params![revision as i64],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(StoreError::from))
            .unwrap_or_else(|| Err(StoreError::NotFound(format!("revision {revision}"))))
    }

    /// Atomically persist a new state derived from `base_revision`, its events,
    /// outbox items and any extra writes. Fails on revision conflict.
    pub fn commit_state<F>(
        &self,
        base_revision: u64,
        new_state: &OrgState,
        effects: Vec<EventKind>,
        meta: &EventMeta,
        outbox: Vec<NewOutbox>,
        extra: F,
    ) -> StoreResult<Vec<DomainEvent>>
    where
        F: FnOnce(&Transaction<'_>) -> StoreResult<()>,
    {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let actual: i64 = tx.query_row(
            "SELECT COALESCE(MAX(revision), -1) FROM org_snapshots",
            [],
            |r| r.get(0),
        )?;
        if actual != base_revision as i64 {
            return Err(StoreError::RevisionConflict {
                expected: base_revision,
                actual: actual.max(0) as u64,
            });
        }
        if new_state.revision != base_revision {
            if self.audit_commits {
                audit_commit(&tx, base_revision, new_state)?;
            }
            insert_snapshot(&tx, new_state)?;
        }
        let events = insert_events(&tx, &self.org_id, new_state.revision, meta, effects)?;
        for item in outbox {
            enqueue(&tx, &item)?;
        }
        extra(&tx)?;
        tx.commit()?;
        Ok(events)
    }

    /// Append events that do not change organization state (e.g. change status
    /// transitions, provisioning receipts).
    pub fn append_events<F>(
        &self,
        effects: Vec<EventKind>,
        meta: &EventMeta,
        extra: F,
    ) -> StoreResult<Vec<DomainEvent>>
    where
        F: FnOnce(&Transaction<'_>) -> StoreResult<()>,
    {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let revision: i64 = tx.query_row(
            "SELECT COALESCE(MAX(revision), 0) FROM org_snapshots",
            [],
            |r| r.get(0),
        )?;
        let events = insert_events(&tx, &self.org_id, revision as u64, meta, effects)?;
        extra(&tx)?;
        tx.commit()?;
        Ok(events)
    }

    pub fn events_after(&self, after_seq: u64, limit: u32) -> StoreResult<Vec<DomainEvent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT seq, event_id, schema_version, org_id, mission_id, correlation_id, causation_id,
                    revision, event_type, payload_json, created_at
             FROM events WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after_seq as i64, limit as i64], row_to_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    // -- organization changes -------------------------------------------------

    /// Insert a change record, or return the existing record for a repeated
    /// client request ID.
    pub fn insert_change(
        &self,
        client_request_id: &str,
        record: &OrgChangeRecord,
    ) -> StoreResult<(OrgChangeRecord, bool)> {
        let conn = self.lock();
        if let Some(existing) = conn
            .query_row(
                "SELECT record_json FROM org_changes WHERE client_request_id = ?1",
                params![client_request_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok((serde_json::from_str(&existing)?, false));
        }
        conn.execute(
            "INSERT INTO org_changes (change_id, client_request_id, status, record_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                record.change_id.as_str(),
                client_request_id,
                format!("{:?}", record.status),
                serde_json::to_string(record)?,
                ts(record.created_at)
            ],
        )?;
        Ok((record.clone(), true))
    }

    /// The requirement and request digest stored for a client request ID.
    pub fn find_requirement_by_request(
        &self,
        client_request_id: &str,
    ) -> StoreResult<Option<(String, String)>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT requirement_id, request_sha256 FROM requirement_requests WHERE client_request_id = ?1",
                params![client_request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// The change stored for a client request ID, if any.
    pub fn find_change_by_request(
        &self,
        client_request_id: &str,
    ) -> StoreResult<Option<OrgChangeRecord>> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT record_json FROM org_changes WHERE client_request_id = ?1",
                params![client_request_id],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(StoreError::from))
            .transpose()
    }

    pub fn get_change(&self, id: &ChangeId) -> StoreResult<OrgChangeRecord> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT record_json FROM org_changes WHERE change_id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(StoreError::from))
            .unwrap_or_else(|| Err(StoreError::NotFound(format!("change {id}"))))
    }

    pub fn list_changes(&self) -> StoreResult<Vec<OrgChangeRecord>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT record_json FROM org_changes ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r?)?);
        }
        Ok(out)
    }

    // -- jobs -------------------------------------------------------------------

    pub fn insert_job(&self, client_request_id: &str, job: &Job) -> StoreResult<(Job, bool)> {
        let conn = self.lock();
        if let Some(existing) = conn
            .query_row(
                "SELECT job_json FROM jobs WHERE client_request_id = ?1",
                params![client_request_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok((serde_json::from_str(&existing)?, false));
        }
        conn.execute(
            "INSERT INTO jobs (job_id, client_request_id, status, job_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                job.job_id.as_str(),
                client_request_id,
                format!("{:?}", job.status),
                serde_json::to_string(job)?,
                ts(job.created_at)
            ],
        )?;
        Ok((job.clone(), true))
    }

    /// The job stored for a client request ID, if any.
    pub fn find_job_by_request(&self, client_request_id: &str) -> StoreResult<Option<Job>> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT job_json FROM jobs WHERE client_request_id = ?1",
                params![client_request_id],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(StoreError::from))
            .transpose()
    }

    pub fn get_job(&self, id: &JobId) -> StoreResult<Job> {
        let conn = self.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT job_json FROM jobs WHERE job_id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(StoreError::from))
            .unwrap_or_else(|| Err(StoreError::NotFound(format!("job {id}"))))
    }

    pub fn list_jobs(&self) -> StoreResult<Vec<Job>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT job_json FROM jobs ORDER BY created_at DESC LIMIT 200")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r?)?);
        }
        Ok(out)
    }

    pub fn commitment_exists(&self, op: &OperationId) -> StoreResult<bool> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM commitments WHERE operation_id = ?1",
            params![op.as_str()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn commitments(&self) -> StoreResult<Vec<serde_json::Value>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT operation_id, job_id, payload_json, created_at FROM commitments ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "operation_id": r.get::<_, String>(0)?,
                "job_id": r.get::<_, String>(1)?,
                "payload": serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(2)?).unwrap_or_default(),
                "created_at": r.get::<_, String>(3)?,
            }))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // -- outbox -----------------------------------------------------------------

    pub fn due_outbox(&self, now: DateTime<Utc>, limit: u32) -> StoreResult<Vec<OutboxItem>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, destination, kind, payload_json, status, attempts, last_error, next_attempt_at, created_at
             FROM outbox WHERE status = 'pending' AND next_attempt_at <= ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![ts(now), limit as i64], row_to_outbox)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    pub fn list_outbox(&self, limit: u32) -> StoreResult<Vec<OutboxItem>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, destination, kind, payload_json, status, attempts, last_error, next_attempt_at, created_at
             FROM outbox ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_outbox)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    pub fn mark_outbox_delivered(&self, id: i64) -> StoreResult<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE outbox SET status = 'delivered', attempts = attempts + 1, last_error = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_outbox_retry(
        &self,
        id: i64,
        error: &str,
        next: DateTime<Utc>,
        give_up: bool,
    ) -> StoreResult<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2, next_attempt_at = ?3,
                    status = CASE WHEN ?4 THEN 'dead' ELSE 'pending' END
             WHERE id = ?1",
            params![id, error, ts(next), give_up],
        )?;
        Ok(())
    }
}

// -- transaction helpers usable inside `extra` closures ----------------------

/// Audit `next` and the step from the stored `base_revision` to it.
fn audit_commit(tx: &Transaction<'_>, base_revision: u64, next: &OrgState) -> StoreResult<()> {
    let prev: Option<String> = tx
        .query_row(
            "SELECT state_json FROM org_snapshots WHERE revision = ?1",
            params![base_revision as i64],
            |r| r.get(0),
        )
        .optional()?;
    let mut found = genesis_core::audit::audit(next);
    if let Some(prev) = prev {
        let prev: OrgState = serde_json::from_str(&prev)?;
        found.extend(genesis_core::audit::audit_transition(&prev, next));
    }
    if found.is_empty() {
        return Ok(());
    }
    let summary: Vec<String> = found
        .iter()
        .take(5)
        .map(|v| format!("{}: {}", v.invariant, v.detail))
        .collect();
    tracing::error!(
        revision = next.revision,
        violations = found.len(),
        "refusing an ill-formed commit"
    );
    Err(StoreError::InvariantViolation(summary.join("; ")))
}

pub fn insert_snapshot(tx: &Transaction<'_>, state: &OrgState) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO org_snapshots (revision, state_json, created_at) VALUES (?1, ?2, ?3)",
        params![
            state.revision as i64,
            serde_json::to_string(state)?,
            ts(Utc::now())
        ],
    )?;
    Ok(())
}

fn insert_events(
    tx: &Transaction<'_>,
    org_id: &OrgId,
    revision: u64,
    meta: &EventMeta,
    effects: Vec<EventKind>,
) -> StoreResult<Vec<DomainEvent>> {
    let mut out = Vec::with_capacity(effects.len());
    for kind in effects {
        let event_id = EventId::generate();
        let now = Utc::now();
        let payload = serde_json::to_value(&kind)?;
        let payload_only = payload
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        tx.execute(
            "INSERT INTO events (event_id, schema_version, org_id, mission_id, correlation_id, causation_id,
                                 revision, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event_id.as_str(),
                EVENT_SCHEMA_VERSION,
                org_id.as_str(),
                meta.mission_id.as_ref().map(|m| m.as_str().to_owned()),
                meta.correlation_id.as_ref().map(|c| c.as_str().to_owned()),
                meta.causation_id.clone(),
                revision as i64,
                kind.type_name(),
                serde_json::to_string(&payload_only)?,
                ts(now),
            ],
        )?;
        out.push(DomainEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            seq: tx.last_insert_rowid() as u64,
            event_id,
            org_id: org_id.clone(),
            mission_id: meta.mission_id.clone(),
            correlation_id: meta.correlation_id.clone(),
            causation_id: meta.causation_id.clone(),
            revision,
            timestamp: now,
            kind,
        });
    }
    Ok(out)
}

pub fn enqueue(tx: &Transaction<'_>, item: &NewOutbox) -> StoreResult<()> {
    let now = ts(Utc::now());
    tx.execute(
        "INSERT INTO outbox (destination, kind, payload_json, next_attempt_at, created_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![item.destination, item.kind, serde_json::to_string(&item.payload)?, now],
    )?;
    Ok(())
}

/// Bind a client request ID to the requirement created in this transaction.
pub fn insert_requirement_request_tx(
    tx: &Transaction<'_>,
    client_request_id: &str,
    requirement_id: &str,
    request_sha256: &str,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO requirement_requests (client_request_id, requirement_id, request_sha256, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![client_request_id, requirement_id, request_sha256, ts(Utc::now())],
    )?;
    Ok(())
}

/// Insert a new change record inside a transaction (with its events).
pub fn insert_change_tx(
    tx: &Transaction<'_>,
    client_request_id: &str,
    record: &OrgChangeRecord,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO org_changes (change_id, client_request_id, status, record_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            record.change_id.as_str(),
            client_request_id,
            format!("{:?}", record.status),
            serde_json::to_string(record)?,
            ts(record.created_at),
            ts(record.updated_at)
        ],
    )?;
    Ok(())
}

/// Insert a new job inside a transaction (with its dispatch event and outbox).
pub fn insert_job_tx(tx: &Transaction<'_>, client_request_id: &str, job: &Job) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO jobs (job_id, client_request_id, status, job_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            job.job_id.as_str(),
            client_request_id,
            format!("{:?}", job.status),
            serde_json::to_string(job)?,
            ts(job.created_at)
        ],
    )?;
    Ok(())
}

pub fn put_change(tx: &Transaction<'_>, record: &OrgChangeRecord) -> StoreResult<()> {
    let n = tx.execute(
        "UPDATE org_changes SET status = ?2, record_json = ?3, updated_at = ?4 WHERE change_id = ?1",
        params![
            record.change_id.as_str(),
            format!("{:?}", record.status),
            serde_json::to_string(record)?,
            ts(record.updated_at)
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("change {}", record.change_id)));
    }
    Ok(())
}

pub fn put_job(tx: &Transaction<'_>, job: &Job) -> StoreResult<()> {
    let n = tx.execute(
        "UPDATE jobs SET status = ?2, job_json = ?3, updated_at = ?4 WHERE job_id = ?1",
        params![
            job.job_id.as_str(),
            format!("{:?}", job.status),
            serde_json::to_string(job)?,
            ts(job.updated_at)
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("job {}", job.job_id)));
    }
    Ok(())
}

/// Insert a ledger commitment. Returns `false` if the operation already exists
/// (duplicate delivery), in which case nothing is written.
pub fn insert_commitment(
    tx: &Transaction<'_>,
    op: &OperationId,
    job: &JobId,
    payload: &serde_json::Value,
) -> StoreResult<bool> {
    let n = tx.execute(
        "INSERT OR IGNORE INTO commitments (operation_id, job_id, payload_json, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![op.as_str(), job.as_str(), serde_json::to_string(payload)?, ts(Utc::now())],
    )?;
    Ok(n == 1)
}

type RowResult<T> = rusqlite::Result<StoreResult<T>>;

fn row_to_event(r: &rusqlite::Row<'_>) -> RowResult<DomainEvent> {
    let event_type: String = r.get(8)?;
    let payload_json: String = r.get(9)?;
    let built = (|| -> StoreResult<DomainEvent> {
        let payload: serde_json::Value = serde_json::from_str(&payload_json)?;
        let kind: EventKind =
            serde_json::from_value(serde_json::json!({"type": event_type, "payload": payload}))?;
        Ok(DomainEvent {
            seq: r.get::<_, i64>(0)? as u64,
            event_id: EventId::new(r.get::<_, String>(1)?),
            schema_version: r.get(2)?,
            org_id: OrgId::new(r.get::<_, String>(3)?),
            mission_id: r.get::<_, Option<String>>(4)?.map(MissionId::new),
            correlation_id: r.get::<_, Option<String>>(5)?.map(CorrelationId::new),
            causation_id: r.get(6)?,
            revision: r.get::<_, i64>(7)? as u64,
            timestamp: parse_ts(&r.get::<_, String>(10)?),
            kind,
        })
    })();
    Ok(built)
}

fn row_to_outbox(r: &rusqlite::Row<'_>) -> RowResult<OutboxItem> {
    let payload_json: String = r.get(3)?;
    let built = (|| -> StoreResult<OutboxItem> {
        Ok(OutboxItem {
            id: r.get(0)?,
            destination: r.get(1)?,
            kind: r.get(2)?,
            payload: serde_json::from_str(&payload_json)?,
            status: r.get(4)?,
            attempts: r.get::<_, i64>(5)? as u32,
            last_error: r.get(6)?,
            next_attempt_at: parse_ts(&r.get::<_, String>(7)?),
            created_at: parse_ts(&r.get::<_, String>(8)?),
        })
    })();
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::model::Constitution;

    fn store() -> (Store, OrgState) {
        let s = Store::open_in_memory(OrgId::new("org_t")).unwrap();
        let init = OrgState::new(OrgId::new("org_t"), "T", Constitution::default());
        let (st, ev) = s.init(&init).unwrap();
        assert_eq!(ev.len(), 1);
        (s, st)
    }

    #[test]
    fn init_is_idempotent() {
        let (s, st) = store();
        let (again, ev) = s.init(&st).unwrap();
        assert!(ev.is_empty());
        assert_eq!(again, st);
    }

    #[test]
    fn commit_state_detects_revision_conflict() {
        let (s, st) = store();
        let mut next = st.clone();
        next.revision = 1;
        s.commit_state(0, &next, vec![], &EventMeta::default(), vec![], |_| Ok(()))
            .unwrap();
        let mut other = st.clone();
        other.revision = 1;
        let err = s
            .commit_state(0, &other, vec![], &EventMeta::default(), vec![], |_| Ok(()))
            .unwrap_err();
        assert!(matches!(
            err,
            StoreError::RevisionConflict {
                expected: 0,
                actual: 1
            }
        ));
        assert_eq!(s.current_state().unwrap().revision, 1);
    }

    #[test]
    fn events_round_trip_with_sequence() {
        let (s, _) = store();
        let evs = s
            .append_events(
                vec![EventKind::RoleActivated {
                    role_id: RoleId::new("role_1"),
                }],
                &EventMeta::default(),
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(evs[0].seq, 2);
        let back = s.events_after(1, 10).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].kind, evs[0].kind);
        assert_eq!(s.events_after(0, 10).unwrap().len(), 2);
    }

    #[test]
    fn duplicate_commitment_is_ignored() {
        let (s, _) = store();
        let op = OperationId::new("op_1");
        let job = JobId::new("job_1");
        let mut conn = s.lock();
        let tx = conn.transaction().unwrap();
        assert!(insert_commitment(&tx, &op, &job, &serde_json::json!({})).unwrap());
        assert!(!insert_commitment(&tx, &op, &job, &serde_json::json!({})).unwrap());
        tx.commit().unwrap();
    }
}
