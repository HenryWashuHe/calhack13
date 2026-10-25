//! Durable, requirement-bound method adoption decisions.
//!
//! One record is one explicit receiver decision about one exact selected
//! method version: adopt (with acceptance recomputed from the receiver's own
//! retained trial) or decline. Only an `adopted` record can later authorize
//! invocation, and only while the requirement, mission and owner grant it was
//! bound to are unchanged. Records pin the exact reference; a newer
//! publication never upgrades one. Nothing here executes a method.
use crate::capability_adoption_store::{authority_snapshot, state, AdoptionError, AdoptionFence};
use crate::capability_authority::authorize_current;
use crate::method_adoption::{
    assess_selected, Acceptance, AcceptanceEvidence, MethodAcceptanceError, MethodReceiver,
    SelectedMethod,
};
use crate::store::Store;
use chrono::{DateTime, Utc};
use genesis_core::development_method::ExactContentRef;
use genesis_core::engine::DispatchGrant;
use genesis_core::ids::*;
use genesis_core::model::{OrgState, Owner};
use genesis_verification::{decode_json, json_sha256};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAX_METHOD_ADOPTIONS: usize = 64;
const MAX_RECORD_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMethodAdoption {
    pub client_request_id: String,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub publisher: OrgId,
    pub reference: ExactContentRef,
    pub decision: MethodDecision,
}

