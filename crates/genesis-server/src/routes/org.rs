use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{validation_error, ApiResult};
use crate::store::EventMeta;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::ids::{ContractId, MissionId, ObservationId};
use genesis_core::lifecycle::ContractState;
use genesis_core::model::OrgState;
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn health(State(app): State<AppState>) -> ApiResult<Json<Value>> {
    let state = app.current_state()?;
    Ok(Json(json!({
        "org_id": app.org_id,
        "name": app.name,
        "revision": state.revision,
        "dev_stub_enabled": app.providers.dev_stub_enabled(),
        "configured_providers": app.providers.configured(),
        "registry_configured": app.registry.is_some(),
        "acquisition_enabled": app.acquisition_policy.is_some(),
        // Configuration, not a worker heartbeat or semantic readiness claim.
        "automatic_acquisition_enabled": app.automatic_acquisition,
    })))
}

pub async fn organization(State(app): State<AppState>, p: Principal) -> ApiResult<Json<OrgState>> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    Ok(Json(app.current_state()?))
}

pub async fn revision(
    State(app): State<AppState>,
    p: Principal,
    Path(revision): Path<u64>,
) -> ApiResult<Json<OrgState>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(app.store.state_at(revision)?))
}

pub async fn create_mission(
    State(app): State<AppState>,
    p: Principal,
    Json(spec): Json<MissionBootstrap>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator])?;
    let m = app
        .mutate(EventMeta::default(), |s| {
            engine::bootstrap_mission(s, &spec, Utc::now()).map_err(validation_error)
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "mission_id": m.value, "revision": m.state.revision })),
    ))
}

#[derive(Deserialize)]
pub struct ObservationBody {
    pub source: String,
    pub summary: String,
    pub evidence_ref: Option<String>,
}

pub async fn add_observation(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<ObservationBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    let mission = MissionId::new(id);
    let m = app
        .mutate(EventMeta::mission(mission.clone()), |s| {
            engine::record_observation(
                s,
                &mission,
                &body.source,
                &body.summary,
                body.evidence_ref.clone(),
                Utc::now(),
            )
            .map_err(Into::into)
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "observation_id": m.value, "revision": m.state.revision })),
    ))
}

#[derive(Deserialize)]
pub struct RequirementBody {
    pub summary: String,
    pub evidence: Vec<ObservationId>,
    pub acceptance: String,
}

pub async fn add_requirement(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<RequirementBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    let mission = MissionId::new(id);
    let m = app
        .mutate(EventMeta::mission(mission.clone()), |s| {
            engine::record_requirement(s, &mission, &body.summary, &body.evidence, &body.acceptance)
                .map_err(Into::into)
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "requirement_id": m.value, "revision": m.state.revision })),
    ))
}

#[derive(Deserialize)]
pub struct ContractStateBody {
    pub state: ContractState,
    /// Transport sender identity as seen by the Fetch bridge.
    pub sender_address: String,
}

/// Fetch bridge reports offer / acceptance / release. The sender must match the
/// contracted specialist address.
pub async fn update_contract(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<ContractStateBody>,
) -> ApiResult<Json<Value>> {
    p.require(&[Principal::Adapter, Principal::Operator])?;
    let contract_id = ContractId::new(id);
    let m = app
        .mutate(EventMeta::default(), |s| {
            let c = s.contracts.get(&contract_id).ok_or_else(|| {
                crate::error::ApiError::not_found(format!("contract {contract_id}"))
            })?;
            if c.specialist_address != body.sender_address {
                return Err(crate::error::ApiError::forbidden(format!(
                    "sender {} is not the contracted specialist",
                    body.sender_address
                )));
            }
            let out = engine::update_contract(s, &contract_id, body.state)?;
            Ok((out, ()))
        })
        .await?;
    Ok(Json(
        json!({ "revision": m.state.revision, "contract": m.state.contracts.get(&contract_id) }),
    ))
}
