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
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::{
    BudgetAssessment, ContractError, ExecutionReceipt, InvocationGrant, ResourceGrant, Usage,
};
use genesis_core::development_trial::{TrialPlan, TrialPurpose};
use genesis_core::model::OrgState;
use genesis_verification::{decode_json, json_sha256};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAX_METHOD_RUNS: usize = 256;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
/// Room kept in every prepared record for its one receipt, so a run that
/// starts can always be completed. A larger receipt is refused on its own.
const RECEIPT_ALLOWANCE: usize = 64 * 1024;
/// Every matching receipt repeats the frozen binding, so the binding gets at
/// most a quarter of the receipt allowance. The rest is for the actual
/// method/environment, usage, outcome and outputs.
const BINDING_ALLOWANCE: usize = RECEIPT_ALLOWANCE / 4;
/// Room kept, beside the receipt's, for the run's one evaluation receipt.
const EVALUATION_ALLOWANCE: usize = 32 * 1024;

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

/// The receipt's usage against the frozen development grant. An overrun is
/// recorded, never hidden, and authorizes nothing further.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunBudget {
    WithinLimits,
    Unknown,
    Exceeded { resource: String },
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
    pub budget: Option<RunBudget>,
    /// The scored output, once recorded, and whether it counts as work for
    /// the responsibility. Absent until then (and in records written before
    /// this step existed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<MethodRunResult>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One evaluation of a completed run's output, checked against the frozen
/// plan's evaluation request. It describes that invocation only: it is not an
/// acceptance, a comparison or a reason to adopt anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodRunResult {
    pub evaluation: EvaluationReceipt,
    /// The evaluation's usage against the frozen scoring grant.
    pub scoring_budget: RunBudget,
    pub attachment: Attachment,
}

