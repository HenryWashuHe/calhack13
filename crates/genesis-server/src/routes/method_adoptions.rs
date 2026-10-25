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
    let _guard = app.kernel.lock().await;
    let (record, created) = app
        .store
        .create_method_adoption(&body, &package_json, &receiver, Utc::now())
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
