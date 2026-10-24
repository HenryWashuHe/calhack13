//! Bounded observational history for organization-owned acquisition records.
//! The journal contains no executable source, oracle details or provider receipts.
//! Neither a snapshot nor a historical phase grants execution or publication.

use crate::capability_acquisition_driver::DriverErrorCode;
use crate::capability_acquisition_store::{AcquisitionDebit, AcquisitionFence, AcquisitionRecord};
use crate::store::Store;
use chrono::{DateTime, Utc};
use genesis_forge::{AcquisitionPhase, ActionKind, Phase, SynthesisPhase};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

const MAX_SNAPSHOT_BYTES: usize = 4 * 1024;
const MAX_ROW_BYTES: usize = 8 * 1024;
const MAX_PAGE_BYTES: usize = 1024 * 1024;
const MAX_LIMIT: usize = 100;
// Preserve existing domain IDs exactly. SQL parameters and JSON escaping handle
// their UTF-8 content; minted acquisition IDs retain the separate ASCII rule.
const MAX_DOMAIN_ID_BYTES: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("invalid acquisition history query")]
    InvalidQuery,
    #[error("acquisition history is not initialized completely")]
    Unavailable,
    #[error("acquisition history not found")]
    NotFound,
    #[error("invalid acquisition history record")]
    InvalidRecord,
    #[error("acquisition history storage failed")]
    Db(#[from] rusqlite::Error),
    #[error("cannot encode acquisition history")]
    Encoding,
}
pub type Result<T> = std::result::Result<T, HistoryError>;

