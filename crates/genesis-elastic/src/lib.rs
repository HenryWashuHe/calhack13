//! # genesis-elastic
//!
//! Searchable projections of GENESIS state in Elasticsearch.
//!
//! - Each organization has its own **evidence index** (missions, observations,
//!   requirements, responsibilities, roles). Another organization never needs
//!   access to it.
//! - Approved capability manifests go to one shared **capability index** that
//!   any organization may search.
//!
//! Elastic is a projection, not the source of truth: the kernel's SQLite store
//! owns every organizational change. A search hit is a *candidate*; callers
//! re-read the authoritative record (and, for capabilities, the canonical
//! manifest and its verification) before acting on it.
//!
//! The client speaks the plain Elasticsearch REST API (`PUT /{index}`,
//! `POST /_bulk`, `POST /{index}/_search`) with `Authorization: ApiKey`.
//! Errors are sanitized: response bodies are never echoed into messages.

pub mod agent_builder;
#[cfg(feature = "fake")]
pub mod fake;
pub mod projection;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

pub use agent_builder::{AgentBuilder, AgentBuilderConfig, ConverseResult};
pub use projection::{capability_doc, org_docs};

/// Connection settings for the Elasticsearch data endpoint.
#[derive(Clone)]
pub struct ElasticConfig {
    /// Elasticsearch URL of the project (not the Kibana URL).
    pub elasticsearch_url: String,
    /// Encoded API key with read/write access to the GENESIS indices only.
    pub api_key: String,
    /// Prefix for every index name, e.g. `genesis`.
    pub index_prefix: String,
    pub timeout: Duration,
}

impl fmt::Debug for ElasticConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElasticConfig")
            .field("elasticsearch_url", &self.elasticsearch_url)
            .field("api_key", &"<redacted>")
            .field("index_prefix", &self.index_prefix)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl ElasticConfig {
    /// Read `ELASTICSEARCH_URL` and `ELASTIC_DATA_API_KEY` (optional
    /// `GENESIS_ELASTIC_INDEX_PREFIX`). Returns `None` when Elastic is not
    /// configured, so callers can run without it.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("ELASTICSEARCH_URL").ok()?;
        let key = std::env::var("ELASTIC_DATA_API_KEY").ok()?;
        if url.trim().is_empty() || key.trim().is_empty() {
            return None;
        }
        Some(Self {
            elasticsearch_url: url.trim().trim_end_matches('/').to_owned(),
            api_key: key.trim().to_owned(),
            index_prefix: std::env::var("GENESIS_ELASTIC_INDEX_PREFIX")
                .ok()
                .filter(|p| !p.trim().is_empty())
                .unwrap_or_else(|| "genesis".into()),
            timeout: Duration::from_secs(20),
        })
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ElasticError {
    #[error("elastic configuration: {0}")]
    Config(String),
    /// Non-success HTTP status. `kind` is Elasticsearch's error `type` when
    /// present; the body is not retained.
    #[error("elastic {operation} returned HTTP {status} ({kind})")]
    Http {
        operation: &'static str,
        status: u16,
        kind: String,
    },
    #[error("elastic {operation} transport failure: {kind}")]
    Transport {
        operation: &'static str,
        kind: String,
    },
    #[error("elastic {operation} response could not be parsed")]
    Decode { operation: &'static str },
    /// Some bulk items failed. Lists document IDs and error types only.
    #[error("elastic bulk indexing failed for {} document(s)", failed.len())]
    Partial { failed: Vec<(String, String)> },
}

pub type ElasticResult<T> = Result<T, ElasticError>;

/// Kind of a projected organization document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    Mission,
    Observation,
    Requirement,
    Responsibility,
    Role,
}

/// One searchable organization record. `doc_id` is deterministic, so
/// re-projecting the same state overwrites rather than duplicates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceDoc {
    pub doc_id: String,
    pub org_id: String,
    pub kind: DocKind,
    pub title: String,
    pub body: String,
    pub mission_id: Option<String>,
    /// Lifecycle status of the record, when it has one.
    pub status: Option<String>,
    /// Organization revision the projection was taken from.
    pub revision: u64,
    /// Related record IDs, e.g. `{"owner": "role_..."}`.
    #[serde(default)]
    pub refs: BTreeMap<String, String>,
}