/// Whether the scored output counts as work done for the run's
/// responsibility under the current grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Attachment {
    Attached {
        requirement_id: String,
        responsibility_id: String,
        role_id: String,
        adoption_id: String,
    },
    /// Retained as evidence of that invocation; it completes nothing.
    NotAttached { reason: String },
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
    if accepted.slots.iter().any(|a| {
        a.run_id == slot.run_id
            || a.attempt_id == slot.attempt_id
            || a.scoring_request_id == slot.scoring_request_id
    }) {
        return Err(E::Plan(
            "run, attempt or scoring ids reuse the accepted trial's",
        ));
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

fn run_budget(usage: &Usage, grant: &ResourceGrant) -> Result<RunBudget> {
    Ok(match usage.assess(grant) {
        Ok(BudgetAssessment::WithinLimits) => RunBudget::WithinLimits,
        Ok(BudgetAssessment::Unknown) => RunBudget::Unknown,
        Err(ContractError::BudgetExceeded(resource)) => RunBudget::Exceeded {
            resource: resource.into(),
        },
        Err(e) => return Err(E::Receipt(e.to_string())),
    })
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
        self.lock().execute_batch("CREATE TABLE IF NOT EXISTS method_runs(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,client_request_id TEXT NOT NULL UNIQUE,plan_sha256 TEXT NOT NULL UNIQUE,run_id TEXT NOT NULL UNIQUE,attempt_id TEXT NOT NULL UNIQUE,scoring_request_id TEXT NOT NULL UNIQUE,status TEXT NOT NULL,record_json TEXT NOT NULL);")?;
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
            budget: None,
            result: None,
            created_at: now,
            updated_at: now,
        };
        let json = serde_json::to_string(&record).map_err(|_| E::InvalidRecord)?;
        if json.len() > MAX_RECORD_BYTES - RECEIPT_ALLOWANCE - EVALUATION_ALLOWANCE {
            return Err(E::InvalidInput);
        }
        // A run that starts must be able to record a terminal receipt, and
        // every receipt carries this binding.
        let binding = record
            .plan
            .expected_binding(&record.plan.slots[0].slot_id)
            .map_err(|_| E::InvalidRecord)?;
        let binding_bytes = serde_json::to_vec(&binding).map_err(|_| E::InvalidRecord)?;
        if binding_bytes.len() > BINDING_ALLOWANCE {
            return Err(E::Plan(
                "the run binding leaves no room for its receipt; shorten the plan's identifiers",
            ));
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM method_runs", [], |r| r.get(0))?;
        if count as usize >= MAX_METHOD_RUNS {
            return Err(E::Limit);
        }
        // One run per exact plan: two runs of one plan would share run and
        // attempt identities, so either's receipt would fit the other.
        let plan_sha256 = record
            .plan
            .reference()
            .map_err(|_| E::InvalidRecord)?
            .sha256;
        let taken: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM method_runs WHERE plan_sha256=?1)",
            params![plan_sha256],
            |r| r.get(0),
        )?;
        if taken {
            return Err(E::Plan("this exact plan is already frozen in another run"));
        }
        // Run, attempt and scoring ids are unique across runs, not just plans.
        let slot = &record.plan.slots[0];
        let reused: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM method_runs WHERE run_id=?1 OR attempt_id=?2 OR scoring_request_id=?3)",
            params![slot.run_id, slot.attempt_id.as_str(), slot.scoring_request_id],
            |r| r.get(0),
        )?;
        if reused {
            return Err(E::Plan(
                "run, attempt or scoring ids are used by another run",
            ));
        }
        tx.execute(
            "INSERT INTO method_runs(id,client_request_id,plan_sha256,run_id,attempt_id,scoring_request_id,status,record_json) VALUES(?1,?2,?3,?4,?5,?6,'prepared',?7)",
            params![record.id, record.client_request_id, plan_sha256, slot.run_id, slot.attempt_id.as_str(), slot.scoring_request_id, json],
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
        self.retain_method_run_receipt(id, receipt, state, Some(receiver), now)
    }

    /// [`Self::complete_method_run`], with the receiver optional. With none
    /// configured (new execution disabled) the receipt is still retained, as
    /// completed after fence: disabling stops new authority, it doesn't
    /// discard the result of a run already started.
    pub fn retain_method_run_receipt(
        &self,
        id: &str,
        receipt: &ExecutionReceipt,
        state: &OrgState,
        receiver: Option<&MethodReceiver>,
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
        let receipt_bytes = serde_json::to_vec(receipt).map_err(|_| E::InvalidRecord)?;
        if receipt_bytes.len() > RECEIPT_ALLOWANCE {
            return Err(E::Receipt("receipt exceeds its size bound".into()));
        }
        record.budget = Some(run_budget(&receipt.usage, &binding.budget)?);
        let authorized =
            receiver.is_some_and(|receiver| still_authorized(self, state, receiver, &record, now));
        record.status = if authorized {
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

    /// Records the completed run's one evaluation and whether its output is
    /// attached to the responsibility. The evaluation must be exactly the
    /// frozen plan's request for this run's receipt. It is attached only if
    /// the run completed under its grant, that grant still authorizes now,
    /// the development budget wasn't exceeded and the output was scored.
    /// Otherwise it is kept as evidence and attaches to nothing.
    pub fn record_method_run_result(
        &self,
        id: &str,
        evaluation: &EvaluationReceipt,
        state: &OrgState,
        receiver: Option<&MethodReceiver>,
        now: DateTime<Utc>,
    ) -> Result<MethodRunRecord> {
        let mut record = self.method_run(id)?;
        if !matches!(
            record.status,
            MethodRunStatus::Completed | MethodRunStatus::CompletedAfterFence
        ) || record.result.is_some()
        {
            return Err(E::Status(record.status));
        }
        let receipt = record.receipt.clone().ok_or(E::InvalidRecord)?;
        let slot = record.plan.slots[0].slot_id.clone();
        let binding = record
            .plan
            .expected_binding(&slot)
            .map_err(|_| E::InvalidRecord)?;
        let expected = record
            .plan
            .evaluation_request_for_accounting(&slot, &receipt)
            .map_err(|e| E::Receipt(e.to_string()))?
            .ok_or(E::Receipt("the run produced no output to score".into()))?;
        evaluation
            .validate_identity_against(&expected, &receipt, &binding)
            .map_err(|e| E::Receipt(e.to_string()))?;
        let bytes = serde_json::to_vec(evaluation).map_err(|_| E::InvalidRecord)?;
        if bytes.len() > EVALUATION_ALLOWANCE {
            return Err(E::Receipt("evaluation exceeds its size bound".into()));
        }
        let scoring_budget = run_budget(&evaluation.usage, &expected.budget)?;
        let reason = if record.status == MethodRunStatus::CompletedAfterFence {
            Some("the run completed after its grant changed".to_string())
        } else if !receiver
            .is_some_and(|receiver| still_authorized(self, state, receiver, &record, now))
        {
            Some("the run's grant no longer authorizes".to_string())
        } else if matches!(record.budget, Some(RunBudget::Exceeded { .. })) {
            Some("the run exceeded its development grant".to_string())
        } else if matches!(scoring_budget, RunBudget::Exceeded { .. }) {
            Some("the evaluation exceeded its scoring grant".to_string())
        } else if !matches!(evaluation.outcome, EvaluationOutcome::Scored { .. }) {
            Some("the output was not scored".to_string())
        } else {
            None
        };
        let grant = &record.authorization;
        let attachment = match reason {
            Some(reason) => Attachment::NotAttached { reason },
            None => Attachment::Attached {
                requirement_id: grant.requirement_id.as_str().into(),
                responsibility_id: grant.grant.responsibility_id.as_str().into(),
                role_id: grant.grant.role_id.as_str().into(),
                adoption_id: grant.adoption_id.clone(),
            },
        };
        record.result = Some(MethodRunResult {
            evaluation: evaluation.clone(),
            scoring_budget,
            attachment,
        });
        record.updated_at = now;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load(&tx, id)?;
        if current.status != record.status || current.result.is_some() {
            return Err(E::Status(current.status));
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
