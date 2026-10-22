//! Backend-only semantic finalization of fresh capability invocations.
//! A successful lookup is not a booking, ledger entry, or new capability grant.
//! Bindings are frozen at preparation; legacy transport-only runs stay unbound.

use crate::app::AppState;
use crate::capability_authority::authorize_job;
use crate::capability_runtime::{CapabilityRunPhase, CapabilityRunRecord, ExecutionPurpose};
use crate::capability_store::LocalAcceptance;
use crate::error::{ApiError, ApiResult};
use crate::routes::capabilities::{artifact, local_evidence};
use crate::store::{put_job, EventMeta, Store, StoreError, StoreResult};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::{InstallationId, JobId};
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{InstallationStatus, JobStatus};
use genesis_core::model::{OrgState, Provider};
use genesis_verification::{decode_json, json_sha256, verify_case, OracleSuite, SingleCaseReport};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_PROOF_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationBinding {
    schema_version: u32,
    job_id: JobId,
    installation_id: InstallationId,
    request_sha256: String,
    suite_id: String,
    suite_sha256: String,
    acceptance_report_ref: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationDecision {
    Verified,
    SemanticRejected,
    AuthorityRejected,
}

/// Serialize-only evidence. No HTTP endpoint accepts this proof as authority.
#[derive(Clone, Debug, Serialize)]
pub struct InvocationProof {
    pub schema_version: u32,
    pub job_id: JobId,
    pub installation_id: InstallationId,
    pub request_sha256: String,
    pub suite_id: String,
    pub suite_sha256: String,
    pub acceptance_report_ref: String,
    pub org_revision: u64,
    pub verified_at: DateTime<Utc>,
    pub decision: InvocationDecision,
    pub case_report: Option<SingleCaseReport>,
    pub proof_ref: String,
}

// This private envelope is untrusted storage on read, not an accepted proof.
// Reconstruct the proof from collected evidence and historical authority, then
// compare every field. Never deserialize the verifier's accepted wrapper.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredProof {
    org_revision: u64,
    verified_at: DateTime<Utc>,
    record_sha256: String,
    proof: Value,
}

fn invalid(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "invocation_binding", message)
}

fn encoded<T: Serialize>(value: &T) -> ApiResult<Value> {
    serde_json::to_value(value).map_err(|_| ApiError::internal("cannot encode invocation proof"))
}

fn record_digest(record: &CapabilityRunRecord) -> ApiResult<String> {
    Ok(json_sha256(&json!({
        "domain": "genesis-collected-invocation-v1",
        "record": encoded(record)?,
    })))
}

impl Store {
    pub fn init_capability_invocations(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_invocation_bindings (
                job_id TEXT PRIMARY KEY REFERENCES jobs(job_id), binding_json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS capability_invocation_verifications (
                job_id TEXT PRIMARY KEY REFERENCES jobs(job_id), proof_json TEXT NOT NULL);",
        )?;
        Ok(())
    }

    fn invocation_binding(&self, id: &JobId) -> ApiResult<InvocationBinding> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT binding_json FROM capability_invocation_bindings WHERE job_id=?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)?;
        let text = text.ok_or_else(|| invalid("invocation has no frozen backend binding"))?;
        if text.len() > MAX_PROOF_BYTES {
            return Err(invalid("invalid stored invocation binding"));
        }
        decode_json(text.as_bytes()).map_err(|_| invalid("invalid stored invocation binding"))
    }

    fn invocation_proof(&self, id: &JobId) -> ApiResult<Option<StoredProof>> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT proof_json FROM capability_invocation_verifications WHERE job_id=?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)?;
        text.map(|text| {
            if text.len() > MAX_PROOF_BYTES {
                return Err(invalid("invalid stored invocation proof"));
            }
            decode_json(text.as_bytes()).map_err(|_| invalid("invalid stored invocation proof"))
        })
        .transpose()
    }
}

pub(crate) fn prepare_binding(
    app: &AppState,
    job: &Job,
    record: &CapabilityRunRecord,
    installation_id: &InstallationId,
    acceptance: &LocalAcceptance,
) -> ApiResult<InvocationBinding> {
    record.validate(job)?;
    if record.purpose != ExecutionPurpose::Invocation
        || record.phase != CapabilityRunPhase::Prepared
    {
        return Err(invalid("only new invocations can bind verification policy"));
    }
    let suite = app
        .evidence_policy
        .suite(&acceptance.suite_id)
        .ok_or_else(|| invalid("unknown invocation suite"))?;
    Ok(InvocationBinding {
        schema_version: 1,
        job_id: job.job_id.clone(),
        installation_id: installation_id.clone(),
        request_sha256: record.request_sha256.clone(),
        suite_id: acceptance.suite_id.clone(),
        suite_sha256: json_sha256(&encoded(suite)?),
        acceptance_report_ref: acceptance.report_ref.clone(),
    })
}

pub(crate) fn insert_binding(tx: &Transaction<'_>, binding: &InvocationBinding) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO capability_invocation_bindings (job_id,binding_json) VALUES (?1,?2)",
        params![binding.job_id.as_str(), serde_json::to_string(binding)?],
    )?;
    Ok(())
}

