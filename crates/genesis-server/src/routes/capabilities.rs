//! Evidence-backed capability publication, independent adoption and execution.

use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_authority::{authorize_current, authorize_job};
use crate::capability_evidence::{EvidencePolicyError, VerifiedEvidence};
use crate::capability_runtime::{
    self, CapabilityRunRecord, CapabilityRuntimeError, ExecutionPurpose,
};
use crate::capability_store::{self, LocalAcceptance};
use crate::error::{validation_error, ApiError, ApiResult};
use crate::registry::{SearchCandidate, SearchRequest};
use crate::store::{EventMeta, StoreError};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use genesis_apify::RunRequest;
use genesis_core::capability::CapabilityManifest;
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::*;
use genesis_core::jobs::{DispatchRequest, Job};
use genesis_core::lifecycle::{CapabilityState, InstallationStatus, JobStatus};
use genesis_core::model::{Installation, OrgState, Provider};
use genesis_verification::{validate_artifact, ArtifactPin, SourceSnapshot};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const DEV_ASSERTION: &str = "genesis:development-assertion";

fn registry(app: &AppState) -> ApiResult<&crate::registry::RegistryClient> {
    app.registry.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_configuration",
            "no capability registry is configured",
        )
    })
}

pub(crate) fn development(app: &AppState) -> bool {
    app.providers.dev_stub_enabled()
        && app
            .providers
            .executor()
            .is_ok_and(|executor| executor.provider() == Provider::DevStub)
}

fn evidence_error(error: EvidencePolicyError) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "verification_failed",
        error.to_string(),
    )
}

impl From<CapabilityRuntimeError> for ApiError {
    fn from(error: CapabilityRuntimeError) -> Self {
        match error {
            CapabilityRuntimeError::Store(error) => error.into(),
            other => ApiError::new(
                StatusCode::CONFLICT,
                "capability_run_state",
                other.to_string(),
            ),
        }
    }
}

pub(crate) fn artifact(manifest: &CapabilityManifest) -> ArtifactPin {
    ArtifactPin {
        capability_id: manifest.capability_id.to_string(),
        capability_version: manifest.version.clone(),
        source_sha256: manifest.source_digest.clone(),
        actor_id: manifest.artifact.actor_id.clone(),
        build_id: manifest.artifact.build_id.clone(),
        build_number: manifest.artifact.build_number.clone(),
    }
}

pub(crate) fn check_manifest(app: &AppState, manifest: &CapabilityManifest) -> ApiResult<()> {
    if !development(app) {
        if manifest.provenance.iter().any(|item| item == DEV_ASSERTION) {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "development_evidence",
                "development assertions cannot be installed by a real executor",
            ));
        }
        validate_artifact(&artifact(manifest)).map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    manifest.check_publishable().map_err(|errors| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "verification_failed",
            errors.join("; "),
        )
    })
}

fn key(value: &str) -> ApiResult<()> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request("client_request_id must contain 1–128 non-control characters without surrounding whitespace"));
    }
    Ok(())
}

fn installed<'a>(
    state: &'a OrgState,
    id: &InstallationId,
    expected: InstallationStatus,
) -> ApiResult<&'a Installation> {
    let installation = state
        .installations
        .get(id)
        .ok_or_else(|| ApiError::not_found(format!("installation {id}")))?;
    if installation.status != expected {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "installation_state",
            format!("installation must be {expected:?}"),
        ));
    }
    Ok(installation)
}

pub(crate) fn authorize_permissions(
    state: &OrgState,
    role: &RoleId,
    responsibility: &ResponsibilityId,
    manifest: &CapabilityManifest,
) -> ApiResult<()> {
    let now = Utc::now();
    // Authority is required even when the manifest declares no extra scopes.
    authorize_current(state, role, responsibility, None, now)?;
    for scope in &manifest.required_permissions {
        authorize_current(state, role, responsibility, Some(scope), now)?;
    }
    Ok(())
}

