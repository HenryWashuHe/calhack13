//! Elastic Agent Builder specialists (Kibana API).
//!
//! A kernel-approved role with `provider: elastic` becomes an Agent Builder
//! agent whose only tool is an `index_search` tool scoped to the
//! organization's own evidence index. Creation is reconcilable: agent and tool
//! IDs are derived from the organization, change and role key, so a retry
//! after a lost response finds the existing agent instead of creating a
//! second one. An existing agent with a different configuration is an error,
//! never silently adopted.
//!
//! The agent proposes; it holds no organizational authority. Ownership,
//! dispatch and retirement stay in the kernel.
//!
//! Endpoints (relative to the Kibana URL, optionally under `/s/{space}`):
//! `POST/GET /api/agent_builder/tools[/{id}]`,
//! `POST/GET /api/agent_builder/agents[/{id}]`,
//! `POST /api/agent_builder/converse`. Writes send `kbn-xsrf: true`.

use async_trait::async_trait;
use genesis_core::digest::canonical_sha256;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::{ProviderError, ProvisionRequest, RoleProvisioner};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::time::Duration;

/// Kibana connection for Agent Builder management.
#[derive(Clone)]
pub struct AgentBuilderConfig {
    pub kibana_url: String,
    /// Encoded API key with Agent Builder management privileges.
    pub api_key: String,
    /// Kibana space; `None` for the default space.
    pub space: Option<String>,
    /// Same prefix as the Elasticsearch projection, so the tool's index
    /// pattern matches the organization's evidence index.
    pub index_prefix: String,
    pub timeout: Duration,
}

impl fmt::Debug for AgentBuilderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBuilderConfig")
            .field("kibana_url", &self.kibana_url)
            .field("api_key", &"<redacted>")
            .field("space", &self.space)
            .field("index_prefix", &self.index_prefix)
            .finish()
    }
}

impl AgentBuilderConfig {
    /// `ELASTIC_KIBANA_URL` and `ELASTIC_MANAGEMENT_API_KEY` (optional
    /// `ELASTIC_KIBANA_SPACE`, `GENESIS_ELASTIC_INDEX_PREFIX`).
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("ELASTIC_KIBANA_URL").ok()?;
        let key = std::env::var("ELASTIC_MANAGEMENT_API_KEY").ok()?;
        if url.trim().is_empty() || key.trim().is_empty() {
            return None;
        }
        Some(Self {
            kibana_url: url.trim().trim_end_matches('/').to_owned(),
            api_key: key.trim().to_owned(),
            space: std::env::var("ELASTIC_KIBANA_SPACE")
                .ok()
                .filter(|s| !s.trim().is_empty() && s.trim() != "default"),
            index_prefix: std::env::var("GENESIS_ELASTIC_INDEX_PREFIX")
                .ok()
                .filter(|p| !p.trim().is_empty())
                .unwrap_or_else(|| "genesis".into()),
            timeout: Duration::from_secs(30),
        })
    }
}

/// Outcome of one converse round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConverseResult {
    pub conversation_id: String,
    pub round_id: Option<String>,
    pub status: String,
    pub message: String,
    /// IDs of tools the agent called in this round.
    pub tool_calls: Vec<String>,
}

#[derive(Clone)]
pub struct AgentBuilder {
    cfg: AgentBuilderConfig,
    http: reqwest::Client,
}

impl fmt::Debug for AgentBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBuilder")
            .field("cfg", &self.cfg)
            .finish()
    }
}

/// Lowercase ID made of `[a-z0-9_-]`, at most `max` characters.
fn slug(raw: &str, max: usize) -> String {
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
    let s = s.trim_matches(['-', '_']);
    s.chars().take(max).collect()
}

fn sanitized_kind(body: &Value) -> String {
    body.get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_')
        .take(60)
        .collect()
}

fn http_err(status: u16, body: &Value) -> ProviderError {
    // Never forward provider bodies: they can echo request content.
    ProviderError::Http {
        status,
        body: sanitized_kind(body),
    }
}

