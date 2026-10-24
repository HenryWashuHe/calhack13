//! API error type with explicit, machine-readable codes (TECH_SPEC §10).

use crate::store::StoreError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use genesis_core::engine::{DispatchError, ValidationCode, ValidationError};
use genesis_providers::ProviderError;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    /// Stable error code: `revision_conflict`, `missing_authority`,
    /// `incompatible_capability`, `verification_failed`,
    /// `provider_configuration`, `unknown_external_outcome`, ...
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub details: serde_json::Value,
}

impl ApiError {
    pub fn new(status: StatusCode, error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            error: error.into(),
            message: message.into(),
            details: serde_json::Value::Null,
        }
    }

    pub fn with_details(mut self, details: impl Serialize) -> Self {
        self.details = serde_json::to_value(details).unwrap_or_default();
        self
    }

    pub fn not_found(what: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", what)
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", msg)
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid bearer token",
        )
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "missing_authority", msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::RevisionConflict { .. } => {
                ApiError::new(StatusCode::CONFLICT, "revision_conflict", e.to_string())
            }
            StoreError::NotFound(what) => ApiError::not_found(what),
            other => {
                tracing::error!(error = %other, "store error");
                ApiError::internal(other.to_string())
            }
        }
    }
}

/// Validation failures. A revision conflict is a 409; anything else is 422.
pub fn validation_error(errors: Vec<ValidationError>) -> ApiError {
    let conflict = errors
        .iter()
        .any(|e| e.code == ValidationCode::RevisionConflict);
    let authority = errors
        .iter()
        .any(|e| e.code == ValidationCode::AuthorityExceedsConstitution);
    let (status, code) = if conflict {
        (StatusCode::CONFLICT, "revision_conflict")
    } else if authority {
        (StatusCode::UNPROCESSABLE_ENTITY, "missing_authority")
    } else if errors.iter().any(|e| {
        matches!(
            e.code,
            ValidationCode::CapabilityNotApproved | ValidationCode::CapabilityUnknown
        )
    }) {
        (StatusCode::UNPROCESSABLE_ENTITY, "incompatible_capability")
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed")
    };
    let message = errors.first().map(|e| e.to_string()).unwrap_or_default();
    ApiError::new(status, code, message).with_details(errors)
}

impl From<ValidationError> for ApiError {
    fn from(e: ValidationError) -> Self {
        validation_error(vec![e])
    }
}

impl From<DispatchError> for ApiError {
    fn from(e: DispatchError) -> Self {
        let (status, code) = match &e {
            DispatchError::UnknownRole { .. } | DispatchError::UnknownResponsibility { .. } => {
                (StatusCode::NOT_FOUND, "not_found")
            }
            DispatchError::StaleEpoch { .. } => (StatusCode::CONFLICT, "stale_epoch"),
            DispatchError::ScopeNotGranted { .. }
            | DispatchError::NotAssigned { .. }
            | DispatchError::RoleNotActive { .. }
            | DispatchError::CapabilityNotAttached { .. } => {
                (StatusCode::FORBIDDEN, "missing_authority")
            }
            DispatchError::ResponsibilityNotOpen { .. } => {
                (StatusCode::CONFLICT, "responsibility_closed")
            }
        };
        ApiError::new(status, code, e.to_string()).with_details(&e)
    }
}

impl From<ProviderError> for ApiError {
    fn from(e: ProviderError) -> Self {
        let (status, code) = match &e {
            ProviderError::NotConfigured { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "provider_configuration")
            }
            ProviderError::Unknown(_) | ProviderError::Timeout => {
                (StatusCode::BAD_GATEWAY, "unknown_external_outcome")
            }
            ProviderError::Http { .. }
            | ProviderError::Rejected(_)
            | ProviderError::RunFailed(_) => (StatusCode::BAD_GATEWAY, "provider_error"),
        };
        ApiError::new(status, code, e.to_string()).with_details(&e)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
