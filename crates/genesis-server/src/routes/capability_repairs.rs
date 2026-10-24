//! Operator entry point: stored failure references only, never caller verdicts.
use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_repair::{self, CapabilityRepair, CreateCapabilityRepair};
use crate::error::ApiResult;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

pub async fn create(
    State(app): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateCapabilityRepair>,
) -> ApiResult<(StatusCode, Json<CapabilityRepair>)> {
    principal.require(&[Principal::Operator])?;
    let (repair, created) = capability_repair::create(&app, &request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(repair),
    ))
}

pub async fn status(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<CapabilityRepair>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(capability_repair::read(&app, &id).await?))
}
