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

/// What the planner (agent token) needs to make a `RequestCapability` the
/// operator's trusted gap policy can act on: each rule's capability name and
/// its allowed public source URLs. The policy narrows to the requested
/// sources and the suite must still cover its own pages, so the request has
/// to list them. Suite IDs, oracle answers, example HTML and model settings
/// stay server-side.
#[derive(Debug, serde::Serialize)]
pub struct Requestable {
    pub capabilities: Vec<RequestableCapability>,
}

#[derive(Debug, serde::Serialize)]
pub struct RequestableCapability {
    pub capability_id: String,
    pub observed_sources: Vec<String>,
}

pub async fn requestable(
    State(app): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<Requestable>> {
    principal.require(&[Principal::Operator, Principal::Agent])?;
    let mut capabilities: Vec<RequestableCapability> = app
        .gap_acquisition_policy
        .as_deref()
        .map(|p| {
            p.rules
                .iter()
                .map(|r| RequestableCapability {
                    capability_id: r.capability_id.clone(),
                    observed_sources: r.allowed_source_urls.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    capabilities.sort_by(|a, b| a.capability_id.cmp(&b.capability_id));
    Ok(Json(Requestable { capabilities }))
}
