//! Only operators can choose event constraints or submit a monitored update.
use crate::app::AppState;
use crate::auth::Principal;
use crate::error::ApiResult;
use crate::venue_assessments::{self, AssessmentRecord, CreateAssessmentRequest};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

pub async fn create(
    State(app): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateAssessmentRequest>,
) -> ApiResult<(StatusCode, Json<AssessmentRecord>)> {
    principal.require(&[Principal::Operator])?;
    let (record, created) = venue_assessments::create(&app, request).await?;
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
) -> ApiResult<Json<AssessmentRecord>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(venue_assessments::read(&app, &id).await?))
}

pub async fn verify(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<AssessmentRecord>> {
    principal.require(&[Principal::Operator])?;
    Ok(Json(venue_assessments::finalize(&app, &id).await?))
}
