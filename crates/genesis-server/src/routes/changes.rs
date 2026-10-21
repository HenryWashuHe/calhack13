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
