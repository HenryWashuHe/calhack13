//! Durable recruitment and task evidence. A native contract is not a kernel
//! assignment, and collected output never grants authority or business success.
#[path = "receipt.rs"]
mod receipt;
pub use receipt::{RecruitmentReceipt, TaskReceipt};

use crate::protocol::*;
use chrono::{DateTime, Utc};
use genesis_verification::{decode_json, json_sha256};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("invalid Fetch operation or checkpoint")]
    Invalid,
    #[error("Fetch operation conflicts with saved identity or state")]
    Conflict,
    #[error("Fetch runtime storage unavailable")]
    Storage,
    #[error("Fetch contract not found")]
    NotFound,
    #[error("Fetch runtime capacity exceeded")]
    Capacity,
}
impl From<ProtocolError> for RuntimeError {
    fn from(_: ProtocolError) -> Self {
        Self::Invalid
    }
}
impl From<rusqlite::Error> for RuntimeError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractPhase {
    Requested,
    Offered,
    Accepting,
    Active,
    Released,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Sent,
    Accepted,
    Collected,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    pub request: Message,
    pub phase: TaskPhase,
    pub evidence: Option<Message>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub peer: String,
    pub phase: ContractPhase,
    pub recruit: Message,
    pub offer: Option<Message>,
    pub acceptance: Option<Message>,
    pub release: Option<Message>,
    pub release_ack: Option<Message>,
    pub tasks: BTreeMap<String, TaskRecord>,
    pub revision: u64,
}
impl Contract {
    pub fn lease(&self) -> DateTime<Utc> {
        if let Some(Message {
            body: Body::Offer {
                lease_expires_at, ..
            },
            ..
        }) = &self.offer
        {
            *lease_expires_at
        } else if let Body::RecruitRequest {
            lease_expires_at, ..
        } = self.recruit.body
        {
            lease_expires_at
        } else {
            self.recruit.expires_at
        }
    }
    fn context(&self, message: &Message) -> bool {
        message.org_id == self.recruit.org_id
            && message.mission_id == self.recruit.mission_id
            && message.contract_id == self.recruit.contract_id
            && message.correlation_id == self.recruit.correlation_id
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum Disposition {
    Applied,
    Duplicate,
    Rejected { reason: String },
}
#[derive(Debug, Clone, Serialize)]
pub struct Ingested {
    pub message_id: String,
    pub disposition: Disposition,
}

pub struct RuntimeStore {
    connection: Mutex<Connection>,
    org_id: String,
    bridge_address: String,
}
impl RuntimeStore {
    pub fn open(
        path: impl AsRef<Path>,
        org_id: &str,
        bridge_address: &str,
    ) -> Result<Self, RuntimeError> {
        let path = path.as_ref();
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map_err(|_| RuntimeError::Storage)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| RuntimeError::Storage)?;
        }
        Self::init(Connection::open(path)?, org_id, bridge_address)
    }
    pub fn in_memory(org_id: &str, bridge_address: &str) -> Result<Self, RuntimeError> {
        Self::init(Connection::open_in_memory()?, org_id, bridge_address)
    }
    fn init(
        mut connection: Connection,
        org_id: &str,
        bridge_address: &str,
    ) -> Result<Self, RuntimeError> {
        if !identifier(org_id) || !address(bridge_address) {
            return Err(RuntimeError::Invalid);
        }
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS fetch_meta (key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS fetch_contracts (id TEXT PRIMARY KEY,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS fetch_outbox (id TEXT PRIMARY KEY,contract_id TEXT NOT NULL,peer TEXT NOT NULL,data TEXT NOT NULL,digest TEXT NOT NULL,queued INTEGER NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS fetch_inbox (id TEXT PRIMARY KEY,digest TEXT NOT NULL,disposition TEXT NOT NULL,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS fetch_rejected (sequence INTEGER PRIMARY KEY,data TEXT NOT NULL);")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (key, value) in [("schema", "1"), ("org", org_id), ("bridge", bridge_address)] {
            tx.execute(
                "INSERT OR IGNORE INTO fetch_meta VALUES (?1,?2)",
                params![key, value],
            )?;
            let saved: String =
                tx.query_row("SELECT value FROM fetch_meta WHERE key=?1", [key], |r| {
                    r.get(0)
                })?;
            if saved != value {
                return Err(RuntimeError::Conflict);
            }
        }
        tx.execute("INSERT OR IGNORE INTO fetch_meta VALUES ('cursor','0')", [])?;
        tx.commit()?;
        Ok(Self {
            connection: Mutex::new(connection),
            org_id: org_id.into(),
            bridge_address: bridge_address.into(),
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, Connection>, RuntimeError> {
        self.connection.lock().map_err(|_| RuntimeError::Storage)
    }
    pub fn org_id(&self) -> &str {
        &self.org_id
    }
    pub fn bridge_address(&self) -> &str {
        &self.bridge_address
    }
    pub fn cursor(&self) -> Result<u64, RuntimeError> {
        cursor(&*self.lock()?)
    }
    pub fn get(&self, id: &str) -> Result<Contract, RuntimeError> {
        load(&*self.lock()?, id)?.ok_or(RuntimeError::NotFound)
    }

    /// Saves both the intent and its outgoing message in one transaction. The
    /// caller must separately authorize the current kernel assignment/grant.
    pub fn prepare(
        &self,
        peer: &str,
        message: Message,
        now: DateTime<Utc>,
    ) -> Result<Contract, RuntimeError> {
        message.validate()?;
        if !address(peer) || peer == self.bridge_address || message.org_id != self.org_id {
            return Err(RuntimeError::Invalid);
        }
        let digest = send_digest(peer, &message)?;
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(saved) = tx
            .query_row(
                "SELECT digest FROM fetch_outbox WHERE id=?1",
                [&message.message_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            if saved != digest {
                return Err(RuntimeError::Conflict);
            }
            return load(&tx, &message.contract_id)?.ok_or(RuntimeError::Invalid);
        }
        if message.expires_at <= now {
            return Err(RuntimeError::Invalid);
        }
        let control = matches!(message.body, Body::ReleaseContract { .. });
        capacity_limit(&tx, "fetch_outbox", if control { 20_000 } else { 10_000 })?;
        let mut contract = load(&tx, &message.contract_id)?;
        match &message.body {
            Body::RecruitRequest {
                lease_expires_at, ..
            } => {
                if contract.is_some() || *lease_expires_at <= now {
                    return Err(RuntimeError::Conflict);
                }
                capacity(&tx, "fetch_contracts")?;
                contract = Some(Contract {
                    peer: peer.into(),
                    phase: ContractPhase::Requested,
                    recruit: message.clone(),
                    offer: None,
                    acceptance: None,
                    release: None,
                    release_ack: None,
                    tasks: BTreeMap::new(),
                    revision: 0,
                });
            }
            _ => {
                let c = contract.as_mut().ok_or(RuntimeError::NotFound)?;
                if c.peer != peer || !c.context(&message) {
                    return Err(RuntimeError::Conflict);
                }
                if !matches!(message.body, Body::ReleaseContract { .. })
                    && (now >= c.lease() || message.expires_at > c.lease())
                {
                    return Err(RuntimeError::Conflict);
                }
                match &message.body {
                    Body::AcceptOffer {
                        offer_message_id,
                        offer_id,
                    } => {
                        let offer = c.offer.as_ref().ok_or(RuntimeError::Conflict)?;
                        if c.phase != ContractPhase::Offered
                            || offer.message_id != *offer_message_id
                            || !matches!(&offer.body,Body::Offer{offer_id:id,..} if id==offer_id)
                        {
                            return Err(RuntimeError::Conflict);
                        }
                        c.acceptance = Some(message.clone());
                        c.phase = ContractPhase::Accepting;
                    }
                    Body::TaskRequest { task } => {
                        if c.phase != ContractPhase::Active
                            || task.deadline <= now
                            || c.tasks.len() >= 128
                        {
                            return Err(RuntimeError::Conflict);
                        }
                        if c.tasks.values().any(|r|matches!(&r.request.body,Body::TaskRequest{task:old} if (old.job_id==task.job_id&&old.attempt_id==task.attempt_id)||old.idempotency_key==task.idempotency_key)) {return Err(RuntimeError::Conflict)}
                        c.tasks.insert(
                            message.message_id.clone(),
                            TaskRecord {
                                request: message.clone(),
                                phase: TaskPhase::Sent,
                                evidence: None,
                            },
                        );
                    }
                    Body::ReleaseContract { .. } => {
                        if c.phase == ContractPhase::Released {
                            return Err(RuntimeError::Conflict);
                        }
                        c.release = Some(message.clone());
                        c.phase = ContractPhase::Released;
                    }
                    _ => return Err(RuntimeError::Invalid),
                }
            }
        }
        let mut contract = contract.ok_or(RuntimeError::Invalid)?;
        contract.revision = contract
            .revision
            .checked_add(1)
            .ok_or(RuntimeError::Invalid)?;
        save(&tx, &contract)?;
        let data = encode(&message)?;
        tx.execute(
            "INSERT INTO fetch_outbox(id,contract_id,peer,data,digest) VALUES (?1,?2,?3,?4,?5)",
            params![message.message_id, message.contract_id, peer, data, digest],
        )?;
        storage_capacity(&tx, control)?;
        tx.commit()?;
        Ok(contract)
    }

    /// Only currently relevant outgoing intents are eligible. Release fences
    /// queued work immediately, before any native delivery acknowledgment.
    pub fn pending(&self, now: DateTime<Utc>) -> Result<Vec<SendRequest>, RuntimeError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT peer,data FROM fetch_outbox WHERE queued=0 ORDER BY rowid")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut result = Vec::new();
        for row in rows {
            let (peer, data) = row?;
            let message = Message::decode(data.as_bytes())?;
            let c = load(&connection, &message.contract_id)?.ok_or(RuntimeError::Invalid)?;
            if message.expires_at <= now {
                continue;
            }
            let eligible = match &message.body {
                Body::RecruitRequest { .. } => c.phase == ContractPhase::Requested,
                Body::AcceptOffer { .. } => c.phase == ContractPhase::Accepting,
                Body::TaskRequest { task } => {
                    c.phase == ContractPhase::Active
                        && now < c.lease()
                        && now < task.deadline
                        && c.tasks
                            .get(&message.message_id)
                            .is_some_and(|t| t.phase == TaskPhase::Sent)
                }
                Body::ReleaseContract { .. } => c.phase == ContractPhase::Released,
                _ => false,
            };
            if eligible {
                result.push(SendRequest {
                    schema_version: 1,
                    recipient: peer,
                    message,
                });
            }
            if result.len() == 50 {
                break;
            }
        }
        Ok(result)
    }
    pub fn acknowledge(&self, receipt: &SendReceipt) -> Result<(), RuntimeError> {
        if receipt.schema_version != 1
            || receipt.bridge_address != self.bridge_address
            || receipt.state != "queued"
        {
            return Err(RuntimeError::Invalid);
        }
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE fetch_outbox SET queued=1 WHERE id=?1 AND peer=?2",
            params![receipt.message_id, receipt.recipient],
        )?;
        if changed != 1 {
            return Err(RuntimeError::Conflict);
        }
        Ok(())
    }

    /// Saves all observations, their dispositions, the contract changes and the
    /// polling cursor atomically. Invalid semantic replies are quarantined so one
    /// peer cannot block later messages. A malformed transport page is rejected.
    pub fn ingest(
        &self,
        page: &InboxPage,
        now: DateTime<Utc>,
    ) -> Result<Vec<Ingested>, RuntimeError> {
        if page.schema_version != 1 || page.address != self.bridge_address || page.items.len() > 50
        {
            return Err(RuntimeError::Invalid);
        }
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut last = cursor(&tx)?;
        for item in &page.items {
            item.message.validate()?;
            if !address(&item.sender) || item.sequence <= last || item.sequence > i64::MAX as u64 {
                return Err(RuntimeError::Invalid);
            }
            last = item.sequence;
        }
        if page.next_cursor != last {
            return Err(RuntimeError::Conflict);
        }
        let mut results = Vec::new();
        for item in &page.items {
            let digest = send_digest(&item.sender, &item.message)?;
            let old: Option<String> = tx
                .query_row(
                    "SELECT digest FROM fetch_inbox WHERE id=?1",
                    [&item.message.message_id],
                    |r| r.get(0),
                )
                .optional()?;
            let disposition = if let Some(old) = old {
                if old == digest {
                    Disposition::Duplicate
                } else {
                    {
                        capacity(&tx, "fetch_rejected")?;
                        tx.execute(
                            "INSERT INTO fetch_rejected VALUES (?1,?2)",
                            params![item.sequence, encode(item)?],
                        )?;
                        reject("message_identity_conflict")
                    }
                }
            } else {
                capacity(&tx, "fetch_inbox")?;
                let disposition = apply(&tx, &self.org_id, item, now)?;
                tx.execute(
                    "INSERT INTO fetch_inbox VALUES (?1,?2,?3,?4)",
                    params![
                        item.message.message_id,
                        digest,
                        encode(&disposition)?,
                        encode(item)?
                    ],
                )?;
                disposition
            };
            results.push(Ingested {
                message_id: item.message.message_id.clone(),
                disposition,
            });
        }
        tx.execute(
            "UPDATE fetch_meta SET value=?1 WHERE key='cursor'",
            [last.to_string()],
        )?;
        storage_capacity(&tx, false)?;
        tx.commit()?;
        Ok(results)
    }
}
fn reject(reason: &str) -> Disposition {
    Disposition::Rejected {
        reason: reason.into(),
    }
}
fn apply(
    tx: &Transaction<'_>,
    org: &str,
    item: &InboxItem,
    now: DateTime<Utc>,
) -> Result<Disposition, RuntimeError> {
    let m = &item.message;
    if m.org_id != org {
        return Ok(reject("wrong_organization"));
    }
    let Some(mut c) = load(tx, &m.contract_id)? else {
        return Ok(reject("unknown_contract"));
    };
    if c.peer != item.sender || !c.context(m) {
        return Ok(reject("wrong_sender_or_context"));
    }
    if m.expires_at <= now {
        return Ok(reject("expired_message"));
    }
    if !matches!(m.body, Body::ContractReleased { .. })
        && (c.phase == ContractPhase::Released || now >= c.lease() || m.expires_at > c.lease())
    {
        return Ok(reject("inactive_contract"));
    }
    let valid = match &m.body {
        Body::Offer {
            recruit_message_id,
            capabilities,
            lease_expires_at,
            ..
        } => {
            let required = match &c.recruit.body {
                Body::RecruitRequest { capabilities, .. } => capabilities,
                _ => return Err(RuntimeError::Invalid),
            };
            if c.phase != ContractPhase::Requested
                || recruit_message_id != &c.recruit.message_id
                || *lease_expires_at > c.lease()
                || *lease_expires_at <= now
                || required.iter().any(|r| !capabilities.contains(r))
            {
                false
            } else {
                c.offer = Some(m.clone());
                c.phase = ContractPhase::Offered;
                true
            }
        }
        Body::ContractAccepted {
            accept_message_id,
            offer_id,
        } => {
            if c.phase != ContractPhase::Accepting
                || !c.acceptance.as_ref().is_some_and(|a| {
                    a.message_id == *accept_message_id
                        && matches!(&a.body,Body::AcceptOffer{offer_id:id,..} if id==offer_id)
                })
            {
                false
            } else {
                c.phase = ContractPhase::Active;
                true
            }
        }
        Body::TaskAccepted {
            task_request_id,
            job_id,
            attempt_id,
        }
        | Body::TaskProgress {
            task_request_id,
            job_id,
            attempt_id,
            ..
        }
        | Body::TaskResult {
            task_request_id,
            job_id,
            attempt_id,
            ..
        } => {
            if c.phase != ContractPhase::Active {
                false
            } else if let Some(record) = c.tasks.get_mut(task_request_id) {
                let task = match &record.request.body {
                    Body::TaskRequest { task } => task,
                    _ => return Err(RuntimeError::Invalid),
                };
                if &task.job_id != job_id
                    || &task.attempt_id != attempt_id
                    || task.deadline <= now
                    || m.expires_at > record.request.expires_at
                    || record.phase == TaskPhase::Collected
                    || matches!(&m.body,Body::TaskResult{epoch,..} if *epoch!=task.epoch)
                {
                    false
                } else {
                    record.phase = if matches!(m.body, Body::TaskResult { .. }) {
                        TaskPhase::Collected
                    } else {
                        TaskPhase::Accepted
                    };
                    record.evidence = Some(m.clone());
                    true
                }
            } else {
                false
            }
        }
        Body::ContractReleased { release_message_id } => {
            let matches = c.phase == ContractPhase::Released
                && c.release
                    .as_ref()
                    .is_some_and(|r| r.message_id == *release_message_id);
            if matches {
                c.release_ack = Some(m.clone());
            }
            matches
        }
        _ => false,
    };
    if !valid {
        return Ok(reject("unexpected_or_stale_message"));
    }
    c.revision = c.revision.checked_add(1).ok_or(RuntimeError::Invalid)?;
    save(tx, &c)?;
    Ok(Disposition::Applied)
}
fn cursor(connection: &Connection) -> Result<u64, RuntimeError> {
    connection
        .query_row("SELECT value FROM fetch_meta WHERE key='cursor'", [], |r| {
            r.get::<_, String>(0)
        })?
        .parse()
        .map_err(|_| RuntimeError::Invalid)
}
fn encode<T: Serialize>(value: &T) -> Result<String, RuntimeError> {
    let data = serde_json::to_string(value).map_err(|_| RuntimeError::Invalid)?;
    if data.len() > 8 * 1024 * 1024 - 64 * 1024 {
        Err(RuntimeError::Capacity)
    } else {
        Ok(data)
    }
}
fn send_digest(peer: &str, message: &Message) -> Result<String, RuntimeError> {
    Ok(json_sha256(
        &serde_json::json!({"sender_or_recipient":peer,"message":message}),
    ))
}
fn load(connection: &Connection, id: &str) -> Result<Option<Contract>, RuntimeError> {
    let data: Option<String> = connection
        .query_row("SELECT data FROM fetch_contracts WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    data.map(|data| {
        if data.len() > 8 * 1024 * 1024 {
            return Err(RuntimeError::Invalid);
        }
        let c: Contract = decode_json(data.as_bytes()).map_err(|_| RuntimeError::Invalid)?;
        c.recruit.validate()?;
        if !address(&c.peer)
            || c.recruit.contract_id != id
            || !matches!(c.recruit.body, Body::RecruitRequest { .. })
            || c.tasks.len() > 128
        {
            return Err(RuntimeError::Invalid);
        }
        for m in c
            .offer
            .iter()
            .chain(c.acceptance.iter())
            .chain(c.release.iter())
            .chain(c.release_ack.iter())
            .chain(c.tasks.values().map(|t| &t.request))
            .chain(c.tasks.values().filter_map(|t| t.evidence.as_ref()))
        {
            m.validate()?;
            if !c.context(m) {
                return Err(RuntimeError::Invalid);
            }
        }
        Ok(c)
    })
    .transpose()
}
fn save(tx: &Transaction<'_>, contract: &Contract) -> Result<(), RuntimeError> {
    tx.execute("INSERT INTO fetch_contracts(id,data) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![contract.recruit.contract_id,encode_contract(contract)?])?;
    Ok(())
}
fn capacity(tx: &Transaction<'_>, table: &str) -> Result<(), RuntimeError> {
    capacity_limit(tx, table, 10_000)
}
fn capacity_limit(tx: &Transaction<'_>, table: &str, limit: u64) -> Result<(), RuntimeError> {
    let count: u64 = tx.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
    if count >= limit {
        Err(RuntimeError::Capacity)
    } else {
        Ok(())
    }
}

fn storage_capacity(tx: &Transaction<'_>, control: bool) -> Result<(), RuntimeError> {
    let used:u64=tx.query_row("SELECT COALESCE((SELECT SUM(length(CAST(data AS BLOB))) FROM fetch_contracts),0)+COALESCE((SELECT SUM(length(CAST(data AS BLOB))) FROM fetch_outbox),0)+COALESCE((SELECT SUM(length(CAST(data AS BLOB))) FROM fetch_inbox),0)+COALESCE((SELECT SUM(length(CAST(data AS BLOB))) FROM fetch_rejected),0)",[],|r|r.get(0))?;
    if used > (if control { 256 } else { 64 }) * 1024 * 1024 {
        Err(RuntimeError::Capacity)
    } else {
        Ok(())
    }
}

fn encode_contract(c: &Contract) -> Result<String, RuntimeError> {
    if c.phase != ContractPhase::Released {
        return encode(c);
    }
    let data = serde_json::to_string(c).map_err(|_| RuntimeError::Invalid)?;
    if data.len() > 8 * 1024 * 1024 {
        Err(RuntimeError::Capacity)
    } else {
        Ok(data)
    }
}
