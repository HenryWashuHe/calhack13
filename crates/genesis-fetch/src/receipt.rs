//! Read-only reconstruction of immutable evidence from the private journal.
//!
//! These are transport provenance receipts, not signatures, kernel grants or
//! semantic decisions. The bridge authenticated the sender before ingestion;
//! this journal retains its attestation, not the original signed envelope.
use super::*;
use serde_json::Value;
use std::fmt;

const MAX_ITEM_BYTES: usize = MAX_MESSAGE_BYTES + 1024;

macro_rules! string_getters {
    ($($name:ident),+ $(,)?) => { $(
        pub fn $name(&self) -> &str { &self.$name }
    )+ };
}

/// Immutable recruitment evidence. Only a validated journal read constructs it.
/// Exact consent is bound by the expected outgoing AcceptOffer and the stored
/// full offer (including terms), not by the mutable contract's phase label.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct RecruitmentReceipt {
    schema_version: u32,
    org_id: String,
    mission_id: String,
    contract_id: String,
    correlation_id: String,
    bridge_address: String,
    peer_address: String,
    recruit_message_id: String,
    offer_message_id: String,
    accept_message_id: String,
    accepted_message_id: String,
    offer_id: String,
    lease_expires_at: DateTime<Utc>,
    recruit_sha256: String,
    offer_sha256: String,
    accept_sha256: String,
    accepted_sha256: String,
    terms_sha256: String,
    receipt_sha256: String,
}

impl RecruitmentReceipt {
    string_getters!(
        org_id,
        mission_id,
        contract_id,
        correlation_id,
        bridge_address,
        peer_address,
        recruit_message_id,
        offer_message_id,
        accept_message_id,
        accepted_message_id,
        offer_id,
        recruit_sha256,
        offer_sha256,
        accept_sha256,
        accepted_sha256,
        terms_sha256,
        receipt_sha256,
    );
    pub fn lease_expires_at(&self) -> DateTime<Utc> {
        self.lease_expires_at
    }
}

impl fmt::Debug for RecruitmentReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecruitmentReceipt")
            .field("receipt_sha256", &self.receipt_sha256)
            .finish_non_exhaustive()
    }
}

/// Historical native task result; output remains untrusted domain input.
/// Null output is retained. No network request or state change is performed by
/// receipt access. Consumers must separately check current kernel authority.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct TaskReceipt {
    schema_version: u32,
    recruitment: RecruitmentReceipt,
    task_request_id: String,
    result_message_id: String,
    job_id: String,
    attempt_id: String,
    role_id: String,
    assignment_id: String,
    epoch: u64,
    idempotency_key: String,
    deadline: DateTime<Utc>,
    request_sha256: String,
    result_sha256: String,
    output_sha256: String,
    receipt_sha256: String,
    output: Value,
}

