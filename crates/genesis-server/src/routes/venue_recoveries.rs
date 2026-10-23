use crate::app::AppState;
use crate::auth::Principal;
use crate::error::ApiResult;
use crate::venue_recovery::{self, CreateRecoveryRequest, RecoveryRecord};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

pub async fn create(
    State(app): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateRecoveryRequest>,
) -> ApiResult<(StatusCode, Json<RecoveryRecord>)> {
    principal.require(&[Principal::Operator])?;
    let (record, created) = venue_recovery::create(&app, request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(record),
    ))
}
pub async fn get(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<RecoveryRecord>> {
    principal.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(venue_recovery::read(&app, &id).await?))
}
pub async fn step(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<RecoveryRecord>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(venue_recovery::step(&app, &id).await?))
}
pub async fn retry(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<RecoveryRecord>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(venue_recovery::retry(&app, &id).await?))
}