/// The receiver's explicit choice. `Adopt` names a retained trial under the
/// receiver's records root; the request never carries receipts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum MethodDecision {
    Adopt { trial: String },
    Decline { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MethodAdoptionOutcome {
    Adopted { evidence: Box<AcceptanceEvidence> },
    NotApplicable { evidence: Box<AcceptanceEvidence> },
    InsufficientEvidence { evidence: Box<AcceptanceEvidence> },
    Unknown { evidence: Box<AcceptanceEvidence> },
    Declined { reason: String },
}
impl MethodAdoptionOutcome {
    /// Each outcome holds exactly the acceptance it was derived from.
    fn consistent(&self) -> bool {
        match self {
            Self::Adopted { evidence } => {
                matches!(evidence.acceptance, Acceptance::Accepted { .. })
            }
            Self::NotApplicable { evidence } => {
                matches!(evidence.acceptance, Acceptance::Rejected { .. })
            }
            Self::InsufficientEvidence { evidence } => {
                matches!(evidence.acceptance, Acceptance::Insufficient { .. })
            }
            Self::Unknown { evidence } => matches!(evidence.acceptance, Acceptance::Unknown { .. }),
            Self::Declined { .. } => true,
        }
    }
    fn evidence(&self) -> Option<&AcceptanceEvidence> {
        match self {
            Self::Adopted { evidence }
            | Self::NotApplicable { evidence }
            | Self::InsufficientEvidence { evidence }
            | Self::Unknown { evidence } => Some(evidence),
            Self::Declined { .. } => None,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Adopted { .. } => "adopted",
            Self::NotApplicable { .. } => "not_applicable",
            Self::InsufficientEvidence { .. } => "insufficient_evidence",
            Self::Unknown { .. } => "unknown",
            Self::Declined { .. } => "declined",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MethodAdoptionError {
    #[error("invalid method adoption intent")]
    InvalidInput,
    #[error("invalid method adoption record")]
    InvalidRecord,
    #[error("method adoption not found")]
    NotFound,
    #[error("method adoption idempotency key has a different intent")]
    IdempotencyConflict,
    #[error("this exact method is already adopted for this requirement and grant")]
    AlreadyAdopted,
    #[error("method adoption count limit reached")]
    Limit,
    #[error("method adoption authority is fenced: {0:?}")]
    Authority(AdoptionFence),
    #[error(transparent)]
    Acceptance(#[from] MethodAcceptanceError),
    #[error("the method was not adopted")]
    NotAdopted,
    #[error("the deciding role does not hold `{METHOD_SCOPE}`, which this organization's constitution reserves for method work")]
    ScopeMissing,
    #[error("method adoption storage failed")]
    Db(#[from] rusqlite::Error),
}
type Result<T> = std::result::Result<T, MethodAdoptionError>;
use MethodAdoptionError as E;

/// The scope that method adoption and invocation require of the grant's role,
/// wherever the organization's constitution grants it at all. An organization
/// whose constitution never mentions it keeps the earlier behavior.
pub const METHOD_SCOPE: &str = "method.develop";

/// Refuses a role without [`METHOD_SCOPE`] when the constitution reserves it.
fn require_method_scope(
    state: &OrgState,
    role: &RoleId,
    responsibility: &ResponsibilityId,
    now: DateTime<Utc>,
) -> Result<()> {
    if !state.constitution.scopes.contains(METHOD_SCOPE) {
        return Ok(());
    }
    authorize_current(state, role, responsibility, Some(METHOD_SCOPE), now)
        .map(|_| ())
        .map_err(|_| E::ScopeMissing)
}

fn adoption_error(error: AdoptionError) -> E {
    match error {
        AdoptionError::Authority(fence) => E::Authority(fence),
        AdoptionError::Db(e) => E::Db(e),
        _ => E::InvalidRecord,
    }
}

/// Backend-only record; the exact selection, binding and outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodAdoptionRecord {
    schema_version: u32,
    pub id: String,
    pub client_request_id: String,
    intent_sha256: String,
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub grant: DispatchGrant,
    requirement_sha256: String,
    mission_sha256: String,
    authority_sha256: String,
    lease_expires_at: Option<DateTime<Utc>>,
    pub publisher: OrgId,
    pub reference: ExactContentRef,
    pub outcome: MethodAdoptionOutcome,
    pub created_at: DateTime<Utc>,
    /// The receiver runtime and protocol the method was checked against when
    /// this decision was made. Absent only in schema 1 records, which predate
    /// it and so can't show current compatibility: they never authorize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<Compatibility>,
}

/// What the receiver could run when it decided: a change to either fences
/// the decision, as `assess_selected` would now refuse the same package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub runtime: ExactContentRef,
    pub protocol: ExactContentRef,
}

impl MethodAdoptionRecord {
    /// The record's own invariants: the outcome matches its acceptance, and
    /// assessed evidence names exactly this record's selection.
    fn validate(&self) -> Result<()> {
        let bound = self
            .outcome
            .evidence()
            .is_none_or(|e| e.method == self.reference && e.publisher == self.publisher);
        let versioned = match self.schema_version {
            1 => self.compatibility.is_none(),
            2 => self.compatibility.is_some(),
            _ => false,
        };
        if !versioned || !self.outcome.consistent() || !bound {
            return Err(E::InvalidRecord);
        }
        Ok(())
    }

    /// Everything besides the grant that fences this record: a change to any
    /// of these makes it stop authorizing, so it must not block a fresh
    /// decision under the same assignment and epoch either. From schema 2 that
    /// includes the receiver policy the decision was made under (the pinned
    /// plan and the runtime/protocol), so a fresh acceptance under a changed
    /// policy is a new decision, not a duplicate. Schema 1 keeps its original
    /// digest so stored rows still match.
    fn fence_sha256(&self) -> Result<String> {
        let authority = (
            &self.requirement_sha256,
            &self.mission_sha256,
            &self.authority_sha256,
            &self.lease_expires_at,
        );
        match &self.compatibility {
            None => digest(&authority),
            Some(compatibility) => digest(&(
                authority,
                self.outcome.evidence().map(|e| &e.plan),
                compatibility,
            )),
        }
    }

    /// Whether `receiver` still runs what this decision was checked against.
    pub fn compatible_with(&self, receiver: &MethodReceiver) -> bool {
        self.compatibility
            .as_ref()
            .is_some_and(|c| c.runtime == receiver.runtime && c.protocol == receiver.protocol)
    }

    /// The retained acceptance this decision was derived from, if any.
    pub fn acceptance_evidence(&self) -> Option<&AcceptanceEvidence> {
        self.outcome.evidence()
    }

    /// The current owner grant this adopted method may run under, or why not.
    /// The invocation route always authorizes records reloaded from the store.
    pub fn authorize(&self, state: &OrgState, now: DateTime<Utc>) -> Result<DispatchGrant> {
        self.validate()?;
        if !matches!(self.outcome, MethodAdoptionOutcome::Adopted { .. }) {
            return Err(E::NotAdopted);
        }
        if self.org_id != state.org_id {
            return Err(E::Authority(AdoptionFence::AuthorityChanged));
        }
        let current = authority_snapshot(
            state,
            &self.mission_id,
            &self.requirement_id,
            &self.role_id,
            &self.responsibility_id,
            now,
        )
        .map_err(adoption_error)?;
        if current.requirement_sha256 != self.requirement_sha256 {
            return Err(E::Authority(AdoptionFence::RequirementChanged));
        }
        if current.grant != self.grant
            || current.authority_sha256 != self.authority_sha256
            || current.mission_sha256 != self.mission_sha256
            || current.lease_expires_at != self.lease_expires_at
        {
            return Err(E::Authority(AdoptionFence::AuthorityChanged));
        }
        require_method_scope(state, &self.role_id, &self.responsibility_id, now)?;
        Ok(current.grant)
    }
}

fn id_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn digest<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value).map_err(|_| E::InvalidRecord)?;
    Ok(json_sha256(&value))
}

fn load(conn: &Connection, id: &str) -> Result<MethodAdoptionRecord> {
    type Row = (String, String, String, String, String, [String; 3], i64);
    let row: Option<Row> = conn
        .query_row(
            "SELECT record_json,outcome,client_request_id,publisher,method_sha256,requirement_id,assignment_id,fence_sha256,epoch FROM method_adoptions WHERE id=?1 AND length(CAST(record_json AS BLOB))<=?2",
            params![id, MAX_RECORD_BYTES as i64],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    [row.get(5)?, row.get(6)?, row.get(7)?],
                    row.get(8)?,
                ))
            },
        )
        .optional()?;
    let (
        json,
        outcome,
        client_request_id,
        publisher,
        sha256,
        [requirement, assignment, fence],
        epoch,
    ) = row.ok_or(E::NotFound)?;
    let record: MethodAdoptionRecord =
        decode_json(json.as_bytes()).map_err(|_| E::InvalidRecord)?;
    record.validate()?;
    if record.id != id
        || record.outcome.label() != outcome
        || record.client_request_id != client_request_id
        || record.publisher.as_str() != publisher
        || record.reference.sha256 != sha256
        || record.requirement_id.as_str() != requirement
        || record.grant.assignment_id.as_str() != assignment
        || record.grant.epoch as i64 != epoch
        || record.fence_sha256()? != fence
    {
        return Err(E::InvalidRecord);
    }
    Ok(record)
}

