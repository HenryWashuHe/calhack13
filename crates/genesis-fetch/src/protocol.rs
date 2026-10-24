//! The application context lives inside the SDK-signed payload. The native
//! envelope's unsigned version/protocol metadata is not an authority boundary.
use chrono::{DateTime, Utc};
use genesis_verification::{decode_json, json_sha256};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_DIGEST: &str =
    "proto:5ff9983d2f971db76bc44c6bb98c9de76f825023ebfff1366d0352a37f2b0756";
pub const MODEL_DIGEST: &str =
    "model:3d35410a0bac7663333dda8af6d33f8e7c4d2677da8dd74804a2559574d46e2b";
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("invalid, ambiguous or oversized Fetch protocol message")]
pub struct ProtocolError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub schema_version: u32,
    pub message_id: String,
    pub org_id: String,
    pub mission_id: String,
    pub contract_id: String,
    pub correlation_id: String,
    pub expires_at: DateTime<Utc>,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Body {
    RecruitRequest {
        requirement: String,
        capabilities: Vec<String>,
        lease_expires_at: DateTime<Utc>,
    },
    Offer {
        recruit_message_id: String,
        offer_id: String,
        capabilities: Vec<String>,
        terms: Value,
        lease_expires_at: DateTime<Utc>,
    },
    AcceptOffer {
        offer_message_id: String,
        offer_id: String,
    },
    ContractAccepted {
        accept_message_id: String,
        offer_id: String,
    },
    TaskRequest {
        task: Task,
    },
    TaskAccepted {
        task_request_id: String,
        job_id: String,
        attempt_id: String,
    },
    TaskProgress {
        task_request_id: String,
        job_id: String,
        attempt_id: String,
        detail: String,
    },
    TaskResult {
        task_request_id: String,
        job_id: String,
        attempt_id: String,
        epoch: u64,
        output: Value,
    },
    ReleaseContract {
        reason: String,
    },
    ContractReleased {
        release_message_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub job_id: String,
    pub attempt_id: String,
    pub role_id: String,
    pub assignment_id: String,
    pub idempotency_key: String,
    pub epoch: u64,
    pub deadline: DateTime<Utc>,
    pub input: Value,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}
fn capabilities(values: &[String]) -> bool {
    !values.is_empty()
        && values.len() <= 16
        && values.iter().all(|v| identifier(v))
        && values
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == values.len()
}
fn bounded(value: &Value) -> bool {
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 32 * 1024)
}

/// Native agent address syntax and Bech32 checksum. Sender authentication still
/// requires the SDK signature check at the bridge; an address alone is no proof.
pub fn address(value: &str) -> bool {
    if value.len() != 65 || !value.starts_with("agent1") {
        return false;
    }
    let alphabet = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    let mut data = Vec::new();
    for b in b"agent" {
        data.push(b >> 5);
    }
    data.push(0);
    for b in b"agent" {
        data.push(b & 31);
    }
    for b in value[6..].bytes() {
        let Some(n) = alphabet.iter().position(|c| *c == b) else {
            return false;
        };
        data.push(n as u8);
    }
    let mut check = 1u32;
    for value in data {
        let top = check >> 25;
        check = ((check & 0x1ffffff) << 5) ^ value as u32;
        for (i, generator) in [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
            .iter()
            .enumerate()
        {
            if (top >> i) & 1 != 0 {
                check ^= generator;
            }
        }
    }
    check == 1
}

impl Task {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if [
            &self.job_id,
            &self.attempt_id,
            &self.role_id,
            &self.assignment_id,
            &self.idempotency_key,
        ]
        .iter()
        .all(|v| identifier(v))
            && bounded(&self.input)
        {
            Ok(())
        } else {
            Err(ProtocolError)
        }
    }
}
impl Message {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(ProtocolError);
        }
        let result: Self = decode_json(bytes).map_err(|_| ProtocolError)?;
        result.validate()?;
        Ok(result)
    }
    pub fn canonical_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        Ok(json_sha256(
            &serde_json::to_value(self).map_err(|_| ProtocolError)?,
        ))
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1
            || !uuid(&self.message_id)
            || ![
                &self.org_id,
                &self.mission_id,
                &self.contract_id,
                &self.correlation_id,
            ]
            .iter()
            .all(|v| identifier(v))
            || serde_json::to_vec(self).map_err(|_| ProtocolError)?.len() > MAX_MESSAGE_BYTES
        {
            return Err(ProtocolError);
        }
        let valid = match &self.body {
            Body::RecruitRequest {
                requirement,
                capabilities: caps,
                lease_expires_at,
            } => {
                text(requirement, 4096)
                    && capabilities(caps)
                    && self.expires_at <= *lease_expires_at
            }
            Body::Offer {
                recruit_message_id,
                offer_id,
                capabilities: caps,
                terms,
                lease_expires_at,
            } => {
                uuid(recruit_message_id)
                    && identifier(offer_id)
                    && capabilities(caps)
                    && bounded(terms)
                    && self.expires_at <= *lease_expires_at
            }
            Body::AcceptOffer {
                offer_message_id,
                offer_id,
            } => uuid(offer_message_id) && identifier(offer_id),
            Body::ContractAccepted {
                accept_message_id,
                offer_id,
            } => uuid(accept_message_id) && identifier(offer_id),
            Body::TaskRequest { task } => {
                task.validate().is_ok() && task.deadline <= self.expires_at
            }
            Body::TaskAccepted {
                task_request_id,
                job_id,
                attempt_id,
            } => uuid(task_request_id) && identifier(job_id) && identifier(attempt_id),
            Body::TaskProgress {
                task_request_id,
                job_id,
                attempt_id,
                detail,
            } => {
                uuid(task_request_id)
                    && identifier(job_id)
                    && identifier(attempt_id)
                    && text(detail, 4096)
            }
            Body::TaskResult {
                task_request_id,
                job_id,
                attempt_id,
                output,
                ..
            } => {
                uuid(task_request_id)
                    && identifier(job_id)
                    && identifier(attempt_id)
                    && bounded(output)
            }
            Body::ReleaseContract { reason } => text(reason, 4096),
            Body::ContractReleased { release_message_id } => uuid(release_message_id),
        };
        if valid {
            Ok(())
        } else {
            Err(ProtocolError)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendRequest {
    pub schema_version: u32,
    pub recipient: String,
    pub message: Message,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendReceipt {
    pub schema_version: u32,
    pub message_id: String,
    pub bridge_address: String,
    pub recipient: String,
    pub state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeIdentity {
    pub schema_version: u32,
    pub address: String,
    pub protocol_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxItem {
    pub sequence: u64,
    pub sender: String,
    pub message: Message,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxPage {
    pub schema_version: u32,
    pub address: String,
    pub items: Vec<InboxItem>,
    pub next_cursor: u64,
}
