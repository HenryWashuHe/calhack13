//! Readiness proven by a backend-observed gateway receipt.
//!
//! An asynchronously created agent (CREAO) is not ready because its creation
//! run completed or because it says so. [`GatewayReadiness`] proves readiness
//! by a round trip the backend observes itself:
//!
//! 1. The kernel issues a one-time **challenge** for the exact created agent
//!    (organization, role, provider agent ID, configuration digest). Only the
//!    SHA-256 of its random nonce is stored.
//! 2. A provider-specific [`ReadinessProbe`] asks that agent to run its
//!    gateway skill with the challenge (for CREAO: a readiness run on the
//!    created agent; implemented on the provider side).
//! 3. The skill posts the challenge back to
//!    `POST /api/v1/gateway/readiness-receipts`. The kernel accepts it only
//!    for a live challenge with a matching nonce and agent ID, once.
//! 4. The next provisioning pass sees the receipt and reports
//!    [`Readiness::Ready`] with `gateway-receipt:{challenge_id}` as evidence.
//!
//! A challenge is dispatched at most once. If the dispatch outcome is unknown
//! the challenge stays valid until it expires, so a late receipt still counts;
//! then a new challenge is issued, up to `max_attempts`, after which the role
//! fails readiness. Created provider IDs are kept either way.

use crate::specialist::{Readiness, ReadinessVerifier};
use crate::store::Store;
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use genesis_core::engine::RoleToProvision;
use genesis_core::ids::OrgId;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::ProviderError;
use genesis_verification::json_sha256;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Path of the receipt route under the API root.
pub const RECEIPT_PATH: &str = "/api/v1/gateway/readiness-receipts";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS readiness_challenges (
    challenge_id       TEXT PRIMARY KEY,
    subject            TEXT NOT NULL,
    attempt            INTEGER NOT NULL,
    nonce_sha256       TEXT NOT NULL,
    provider           TEXT NOT NULL,
    role_id            TEXT NOT NULL,
    role_key           TEXT NOT NULL,
    provider_agent_id  TEXT NOT NULL,
    config_digest      TEXT NOT NULL,
    status             TEXT NOT NULL,
    probe_ref          TEXT,
    detail             TEXT,
    issued_at          TEXT NOT NULL,
    expires_at         TEXT NOT NULL,
    received_at        TEXT,
    UNIQUE(subject, attempt)
);
"#;

/// Create the challenge table if needed. Idempotent.
pub fn ensure_schema(store: &Store) -> rusqlite::Result<()> {
    store.lock().execute_batch(SCHEMA)
}

/// Lifecycle of one challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStatus {
    /// Saved; the probe dispatch has started but its outcome is not recorded
    /// (in flight, crashed, or unknown). A receipt is accepted until expiry.
    Dispatching,
    /// The probe accepted the request.
    Dispatched,
    /// The probe definitely failed; the nonce never left the backend.
    DispatchFailed,
    /// A valid receipt arrived: readiness evidence.
    Received,
    /// Expired without a receipt.
    Expired,
}

impl ChallengeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dispatching => "dispatching",
            Self::Dispatched => "dispatched",
            Self::DispatchFailed => "dispatch_failed",
            Self::Received => "received",
            Self::Expired => "expired",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "dispatching" => Self::Dispatching,
            "dispatched" => Self::Dispatched,
            "dispatch_failed" => Self::DispatchFailed,
            "received" => Self::Received,
            _ => Self::Expired,
        }
    }
    fn accepts_receipt(self) -> bool {
        matches!(self, Self::Dispatching | Self::Dispatched)
    }
}

/// A challenge as shown to operators. Never includes the nonce or its hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Challenge {
    pub challenge_id: String,
    pub attempt: u32,
    pub provider: String,
    pub role_id: String,
    pub role_key: String,
    pub provider_agent_id: String,
    pub config_digest: String,
    pub status: ChallengeStatus,
    pub probe_ref: Option<String>,
    pub detail: Option<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub received_at: Option<DateTime<Utc>>,
}

