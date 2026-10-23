//! A bounded replacement responsibility delegated to an independent Fetch peer.
//! Native message receipts and independently checked venue decisions are separate.
//! The organization journal owns exact intents before the transport journal sees
//! them. A restart never creates a second contract, task, or CREAO assessment.

use crate::app::AppState;
use crate::capability_authority::authorize_job;
use crate::error::{validation_error, ApiError, ApiResult};
use crate::store::{insert_job_tx, put_job, EventMeta, Store, StoreError, StoreResult};
use crate::venue_assessment_domain::{AssessmentVerdict, REQUIRED_SCOPE as MONITOR_SCOPE};
use crate::venue_assessments::{self, AssessmentDecision, CreateAssessmentRequest};
use crate::venue_recovery_domain::{self as domain, RecoveryContext, RecoveryOutput};
use crate::venue_recovery_sources::{self as sources, SourceBinding};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use genesis_core::change::*;
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::*;
use genesis_core::jobs::{DispatchRequest, Job};
use genesis_core::lifecycle::{ContractState, JobStatus};
use genesis_core::model::{
    AgentBinding, ContractorAgreement, Executor, OrgState, Provider, Requirement,
};
use genesis_fetch::client::{BridgeClient, ClientLimits};
use genesis_fetch::protocol::{self, Body, Message, SendRequest, Task, PROTOCOL_DIGEST};
use genesis_fetch::runtime::{ContractPhase, RuntimeStore, TaskPhase};
use genesis_verification::{decode_json, json_sha256};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const SCOPE: &str = "contract.logistics";
const CAPABILITY: &str = "venue.select";
const TASK: &str = "GENESIS verified venue replacement v1: select from the frozen candidate facts; return the exact venue_replacement_v1 contract. No booking or mission-completion claim.";
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_SEND_ATTEMPTS: u32 = 3;

/// Operator-selected private bridge and independent specialist. No discovery
/// response may replace the trusted local URL or selected peer.
pub struct FetchRecoveryRuntime {
    client: BridgeClient,
    journal: Arc<RuntimeStore>,
    peer: String,
    gate: Mutex<()>,
}

impl FetchRecoveryRuntime {
    pub fn new(
        client: BridgeClient,
        journal: Arc<RuntimeStore>,
        peer: String,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            protocol::address(&peer) && peer != journal.bridge_address(),
            "invalid Fetch recovery peer"
        );
        Ok(Self {
            client,
            journal,
            peer,
            gate: Mutex::new(()),
        })
    }
}

