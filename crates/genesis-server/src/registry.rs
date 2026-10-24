//! Shared capability service: immutable manifest publication and discovery
//! (TECH_SPEC §9).
//!
//! Runs as its own process (`genesis registry`) so that Org A and Org B share
//! nothing but published manifests. Search here is a simple keyword ranking;
//! the Elastic workstream replaces it with native retrieval over the same
//! manifests. Installation always re-fetches the canonical manifest.

use crate::error::{ApiError, ApiResult};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use genesis_core::capability::CapabilityManifest;
use genesis_core::ids::{CapabilityId, OrgId};
use genesis_core::lifecycle::CapabilityState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct RegistryState {
    conn: Arc<Mutex<Connection>>,
    /// Publisher token -> organization allowed to publish under that token.
    publishers: Arc<HashMap<String, OrgId>>,
    /// Tokens allowed to search and read (publisher tokens are always readers).
    readers: Arc<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Only return capabilities whose required permissions are a subset of these.
    #[serde(default)]
    pub available_scopes: Option<Vec<String>>,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct SearchCandidate {
    pub capability_id: CapabilityId,
    pub version: String,
    pub publisher: OrgId,
    pub intent: String,
    pub applicability_conditions: Vec<String>,
    pub required_permissions: Vec<String>,
    pub source_digest: String,
    pub score: f32,
}

impl RegistryState {
    pub fn open(
        path: impl AsRef<FsPath>,
        publishers: HashMap<String, OrgId>,
        readers: Vec<String>,
    ) -> anyhow::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        Self::from_conn(Connection::open(path)?, publishers, readers)
    }

    pub fn open_in_memory(
        publishers: HashMap<String, OrgId>,
        readers: Vec<String>,
    ) -> anyhow::Result<Self> {
        Self::from_conn(Connection::open_in_memory()?, publishers, readers)
    }

    fn from_conn(
        conn: Connection,
        publishers: HashMap<String, OrgId>,
        mut readers: Vec<String>,
    ) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS capability_versions (
                capability_id  TEXT NOT NULL,
                version        TEXT NOT NULL,
                publisher      TEXT NOT NULL,
                source_digest  TEXT NOT NULL,
                manifest_json  TEXT NOT NULL,
                published_at   TEXT NOT NULL,
                PRIMARY KEY (capability_id, version)
             );",
        )?;
        readers.extend(publishers.keys().cloned());
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            publishers: Arc::new(publishers),
            readers: Arc::new(readers),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn token(headers: &HeaderMap) -> Option<String> {
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned)
    }

    fn require_reader(&self, headers: &HeaderMap) -> ApiResult<()> {
        match Self::token(headers) {
            Some(t) if self.readers.contains(&t) => Ok(()),
            _ => Err(ApiError::unauthorized()),
        }
    }

    fn all_manifests(&self) -> ApiResult<Vec<CapabilityManifest>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT manifest_json FROM capability_versions ORDER BY published_at DESC")
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            let json = r.map_err(|e| ApiError::internal(e.to_string()))?;
            out.push(serde_json::from_str(&json).map_err(|e| ApiError::internal(e.to_string()))?);
        }
        Ok(out)
    }
}

pub fn router(state: RegistryState) -> Router {
    Router::new()
        .route(
            "/registry/v1/health",
            get(|| async { Json(serde_json::json!({"ok": true})) }),
        )
        .route("/registry/v1/manifests", post(publish))
        .route("/registry/v1/capabilities", get(list))
        .route(
            "/registry/v1/capabilities/{id}/versions/{version}",
            get(fetch),
        )
        .route("/registry/v1/search", post(search))
        .with_state(state)
}