/// What a probe needs to ask the created agent to call the gateway.
#[derive(Clone)]
pub struct ProbeRequest {
    pub org_id: OrgId,
    pub challenge_id: String,
    /// One-time secret the agent must echo. Never log it.
    pub nonce: String,
    /// Absolute URL of the receipt route.
    pub receipt_url: String,
    pub role_id: String,
    pub role_key: String,
    pub provider_agent_id: String,
    pub config_digest: String,
    pub expires_at: DateTime<Utc>,
}

impl fmt::Debug for ProbeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProbeRequest")
            .field("challenge_id", &self.challenge_id)
            .field("nonce", &"[REDACTED]")
            .field("receipt_url", &self.receipt_url)
            .field("provider_agent_id", &self.provider_agent_id)
            .finish_non_exhaustive()
    }
}

impl ProbeRequest {
    /// The JSON body the agent's gateway skill must POST to `receipt_url`.
    pub fn expected_receipt(&self) -> serde_json::Value {
        json!({
            "challenge_id": self.challenge_id,
            "nonce": self.nonce,
            "provider_agent_id": self.provider_agent_id,
        })
    }
}

/// Provider-side reference for a dispatched probe (e.g. a run ID).
#[derive(Debug, Clone, Default)]
pub struct ProbeDispatch {
    pub probe_ref: Option<String>,
}

/// Asks a created agent to run its gateway skill with a challenge. Called at
/// most once per challenge. Return [`ProviderError::Unknown`] or
/// [`ProviderError::Timeout`] when the request may have reached the provider.
#[async_trait]
pub trait ReadinessProbe: Send + Sync {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError>;
}

/// Gateway readiness settings.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub receipt_url: String,
    /// How long a dispatched challenge waits for its receipt.
    pub ttl: Duration,
    /// Wait after a definite dispatch failure before the next attempt.
    pub retry_after: Duration,
    pub max_attempts: u32,
}

impl GatewayConfig {
    /// `public_base_url` is where the provider's sandbox reaches this kernel,
    /// e.g. `https://genesis.example.com`. Plain HTTP is allowed only for
    /// loopback hosts (local tests).
    pub fn from_public_url(public_base_url: &str) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(public_base_url.trim())
            .map_err(|_| anyhow::anyhow!("public URL is not a valid absolute URL"))?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        anyhow::ensure!(
            url.scheme() == "https" || (url.scheme() == "http" && loopback),
            "public URL must use https (http only for loopback)"
        );
        anyhow::ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "public URL must not carry credentials, a query or a fragment"
        );
        let base = url.as_str().trim_end_matches('/');
        Ok(Self {
            receipt_url: format!("{base}{RECEIPT_PATH}"),
            ttl: Duration::from_secs(300),
            retry_after: Duration::from_secs(15),
            max_attempts: 3,
        })
    }
}

/// [`ReadinessVerifier`] backed by gateway receipts.
pub struct GatewayReadiness {
    store: Store,
    org_id: OrgId,
    provider: Provider,
    probe: Arc<dyn ReadinessProbe>,
    cfg: GatewayConfig,
}

fn store_error(_e: impl fmt::Display) -> ProviderError {
    ProviderError::Rejected("readiness challenge store unavailable".into())
}