/// Searchable summary of a published capability manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDoc {
    pub doc_id: String,
    pub capability_id: String,
    pub version: String,
    pub publisher: String,
    pub intent: String,
    pub applicability_conditions: Vec<String>,
    pub known_limitations: Vec<String>,
    pub required_permissions: Vec<String>,
    pub source_digest: String,
    pub state: String,
}

/// Filters for evidence search.
#[derive(Debug, Clone, Default)]
pub struct EvidenceFilter {
    pub kinds: Vec<DocKind>,
    pub mission_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit<T> {
    pub score: f64,
    pub doc: T,
}

/// Elasticsearch client for GENESIS projections.
#[derive(Clone)]
pub struct ElasticClient {
    cfg: ElasticConfig,
    http: reqwest::Client,
}

impl fmt::Debug for ElasticClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElasticClient")
            .field("cfg", &self.cfg)
            .finish()
    }
}

/// Lower-case index-safe form of an ID.
fn index_part(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    s.trim_start_matches(['-', '_']).to_owned()
}

/// Collision-resistant, index-safe namespace for one organization: a readable
/// lowercase prefix plus a digest of the *exact* organization ID, so `Org_A`
/// and `org_a` (or two long IDs sharing a prefix) never share an index or a
/// tool. Projection and Agent Builder both use this one mapping.
pub fn org_namespace(org_id: &str) -> String {
    let readable: String = index_part(org_id).chars().take(40).collect();
    let readable = readable.trim_end_matches(['-', '_']);
    let digest = genesis_core::digest::canonical_sha256(&Value::String(org_id.to_owned()));
    if readable.is_empty() {
        format!("org-{}", &digest[..16])
    } else {
        format!("{readable}-{}", &digest[..16])
    }
}

/// Index name for an organization's evidence under `prefix`.
pub fn evidence_index_name(prefix: &str, org_id: &str) -> String {
    format!("{}-evidence-{}", index_part(prefix), org_namespace(org_id))
}

impl ElasticClient {
    pub fn new(cfg: ElasticConfig) -> ElasticResult<Self> {
        if !(cfg.elasticsearch_url.starts_with("https://")
            || cfg.elasticsearch_url.starts_with("http://"))
        {
            return Err(ElasticError::Config(
                "ELASTICSEARCH_URL must be an http(s) URL".into(),
            ));
        }
        if index_part(&cfg.index_prefix).is_empty() {
            return Err(ElasticError::Config("index prefix is empty".into()));
        }
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .map_err(|_| ElasticError::Config("cannot build HTTP client".into()))?;
        Ok(Self { cfg, http })
    }

    /// The Elasticsearch URL this client talks to.
    pub fn elasticsearch_url(&self) -> &str {
        &self.cfg.elasticsearch_url
    }

    /// Index holding one organization's evidence.
    pub fn evidence_index(&self, org_id: &str) -> String {
        evidence_index_name(&self.cfg.index_prefix, org_id)
    }