/// All five variables are required together. Credentials stay in the client;
/// the organization records only the stable bridge/peer identities.
pub fn attach_from_env(mut app: AppState) -> anyhow::Result<AppState> {
    let names = [
        "GENESIS_FETCH_BRIDGE_URL",
        "GENESIS_FETCH_BRIDGE_TOKEN",
        "GENESIS_FETCH_BRIDGE_ADDRESS",
        "GENESIS_FETCH_SPECIALIST_ADDRESS",
        "GENESIS_FETCH_RUNTIME_DB",
    ];
    let values: Vec<_> = names.iter().map(std::env::var).collect();
    if values
        .iter()
        .all(|value| matches!(value, Err(std::env::VarError::NotPresent)))
    {
        return Ok(app);
    }
    anyhow::ensure!(
        values
            .iter()
            .all(|value| value.as_ref().is_ok_and(|text| !text.is_empty())),
        "Fetch recovery requires all five GENESIS_FETCH_* configuration variables"
    );
    let values: Vec<_> = values.into_iter().collect::<Result<_, _>>()?;
    let client = BridgeClient::new(
        &values[0],
        &values[1],
        &values[2],
        PROTOCOL_DIGEST,
        ClientLimits::default(),
    )?;
    let journal = Arc::new(RuntimeStore::open(
        &values[4],
        app.org_id.as_str(),
        &values[2],
    )?);
    app.fetch_recovery = Some(Arc::new(FetchRecoveryRuntime::new(
        client,
        journal,
        values[3].clone(),
    )?));
    Ok(app)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRecoveryRequest {
    pub client_request_id: String,
    pub reopened_assessment_id: String,
    pub candidate_job_ids: Vec<JobId>,
    pub deadline: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Recruiting,
    Accepting,
    Selecting,
    Assessing,
    Completed,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    recovery_id: String,
    request: CreateRecoveryRequest,
    bridge_address: String,
    peer_address: String,
    contract: ContractorAgreement,
    requirement: Requirement,
    agent: AgentBinding,
    job: Job,
    source: SourceBinding,
    context: RecoveryContext,
    recruit: Message,
    task: Message,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    binding: Binding,
    revision: u64,
    phase: RecoveryPhase,
    attention: Option<String>,
    accept: Option<Message>,
    send_attempts: BTreeMap<String, u32>,
    next_check_at: DateTime<Utc>,
    selection: Option<RecoveryOutput>,
    receipt_sha256: Option<String>,
    proof_ref: Option<String>,
    observation_id: Option<ObservationId>,
    child_request: Option<CreateAssessmentRequest>,
    child_id: Option<String>,
    child_proof_ref: Option<String>,
}

/// Contains public identities and proof references, never the raw message input.
#[derive(Clone, Debug, Serialize)]
pub struct RecoveryRecord {
    pub recovery_id: String,
    pub phase: RecoveryPhase,
    pub attention: Option<String>,
    pub requirement_id: RequirementId,
    pub responsibility_id: ResponsibilityId,
    pub contract_id: ContractId,
    pub job_id: JobId,
    pub recruit_message_id: String,
    pub accept_message_id: Option<String>,
    pub task_message_id: String,
    pub receipt_sha256: Option<String>,
    pub proof_ref: Option<String>,
    pub selected_venue_id: Option<String>,
    pub assessment_id: Option<String>,
    pub assessment_proof_ref: Option<String>,
}

impl Recovery {
    fn record(&self) -> RecoveryRecord {
        RecoveryRecord {
            recovery_id: self.binding.recovery_id.clone(),
            phase: self.phase,
            attention: self.attention.clone(),
            requirement_id: self.binding.requirement.requirement_id.clone(),
            responsibility_id: self.binding.job.responsibility_id.clone(),
            contract_id: self.binding.contract.contract_id.clone(),
            job_id: self.binding.job.job_id.clone(),
            recruit_message_id: self.binding.recruit.message_id.clone(),
            accept_message_id: self.accept.as_ref().map(|m| m.message_id.clone()),
            task_message_id: self.binding.task.message_id.clone(),
            receipt_sha256: self.receipt_sha256.clone(),
            proof_ref: self.proof_ref.clone(),
            selected_venue_id: self
                .selection
                .as_ref()
                .and_then(|s| s.selected.as_ref())
                .map(|s| s.venue_id.clone()),
            assessment_id: self.child_id.clone(),
            assessment_proof_ref: self.child_proof_ref.clone(),
        }
    }
    fn pause(&mut self, reason: &'static str) {
        self.attention = Some(reason.into());
    }
}

fn conflict(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "venue_recovery", message)
}
fn value(data: &impl Serialize) -> ApiResult<Value> {
    serde_json::to_value(data).map_err(|_| ApiError::internal("cannot encode venue recovery"))
}
fn digest(data: &impl Serialize) -> ApiResult<String> {
    Ok(json_sha256(&value(data)?))
}
fn encode(record: &Recovery) -> ApiResult<String> {
    let data = value(record)?;
    let text = serde_json::to_string(&json!({"data":data,"sha256":json_sha256(&data)}))
        .map_err(|_| ApiError::internal("cannot encode venue recovery"))?;
    if text.len() > MAX_RECORD_BYTES {
        return Err(conflict("venue recovery exceeds storage bound"));
    }
    Ok(text)
}

impl Store {
    pub fn init_venue_recoveries(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS venue_recoveries (
          recovery_id TEXT PRIMARY KEY, client_request_id TEXT NOT NULL UNIQUE,
          requirement_id TEXT NOT NULL UNIQUE, contract_id TEXT NOT NULL UNIQUE,
          job_id TEXT NOT NULL UNIQUE REFERENCES jobs(job_id), revision INTEGER NOT NULL,
          active INTEGER NOT NULL, next_check_at TEXT NOT NULL, data TEXT NOT NULL);",
        )?;
        Ok(())
    }
    fn recovery_by_request(&self, key: &str) -> ApiResult<Option<String>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT recovery_id FROM venue_recoveries WHERE client_request_id=?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)?)
    }
    fn recovery(&self, id: &str) -> ApiResult<Recovery> {
        let row: Option<(String, u64, String, String, String, String)> = self.lock().query_row(
            "SELECT data,revision,client_request_id,requirement_id,contract_id,job_id FROM venue_recoveries WHERE recovery_id=?1 AND length(CAST(data AS BLOB))<=?2",
            params![id, MAX_RECORD_BYTES], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional().map_err(StoreError::from)?;
        let (text, revision, key, requirement, contract, job) =
            row.ok_or_else(|| ApiError::not_found("venue recovery"))?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Sealed {
            data: Value,
            sha256: String,
        }
        let sealed: Sealed =
            decode_json(text.as_bytes()).map_err(|_| conflict("invalid venue recovery record"))?;
        if json_sha256(&sealed.data) != sealed.sha256 {
            return Err(conflict("venue recovery digest changed"));
        }
        let record: Recovery = serde_json::from_value(sealed.data)
            .map_err(|_| conflict("invalid venue recovery record"))?;
        let b = &record.binding;
        if b.recovery_id != id
            || record.revision != revision
            || b.request.client_request_id != key
            || b.requirement.requirement_id.as_str() != requirement
            || b.contract.contract_id.as_str() != contract
            || b.job.job_id.as_str() != job
        {
            return Err(conflict("venue recovery index changed"));
        }
        validate_binding(b)?;
        if let Some(selection) = &record.selection {
            domain::verify_output(&b.context, &value(selection)?)
                .map_err(|_| conflict("stored selection is invalid"))?;
        }
        Ok(record)
    }
    pub(crate) fn recovery_manages_contract(&self, id: &ContractId) -> ApiResult<bool> {
        Ok(self
            .lock()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM venue_recoveries WHERE contract_id=?1)",
                [id.as_str()],
                |row| row.get(0),
            )
            .map_err(StoreError::from)?)
    }
    pub(crate) fn recovery_manages_job(&self, job: &Job) -> ApiResult<bool> {
        if managed(job) {
            return Ok(true);
        }
        Ok(self
            .lock()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM venue_recoveries WHERE job_id=?1)",
                [job.job_id.as_str()],
                |row| row.get(0),
            )
            .map_err(StoreError::from)?)
    }
}

