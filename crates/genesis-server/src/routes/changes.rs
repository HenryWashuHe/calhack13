use crate::app::AppState;
use crate::auth::Principal;
use crate::error::ApiResult;
use crate::saga;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use genesis_core::change::{OrgChangeProposal, OrgChangeRecord};
use genesis_core::ids::ChangeId;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct SubmitBody {
    pub client_request_id: String,
    pub proposal: OrgChangeProposal,
}

/// Submit a schema-validated mutation proposal. Agents use their restricted
/// identity here; they cannot commit directly.
pub async fn submit(
    State(app): State<AppState>,
    p: Principal,
    Json(body): Json<SubmitBody>,
) -> ApiResult<(StatusCode, Json<OrgChangeRecord>)> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    let (status, record) =
        saga::submit(&app, p.label(), &body.client_request_id, body.proposal).await?;
    Ok((status, Json(record)))
}

#[derive(Deserialize)]
pub struct AbandonBody {
    pub reason: String,
}

/// Operator: stop waiting on a role's asynchronous provider operation; the
/// change ends FAILED with all provider IDs retained.
pub async fn abandon_operation(
    State(app): State<AppState>,
    p: Principal,
    Path((id, role_key)): Path<(String, String)>,
    Json(body): Json<AbandonBody>,
) -> ApiResult<Json<OrgChangeRecord>> {
    p.require(&[Principal::Operator])?;
    if body.reason.trim().is_empty() {
        return Err(crate::error::ApiError::bad_request("reason is required"));
    }
    Ok(Json(
        saga::abandon_operation(&app, &ChangeId::new(id), &role_key, &body.reason).await?,
    ))
}

pub async fn list(
    State(app): State<AppState>,
    p: Principal,
) -> ApiResult<Json<Vec<OrgChangeRecord>>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(app.store.list_changes()?))
}

pub async fn get_one(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<OrgChangeRecord>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(app.store.get_change(&ChangeId::new(id))?))
}

/// Look up a change by the `client_request_id` it was submitted with, so a
/// client whose submission response was lost can reconcile without
/// resubmitting. 404 when no change used that request ID.
pub async fn by_request(
    State(app): State<AppState>,
    p: Principal,
    Path(request_id): Path<String>,
) -> ApiResult<Json<OrgChangeRecord>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    app.store
        .find_change_by_request(&request_id)?
        .map(Json)
        .ok_or_else(|| {
            crate::error::ApiError::not_found(format!("change for request {request_id}"))
        })
}