    /// Shared index of approved capability manifests.
    pub fn capability_index(&self) -> String {
        format!("{}-capabilities", index_part(&self.cfg.index_prefix))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.cfg.elasticsearch_url, path)
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("Authorization", format!("ApiKey {}", self.cfg.api_key))
    }

    async fn send(
        &self,
        operation: &'static str,
        req: reqwest::RequestBuilder,
    ) -> ElasticResult<(u16, Value)> {
        let resp = self
            .auth(req)
            .send()
            .await
            .map_err(|e| ElasticError::Transport {
                operation,
                kind: if e.is_timeout() {
                    "timeout".into()
                } else if e.is_connect() {
                    "connect".into()
                } else {
                    "request".into()
                },
            })?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok((status, body))
    }

    fn http_error(operation: &'static str, status: u16, body: &Value) -> ElasticError {
        let kind = body
            .pointer("/error/type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(80)
            .collect();
        ElasticError::Http {
            operation,
            status,
            kind,
        }
    }

    /// Create an index with the given mappings if it does not exist.
    async fn ensure_index(&self, index: &str, mappings: Value) -> ElasticResult<()> {
        let (status, body) = self
            .send(
                "create_index",
                self.http
                    .put(self.url(index))
                    .json(&json!({ "mappings": mappings })),
            )
            .await?;
        let exists = body.pointer("/error/type").and_then(Value::as_str)
            == Some("resource_already_exists_exception");
        if (200..300).contains(&status) || exists {
            Ok(())
        } else {
            Err(Self::http_error("create_index", status, &body))
        }
    }

    /// Ensure the organization's evidence index and the shared capability index.
    /// Ensure only the shared capability index (used by the registry).
    pub async fn ensure_capability_index(&self) -> ElasticResult<()> {
        self.ensure_index(&self.capability_index(), capability_mappings())
            .await
    }

    pub async fn ensure_indices(&self, org_id: &str) -> ElasticResult<()> {
        self.ensure_index(&self.evidence_index(org_id), evidence_mappings())
            .await?;
        self.ensure_index(&self.capability_index(), capability_mappings())
            .await
    }

    /// Index documents with deterministic IDs. Returns the number written.
    async fn bulk<T: Serialize>(&self, index: &str, docs: &[(String, T)]) -> ElasticResult<usize> {
        if docs.is_empty() {
            return Ok(0);
        }
        let mut ndjson = String::new();
        for (id, doc) in docs {
            ndjson.push_str(&json!({ "index": { "_index": index, "_id": id } }).to_string());
            ndjson.push('\n');
            ndjson.push_str(
                &serde_json::to_string(doc)
                    .map_err(|_| ElasticError::Decode { operation: "bulk" })?,
            );
            ndjson.push('\n');
        }
        let (status, body) = self
            .send(
                "bulk",
                self.http
                    .post(self.url("_bulk"))
                    .header("Content-Type", "application/x-ndjson")
                    .body(ndjson),
            )
            .await?;
        if !(200..300).contains(&status) {
            return Err(Self::http_error("bulk", status, &body));
        }
        let items = body
            .get("items")
            .and_then(Value::as_array)
            .ok_or(ElasticError::Decode { operation: "bulk" })?;
        let mut failed = Vec::new();
        for item in items {
            let r = item.get("index").unwrap_or(&Value::Null);
            let code = r.get("status").and_then(Value::as_u64).unwrap_or(0);
            if !(200..300).contains(&code) {
                let id = r
                    .get("_id")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned();
                let kind = r
                    .pointer("/error/type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                failed.push((id, kind));
            }
        }
        if items.len() != docs.len() {
            return Err(ElasticError::Decode { operation: "bulk" });
        }
        if failed.is_empty() {
            Ok(docs.len())
        } else {
            Err(ElasticError::Partial { failed })
        }
    }

    /// Write (or overwrite) an organization's evidence documents.
    pub async fn index_evidence(&self, org_id: &str, docs: &[EvidenceDoc]) -> ElasticResult<usize> {
        if let Some(bad) = docs.iter().find(|d| d.org_id != org_id) {
            return Err(ElasticError::Config(format!(
                "document {} belongs to another organization",
                bad.doc_id
            )));
        }
        let pairs: Vec<_> = docs.iter().map(|d| (d.doc_id.clone(), d)).collect();
        self.bulk(&self.evidence_index(org_id), &pairs).await
    }

    /// Write (or overwrite) capability documents in the shared index.
    pub async fn index_capabilities(&self, docs: &[CapabilityDoc]) -> ElasticResult<usize> {
        let pairs: Vec<_> = docs.iter().map(|d| (d.doc_id.clone(), d)).collect();
        self.bulk(&self.capability_index(), &pairs).await
    }

    async fn search<T: for<'de> Deserialize<'de>>(
        &self,
        index: &str,
        query: Value,
        size: usize,
    ) -> ElasticResult<Vec<Hit<T>>> {
        let (status, body) = self
            .send(
                "search",
                self.http
                    .post(self.url(&format!("{index}/_search")))
                    .json(&json!({ "query": query, "size": size.clamp(1, 100) })),
            )
            .await?;
        if status == 404 {
            return Ok(vec![]); // nothing projected yet
        }
        if !(200..300).contains(&status) {
            return Err(Self::http_error("search", status, &body));
        }
        let hits =
            body.pointer("/hits/hits")
                .and_then(Value::as_array)
                .ok_or(ElasticError::Decode {
                    operation: "search",
                })?;
        hits.iter()
            .map(|h| {
                let doc = serde_json::from_value(h.get("_source").cloned().unwrap_or_default())
                    .map_err(|_| ElasticError::Decode {
                        operation: "search",
                    })?;
                Ok(Hit {
                    score: h.get("_score").and_then(Value::as_f64).unwrap_or(0.0),
                    doc,
                })
            })
            .collect()
    }

    /// Full-text search over one organization's own evidence.
    pub async fn search_evidence(
        &self,
        org_id: &str,
        text: &str,
        filter: &EvidenceFilter,
        size: usize,
    ) -> ElasticResult<Vec<Hit<EvidenceDoc>>> {
        let mut filters = vec![json!({ "term": { "org_id": org_id } })];
        if !filter.kinds.is_empty() {
            filters.push(json!({ "terms": { "kind": filter.kinds } }));
        }
        if let Some(m) = &filter.mission_id {
            filters.push(json!({ "term": { "mission_id": m } }));
        }
        let query = json!({ "bool": {
            "must": [ { "multi_match": { "query": text, "fields": ["title^2", "body"] } } ],
            "filter": filters,
        }});
        let hits: Vec<Hit<EvidenceDoc>> = self
            .search(&self.evidence_index(org_id), query, size)
            .await?;
        // Defense in depth: never return another organization's record.
        Ok(hits
            .into_iter()
            .filter(|h| h.doc.org_id == org_id)
            .collect())
    }

    /// Search published capabilities. When `available_scopes` is given, only
    /// capabilities whose required permissions are all available are returned.
    /// Hits are candidates: install only after re-reading the canonical manifest.
    pub async fn search_capabilities(
        &self,
        text: &str,
        available_scopes: Option<&[String]>,
        size: usize,
    ) -> ElasticResult<Vec<Hit<CapabilityDoc>>> {
        let query = json!({ "bool": {
            "must": [ { "multi_match": {
                "query": text,
                "fields": ["intent^3", "applicability_conditions^2", "known_limitations", "capability_id"],
            } } ],
            "filter": [ { "term": { "state": "PUBLISHED" } } ],
        }});
        let hits: Vec<Hit<CapabilityDoc>> = self
            .search(&self.capability_index(), query, size.saturating_mul(3))
            .await?;
        let allowed: Option<BTreeSet<&str>> =
            available_scopes.map(|s| s.iter().map(String::as_str).collect());
        Ok(hits
            .into_iter()
            .filter(|h| h.doc.state == "PUBLISHED")
            .filter(|h| match &allowed {
                Some(a) => h
                    .doc
                    .required_permissions
                    .iter()
                    .all(|p| a.contains(p.as_str())),
                None => true,
            })
            .take(size)
            .collect())
    }
}

fn evidence_mappings() -> Value {
    json!({
        "dynamic": "strict",
        "properties": {
            "doc_id": { "type": "keyword" },
            "org_id": { "type": "keyword" },
            "kind": { "type": "keyword" },
            "title": { "type": "text" },
            "body": { "type": "text" },
            "mission_id": { "type": "keyword" },
            "status": { "type": "keyword" },
            "revision": { "type": "long" },
            "refs": { "type": "flattened" }
        }
    })
}

fn capability_mappings() -> Value {
    json!({
        "dynamic": "strict",
        "properties": {
            "doc_id": { "type": "keyword" },
            "capability_id": { "type": "keyword" },
            "version": { "type": "keyword" },
            "publisher": { "type": "keyword" },
            "intent": { "type": "text" },
            "applicability_conditions": { "type": "text" },
            "known_limitations": { "type": "text" },
            "required_permissions": { "type": "keyword" },
            "source_digest": { "type": "keyword" },
            "state": { "type": "keyword" }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ElasticConfig {
        ElasticConfig {
            elasticsearch_url: "https://example.es".into(),
            api_key: "c2VjcmV0".into(),
            index_prefix: "Genesis".into(),
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn index_names_are_lowercase_and_org_scoped() {
        let c = ElasticClient::new(cfg()).unwrap();
        let a = c.evidence_index("Org_A.1");
        assert!(a.starts_with("genesis-evidence-org_a-1-"), "{a}");
        assert_eq!(a, a.to_lowercase());
        assert_ne!(c.evidence_index("Org_A"), c.evidence_index("org_a"));
        assert_ne!(
            c.evidence_index(&"a".repeat(81)),
            c.evidence_index(&"a".repeat(80))
        );
        assert_eq!(c.capability_index(), "genesis-capabilities");
    }

    #[test]
    fn debug_output_redacts_the_key() {
        let s = format!("{:?}", ElasticClient::new(cfg()).unwrap());
        assert!(!s.contains("c2VjcmV0"));
        assert!(s.contains("redacted"));
    }

    #[test]
    fn rejects_non_http_urls() {
        let mut c = cfg();
        c.elasticsearch_url = "ftp://x".into();
        assert!(matches!(
            ElasticClient::new(c),
            Err(ElasticError::Config(_))
        ));
    }
}
