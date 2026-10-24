//! Retrieval over this organization's Elastic projection.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::projection::ProjectionStatus;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use genesis_elastic::{DocKind, EvidenceDoc, EvidenceFilter, Hit};
use serde::{Deserialize, Serialize};

pub async fn status(
    State(app): State<AppState>,
    p: Principal,
) -> ApiResult<Json<ProjectionStatus>> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    Ok(Json(app.projection.status()))
}

#[derive(Deserialize)]
pub struct EvidenceQuery {
    pub q: String,
    /// Comma-separated kinds: mission, observation, requirement, responsibility, role.
    pub kinds: Option<String>,
    pub mission_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct EvidenceResults {
    /// Revision the index reflects; it may lag the kernel. Re-read the
    /// authoritative record before acting on a hit.
    pub projected_revision: Option<u64>,
    pub hits: Vec<Hit<EvidenceDoc>>,
}

fn parse_kind(s: &str) -> Result<DocKind, ApiError> {
    serde_json::from_value(serde_json::Value::String(s.trim().to_owned()))
        .map_err(|_| ApiError::bad_request(format!("unknown kind `{}`", s.trim())))
}

pub async fn evidence(
    State(app): State<AppState>,
    p: Principal,
    Query(q): Query<EvidenceQuery>,
) -> ApiResult<Json<EvidenceResults>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    let Some(client) = &app.projection.client else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "elastic_not_configured",
            "set ELASTICSEARCH_URL and ELASTIC_DATA_API_KEY to enable retrieval",
        ));
    };
    if q.q.trim().is_empty() {
        return Err(ApiError::bad_request("q is required"));
    }
    let kinds = q
        .kinds
        .as_deref()
        .map(|k| {
            k.split(',')
                .filter(|s| !s.trim().is_empty())
                .map(parse_kind)
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let filter = EvidenceFilter {
        kinds,
        mission_id: q.mission_id,
    };
    let hits = client
        .search_evidence(
            &app.org_id.to_string(),
            &q.q,
            &filter,
            q.limit.unwrap_or(10).min(50),
        )
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, "elastic_error", e.to_string()))?;
    Ok(Json(EvidenceResults {
        projected_revision: app.projection.status().projected_revision,
        hits,
    }))
}
