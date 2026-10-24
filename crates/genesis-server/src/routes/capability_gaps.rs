//! Redacted operator visibility into automatic gap intake.
use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_gap_driver::{self, GapDriverStatus};
use crate::error::{ApiError, ApiResult};
use crate::store::StoreError;
use axum::extract::{Path, State};
use axum::Json;
use genesis_core::ids::CapabilityGapId;

pub async fn acquisition(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<GapDriverStatus>> {
    principal.require(&[Principal::Operator])?;
    capability_gap_driver::status(&app, &CapabilityGapId::new(id))
        .map(Json)
        .map_err(|error| match error {
            StoreError::NotFound(_) => ApiError::not_found("capability gap"),
            _ => ApiError::internal("capability gap progress could not be read"),
        })
}