pub async fn search(
    State(app): State<AppState>,
    p: Principal,
    Json(req): Json<SearchRequest>,
) -> ApiResult<Json<Vec<SearchCandidate>>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(registry(&app)?.search(&req).await?))
}

pub async fn manifest(
    State(app): State<AppState>,
    p: Principal,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Json<CapabilityManifest>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(
        registry(&app)?
            .manifest(&CapabilityId::new(id), &version)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePublication {
    manifest: CapabilityManifest,
    verification_ref: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PublishBody {
    Verified(EvidencePublication),
    Development(CapabilityManifest),
}

pub async fn publish(
    State(app): State<AppState>,
    p: Principal,
    Json(body): Json<PublishBody>,
) -> ApiResult<(StatusCode, Json<CapabilityManifest>)> {
    p.require(&[Principal::Operator])?;
    let mut manifest = match body {
        PublishBody::Verified(body) => {
            let verified = if let Some(id) = body.verification_ref.strip_prefix("acquisition:") {
                super::acquisitions::configured(&app)?;
                let record = app
                    .acquisition_runtime
                    .verified_record(id)
                    .await
                    .map_err(super::acquisitions::runtime_error)?;
                let forge = record.session().forge().ok_or_else(|| {
                    ApiError::internal("verified acquisition has no forge checkpoint")
                })?;
                app.evidence_policy
                    .verify_session(record.suite_id(), forge)
                    .map_err(evidence_error)?
            } else {
                app.evidence_policy
                    .verified_forge(&body.verification_ref)
                    .map_err(evidence_error)?
            };
            if artifact(&body.manifest) != verified.artifact {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "artifact_mismatch",
                    "manifest must match the backend verified forge artifact exactly",
                ));
            }
            let mut manifest = body.manifest;
            manifest.acceptance_suite_ref = verified.report.suite_id;
            manifest.verification = verified.kernel_report;
            manifest.provenance.retain(|item| item != DEV_ASSERTION);
            manifest
                .provenance
                .push(format!("genesis:forge:{}", verified.reference));
            manifest.state = CapabilityState::Approved;
            manifest
        }
        PublishBody::Development(mut manifest) if development(&app) => {
            manifest.provenance.push(DEV_ASSERTION.into());
            manifest.verification.limitations.push(
                "Development assertion; no authenticated provider execution was verified".into(),
            );
            manifest
        }
        PublishBody::Development(_) => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "verification_required",
                "publication requires a configured backend forge verification_ref",
            ))
        }
    };
    if manifest.publisher != app.org_id {
        return Err(ApiError::forbidden(
            "manifest publisher is not this organization",
        ));
    }
    manifest.published_at = None;
    check_manifest(&app, &manifest)?;
    let published = registry(&app)?.publish(&manifest).await?;
    Ok((StatusCode::CREATED, Json(published)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallBody {
    pub capability_id: CapabilityId,
    pub version: String,
    #[serde(default)]
    pub configuration: Value,
}

pub async fn install(
    State(app): State<AppState>,
    p: Principal,
    Json(body): Json<InstallBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator])?;
    let manifest = registry(&app)?
        .manifest(&body.capability_id, &body.version)
        .await?;
    check_manifest(&app, &manifest)?;
    let mutation = app
        .mutate_with(
            EventMeta::default(),
            |state| {
                let (outcome, id) =
                    engine::begin_installation(state, &manifest, body.configuration, Utc::now())
                        .map_err(validation_error)?;
                Ok((outcome, id, vec![]))
            },
            |tx, id| capability_store::insert_pin(tx, id, &manifest),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"installation_id":mutation.value,
        "installation":mutation.state.installations.get(&mutation.value),"manifest_source_digest":manifest.source_digest,
        "artifact":manifest.artifact,"synthesis_calls":0,"revision":mutation.state.revision})),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecksBody {
    pub client_request_id: String,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub suite_id: String,
}