fn provider_name(p: Provider) -> String {
    serde_json::to_value(p)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{p:?}").to_lowercase())
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn chrono(d: Duration) -> ChronoDuration {
    ChronoDuration::from_std(d).unwrap_or(ChronoDuration::seconds(60))
}

fn nonce_hash(nonce: &str) -> String {
    json_sha256(&serde_json::Value::String(nonce.to_owned()))
}

/// Constant-time comparison of two equal-length hex digests.
fn digest_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

const COLUMNS: &str = "challenge_id, attempt, provider, role_id, role_key, provider_agent_id, \
    config_digest, status, probe_ref, detail, issued_at, expires_at, received_at, nonce_sha256";

fn row_to_challenge(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Challenge, String)> {
    Ok((
        Challenge {
            challenge_id: row.get(0)?,
            attempt: row.get(1)?,
            provider: row.get(2)?,
            role_id: row.get(3)?,
            role_key: row.get(4)?,
            provider_agent_id: row.get(5)?,
            config_digest: row.get(6)?,
            status: ChallengeStatus::parse(&row.get::<_, String>(7)?),
            probe_ref: row.get(8)?,
            detail: row.get(9)?,
            issued_at: parse_ts(&row.get::<_, String>(10)?),
            expires_at: parse_ts(&row.get::<_, String>(11)?),
            received_at: row.get::<_, Option<String>>(12)?.map(|s| parse_ts(&s)),
        },
        row.get(13)?,
    ))
}

/// Look up a challenge for operators (no secret material).
pub fn get_challenge(store: &Store, challenge_id: &str) -> rusqlite::Result<Option<Challenge>> {
    ensure_schema(store)?;
    store
        .lock()
        .query_row(
            &format!("SELECT {COLUMNS} FROM readiness_challenges WHERE challenge_id = ?1"),
            params![challenge_id],
            row_to_challenge,
        )
        .optional()
        .map(|o| o.map(|(c, _)| c))
}

fn set_status(
    store: &Store,
    challenge_id: &str,
    status: ChallengeStatus,
    probe_ref: Option<&str>,
    detail: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> rusqlite::Result<()> {
    store.lock().execute(
        "UPDATE readiness_challenges SET status = ?2, probe_ref = COALESCE(?3, probe_ref), \
         detail = COALESCE(?4, detail), expires_at = COALESCE(?5, expires_at) \
         WHERE challenge_id = ?1",
        params![
            challenge_id,
            status.as_str(),
            probe_ref,
            detail,
            expires_at.map(ts)
        ],
    )?;
    Ok(())
}

impl GatewayReadiness {
    pub fn new(
        store: Store,
        org_id: OrgId,
        provider: Provider,
        probe: Arc<dyn ReadinessProbe>,
        cfg: GatewayConfig,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(cfg.max_attempts >= 1, "max_attempts must be at least 1");
        ensure_schema(&store)?;
        Ok(Self {
            store,
            org_id,
            provider,
            probe,
            cfg,
        })
    }

    fn subject(&self, role: &RoleToProvision, binding: &AgentBinding) -> String {
        json_sha256(&json!({
            "org_id": self.org_id,
            "provider": provider_name(binding.provider),
            "role_id": role.role_id,
            "provider_agent_id": binding.provider_agent_id,
            "config_digest": binding.config_digest,
        }))
    }

    fn latest(&self, subject: &str) -> Result<Option<Challenge>, ProviderError> {
        self.store
            .lock()
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM readiness_challenges WHERE subject = ?1 \
                     ORDER BY attempt DESC LIMIT 1"
                ),
                params![subject],
                row_to_challenge,
            )
            .optional()
            .map(|o| o.map(|(c, _)| c))
            .map_err(store_error)
    }

    /// Save a new challenge, then dispatch its probe once.
    async fn issue(
        &self,
        subject: &str,
        attempt: u32,
        role: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        let challenge_id = format!("rc-{}", uuid::Uuid::new_v4().simple());
        let nonce = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let now = Utc::now();
        let expires_at = now + chrono(self.cfg.ttl);
        let inserted = self
            .store
            .lock()
            .execute(
                "INSERT OR IGNORE INTO readiness_challenges (challenge_id, subject, attempt, \
                 nonce_sha256, provider, role_id, role_key, provider_agent_id, config_digest, \
                 status, issued_at, expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    challenge_id,
                    subject,
                    attempt,
                    nonce_hash(&nonce),
                    provider_name(binding.provider),
                    role.role_id.to_string(),
                    role.key,
                    binding.provider_agent_id,
                    binding.config_digest,
                    ChallengeStatus::Dispatching.as_str(),
                    ts(now),
                    ts(expires_at),
                ],
            )
            .map_err(store_error)?;
        if inserted == 0 {
            return Ok(Readiness::Pending); // another pass issued this attempt
        }
        let request = ProbeRequest {
            org_id: self.org_id.clone(),
            challenge_id: challenge_id.clone(),
            nonce,
            receipt_url: self.cfg.receipt_url.clone(),
            role_id: role.role_id.to_string(),
            role_key: role.key.clone(),
            provider_agent_id: binding.provider_agent_id.clone(),
            config_digest: binding.config_digest.clone(),
            expires_at,
        };
        tracing::info!(challenge = %challenge_id, attempt, role = %role.key, "readiness challenge issued");
        match self.probe.dispatch(&request).await {
            Ok(d) => {
                set_status(
                    &self.store,
                    &challenge_id,
                    ChallengeStatus::Dispatched,
                    d.probe_ref.as_deref(),
                    None,
                    None,
                )
                .map_err(store_error)?;
                Ok(Readiness::Pending)
            }
            // May have reached the provider: keep the challenge live until expiry.
            Err(e @ (ProviderError::Unknown(_) | ProviderError::Timeout)) => {
                set_status(
                    &self.store,
                    &challenge_id,
                    ChallengeStatus::Dispatching,
                    None,
                    Some("probe outcome unknown; a receipt is accepted until expiry"),
                    None,
                )
                .map_err(store_error)?;
                Err(e)
            }
            Err(e) => {
                set_status(
                    &self.store,
                    &challenge_id,
                    ChallengeStatus::DispatchFailed,
                    None,
                    Some("probe dispatch failed"),
                    Some(Utc::now() + chrono(self.cfg.retry_after)),
                )
                .map_err(store_error)?;
                Err(e)
            }
        }
    }
}

