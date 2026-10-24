//! Gateway routes called by provider-side skills.
//!
//! The readiness receipt route takes no bearer token: provider skills run in
//! account-wide sandboxes that must not hold organization tokens. The
//! one-time challenge nonce is the credential (see
//! [`crate::gateway_readiness`]).

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::gateway_readiness::{
    self, Challenge, ReadinessReceipt, ReceiptAccepted, ReceiptRejection,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;

pub async fn readiness_receipt(
    State(app): State<AppState>,
    Json(body): Json<ReadinessReceipt>,
) -> ApiResult<Json<ReceiptAccepted>> {
    let outcome = gateway_readiness::accept_receipt(&app.store, &body, Utc::now())
        .map_err(|_| ApiError::internal("readiness receipt could not be recorded"))?;
    match outcome {
        Ok(accepted) => Ok(Json(accepted)),
        Err(ReceiptRejection::Invalid) => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "invalid_receipt",
            "invalid readiness receipt",
        )),
        Err(ReceiptRejection::Expired) => Err(ApiError::new(
            StatusCode::GONE,
            "challenge_expired",
            "readiness challenge expired",
        )),
    }
}

pub async fn readiness_challenge(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Challenge>> {
    p.require(&[Principal::Operator])?;
    gateway_readiness::get_challenge(&app.store, &id)
        .map_err(|_| ApiError::internal("readiness challenge store unavailable"))?
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("readiness challenge {id}")))
}
