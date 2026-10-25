//! Operator routes for method adoption decisions and the invocation check.
//! The selected source is always re-fetched from the configured registry by
//! publisher and exact reference; acceptance evidence is always read from the
//! receiver's own records root. Neither comes from the request body.
use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::method_adoption_store::{CreateMethodAdoption, MethodAdoptionError, MethodDecision};
use crate::method_invocation::{authorize_method_invocation, MethodInvocationRequest};
use crate::registry::methods::MethodLookup;
use axum::extract::{rejection::JsonRejection, Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde_json::{json, Value};

fn error(error: MethodAdoptionError) -> ApiError {
    use MethodAdoptionError as E;
    let conflict = |code: &str, message: String| ApiError::new(StatusCode::CONFLICT, code, message);
    match error {
        E::InvalidInput => ApiError::bad_request("invalid method adoption request"),
        E::NotFound => ApiError::not_found("method adoption"),
        E::IdempotencyConflict => conflict(
            "idempotency_conflict",
            "method adoption key is bound to another intent".into(),
        ),
        E::AlreadyAdopted => conflict("already_adopted", error.to_string()),
        E::Limit => conflict("method_adoption_limit", error.to_string()),
        E::Authority(fence) => conflict(
            "method_authority",
            format!("method authority changed or expired: {fence:?}"),
        ),
        E::NotAdopted => conflict("method_not_adopted", error.to_string()),
        E::ScopeMissing => ApiError::new(StatusCode::FORBIDDEN, "method_scope", error.to_string()),
        E::Acceptance(e) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "method_acceptance",
            e.to_string(),
        ),
        E::InvalidRecord | E::Db(_) => ApiError::internal("method adoption state is unavailable"),
    }
}

fn configured(app: &AppState) -> ApiResult<std::sync::Arc<crate::method_adoption::MethodReceiver>> {
    app.method_receiver.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "method_adoption_configuration",
            "method adoption requires a configured receiver",
        )
    })
}

pub async fn create(
    State(app): State<AppState>,
    principal: Principal,
    body: Result<Json<CreateMethodAdoption>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    principal.require(&[Principal::Operator])?;
    let Json(body) = body.map_err(|_| ApiError::bad_request("invalid method adoption request"))?;
    let receiver = configured(&app)?;
    // An identical retry returns its stored decision without new external work.
    if let Some(record) = app.store.method_adoption_replay(&body).map_err(error)? {
        return Ok((StatusCode::OK, Json(json!(record))));
    }
    // Only an adoption needs the selected bytes; a decline records a choice.
    let package_json = match &body.decision {
        MethodDecision::Decline { .. } => String::new(),
        MethodDecision::Adopt { .. } => {
            let registry = app.registry.as_ref().ok_or_else(|| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "method_adoption_configuration",
                    "method adoption requires a registry",
                )
            })?;
            let lookup = MethodLookup {
                publisher: body.publisher.clone(),
                reference: body.reference.clone(),
            };
            registry
                .method_offer(&lookup)
                .await?
                .publication
                .package_json
        }
    };
    // The rescore runs a subprocess, so it happens before any lock; the store
    // then checks assessment read the same bytes.
    let rescored = match (&body.decision, app.method_rescorer.clone()) {
        (MethodDecision::Adopt { trial }, Some(rescorer)) => {
            let (root, trial) = (receiver.records_root.clone(), trial.clone());
            let joined =
                tokio::task::spawn_blocking(move || rescorer.rescore_trial(&root, &trial)).await;
            let rescored = joined
                .map_err(|_| ApiError::internal("method rescoring did not complete"))?
                .map_err(|e| error(MethodAdoptionError::Acceptance(e)))?;
            Some(rescored)
        }
        _ => None,
    };
    let _guard = app.kernel.lock().await;
    let (record, created) = app
        .store
        .create_method_adoption_rescored(
            &body,
            &package_json,
            &receiver,
            rescored.as_ref(),
            Utc::now(),
        )
        .map_err(error)?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(json!(record))))
}

pub async fn status(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(json!(app
        .store
        .method_adoption(&id)
        .map_err(error)?)))
}

pub async fn authorize(
    State(app): State<AppState>,
    principal: Principal,
    body: Result<Json<MethodInvocationRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    let Json(body) =
        body.map_err(|_| ApiError::bad_request("invalid method invocation request"))?;
    // A disabled receiver issues no new invocation grants; reads stay available.
    let receiver = configured(&app)?;
    let _guard = app.kernel.lock().await;
    let state = app
        .store
        .current_state()
        .map_err(|_| ApiError::internal("organization state is unavailable"))?;
    let grant = authorize_method_invocation(&app.store, &state, &receiver, &body, Utc::now())
        .map_err(error)?;
    Ok(Json(json!(grant)))
}

fn run_error(error: crate::method_invocation_run::MethodRunError) -> ApiError {
    use crate::method_invocation_run::MethodRunError as E;
    let conflict = |code: &str, message: String| ApiError::new(StatusCode::CONFLICT, code, message);
    match error {
        E::InvalidInput => ApiError::bad_request("invalid method run request"),
        E::Plan(_) | E::Receipt(_) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "method_run_binding",
            error.to_string(),
        ),
        E::NotFound => ApiError::not_found("method run"),
        E::IdempotencyConflict => conflict(
            "idempotency_conflict",
            "method run key is bound to another intent".into(),
        ),
        E::Limit => conflict("method_run_limit", error.to_string()),
        E::Status(_) => conflict("method_run_status", error.to_string()),
        E::Adoption(e) => self::error(e),
        E::InvalidRecord | E::Db(_) => ApiError::internal("method run state is unavailable"),
    }
}