#[async_trait]
impl ReadinessVerifier for GatewayReadiness {
    async fn verify(
        &self,
        role: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        if binding.provider != self.provider || role.provider != self.provider {
            return Ok(Readiness::NotReady(
                "binding belongs to a different provider".into(),
            ));
        }
        let subject = self.subject(role, binding);
        let now = Utc::now();
        let attempt = match self.latest(&subject)? {
            None => 1,
            Some(c) if c.status == ChallengeStatus::Received => {
                return Ok(Readiness::Ready {
                    evidence_ref: format!("gateway-receipt:{}", c.challenge_id),
                })
            }
            Some(c) if c.status != ChallengeStatus::Expired && c.expires_at > now => {
                return Ok(Readiness::Pending)
            }
            Some(c) => {
                // A failed dispatch keeps its status; only a live challenge expires.
                if c.status.accepts_receipt() {
                    set_status(
                        &self.store,
                        &c.challenge_id,
                        ChallengeStatus::Expired,
                        None,
                        None,
                        None,
                    )
                    .map_err(store_error)?;
                }
                if c.attempt >= self.cfg.max_attempts {
                    return Ok(Readiness::NotReady(format!(
                        "no gateway readiness receipt after {} attempt(s)",
                        c.attempt
                    )));
                }
                c.attempt + 1
            }
        };
        self.issue(&subject, attempt, role, binding).await
    }
}

/// Body posted by the agent's gateway skill.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessReceipt {
    pub challenge_id: String,
    pub nonce: String,
    pub provider_agent_id: String,
    #[serde(default)]
    pub probe_ref: Option<String>,
}

impl fmt::Debug for ReadinessReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadinessReceipt")
            .field("challenge_id", &self.challenge_id)
            .field("nonce", &"[REDACTED]")
            .field("provider_agent_id", &self.provider_agent_id)
            .field("probe_ref", &self.probe_ref)
            .finish()
    }
}

/// Why a receipt was refused. Unknown challenge, wrong nonce and wrong agent
/// are indistinguishable to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptRejection {
    Invalid,
    Expired,
}

/// Accepted receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiptAccepted {
    pub challenge_id: String,
    pub duplicate: bool,
}

