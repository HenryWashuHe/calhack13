//! Persistent CREAO mission work over backend-verified capability observations.
//!
//! An operator chooses a frozen event policy and a fresh invocation. The same
//! accountable specialist may assess later updates; one unresolved update per
//! responsibility prevents old results overtaking newer work. Native receipts
//! and domain acceptance remain separate. No reservation or mission completion
//! is implied by an accepted assessment.
use crate::app::AppState;
use crate::capability_authority::authorize_job;
use crate::capability_invocation::{self, InvocationDecision};
use crate::error::{ApiError, ApiResult};
use crate::store::{insert_job_tx, put_job, EventMeta, Store, StoreError, StoreResult};
use crate::venue_assessment_domain::{
    self as domain, AssessmentContext, AssessmentOutput, AssessmentVerdict, VenueAssessmentPolicy,
    REQUIRED_SCOPE,
};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::{JobId, ObservationId, RequirementId, ResponsibilityId, RoleId};
use genesis_core::jobs::{DispatchRequest, Job};
use genesis_core::lifecycle::{JobStatus, RequirementStatus};
use genesis_core::model::{AgentBinding, OrgState, Owner, Provider};
use genesis_providers::SpecialistExecutionReceipt;
use genesis_verification::{decode_json, json_sha256, ExtractionOutcome};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_RECORD_BYTES: usize = 128 * 1024;
const MAX_LOCAL_ATTEMPTS: u32 = 3;
const TASK: &str = "Assess the selected venue against every frozen event constraint. Return only the specified JSON inside your text result. Cite the exact verified source; do not book a venue or declare the mission complete.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAssessmentRequest {
    pub client_request_id: String,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub source_job_id: JobId,
    pub policy: VenueAssessmentPolicy,
    pub deadline: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: u32,
    request: CreateAssessmentRequest,
    context: AssessmentContext,
    job: Job,
    agent: AgentBinding,
    source_created_at: DateTime<Utc>,
    source_output_sha256: String,
    created_revision: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedBinding {
    binding: Binding,
    sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentDecision {
    Accepted,
    SemanticRejected,
    AuthorityRejected,
}

/// Accepted only when reconstructed from the private provider journal and
/// historical organization state. No endpoint deserializes a caller's proof.
#[derive(Clone, Debug, Serialize)]
pub struct AssessmentProof {
    pub schema_version: u32,
    pub assessment_id: String,
    pub decision: AssessmentDecision,
    pub output: Option<AssessmentOutput>,
    pub invocation_proof_ref: String,
    pub execution_receipt_sha256: String,
    pub run_id: String,
    pub provider_agent_id: String,
    pub org_revision: u64,
    pub verified_at: DateTime<Utc>,
    pub proof_ref: String,
    pub observation_id: Option<ObservationId>,
    pub requirement_id: Option<RequirementId>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDecision {
    decision_revision: u64,
    committed_revision: u64,
    verified_at: DateTime<Utc>,
    observation_id: Option<ObservationId>,
    requirement_id: Option<RequirementId>,
    proof: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssessmentRecord {
    pub assessment_id: String,
    pub job_id: JobId,
    pub source_job_id: JobId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub status: JobStatus,
    pub policy: VenueAssessmentPolicy,
    pub proof: Option<AssessmentProof>,
    pub finalization_attempts: u32,
    pub attention: Option<String>,
}

fn conflict(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "venue_assessment", message)
}
fn encode(value: &impl Serialize) -> ApiResult<Value> {
    serde_json::to_value(value).map_err(|_| ApiError::internal("cannot encode venue assessment"))
}
fn digest(value: &impl Serialize) -> ApiResult<String> {
    Ok(json_sha256(&encode(value)?))
}
fn decode<T: serde::de::DeserializeOwned>(text: &str) -> ApiResult<T> {
    if text.len() > MAX_RECORD_BYTES {
        return Err(conflict("stored venue assessment exceeds its bound"));
    }
    decode_json(text.as_bytes()).map_err(|_| conflict("invalid stored venue assessment"))
}
fn job_spec(job: &Job) -> Job {
    let mut frozen = job.clone();
    frozen.status = JobStatus::Queued;
    frozen.updated_at = frozen.created_at;
    frozen.provider_refs.clear();
    frozen.raw_status = None;
    frozen.result = None;
    frozen.rejection_reason = None;
    frozen
}

impl Store {
    pub fn init_venue_assessments(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS venue_assessments (
                assessment_id TEXT PRIMARY KEY,
                client_request_id TEXT NOT NULL UNIQUE,
                job_id TEXT NOT NULL UNIQUE REFERENCES jobs(job_id),
                responsibility_id TEXT NOT NULL,
                source_job_id TEXT NOT NULL,
                binding_json TEXT NOT NULL,
                proof_json TEXT,
                finalization_attempts INTEGER NOT NULL DEFAULT 0,
                attention TEXT,
                UNIQUE(responsibility_id, source_job_id));",
        )?;
        Ok(())
    }

    fn assessment_binding(&self, id: &str) -> ApiResult<Binding> {
        let row: Option<(String, String, String, String, String)> = self.lock().query_row(
            "SELECT binding_json,client_request_id,job_id,responsibility_id,source_job_id FROM venue_assessments WHERE assessment_id=?1",
            [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional().map_err(StoreError::from)?;
        let (text, request_key, job_id, responsibility_id, source_job_id) =
            row.ok_or_else(|| ApiError::not_found("venue assessment"))?;
        let sealed: SealedBinding = decode(&text)?;
        let binding = sealed.binding;
        if digest(&binding)? != sealed.sha256
            || binding.schema_version != 1
            || request_key != binding.request.client_request_id
            || job_id != binding.job.job_id.as_str()
            || responsibility_id != binding.job.responsibility_id.as_str()
            || source_job_id != binding.context.source_job_id.as_str()
            || binding.context.assessment_id != id
            || binding.request.source_job_id != binding.context.source_job_id
            || binding.context.policy != binding.request.policy
            || binding.context.policy_sha256 != digest(&binding.request.policy)?
            || binding.job.input != domain::provider_input(&binding.context)
            || binding.job.task != TASK
            || binding.job.role_id != binding.request.role_id
            || binding.job.responsibility_id != binding.request.responsibility_id
            || binding.job.deadline != Some(binding.request.deadline)
            || binding.job.provider != Some(Provider::Creao)
            || binding.agent.provider != Provider::Creao
            || binding.job != job_spec(&binding.job)
        {
            return Err(conflict("venue assessment binding changed"));
        }
        Ok(binding)
    }

    pub(crate) fn assessment_for_job(&self, job: &JobId) -> ApiResult<Option<String>> {
        self.lock()
            .query_row(
                "SELECT assessment_id FROM venue_assessments WHERE job_id=?1",
                [job.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)
            .map_err(Into::into)
    }

    fn assessment_by_request(&self, key: &str) -> ApiResult<Option<String>> {
        self.lock()
            .query_row(
                "SELECT assessment_id FROM venue_assessments WHERE client_request_id=?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)
            .map_err(Into::into)
    }

    fn assessment_decision(&self, id: &str) -> ApiResult<Option<StoredDecision>> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT proof_json FROM venue_assessments WHERE assessment_id=?1",
                [id],
                |row| row.get(0),
            )
            .map_err(StoreError::from)?;
        text.map(|text| decode(&text)).transpose()
    }

    fn responsibility_assessments(&self, id: &ResponsibilityId) -> StoreResult<Vec<String>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT assessment_id FROM venue_assessments WHERE responsibility_id=?1")?;
        let values = stmt
            .query_map([id.as_str()], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }
}

/// Live domain-specific authority, supplementing generic CREAO job fencing.
/// The caller holds the organization's kernel lock.
pub(crate) fn authorize_bound_job(
    app: &AppState,
    state: &OrgState,
    job: &Job,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    if let Some(id) = app.store.assessment_for_job(&job.job_id)? {
        let binding = app.store.assessment_binding(&id)?;
        authorize_binding(state, job, &binding, now)?;
    } else if managed(job) {
        return Err(conflict("assessment job has no durable domain binding"));
    }
    Ok(())
}

/// Frozen backend task marker also closes missing/corrupted index fallthrough.
pub(crate) fn managed(job: &Job) -> bool {
    job.task == TASK
}

fn authorize_binding(
    state: &OrgState,
    job: &Job,
    binding: &Binding,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    if job_spec(job) != binding.job {
        return Err(conflict("assessment job changed after preparation"));
    }
    authorize_job(state, job, Some(REQUIRED_SCOPE), now)?;
    if state
        .roles
        .get(&job.role_id)
        .and_then(|role| role.binding.as_ref())
        != Some(&binding.agent)
    {
        return Err(conflict("assessment specialist binding changed"));
    }
    Ok(())
}

/// Resolve only trusted, semantically accepted invocation evidence. Historical
/// producer retirement is allowed, but foreign missions and acceptance runs are
/// not monitored updates. No oracle answers enter the specialist input.
fn source(
    app: &AppState,
    id: &JobId,
) -> ApiResult<(
    Job,
    crate::capability_invocation::InvocationProof,
    ExtractionOutcome,
    String,
)> {
    let proof = capability_invocation::read_locked(app, id)?;
    if proof.decision != InvocationDecision::Verified {
        return Err(conflict("source invocation is not verified"));
    }
    let job = app.store.get_job(id)?;
    let record = app.store.get_capability_run(id)?;
    let output = &record
        .evidence
        .as_ref()
        .ok_or_else(|| conflict("source invocation has no evidence"))?
        .output;
    let extraction = decode_json(
        &serde_json::to_vec(output)
            .map_err(|_| ApiError::internal("cannot encode source evidence"))?,
    )
    .map_err(|_| conflict("source invocation has no typed venue facts"))?;
    Ok((job, proof, extraction, json_sha256(output)))
}

fn validate_source(app: &AppState, binding: &Binding) -> ApiResult<()> {
    let (job, proof, extraction, output_sha) = source(app, &binding.context.source_job_id)?;
    let venue = domain::select_venue(&extraction, &binding.context.policy)
        .map_err(|_| conflict("source has insufficient venue evidence"))?;
    if job.org_id != binding.job.org_id
        || job.mission_id != binding.job.mission_id
        || job.created_at != binding.source_created_at
        || proof.proof_ref != binding.context.invocation_proof_ref
        || output_sha != binding.source_output_sha256
        || venue != binding.context.venue
    {
        return Err(conflict(
            "source invocation changed after assessment preparation",
        ));
    }
    Ok(())
}

pub async fn create(
    app: &AppState,
    request: CreateAssessmentRequest,
) -> ApiResult<(AssessmentRecord, bool)> {
    let _guard = app.kernel.lock().await;
    create_locked(app, request)
}

/// Internal callers retain the kernel lock across their own frozen-authority
/// check and this preparation. This performs no native provider request.
pub(crate) fn create_locked(
    app: &AppState,
    request: CreateAssessmentRequest,
) -> ApiResult<(AssessmentRecord, bool)> {
    if request.client_request_id.trim().is_empty() || request.client_request_id.len() > 128 {
        return Err(ApiError::bad_request(
            "client_request_id must contain 1 to 128 bytes",
        ));
    }
    // Exact replay is historical lookup; it cannot renew expired authority or
    // acquire a fresh provider operation. Changed semantics always conflict.
    if let Some(id) = app
        .store
        .assessment_by_request(&request.client_request_id)?
    {
        if app.store.assessment_binding(&id)?.request != request {
            return Err(conflict("request key already binds a different assessment"));
        }
        return Ok((read_locked(app, &id)?, false));
    }
    domain::validate_policy(&request.policy)
        .map_err(|_| ApiError::bad_request("invalid venue assessment policy"))?;
    let now = Utc::now();
    if request.deadline <= now || request.deadline > now + Duration::hours(24) {
        return Err(ApiError::bad_request(
            "assessment deadline must be within the next 24 hours",
        ));
    }
    let state = app.current_state()?;
    let agent = state
        .roles
        .get(&request.role_id)
        .and_then(|role| role.binding.clone())
        .filter(|agent| agent.provider == Provider::Creao)
        .ok_or_else(|| conflict("assessment role must have a CREAO binding"))?;
    app.providers.specialist(Provider::Creao)?;
    let (source_job, source_proof, extraction, source_output_sha256) =
        source(app, &request.source_job_id)?;
    if source_job.org_id != app.org_id
        || state
            .responsibilities
            .get(&request.responsibility_id)
            .is_none_or(|r| r.mission_id != source_job.mission_id)
        || source_proof.verified_at > now
        || source_proof.verified_at < now - Duration::minutes(30)
        || source_job.created_at > now
        || source_job.created_at < now - Duration::minutes(30)
    {
        return Err(conflict(
            "source must be a recent verified invocation from this mission",
        ));
    }
    for id in app
        .store
        .responsibility_assessments(&request.responsibility_id)?
    {
        let previous = app.store.assessment_binding(&id)?;
        let previous_job = app.store.get_job(&previous.job.job_id)?;
        if previous.source_created_at >= source_job.created_at {
            return Err(conflict(
                "assessment source was already used or predates an earlier update",
            ));
        }
        if !matches!(
            previous_job.status,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::ResultRejected
        ) {
            return Err(conflict(
                "responsibility already has an unresolved assessment",
            ));
        }
    }
    let context = AssessmentContext {
        assessment_id: format!("assessment_{}", uuid::Uuid::new_v4()),
        source_job_id: request.source_job_id.clone(),
        invocation_proof_ref: source_proof.proof_ref,
        policy_sha256: digest(&request.policy)?,
        venue: domain::select_venue(&extraction, &request.policy)
            .map_err(|_| conflict("source has insufficient venue evidence"))?,
        policy: request.policy.clone(),
    };
    let job_key = format!("venue-assessment:{}", context.assessment_id);
    let dispatch = DispatchRequest {
        client_request_id: job_key.clone(),
        role_id: request.role_id.clone(),
        responsibility_id: request.responsibility_id.clone(),
        task: TASK.into(),
        input: domain::provider_input(&context),
        required_scope: Some(REQUIRED_SCOPE.into()),
        capability: None,
        provider: Some(Provider::Creao),
        deadline: Some(request.deadline),
    };
    let (job, effects, outbox) = crate::routes::jobs::new_job(app, &state, &dispatch)?;
    let binding = Binding {
        schema_version: 1,
        request,
        context,
        job: job.clone(),
        agent,
        source_created_at: source_job.created_at,
        source_output_sha256,
        created_revision: state.revision,
    };
    authorize_binding(&state, &job, &binding, now)?;
    let sealed = SealedBinding {
        sha256: digest(&binding)?,
        binding: binding.clone(),
    };
    let text = serde_json::to_string(&sealed)
        .map_err(|_| ApiError::internal("cannot encode assessment binding"))?;
    if text.len() > MAX_RECORD_BYTES {
        return Err(ApiError::bad_request("assessment exceeds storage bound"));
    }
    let events = app.store.commit_state(state.revision, &state, effects,
        &EventMeta::mission(job.mission_id.clone()).caused_by(binding.context.assessment_id.clone()), outbox, |tx| {
        insert_job_tx(tx, &job_key, &job)?;
        tx.execute("INSERT INTO venue_assessments (assessment_id,client_request_id,job_id,responsibility_id,source_job_id,binding_json) VALUES (?1,?2,?3,?4,?5,?6)", params![binding.context.assessment_id, binding.request.client_request_id, job.job_id.as_str(), job.responsibility_id.as_str(), binding.context.source_job_id.as_str(), text])?;
        Ok(())
    })?;
    app.broadcast(&events);
    Ok((read_locked(app, &binding.context.assessment_id)?, true))
}

/// Recover a prepared child's identity without renewing authority or creating
/// work. Used after interrupted subscription preparation, including after stop.
pub(crate) fn lookup_request_locked(
    app: &AppState,
    request: &CreateAssessmentRequest,
) -> ApiResult<Option<AssessmentRecord>> {
    let Some(id) = app
        .store
        .assessment_by_request(&request.client_request_id)?
    else {
        return Ok(None);
    };
    if app.store.assessment_binding(&id)?.request != *request {
        return Err(conflict("request key already binds a different assessment"));
    }
    Ok(Some(read_locked(app, &id)?))
}

fn receipt(app: &AppState, binding: &Binding) -> ApiResult<SpecialistExecutionReceipt> {
    app.providers
        .specialist(Provider::Creao)?
        .execution_receipt(&binding.job, &binding.agent)
        .map_err(Into::into)
}

fn compute_proof(
    state: &OrgState,
    binding: &Binding,
    job: &Job,
    receipt: &SpecialistExecutionReceipt,
    now: DateTime<Utc>,
) -> ApiResult<AssessmentProof> {
    let checked = domain::verify_output(&binding.context, receipt.output.as_ref());
    let (decision, output) = if authorize_binding(state, job, binding, now).is_err() {
        (AssessmentDecision::AuthorityRejected, None)
    } else if let Ok(output) = checked {
        (AssessmentDecision::Accepted, Some(output))
    } else {
        (AssessmentDecision::SemanticRejected, None)
    };
    let mut proof = AssessmentProof {
        schema_version: 1,
        assessment_id: binding.context.assessment_id.clone(),
        decision,
        output,
        invocation_proof_ref: binding.context.invocation_proof_ref.clone(),
        execution_receipt_sha256: digest(receipt)?,
        run_id: receipt.run_id.clone(),
        provider_agent_id: receipt.provider_agent_id.clone(),
        org_revision: state.revision,
        verified_at: now,
        proof_ref: String::new(),
        observation_id: None,
        requirement_id: None,
    };
    proof.proof_ref = format!("venue-assessment:{}", digest(&proof)?);
    Ok(proof)
}

fn summary(proof: &AssessmentProof, binding: &Binding) -> String {
    let venue = &binding.context.venue;
    let policy = &binding.context.policy;
    let Some(output) = &proof.output else {
        return "Rejected venue assessment".into();
    };
    if output.verdict == AssessmentVerdict::Feasible {
        return format!("Verified venue assessment for {}: capacity, price, event window, declared availability and step-free requirement meet the frozen event constraints.", venue.venue_id);
    }
    let mut failures = Vec::new();
    if !output.checks.capacity {
        failures.push(format!(
            "capacity {} is below {} attendees",
            venue.capacity, policy.attendees
        ));
    }
    if !output.checks.price {
        failures.push(format!(
            "price {} {} minor units does not fit maximum {} {}",
            venue.price.minor_units,
            venue.price.currency,
            policy.budget.minor_units,
            policy.budget.currency
        ));
    }
    if !output.checks.window {
        failures.push("offered window does not contain the event interval".into());
    }
    if !output.checks.availability {
        failures.push("source declares the venue unavailable".into());
    }
    if !output.checks.step_free {
        failures.push("source does not declare required step-free access".into());
    }
    format!(
        "Verified venue assessment for {} requires replacement work: {}.",
        venue.venue_id,
        failures.join("; ")
    )
}
const REQUIREMENT_SUMMARY: &str =
    "Find a replacement venue satisfying the verified event constraints.";
fn acceptance(binding: &Binding) -> String {
    let policy = &binding.context.policy;
    format!("A new verified assessment of a selected replacement venue must cover at least {} attendees, cost at most {} {} minor units, contain the full interval {} through {}, declare availability, and satisfy step_free_required={}. Source assessment: {}. No reservation is implied.", policy.attendees, policy.budget.minor_units, policy.budget.currency, policy.window.start, policy.window.end, policy.step_free_required, binding.context.assessment_id)
}

fn restore(
    app: &AppState,
    binding: &Binding,
    job: &Job,
    stored: StoredDecision,
) -> ApiResult<AssessmentProof> {
    validate_source(app, binding)?;
    let historical = app.store.state_at(stored.decision_revision)?;
    let native = receipt(app, binding)?;
    let mut proof = compute_proof(&historical, binding, job, &native, stored.verified_at)?;
    proof.observation_id = stored.observation_id;
    proof.requirement_id = stored.requirement_id;
    let expected_status = if proof.decision == AssessmentDecision::Accepted {
        JobStatus::Succeeded
    } else {
        JobStatus::ResultRejected
    };
    if encode(&proof)? != stored.proof
        || job.status != expected_status
        || job_spec(job) != binding.job
        || job.result != Some(json!({"kind":"venue_assessment", "proof":proof}))
    {
        return Err(conflict("stored assessment decision changed"));
    }
    let committed = app.store.state_at(stored.committed_revision)?;
    if proof.decision == AssessmentDecision::Accepted {
        let observation = proof
            .observation_id
            .as_ref()
            .and_then(|id| committed.observations.get(id))
            .ok_or_else(|| conflict("assessment observation missing"))?;
        if observation.mission_id != job.mission_id
            || observation.source != "creao-venue-assessment"
            || observation.summary != summary(&proof, binding)
            || observation.evidence_ref.as_ref() != Some(&proof.proof_ref)
            || observation.observed_at != proof.verified_at
        {
            return Err(conflict("assessment observation changed"));
        }
        if proof
            .output
            .as_ref()
            .is_some_and(|o| o.verdict == AssessmentVerdict::Reopen)
        {
            let requirement = proof
                .requirement_id
                .as_ref()
                .and_then(|id| committed.requirements.get(id))
                .ok_or_else(|| conflict("assessment requirement missing"))?;
            if requirement.mission_id != job.mission_id
                || requirement.summary != REQUIREMENT_SUMMARY
                || requirement.acceptance != acceptance(binding)
                || requirement.status != RequirementStatus::Unmet
                || requirement.owner != Owner::MissionController
                || requirement.triggering_evidence != vec![observation.observation_id.clone()]
            {
                return Err(conflict("assessment requirement changed"));
            }
        } else if proof.requirement_id.is_some() {
            return Err(conflict("feasible assessment created replacement work"));
        }
    } else if proof.observation_id.is_some() || proof.requirement_id.is_some() {
        return Err(conflict("rejected assessment has mission effects"));
    }
    Ok(proof)
}

pub async fn read(app: &AppState, id: &str) -> ApiResult<AssessmentRecord> {
    let _guard = app.kernel.lock().await;
    read_locked(app, id)
}
pub(crate) fn read_locked(app: &AppState, id: &str) -> ApiResult<AssessmentRecord> {
    let binding = app.store.assessment_binding(id)?;
    let job = app.store.get_job(&binding.job.job_id)?;
    if job_spec(&job) != binding.job {
        return Err(conflict("assessment job changed"));
    }
    let proof = app
        .store
        .assessment_decision(id)?
        .map(|stored| restore(app, &binding, &job, stored))
        .transpose()?;
    let (attempts, attention): (u32, Option<String>) = app
        .store
        .lock()
        .query_row(
            "SELECT finalization_attempts,attention FROM venue_assessments WHERE assessment_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StoreError::from)?;
    Ok(AssessmentRecord {
        assessment_id: id.into(),
        job_id: job.job_id,
        source_job_id: binding.context.source_job_id,
        role_id: job.role_id,
        responsibility_id: job.responsibility_id,
        status: job.status,
        policy: binding.context.policy,
        proof,
        finalization_attempts: attempts,
        attention: attention.or(job.rejection_reason),
    })
}

pub async fn finalize(app: &AppState, id: &str) -> ApiResult<AssessmentRecord> {
    let _guard = app.kernel.lock().await;
    finalize_locked(app, id)
}
fn finalize_locked(app: &AppState, id: &str) -> ApiResult<AssessmentRecord> {
    let binding = app.store.assessment_binding(id)?;
    let job = app.store.get_job(&binding.job.job_id)?;
    if app.store.assessment_decision(id)?.is_some() {
        return read_locked(app, id);
    }
    if job.status != JobStatus::Verifying {
        return Err(conflict(
            "assessment must collect native output before verification",
        ));
    }
    if job_spec(&job) != binding.job {
        return Err(conflict("assessment job changed"));
    }
    validate_source(app, &binding)?;
    let native = receipt(app, &binding)?;
    let state = app.current_state()?;
    let now = Utc::now();
    let mut proof = compute_proof(&state, &binding, &job, &native, now)?;
    let mut updated = state.clone();
    let mut effects = Vec::new();
    if proof.decision == AssessmentDecision::Accepted {
        let (observation, observation_id) = engine::record_observation(
            &updated,
            &job.mission_id,
            "creao-venue-assessment",
            &summary(&proof, &binding),
            Some(proof.proof_ref.clone()),
            now,
        )
        .map_err(|_| conflict("cannot apply assessment observation"))?;
        updated = observation.state;
        effects.extend(observation.effects);
        proof.observation_id = Some(observation_id.clone());
        if proof
            .output
            .as_ref()
            .is_some_and(|o| o.verdict == AssessmentVerdict::Reopen)
        {
            let (requirement, requirement_id) = engine::record_requirement(
                &updated,
                &job.mission_id,
                REQUIREMENT_SUMMARY,
                &[observation_id],
                &acceptance(&binding),
            )
            .map_err(|_| conflict("cannot apply assessment requirement"))?;
            updated = requirement.state;
            effects.extend(requirement.effects);
            proof.requirement_id = Some(requirement_id);
        }
        // Observation and replacement requirement are one atomic kernel
        // mutation; the intermediate pure-engine state is never committed.
        updated.revision = state.revision + 1;
    }
    let status = if proof.decision == AssessmentDecision::Accepted {
        JobStatus::Succeeded
    } else {
        JobStatus::ResultRejected
    };
    let mut terminal = job.clone();
    let from = terminal
        .set_status(status, now)
        .map_err(|_| conflict("invalid assessment terminal transition"))?;
    terminal.result = Some(json!({"kind":"venue_assessment", "proof":proof}));
    terminal.raw_status = Some(
        if status == JobStatus::Succeeded {
            "CREAO_ASSESSMENT_VERIFIED"
        } else {
            "CREAO_ASSESSMENT_REJECTED"
        }
        .into(),
    );
    terminal.rejection_reason =
        (status == JobStatus::ResultRejected).then(|| match proof.decision {
            AssessmentDecision::AuthorityRejected => {
                "assessment authority no longer permits application".into()
            }
            _ => "native output failed the frozen venue assessment policy".into(),
        });
    effects.push(EventKind::JobStatusChanged {
        job_id: job.job_id.clone(),
        from,
        to: status,
    });
    if let Some(reason) = &terminal.rejection_reason {
        effects.push(EventKind::JobResultRejected {
            job_id: job.job_id.clone(),
            reason: reason.clone(),
        });
    }
    let stored = StoredDecision {
        decision_revision: state.revision,
        committed_revision: updated.revision,
        verified_at: now,
        observation_id: proof.observation_id.clone(),
        requirement_id: proof.requirement_id.clone(),
        proof: encode(&proof)?,
    };
    let text = serde_json::to_string(&stored)
        .map_err(|_| ApiError::internal("cannot encode assessment proof"))?;
    let events = app.store.commit_state(state.revision, &updated, effects,
        &EventMeta::mission(job.mission_id.clone()).caused_by(id.to_owned()), vec![], |tx| {
        let saved: String = tx.query_row("SELECT job_json FROM jobs WHERE job_id=?1", [job.job_id.as_str()], |row| row.get(0))?;
        if serde_json::from_str::<Job>(&saved)? != job { return Err(StoreError::NotFound("unchanged assessment job".into())); }
        let n = tx.execute("UPDATE venue_assessments SET proof_json=?2,attention=NULL WHERE assessment_id=?1 AND proof_json IS NULL", params![id, text])?;
        if n != 1 { return Err(StoreError::NotFound("unfinalized assessment".into())); }
        put_job(tx, &terminal)
    })?;
    app.broadcast(&events);
    read_locked(app, id)
}

/// Bounded local-only retries; a restart resumes existing intents and never
/// creates a replacement CREAO job. Operator /verify can retry after repair.
pub async fn drain_once(app: &AppState) -> ApiResult<usize> {
    let _guard = app.kernel.lock().await;
    let ids: Vec<String> = {
        let conn = app.store.lock();
        let mut stmt = conn.prepare("SELECT a.assessment_id FROM venue_assessments a JOIN jobs j ON j.job_id=a.job_id WHERE a.proof_json IS NULL AND a.finalization_attempts < ?1 AND j.status='Verifying' ORDER BY a.assessment_id LIMIT 16").map_err(StoreError::from)?;
        let values = stmt
            .query_map([MAX_LOCAL_ATTEMPTS], |row| row.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)?;
        values
    };
    for id in &ids {
        // Claim and finalize share the kernel lock; no native request occurs.
        app.store.lock().execute("UPDATE venue_assessments SET finalization_attempts=finalization_attempts+1,attention='local_verification_pending' WHERE assessment_id=?1", [id]).map_err(StoreError::from)?;
        if finalize_locked(app, id).is_err() {
            app.store.lock().execute("UPDATE venue_assessments SET attention='local_verification_failed' WHERE assessment_id=?1 AND proof_json IS NULL", [id]).map_err(StoreError::from)?;
        }
    }
    Ok(ids.len())
}

pub fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tick.tick().await;
            if let Err(error) = drain_once(&app).await {
                tracing::warn!(code = %error.error, "venue assessment finalization failed");
            }
        }
    })
}
