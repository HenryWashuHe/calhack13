use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_adoption::{self, error};
use crate::capability_adoption_store::CreateAdoption;
use crate::error::{ApiError, ApiResult};
use axum::extract::{rejection::JsonRejection, rejection::QueryRejection, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

pub async fn create(
    State(app): State<AppState>,
    principal: Principal,
    body: Result<Json<CreateAdoption>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    principal.require(&[Principal::Operator])?;
    let Json(body) = body.map_err(|_| ApiError::bad_request("invalid adoption request"))?;
    capability_adoption::configured(&app)?;
    let suite = app
        .evidence_policy
        .suite(&body.suite_id)
        .ok_or_else(|| ApiError::bad_request("unknown backend adoption suite"))?;
    let _guard = app.kernel.lock().await;
    let (record, created) = app
        .store
        .create_adoption(&body, suite, Utc::now())
        .map_err(error)?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!(record.status())),
    ))
}

pub async fn status(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(json!(app
        .store
        .get_adoption(&id)
        .map_err(error)?
        .status())))
}

pub async fn step(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
    body: Result<Json<Empty>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    let _ = body.map_err(|_| ApiError::bad_request("adoption step accepts an empty object"))?;
    Ok(Json(json!(capability_adoption::step(&app, &id)
        .await?
        .status())))
}

pub async fn list(
    State(app): State<AppState>,
    principal: Principal,
    query: Result<Query<Empty>, QueryRejection>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Principal::Operator])?;
    let _ = query.map_err(|_| ApiError::bad_request("adoption listing accepts no query fields"))?;
    let records = app
        .store
        .list_adoptions(0, crate::capability_adoption_store::MAX_ADOPTIONS)
        .map_err(error)?;
    Ok(Json(
        json!({"items": records.into_iter().map(|record| record.status()).collect::<Vec<_>>()}),
    ))
}