/// Validate and record a receipt. Single use: a repeat of an accepted receipt
/// is reported as a duplicate and changes nothing.
pub fn accept_receipt(
    store: &Store,
    receipt: &ReadinessReceipt,
    now: DateTime<Utc>,
) -> Result<Result<ReceiptAccepted, ReceiptRejection>, rusqlite::Error> {
    let bounded = |s: &str| !s.is_empty() && s.len() <= 256;
    if !bounded(&receipt.challenge_id)
        || !bounded(&receipt.nonce)
        || !bounded(&receipt.provider_agent_id)
        || receipt.probe_ref.as_deref().is_some_and(|p| !bounded(p))
    {
        return Ok(Err(ReceiptRejection::Invalid));
    }
    ensure_schema(store)?;
    let conn = store.lock();
    let found = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM readiness_challenges WHERE challenge_id = ?1"),
            params![receipt.challenge_id],
            row_to_challenge,
        )
        .optional()?;
    let Some((c, stored_hash)) = found else {
        return Ok(Err(ReceiptRejection::Invalid));
    };
    let probe_mismatch = matches!(
        (&c.probe_ref, &receipt.probe_ref),
        (Some(a), Some(b)) if a != b
    );
    if !digest_eq(&stored_hash, &nonce_hash(&receipt.nonce))
        || c.provider_agent_id != receipt.provider_agent_id
        || probe_mismatch
    {
        return Ok(Err(ReceiptRejection::Invalid));
    }
    if c.status == ChallengeStatus::Received {
        return Ok(Ok(ReceiptAccepted {
            challenge_id: c.challenge_id,
            duplicate: true,
        }));
    }
    if c.status == ChallengeStatus::DispatchFailed {
        return Ok(Err(ReceiptRejection::Invalid));
    }
    if !c.status.accepts_receipt() || c.expires_at <= now {
        return Ok(Err(ReceiptRejection::Expired));
    }
    let updated = conn.execute(
        "UPDATE readiness_challenges SET status = 'received', received_at = ?2, \
         probe_ref = COALESCE(probe_ref, ?3) WHERE challenge_id = ?1 \
         AND status IN ('dispatching', 'dispatched')",
        params![c.challenge_id, ts(now), receipt.probe_ref],
    )?;
    tracing::info!(challenge = %c.challenge_id, role = %c.role_key, "readiness receipt accepted");
    Ok(Ok(ReceiptAccepted {
        challenge_id: c.challenge_id,
        duplicate: updated == 0,
    }))
}

/// Register gateway readiness for `provider` on an organization.
pub fn attach(
    app: crate::app::AppState,
    provider: Provider,
    probe: Arc<dyn ReadinessProbe>,
    cfg: GatewayConfig,
) -> anyhow::Result<crate::app::AppState> {
    let verifier =
        GatewayReadiness::new(app.store.clone(), app.org_id.clone(), provider, probe, cfg)?;
    Ok(app.with_readiness_verifier(provider, Arc::new(verifier)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_url_rules() {
        let c = GatewayConfig::from_public_url("https://genesis.example.com/").unwrap();
        assert_eq!(
            c.receipt_url,
            "https://genesis.example.com/api/v1/gateway/readiness-receipts"
        );
        assert!(GatewayConfig::from_public_url("http://127.0.0.1:8080").is_ok());
        assert!(GatewayConfig::from_public_url("http://genesis.example.com").is_err());
        assert!(GatewayConfig::from_public_url("https://u:p@genesis.example.com").is_err());
        assert!(GatewayConfig::from_public_url("https://genesis.example.com/?x=1").is_err());
    }

    #[test]
    fn digest_comparison() {
        assert!(digest_eq("abcd", "abcd"));
        assert!(!digest_eq("abcd", "abce"));
        assert!(!digest_eq("abcd", "abc"));
    }

    #[test]
    fn probe_request_debug_redacts_nonce() {
        let r = ProbeRequest {
            org_id: OrgId::new("org_a"),
            challenge_id: "rc-1".into(),
            nonce: "supersecretnonce".into(),
            receipt_url: "https://x/".into(),
            role_id: "r".into(),
            role_key: "k".into(),
            provider_agent_id: "a".into(),
            config_digest: "d".into(),
            expires_at: Utc::now(),
        };
        assert!(!format!("{r:?}").contains("supersecretnonce"));
    }
}