impl Store {
    pub fn init_method_adoptions(&self) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS method_adoptions(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,client_request_id TEXT NOT NULL UNIQUE,requirement_id TEXT NOT NULL,assignment_id TEXT NOT NULL,epoch INTEGER NOT NULL,publisher TEXT NOT NULL,method_sha256 TEXT NOT NULL,fence_sha256 TEXT NOT NULL,outcome TEXT NOT NULL,record_json TEXT NOT NULL);")?;
        // A table created before the fence column existed (#155/#159): add
        // it, backfill each row from its own record and rebuild the index,
        // all in this transaction. Record bytes and identities are unchanged.
        let has_fence: bool = tx.query_row(
            "SELECT COUNT(*)>0 FROM pragma_table_info('method_adoptions') WHERE name='fence_sha256'",
            [],
            |row| row.get(0),
        )?;
        if !has_fence {
            tx.execute_batch(
                "ALTER TABLE method_adoptions ADD COLUMN fence_sha256 TEXT NOT NULL DEFAULT ''",
            )?;
            let rows = {
                let mut query = tx.prepare("SELECT id,record_json FROM method_adoptions")?;
                let rows = query
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            for (id, json) in rows {
                let record: MethodAdoptionRecord =
                    decode_json(json.as_bytes()).map_err(|_| E::InvalidRecord)?;
                record.validate()?;
                tx.execute(
                    "UPDATE method_adoptions SET fence_sha256=?1 WHERE id=?2",
                    params![record.fence_sha256()?, id],
                )?;
            }
        }
        let index: Option<String> = tx
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='index' AND name='method_adoption_adopted'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if index.is_some_and(|sql| !sql.contains("fence_sha256")) {
            tx.execute_batch("DROP INDEX method_adoption_adopted")?;
        }
        tx.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS method_adoption_adopted ON method_adoptions(requirement_id,assignment_id,epoch,fence_sha256,publisher,method_sha256) WHERE outcome='adopted';")?;
        tx.commit()?;
        Ok(())
    }