async fn publish(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
    Json(mut manifest): Json<CapabilityManifest>,
) -> ApiResult<(StatusCode, Json<CapabilityManifest>)> {
    let token = RegistryState::token(&headers).ok_or_else(ApiError::unauthorized)?;
    let publisher = reg
        .publishers
        .get(&token)
        .ok_or_else(ApiError::unauthorized)?;
    if &manifest.publisher != publisher {
        return Err(ApiError::forbidden(format!(
            "token belongs to {publisher}, manifest names {}",
            manifest.publisher
        )));
    }
    if let Err(problems) = manifest.check_publishable() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "verification_failed",
            problems.join("; "),
        )
        .with_details(problems));
    }
    if manifest.state == CapabilityState::Approved {
        manifest.state = CapabilityState::Published;
    }
    let now = Utc::now();
    manifest.published_at.get_or_insert(now);

    let conn = reg.lock();
    let existing: Option<String> = conn
        .query_row(
            "SELECT manifest_json FROM capability_versions WHERE capability_id = ?1 AND version = ?2",
            params![manifest.capability_id.as_str(), manifest.version],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if let Some(encoded) = existing {
        let stored: CapabilityManifest =
            serde_json::from_str(&encoded).map_err(|e| ApiError::internal(e.to_string()))?;
        // Retries may have fresh timestamps, but an immutable version must keep
        // its entire artifact, publisher, oracle and permission contract.
        let mut comparable = manifest.clone();
        comparable.published_at = stored.published_at;
        comparable.verification.verified_at = stored.verification.verified_at;
        if comparable == stored {
            return Ok((StatusCode::OK, Json(stored)));
        }
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "immutable_version",
            "capability version already has a different manifest; publish a new version",
        ));
    }
    conn.execute(
        "INSERT INTO capability_versions (capability_id, version, publisher, source_digest, manifest_json, published_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            manifest.capability_id.as_str(),
            manifest.version,
            manifest.publisher.as_str(),
            manifest.source_digest,
            serde_json::to_string(&manifest).map_err(|e| ApiError::internal(e.to_string()))?,
            now.to_rfc3339()
        ],
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(manifest)))
}

