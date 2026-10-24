//! An operator explicitly authorizes bounded continuing CREAO assessment.
use crate::app::AppState;
use crate::auth::Principal;
use crate::error::ApiResult;
use crate::venue_monitors::{self, CreateVenueMonitorRequest, VenueMonitorRecord};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

pub async fn create(
    State(app): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateVenueMonitorRequest>,
) -> ApiResult<(StatusCode, Json<VenueMonitorRecord>)> {
    principal.require(&[Principal::Operator])?;
    let (record, created) = venue_monitors::create(&app, request).await?;
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
) -> ApiResult<Json<VenueMonitorRecord>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(venue_monitors::read(&app, &id).await?))
}

pub async fn stop(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<VenueMonitorRecord>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(venue_monitors::stop(&app, &id).await?))
}