fn validate_binding<'a>(
    app: &'a AppState,
    state: &OrgState,
    job: &Job,
    record: &CapabilityRunRecord,
    binding: &InvocationBinding,
) -> ApiResult<&'a OracleSuite> {
    record.validate(job)?;
    if record.purpose != ExecutionPurpose::Invocation
        || record.phase != CapabilityRunPhase::Collected
        || binding.schema_version != 1
        || binding.job_id != job.job_id
        || job.org_id != state.org_id
        || job.provider != Some(Provider::Apify)
        || binding.request_sha256 != record.request_sha256
    {
        return Err(invalid("run does not match a collected bound invocation"));
    }
    let installation = state
        .installations
        .get(&binding.installation_id)
        .ok_or_else(|| invalid("missing invocation installation"))?;
    let manifest = app.store.capability_manifest(&binding.installation_id)?;
    if job.capability.as_ref() != Some(&installation.capability)
        || installation.capability.capability_id != manifest.capability_id
        || installation.capability.version != manifest.version
        || installation.source_digest != manifest.source_digest
        || record.request.artifact != artifact(&manifest)
        || record.required_permissions != manifest.required_permissions
        || installation.acceptance_report_ref.as_deref() != Some(&binding.acceptance_report_ref)
    {
        return Err(invalid("invocation artifact or acceptance binding changed"));
    }
    manifest
        .check_publishable()
        .map_err(|_| invalid("invalid invocation manifest"))?;
    let acceptance = app
        .store
        .capability_acceptance(&binding.installation_id)?
        .ok_or_else(|| invalid("missing local acceptance"))?;
    if acceptance.suite_id != binding.suite_id
        || acceptance.report_ref != binding.acceptance_report_ref
    {
        return Err(invalid("invocation acceptance binding changed"));
    }
    let suite = app
        .evidence_policy
        .suite(&binding.suite_id)
        .ok_or_else(|| invalid("unknown invocation suite"))?;
    if json_sha256(&encoded(suite)?) != binding.suite_sha256 {
        return Err(invalid("full invocation suite changed since preparation"));
    }
    // Revalidate durable original evidence without requiring its retired
    // verifier to regain authority. The fresh job is authorized separately.
    let accepted = local_evidence(
        app,
        &binding.installation_id,
        &binding.suite_id,
        &acceptance.job_ids,
    )
    .map_err(|_| invalid("local acceptance evidence no longer validates"))?;
    if accepted.reference != binding.acceptance_report_ref {
        return Err(invalid("local acceptance proof changed"));
    }
    let case = app
        .evidence_policy
        .case_for_input(&binding.suite_id, &job.input)
        .map_err(|_| invalid("invocation input no longer matches the bound suite"))?;
    if case.case_id != record.request.case_id || case.source != record.request.source {
        return Err(invalid("invocation case binding changed"));
    }
    Ok(suite)
}

fn compute_proof(
    app: &AppState,
    state: &OrgState,
    job: &Job,
    record: &CapabilityRunRecord,
    binding: &InvocationBinding,
    now: DateTime<Utc>,
) -> ApiResult<InvocationProof> {
    let suite = validate_binding(app, state, job, record, binding)?;
    let authority = state
        .installations
        .get(&binding.installation_id)
        .is_some_and(|installation| installation.status == InstallationStatus::Accepted)
        && authorize_job(state, job, record.required_scope.as_deref(), now).is_ok()
        && record
            .required_permissions
            .iter()
            .all(|scope| authorize_job(state, job, Some(scope), now).is_ok())
        && job.capability.as_ref().is_some_and(|capability| {
            engine::authorize_capability(state, &job.role_id, capability).is_ok()
        });
    let (decision, case_report) = if authority {
        let evidence = record
            .evidence
            .as_ref()
            .ok_or_else(|| invalid("missing collected evidence"))?;
        let outcome = verify_case(
            suite,
            &record.request.artifact,
            &record.request.case_id,
            evidence,
        )
        .map_err(|_| invalid("invalid invocation verification configuration"))?;
        let decision = if outcome.is_accepted() {
            InvocationDecision::Verified
        } else {
            InvocationDecision::SemanticRejected
        };
        (decision, Some(outcome.report().clone()))
    } else {
        (InvocationDecision::AuthorityRejected, None)
    };
    let mut proof = InvocationProof {
        schema_version: 1,
        job_id: job.job_id.clone(),
        installation_id: binding.installation_id.clone(),
        request_sha256: binding.request_sha256.clone(),
        suite_id: binding.suite_id.clone(),
        suite_sha256: binding.suite_sha256.clone(),
        acceptance_report_ref: binding.acceptance_report_ref.clone(),
        org_revision: state.revision,
        verified_at: now,
        decision,
        case_report,
        proof_ref: String::new(),
    };
    proof.proof_ref = format!(
        "invocation:{}",
        json_sha256(&json!({
            "domain": "genesis-capability-invocation-v1", "proof": encoded(&proof)?
        }))
    );
    Ok(proof)
}