async fn fetch(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Json<CapabilityManifest>> {
    reg.require_reader(&headers)?;
    let conn = reg.lock();
    let json: Option<String> = conn
        .query_row(
            "SELECT manifest_json FROM capability_versions WHERE capability_id = ?1 AND version = ?2",
            params![id, version],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let json = json.ok_or_else(|| ApiError::not_found(format!("{id}@{version}")))?;
    Ok(Json(
        serde_json::from_str(&json).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}

async fn list(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<SearchCandidate>>> {
    reg.require_reader(&headers)?;
    Ok(Json(
        reg.all_manifests()?
            .iter()
            .map(|m| candidate(m, 0.0))
            .collect(),
    ))
}

fn candidate(m: &CapabilityManifest, score: f32) -> SearchCandidate {
    SearchCandidate {
        capability_id: m.capability_id.clone(),
        version: m.version.clone(),
        publisher: m.publisher.clone(),
        intent: m.intent.clone(),
        applicability_conditions: m.applicability_conditions.clone(),
        required_permissions: m.required_permissions.iter().cloned().collect(),
        source_digest: m.source_digest.clone(),
        score,
    }
}

fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(str::to_owned)
        .collect()
}

/// Keyword ranking over intent, applicability and ID. Placeholder for Elastic.
pub fn rank(manifests: &[CapabilityManifest], req: &SearchRequest) -> Vec<SearchCandidate> {
    let q = tokens(&req.query);
    let mut out: Vec<SearchCandidate> = manifests
        .iter()
        .filter(|m| m.state.is_usable())
        .filter(|m| match &req.available_scopes {
            None => true,
            Some(scopes) => m.required_permissions.iter().all(|p| scopes.contains(p)),
        })
        .filter_map(|m| {
            let hay = tokens(&format!(
                "{} {} {}",
                m.intent,
                m.applicability_conditions.join(" "),
                m.capability_id
            ));
            let hits = q.iter().filter(|t| hay.contains(t)).count();
            if hits == 0 {
                None
            } else {
                Some(candidate(m, hits as f32 / q.len().max(1) as f32))
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(req.limit.max(1));
    out
}

async fn search(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
    Json(req): Json<SearchRequest>,
) -> ApiResult<Json<Vec<SearchCandidate>>> {
    reg.require_reader(&headers)?;
    let all = reg.all_manifests()?;
    Ok(Json(rank(&all, &req)))
}

/// HTTP client an organization uses to reach the shared registry.
/// Requests target a configured HTTP(S) origin. Redirects and transport retries
/// are disabled: publication outcomes must be reconciled by the caller.
#[derive(Clone)]
pub struct RegistryClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

const MAX_REGISTRY_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_QUERY_BYTES: usize = 4096;
const MAX_SEARCH_SCOPES: usize = 128;
const MAX_IDENTIFIER_BYTES: usize = 256;

impl RegistryClient {
    /// Preserve the infallible constructor; malformed configuration is rejected
    /// by each operation before making a request, without returning its contents.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .build()
                .expect("reqwest client"),
        }
    }

    fn configuration_error() -> ApiError {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "registry_configuration",
            "registry origin or credentials are invalid",
        )
    }

    fn invalid_request() -> ApiError {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "registry_invalid_request",
            "registry request exceeds supported bounds or has an invalid identifier",
        )
    }

    fn endpoint(&self, segments: &[&str]) -> ApiResult<reqwest::Url> {
        let mut url =
            reqwest::Url::parse(&self.base_url).map_err(|_| Self::configuration_error())?;
        // Do not silently normalize whitespace or turn a configured path into
        // an origin. An empty userinfo component is also disallowed.
        let authority = self
            .base_url
            .split_once("://")
            .map(|(_, rest)| rest.strip_suffix('/').unwrap_or(rest));
        if self.base_url.trim() != self.base_url
            || self.base_url.chars().any(char::is_control)
            || !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || authority.is_none_or(|authority| authority.contains(['/', '\\', '@']))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || self.token.is_empty()
            || reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.token)).is_err()
        {
            return Err(Self::configuration_error());
        }
        url.path_segments_mut()
            .map_err(|_| Self::configuration_error())?
            .clear()
            .extend(segments);
        Ok(url)
    }

    fn response_too_large() -> ApiError {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "registry_response_too_large",
            "registry response exceeds the supported size limit",
        )
    }

    async fn decode<T: serde::de::DeserializeOwned>(mut resp: reqwest::Response) -> ApiResult<T> {
        let status = resp.status();
        if status.is_redirection() {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "registry_redirect",
                "registry redirects are not supported",
            ));
        }
        if !status.is_success() {
            // Never read or reflect remote error bodies, codes, headers or URLs.
            // Keep the HTTP status useful to callers without trusting its prose.
            let status = if status.is_client_error() || status.is_server_error() {
                status
            } else {
                StatusCode::BAD_GATEWAY
            };
            return Err(ApiError::new(
                status,
                "registry_error",
                "registry request was rejected",
            ));
        }
        if resp
            .content_length()
            .is_some_and(|size| size > MAX_REGISTRY_RESPONSE_BYTES as u64)
        {
            return Err(Self::response_too_large());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(Self::unreachable)? {
            if chunk.len() > MAX_REGISTRY_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(Self::response_too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        genesis_verification::decode_json(&bytes).map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "registry_invalid_response",
                "registry response is invalid or ambiguous",
            )
        })
    }

    fn unreachable(_: reqwest::Error) -> ApiError {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "registry_unreachable",
            "registry transport failed",
        )
    }

    pub async fn search(&self, req: &SearchRequest) -> ApiResult<Vec<SearchCandidate>> {
        if !(1..=100).contains(&req.limit)
            || req.query.len() > MAX_SEARCH_QUERY_BYTES
            || req.available_scopes.as_ref().is_some_and(|scopes| {
                scopes.len() > MAX_SEARCH_SCOPES
                    || scopes
                        .iter()
                        .any(|scope| scope.is_empty() || scope.len() > MAX_IDENTIFIER_BYTES)
            })
        {
            return Err(Self::invalid_request());
        }
        let url = self.endpoint(&["registry", "v1", "search"])?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(req)
            .send()
            .await
            .map_err(Self::unreachable)?;
        Self::decode(resp).await
    }

    pub async fn manifest(
        &self,
        id: &CapabilityId,
        version: &str,
    ) -> ApiResult<CapabilityManifest> {
        // URL segment builders intentionally remove literal dot segments. Refuse
        // those two names rather than silently selecting a different endpoint.
        if [id.as_str(), version].into_iter().any(|part| {
            part.is_empty() || part.len() > MAX_IDENTIFIER_BYTES || matches!(part, "." | "..")
        }) {
            return Err(Self::invalid_request());
        }
        let url = self.endpoint(&[
            "registry",
            "v1",
            "capabilities",
            id.as_str(),
            "versions",
            version,
        ])?;
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(Self::unreachable)?;
        Self::decode(resp).await
    }

    pub async fn publish(&self, manifest: &CapabilityManifest) -> ApiResult<CapabilityManifest> {
        let url = self.endpoint(&["registry", "v1", "manifests"])?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(manifest)
            .send()
            .await
            .map_err(Self::unreachable)?;
        Self::decode(resp).await
    }
}