impl AgentBuilder {
    pub fn new(cfg: AgentBuilderConfig) -> Result<Self, ProviderError> {
        if !(cfg.kibana_url.starts_with("https://") || cfg.kibana_url.starts_with("http://")) {
            return Err(ProviderError::Rejected(
                "ELASTIC_KIBANA_URL must be an http(s) URL".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .map_err(|_| ProviderError::Rejected("cannot build HTTP client".into()))?;
        Ok(Self { cfg, http })
    }

    fn url(&self, path: &str) -> String {
        match &self.cfg.space {
            Some(space) => format!(
                "{}/s/{}/api/agent_builder/{path}",
                self.cfg.kibana_url,
                slug(space, 64)
            ),
            None => format!("{}/api/agent_builder/{path}", self.cfg.kibana_url),
        }
    }

    async fn send(
        &self,
        req: reqwest::RequestBuilder,
        write: bool,
    ) -> Result<(u16, Value), ProviderError> {
        let mut req = req.header("Authorization", format!("ApiKey {}", self.cfg.api_key));
        if write {
            req = req.header("kbn-xsrf", "true");
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                // The write may have happened; callers reconcile by ID.
                ProviderError::Unknown("agent builder request timed out".into())
            } else if e.is_connect() {
                ProviderError::Unknown("agent builder connection failed".into())
            } else {
                ProviderError::Unknown("agent builder request failed".into())
            }
        })?;
        let status = resp.status().as_u16();
        Ok((status, resp.json().await.unwrap_or(Value::Null)))
    }

    async fn get(&self, path: &str) -> Result<Option<Value>, ProviderError> {
        let (status, body) = self.send(self.http.get(self.url(path)), false).await?;
        match status {
            200..=299 => Ok(Some(body)),
            404 => Ok(None),
            _ => Err(http_err(status, &body)),
        }
    }

    /// Delete an agent or tool (`collection` is `agents` or `tools`). A
    /// missing resource counts as deleted. Used for explicit cleanup only.
    pub async fn delete(&self, collection: &str, id: &str) -> Result<(), ProviderError> {
        if collection != "agents" && collection != "tools" {
            return Err(ProviderError::Rejected(
                "can only delete agents or tools".into(),
            ));
        }
        let (status, body) = self
            .send(
                self.http.delete(self.url(&format!("{collection}/{id}"))),
                true,
            )
            .await?;
        match status {
            200..=299 | 404 => Ok(()),
            _ => Err(http_err(status, &body)),
        }
    }

    /// Create `body` at `collection`, or confirm an identical existing one.
    /// `same` decides whether an existing resource matches what we sent.
    async fn ensure(
        &self,
        collection: &str,
        id: &str,
        body: &Value,
        same: impl Fn(&Value) -> bool,
    ) -> Result<(), ProviderError> {
        let path = format!("{collection}/{id}");
        if let Some(existing) = self.get(&path).await? {
            return if same(&existing) {
                Ok(())
            } else {
                Err(ProviderError::Rejected(format!(
                    "{collection} `{id}` exists with a different configuration"
                )))
            };
        }
        let (status, resp) = match self
            .send(self.http.post(self.url(collection)).json(body), true)
            .await
        {
            Ok(r) => r,
            Err(ProviderError::Unknown(reason)) => {
                // The write may have happened. Reconcile by deterministic ID
                // with bounded reads; never send a second creation POST.
                for attempt in 1..=3u64 {
                    tokio::time::sleep(Duration::from_millis(150 * attempt)).await;
                    match self.get(&path).await {
                        Ok(Some(existing)) if same(&existing) => return Ok(()),
                        Ok(Some(_)) => {
                            return Err(ProviderError::Rejected(format!(
                                "{collection} `{id}` exists with a different configuration"
                            )))
                        }
                        Ok(None) | Err(_) => continue,
                    }
                }
                return Err(ProviderError::Unknown(format!(
                    "{reason}; {collection} `{id}` not found after reconciliation"
                )));
            }
            Err(e) => return Err(e),
        };
        if (200..300).contains(&status) {
            return Ok(());
        }
        if status == 409 || status == 400 {
            // Created concurrently or by a lost earlier response: reconcile.
            if let Some(existing) = self.get(&path).await? {
                if same(&existing) {
                    return Ok(());
                }
            }
        }
        Err(http_err(status, &resp))
    }

    /// ID of the organization's evidence search tool.
    pub fn evidence_tool_id(&self, org_id: &str) -> String {
        format!("genesis-{}-evidence-search", crate::org_namespace(org_id))
    }

    /// Deterministic agent ID for a role created by a change.
    pub fn agent_id(&self, req: &ProvisionRequest) -> String {
        let digest = canonical_sha256(&json!({
            "org": req.org_id, "change": req.change_id, "role": req.role.key
        }));
        format!(
            "genesis-{}-{}-{}",
            slug(req.org_id.as_str(), 20),
            slug(&req.role.key, 24),
            &digest[..16]
        )
    }

    fn evidence_index(&self, org_id: &str) -> String {
        crate::evidence_index_name(&self.cfg.index_prefix, org_id)
    }

    /// Ensure the organization's `index_search` tool over its own evidence.
    pub async fn ensure_evidence_tool(&self, org_id: &str) -> Result<String, ProviderError> {
        let id = self.evidence_tool_id(org_id);
        let pattern = self.evidence_index(org_id);
        let body = json!({
            "id": id,
            "type": "index_search",
            "description": format!(
                "Search organization {org_id}'s missions, observations, requirements, responsibilities and roles. \
                 Read-only; results may lag the authoritative kernel state."
            ),
            "tags": ["genesis"],
            "configuration": { "pattern": pattern, "row_limit": 50 },
        });
        self.ensure("tools", &id, &body, |t| {
            t["type"] == "index_search" && t["configuration"]["pattern"] == pattern
        })
        .await?;
        Ok(id)
    }

    /// The agent definition sent for a role. Its digest is the binding's
    /// `config_digest`.
    pub fn agent_body(&self, req: &ProvisionRequest, tool_id: &str) -> Value {
        let r = &req.role;
        let scopes: Vec<&str> = r.authority.scopes.iter().map(String::as_str).collect();
        let instructions = format!(
            "You are the \"{name}\" role in the GENESIS organization {org}.\n\
             Charter: {charter}\n\
             Granted scopes: {scopes}. Budget ceiling: {budget} cents.\n\n\
             Use the evidence search tool to ground every answer in the organization's recorded \
             observations, requirements and responsibilities, and cite their IDs. \
             You cannot change the organization, spend money or contact anyone yourself. \
             When an action is needed, reply with a JSON object \
             {{\"proposal\": {{...}}, \"evidence_ids\": [...]}} for the kernel to validate; \
             otherwise reply with findings and the evidence IDs they rest on.",
            name = r.name,
            org = req.org_id,
            charter = r.charter,
            scopes = if scopes.is_empty() {
                "none".into()
            } else {
                scopes.join(", ")
            },
            budget = r.authority.budget_cents,
        );
        json!({
            "id": self.agent_id(req),
            "name": format!("{} ({})", r.name, req.org_id),
            "description": r.charter.chars().take(300).collect::<String>(),
            "labels": ["genesis", format!("org:{}", req.org_id), format!("role:{}", r.key)],
            "configuration": {
                "instructions": instructions,
                "tools": [ { "tool_ids": [tool_id] } ],
            },
        })
    }

    /// Create (or reconcile) the specialist for a provisioning request.
    pub async fn ensure_specialist(
        &self,
        req: &ProvisionRequest,
    ) -> Result<AgentBinding, ProviderError> {
        let tool_id = self.ensure_evidence_tool(req.org_id.as_str()).await?;
        let body = self.agent_body(req, &tool_id);
        let id = body["id"].as_str().unwrap_or_default().to_owned();
        let expected = effective_config(&body);
        // Adopt an existing agent only if its effective configuration is
        // exactly what we would create (no extra tools, same instructions).
        self.ensure("agents", &id, &body, |a| effective_config(a) == expected)
            .await?;
        Ok(AgentBinding {
            provider: Provider::Elastic,
            provider_agent_id: id,
            config_digest: canonical_sha256(&expected),
            conversation_ref: None,
        })
    }

    /// Ready when the agent's effective configuration still matches the
    /// binding's digest exactly and its only tool is this organization's
    /// evidence tool, scoped to this organization's index.
    pub async fn is_ready(&self, binding: &AgentBinding) -> Result<bool, ProviderError> {
        let Some(agent) = self
            .get(&format!("agents/{}", binding.provider_agent_id))
            .await?
        else {
            return Ok(false);
        };
        let effective = effective_config(&agent);
        if agent["id"] != binding.provider_agent_id
            || canonical_sha256(&effective) != binding.config_digest
        {
            return Ok(false);
        }
        // The digest covers the labels, so the organization label is trusted.
        let Some(org) = effective["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find_map(|l| l.strip_prefix("org:"))
            .map(str::to_owned)
        else {
            return Ok(false);
        };
        let expected_tool = self.evidence_tool_id(&org);
        if effective["tool_ids"] != json!([expected_tool]) {
            return Ok(false);
        }
        let Some(tool) = self.get(&format!("tools/{expected_tool}")).await? else {
            return Ok(false);
        };
        Ok(tool["type"] == "index_search"
            && tool["configuration"]["pattern"] == self.evidence_index(&org))
    }

    /// One synchronous turn with an agent. A round waiting for a
    /// human-in-the-loop prompt is reported as rejected: GENESIS does not
    /// answer provider prompts on anyone's behalf.
    pub async fn converse(
        &self,
        agent_id: &str,
        input: &str,
        conversation_id: Option<&str>,
    ) -> Result<ConverseResult, ProviderError> {
        let mut body = json!({ "agent_id": agent_id, "input": input });
        if let Some(c) = conversation_id {
            body["conversation_id"] = json!(c);
        }
        let (status, resp) = self
            .send(self.http.post(self.url("converse")).json(&body), true)
            .await?;
        if !(200..300).contains(&status) {
            return Err(http_err(status, &resp));
        }
        let conversation_id = resp["conversation_id"]
            .as_str()
            .ok_or_else(|| {
                ProviderError::Unknown("converse response has no conversation_id".into())
            })?
            .to_owned();
        let status = resp["status"].as_str().unwrap_or("unknown").to_owned();
        if status == "awaiting_prompt" {
            return Err(ProviderError::Rejected(format!(
                "conversation {conversation_id} is waiting for a human prompt"
            )));
        }
        let tool_calls = resp["steps"]
            .as_array()
            .map(|s| {
                s.iter()
                    .filter(|st| st["type"] == "tool_call")
                    .filter_map(|st| st["tool_id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ConverseResult {
            conversation_id,
            round_id: resp["round_id"].as_str().map(str::to_owned),
            status,
            message: resp["response"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            tool_calls,
        })
    }
}

/// The parts of an agent definition that determine its behavior and scope,
/// normalized (sorted labels and tool IDs) so the provider's ordering or extra
/// response fields do not matter. Its digest is the binding's `config_digest`.
fn effective_config(agent: &Value) -> Value {
    let mut labels: Vec<String> = agent["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l.as_str().map(str::to_owned))
        .collect();
    labels.sort();
    let mut tools = tool_ids(agent);
    tools.sort();
    json!({
        "id": agent["id"],
        "name": agent["name"],
        "description": agent["description"],
        "labels": labels,
        "instructions": agent["configuration"]["instructions"],
        "tool_ids": tools,
    })
}

fn tool_ids(agent: &Value) -> Vec<String> {
    agent["configuration"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["tool_ids"].as_array().into_iter().flatten())
        .filter_map(|t| t.as_str().map(str::to_owned))
        .collect()
}

#[async_trait]
impl RoleProvisioner for AgentBuilder {
    fn provider(&self) -> Provider {
        Provider::Elastic
    }

    async fn provision(&self, req: &ProvisionRequest) -> Result<AgentBinding, ProviderError> {
        if req.role.provider != Provider::Elastic {
            return Err(ProviderError::Rejected(format!(
                "role `{}` is not an Elastic role",
                req.role.key
            )));
        }
        self.ensure_specialist(req).await
    }

    async fn check_ready(&self, binding: &AgentBinding) -> Result<bool, ProviderError> {
        self.is_ready(binding).await
    }
}
