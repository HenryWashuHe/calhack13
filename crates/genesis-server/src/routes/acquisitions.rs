//! Operator-driven acquisition of capabilities for an existing responsibility.
//! Only backend-selected suites/model configuration and stored evidence are used.
use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_acquisition_runtime::AcquisitionRuntimeError;
use crate::capability_acquisition_store::{
    AcquisitionPolicy, AcquisitionRecord, AcquisitionStoreError, CreateAcquisition,
};
use crate::error::{ApiError, ApiResult};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) fn configured(app: &AppState) -> ApiResult<&AcquisitionPolicy> {
    app.acquisition_policy.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_configuration",
            "capability acquisition is not configured",
        )
    })
}

pub(crate) fn store_error(error: AcquisitionStoreError) -> ApiError {
    use AcquisitionStoreError::*;
    let (status, code) = match &error {
        NotFound => (StatusCode::NOT_FOUND, "not_found"),
        Authority(_) => (StatusCode::FORBIDDEN, "missing_authority"),
        QuotaExceeded(_) => (StatusCode::TOO_MANY_REQUESTS, "acquisition_quota"),
        Conflict | IdempotencyConflict | ActiveConflict => {
            (StatusCode::CONFLICT, "acquisition_conflict")
        }
        InvalidInput => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_acquisition"),
        InvalidPolicy | PolicyChanged => (StatusCode::SERVICE_UNAVAILABLE, "acquisition_policy"),
        InvalidRecord | Db(_) | Encoding => {
            return ApiError::internal("acquisition storage validation failed");
        }
    };
    ApiError::new(status, code, error.to_string())
}

pub(crate) fn runtime_error(error: AcquisitionRuntimeError) -> ApiError {
    use AcquisitionRuntimeError::*;
    let error = match error {
        Store(error) => return store_error(error),
        Persistence(_) => return ApiError::internal("acquisition persistence failed"),
        other => other,
    };
    let (status, code) = match &error {
        Unconfigured(_) | UnknownSuite => {
            (StatusCode::SERVICE_UNAVAILABLE, "provider_configuration")
        }
        Fenced => (StatusCode::FORBIDDEN, "missing_authority"),
        Acquire(genesis_forge::AcquisitionError::NotVerified) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "verification_required")
        }
        _ => (StatusCode::CONFLICT, "acquisition_state"),
    };
    // Provider bodies and checkpoint/source contents never enter this vocabulary.
    ApiError::new(status, code, error.to_string())
}

pub async fn create(
    State(app): State<AppState>,
    p: Principal,
    Json(request): Json<CreateAcquisition>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator])?;
    let policy = configured(&app)?;
    let suite = app
        .evidence_policy
        .suite(&request.suite_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unknown_acquisition_suite",
                "acquisition requires a configured backend oracle suite",
            )
        })?;
    let _kernel = app.kernel.lock().await;
    let (record, created) = app
        .store
        .create_acquisition(&request, policy, suite, Utc::now())
        .map_err(store_error)?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(public_status(&app, &record)?)))
}

pub async fn status(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    p.require(&[Principal::Operator])?;
    configured(&app)?;
    let record = app.acquisition_runtime.status(&id).map_err(runtime_error)?;
    Ok(Json(public_status(&app, &record)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRequest {
    #[serde(default)]
    retry_read: bool,
}

pub async fn step(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(request): Json<StepRequest>,
) -> ApiResult<Json<Value>> {
    p.require(&[Principal::Operator])?;
    configured(&app)?;
    let record = app
        .acquisition_runtime
        .step(&id, request.retry_read)
        .await
        .map_err(runtime_error)?;
    Ok(Json(public_status(&app, &record)?))
}

fn public_status(app: &AppState, record: &AcquisitionRecord) -> ApiResult<Value> {
    let session = record.session();
    let currently_authorized = record.authorize(&app.current_state()?, Utc::now()).is_ok();
    let phase = if record.is_fenced() {
        json!("fenced")
    } else {
        json!(session.phase())
    };
    Ok(json!({
        "id": record.id(),
        "version": record.version(),
        "org_id": record.binding().org_id,
        "mission_id": record.binding().mission_id,
        "requirement_id": record.binding().requirement_id,
        "responsibility_id": record.binding().responsibility_id,
        "role_id": record.binding().role_id,
        "phase": phase,
        "fenced": record.is_fenced(),
        "fence_reason": record.fence_reason(),
        "currently_authorized": currently_authorized,
        "attempt": session.attempt(),
        "generation_phase": session.generation().phase(),
        "generation_failure": session.generation().failure(),
        "forge_phase": session.forge().map(|f| f.phase()),
        "pending_action": session.forge().and_then(|f| f.pending_action()),
        "completed_cases": session.forge().map(|f| f.completed_cases()),
        "artifact": session.forge().and_then(|f| f.artifact()),
        "forge_failure": session.forge().and_then(|f| f.failure()),
        "unchanged_repair": session.unchanged_repair(),
        "organization_usage": app.store.acquisition_counters().map_err(store_error)?,
        "authority": "none: publication and independent local acceptance remain separate"
    }))
}