/// Shared transaction plan: preparation does no provider I/O. Call under the
/// kernel lock and persist the jobs/batch with any enclosing intent checkpoint.
pub(crate) struct ChecksPlan {
    client_request_id: String,
    batch_digest: String,
    pub job_ids: Vec<JobId>,
    prepared: Vec<(String, Job, CapabilityRunRecord)>,
    pub effects: Vec<EventKind>,
    existing: bool,
}

pub(crate) fn plan_checks(
    app: &AppState,
    state: &OrgState,
    id: &InstallationId,
    body: &ChecksBody,
    adoption: Option<(&str, chrono::DateTime<Utc>)>,
) -> ApiResult<ChecksPlan> {
    key(&body.client_request_id)?;
    if adoption.is_none()
        && app
            .store
            .adoption_owns_installation(id)
            .map_err(|_| ApiError::internal("adoption ownership is unavailable"))?
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "adoption_owned",
            "advance the adoption intent to prepare its checks",
        ));
    }
    if app.providers.executor()?.provider() != Provider::Apify {
        return Err(ApiError::bad_request(
            "acceptance checks require a real Apify executor",
        ));
    }
    let suite = app
        .evidence_policy
        .suite(&body.suite_id)
        .ok_or_else(|| ApiError::bad_request("unknown backend oracle suite"))?;
    let installation = installed(state, id, InstallationStatus::Checking)?;
    let manifest = app.store.capability_manifest(id)?;
    check_manifest(app, &manifest)?;
    authorize_permissions(state, &body.role_id, &body.responsibility_id, &manifest)?;
    let batch_digest = genesis_verification::json_sha256(&json!({
        "installation_id":id,"role_id":body.role_id,"responsibility_id":body.responsibility_id,
        "suite":suite,"artifact":artifact(&manifest),"required_permissions":manifest.required_permissions,
    }));
    let batch_digest = if let Some((adoption_id, deadline)) = adoption {
        genesis_verification::json_sha256(
            &json!({"batch":batch_digest,"adoption_id":adoption_id,"deadline":deadline}),
        )
    } else {
        batch_digest
    };
    let purpose = match adoption {
        Some((adoption_id, _)) => ExecutionPurpose::AdoptionAcceptance {
            adoption_id: adoption_id.to_owned(),
            installation_id: id.clone(),
            suite_id: body.suite_id.clone(),
        },
        None => ExecutionPurpose::LocalAcceptance {
            installation_id: id.clone(),
            suite_id: body.suite_id.clone(),
        },
    };
    if let Some(existing) = app.store.capability_check_batch(&body.client_request_id)? {
        if existing.request_sha256 != batch_digest {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "check request key is bound to a different batch",
            ));
        }
        return Ok(ChecksPlan {
            client_request_id: body.client_request_id.clone(),
            batch_digest,
            job_ids: existing.job_ids,
            prepared: vec![],
            effects: vec![],
            existing: true,
        });
    }
    let mut prepared = Vec::new();
    let mut effects = Vec::new();
    let mut job_ids = Vec::new();
    for case in &suite.cases {
        let client_key = format!(
            "check:{}:{}",
            body.client_request_id,
            genesis_verification::json_sha256(&json!(case.case_id))
        );
        let request = RunRequest {
            case_id: case.case_id.clone(),
            artifact: artifact(&manifest),
            input: case.input.clone(),
            source: case.source.clone(),
        };
        let dispatch = DispatchRequest {
            client_request_id: client_key.clone(),
            role_id: body.role_id.clone(),
            responsibility_id: body.responsibility_id.clone(),
            task: format!("accept {id} with {}", body.suite_id),
            input: case.input.clone(),
            required_scope: None,
            capability: None,
            provider: Some(Provider::Apify),
            deadline: adoption.map(|(_, deadline)| deadline),
        };
        let (mut job, mut job_effects, _) = super::jobs::new_job(app, state, &dispatch)?;
        job.capability = Some(installation.capability.clone());
        if let Some(existing) = app.store.find_capability_run(&client_key)? {
            let stored = app.store.get_job(&existing.job_id)?;
            if existing.purpose != purpose
                || existing.required_permissions != manifest.required_permissions
                || stored.role_id != job.role_id
                || stored.responsibility_id != job.responsibility_id
                || existing.request.input != request.input
                || existing.request.artifact != request.artifact
                || existing.request.source != request.source
            {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "idempotency_conflict",
                    "acceptance key is bound to a different request",
                ));
            }
            job_ids.push(existing.job_id);
            continue;
        }
        if app.store.find_capability_job(&client_key)?.is_some() {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "check job key is already in use",
            ));
        }
        let record = CapabilityRunRecord::new_with_purpose(&job, request, None, purpose.clone())?
            .with_required_permissions(&job, manifest.required_permissions.clone())?;
        effects.append(&mut job_effects);
        effects.push(EventKind::CapabilityRunRequested {
            installation_id: id.clone(),
            capability: installation.capability.clone(),
            role_id: job.role_id.clone(),
            job_id: job.job_id.clone(),
        });
        job_ids.push(job.job_id.clone());
        prepared.push((client_key, job, record));
    }
    Ok(ChecksPlan {
        client_request_id: body.client_request_id.clone(),
        batch_digest,
        job_ids,
        prepared,
        effects,
        existing: false,
    })
}

