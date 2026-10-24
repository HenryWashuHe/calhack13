//! Agentverse v1 discovery returns advertisements, never execution authority.
//! The pinned endpoint is documented by Fetch; migration requires a separately
//! verified v2 contract. No advertised URL is fetched by this module.

use crate::client::{
    loopback_base, valid_protocol_digest, ClientError, ClientErrorKind, ClientLimits, Transport,
};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fmt};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub search_text: String,
    /// Required exact protocol, derived from the SDK wire model/protocol.
    pub protocol_digest: String,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub agent_types: Vec<String>,
    #[serde(default)]
    pub offset: u64,
    pub limit: u32,
    #[serde(default)]
    pub search_id: Option<String>,
}

impl SearchRequest {
    fn body(&self) -> Result<Value, ClientError> {
        let allowed_states = ["active", "inactive", "responsive", "unresponsive"];
        let allowed_types = [
            "uagent", "a2a", "hosted", "mailbox", "proxy", "local", "custom",
        ];
        if self.search_text.trim().is_empty()
            || self.search_text.len() > 4096
            || !valid_protocol_digest(&self.protocol_digest)
            || !(1..=50).contains(&self.limit)
            || self.offset.checked_add(u64::from(self.limit)).is_none()
            || !valid_filters(&self.states, &allowed_states)
            || !valid_filters(&self.agent_types, &allowed_types)
            || self.search_id.as_deref().is_some_and(|id| !uuid(id))
        {
            return Err(ClientError::new(ClientErrorKind::InvalidRequest));
        }
        let mut body = json!({
            "search_text": self.search_text,
            "filters": {
                "state": self.states,
                "agent_type": self.agent_types,
                "protocol_digest": [self.protocol_digest],
            },
            "sort": "relevancy", "direction": "desc", "offset": self.offset, "limit": self.limit,
        });
        if let Some(id) = &self.search_id {
            body["search_id"] = json!(id);
        }
        Ok(body)
    }
}

fn valid_filters(values: &[String], allowed: &[&str]) -> bool {
    values.len() <= allowed.len()
        && values.iter().all(|v| allowed.contains(&v.as_str()))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| id.to_string() == value && id.get_version_num() == 4)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub address: String,
    pub protocol_digests: Vec<String>,
    /// Bounded advertisement metadata. Untrusted; never a routing destination,
    /// authenticated sender, readiness proof, or authorization grant.
    pub advertised_metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPage {
    pub offset: u64,
    pub limit: u32,
    /// Provider hit count before discarding incompatible advertisements.
    pub num_hits: u64,
    pub total: u64,
    pub search_id: String,
    pub candidates: Vec<Candidate>,
    pub rejected_candidates: u32,
}

#[derive(Deserialize)]
struct RawPage {
    offset: u64,
    limit: u32,
    num_hits: u64,
    total: u64,
    search_id: String,
    #[serde(default)]
    agents: Vec<Value>,
}

#[derive(Clone)]
pub struct DiscoveryClient {
    transport: Transport,
    endpoint: Url,
}

impl fmt::Debug for DiscoveryClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscoveryClient")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl DiscoveryClient {
    pub fn new(token: &str, limits: ClientLimits) -> Result<Self, ClientError> {
        Ok(Self {
            transport: Transport::new(token, limits)?,
            endpoint: Url::parse("https://agentverse.ai/v1/search/agents").expect("fixed endpoint"),
        })
    }

    /// Explicit test-only HTTP origin. Only literal loopback, exact `/v1` path.
    pub fn for_local_testing(
        token: &str,
        limits: ClientLimits,
        base: &str,
    ) -> Result<Self, ClientError> {
        let mut endpoint = loopback_base(base, "/v1")?;
        endpoint.set_path("/v1/search/agents");
        Ok(Self {
            transport: Transport::new(token, limits)?,
            endpoint,
        })
    }

    pub async fn search(&self, request: &SearchRequest) -> Result<SearchPage, ClientError> {
        let raw: RawPage = self
            .transport
            .json(
                Method::POST,
                self.endpoint.clone(),
                Some(request.body()?),
                200,
            )
            .await?;
        let malformed = || {
            let mut error = ClientError::new(ClientErrorKind::MalformedResponse);
            error.post_may_have_run = true;
            error
        };
        if raw.offset != request.offset
            || raw.limit != request.limit
            || raw.num_hits != raw.agents.len() as u64
            || raw.agents.len() > request.limit as usize
            || raw.total < raw.num_hits
            || !uuid(&raw.search_id)
            || request
                .search_id
                .as_ref()
                .is_some_and(|id| id != &raw.search_id)
        {
            return Err(malformed());
        }
        let mut candidates = Vec::new();
        let mut addresses = BTreeSet::new();
        let mut rejected_candidates = 0;
        for agent in raw.agents {
            let Some(candidate) = candidate(agent, &request.protocol_digest) else {
                rejected_candidates += 1;
                continue;
            };
            if !addresses.insert(candidate.address.clone()) {
                return Err(malformed());
            }
            candidates.push(candidate);
        }
        Ok(SearchPage {
            offset: raw.offset,
            limit: raw.limit,
            num_hits: raw.num_hits,
            total: raw.total,
            search_id: raw.search_id,
            candidates,
            rejected_candidates,
        })
    }
}

fn candidate(raw: Value, requested_digest: &str) -> Option<Candidate> {
    if serde_json::to_vec(&raw).ok()?.len() > 32 * 1024 {
        return None;
    }
    let address = raw.get("address")?.as_str()?.to_owned();
    if !crate::protocol::address(&address) {
        return None;
    }
    let protocols = raw.get("protocols")?.as_array()?;
    if protocols.len() > 128 {
        return None;
    }
    let mut digests = BTreeSet::new();
    for protocol in protocols {
        // Official examples contain both strings and {name,version,digest}.
        let digest = protocol
            .as_str()
            .or_else(|| protocol.get("digest")?.as_str());
        if let Some(digest) = digest.filter(|digest| valid_protocol_digest(digest)) {
            digests.insert(digest.to_owned());
        }
    }
    if !digests.contains(requested_digest) {
        return None;
    }
    Some(Candidate {
        address,
        protocol_digests: digests.into_iter().collect(),
        advertised_metadata: raw,
    })
}