fn restore_proof(
    app: &AppState,
    job: &Job,
    record: &CapabilityRunRecord,
    binding: &InvocationBinding,
    stored: StoredProof,
) -> ApiResult<InvocationProof> {
    // Authority rejection has no semantic report, but still freezes the exact
    // collected receipt. Consistent edits to job.result and evidence.output
    // must not silently change the evidence behind an immutable decision.
    if record_digest(record)? != stored.record_sha256 {
        return Err(invalid(
            "collected invocation record changed after finalization",
        ));
    }
    let historical = app.store.state_at(stored.org_revision)?;
    let proof = compute_proof(app, &historical, job, record, binding, stored.verified_at)?;
    let status = if proof.decision == InvocationDecision::Verified {
        JobStatus::Succeeded
    } else {
        JobStatus::ResultRejected
    };
    if encoded(&proof)? != stored.proof || job.status != status {
        return Err(invalid(
            "stored invocation proof or terminal projection changed",
        ));
    }
    Ok(proof)
}

pub async fn read(app: &AppState, id: &JobId) -> ApiResult<InvocationProof> {
    let _guard = app.kernel.lock().await;
    read_locked(app, id)
}

/// Caller holds the kernel lock, including while freezing downstream work.
pub(crate) fn read_locked(app: &AppState, id: &JobId) -> ApiResult<InvocationProof> {
    let stored = app
        .store
        .invocation_proof(id)?
        .ok_or_else(|| ApiError::not_found("invocation proof"))?;
    let job = app.store.get_job(id)?;
    let record = app.store.get_capability_run(id)?;
    let binding = app.store.invocation_binding(id)?;
    restore_proof(app, &job, &record, &binding, stored)
}

pub async fn finalize(app: &AppState, id: &JobId) -> ApiResult<InvocationProof> {
    let _guard = app.kernel.lock().await;
    finalize_locked(app, id)
}

/// The caller must hold this organization's kernel lock. Shared by the explicit
/// operator endpoint and the bounded local finalization worker.
pub(crate) fn finalize_locked(app: &AppState, id: &JobId) -> ApiResult<InvocationProof> {
    let job = app.store.get_job(id)?;
    let record = app.store.get_capability_run(id)?;
    let binding = app.store.invocation_binding(id)?;
    if let Some(stored) = app.store.invocation_proof(id)? {
        return restore_proof(app, &job, &record, &binding, stored);
    }
    if job.status != JobStatus::Verifying {
        return Err(invalid(
            "invocation must finish collection before verification",
        ));
    }
    let state = app.current_state()?;
    let now = Utc::now();
    let proof = compute_proof(app, &state, &job, &record, &binding, now)?;
    let mut terminal = job.clone();
    let (status, raw, reason) = match proof.decision {
        InvocationDecision::Verified => (JobStatus::Succeeded, "APIFY_SEMANTIC_VERIFIED", None),
        InvocationDecision::SemanticRejected => (
            JobStatus::ResultRejected,
            "APIFY_SEMANTIC_REJECTED",
            Some("invocation output failed its bound oracle"),
        ),
        InvocationDecision::AuthorityRejected => (
            JobStatus::ResultRejected,
            "APIFY_AUTHORITY_REJECTED",
            Some("invocation authority no longer permits finalization"),
        ),
    };
    let from = terminal
        .set_status(status, now)
        .map_err(|_| invalid("invalid invocation terminal transition"))?;
    terminal.raw_status = Some(raw.into());
    terminal.rejection_reason = reason.map(str::to_owned);
    record.validate(&terminal)?;
    let mut effects = vec![EventKind::JobStatusChanged {
        job_id: id.clone(),
        from,
        to: status,
    }];
    if let Some(reason) = reason {
        effects.push(EventKind::JobResultRejected {
            job_id: id.clone(),
            reason: reason.into(),
        });
    }
    let stored = StoredProof {
        org_revision: state.revision,
        verified_at: now,
        record_sha256: record_digest(&record)?,
        proof: encoded(&proof)?,
    };
    let text = serde_json::to_string(&stored)
        .map_err(|_| ApiError::internal("cannot encode invocation proof"))?;
    if text.len() > MAX_PROOF_BYTES {
        return Err(invalid("invocation proof exceeds storage bound"));
    }
    let events = app.store.commit_state(state.revision, &state, effects, &EventMeta::default(), vec![], |tx| {
        let current: String = tx.query_row("SELECT job_json FROM jobs WHERE job_id=?1", params![id.as_str()], |row| row.get(0))?;
        if serde_json::from_str::<Job>(&current)? != job {
            return Err(StoreError::NotFound("unchanged invocation job".into()));
        }
        tx.execute("INSERT INTO capability_invocation_verifications (job_id,proof_json) VALUES (?1,?2)", params![id.as_str(), text])?;
        put_job(tx, &terminal)?;
        crate::capability_finalization::mark_completed(tx, id, now)
    })?;
    app.broadcast(&events);
    Ok(proof)
}
