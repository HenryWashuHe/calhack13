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
/// operator's trusted policies can act on: each acquisition rule's capability
/// name and its allowed public source URLs (the policy narrows to the
/// requested sources and the suite must still cover its own pages, so the
/// request has to list them), plus adopt-first names with no acquisition
/// rule. Suite IDs, oracle answers, example HTML and model settings stay
/// server-side.
#[derive(Debug, serde::Serialize)]
pub struct Requestable {
    pub capabilities: Vec<RequestableCapability>,
    /// Scope the requesting role must hold for acquisition to proceed; absent
    /// when no acquisition policy is configured (adoption needs no scope
    /// beyond the role's current grant for the responsibility).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_scope: Option<&'static str>,
}

#[derive(Debug, serde::Serialize)]
pub struct RequestableCapability {
    pub capability_id: String,
    /// Empty for adoption-only names: adoption ignores them, so the request
    /// lists the observation's own public pages, as the kernel requires.
    pub observed_sources: Vec<String>,
    /// Only the adopt-first coordinator acts on this name: it adopts a
    /// published capability and never synthesizes one (unless that rule
    /// allows a fallback, which needs an acquisition rule anyway).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub adoption_only: bool,
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
                    adoption_only: false,
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(adopt) = app.gap_adoption_policy.as_deref() {
        for rule in &adopt.rules {
            let id = rule.capability_id.as_str();
            if !capabilities.iter().any(|c| c.capability_id == id) {
                capabilities.push(RequestableCapability {
                    capability_id: id.to_owned(),
                    observed_sources: Vec::new(),
                    adoption_only: true,
                });
            }
        }
    }
    capabilities.sort_by(|a, b| a.capability_id.cmp(&b.capability_id));
    Ok(Json(Requestable {
        capabilities,
        required_scope: app
            .gap_acquisition_policy
            .is_some()
            .then_some(crate::capability_acquisition_store::ACQUIRE_SCOPE),
    }))
}
