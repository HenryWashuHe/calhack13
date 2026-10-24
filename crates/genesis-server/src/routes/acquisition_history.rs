//! Read-only operator access to redacted acquisition metadata and history.
//! Provider configuration is irrelevant once the durable journal is initialized.

use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_acquisition_history::{
    HistoryEntry, HistoryError, ListItem, ListQuery, Page, PageQuery,
};
use crate::error::{ApiError, ApiResult};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;

fn invalid_query() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_history_query",
        "invalid acquisition history query",
    )
}

fn history_error(error: HistoryError) -> ApiError {
    match error {
        HistoryError::InvalidQuery => invalid_query(),
        HistoryError::Unavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "acquisition_history_unavailable",
            "acquisition history is not initialized",
        ),
        HistoryError::NotFound => ApiError::not_found("acquisition history not found"),
        HistoryError::InvalidRecord | HistoryError::Db(_) | HistoryError::Encoding => {
            ApiError::internal("acquisition history could not be read")
        }
    }
}

pub async fn list(
    State(app): State<AppState>,
    principal: Principal,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> ApiResult<Json<Page<ListItem>>> {
    principal.require(&[Principal::Operator])?;
    let Query(query) = query.map_err(|_| invalid_query())?;
    Ok(Json(
        app.store.list_acquisitions(&query).map_err(history_error)?,
    ))
}

pub async fn history(
    State(app): State<AppState>,
    principal: Principal,
    Path(id): Path<String>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> ApiResult<Json<Page<HistoryEntry>>> {
    principal.require(&[Principal::Operator])?;
    let Query(query) = query.map_err(|_| invalid_query())?;
    Ok(Json(
        app.store
            .acquisition_history(&id, &query)
            .map_err(history_error)?,
    ))
}