pub(crate) fn managed(job: &Job) -> bool {
    job.task == TASK
}

fn persist_tx(
    tx: &Transaction<'_>,
    record: &Recovery,
    text: &str,
    expected: u64,
) -> StoreResult<()> {
    let changed = tx.execute("UPDATE venue_recoveries SET revision=?1,active=?2,next_check_at=?3,data=?4 WHERE recovery_id=?5 AND revision=?6", params![record.revision,record.phase != RecoveryPhase::Completed && record.attention.is_none(),record.next_check_at.to_rfc3339(),text,record.binding.recovery_id,expected])?;
    if changed != 1 {
        return Err(StoreError::RevisionConflict {
            expected,
            actual: expected + 1,
        });
    }
    Ok(())
}
fn save(app: &AppState, record: &mut Recovery) -> ApiResult<()> {
    let expected = record.revision;
    record.revision += 1;
    record.next_check_at = Utc::now() + Duration::seconds(1);
    let text = encode(record)?;
    let mut conn = app.store.lock();
    let tx = conn.transaction().map_err(StoreError::from)?;
    persist_tx(&tx, record, &text, expected)?;
    tx.commit().map_err(StoreError::from)?;
    Ok(())
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
fn message(b: &Binding, body: Body) -> Message {
    Message {
        schema_version: 1,
        message_id: uuid::Uuid::new_v4().to_string(),
        org_id: b.job.org_id.to_string(),
        mission_id: b.job.mission_id.to_string(),
        contract_id: b.contract.contract_id.to_string(),
        correlation_id: b.job.correlation_id.to_string(),
        expires_at: b.request.deadline,
        body,
    }
}
fn validate_binding(b: &Binding) -> ApiResult<()> {
    if b.context.recovery_id != b.recovery_id
        || b.job != job_spec(&b.job)
        || b.job.task != TASK
        || b.job.provider != Some(Provider::Fetch)
        || b.job.deadline != Some(b.request.deadline)
        || b.job.input
            != domain::provider_input(&b.context)
                .map_err(|_| conflict("invalid recovery context"))?
        || b.contract.specialist_address != b.peer_address
        || b.contract.protocol != PROTOCOL_DIGEST
        || b.contract.scope != SCOPE
        || b.contract.deadline != Some(b.request.deadline)
        || b.contract.supervising_role.as_ref() != Some(&b.job.role_id)
        || b.agent.provider != Provider::Creao
    {
        return Err(conflict("recovery binding changed"));
    }
    for m in [&b.recruit, &b.task] {
        if m.validate().is_err()
            || m.org_id != b.job.org_id.as_str()
            || m.mission_id != b.job.mission_id.as_str()
            || m.contract_id != b.contract.contract_id.as_str()
            || m.correlation_id != b.job.correlation_id.as_str()
            || m.expires_at != b.request.deadline
        {
            return Err(conflict("invalid frozen Fetch message"));
        }
    }
    let Body::TaskRequest { task } = &b.task.body else {
        return Err(conflict("invalid frozen Fetch task"));
    };
    if task.job_id != b.job.job_id.as_str()
        || task.attempt_id != b.job.attempt_id.as_str()
        || task.role_id != b.job.role_id.as_str()
        || task.assignment_id != b.job.assignment_id.as_str()
        || task.epoch != b.job.epoch
        || task.idempotency_key != b.job.idempotency_key
        || task.input != b.job.input
        || task.deadline != b.request.deadline
    {
        return Err(conflict("Fetch task binding changed"));
    }
    Ok(())
}

fn authorize(
    app: &AppState,
    runtime: &FetchRecoveryRuntime,
    state: &OrgState,
    record: &Recovery,
) -> ApiResult<()> {
    let b = &record.binding;
    let now = Utc::now();
    if runtime.journal.org_id() != app.org_id.as_str()
        || runtime.journal.bridge_address() != b.bridge_address
        || runtime.peer != b.peer_address
        || record.attention.is_some()
        || record.phase == RecoveryPhase::Completed
    {
        return Err(conflict("recovery runtime or operational state changed"));
    }
    let job = app.store.get_job(&b.job.job_id)?;
    if job_spec(&job) != b.job {
        return Err(conflict("recovery job changed"));
    }
    authorize_job(state, &job, Some(SCOPE), now)?;
    authorize_job(state, &job, Some(MONITOR_SCOPE), now)?;
    if state.requirements.get(&b.requirement.requirement_id) != Some(&b.requirement)
        || state
            .roles
            .get(&job.role_id)
            .and_then(|role| role.binding.as_ref())
            != Some(&b.agent)
        || state
            .responsibilities
            .get(&job.responsibility_id)
            .and_then(|r| r.executor.as_ref())
            != Some(&Executor::Contractor(b.contract.contract_id.clone()))
    {
        return Err(conflict("recovery authority changed"));
    }
    let mut expected = b.contract.clone();
    expected.state = match record.phase {
        RecoveryPhase::Recruiting => ContractState::Requested,
        RecoveryPhase::Accepting => ContractState::Offered,
        _ => ContractState::Active,
    };
    if state.contracts.get(&b.contract.contract_id) != Some(&expected) {
        return Err(conflict("recovery contract changed"));
    }
    sources::revalidate(app, &b.source)?;
    Ok(())
}

pub async fn create(
    app: &AppState,
    request: CreateRecoveryRequest,
) -> ApiResult<(RecoveryRecord, bool)> {
    let _kernel = app.kernel.lock().await;
    if let Some(id) = app.store.recovery_by_request(&request.client_request_id)? {
        let saved = app.store.recovery(&id)?;
        if saved.binding.request != request {
            return Err(conflict("request key already binds another recovery"));
        }
        return Ok((saved.record(), false));
    }
    if !protocol::identifier(&request.client_request_id)
        || request.deadline.timestamp_subsec_nanos() % 1000 != 0
    {
        return Err(ApiError::bad_request(
            "recovery needs an ASCII request key and a microsecond-precision deadline",
        ));
    }
    let runtime = app
        .fetch_recovery
        .as_ref()
        .ok_or_else(|| conflict("Fetch recovery is not configured"))?;
    if runtime.journal.org_id() != app.org_id.as_str() {
        return Err(conflict("Fetch journal belongs to another organization"));
    }
    let resolved = sources::resolve(
        app,
        &request.reopened_assessment_id,
        &request.candidate_job_ids,
        request.deadline,
    )?;
    let proof = resolved
        .assessment
        .proof
        .as_ref()
        .ok_or_else(|| conflict("missing reopened proof"))?;
    let requirement_id = proof
        .requirement_id
        .clone()
        .ok_or_else(|| conflict("missing replacement requirement"))?;
    let observation_id = proof
        .observation_id
        .clone()
        .ok_or_else(|| conflict("missing reopened observation"))?;
    let state = app.current_state()?;
    let role_id = resolved.assessment.role_id.clone();
    let agent = state
        .roles
        .get(&role_id)
        .and_then(|role| role.binding.clone())
        .filter(|b| b.provider == Provider::Creao)
        .ok_or_else(|| conflict("recovery needs the existing CREAO monitor"))?;
    app.providers.specialist(Provider::Creao)?;
    let total: u64 = app
        .store
        .lock()
        .query_row("SELECT count(*) FROM venue_recoveries", [], |row| {
            row.get(0)
        })
        .map_err(StoreError::from)?;
    if total >= 256 {
        return Err(conflict("venue recovery journal is full"));
    }
    let proposal = OrgChangeProposal { expected_org_revision: state.revision, trigger_evidence_ids: vec![observation_id], rationale: "A verified venue assessment reopened this requirement; recruit a scoped independent replacement selector.".into(), budget_limit_cents: Some(0), operations: vec![
        OrgOp::AssignResponsibility { target: ResponsibilityTarget::FromRequirement { key:"replacement".into(), requirement_id:requirement_id.clone(), purpose:"Find and independently assess a replacement under the original event constraints".into(), trigger:"Accepted venue assessment reopened the event requirement".into(), termination:"Replacement assessment is accepted or the frozen recovery expires".into() }, owner:RoleRef::existing(role_id.clone()) },
        OrgOp::RecruitSpecialist { key:"selector".into(), specialist_address:runtime.peer.clone(), protocol:PROTOCOL_DIGEST.into(), scope:SCOPE.into(), deadline:Some(request.deadline), supervising_role:Some(RoleRef::existing(role_id.clone())) },
        OrgOp::DelegateTask { responsibility:ResponsibilityRef::New {key:"replacement".into()}, executor:ExecutorRef::Contractor {contract:ContractRef::New {key:"selector".into()}}, task:TASK.into() },
    ]};
    let alloc = KeyAllocation::allocate(&proposal);
    let mut outcome = engine::commit(
        &state,
        &proposal,
        &alloc,
        &BTreeMap::new(),
        &engine::local_catalog(&state),
        Utc::now(),
    )
    .map_err(validation_error)?;
    let recovery_id = format!("recovery_{}", uuid::Uuid::new_v4());
    let context = RecoveryContext {
        recovery_id: recovery_id.clone(),
        policy_sha256: digest(&resolved.assessment.policy)?,
        policy: resolved.assessment.policy.clone(),
        candidates: resolved.candidates,
    };
    let dispatch = DispatchRequest {
        client_request_id: format!("recovery:{}", uuid::Uuid::new_v4()),
        role_id,
        responsibility_id: alloc.responsibilities["replacement"].clone(),
        task: TASK.into(),
        input: domain::provider_input(&context)
            .map_err(|_| conflict("invalid replacement candidates"))?,
        required_scope: Some(SCOPE.into()),
        capability: None,
        provider: Some(Provider::Fetch),
        deadline: Some(request.deadline),
    };
    let (job, effects, _) = crate::routes::jobs::new_job(app, &outcome.state, &dispatch)?;
    outcome.effects.extend(effects);
    let base = Message { schema_version:1, message_id:uuid::Uuid::new_v4().to_string(), org_id:job.org_id.to_string(), mission_id:job.mission_id.to_string(), contract_id:alloc.contracts["selector"].to_string(), correlation_id:job.correlation_id.to_string(), expires_at:request.deadline, body:Body::RecruitRequest { requirement:"Choose a feasible replacement venue from independently verified facts under the original event constraints.".into(), capabilities:vec![CAPABILITY.into()], lease_expires_at:request.deadline } };
    let task = Message {
        message_id: uuid::Uuid::new_v4().to_string(),
        body: Body::TaskRequest {
            task: Task {
                job_id: job.job_id.to_string(),
                attempt_id: job.attempt_id.to_string(),
                role_id: job.role_id.to_string(),
                assignment_id: job.assignment_id.to_string(),
                idempotency_key: job.idempotency_key.clone(),
                epoch: job.epoch,
                deadline: request.deadline,
                input: job.input.clone(),
            },
        },
        ..base.clone()
    };
    let binding = Binding {
        recovery_id,
        request,
        bridge_address: runtime.journal.bridge_address().into(),
        peer_address: runtime.peer.clone(),
        contract: outcome.state.contracts[&alloc.contracts["selector"]].clone(),
        requirement: outcome.state.requirements[&requirement_id].clone(),
        agent,
        job,
        source: resolved.binding,
        context,
        recruit: base,
        task,
    };
    validate_binding(&binding)?;
    let record = Recovery {
        binding,
        revision: 0,
        phase: RecoveryPhase::Recruiting,
        attention: None,
        accept: None,
        send_attempts: BTreeMap::new(),
        next_check_at: Utc::now(),
        selection: None,
        receipt_sha256: None,
        proof_ref: None,
        observation_id: None,
        child_request: None,
        child_id: None,
        child_proof_ref: None,
    };
    // Job does not exist yet; perform the same live grant checks on the frozen job.
    authorize_job(&outcome.state, &record.binding.job, Some(SCOPE), Utc::now())?;
    authorize_job(
        &outcome.state,
        &record.binding.job,
        Some(MONITOR_SCOPE),
        Utc::now(),
    )?;
    let text = encode(&record)?;
    let b = &record.binding;
    let events = app.store.commit_state(
        state.revision,
        &outcome.state,
        outcome.effects,
        &EventMeta::mission(b.job.mission_id.clone()).caused_by(b.recovery_id.clone()),
        Vec::new(),
        |tx| {
            insert_job_tx(tx, &dispatch.client_request_id, &b.job)?;
            tx.execute(
                "INSERT INTO venue_recoveries VALUES (?1,?2,?3,?4,?5,0,1,?6,?7)",
                params![
                    b.recovery_id,
                    b.request.client_request_id,
                    b.requirement.requirement_id.as_str(),
                    b.contract.contract_id.as_str(),
                    b.job.job_id.as_str(),
                    record.next_check_at.to_rfc3339(),
                    text
                ],
            )?;
            Ok(())
        },
    )?;
    app.broadcast(&events);
    Ok((record.record(), true))
}

fn commit_record(
    app: &AppState,
    record: &mut Recovery,
    before: u64,
    outcome: engine::Outcome,
    job: Option<&Job>,
) -> ApiResult<()> {
    let expected = record.revision;
    record.revision += 1;
    record.next_check_at = Utc::now() + Duration::seconds(1);
    let text = encode(record)?;
    let events = app.store.commit_state(
        before,
        &outcome.state,
        outcome.effects,
        &EventMeta::mission(record.binding.job.mission_id.clone())
            .caused_by(record.binding.recovery_id.clone()),
        Vec::new(),
        |tx| {
            if let Some(job) = job {
                put_job(tx, job)?;
            }
            persist_tx(tx, record, &text, expected)
        },
    )?;
    app.broadcast(&events);
    Ok(())
}

fn accept_offer(
    app: &AppState,
    runtime: &FetchRecoveryRuntime,
    record: &mut Recovery,
) -> ApiResult<bool> {
    let contract = runtime
        .journal
        .get(record.binding.contract.contract_id.as_str())
        .map_err(|_| conflict("Fetch contract journal unavailable"))?;
    let Some(offer) = contract.offer else {
        return Ok(false);
    };
    let Body::Offer {
        offer_id,
        capabilities,
        lease_expires_at,
        ..
    } = &offer.body
    else {
        return Err(conflict("invalid Fetch offer"));
    };
    if capabilities != &[CAPABILITY.to_owned()]
        || *lease_expires_at != record.binding.request.deadline
    {
        return Err(conflict("Fetch offer does not match the requested work"));
    }
    record.accept = Some(message(
        &record.binding,
        Body::AcceptOffer {
            offer_message_id: offer.message_id,
            offer_id: offer_id.clone(),
        },
    ));
    let state = app.current_state()?;
    authorize(app, runtime, &state, record)?;
    let outcome = engine::update_contract(
        &state,
        &record.binding.contract.contract_id,
        ContractState::Offered,
    )?;
    record.phase = RecoveryPhase::Accepting;
    commit_record(app, record, state.revision, outcome, None)?;
    Ok(true)
}

fn accept_contract(
    app: &AppState,
    runtime: &FetchRecoveryRuntime,
    record: &mut Recovery,
) -> ApiResult<bool> {
    let contract = runtime
        .journal
        .get(record.binding.contract.contract_id.as_str())
        .map_err(|_| conflict("Fetch contract journal unavailable"))?;
    if contract.phase != ContractPhase::Active {
        return Ok(false);
    }
    let accept = record
        .accept
        .as_ref()
        .ok_or_else(|| conflict("missing saved offer acceptance"))?;
    runtime
        .journal
        .recruitment_receipt(
            &record.binding.peer_address,
            &record.binding.recruit,
            accept,
        )
        .map_err(|_| conflict("Fetch consent evidence failed verification"))?;
    let state = app.current_state()?;
    authorize(app, runtime, &state, record)?;
    let outcome = engine::update_contract(
        &state,
        &record.binding.contract.contract_id,
        ContractState::Active,
    )?;
    record.phase = RecoveryPhase::Selecting;
    commit_record(app, record, state.revision, outcome, None)?;
    Ok(true)
}

fn collect_selection(
    app: &AppState,
    runtime: &FetchRecoveryRuntime,
    record: &mut Recovery,
) -> ApiResult<bool> {
    let b = &record.binding;
    let contract = runtime
        .journal
        .get(b.contract.contract_id.as_str())
        .map_err(|_| conflict("Fetch contract journal unavailable"))?;
    if contract
        .tasks
        .get(&b.task.message_id)
        .is_none_or(|task| task.phase != TaskPhase::Collected)
    {
        return Ok(false);
    }
    let receipt = runtime
        .journal
        .task_receipt(
            &b.peer_address,
            &b.recruit,
            record
                .accept
                .as_ref()
                .ok_or_else(|| conflict("missing saved consent"))?,
            &b.task,
        )
        .map_err(|_| conflict("Fetch task receipt failed verification"))?;
    // Retain native provenance even when domain verification refuses the output.
    record.receipt_sha256 = Some(receipt.receipt_sha256().into());
    let selected = domain::verify_output(&b.context, receipt.output())
        .map_err(|_| conflict("Fetch selection does not match verified venue facts"))?;
    let output = selected.output().clone();
    let proof_ref = format!(
        "fetch-recovery:{}",
        digest(&(&b.context, receipt.receipt_sha256(), &output))?
    );
    let state = app.current_state()?;
    authorize(app, runtime, &state, record)?;
    let (mut outcome, observation) = engine::record_observation(&state, &b.job.mission_id, "fetch:venue-replacement", "An independent Fetch specialist evaluated the frozen replacement candidates; the backend recomputed every constraint and the deterministic choice. No booking or mission completion is implied.", Some(proof_ref.clone()), Utc::now())?;
    let mut job = app.store.get_job(&b.job.job_id)?;
    for to in [
        JobStatus::Submitting,
        JobStatus::Running,
        JobStatus::Verifying,
        JobStatus::Succeeded,
    ] {
        let from = job
            .set_status(to, Utc::now())
            .map_err(|_| conflict("recovery job lifecycle changed"))?;
        outcome.effects.push(EventKind::JobStatusChanged {
            job_id: job.job_id.clone(),
            from,
            to,
        });
    }
    job.provider_refs
        .insert("fetch_receipt".into(), receipt.receipt_sha256().into());
    job.provider_refs.insert(
        "fetch_task_request".into(),
        receipt.task_request_id().into(),
    );
    job.provider_refs.insert(
        "fetch_result_message".into(),
        receipt.result_message_id().into(),
    );
    job.raw_status = Some("FETCH_SELECTION_VERIFIED".into());
    job.result = Some(json!({"proof_ref":proof_ref,"selected":output.selected}));
    record.selection = Some(output.clone());
    record.proof_ref = Some(proof_ref);
    record.observation_id = Some(observation);
    if let Some(chosen) = output.selected {
        let mut policy = b.context.policy.clone();
        policy.selected_venue_id = chosen.venue_id;
        record.child_request = Some(CreateAssessmentRequest {
            client_request_id: format!("fetch-recovery:{}", b.recovery_id),
            role_id: b.job.role_id.clone(),
            responsibility_id: b.job.responsibility_id.clone(),
            source_job_id: chosen.source_job_id,
            policy,
            deadline: b.request.deadline,
        });
        record.phase = RecoveryPhase::Assessing;
    } else {
        record.pause("no feasible replacement in the frozen candidate evidence");
    }
    commit_record(app, record, state.revision, outcome, Some(&job))?;
    Ok(true)
}

/// Recover an already committed child before checking live authority. This is
/// historical linking only; a missing child still requires fresh authorization.
fn observe_child(app: &AppState, record: &mut Recovery) -> ApiResult<bool> {
    let Some(request) = &record.child_request else {
        return Ok(false);
    };
    let Some(child) = venue_assessments::lookup_request_locked(app, request)? else {
        return Ok(false);
    };
    if record
        .child_id
        .as_ref()
        .is_some_and(|id| id != &child.assessment_id)
    {
        return Err(conflict("recovery child identity changed"));
    }
    record.child_id = Some(child.assessment_id);
    if let Some(proof) = child.proof {
        if proof.decision == AssessmentDecision::Accepted
            && proof
                .output
                .as_ref()
                .is_some_and(|output| output.verdict == AssessmentVerdict::Feasible)
        {
            record.child_proof_ref = Some(proof.proof_ref);
            record.phase = RecoveryPhase::Completed;
            record.attention = None;
        } else {
            record.pause("replacement assessment was not accepted as feasible");
        }
    } else if child.attention.is_some()
        || matches!(
            child.status,
            JobStatus::Failed | JobStatus::ResultRejected | JobStatus::Unknown
        )
    {
        record.pause("replacement assessment needs attention; original child identity retained");
    }
    Ok(true)
}

/// One bounded exchange step. The gate serializes inbox cursors and exact
/// message attempts; the kernel is never held across transport awaits.
pub async fn step(app: &AppState, id: &str) -> ApiResult<RecoveryRecord> {
    let runtime = app
        .fetch_recovery
        .as_ref()
        .ok_or_else(|| conflict("Fetch recovery is not configured; saved work remains pending"))?;
    let _gate = runtime.gate.lock().await;
    let mut record = {
        let _kernel = app.kernel.lock().await;
        let mut record = app.store.recovery(id)?;
        if record.phase == RecoveryPhase::Completed {
            return Ok(record.record());
        }
        if observe_child(app, &mut record)? {
            save(app, &mut record)?;
            if record.phase == RecoveryPhase::Completed || record.attention.is_some() {
                return Ok(record.record());
            }
        }
        record
    };
    // Read-only polling retains authenticated late replies even when work has
    // since been paused. It never authorizes their application to the mission.
    let cursor = runtime
        .journal
        .cursor()
        .map_err(|_| conflict("Fetch cursor unavailable"))?;
    let page = runtime.client.inbox(cursor, 50).await;
    let to_send = {
        let _kernel = app.kernel.lock().await;
        record = app.store.recovery(id)?;
        match page {
            Ok(page) => {
                runtime
                    .journal
                    .ingest(&page, Utc::now())
                    .map_err(|_| conflict("Fetch inbox journal rejected the page"))?;
            }
            Err(_) => {
                record.pause("Fetch inbox unavailable; exact identities retained");
                save(app, &mut record)?;
                return Ok(record.record());
            }
        }
        if record.attention.is_some() {
            return Ok(record.record());
        }
        if authorize(app, runtime, &app.current_state()?, &record).is_err() {
            record.pause("recovery authority, source evidence, or configuration changed");
            save(app, &mut record)?;
            return Ok(record.record());
        }
        if record.phase == RecoveryPhase::Assessing {
            if !observe_child(app, &mut record)? {
                let request = record
                    .child_request
                    .clone()
                    .ok_or_else(|| conflict("missing frozen assessment intent"))?;
                match venue_assessments::create_locked(app, request) {
                    Ok((child, _)) => {
                        record.child_id = Some(child.assessment_id);
                    }
                    Err(_) => record.pause(
                        "replacement assessment preparation needs attention; intent retained",
                    ),
                }
            }
            save(app, &mut record)?;
            return Ok(record.record());
        }
        // Replaying local preparation is idempotent and can recover a prior
        // cross-journal interruption. No new UUID or deadline is generated.
        let frozen = match record.phase {
            RecoveryPhase::Recruiting => record.binding.recruit.clone(),
            RecoveryPhase::Accepting => record
                .accept
                .clone()
                .ok_or_else(|| conflict("missing saved acceptance"))?,
            RecoveryPhase::Selecting => record.binding.task.clone(),
            _ => unreachable!(),
        };
        if runtime
            .journal
            .prepare(&record.binding.peer_address, frozen.clone(), Utc::now())
            .is_err()
        {
            record.pause("Fetch intent preparation conflicts with its durable journal");
            save(app, &mut record)?;
            return Ok(record.record());
        }
        let checkpoint = record.clone();
        let advanced = match record.phase {
            RecoveryPhase::Recruiting => accept_offer(app, runtime, &mut record),
            RecoveryPhase::Accepting => accept_contract(app, runtime, &mut record),
            RecoveryPhase::Selecting => collect_selection(app, runtime, &mut record),
            _ => unreachable!(),
        };
        match advanced {
            Ok(true) => return Ok(record.record()),
            Err(_) => {
                record = checkpoint;
                record.pause(
                    "Fetch evidence or selection failed verification; exact identities retained",
                );
                save(app, &mut record)?;
                return Ok(record.record());
            }
            Ok(false) => {}
        }
        let pending = runtime
            .journal
            .pending(Utc::now())
            .map_err(|_| conflict("Fetch outbox unavailable"))?;
        let Some(request) = pending
            .into_iter()
            .find(|request| request.message.message_id == frozen.message_id)
        else {
            save(app, &mut record)?;
            return Ok(record.record());
        };
        if request.message != frozen || request.recipient != record.binding.peer_address {
            return Err(conflict("Fetch pending request changed"));
        }
        let attempts = record.send_attempts.entry(frozen.message_id).or_default();
        if *attempts >= MAX_SEND_ATTEMPTS {
            record
                .pause("Fetch delivery requires reconciliation or an explicit exact-message retry");
            save(app, &mut record)?;
            return Ok(record.record());
        }
        *attempts += 1;
        save(app, &mut record)?;
        request
    };
    let expected_revision = record.revision;
    let response = runtime
        .client
        .send_guarded(&to_send, || {
            let Ok(_kernel) = app.kernel.try_lock() else {
                return false;
            };
            app.store
                .recovery(id)
                .and_then(|saved| {
                    if saved.revision != expected_revision {
                        return Err(conflict("recovery checkpoint changed"));
                    }
                    authorize(app, runtime, &app.current_state()?, &saved)
                })
                .is_ok()
        })
        .await;
    let _kernel = app.kernel.lock().await;
    let mut saved = app.store.recovery(id)?;
    if saved.revision != expected_revision {
        return Err(conflict("recovery checkpoint changed while sending"));
    }
    match response {
        Ok(receipt) => runtime
            .journal
            .acknowledge(&receipt)
            .map_err(|_| conflict("Fetch queue receipt could not be saved"))?,
        Err(error)
            if !error.post_may_have_run
                && error.kind == genesis_fetch::client::ClientErrorKind::NoLongerEligible =>
        {
            // Lock contention and operational pauses are not revocation. A later
            // tick rechecks all authority before attempting the same message.
            if let Some(attempts) = saved.send_attempts.get_mut(&to_send.message.message_id) {
                *attempts = attempts.saturating_sub(1);
            }
        }
        Err(_) => {
            if saved
                .send_attempts
                .get(&to_send.message.message_id)
                .copied()
                .unwrap_or(0)
                >= MAX_SEND_ATTEMPTS
            {
                saved.pause(
                    "Fetch delivery outcome uncertain; reconcile or retry the exact saved message",
                );
            }
        }
    }
    save(app, &mut saved)?;
    Ok(saved.record())
}

pub async fn read(app: &AppState, id: &str) -> ApiResult<RecoveryRecord> {
    let _kernel = app.kernel.lock().await;
    Ok(app.store.recovery(id)?.record())
}

/// Clear an operational hold only; immutable grants, messages and deadlines do
/// not change. Failed authority remains fenced by the next step.
pub async fn retry(app: &AppState, id: &str) -> ApiResult<RecoveryRecord> {
    let runtime = app
        .fetch_recovery
        .as_ref()
        .ok_or_else(|| conflict("Fetch recovery is not configured"))?;
    let _gate = runtime.gate.lock().await;
    let _kernel = app.kernel.lock().await;
    let mut record = app.store.recovery(id)?;
    if record.phase == RecoveryPhase::Completed {
        return Ok(record.record());
    }
    if record
        .selection
        .as_ref()
        .is_some_and(|selection| selection.selected.is_none())
    {
        return Err(conflict(
            "the frozen evidence contains no feasible replacement",
        ));
    }
    record.attention = None;
    authorize(app, runtime, &app.current_state()?, &record)?;
    record.send_attempts.clear();
    save(app, &mut record)?;
    Ok(record.record())
}

pub async fn drain_once(app: &AppState) -> ApiResult<usize> {
    if app.fetch_recovery.is_none() {
        return Ok(0);
    }
    let ids: Vec<String> = {
        let conn = app.store.lock();
        let mut stmt = conn.prepare("SELECT recovery_id FROM venue_recoveries WHERE active=1 AND next_check_at<=?1 ORDER BY next_check_at LIMIT 16").map_err(StoreError::from)?;
        stmt.query_map([Utc::now().to_rfc3339()], |row| row.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?
    };
    for id in &ids {
        step(app, id).await?;
    }
    Ok(ids.len())
}

pub fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tick.tick().await;
            if let Err(error) = drain_once(&app).await {
                tracing::warn!(code=%error.error, "venue recovery needs attention");
            }
        }
    })
}