    /// Records one explicit decision. Returns the record and whether it is new;
    /// replaying the same intent under the same key returns the original.
    pub fn create_method_adoption(
        &self,
        intent: &CreateMethodAdoption,
        package_json: &str,
        receiver: &MethodReceiver,
        now: DateTime<Utc>,
    ) -> Result<(MethodAdoptionRecord, bool)> {
        self.create_method_adoption_rescored(intent, package_json, receiver, None, now)
    }

    /// As [`Self::create_method_adoption`]; when `rescored` is given, an
    /// adoption's assessment must have read exactly the bytes that rescore
    /// reproduced, and the evidence records the rescorer.
    pub fn create_method_adoption_rescored(
        &self,
        intent: &CreateMethodAdoption,
        package_json: &str,
        receiver: &MethodReceiver,
        rescored: Option<&crate::method_rescore::RescoredTrial>,
        now: DateTime<Utc>,
    ) -> Result<(MethodAdoptionRecord, bool)> {
        let ids = [
            intent.client_request_id.as_str(),
            intent.mission_id.as_str(),
            intent.requirement_id.as_str(),
            intent.role_id.as_str(),
            intent.responsibility_id.as_str(),
            intent.publisher.as_str(),
        ];
        let reason_ok = match &intent.decision {
            MethodDecision::Decline { reason } => {
                !reason.is_empty()
                    && reason.len() <= 1024
                    && reason.trim() == reason
                    && !reason.chars().any(char::is_control)
            }
            MethodDecision::Adopt { .. } => true,
        };
        if !ids.into_iter().all(id_ok) || intent.reference.validate().is_err() || !reason_ok {
            return Err(E::InvalidInput);
        }
        let intent_sha256 = digest(intent)?;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT id FROM method_adoptions WHERE client_request_id=?1",
                params![intent.client_request_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = previous {
            let previous = load(&tx, &id)?;
            if previous.intent_sha256 != intent_sha256 {
                return Err(E::IdempotencyConflict);
            }
            return Ok((previous, false));
        }
        let state = state(&tx).map_err(adoption_error)?;
        let snapshot = authority_snapshot(
            &state,
            &intent.mission_id,
            &intent.requirement_id,
            &intent.role_id,
            &intent.responsibility_id,
            now,
        )
        .map_err(adoption_error)?;
        // The decision belongs to the requirement's owner, not to a delegated
        // executor working for it.
        let owner = state
            .requirements
            .get(&intent.requirement_id)
            .map(|r| r.owner.clone());
        if owner != Some(Owner::Role(intent.role_id.clone())) {
            return Err(E::Authority(AdoptionFence::AuthorityChanged));
        }
        require_method_scope(&state, &intent.role_id, &intent.responsibility_id, now)?;
        let outcome = match &intent.decision {
            MethodDecision::Decline { reason } => MethodAdoptionOutcome::Declined {
                reason: reason.clone(),
            },
            MethodDecision::Adopt { trial } => {
                let selected = SelectedMethod {
                    publisher: intent.publisher.clone(),
                    reference: intent.reference.clone(),
                    package_json: package_json.into(),
                };
                let pin = receiver
                    .acceptance_plan(&intent.requirement_id)
                    .ok_or_else(|| {
                        MethodAcceptanceError::Binding(
                            "this requirement has no receiver-pinned acceptance plan".into(),
                        )
                    })?;
                if pin.publisher != intent.publisher {
                    return Err(MethodAcceptanceError::Binding(
                        "this requirement's pinned acceptance names another publisher".into(),
                    )
                    .into());
                }
                let pinned = &pin.plan;
                let mut evidence =
                    Box::new(assess_selected(receiver, &state.org_id, &selected, trial)?);
                if let Some(rescored) = rescored {
                    rescored.binds(&evidence)?;
                    evidence.rescorer = Some(rescored.rescorer_sha256.clone());
                }
                if evidence.plan != *pinned {
                    return Err(MethodAcceptanceError::Binding(
                        "the trial is not this requirement's pinned acceptance plan".into(),
                    )
                    .into());
                }
                match evidence.acceptance {
                    Acceptance::Accepted { .. } => MethodAdoptionOutcome::Adopted { evidence },
                    Acceptance::Rejected { .. } => {
                        MethodAdoptionOutcome::NotApplicable { evidence }
                    }
                    Acceptance::Insufficient { .. } => {
                        MethodAdoptionOutcome::InsufficientEvidence { evidence }
                    }
                    Acceptance::Unknown { .. } => MethodAdoptionOutcome::Unknown { evidence },
                }
            }
        };
        let total: usize = tx.query_row("SELECT COUNT(*) FROM method_adoptions", [], |row| {
            row.get(0)
        })?;
        if total >= MAX_METHOD_ADOPTIONS {
            return Err(E::Limit);
        }
        let record = MethodAdoptionRecord {
            schema_version: 2,
            id: format!("madopt_{}", uuid::Uuid::new_v4().simple()),
            client_request_id: intent.client_request_id.clone(),
            intent_sha256,
            org_id: state.org_id.clone(),
            mission_id: intent.mission_id.clone(),
            requirement_id: intent.requirement_id.clone(),
            role_id: intent.role_id.clone(),
            responsibility_id: intent.responsibility_id.clone(),
            grant: snapshot.grant,
            requirement_sha256: snapshot.requirement_sha256,
            mission_sha256: snapshot.mission_sha256,
            authority_sha256: snapshot.authority_sha256,
            lease_expires_at: snapshot.lease_expires_at,
            publisher: intent.publisher.clone(),
            reference: intent.reference.clone(),
            outcome,
            created_at: now,
            compatibility: Some(Compatibility {
                runtime: receiver.runtime.clone(),
                protocol: receiver.protocol.clone(),
            }),
        };
        let json = serde_json::to_string(&record).map_err(|_| E::InvalidRecord)?;
        if json.len() > MAX_RECORD_BYTES {
            return Err(E::InvalidRecord);
        }
        let inserted = tx.execute(
            "INSERT INTO method_adoptions(id,client_request_id,requirement_id,assignment_id,epoch,publisher,method_sha256,fence_sha256,outcome,record_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                record.id,
                record.client_request_id,
                record.requirement_id.as_str(),
                record.grant.assignment_id.as_str(),
                record.grant.epoch as i64,
                record.publisher.as_str(),
                record.reference.sha256,
                record.fence_sha256()?,
                record.outcome.label(),
                json
            ],
        );
        match inserted {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(E::AlreadyAdopted)
            }
            other => other?,
        };
        tx.commit()?;
        Ok((record, true))
    }

    /// The stored decision for this exact intent, if its key was already used.
    /// A different intent under the same key is a conflict.
    pub fn method_adoption_replay(
        &self,
        intent: &CreateMethodAdoption,
    ) -> Result<Option<MethodAdoptionRecord>> {
        let conn = self.lock();
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM method_adoptions WHERE client_request_id=?1",
                params![intent.client_request_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(None) };
        let record = load(&conn, &id)?;
        if record.intent_sha256 != digest(intent)? {
            return Err(E::IdempotencyConflict);
        }
        Ok(Some(record))
    }

    pub fn method_adoption(&self, id: &str) -> Result<MethodAdoptionRecord> {
        load(&self.lock(), id)
    }

    /// Every `adopted` record for one publisher and exact method digest, oldest
    /// first. Callers still compare the full reference and binding.
    pub fn adopted_methods(
        &self,
        publisher: &OrgId,
        sha256: &str,
    ) -> Result<Vec<MethodAdoptionRecord>> {
        let conn = self.lock();
        let mut query = conn.prepare(
            "SELECT id FROM method_adoptions WHERE publisher=?1 AND method_sha256=?2 AND outcome='adopted' ORDER BY ordinal",
        )?;
        let ids = query
            .query_map(params![publisher.as_str(), sha256], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter().map(|id| load(&conn, id)).collect()
    }
}

#[cfg(test)]
#[path = "method_adoption_store/tests.rs"]
pub(crate) mod tests;
