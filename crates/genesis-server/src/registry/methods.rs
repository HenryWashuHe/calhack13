//! Exact source exchange in the existing registry. Publication is not acceptance.
use super::{default_limit, RegistryState};
use crate::error::{ApiError, ApiResult};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use genesis_core::{
    development_method::ExactContentRef, development_package::MethodPackage, ids::OrgId,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Deliberate transport subset of the core package format's larger size limit.
pub const MAX_METHOD_PACKAGE_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodPublication {
    pub reference: ExactContentRef,
    pub package_json: String,
    pub intent: String,
    pub applicability: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodOffer {
    pub publisher: OrgId,
    pub publication: MethodPublication,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodLookup {
    pub publisher: OrgId,
    pub reference: ExactContentRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodCandidate {
    pub publisher: OrgId,
    pub reference: ExactContentRef,
    pub intent: String,
    pub applicability: Vec<String>,
    pub runtime: ExactContentRef,
    pub protocol: ExactContentRef,
    pub dependencies: Vec<ExactContentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodSearch {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub runtime: Option<ExactContentRef>,
    pub protocol: Option<ExactContentRef>,
    pub exclude_publisher: Option<OrgId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodSearchResults {
    pub candidates: Vec<MethodCandidate>,
    /// More matching publications exist; an empty bounded search is not universal absence.
    pub truncated: bool,
}

fn invalid() -> ApiError {
    ApiError::bad_request("invalid method publication or query")
}
pub(super) fn text_ok(s: &str, max: usize) -> bool {
    !s.is_empty() && s.trim() == s && s.len() <= max && !s.chars().any(char::is_control)
}
pub(super) fn reference_ok(r: &ExactContentRef) -> bool {
    r.validate().is_ok()
        && [&r.domain, &r.schema, &r.id, &r.version]
            .iter()
            .all(|s| s.len() <= 256)
}
fn encode<T: Serialize>(value: &T) -> ApiResult<String> {
    serde_json::to_string(value).map_err(|e| ApiError::internal(e.to_string()))
}
impl MethodPublication {
    /// Checks source identity only; the declared runtime/dependencies are not trusted or resolved.
    pub(crate) fn validate(&self) -> ApiResult<MethodPackage> {
        if self.package_json.len() > MAX_METHOD_PACKAGE_BYTES
            || !reference_ok(&self.reference)
            || !text_ok(&self.intent, 1024)
            || self.applicability.len() > 16
            || self.applicability.iter().any(|s| !text_ok(s, 256))
        {
            return Err(invalid());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            domain: String,
            version: u8,
            package: MethodPackage,
        }
        let e: Envelope = serde_json::from_str(&self.package_json).map_err(|_| invalid())?;
        if e.domain != "genesis.development_method_source"
            || e.version != 1
            || !reference_ok(e.package.runtime())
            || !reference_ok(e.package.protocol())
            || e.package.dependencies().len() > 16
            || e.package.dependencies().iter().any(|r| !reference_ok(r))
        {
            return Err(invalid());
        }
        MethodPackage::load_source_for(
            self.package_json.as_bytes(),
            &self.reference,
            e.package.dependencies(),
            e.package.runtime(),
            e.package.protocol(),
        )
        .map_err(|_| invalid())
    }
}
impl MethodOffer {
    pub(crate) fn candidate(&self, package: &MethodPackage) -> MethodCandidate {
        MethodCandidate {
            publisher: self.publisher.clone(),
            reference: self.publication.reference.clone(),
            intent: self.publication.intent.clone(),
            applicability: self.publication.applicability.clone(),
            runtime: package.runtime().clone(),
            protocol: package.protocol().clone(),
            dependencies: package.dependencies().to_vec(),
        }
    }
}

pub(super) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS method_versions (
        publisher TEXT NOT NULL, method_id TEXT NOT NULL, version TEXT NOT NULL,
        offer_json TEXT NOT NULL, candidate_json TEXT NOT NULL, search_text TEXT NOT NULL,
        runtime_json TEXT NOT NULL, protocol_json TEXT NOT NULL,
        PRIMARY KEY (publisher, method_id, version));",
    )
}
pub(super) fn routes() -> Router<RegistryState> {
    Router::new()
        .route("/registry/v1/methods", post(publish))
        .route("/registry/v1/methods/search", post(search))
        .route("/registry/v1/methods/fetch", post(fetch))
}
async fn publish(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
    Json(publication): Json<MethodPublication>,
) -> ApiResult<(StatusCode, Json<MethodOffer>)> {
    let token = RegistryState::token(&headers).ok_or_else(ApiError::unauthorized)?;
    let publisher = reg
        .publishers
        .get(&token)
        .ok_or_else(ApiError::unauthorized)?;
    let package = publication.validate()?;
    let offer = MethodOffer {
        publisher: publisher.clone(),
        publication,
    };
    if !text_ok(publisher.as_str(), 256) {
        return Err(invalid());
    }
    let candidate = offer.candidate(&package);
    candidate.validate()?;
    let key = &offer.publication.reference;
    let search_text = format!(
        "{} {}",
        offer.publication.intent,
        offer.publication.applicability.join(" ")
    )
    .to_lowercase();
    let conn = reg.lock();
    // The unique key also protects concurrent publishers using separate connections.
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO method_versions VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                publisher.as_str(),
                key.id,
                key.version,
                encode(&offer)?,
                encode(&candidate)?,
                search_text,
                encode(package.runtime())?,
                encode(package.protocol())?
            ],
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if inserted == 0 {
        let saved: String = conn.query_row(
            "SELECT offer_json FROM method_versions WHERE publisher=?1 AND method_id=?2 AND version=?3",
            params![publisher.as_str(), key.id, key.version], |r| r.get(0))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let saved: MethodOffer =
            serde_json::from_str(&saved).map_err(|e| ApiError::internal(e.to_string()))?;
        if saved == offer {
            return Ok((StatusCode::OK, Json(saved)));
        }
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "immutable_version",
            "method publication already exists; publish a new version",
        ));
    }
    Ok((StatusCode::CREATED, Json(offer)))
}
async fn fetch(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
    Json(lookup): Json<MethodLookup>,
) -> ApiResult<Json<MethodOffer>> {
    reg.require_reader(&headers)?;
    if !reference_ok(&lookup.reference) || !text_ok(lookup.publisher.as_str(), 256) {
        return Err(invalid());
    }
    let conn = reg.lock();
    let saved: Option<String> = conn.query_row(
        "SELECT offer_json FROM method_versions WHERE publisher=?1 AND method_id=?2 AND version=?3",
        params![lookup.publisher.as_str(), lookup.reference.id, lookup.reference.version], |r| r.get(0))
        .optional().map_err(|e| ApiError::internal(e.to_string()))?;
    let offer: MethodOffer =
        serde_json::from_str(&saved.ok_or_else(|| ApiError::not_found("method publication"))?)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    if offer.publication.reference != lookup.reference {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "identity_mismatch",
            "stored method differs from requested exact reference",
        ));
    }
    Ok(Json(offer))
}
async fn search(
    State(reg): State<RegistryState>,
    headers: HeaderMap,
    Json(req): Json<MethodSearch>,
) -> ApiResult<Json<MethodSearchResults>> {
    reg.require_reader(&headers)?;
    req.validate()?;
    let runtime = req.runtime.as_ref().map(encode).transpose()?;
    let protocol = req.protocol.as_ref().map(encode).transpose()?;
    let conn = reg.lock();
    let mut stmt = conn
        .prepare(
            "SELECT candidate_json FROM method_versions WHERE instr(search_text,?1)>0
        AND (?2 IS NULL OR runtime_json=?2) AND (?3 IS NULL OR protocol_json=?3)
        AND (?4 IS NULL OR publisher!=?4) ORDER BY publisher,method_id,version LIMIT ?5",
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let rows = stmt
        .query_map(
            params![
                req.query.to_lowercase(),
                runtime,
                protocol,
                req.exclude_publisher.as_ref().map(OrgId::as_str),
                req.limit + 1
            ],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut candidates = Vec::new();
    for row in rows {
        let json = row.map_err(|e| ApiError::internal(e.to_string()))?;
        candidates
            .push(serde_json::from_str(&json).map_err(|e| ApiError::internal(e.to_string()))?);
    }
    let truncated = candidates.len() > req.limit;
    candidates.truncate(req.limit);
    Ok(Json(MethodSearchResults {
        candidates,
        truncated,
    }))
}

impl MethodSearch {
    pub(crate) fn validate(&self) -> ApiResult<()> {
        if !text_ok(&self.query, 256)
            || !(1..=50).contains(&self.limit)
            || self.runtime.as_ref().is_some_and(|r| !reference_ok(r))
            || self.protocol.as_ref().is_some_and(|r| !reference_ok(r))
            || self
                .exclude_publisher
                .as_ref()
                .is_some_and(|p| !text_ok(p.as_str(), 256))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl MethodCandidate {
    pub(crate) fn validate(&self) -> ApiResult<()> {
        if !text_ok(self.publisher.as_str(), 256)
            || !reference_ok(&self.reference)
            || self.reference.domain != "genesis"
            || self.reference.schema != "development-method-source/1"
            || !text_ok(&self.intent, 1024)
            || self.applicability.len() > 16
            || self.applicability.iter().any(|s| !text_ok(s, 256))
            || !reference_ok(&self.runtime)
            || !reference_ok(&self.protocol)
            || self.dependencies.len() > 16
            || self.dependencies.iter().any(|r| !reference_ok(r))
            || encode(self)?.len() > 8192
        {
            return Err(invalid());
        }
        Ok(())
    }
}