impl TaskReceipt {
    string_getters!(
        task_request_id,
        result_message_id,
        job_id,
        attempt_id,
        role_id,
        assignment_id,
        idempotency_key,
        request_sha256,
        result_sha256,
        output_sha256,
        receipt_sha256,
    );
    pub fn recruitment(&self) -> &RecruitmentReceipt {
        &self.recruitment
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
    pub fn output(&self) -> &Value {
        &self.output
    }
}

impl fmt::Debug for TaskReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskReceipt")
            .field("receipt_sha256", &self.receipt_sha256)
            .field("output", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl RuntimeStore {
    /// Reconstruct accepted recruitment from exact trusted backend expectations.
    /// No wall-clock expiry check: this is historical evidence. The selected
    /// chain must exist in the journal; no missing legacy evidence is invented.
    pub fn recruitment_receipt(
        &self,
        peer: &str,
        expected_recruit: &Message,
        expected_accept: &Message,
    ) -> Result<RecruitmentReceipt, RuntimeError> {
        let connection = self.lock()?;
        let tx = connection.unchecked_transaction()?;
        let (receipt, _) = recruitment(&tx, self, peer, expected_recruit, expected_accept)?;
        Ok(receipt)
    }

    /// Reconstruct an exact task attempt, never a caller-supplied result or a
    /// public deserialized receipt. Reads share one SQLite snapshot. Hashes bind
    /// only immutable selected messages, not mutable queue acknowledgments,
    /// cursor, contract revision, later tasks or release state.
    pub fn task_receipt(
        &self,
        peer: &str,
        expected_recruit: &Message,
        expected_accept: &Message,
        expected_task: &Message,
    ) -> Result<TaskReceipt, RuntimeError> {
        expected_task.validate()?;
        if !matches!(&expected_task.body, Body::TaskRequest { .. }) {
            return Err(RuntimeError::Invalid);
        }
        let connection = self.lock()?;
        let tx = connection.unchecked_transaction()?;
        let (recruitment, accepted_sequence) =
            recruitment(&tx, self, peer, expected_recruit, expected_accept)?;
        if !same_context(expected_recruit, expected_task)
            || expected_task.expires_at > recruitment.lease_expires_at
        {
            return Err(RuntimeError::Conflict);
        }
        let stored_task = outgoing(&tx, peer, expected_task)?;
        let expected_task = &stored_task;
        let Body::TaskRequest { task } = &expected_task.body else {
            return Err(RuntimeError::Invalid);
        };
        let contract = read_contract(&tx, &expected_recruit.contract_id)?;
        let saved = contract
            .tasks
            .get(&expected_task.message_id)
            .ok_or(RuntimeError::NotFound)?;
        if saved.phase != TaskPhase::Collected
            || !canonical_message_eq(&saved.request, expected_task)?
        {
            return Err(RuntimeError::Conflict);
        }
        let result = saved.evidence.as_ref().ok_or(RuntimeError::Invalid)?;
        let item = incoming(&tx, peer, result)?;
        let result = &item.message;
        let Body::TaskResult {
            task_request_id,
            job_id,
            attempt_id,
            epoch,
            output,
        } = &result.body
        else {
            return Err(RuntimeError::Invalid);
        };
        if item.sequence <= accepted_sequence
            || !same_context(expected_task, result)
            || task_request_id != &expected_task.message_id
            || job_id != &task.job_id
            || attempt_id != &task.attempt_id
            || *epoch != task.epoch
            || result.expires_at > expected_task.expires_at
        {
            return Err(RuntimeError::Invalid);
        }
        // Message validation bounds the complete output before hashing/cloning.
        let request_sha256 = expected_task.canonical_sha256()?;
        let result_sha256 = result.canonical_sha256()?;
        let output_sha256 = json_sha256(output);
        let receipt_sha256 = json_sha256(&serde_json::json!({
            "domain":"genesis-fetch-task-receipt-v1",
            "recruitment_sha256":recruitment.receipt_sha256,
            "request":expected_task,"result":result,"sender":peer,
        }));
        Ok(TaskReceipt {
            schema_version: 1,
            recruitment,
            task_request_id: expected_task.message_id.clone(),
            result_message_id: result.message_id.clone(),
            job_id: task.job_id.clone(),
            attempt_id: task.attempt_id.clone(),
            role_id: task.role_id.clone(),
            assignment_id: task.assignment_id.clone(),
            epoch: task.epoch,
            idempotency_key: task.idempotency_key.clone(),
            deadline: task.deadline,
            request_sha256,
            result_sha256,
            output_sha256,
            receipt_sha256,
            output: output.clone(),
        })
    }
}

fn same_context(a: &Message, b: &Message) -> bool {
    a.org_id == b.org_id
        && a.mission_id == b.mission_id
        && a.contract_id == b.contract_id
        && a.correlation_id == b.correlation_id
}

fn read_contract(db: &Connection, id: &str) -> Result<Contract, RuntimeError> {
    let bytes: Option<i64> = db
        .query_row(
            "SELECT length(CAST(data AS BLOB)) FROM fetch_contracts WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if bytes.is_none_or(|bytes| !(1..=8 * 1024 * 1024).contains(&bytes)) {
        return Err(RuntimeError::Invalid);
    }
    load(db, id)?.ok_or(RuntimeError::NotFound)
}

fn outgoing(db: &Connection, peer: &str, expected: &Message) -> Result<Message, RuntimeError> {
    expected.validate()?;
    let row: Option<(String, String, String, String, i64)> = db
        .query_row(
            "SELECT contract_id,peer,data,digest,queued FROM fetch_outbox WHERE id=?1
             AND length(CAST(data AS BLOB))<=?2 AND length(contract_id)<=128
             AND length(peer)=65 AND length(digest)=64",
            params![expected.message_id, MAX_MESSAGE_BYTES],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let (contract, recipient, data, digest, queued) = row.ok_or(RuntimeError::NotFound)?;
    let actual = Message::decode(data.as_bytes())?;
    if contract != expected.contract_id
        || recipient != peer
        || !canonical_message_eq(&actual, expected)?
        || digest != send_digest(peer, &actual)?
        || !matches!(queued, 0 | 1)
    {
        return Err(RuntimeError::Conflict);
    }
    Ok(actual)
}

fn incoming(db: &Connection, peer: &str, expected: &Message) -> Result<InboxItem, RuntimeError> {
    let row: Option<(String, String, String)> = db
        .query_row(
            "SELECT digest,disposition,data FROM fetch_inbox WHERE id=?1
             AND length(CAST(data AS BLOB))<=?2 AND length(CAST(disposition AS BLOB))<=256
             AND length(digest)=64",
            params![expected.message_id, MAX_ITEM_BYTES],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (digest, disposition, data) = row.ok_or(RuntimeError::NotFound)?;
    if data.len() > MAX_ITEM_BYTES || disposition.len() > 256 {
        return Err(RuntimeError::Invalid);
    }
    let disposition: Disposition =
        decode_json(disposition.as_bytes()).map_err(|_| RuntimeError::Invalid)?;
    let item: InboxItem = decode_json(data.as_bytes()).map_err(|_| RuntimeError::Invalid)?;
    item.message.validate()?;
    if disposition != Disposition::Applied
        || item.sender != peer
        || !canonical_message_eq(&item.message, expected)?
        || digest != send_digest(peer, &item.message)?
        || item.sequence == 0
        || item.sequence > i64::MAX as u64
        || item.sequence > cursor(db)?
    {
        return Err(RuntimeError::Invalid);
    }
    Ok(item)
}

fn recruitment(
    db: &Connection,
    store: &RuntimeStore,
    peer: &str,
    expected_recruit: &Message,
    expected_accept: &Message,
) -> Result<(RecruitmentReceipt, u64), RuntimeError> {
    expected_recruit.validate()?;
    expected_accept.validate()?;
    if !address(peer)
        || peer == store.bridge_address
        || expected_recruit.org_id != store.org_id
        || !same_context(expected_recruit, expected_accept)
    {
        return Err(RuntimeError::Conflict);
    }
    // Recheck identity metadata inside this read snapshot, including after reopen.
    for (key, expected) in [
        ("schema", "1"),
        ("org", store.org_id.as_str()),
        ("bridge", store.bridge_address.as_str()),
    ] {
        let actual: String = db.query_row(
            "SELECT value FROM fetch_meta WHERE key=?1 AND length(CAST(value AS BLOB))<=128",
            [key],
            |r| r.get(0),
        )?;
        if actual != expected {
            return Err(RuntimeError::Conflict);
        }
    }
    let Body::RecruitRequest {
        capabilities: required,
        lease_expires_at: requested_lease,
        ..
    } = &expected_recruit.body
    else {
        return Err(RuntimeError::Invalid);
    };
    let Body::AcceptOffer {
        offer_message_id,
        offer_id: accepted_offer_id,
    } = &expected_accept.body
    else {
        return Err(RuntimeError::Invalid);
    };
    let stored_recruit = outgoing(db, peer, expected_recruit)?;
    let stored_accept = outgoing(db, peer, expected_accept)?;
    let expected_recruit = &stored_recruit;
    let expected_accept = &stored_accept;
    let contract = read_contract(db, &expected_recruit.contract_id)?;
    if contract.peer != peer
        || !canonical_message_eq(&contract.recruit, expected_recruit)?
        || !contract
            .acceptance
            .as_ref()
            .map(|actual| canonical_message_eq(actual, expected_accept))
            .transpose()?
            .unwrap_or(false)
        || !matches!(
            contract.phase,
            ContractPhase::Active | ContractPhase::Released
        )
    {
        return Err(RuntimeError::Conflict);
    }
    let offer = contract.offer.as_ref().ok_or(RuntimeError::Invalid)?;
    let offer_item = incoming(db, peer, offer)?;
    let offer = &offer_item.message;
    let Body::Offer {
        recruit_message_id,
        offer_id,
        capabilities,
        terms,
        lease_expires_at,
    } = &offer.body
    else {
        return Err(RuntimeError::Invalid);
    };
    if !same_context(expected_recruit, offer)
        || recruit_message_id != &expected_recruit.message_id
        || offer.message_id != *offer_message_id
        || offer_id != accepted_offer_id
        || lease_expires_at > requested_lease
        || expected_accept.expires_at > *lease_expires_at
        || required.iter().any(|cap| !capabilities.contains(cap))
    {
        return Err(RuntimeError::Invalid);
    }
    // Rejected acknowledgments are ordinary quarantined evidence and do not
    // invalidate the single previously Applied native acceptance. LIMIT 2
    // bounds materialized rows and detects an ambiguous Applied chain.
    let mut query = db.prepare(
        "SELECT id FROM fetch_inbox WHERE length(id)=36 AND length(CAST(disposition AS BLOB))<=256
        AND length(CAST(data AS BLOB))<=?2 AND json_valid(disposition)
        AND json_extract(disposition,'$.disposition')='applied' AND json_valid(data)
        AND json_extract(data,'$.message.body.kind')='contract_accepted'
        AND json_extract(data,'$.message.body.accept_message_id')=?1 LIMIT 2",
    )?;
    let ids = query
        .query_map(params![expected_accept.message_id, MAX_ITEM_BYTES], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if ids.len() != 1 {
        return Err(RuntimeError::Invalid);
    }
    let data: String = db.query_row(
        "SELECT data FROM fetch_inbox WHERE id=?1 AND length(CAST(data AS BLOB))<=?2",
        params![ids[0], MAX_ITEM_BYTES],
        |r| r.get(0),
    )?;
    if data.len() > MAX_ITEM_BYTES {
        return Err(RuntimeError::Invalid);
    }
    let candidate: InboxItem = decode_json(data.as_bytes()).map_err(|_| RuntimeError::Invalid)?;
    if candidate.message.message_id != ids[0] {
        return Err(RuntimeError::Invalid);
    }
    let accepted_item = incoming(db, peer, &candidate.message)?;
    let accepted = &accepted_item.message;
    if !same_context(expected_recruit, accepted)
        || accepted_item.sequence <= offer_item.sequence
        || accepted.expires_at > *lease_expires_at
        || !matches!(&accepted.body,Body::ContractAccepted { accept_message_id,offer_id: id }
            if accept_message_id == &expected_accept.message_id && id == offer_id)
    {
        return Err(RuntimeError::Invalid);
    }
    let receipt_sha256 = json_sha256(&serde_json::json!({
        "domain":"genesis-fetch-recruitment-receipt-v1",
        "bridge_address":store.bridge_address,"peer_address":peer,
        "recruit":expected_recruit,"offer":offer,"accept":expected_accept,"accepted":accepted,
    }));
    Ok((
        RecruitmentReceipt {
            schema_version: 1,
            org_id: expected_recruit.org_id.clone(),
            mission_id: expected_recruit.mission_id.clone(),
            contract_id: expected_recruit.contract_id.clone(),
            correlation_id: expected_recruit.correlation_id.clone(),
            bridge_address: store.bridge_address.clone(),
            peer_address: peer.into(),
            recruit_message_id: expected_recruit.message_id.clone(),
            offer_message_id: offer.message_id.clone(),
            accept_message_id: expected_accept.message_id.clone(),
            accepted_message_id: accepted.message_id.clone(),
            offer_id: offer_id.clone(),
            lease_expires_at: *lease_expires_at,
            recruit_sha256: expected_recruit.canonical_sha256()?,
            offer_sha256: offer.canonical_sha256()?,
            accept_sha256: expected_accept.canonical_sha256()?,
            accepted_sha256: accepted.canonical_sha256()?,
            terms_sha256: json_sha256(terms),
            receipt_sha256,
        },
        accepted_item.sequence,
    ))
}
