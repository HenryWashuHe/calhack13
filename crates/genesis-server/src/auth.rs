//! Bearer-token principals.
//!
//! The authenticated caller determines what it may do; a caller-supplied
//! `org_id` is never trusted. Three principals exist per organization:
//!
//! - `Operator`: kernel/operator management routes and the UI.
//! - `Agent`: restricted identity for agents (e.g. the Elastic mission agent);
//!   may submit observations, requirements and change proposals.
//! - `Adapter`: provider adapters reporting job results and contract updates.

use crate::app::AppState;
use crate::error::ApiError;
use axum::extract::{FromRef, FromRequestParts, Query};
use axum::http::request::Parts;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    Operator,
    Agent,
    Adapter,
}

impl Principal {
    pub fn label(self) -> &'static str {
        match self {
            Principal::Operator => "operator",
            Principal::Agent => "agent",
            Principal::Adapter => "adapter",
        }
    }

    pub fn require(self, allowed: &[Principal]) -> Result<(), ApiError> {
        if allowed.contains(&self) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "{} may not call this route",
                self.label()
            )))
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tokens {
    pub operator: String,
    pub agent: String,
    pub adapter: String,
}

impl Tokens {
    pub fn principal_for(&self, token: &str) -> Option<Principal> {
        // Constant-time comparison is unnecessary for the demo, but compare
        // full strings only.
        if token == self.operator {
            Some(Principal::Operator)
        } else if token == self.agent {
            Some(Principal::Agent)
        } else if token == self.adapter {
            Some(Principal::Adapter)
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

impl<S> FromRequestParts<S> for Principal
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        // EventSource cannot set headers, so `?token=` is accepted as well.
        let token = match header {
            Some(t) => Some(t),
            None => Query::<TokenQuery>::try_from_uri(&parts.uri)
                .ok()
                .and_then(|q| q.0.token),
        };
        token
            .and_then(|t| app.tokens.principal_for(&t))
            .ok_or_else(ApiError::unauthorized)
    }
}