pub(crate) fn persist_checks(
    tx: &rusqlite::Transaction<'_>,
    plan: &ChecksPlan,
) -> crate::store::StoreResult<()> {
    if !plan.existing {
        for (client_key, job, record) in &plan.prepared {
            capability_runtime::insert_in_tx(tx, client_key, job, record)?;
        }
        capability_store::insert_check_batch(
            tx,
            &plan.client_request_id,
            &plan.batch_digest,
            &plan.job_ids,
        )?;
    }
    Ok(())
}

/// Evaluation is a separate, operator-authorized purpose. It requires an active
/// assignment and every declared permission, but cannot grant general use yet.
pub async fn checks(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<ChecksBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator])?;
    let id = InstallationId::new(id);
    let _guard = app.kernel.lock().await;
    if crate::capability_gap_finalizer::owns_installation(&app.store, &id) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "gap_finalizer_owned",
            "this installation's checks belong to its capability gap finalization",
        ));
    }
    let state = app.current_state()?;
    let plan = plan_checks(&app, &state, &id, &body, None)?;
    let status = if plan.prepared.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    let events = app.store.commit_state(
        state.revision,
        &state,
        plan.effects.clone(),
        &EventMeta::default().caused_by(body.client_request_id),
        vec![],
        |tx| persist_checks(tx, &plan),
    )?;
    app.broadcast(&events);
    Ok((
        status,
        Json(
            json!({"installation_id":id,"suite_id":body.suite_id,"job_ids":plan.job_ids,"synthesis_calls":0}),
        ),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceAcceptance {
    pub suite_id: String,
    pub job_ids: Vec<JobId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentAcceptance {
    passed: bool,
    report_ref: String,
    #[serde(default)]
    reasons: Vec<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
pub enum AcceptanceBody {
    Verified(EvidenceAcceptance),
    Development(DevelopmentAcceptance),
}

pub(crate) fn local_runs(
    app: &AppState,
    installation_id: &InstallationId,
    suite_id: &str,
    job_ids: &[JobId],
) -> ApiResult<Vec<genesis_verification::ExecutionEvidence>> {
    let suite = app
        .evidence_policy
        .suite(suite_id)
        .ok_or_else(|| ApiError::bad_request("unknown backend oracle suite"))?;
    if job_ids.len() != suite.cases.len()
        || job_ids.iter().collect::<BTreeSet<_>>().len() != job_ids.len()
    {
        return Err(ApiError::bad_request(
            "supply exactly one distinct locally collected job for each oracle case",
        ));
    }
    let mut evidence = Vec::new();
    for job_id in job_ids {
        let record = app.store.get_capability_run(job_id)?;
        if !record.purpose.is_acceptance_for(installation_id, suite_id) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "evidence_binding",
                "run belongs to a different installation or suite",
            ));
        }
        evidence.push(record.evidence.ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "evidence_pending",
                "all acceptance runs must finish collection",
            )
        })?);
    }
    Ok(evidence)
}