fn default_limit() -> usize {
    25
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageQuery {
    #[serde(default)]
    pub after: u64,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
impl Default for PageQuery {
    fn default() -> Self {
        Self {
            after: 0,
            limit: default_limit(),
        }
    }
}
impl PageQuery {
    fn validate(&self) -> Result<()> {
        if self.after > i64::MAX as u64 || !(1..=MAX_LIMIT).contains(&self.limit) {
            return Err(HistoryError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default)]
    pub after: u64,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub mission_id: Option<String>,
    pub requirement_id: Option<String>,
}
impl Default for ListQuery {
    fn default() -> Self {
        Self {
            after: 0,
            limit: default_limit(),
            mission_id: None,
            requirement_id: None,
        }
    }
}
impl ListQuery {
    fn validate(&self) -> Result<()> {
        PageQuery {
            after: self.after,
            limit: self.limit,
        }
        .validate()?;
        if [&self.mission_id, &self.requirement_id]
            .into_iter()
            .flatten()
            .any(|id| !domain_id(id))
        {
            return Err(HistoryError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Exclusive cursor for another page, present only when more rows existed
    /// in this read. Listing uses ordinal; history uses the global journal seq.
    pub next_after: Option<u64>,
}

/// A small historical checkpoint projection, never a verification token.
/// Existing domain IDs are nonempty UTF-8 strings of at most 256 bytes, retained
/// exactly. The complete serialized snapshot additionally has a 4 KiB bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub id: String,
    pub org_id: String,
    pub mission_id: String,
    pub requirement_id: String,
    pub role_id: String,
    pub responsibility_id: String,
    pub assignment_id: String,
    pub epoch: u64,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub phase: AcquisitionPhase,
    pub generation_phase: SynthesisPhase,
    pub forge_phase: Option<Phase>,
    pub attempt: u8,
    pub pending_action: Option<ActionKind>,
    pub completed_case_count: u32,
    pub fence_reason: Option<AcquisitionFence>,
    pub active: bool,
}
impl Snapshot {
    /// The calling store has already validated the complete checkpoint against
    /// its trusted suite. This projection never copies input or receipt fields.
    fn from_record(record: &AcquisitionRecord, active: bool) -> Result<Self> {
        let binding = record.binding();
        let session = record.session();
        let snapshot = Self {
            id: record.id().into(),
            org_id: binding.org_id.to_string(),
            mission_id: binding.mission_id.to_string(),
            requirement_id: binding.requirement_id.to_string(),
            role_id: binding.role_id.to_string(),
            responsibility_id: binding.responsibility_id.to_string(),
            assignment_id: binding.grant.assignment_id.to_string(),
            epoch: binding.grant.epoch,
            version: record.version(),
            created_at: record.created_at(),
            updated_at: record.updated_at(),
            phase: session.phase(),
            generation_phase: session.generation().phase(),
            forge_phase: session.forge().map(|forge| forge.phase()),
            attempt: u8::try_from(session.attempt()).map_err(|_| HistoryError::InvalidRecord)?,
            pending_action: session.forge().and_then(|forge| forge.pending_action()),
            completed_case_count: u32::try_from(
                session.forge().map_or(0, |forge| forge.completed_cases()),
            )
            .map_err(|_| HistoryError::InvalidRecord)?,
            fence_reason: record.fence_reason(),
            active,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<()> {
        if !safe_id(&self.id)
            || [
                &self.org_id,
                &self.mission_id,
                &self.requirement_id,
                &self.role_id,
                &self.responsibility_id,
                &self.assignment_id,
            ]
            .into_iter()
            .any(|id| !domain_id(id))
            || self.version == 0
            || self.version > i64::MAX as u64
            || self.epoch > i64::MAX as u64
            || self.created_at > self.updated_at
            || self.attempt > 1
            || self.completed_case_count > 128
            || (self.forge_phase.is_none()
                && (self.pending_action.is_some() || self.completed_case_count != 0))
            || (self.fence_reason.is_some() && self.active)
            || (matches!(
                self.phase,
                AcquisitionPhase::Verified
                    | AcquisitionPhase::Failed
                    | AcquisitionPhase::NeedsReconciliation
            ) && self.active)
        {
            return Err(HistoryError::InvalidRecord);
        }
        bounded_json(self, MAX_SNAPSHOT_BYTES)?;
        Ok(())
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.id == other.id
            && self.org_id == other.org_id
            && self.mission_id == other.mission_id
            && self.requirement_id == other.requirement_id
            && self.role_id == other.role_id
            && self.responsibility_id == other.responsibility_id
            && self.assignment_id == other.assignment_id
            && self.epoch == other.epoch
            && self.created_at == other.created_at
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ListItem {
    pub ordinal: u64,
    pub snapshot: Snapshot,
    /// Same-version scheduler attention, not a current authority evaluation.
    pub attention: Option<DriverErrorCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    Created,
    Transition,
    BaselineObserved,
    Attention,
    AttentionCleared,
}
impl HistoryKind {
    fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Transition => "transition",
            Self::BaselineObserved => "baseline_observed",
            Self::Attention => "attention",
            Self::AttentionCleared => "attention_cleared",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub seq: u64,
    pub kind: HistoryKind,
    /// Organization revision associated with this checkpoint. Attention rows
    /// retain that context; they do not claim to snapshot current organization state.
    pub checkpoint_org_revision: u64,
    pub recorded_at: DateTime<Utc>,
    pub snapshot: Snapshot,
    pub debit: Option<AcquisitionDebit>,
    pub attention: Option<DriverErrorCode>,
}
impl HistoryEntry {
    fn validate(&self) -> Result<()> {
        self.snapshot.validate()?;
        if self.seq == 0
            || self.seq > i64::MAX as u64
            || self.checkpoint_org_revision > i64::MAX as u64
            || (self.kind != HistoryKind::BaselineObserved
                && self.recorded_at < self.snapshot.updated_at)
            || (self.kind == HistoryKind::Attention) != self.attention.is_some()
            || (self.kind != HistoryKind::Transition && self.debit.is_some())
            || (self.kind == HistoryKind::Created && self.snapshot.version != 1)
        {
            return Err(HistoryError::InvalidRecord);
        }
        bounded_json(self, MAX_ROW_BYTES)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckpointKind {
    Created,
    Transition,
    BaselineObserved,
}
impl From<CheckpointKind> for HistoryKind {
    fn from(kind: CheckpointKind) -> Self {
        match kind {
            CheckpointKind::Created => Self::Created,
            CheckpointKind::Transition => Self::Transition,
            CheckpointKind::BaselineObserved => Self::BaselineObserved,
        }
    }
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}
fn domain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_DOMAIN_ID_BYTES
}
fn bounded_json<T: Serialize>(value: &T, max: usize) -> Result<String> {
    let json = serde_json::to_string(value).map_err(|_| HistoryError::Encoding)?;
    if json.len() > max {
        return Err(HistoryError::InvalidRecord);
    }
    Ok(json)
}
fn decode<T: DeserializeOwned>(raw: Option<String>, max: usize) -> Result<T> {
    let raw = raw.ok_or(HistoryError::InvalidRecord)?;
    if raw.len() > max {
        return Err(HistoryError::InvalidRecord);
    }
    genesis_verification::decode_json(raw.as_bytes()).map_err(|_| HistoryError::InvalidRecord)
}
fn positive(raw: i64) -> Result<u64> {
    u64::try_from(raw)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HistoryError::InvalidRecord)
}
fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        params![name],
        |row| row.get(0),
    )?)
}

pub(crate) fn require_ready(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "acquisition_history_meta")? {
        return Err(HistoryError::Unavailable);
    }
    let marker: Option<(i64, i64)> = conn
        .query_row(
            "SELECT schema_version,ready FROM acquisition_history_meta WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match marker {
        Some((1, 1)) => Ok(()),
        Some((1, 0)) | None => Err(HistoryError::Unavailable),
        _ => Err(HistoryError::InvalidRecord),
    }
}

fn coverage(conn: &Connection) -> Result<bool> {
    if !table_exists(conn, "capability_acquisitions")? {
        return Ok(true);
    }
    Ok(!conn.query_row::<bool, _, _>(
        "SELECT EXISTS(SELECT 1 FROM capability_acquisitions a
         LEFT JOIN acquisition_catalog c ON c.id=a.id
         WHERE c.id IS NULL OR c.version<>a.version)",
        [],
        |row| row.get(0),
    )?)
}

pub(crate) fn mark_ready(tx: &Transaction<'_>) -> Result<()> {
    if !coverage(tx)? {
        return Err(HistoryError::Unavailable);
    }
    if tx.execute(
        "UPDATE acquisition_history_meta SET ready=1 WHERE singleton=1 AND schema_version=1",
        [],
    )? != 1
    {
        return Err(HistoryError::InvalidRecord);
    }
    Ok(())
}

#[derive(Debug)]
struct Catalog {
    ordinal: u64,
    snapshot: Snapshot,
    org_revision: u64,
    last_seq: u64,
    attention: Option<DriverErrorCode>,
}

fn catalog(conn: &Connection, id: &str) -> Result<Option<Catalog>> {
    type Raw = (
        i64,
        Option<String>,
        Option<String>,
        i64,
        i64,
        i64,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let row: Option<Raw> = conn.query_row(
        "SELECT ordinal,
         CASE WHEN length(CAST(mission_id AS BLOB))<=256 THEN mission_id END,
         CASE WHEN length(CAST(requirement_id AS BLOB))<=256 THEN requirement_id END,
         version,org_revision,last_seq,
         CASE WHEN length(CAST(snapshot_json AS BLOB))<=?2 THEN snapshot_json END,
         attention_version,CASE WHEN length(CAST(attention_code AS BLOB))<=64 THEN attention_code END FROM acquisition_catalog WHERE id=?1",
        params![id, MAX_SNAPSHOT_BYTES as i64],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?)),
    ).optional()?;
    row.map(|(ordinal, mission, requirement, version, revision, last, json, attention_version, attention)| {
        let snapshot: Snapshot = decode(json, MAX_SNAPSHOT_BYTES)?;
        snapshot.validate()?;
        let version = positive(version)?;
        if snapshot.id != id || Some(&snapshot.mission_id) != mission.as_ref() || Some(&snapshot.requirement_id) != requirement.as_ref() || snapshot.version != version {
            return Err(HistoryError::InvalidRecord);
        }
        let attention = match (attention_version, attention) {
            (None, None) => None,
            (Some(v), Some(code)) if positive(v)? == version => Some(decode::<DriverErrorCode>(Some(code), 64)?),
            _ => return Err(HistoryError::InvalidRecord),
        };
        let last_seq=positive(last)?;
        let tail_exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM acquisition_history WHERE seq=?1 AND acquisition_id=?2 AND record_version=?3)",params![last,id,version as i64],|row|row.get(0))?;
        if !tail_exists { return Err(HistoryError::InvalidRecord); }
        Ok(Catalog { ordinal: positive(ordinal)?, snapshot, org_revision: u64::try_from(revision).map_err(|_|HistoryError::InvalidRecord)?, last_seq, attention })
    }).transpose()
}

fn current_index(conn: &Connection, id: &str, expected_version: u64) -> Result<()> {
    if !table_exists(conn, "capability_acquisitions")? {
        return Err(HistoryError::InvalidRecord);
    }
    let current: Option<i64> = conn
        .query_row(
            "SELECT version FROM capability_acquisitions WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    if current.map(positive).transpose()? != Some(expected_version) {
        return Err(HistoryError::InvalidRecord);
    }
    Ok(())
}

impl Store {
    /// Schema creation is independent of provider configuration. Old records
    /// remain unavailable until a trusted bootstrap observes their current state.
    pub fn init_acquisition_history(&self) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS acquisition_history_meta (
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL,ready INTEGER NOT NULL CHECK(ready IN(0,1)));
             CREATE TABLE IF NOT EXISTS acquisition_catalog (
                ordinal INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,
                mission_id TEXT NOT NULL,requirement_id TEXT NOT NULL,version INTEGER NOT NULL,
                org_revision INTEGER NOT NULL,last_seq INTEGER NOT NULL,snapshot_json TEXT NOT NULL,
                attention_version INTEGER,attention_code TEXT);
             CREATE INDEX IF NOT EXISTS acquisition_catalog_mission ON acquisition_catalog(mission_id,ordinal);
             CREATE INDEX IF NOT EXISTS acquisition_catalog_requirement ON acquisition_catalog(requirement_id,ordinal);
             CREATE TABLE IF NOT EXISTS acquisition_history (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,acquisition_id TEXT NOT NULL REFERENCES acquisition_catalog(id),
                record_version INTEGER NOT NULL,event_kind TEXT NOT NULL,payload_json TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS acquisition_history_page ON acquisition_history(acquisition_id,seq);
             INSERT OR IGNORE INTO acquisition_history_meta(singleton,schema_version,ready) VALUES(1,1,0);",
        )?;
        let schema: i64 = tx.query_row(
            "SELECT schema_version FROM acquisition_history_meta WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if schema != 1 {
            return Err(HistoryError::InvalidRecord);
        }
        tx.execute(
            "UPDATE acquisition_history_meta SET ready=?1 WHERE singleton=1",
            params![coverage(&tx)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Read only the small catalog projection. Ordinals do not change when a
    /// checkpoint advances, so concurrent updates cannot skip or duplicate items.
    pub fn list_acquisitions(&self, query: &ListQuery) -> Result<Page<ListItem>> {
        query.validate()?;
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        require_ready(&tx)?;
        let mut statement = tx.prepare(
            "SELECT CASE WHEN length(CAST(id AS BLOB))<=256 THEN id END,ordinal FROM acquisition_catalog WHERE ordinal>?1
             AND (?2 IS NULL OR mission_id=?2) AND (?3 IS NULL OR requirement_id=?3)
             ORDER BY ordinal LIMIT ?4",
        )?;
        let ids = statement.query_map(
            params![
                query.after as i64,
                query.mission_id,
                query.requirement_id,
                (query.limit + 1) as i64
            ],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let mut items = Vec::with_capacity(query.limit + 1);
        for row in ids {
            let (id, ordinal) = row?;
            let id = id.ok_or(HistoryError::InvalidRecord)?;
            let saved = catalog(&tx, &id)?.ok_or(HistoryError::InvalidRecord)?;
            current_index(&tx, &id, saved.snapshot.version)?;
            if saved.ordinal != positive(ordinal)? {
                return Err(HistoryError::InvalidRecord);
            }
            items.push(ListItem {
                ordinal: saved.ordinal,
                snapshot: saved.snapshot,
                attention: saved.attention,
            });
        }
        let next_after = (items.len() > query.limit).then(|| items[query.limit - 1].ordinal);
        items.truncate(query.limit);
        let page = Page { items, next_after };
        bounded_json(&page, MAX_PAGE_BYTES)?;
        Ok(page)
    }

    pub fn acquisition_history(&self, id: &str, query: &PageQuery) -> Result<Page<HistoryEntry>> {
        query.validate()?;
        if !safe_id(id) {
            return Err(HistoryError::InvalidQuery);
        }
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        require_ready(&tx)?;
        let saved = catalog(&tx, id)?.ok_or(HistoryError::NotFound)?;
        current_index(&tx, id, saved.snapshot.version)?;
        let mut statement = tx.prepare(
            "SELECT seq,record_version,substr(event_kind,1,64),
             CASE WHEN length(CAST(payload_json AS BLOB))<=?4 THEN payload_json END
             FROM acquisition_history WHERE acquisition_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                id,
                query.after as i64,
                (query.limit + 1) as i64,
                MAX_ROW_BYTES as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )?;
        let mut items = Vec::with_capacity(query.limit + 1);
        for row in rows {
            let (seq, version, kind, json) = row?;
            let entry: HistoryEntry = decode(json, MAX_ROW_BYTES)?;
            entry.validate()?;
            if entry.seq != positive(seq)?
                || entry.snapshot.version != positive(version)?
                || entry.snapshot.id != id
                || entry.kind.label() != kind
                || entry.seq > saved.last_seq
                || entry.snapshot.version > saved.snapshot.version
                || !entry.snapshot.same_identity(&saved.snapshot)
            {
                return Err(HistoryError::InvalidRecord);
            }
            items.push(entry);
        }
        let next_after = (items.len() > query.limit).then(|| items[query.limit - 1].seq);
        items.truncate(query.limit);
        let page = Page { items, next_after };
        bounded_json(&page, MAX_PAGE_BYTES)?;
        Ok(page)
    }
}

/// Caller owns the checkpoint/quota transaction and has validated the record
/// against its trusted suite. A journal failure rolls back that entire mutation.
pub(crate) fn append_checkpoint(
    tx: &Transaction<'_>,
    record: &AcquisitionRecord,
    org_revision: u64,
    kind: CheckpointKind,
    debit: Option<AcquisitionDebit>,
    active: bool,
    recorded_at: DateTime<Utc>,
) -> Result<()> {
    append_snapshot(
        tx,
        Snapshot::from_record(record, active)?,
        org_revision,
        kind,
        debit,
        recorded_at,
    )
}

fn append_snapshot(
    tx: &Transaction<'_>,
    snapshot: Snapshot,
    org_revision: u64,
    kind: CheckpointKind,
    debit: Option<AcquisitionDebit>,
    recorded_at: DateTime<Utc>,
) -> Result<()> {
    snapshot.validate()?;
    if org_revision > i64::MAX as u64
        || (kind != CheckpointKind::BaselineObserved && recorded_at < snapshot.updated_at)
    {
        return Err(HistoryError::InvalidRecord);
    }
    if kind != CheckpointKind::BaselineObserved {
        require_ready(tx)?;
    }
    current_index(tx, &snapshot.id, snapshot.version)?;
    let previous = catalog(tx, &snapshot.id)?;
    match (kind, previous.as_ref()) {
        (CheckpointKind::BaselineObserved, Some(old))
            if old.snapshot == snapshot && old.org_revision == org_revision && debit.is_none() =>
        {
            return Ok(());
        }
        (CheckpointKind::Transition, Some(old))
            if old.snapshot.same_identity(&snapshot)
                && snapshot.version == old.snapshot.version + 1
                && snapshot.updated_at >= old.snapshot.updated_at
                && org_revision >= old.org_revision => {}
        (CheckpointKind::Created, None) if snapshot.version == 1 && debit.is_none() => {}
        (CheckpointKind::BaselineObserved, None) if debit.is_none() => {}
        _ => return Err(HistoryError::InvalidRecord),
    }
    let json = bounded_json(&snapshot, MAX_SNAPSHOT_BYTES)?;
    tx.execute(
        "INSERT INTO acquisition_catalog(id,mission_id,requirement_id,version,org_revision,last_seq,snapshot_json)
         VALUES(?1,?2,?3,?4,?5,0,?6) ON CONFLICT(id) DO UPDATE SET
         version=excluded.version,org_revision=excluded.org_revision,snapshot_json=excluded.snapshot_json,
         attention_version=NULL,attention_code=NULL",
        params![snapshot.id, snapshot.mission_id, snapshot.requirement_id, snapshot.version as i64, org_revision as i64, json],
    )?;
    insert_entry(
        tx,
        HistoryEntry {
            seq: 0,
            kind: kind.into(),
            checkpoint_org_revision: org_revision,
            recorded_at,
            snapshot,
            debit,
            attention: None,
        },
    )
}

fn insert_entry(tx: &Transaction<'_>, mut entry: HistoryEntry) -> Result<()> {
    tx.execute(
        "INSERT INTO acquisition_history(acquisition_id,record_version,event_kind,payload_json) VALUES(?1,?2,?3,'')",
        params![entry.snapshot.id, entry.snapshot.version as i64, entry.kind.label()],
    )?;
    entry.seq = positive(tx.last_insert_rowid())?;
    entry.validate()?;
    let json = bounded_json(&entry, MAX_ROW_BYTES)?;
    tx.execute(
        "UPDATE acquisition_history SET payload_json=?1 WHERE seq=?2",
        params![json, entry.seq as i64],
    )?;
    tx.execute(
        "UPDATE acquisition_catalog SET last_seq=?1 WHERE id=?2",
        params![entry.seq as i64, entry.snapshot.id],
    )?;
    Ok(())
}

/// Scheduler clock refreshes are not history. Only a same-version attention
/// change is appended, including an explicit clear after a prior error.
pub(crate) fn append_attention(
    tx: &Transaction<'_>,
    id: &str,
    version: u64,
    error: Option<DriverErrorCode>,
    at: DateTime<Utc>,
) -> Result<()> {
    require_ready(tx)?;
    let saved = catalog(tx, id)?.ok_or(HistoryError::InvalidRecord)?;
    current_index(tx, id, version)?;
    if saved.snapshot.version != version || at < saved.snapshot.updated_at {
        return Err(HistoryError::InvalidRecord);
    }
    if saved.attention == error {
        return Ok(());
    }
    tx.execute(
        "UPDATE acquisition_catalog SET attention_version=?1,attention_code=?2 WHERE id=?3",
        params![
            error.map(|_| version as i64),
            error.map(|code| bounded_json(&code, 64)).transpose()?,
            id
        ],
    )?;
    insert_entry(
        tx,
        HistoryEntry {
            seq: 0,
            kind: if error.is_some() {
                HistoryKind::Attention
            } else {
                HistoryKind::AttentionCleared
            },
            checkpoint_org_revision: saved.org_revision,
            recorded_at: at,
            snapshot: saved.snapshot,
            debit: None,
            attention: error,
        },
    )
}

#[cfg(test)]
mod tests;
