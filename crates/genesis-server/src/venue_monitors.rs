//! Opt-in, finite subscriptions to verified venue invocation evidence.
//! A subscription never invokes Apify, creates a role, or accepts a provider
//! result itself. Durable intent precedes child preparation; the existing
//! assessment runtime owns native execution and semantic mission effects.
use crate::app::AppState;
use crate::capability_authority::authorize_current;
use crate::capability_invocation::{self, InvocationDecision, InvocationProof};
use crate::error::{ApiError, ApiResult};
use crate::store::{Store, StoreError, StoreResult};
use crate::venue_assessment_domain::{
    self as domain, AssessmentVerdict, VenueAssessmentPolicy, REQUIRED_SCOPE,
};
use crate::venue_assessments::{
    self, AssessmentDecision, AssessmentRecord, CreateAssessmentRequest,
};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::{
    AssignmentId, InstallationId, JobId, MissionId, RequirementId, ResponsibilityId, RoleId,
};
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{InstallationStatus, JobStatus};
use genesis_core::model::{AgentBinding, OrgState, Provider};
use genesis_verification::{decode_json, json_sha256, ArtifactPin};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_RECORD_BYTES: usize = 128 * 1024;
const MAX_ASSESSMENTS: u16 = 16;
const MAX_ACTIVE: u32 = 16;
const EVENT_PAGE: u32 = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateVenueMonitorRequest {
    pub client_request_id: String,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub source_job_id: JobId,
    pub policy: VenueAssessmentPolicy,
    pub expires_at: DateTime<Utc>,
    pub max_assessments: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorStatus {
    Active,
    Attention,
    Completed,
    Stopped,
}
impl MonitorStatus {
    fn open(self) -> bool {
        matches!(self, Self::Active | Self::Attention)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct VenueMonitorRecord {
    pub monitor_id: String,
    pub request: CreateVenueMonitorRequest,
    pub status: MonitorStatus,
    pub attention: Option<String>,
    /// Counts prepared intents, including the explicitly selected first source.
    pub assessment_count: usize,
    pub pending_source_job_id: Option<JobId>,
    pub pending_assessment_id: Option<String>,
    pub last_assessment_id: Option<String>,
    pub requirement_id: Option<RequirementId>,
    pub installation_id: InstallationId,
    pub suite_id: String,
    pub suite_sha256: String,
    pub acceptance_report_ref: String,
    pub event_cursor: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: u32,
    monitor_id: String,
    request: CreateVenueMonitorRequest,
    mission_id: MissionId,
    assignment_id: AssignmentId,
    epoch: u64,
    agent: AgentBinding,
    installation_id: InstallationId,
    suite_id: String,
    suite_sha256: String,
    acceptance_report_ref: String,
    artifact: ArtifactPin,
    event_input: Value,
    initial_proof_ref: String,
    initial_cursor: u64,
    created_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    request: CreateAssessmentRequest,
    source_created_at: DateTime<Utc>,
    source_proof_ref: String,
    assessment_id: Option<String>,
    finished: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skipped: Option<SkippedIntent>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SkippedIntent {
    SourceConsumed,
    SourceExpired,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanCandidate {
    source_job_id: JobId,
    source_created_at: DateTime<Utc>,
    source_proof_ref: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scan {
    /// Finish this durable backlog before choosing its newest-created source.
    through: u64,
    candidate: Option<ScanCandidate>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Monitor {
    binding: Binding,
    binding_sha256: String,
    cursor: u64,
    status: MonitorStatus,
    attention: Option<String>,
    intents: Vec<Intent>,
    last_assessment_id: Option<String>,
    requirement_id: Option<RequirementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scan: Option<Scan>,
    #[serde(skip)]
    revision: u64,
}
impl Monitor {
    fn pending(&self) -> Option<&Intent> {
        self.intents.last().filter(|i| !i.finished)
    }
    fn record(&self) -> VenueMonitorRecord {
        VenueMonitorRecord {
            monitor_id: self.binding.monitor_id.clone(),
            request: self.binding.request.clone(),
            status: self.status,
            attention: self.attention.clone(),
            assessment_count: self.intents.len(),
            pending_source_job_id: self.pending().map(|i| i.request.source_job_id.clone()),
            pending_assessment_id: self.pending().and_then(|i| i.assessment_id.clone()),
            last_assessment_id: self.last_assessment_id.clone(),
            requirement_id: self.requirement_id.clone(),
            installation_id: self.binding.installation_id.clone(),
            suite_id: self.binding.suite_id.clone(),
            suite_sha256: self.binding.suite_sha256.clone(),
            acceptance_report_ref: self.binding.acceptance_report_ref.clone(),
            event_cursor: self.cursor,
        }
    }
    fn attend(&mut self, reason: &'static str) {
        if self.status.open() {
            self.status = MonitorStatus::Attention;
        }
        self.attention = Some(reason.into());
    }
}

fn conflict(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "venue_monitor", message)
}
fn digest(value: &impl Serialize) -> ApiResult<String> {
    Ok(json_sha256(&serde_json::to_value(value).map_err(|_| {
        ApiError::internal("cannot encode venue monitor")
    })?))
}
fn child_key(monitor: &str, source: &JobId) -> ApiResult<String> {
    Ok(format!("venue-monitor:{}", digest(&(monitor, source))?))
}
fn event_input(job: &Job) -> ApiResult<Value> {
    let mut input = job
        .input
        .as_object()
        .cloned()
        .ok_or_else(|| conflict("source input must be an object"))?;
    if input
        .remove("source_url")
        .and_then(|v| v.as_str().map(str::to_owned))
        .is_none()
    {
        return Err(conflict("source input needs a source_url"));
    }
    Ok(Value::Object(input))
}
fn new_intent(
    binding: &Binding,
    job: &Job,
    proof: &InvocationProof,
    now: DateTime<Utc>,
) -> ApiResult<Intent> {
    let deadline = binding
        .request
        .expires_at
        .min(now + Duration::minutes(30))
        .min(job.created_at + Duration::minutes(30))
        .min(proof.verified_at + Duration::minutes(30));
    if deadline <= now {
        return Err(conflict("source freshness window has expired"));
    }
    Ok(Intent {
        request: CreateAssessmentRequest {
            client_request_id: child_key(&binding.monitor_id, &job.job_id)?,
            role_id: binding.request.role_id.clone(),
            responsibility_id: binding.request.responsibility_id.clone(),
            source_job_id: job.job_id.clone(),
            policy: binding.request.policy.clone(),
            deadline,
        },
        source_created_at: job.created_at,
        source_proof_ref: proof.proof_ref.clone(),
        assessment_id: None,
        finished: false,
        skipped: None,
    })
}

impl Store {
    pub fn init_venue_monitors(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS venue_monitors (
            monitor_id TEXT PRIMARY KEY, client_request_id TEXT NOT NULL UNIQUE,
            responsibility_id TEXT NOT NULL, open INTEGER NOT NULL, pending INTEGER NOT NULL,
            revision INTEGER NOT NULL, checked_at TEXT NOT NULL, record_json TEXT NOT NULL);
            CREATE UNIQUE INDEX IF NOT EXISTS venue_monitor_open_responsibility
            ON venue_monitors(responsibility_id) WHERE open=1;",
        )?;
        Ok(())
    }
    fn venue_monitor(&self, id: &str) -> ApiResult<Monitor> {
        let row: Option<(String, String, String, bool, bool, u64)> = self.lock().query_row(
            "SELECT record_json,client_request_id,responsibility_id,open,pending,revision FROM venue_monitors WHERE monitor_id=?1",
            [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
        ).optional().map_err(StoreError::from)?;
        let (text, key, responsibility, open, pending, revision) =
            row.ok_or_else(|| ApiError::not_found("venue monitor"))?;
        if text.len() > MAX_RECORD_BYTES {
            return Err(conflict("stored monitor exceeds its bound"));
        }
        let mut monitor: Monitor =
            decode_json(text.as_bytes()).map_err(|_| conflict("invalid stored monitor"))?;
        let b = &monitor.binding;
        if b.schema_version != 1
            || b.monitor_id != id
            || b.request.client_request_id != key
            || b.request.responsibility_id.as_str() != responsibility
            || digest(b)? != monitor.binding_sha256
            || b.request.max_assessments == 0
            || b.request.max_assessments > MAX_ASSESSMENTS
            || monitor.intents.is_empty()
            || monitor.intents.len() > usize::from(b.request.max_assessments)
            || monitor.cursor < b.initial_cursor
            || open != monitor.status.open()
            || pending != monitor.pending().is_some()
            || monitor.intents[0].request.source_job_id != b.request.source_job_id
            || monitor.intents[0].source_proof_ref != b.initial_proof_ref
        {
            return Err(conflict("stored monitor binding changed"));
        }
        for (index, intent) in monitor.intents.iter().enumerate() {
            if intent.request.client_request_id != child_key(id, &intent.request.source_job_id)?
                || intent.request.role_id != b.request.role_id
                || intent.request.responsibility_id != b.request.responsibility_id
                || intent.request.policy != b.request.policy
                || intent.request.deadline > b.request.expires_at
                || intent.request.deadline <= b.created_at
                || (intent.finished && intent.assessment_id.is_none() && intent.skipped.is_none())
                || (intent.skipped.is_some()
                    && (!intent.finished || intent.assessment_id.is_some()))
                || (index + 1 < monitor.intents.len() && !intent.finished)
                || (index > 0
                    && intent.source_created_at <= monitor.intents[index - 1].source_created_at)
            {
                return Err(conflict("stored monitor intent changed"));
            }
        }
        if let Some(scan) = &monitor.scan {
            if scan.through < monitor.cursor
                || monitor.pending().is_some()
                || scan.candidate.as_ref().is_some_and(|candidate| {
                    candidate.source_created_at <= monitor.intents.last().unwrap().source_created_at
                        || candidate.source_job_id.as_str().is_empty()
                        || candidate.source_job_id.as_str().len() > 256
                        || candidate.source_proof_ref.len() != "invocation:".len() + 64
                        || !candidate.source_proof_ref.starts_with("invocation:")
                        || !candidate.source_proof_ref["invocation:".len()..]
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            {
                return Err(conflict("stored monitor scan changed"));
            }
        }
        monitor.revision = revision;
        Ok(monitor)
    }
    fn save_venue_monitor(&self, monitor: &mut Monitor) -> ApiResult<()> {
        let text = serde_json::to_string(monitor).map_err(StoreError::from)?;
        if text.len() > MAX_RECORD_BYTES {
            return Err(conflict("monitor exceeds storage bound"));
        }
        let changed = self.lock().execute(
            "UPDATE venue_monitors SET record_json=?1,open=?2,pending=?3,revision=revision+1,checked_at=?4 WHERE monitor_id=?5 AND revision=?6",
            params![text, monitor.status.open(), monitor.pending().is_some(), Utc::now().to_rfc3339(), monitor.binding.monitor_id, monitor.revision],
        ).map_err(StoreError::from)?;
        if changed != 1 {
            return Err(conflict("monitor changed concurrently"));
        }
        monitor.revision += 1;
        Ok(())
    }
}

fn source(app: &AppState, id: &JobId) -> ApiResult<(Job, InvocationProof)> {
    let proof = capability_invocation::read_locked(app, id)?;
    let job = app.store.get_job(id)?;
    if proof.decision != InvocationDecision::Verified
        || job.status != JobStatus::Succeeded
        || job.org_id != app.org_id
        || job.provider != Some(Provider::Apify)
    {
        return Err(conflict(
            "monitor source must be a verified local invocation",
        ));
    }
    Ok((job, proof))
}

fn current(
    app: &AppState,
    state: &OrgState,
    binding: &Binding,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let grant = authorize_current(
        state,
        &binding.request.role_id,
        &binding.request.responsibility_id,
        Some(REQUIRED_SCOPE),
        now,
    )?;
    if grant.assignment_id != binding.assignment_id
        || grant.epoch != binding.epoch
        || state
            .roles
            .get(&binding.request.role_id)
            .and_then(|r| r.binding.as_ref())
            != Some(&binding.agent)
        || state
            .responsibilities
            .get(&binding.request.responsibility_id)
            .is_none_or(|r| r.mission_id != binding.mission_id)
    {
        return Err(conflict("monitor's frozen authority changed"));
    }
    let installation = state
        .installations
        .get(&binding.installation_id)
        .ok_or_else(|| conflict("monitor installation missing"))?;
    if installation.status != InstallationStatus::Accepted
        || installation.acceptance_report_ref.as_deref() != Some(&binding.acceptance_report_ref)
        || installation.capability.capability_id.as_str() != binding.artifact.capability_id
        || installation.capability.version != binding.artifact.capability_version
        || installation.source_digest != binding.artifact.source_sha256
    {
        return Err(conflict("monitor installation changed"));
    }
    engine::authorize_capability(state, &binding.request.role_id, &installation.capability)?;
    // Historical verification can outlive the producer's role. Reconstruct it
    // instead of trusting an event, a caller-supplied proof or a bare status.
    let (_, proof) = source(app, &binding.request.source_job_id)?;
    if !matches_pin(binding, &proof) || proof.proof_ref != binding.initial_proof_ref {
        return Err(conflict("monitor's frozen source evidence changed"));
    }
    Ok(())
}
fn matches_pin(binding: &Binding, proof: &InvocationProof) -> bool {
    proof.installation_id == binding.installation_id
        && proof.suite_id == binding.suite_id
        && proof.suite_sha256 == binding.suite_sha256
        && proof.acceptance_report_ref == binding.acceptance_report_ref
        && proof
            .case_report
            .as_ref()
            .is_some_and(|r| r.artifact == binding.artifact)
}

pub async fn create(
    app: &AppState,
    request: CreateVenueMonitorRequest,
) -> ApiResult<(VenueMonitorRecord, bool)> {
    let _guard = app.kernel.lock().await;
    if request.client_request_id.trim().is_empty() || request.client_request_id.len() > 128 {
        return Err(ApiError::bad_request(
            "client_request_id must contain 1 to 128 bytes",
        ));
    }
    let existing: Option<String> = app
        .store
        .lock()
        .query_row(
            "SELECT monitor_id FROM venue_monitors WHERE client_request_id=?1",
            [&request.client_request_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)?;
    if let Some(id) = existing {
        let monitor = app.store.venue_monitor(&id)?;
        if monitor.binding.request != request {
            return Err(conflict("request key already binds a different monitor"));
        }
        return Ok((monitor.record(), false));
    }
    let now = Utc::now();
    if request.expires_at <= now
        || request.expires_at > now + Duration::hours(24)
        || request.max_assessments == 0
        || request.max_assessments > MAX_ASSESSMENTS
    {
        return Err(ApiError::bad_request(
            "monitor needs expiry within 24 hours and 1 to 16 assessment intents",
        ));
    }
    domain::validate_policy(&request.policy)
        .map_err(|_| ApiError::bad_request("invalid venue assessment policy"))?;
    let state = app.current_state()?;
    let (job, proof) = source(app, &request.source_job_id)?;
    if job.responsibility_id != request.responsibility_id
        || job.created_at > now
        || job.created_at < now - Duration::minutes(30)
        || proof.verified_at > now
        || proof.verified_at < now - Duration::minutes(30)
    {
        return Err(conflict(
            "initial source must be a fresh invocation for this responsibility",
        ));
    }
    let agent = state
        .roles
        .get(&request.role_id)
        .and_then(|r| r.binding.clone())
        .filter(|b| b.provider == Provider::Creao)
        .ok_or_else(|| conflict("monitor needs a native CREAO binding"))?;
    let grant = authorize_current(
        &state,
        &request.role_id,
        &request.responsibility_id,
        Some(REQUIRED_SCOPE),
        now,
    )?;
    let cursor: u64 = app
        .store
        .lock()
        .query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))
        .map_err(StoreError::from)?;
    let binding = Binding {
        schema_version: 1,
        monitor_id: format!("monitor_{}", uuid::Uuid::new_v4()),
        request,
        mission_id: job.mission_id.clone(),
        assignment_id: grant.assignment_id,
        epoch: grant.epoch,
        agent,
        installation_id: proof.installation_id.clone(),
        suite_id: proof.suite_id.clone(),
        suite_sha256: proof.suite_sha256.clone(),
        acceptance_report_ref: proof.acceptance_report_ref.clone(),
        artifact: proof
            .case_report
            .as_ref()
            .ok_or_else(|| conflict("source has no case proof"))?
            .artifact
            .clone(),
        event_input: event_input(&job)?,
        initial_proof_ref: proof.proof_ref.clone(),
        initial_cursor: cursor,
        created_at: now,
    };
    current(app, &state, &binding, now)?;
    app.providers.specialist(Provider::Creao)?;
    let intake = venue_assessments::intake_locked(app, &binding.request.responsibility_id)?;
    if intake.consumed(job.created_at) {
        return Err(conflict(
            "initial source was already assessed or superseded",
        ));
    }
    if intake.unresolved {
        return Err(conflict(
            "responsibility already has an unresolved assessment",
        ));
    }
    let initial = new_intent(&binding, &job, &proof, now)?;
    let monitor = Monitor {
        binding_sha256: digest(&binding)?,
        binding,
        cursor,
        status: MonitorStatus::Active,
        attention: None,
        intents: vec![initial],
        last_assessment_id: None,
        requirement_id: None,
        scan: None,
        revision: 0,
    };
    let text = serde_json::to_string(&monitor).map_err(StoreError::from)?;
    if text.len() > MAX_RECORD_BYTES {
        return Err(ApiError::bad_request("monitor exceeds storage bound"));
    }
    let mut conn = app.store.lock();
    let tx = conn.transaction().map_err(StoreError::from)?;
    let active: u32 = tx
        .query_row(
            "SELECT COUNT(*) FROM venue_monitors WHERE open=1",
            [],
            |r| r.get(0),
        )
        .map_err(StoreError::from)?;
    let occupied: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM venue_monitors WHERE responsibility_id=?1 AND open=1)",
            [monitor.binding.request.responsibility_id.as_str()],
            |r| r.get(0),
        )
        .map_err(StoreError::from)?;
    if active >= MAX_ACTIVE || occupied {
        return Err(conflict(
            "monitor capacity or responsibility already occupied",
        ));
    }
    tx.execute("INSERT INTO venue_monitors (monitor_id,client_request_id,responsibility_id,open,pending,revision,checked_at,record_json) VALUES (?1,?2,?3,1,1,0,?4,?5)",
        params![monitor.binding.monitor_id, monitor.binding.request.client_request_id, monitor.binding.request.responsibility_id.as_str(), now.to_rfc3339(), text]).map_err(StoreError::from)?;
    tx.commit().map_err(StoreError::from)?;
    Ok((monitor.record(), true))
}

pub async fn read(app: &AppState, id: &str) -> ApiResult<VenueMonitorRecord> {
    let _guard = app.kernel.lock().await;
    Ok(app.store.venue_monitor(id)?.record())
}
pub async fn stop(app: &AppState, id: &str) -> ApiResult<VenueMonitorRecord> {
    let _guard = app.kernel.lock().await;
    let mut monitor = app.store.venue_monitor(id)?;
    if monitor.status.open() {
        monitor.status = MonitorStatus::Stopped;
        monitor.attention = Some("stopped_by_operator".into());
        app.store.save_venue_monitor(&mut monitor)?;
    }
    Ok(monitor.record())
}

fn observe_child(monitor: &mut Monitor, child: AssessmentRecord) -> ApiResult<()> {
    let intent = monitor
        .intents
        .last_mut()
        .ok_or_else(|| conflict("monitor has no intent"))?;
    if child.source_job_id != intent.request.source_job_id
        || child.role_id != intent.request.role_id
        || child.responsibility_id != intent.request.responsibility_id
        || child.policy != intent.request.policy
        || intent
            .assessment_id
            .as_ref()
            .is_some_and(|id| id != &child.assessment_id)
    {
        return Err(conflict("monitor child identity changed"));
    }
    intent.assessment_id = Some(child.assessment_id.clone());
    if child.status == JobStatus::Succeeded {
        let proof = child
            .proof
            .as_ref()
            .filter(|p| p.decision == AssessmentDecision::Accepted)
            .ok_or_else(|| conflict("successful monitor child has no accepted proof"))?;
        intent.finished = true;
        monitor.last_assessment_id = Some(child.assessment_id);
        if monitor.status.open() {
            monitor.status = MonitorStatus::Active;
            monitor.attention = None;
            if proof
                .output
                .as_ref()
                .is_some_and(|o| o.verdict == AssessmentVerdict::Reopen)
            {
                if proof.requirement_id.is_none() {
                    return Err(conflict("reopen proof has no requirement"));
                }
                monitor.status = MonitorStatus::Completed;
                monitor.attention = Some("replacement_requirement_created".into());
            }
        }
        monitor.requirement_id = proof.requirement_id.clone();
    } else if matches!(
        child.status,
        JobStatus::Failed | JobStatus::ResultRejected | JobStatus::Unknown
    ) {
        monitor.attend("assessment_needs_attention");
    } else if child.finalization_attempts >= 3 && child.attention.is_some() {
        monitor.attend("assessment_verification_needs_attention");
    }
    Ok(())
}

fn fresh(job: &Job, proof: &InvocationProof, now: DateTime<Utc>) -> bool {
    job.created_at <= now
        && job.created_at > now - Duration::minutes(30)
        && proof.verified_at <= now
        && proof.verified_at > now - Duration::minutes(30)
}

fn watermark(app: &AppState) -> ApiResult<u64> {
    app.store
        .lock()
        .query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))
        .map_err(StoreError::from)
        .map_err(Into::into)
}

fn skip_pending(monitor: &mut Monitor, reason: SkippedIntent) {
    let intent = monitor.intents.last_mut().unwrap();
    intent.finished = true;
    intent.skipped = Some(reason);
}

fn advance(app: &AppState, monitor: &mut Monitor) -> ApiResult<()> {
    let now = Utc::now();
    if monitor.status.open() && monitor.binding.request.expires_at <= now {
        monitor.status = MonitorStatus::Stopped;
        monitor.attention = Some("expired".into());
    }
    if let Some(intent) = monitor.pending().cloned() {
        // Historical lookup precedes intake, even after stop/expiry/reassignment.
        // Missing provider configuration can prevent receipt reconstruction;
        // retain the exact child intent until its journal is available again.
        let child = match venue_assessments::lookup_request_locked(app, &intent.request) {
            Err(error)
                if error.error == "provider_configuration"
                    && app.providers.specialist(Provider::Creao).is_err() =>
            {
                return Ok(());
            }
            result => result?,
        };
        if let Some(child) = child {
            observe_child(monitor, child)?;
        } else if monitor.status == MonitorStatus::Active {
            current(app, &app.current_state()?, &monitor.binding, now)?;
            let (job, proof) = source(app, &intent.request.source_job_id)?;
            if !matches_pin(&monitor.binding, &proof)
                || proof.proof_ref != intent.source_proof_ref
                || job.created_at != intent.source_created_at
                || event_input(&job)? != monitor.binding.event_input
            {
                return Err(conflict("pending monitor source changed"));
            }
            let intake =
                venue_assessments::intake_locked(app, &monitor.binding.request.responsibility_id)?;
            if intake.consumed(job.created_at) {
                // An operator may prepare the source after our intent commit.
                // Keep its reserved count, but never replay it or halt newer work.
                skip_pending(monitor, SkippedIntent::SourceConsumed);
            } else if intent.request.deadline <= now || !fresh(&job, &proof, now) {
                skip_pending(monitor, SkippedIntent::SourceExpired);
            } else if intake.unresolved || app.providers.specialist(Provider::Creao).is_err() {
                return Ok(());
            } else {
                let (child, _) = venue_assessments::create_locked(app, intent.request)?;
                observe_child(monitor, child)?;
            }
        }
        // An existing pending/unknown/failed child never permits a later source.
        if monitor.pending().is_some() {
            return Ok(());
        }
    }
    if monitor.status != MonitorStatus::Active {
        return Ok(());
    }
    if monitor.intents.len() >= usize::from(monitor.binding.request.max_assessments) {
        monitor.status = MonitorStatus::Completed;
        monitor.attention = Some("assessment_limit_reached".into());
        return Ok(());
    }
    current(app, &app.current_state()?, &monitor.binding, now)?;
    let intake = venue_assessments::intake_locked(app, &monitor.binding.request.responsibility_id)?;
    let floor = intake
        .source_floor
        .map_or(monitor.intents.last().unwrap().source_created_at, |floor| {
            floor.max(monitor.intents.last().unwrap().source_created_at)
        });
    let latest = watermark(app)?;
    let scan = monitor.scan.get_or_insert(Scan {
        through: latest,
        candidate: None,
    });
    // If delivery was blocked by another assessment/provider configuration,
    // include the new backlog before acting on the saved candidate.
    if monitor.cursor == scan.through {
        scan.through = latest;
    }
    if scan.through > latest || monitor.cursor > scan.through {
        return Err(conflict("monitor event watermark changed"));
    }
    if scan
        .candidate
        .as_ref()
        .is_some_and(|c| c.source_created_at <= floor)
    {
        scan.candidate = None;
    }
    for event in app.store.events_after(monitor.cursor, EVENT_PAGE)? {
        if event.seq > scan.through {
            break;
        }
        monitor.cursor = event.seq;
        let EventKind::JobStatusChanged {
            job_id,
            to: JobStatus::Succeeded,
            ..
        } = event.kind
        else {
            continue;
        };
        let job = app.store.get_job(&job_id)?;
        if job.org_id != app.org_id
            || job.mission_id != monitor.binding.mission_id
            || job.responsibility_id != monitor.binding.request.responsibility_id
            || job.provider != Some(Provider::Apify)
            || job.status != JobStatus::Succeeded
            || job.created_at <= floor
            || job.created_at > now
            || job.created_at <= now - Duration::minutes(30)
        {
            continue;
        }
        let verified: bool = app
            .store
            .lock()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM capability_invocation_verifications WHERE job_id=?1)",
                [job_id.as_str()],
                |r| r.get(0),
            )
            .map_err(StoreError::from)?;
        if !verified {
            continue;
        }
        let (_, proof) = source(app, &job_id)?;
        if !matches_pin(&monitor.binding, &proof)
            || event_input(&job)? != monitor.binding.event_input
            || !fresh(&job, &proof, now)
        {
            continue;
        }
        let newer = scan.candidate.as_ref().is_none_or(|candidate| {
            (job.created_at, job_id.as_str())
                > (
                    candidate.source_created_at,
                    candidate.source_job_id.as_str(),
                )
        });
        if newer {
            scan.candidate = Some(ScanCandidate {
                source_job_id: job_id,
                source_created_at: job.created_at,
                source_proof_ref: proof.proof_ref,
            });
        }
    }
    if monitor.cursor < scan.through {
        return Ok(());
    }
    let Some(candidate) = scan.candidate.clone() else {
        monitor.scan = None;
        return Ok(());
    };
    // A saved candidate is only a discovery hint. Reconstruct the proof again
    // after restart and just before spending a bounded intent.
    let (job, proof) = source(app, &candidate.source_job_id)?;
    if job.created_at != candidate.source_created_at
        || job.mission_id != monitor.binding.mission_id
        || job.responsibility_id != monitor.binding.request.responsibility_id
        || proof.proof_ref != candidate.source_proof_ref
        || !matches_pin(&monitor.binding, &proof)
        || event_input(&job)? != monitor.binding.event_input
    {
        return Err(conflict("saved monitor candidate changed"));
    }
    if !fresh(&job, &proof, now) || intake.consumed(job.created_at) {
        monitor.scan = None;
        return Ok(());
    }
    if intake.unresolved || app.providers.specialist(Provider::Creao).is_err() {
        return Ok(());
    }
    monitor
        .intents
        .push(new_intent(&monitor.binding, &job, &proof, now)?);
    monitor.scan = None;
    Ok(())
}

pub async fn drain_once(app: &AppState) -> ApiResult<usize> {
    let _guard = app.kernel.lock().await;
    let ids: Vec<String> = {
        let conn = app.store.lock();
        let mut statement = conn.prepare("SELECT monitor_id FROM venue_monitors WHERE open=1 OR pending=1 ORDER BY checked_at,monitor_id LIMIT 16").map_err(StoreError::from)?;
        let ids = statement
            .query_map([], |r| r.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?;
        ids
    };
    for id in &ids {
        let mut monitor = match app.store.venue_monitor(id) {
            Ok(monitor) => monitor,
            Err(error) => {
                // Corrupt rows cannot monopolize a bounded worker pass.
                app.store
                    .lock()
                    .execute(
                        "UPDATE venue_monitors SET checked_at=?1 WHERE monitor_id=?2",
                        params![Utc::now().to_rfc3339(), id],
                    )
                    .map_err(StoreError::from)?;
                tracing::warn!(code=%error.error, "venue monitor binding could not be restored");
                continue;
            }
        };
        if let Err(error) = advance(app, &mut monitor) {
            monitor.attend("monitor_preparation_needs_attention");
            tracing::warn!(code=%error.error, "venue monitor stopped before new work");
        }
        app.store.save_venue_monitor(&mut monitor)?;
    }
    Ok(ids.len())
}

pub fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tick.tick().await;
            if let Err(error) = drain_once(&app).await {
                tracing::warn!(code=%error.error, "venue monitor worker failed");
            }
        }
    })
}