pub(crate) fn local_evidence(
    app: &AppState,
    installation_id: &InstallationId,
    suite_id: &str,
    job_ids: &[JobId],
) -> ApiResult<VerifiedEvidence> {
    let pin = artifact(&app.store.capability_manifest(installation_id)?);
    let runs = local_runs(app, installation_id, suite_id, job_ids)?;
    app.evidence_policy
        .accept_suite(suite_id, &pin, &runs)
        .map_err(evidence_error)
}

pub async fn acceptance(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<AcceptanceBody>,
) -> ApiResult<Json<Value>> {
    p.require(&[Principal::Operator, Principal::Adapter])?;
    let id = InstallationId::new(id);
    let _guard = app.kernel.lock().await;
    if app
        .store
        .adoption_owns_installation(&id)
        .map_err(|_| ApiError::internal("adoption ownership is unavailable"))?
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "adoption_owned",
            "advance the adoption intent to commit its acceptance",
        ));
    }
    if crate::capability_gap_finalizer::owns_installation(&app.store, &id) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "gap_finalizer_owned",
            "this installation's acceptance belongs to its capability gap finalization",
        ));
    }
    let state = app.current_state()?;
    // Replays of completed acceptance are read-only only when the same proof is supplied.
    let mut proof = None;
    let (passed, reference, reasons) = match body {
        AcceptanceBody::Verified(body) => {
            // Adoption acceptance and its intent checkpoint must commit together.
            // Its coordinator owns this transition; the ordinary endpoint cannot
            // leave an accepted installation behind a still-checking intent.
            for job_id in &body.job_ids {
                if matches!(
                    app.store.get_capability_run(job_id)?.purpose,
                    ExecutionPurpose::AdoptionAcceptance { .. }
                ) {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "adoption_owned",
                        "advance the adoption intent to commit its acceptance",
                    ));
                }
            }
            let evidence = local_evidence(&app, &id, &body.suite_id, &body.job_ids)?;
            proof = Some(LocalAcceptance {
                suite_id: body.suite_id,
                job_ids: body.job_ids,
                report_ref: evidence.reference.clone(),
            });
            (true, evidence.reference, vec![])
        }
        AcceptanceBody::Development(body) if development(&app) => (
            body.passed,
            format!("{DEV_ASSERTION}:{}", body.report_ref),
            body.reasons,
        ),
        AcceptanceBody::Development(_) => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "verification_required",
                "acceptance requires backend-collected job_ids and a configured suite_id",
            ))
        }
    };
    let installation = state
        .installations
        .get(&id)
        .ok_or_else(|| ApiError::not_found("installation"))?;
    if installation.status == InstallationStatus::Accepted
        && installation.acceptance_report_ref.as_deref() == Some(&reference)
    {
        return Ok(Json(
            json!({"installation":installation,"revision":state.revision}),
        ));
    }
    installed(&state, &id, InstallationStatus::Checking)?;
    // Initial acceptance requires a live verifier grant. After commitment the
    // semantic proof belongs to the organization and survives verifier handoff.
    if let Some(proof) = &proof {
        let manifest = app.store.capability_manifest(&id)?;
        let now = Utc::now();
        for job_id in &proof.job_ids {
            let job = app.store.get_job(job_id)?;
            let record = app.store.get_capability_run(job_id)?;
            authorize_job(&state, &job, record.required_scope.as_deref(), now)?;
            for scope in &record.required_permissions {
                authorize_job(&state, &job, Some(scope), now)?;
            }
            authorize_permissions(&state, &job.role_id, &job.responsibility_id, &manifest)?;
        }
    }
    let outcome = engine::finish_installation(&state, &id, passed, &reference, reasons)?;
    let events = app.store.commit_state(
        state.revision,
        &outcome.state,
        outcome.effects,
        &EventMeta::default(),
        vec![],
        |tx| {
            if let Some(proof) = &proof {
                capability_store::save_acceptance(tx, &id, proof)?;
            }
            Ok(())
        },
    )?;
    app.broadcast(&events);
    Ok(Json(
        json!({"installation":outcome.state.installations.get(&id),"revision":outcome.state.revision}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    pub client_request_id: String,
    pub installation_id: InstallationId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    #[serde(default)]
    pub input: Value,
    pub required_scope: Option<String>,
    pub suite_id: Option<String>,
}

pub async fn run(
    State(app): State<AppState>,
    p: Principal,
    Json(body): Json<RunBody>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    key(&body.client_request_id)?;
    let _guard = app.kernel.lock().await;
    let state = app.current_state()?;
    let installation = installed(&state, &body.installation_id, InstallationStatus::Accepted)?;
    let manifest = app.store.capability_manifest(&body.installation_id)?;
    check_manifest(&app, &manifest)?;
    authorize_permissions(&state, &body.role_id, &body.responsibility_id, &manifest)?;
    let req = DispatchRequest {
        client_request_id: body.client_request_id.clone(),
        role_id: body.role_id.clone(),
        responsibility_id: body.responsibility_id.clone(),
        task: format!("run {}", body.installation_id),
        input: body.input.clone(),
        required_scope: body.required_scope.clone(),
        capability: Some(installation.capability.clone()),
        provider: Some(Provider::Apify),
        deadline: None,
    };
    let (mut job, mut effects, _) = super::jobs::new_job(&app, &state, &req)?;
    if development(&app) {
        // A stub receipt is deliberately excluded from the durable real-evidence table.
        let request = RunRequest {
            case_id: "development-only".into(),
            artifact: artifact(&manifest),
            input: body.input,
            source: SourceSnapshot {
                url: "https://devstub.invalid/".into(),
                sha256: "0".repeat(64),
            },
        };
        if let Some(existing) = app.store.find_capability_job(&body.client_request_id)? {
            if existing.input != job.input
                || existing.role_id != job.role_id
                || existing.responsibility_id != job.responsibility_id
                || existing.task != job.task
            {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "idempotency_conflict",
                    "request key is already bound",
                ));
            }
            return Ok((StatusCode::OK, Json(existing)));
        }
        let receipt = app.providers.executor()?.submit(&request).await?;
        // The development provider also waits asynchronously; a lease or
        // mission deadline may expire while the kernel state stays unchanged.
        authorize_job(&state, &job, body.required_scope.as_deref(), Utc::now())?;
        for to in [JobStatus::Submitting, JobStatus::Running] {
            let from = job
                .set_status(to, Utc::now())
                .map_err(|e| ApiError::internal(e.to_string()))?;
            effects.push(EventKind::JobStatusChanged {
                job_id: job.job_id.clone(),
                from,
                to,
            });
        }
        job.provider = Some(Provider::DevStub);
        job.raw_status = Some(receipt.raw_status);
        job.provider_refs = receipt.provider_refs;
        effects.push(EventKind::CapabilityRunRequested {
            installation_id: body.installation_id,
            capability: installation.capability.clone(),
            role_id: job.role_id.clone(),
            job_id: job.job_id.clone(),
        });
        let events = app.store.commit_state(
            state.revision,
            &state,
            effects,
            &EventMeta::default(),
            vec![],
            |tx| capability_store::insert_development_job(tx, &body.client_request_id, &job),
        )?;
        app.broadcast(&events);
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }
    let proof = app
        .store
        .capability_acceptance(&body.installation_id)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "verification_required",
                "installation lacks backend local acceptance",
            )
        })?;
    let suite_id = body.suite_id.as_deref().unwrap_or(&proof.suite_id);
    if suite_id != proof.suite_id {
        return Err(ApiError::bad_request(
            "run suite must match local acceptance",
        ));
    }
    let verified = local_evidence(&app, &body.installation_id, suite_id, &proof.job_ids)?;
    if verified.reference != proof.report_ref
        || installation.acceptance_report_ref.as_deref() != Some(&proof.report_ref)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "evidence_binding",
            "local acceptance no longer matches backend policy",
        ));
    }
    let case = app
        .evidence_policy
        .case_for_input(suite_id, &body.input)
        .map_err(evidence_error)?;
    let request = RunRequest {
        case_id: case.case_id.clone(),
        artifact: artifact(&manifest),
        input: body.input,
        source: case.source.clone(),
    };
    if let Some(existing) = app.store.find_capability_run(&body.client_request_id)? {
        let stored = app.store.get_job(&existing.job_id)?;
        if existing.required_permissions != manifest.required_permissions
            || !existing.matches_submission(&stored, &job, &request, body.required_scope.as_deref())
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "request key is already bound to a different invocation",
            ));
        }
        return Ok((StatusCode::OK, Json(stored)));
    }
    if app
        .store
        .find_capability_job(&body.client_request_id)?
        .is_some()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "request key is already in use by another job",
        ));
    }
    let record = CapabilityRunRecord::new(&job, request, body.required_scope)?
        .with_required_permissions(&job, manifest.required_permissions.clone())?;
    let binding = crate::capability_invocation::prepare_binding(
        &app,
        &job,
        &record,
        &body.installation_id,
        &proof,
    )?;
    effects.push(EventKind::CapabilityRunRequested {
        installation_id: body.installation_id,
        capability: installation.capability.clone(),
        role_id: job.role_id.clone(),
        job_id: job.job_id.clone(),
    });
    let events = app.store.commit_state(
        state.revision,
        &state,
        effects,
        &EventMeta::default().caused_by(body.client_request_id.clone()),
        vec![],
        |tx| {
            capability_runtime::insert_in_tx(tx, &body.client_request_id, &job, &record)?;
            crate::capability_invocation::insert_binding(tx, &binding)
        },
    )?;
    app.broadcast(&events);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub async fn get_run(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<CapabilityRunRecord>> {
    p.require(&[Principal::Operator, Principal::Adapter])?;
    Ok(Json(app.store.get_capability_run(&JobId::new(id))?))
}

pub async fn verify_run(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<crate::capability_invocation::InvocationProof>> {
    p.require(&[Principal::Operator])?;
    if !body.as_object().is_some_and(|object| object.is_empty()) {
        return Err(ApiError::bad_request(
            "verification accepts only an empty object",
        ));
    }
    Ok(Json(
        crate::capability_invocation::finalize(&app, &JobId::new(id)).await?,
    ))
}

pub async fn run_verification(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::capability_invocation::InvocationProof>> {
    p.require(&[Principal::Operator])?;
    Ok(Json(
        crate::capability_invocation::read(&app, &JobId::new(id)).await?,
    ))
}

pub async fn run_finalization(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::capability_finalization::FinalizationStatus>> {
    p.require(&[Principal::Operator])?;
    Ok(Json(
        app.store.capability_finalization_status(&JobId::new(id))?,
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepBody {
    #[serde(default)]
    pub retry_read: bool,
}
pub async fn step_run(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<StepBody>,
) -> ApiResult<Json<CapabilityRunRecord>> {
    p.require(&[Principal::Operator])?;
    Ok(Json(
        app.capability_runtime
            .step(&JobId::new(id), body.retry_read)
            .await?,
    ))
}

/// Generic adapter result JSON may never advance managed real capability jobs.
pub(crate) fn reject_managed_result(app: &AppState, job: &Job) -> ApiResult<()> {
    match app.store.get_capability_run(&job.job_id) {
        Ok(_) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "managed_capability_result",
            "capability evidence is collected by the backend; caller result flags cannot verify it",
        )),
        Err(CapabilityRuntimeError::Store(StoreError::NotFound(_))) => {
            if !development(app)
                && job.provider == Some(Provider::Apify)
                && job.capability.is_some()
            {
                Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "managed_capability_result",
                    "use the capability execution API",
                ))
            } else {
                Ok(())
            }
        }
        Err(error) => Err(error.into()),
    }
}