/// Freeze one invocation plan under the current grant. Nothing runs here.
pub async fn prepare_run(
    State(app): State<AppState>,
    principal: Principal,
    body: Result<Json<crate::method_invocation_run::CreateMethodRun>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    principal.require(&[Principal::Operator])?;
    let Json(body) = body.map_err(|_| ApiError::bad_request("invalid method run request"))?;
    let receiver = configured(&app)?;
    let _guard = app.kernel.lock().await;
    let state = app
        .store
        .current_state()
        .map_err(|_| ApiError::internal("organization state is unavailable"))?;
    let (record, created) = app
        .store
        .prepare_method_run(&body, &state, &receiver, Utc::now())
        .map_err(run_error)?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(json!(record))))
}

/// Start a prepared run once, if its grant is still the current one.
pub async fn start_run(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    let receiver = configured(&app)?;
    let _guard = app.kernel.lock().await;
    let state = app
        .store
        .current_state()
        .map_err(|_| ApiError::internal("organization state is unavailable"))?;
    let record = app
        .store
        .start_method_run(&id, &state, &receiver, Utc::now())
        .map_err(run_error)?;
    Ok(Json(json!(record)))
}

/// Record a started run's one execution receipt.
pub async fn complete_run(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
    body: Result<Json<genesis_core::development_method::ExecutionReceipt>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    let Json(receipt) = body.map_err(|_| ApiError::bad_request("invalid execution receipt"))?;
    // No `configured` check: a disabled receiver grants nothing new, but a
    // started run's receipt is still retained (as completed after fence).
    let receiver = app.method_receiver.clone();
    let _guard = app.kernel.lock().await;
    let state = app
        .store
        .current_state()
        .map_err(|_| ApiError::internal("organization state is unavailable"))?;
    let record = app
        .store
        .retain_method_run_receipt(&id, &receipt, &state, receiver.as_deref(), Utc::now())
        .map_err(run_error)?;
    Ok(Json(json!(record)))
}

/// A result body: the evaluation alone, or with the scored output's bytes
/// (lowercase hex) for an organization that rescores results.
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum RunResultBody {
    WithOutput {
        evaluation: genesis_core::development_evaluation::EvaluationReceipt,
        output_hex: String,
    },
    Evaluation(genesis_core::development_evaluation::EvaluationReceipt),
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            text.get(i..i + 2)
                .filter(|p| p.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
                .and_then(|p| u8::from_str_radix(p, 16).ok())
        })
        .collect()
}

/// Record a completed run's one evaluation and whether it attaches to the
/// responsibility. Like receipts, no `configured` check: with the receiver
/// disabled the evaluation is kept and attaches to nothing. With a rescorer
/// configured, the output must come with it and the evaluation must
/// reproduce under the pinned scorer before anything is recorded.
pub async fn record_run_result(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
    body: Result<Json<RunResultBody>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    let Json(body) = body.map_err(|_| ApiError::bad_request("invalid evaluation receipt"))?;
    let (evaluation, output_hex) = match body {
        RunResultBody::WithOutput {
            evaluation,
            output_hex,
        } => (evaluation, Some(output_hex)),
        RunResultBody::Evaluation(evaluation) => (evaluation, None),
    };
    let rescorer = match app.method_rescorer.clone() {
        None => None,
        Some(rescorer) => {
            let refuse = |message: &str| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "method_rescore",
                    message.to_string(),
                )
            };
            // Only a completed run with a receipt and no result can take a
            // result; refusing anything else here means no result is ever
            // recorded without the rescore.
            let record = app.store.method_run(&id).map_err(run_error)?;
            match (record.receipt.clone(), record.result.is_none()) {
                (Some(receipt), true) => {
                    let output = output_hex.as_deref().and_then(unhex).ok_or_else(|| {
                        refuse("this organization rescores results: send output_hex")
                    })?;
                    let (plan, check) = (record.plan.clone(), evaluation.clone());
                    let slot = plan.slots[0].slot_id.clone();
                    let pinned = rescorer.sha256.clone();
                    tokio::task::spawn_blocking(move || {
                        rescorer.rescore_run(&plan, &slot, &receipt, &output, &check)
                    })
                    .await
                    .map_err(|_| ApiError::internal("method rescoring did not complete"))?
                    .map_err(|e| refuse(&e.to_string()))?;
                    Some(pinned)
                }
                _ => {
                    return Err(run_error(
                        crate::method_invocation_run::MethodRunError::Status(record.status),
                    ))
                }
            }
        }
    };
    let receiver = app.method_receiver.clone();
    let _guard = app.kernel.lock().await;
    let state = app
        .store
        .current_state()
        .map_err(|_| ApiError::internal("organization state is unavailable"))?;
    let record = app
        .store
        .record_method_run_result_rescored(
            &id,
            &evaluation,
            &state,
            receiver.as_deref(),
            rescorer,
            Utc::now(),
        )
        .map_err(run_error)?;
    Ok(Json(json!(record)))
}

pub async fn run_status(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(json!(app.store.method_run(&id).map_err(run_error)?)))
}
