//! Running an adopted method under its invocation grant, in three durable
//! steps that each re-check authority: prepare (freeze one invocation plan),
//! start (once, only under the same current grant), and record the receipt.
//!
//! The operator proposes the plan; the server never builds or runs it. A plan
//! is accepted only if it is a one-slot `Invocation` plan naming exactly the
//! current grant, the adopted method and its accepted dependencies, for the
//! task, evaluator and environment the method was accepted on, with the same
//! model settings and no larger grants than the accepted plan. A start that
//! meets a changed grant fences the run for good. A receipt recorded after the
//! grant changed is kept but marked `completed_after_fence`. There is no retry
//! or replay of a started run: a lost runner leaves it `started`.
//!
//! Callers hold the kernel lock across each step, as the adoption routes do,
//! so the organization state cannot change between authorization and commit.
use crate::method_adoption::MethodReceiver;
use crate::method_adoption_store::MethodAdoptionError;
use crate::method_invocation::{
    authorize_method_invocation, MethodInvocationGrant, MethodInvocationRequest,
};
use crate::store::Store;
use chrono::{DateTime, Utc};
use genesis_core::development_method::{ExecutionReceipt, InvocationGrant, ResourceGrant};
use genesis_core::development_trial::{TrialPlan, TrialPurpose};
use genesis_core::model::OrgState;
use genesis_verification::{decode_json, json_sha256};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAX_METHOD_RUNS: usize = 256;
const MAX_RECORD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMethodRun {
    pub client_request_id: String,
    pub request: MethodInvocationRequest,
    pub plan: TrialPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodRunStatus {
    Prepared,
    Started,
    /// The grant no longer stood when the run was to start; nothing ran.
    Fenced,
    Completed,
    /// A receipt arrived after the grant changed. Kept, attached to nothing.
    CompletedAfterFence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodRunRecord {
    schema_version: u32,
    pub id: String,
    pub client_request_id: String,
    intent_sha256: String,
    pub authorization: MethodInvocationGrant,
    pub plan: TrialPlan,
    pub status: MethodRunStatus,
    pub receipt: Option<ExecutionReceipt>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum MethodRunError {
    #[error("invalid method run request")]
    InvalidInput,
    #[error("the invocation plan is not acceptable: {0}")]
    Plan(&'static str),
    #[error("method run not found")]
    NotFound,
    #[error("method run idempotency key has a different intent")]
    IdempotencyConflict,
    #[error("method run count limit reached")]
    Limit,
    #[error("method run is {0:?}, not ready for this step")]
    Status(MethodRunStatus),
    #[error("the receipt does not belong to this run: {0}")]
    Receipt(String),
    #[error("invalid method run record")]
    InvalidRecord,
    #[error(transparent)]
    Adoption(#[from] MethodAdoptionError),
    #[error("method run storage failed")]
    Db(#[from] rusqlite::Error),
}
type Result<T> = std::result::Result<T, MethodRunError>;
use MethodRunError as E;

fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(json_sha256(
        &serde_json::to_value(value).map_err(|_| E::InvalidRecord)?,
    ))
}

fn within(grant: &ResourceGrant, cap: &ResourceGrant) -> bool {
    grant.model_calls <= cap.model_calls
        && grant.input_tokens <= cap.input_tokens
        && grant.output_tokens <= cap.output_tokens
        && grant.wall_time_ms <= cap.wall_time_ms
        && grant.simulation_steps <= cap.simulation_steps
}

/// The grant identity a plan must name for this authorization.
pub fn invocation_grant(authorization: &MethodInvocationGrant) -> InvocationGrant {
    InvocationGrant {
        adoption_id: authorization.adoption_id.clone(),
        requirement_id: authorization.requirement_id.clone(),
        role_id: authorization.grant.role_id.clone(),
        responsibility_id: authorization.grant.responsibility_id.clone(),
        assignment_id: authorization.grant.assignment_id.clone(),
        epoch: authorization.grant.epoch,
    }
}

/// The proposed plan against the adoption it runs under.
fn check_plan(
    store: &Store,
    receiver: &MethodReceiver,
    state: &OrgState,
    authorization: &MethodInvocationGrant,
    plan: &TrialPlan,
) -> Result<()> {
    plan.validate()
        .map_err(|_| E::Plan("not a valid trial plan"))?;
    let [slot] = plan.slots.as_slice() else {
        return Err(E::Plan("an invocation plan has exactly one slot"));
    };
    if slot.purpose
        != (TrialPurpose::Invocation {
            grant: invocation_grant(authorization),
        })
    {
        return Err(E::Plan(
            "the slot does not name the current invocation grant",
        ));
    }
    let record = store.method_adoption(&authorization.adoption_id)?;
    let evidence = record
        .acceptance_evidence()
        .ok_or(E::Plan("the adoption has no acceptance evidence"))?;
    // The plan the method was accepted under bounds every invocation of it.
    let accepted: TrialPlan = std::fs::read(
        receiver
            .records_root
            .join(&evidence.trial)
            .join("plan.json"),
    )
    .ok()
    .and_then(|bytes| decode_json(&bytes).ok())
    .ok_or(E::InvalidRecord)?;
    if accepted.reference().ok().as_ref() != Some(&evidence.plan) {
        return Err(E::InvalidRecord);
    }
    if slot.org_id != state.org_id
        || slot.method != authorization.reference
        || slot.dependencies != evidence.dependencies
    {
        return Err(E::Plan("organization, method or dependencies differ"));
    }
    if plan.context.public_task != evidence.public_task
        || plan.context.evaluator != evidence.evaluator
        || plan.development_environment != evidence.environment
        || plan.model_settings != accepted.model_settings
    {
        return Err(E::Plan(
            "task, evaluator, environment or model settings differ",
        ));
    }
    if !within(&plan.development_grant, &accepted.development_grant)
        || !within(&plan.scoring_grant, &accepted.scoring_grant)
    {
        return Err(E::Plan("grants exceed the accepted plan's"));
    }
    Ok(())
}

fn load(conn: &Connection, id: &str) -> Result<MethodRunRecord> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT record_json,status,client_request_id FROM method_runs WHERE id=?1 AND length(CAST(record_json AS BLOB))<=?2",
            params![id, MAX_RECORD_BYTES as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (json, status, client_request_id) = row.ok_or(E::NotFound)?;
    let record: MethodRunRecord = decode_json(json.as_bytes()).map_err(|_| E::InvalidRecord)?;
    if record.schema_version != 1
        || record.id != id
        || record.client_request_id != client_request_id
        || serde_json::to_value(record.status).ok() != Some(status.into())
    {
        return Err(E::InvalidRecord);
    }
    Ok(record)
}

fn save(conn: &Connection, record: &MethodRunRecord) -> Result<()> {
    let json = serde_json::to_string(record).map_err(|_| E::InvalidRecord)?;
    if json.len() > MAX_RECORD_BYTES {
        return Err(E::InvalidInput);
    }
    let status = serde_json::to_value(record.status).map_err(|_| E::InvalidRecord)?;
    conn.execute(
        "UPDATE method_runs SET status=?2,record_json=?3 WHERE id=?1",
        params![record.id, status.as_str(), json],
    )?;
    Ok(())
}

/// Whether the record's grant still authorizes, as of now.
fn still_authorized(
    store: &Store,
    state: &OrgState,
    receiver: &MethodReceiver,
    record: &MethodRunRecord,
    now: DateTime<Utc>,
) -> bool {
    let request = MethodInvocationRequest {
        role_id: record.authorization.grant.role_id.clone(),
        responsibility_id: record.authorization.grant.responsibility_id.clone(),
        publisher: record.authorization.publisher.clone(),
        reference: record.authorization.reference.clone(),
    };
    authorize_method_invocation(store, state, receiver, &request, now)
        .is_ok_and(|current| current == record.authorization)
}

impl Store {
    pub fn init_method_runs(&self) -> Result<()> {
        self.lock().execute_batch("CREATE TABLE IF NOT EXISTS method_runs(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,client_request_id TEXT NOT NULL UNIQUE,status TEXT NOT NULL,record_json TEXT NOT NULL);")?;
        Ok(())
    }

    /// Freezes one invocation plan under the current grant. Replaying the
    /// same intent under the same key returns the original record.
    pub fn prepare_method_run(
        &self,
        intent: &CreateMethodRun,
        state: &OrgState,
        receiver: &MethodReceiver,
        now: DateTime<Utc>,
    ) -> Result<(MethodRunRecord, bool)> {
        let key = &intent.client_request_id;
        if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
            return Err(E::InvalidInput);
        }
        let intent_sha256 = digest(intent)?;
        if let Some(previous) = self.method_run_by_key(key)? {
            if previous.intent_sha256 != intent_sha256 {
                return Err(E::IdempotencyConflict);
            }
            return Ok((previous, false));
        }
        let authorization =
            authorize_method_invocation(self, state, receiver, &intent.request, now)?;
        check_plan(self, receiver, state, &authorization, &intent.plan)?;
        let record = MethodRunRecord {
            schema_version: 1,
            id: format!("mrun_{}", &intent_sha256[..24]),
            client_request_id: key.clone(),
            intent_sha256,
            authorization,
            plan: intent.plan.clone(),
            status: MethodRunStatus::Prepared,
            receipt: None,
            created_at: now,
            updated_at: now,
        };
        let json = serde_json::to_string(&record).map_err(|_| E::InvalidRecord)?;
        if json.len() > MAX_RECORD_BYTES {
            return Err(E::InvalidInput);
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM method_runs", [], |r| r.get(0))?;
        if count as usize >= MAX_METHOD_RUNS {
            return Err(E::Limit);
        }
        tx.execute(
            "INSERT INTO method_runs(id,client_request_id,status,record_json) VALUES(?1,?2,'prepared',?3)",
            params![record.id, record.client_request_id, json],
        )?;
        tx.commit()?;
        Ok((record, true))
    }

    /// Prepared → Started once, only while the frozen grant is still current;
    /// otherwise Prepared → Fenced. Returns the record with the plan to run.
    pub fn start_method_run(
        &self,
        id: &str,
        state: &OrgState,
        receiver: &MethodReceiver,
        now: DateTime<Utc>,
    ) -> Result<MethodRunRecord> {
        let mut record = self.method_run(id)?;
        if record.status != MethodRunStatus::Prepared {
            return Err(E::Status(record.status));
        }
        let authorized = still_authorized(self, state, receiver, &record, now);
        record.status = if authorized {
            MethodRunStatus::Started
        } else {
            MethodRunStatus::Fenced
        };
        record.updated_at = now;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load(&tx, id)?.status != MethodRunStatus::Prepared {
            return Err(E::Status(load(&tx, id)?.status));
        }
        save(&tx, &record)?;
        tx.commit()?;
        if !authorized {
            return Err(E::Status(MethodRunStatus::Fenced));
        }
        Ok(record)
    }

    /// Records the run's one execution receipt, checked against the frozen
    /// plan's binding. Any outcome, Unknown included, is final.
    pub fn complete_method_run(
        &self,
        id: &str,
        receipt: &ExecutionReceipt,
        state: &OrgState,
        receiver: &MethodReceiver,
        now: DateTime<Utc>,
    ) -> Result<MethodRunRecord> {
        let mut record = self.method_run(id)?;
        if record.status != MethodRunStatus::Started {
            return Err(E::Status(record.status));
        }
        let slot = &record.plan.slots[0].slot_id;
        let binding = record
            .plan
            .expected_binding(slot)
            .map_err(|_| E::InvalidRecord)?;
        receipt
            .validate_identity_against(&binding)
            .map_err(|e| E::Receipt(e.to_string()))?;
        record.status = if still_authorized(self, state, receiver, &record, now) {
            MethodRunStatus::Completed
        } else {
            MethodRunStatus::CompletedAfterFence
        };
        record.receipt = Some(receipt.clone());
        record.updated_at = now;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load(&tx, id)?.status != MethodRunStatus::Started {
            return Err(E::Status(load(&tx, id)?.status));
        }
        save(&tx, &record)?;
        tx.commit()?;
        Ok(record)
    }

    pub fn method_run(&self, id: &str) -> Result<MethodRunRecord> {
        load(&self.lock(), id)
    }

    fn method_run_by_key(&self, key: &str) -> Result<Option<MethodRunRecord>> {
        let conn = self.lock();
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM method_runs WHERE client_request_id=?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| load(&conn, &id)).transpose()
    }
}

#[cfg(test)]
#[path = "method_invocation_run/tests.rs"]
mod tests;
