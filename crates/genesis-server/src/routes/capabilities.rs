//! Capability discovery, publication, installation and execution brokerage.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{validation_error, ApiError, ApiResult};
use crate::registry::{SearchCandidate, SearchRequest};
use crate::store::{put_job, EventMeta};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use genesis_core::capability::CapabilityManifest;
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::*;
use genesis_core::jobs::{DispatchRequest, Job};
use genesis_core::lifecycle::{InstallationStatus, JobStatus, StateMachine};
use genesis_core::model::Provider;
use genesis_providers::ProviderError;
use serde::Deserialize;
use serde_json::{json, Value};

fn registry(app: &AppState) -> ApiResult<&crate::registry::RegistryClient> {
    app.registry.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_configuration",
            "no capability registry is configured for this organization",
        )
    })
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

/// Publish a verified capability. Only the operator can publish; an agent can
/// never self-approve publication.
pub async fn publish(
    State(app): State<AppState>,
    p: Principal,
    Json(manifest): Json<CapabilityManifest>,
) -> ApiResult<(StatusCode, Json<CapabilityManifest>)> {
    p.require(&[Principal::Operator])?;
    if manifest.publisher != app.org_id {
        return Err(ApiError::forbidden(format!(
            "this organization is {}, manifest names publisher {}",
            app.org_id, manifest.publisher
        )));
    }
    let published = registry(&app)?.publish(&manifest).await?;
    Ok((StatusCode::CREATED, Json(published)))
}

#[derive(Deserialize)]
pub struct InstallBody {
    pub capability_id: CapabilityId,
    pub version: String,
    #[serde(default)]
    pub configuration: Value,
}

/// Fetch the canonical manifest, check compatibility with this organization's
/// constitution, and record a local installation pending acceptance. No source
/// generation happens on this path.
pub async fn install(
    State(app): State<AppState>,
    p: Principal,
    Json(body): Json<InstallBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator])?;
    let manifest = registry(&app)?
        .manifest(&body.capability_id, &body.version)
        .await?;
    let m = app
        .mutate(EventMeta::default(), |s| {
            engine::begin_installation(s, &manifest, body.configuration.clone(), Utc::now())
                .map_err(|errs| {
                    let mut e = validation_error(errs);
                    e.error = "incompatible_capability".into();
                    e
                })
        })
        .await?;
    let inst = m.state.installations.get(&m.value).cloned();
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "installation_id": m.value,
            "installation": inst,
            "manifest_source_digest": manifest.source_digest,
            "artifact": manifest.artifact,
            "synthesis_calls": 0,
            "revision": m.state.revision,
        })),
    ))
}

#[derive(Deserialize)]
pub struct AcceptanceBody {
    pub passed: bool,
    pub report_ref: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

pub async fn acceptance(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<AcceptanceBody>,
) -> ApiResult<Json<Value>> {
    p.require(&[Principal::Operator, Principal::Adapter])?;
    let id = InstallationId::new(id);
    let m = app
        .mutate(EventMeta::default(), |s| {
            Ok((
                engine::finish_installation(
                    s,
                    &id,
                    body.passed,
                    &body.report_ref,
                    body.reasons.clone(),
                )?,
                (),
            ))
        })
        .await?;
    Ok(Json(json!({
        "installation": m.state.installations.get(&id),
        "revision": m.state.revision,
    })))
}

#[derive(Deserialize)]
pub struct RunBody {
    pub client_request_id: String,
    pub installation_id: InstallationId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    #[serde(default)]
    pub input: Value,
    pub required_scope: Option<String>,
}

/// Execute an installed, accepted, attached capability under a checked grant.
pub async fn run(
    State(app): State<AppState>,
    p: Principal,
    Json(body): Json<RunBody>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    let state = app.current_state()?;
    let inst = state
        .installations
        .get(&body.installation_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("installation {}", body.installation_id)))?;
    if inst.status != InstallationStatus::Accepted {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "incompatible_capability",
            format!(
                "installation is {:?}; local acceptance has not passed",
                inst.status
            ),
        ));
    }
    let manifest = registry(&app)?
        .manifest(&inst.capability.capability_id, &inst.capability.version)
        .await?;
    if manifest.source_digest != inst.source_digest {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "artifact_mismatch",
            "registry manifest digest differs from the installed digest",
        ));
    }

    // 1. Persist the queued job before calling the provider.
    let req = DispatchRequest {
        client_request_id: body.client_request_id.clone(),
        role_id: body.role_id.clone(),
        responsibility_id: body.responsibility_id.clone(),
        task: format!(
            "run {}@{}",
            inst.capability.capability_id, inst.capability.version
        ),
        input: body.input.clone(),
        required_scope: body.required_scope.clone(),
        capability: Some(inst.capability.clone()),
        provider: Some(Provider::Apify),
        deadline: None,
    };
    let mut job = {
        let _guard = app.kernel.lock().await;
        let state = app.current_state()?;
        let (job, mut effects, _outbox) = super::jobs::new_job(&app, &state, &req)?;
        let (stored, fresh) = app.store.insert_job(&body.client_request_id, &job)?;
        if !fresh {
            return Ok((StatusCode::OK, Json(stored)));
        }
        effects.push(EventKind::CapabilityRunRequested {
            installation_id: inst.installation_id.clone(),
            capability: inst.capability.clone(),
            role_id: job.role_id.clone(),
            job_id: job.job_id.clone(),
        });
        let meta =
            EventMeta::mission(job.mission_id.clone()).caused_by(body.client_request_id.clone());
        let events =
            app.store
                .commit_state(state.revision, &state, effects, &meta, vec![], |_| Ok(()))?;
        app.broadcast(&events);
        job
    };

    // 2. Submit the pinned artifact.
    let executor = app.providers.executor()?;
    let submitted = executor
        .submit(
            &inst.capability,
            &manifest.artifact.actor_id,
            &manifest.artifact.build_number,
            &body.input,
        )
        .await;
    let now = Utc::now();
    let mut effects = Vec::new();
    let mut push = |job: &mut Job, to: JobStatus| -> ApiResult<()> {
        if job.status.can_transition_to(to) {
            let from = job
                .set_status(to, now)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            effects.push(EventKind::JobStatusChanged {
                job_id: job.job_id.clone(),
                from,
                to,
            });
        }
        Ok(())
    };
    let outcome: ApiResult<()> = match submitted {
        Ok(receipt) => {
            push(&mut job, JobStatus::Submitting)?;
            push(&mut job, JobStatus::Running)?;
            job.raw_status = Some(receipt.raw_status);
            job.provider_refs.extend(receipt.provider_refs);
            job.provider = Some(receipt.provider);
            Ok(())
        }
        Err(e @ (ProviderError::Unknown(_) | ProviderError::Timeout)) => {
            push(&mut job, JobStatus::Submitting)?;
            push(&mut job, JobStatus::Unknown)?;
            job.raw_status = Some(e.to_string());
            Ok(())
        }
        Err(e) => {
            push(&mut job, JobStatus::Failed)?;
            job.rejection_reason = Some(e.to_string());
            Err(e.into())
        }
    };
    let meta = EventMeta::mission(job.mission_id.clone()).caused_by(job.job_id.to_string());
    let events = app
        .store
        .append_events(effects, &meta, |tx| put_job(tx, &job))?;
    app.broadcast(&events);
    outcome?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}
