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

/// Capability names the operator's trusted policy can act on, for the planner
/// (agent token) to use in `RequestCapability`. Names only: suites, sources,
/// examples and model settings stay server-side.
#[derive(Debug, serde::Serialize)]
pub struct Requestable {
    pub capability_ids: Vec<String>,
}

pub async fn requestable(
    State(app): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<Requestable>> {
    principal.require(&[Principal::Operator, Principal::Agent])?;
    let mut capability_ids: Vec<String> = app
        .gap_acquisition_policy
        .as_deref()
        .map(|p| {
            p.rules
                .iter()
                .map(|r| r.capability_id.to_string())
                .collect()
        })
        .unwrap_or_default();
    capability_ids.sort();
    capability_ids.dedup();
    Ok(Json(Requestable { capability_ids }))
}
