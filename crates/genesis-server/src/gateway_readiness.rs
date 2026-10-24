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
//! fails readiness. Definite dispatch failures back off and are limited
//! separately. Status changes after a dispatch apply only to a challenge that
//! is still `dispatching`, so a receipt accepted meanwhile is never lost.
//! Created provider IDs are kept either way.

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
    receipt_probe_ref  TEXT,
    dispatch_resolved_at TEXT,
    detail             TEXT,
    issued_at          TEXT NOT NULL,
    expires_at         TEXT NOT NULL,
    received_at        TEXT,
    UNIQUE(subject, attempt)
);
"#;

/// Create the challenge table if needed. Idempotent. Tables created before
/// `receipt_probe_ref`/`dispatch_resolved_at` gain them; rows that had already
/// left `dispatching` recorded their dispatch outcome under the old code, so
/// they count as resolved.
pub fn ensure_schema(store: &Store) -> rusqlite::Result<()> {
    let conn = store.lock();
    conn.execute_batch(SCHEMA)?;
    let has = |col: &str| -> rusqlite::Result<bool> {
        conn.prepare("SELECT 1 FROM pragma_table_info('readiness_challenges') WHERE name = ?1")?
            .exists(params![col])
    };
    if !has("receipt_probe_ref")? {
        conn.execute_batch("ALTER TABLE readiness_challenges ADD COLUMN receipt_probe_ref TEXT")?;
    }
    if !has("dispatch_resolved_at")? {
        conn.execute_batch(
            "ALTER TABLE readiness_challenges ADD COLUMN dispatch_resolved_at TEXT; \
             UPDATE readiness_challenges SET dispatch_resolved_at = issued_at \
             WHERE status != 'dispatching';",
        )?;
    }
    Ok(())
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
    /// Provider run the probe's dispatch reported (trusted), once known.
    pub probe_ref: Option<String>,
    /// Run the agent's receipt named, if any (untrusted until it matches).
    pub receipt_probe_ref: Option<String>,
    /// When dispatch's outcome was durably recorded. `None` means it was
    /// never recorded (in flight, or lost to a crash or failed write); a
    /// receipt is never accepted as readiness until it is set.
    pub dispatch_resolved_at: Option<DateTime<Utc>>,
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
    /// Challenges that may reach the agent (dispatched or with an unknown
    /// dispatch outcome) before readiness fails for lack of a receipt.
    pub max_attempts: u32,
    /// First wait after a definite dispatch failure; doubles per failure,
    /// capped at ten minutes.
    pub retry_after: Duration,
    /// Definite dispatch failures before readiness fails. They do not count
    /// toward `max_attempts`: their nonce never left the backend.
    pub max_dispatch_failures: u32,
}

impl GatewayConfig {
    fn backoff(&self, failures: u32) -> Duration {
        let factor = 2u32.saturating_pow(failures.saturating_sub(1).min(16));
        self.retry_after
            .saturating_mul(factor)
            .min(Duration::from_secs(600))
    }
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
            max_attempts: 3,
            retry_after: Duration::from_secs(15),
            max_dispatch_failures: 5,
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
    config_digest, status, probe_ref, detail, issued_at, expires_at, received_at, nonce_sha256, \
    receipt_probe_ref, dispatch_resolved_at";

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
            receipt_probe_ref: row.get(14)?,
            dispatch_resolved_at: row.get::<_, Option<String>>(15)?.map(|s| parse_ts(&s)),
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

/// Move a challenge to `to` only while it is still in one of `from`. Returns
/// whether it moved. A receipt accepted meanwhile is never overwritten.
fn transition(
    store: &Store,
    challenge_id: &str,
    from: &[ChallengeStatus],
    to: ChallengeStatus,
    probe_ref: Option<&str>,
    detail: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> rusqlite::Result<bool> {
    // Status names are fixed identifiers, never caller input.
    let allowed = from
        .iter()
        .map(|s| format!("'{}'", s.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let moved = store.lock().execute(
        &format!(
            "UPDATE readiness_challenges SET status = ?2, probe_ref = COALESCE(?3, probe_ref), \
             detail = COALESCE(?4, detail), expires_at = COALESCE(?5, expires_at) \
             WHERE challenge_id = ?1 AND status IN ({allowed})"
        ),
        params![
            challenge_id,
            to.as_str(),
            probe_ref,
            detail,
            expires_at.map(ts)
        ],
    )?;
    Ok(moved == 1)
}

/// What an operator may see about a probe error: its kind and HTTP status,
/// never the provider's message or body (which could echo the run input).
fn probe_error_kind(e: &ProviderError) -> String {
    match e {
        ProviderError::NotConfigured { .. } => "provider not configured".into(),
        ProviderError::Http { status, .. } => format!("provider returned HTTP {status}"),
        ProviderError::Timeout => "timed out".into(),
        ProviderError::Rejected(_) => "rejected by the provider".into(),
        ProviderError::Unknown(_) => "outcome unknown".into(),
    }
}

fn hhmmss(t: DateTime<Utc>) -> String {
    t.format("%H:%M:%S UTC").to_string()
}

/// Stable operator note for a live challenge (no countdowns: a changed note
/// is a new progress event).
fn note(c: &Challenge) -> String {
    match c.status {
        ChallengeStatus::Dispatched => format!(
            "waiting for the agent's gateway receipt (challenge {}, expires {})",
            c.challenge_id,
            hhmmss(c.expires_at)
        ),
        ChallengeStatus::DispatchFailed => format!(
            "readiness probe failed ({}); next challenge after {}",
            c.detail.as_deref().unwrap_or("dispatch failed"),
            hhmmss(c.expires_at)
        ),
        _ => format!(
            "readiness probe outcome unknown (challenge {}); its receipt is accepted until {}",
            c.challenge_id,
            hhmmss(c.expires_at)
        ),
    }
}

const EARLY_RUN_MISMATCH: &str = "gateway receipt named a different run than the dispatched probe";

fn ready(c: &Challenge) -> Readiness {
    Readiness::Ready {
        evidence_ref: format!("gateway-receipt:{}", c.challenge_id),
    }
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

    /// Every challenge for this agent, oldest first.
    fn history(&self, subject: &str) -> Result<Vec<Challenge>, ProviderError> {
        let conn = self.store.lock();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM readiness_challenges WHERE subject = ?1 ORDER BY attempt"
            ))
            .map_err(store_error)?;
        let rows = stmt
            .query_map(params![subject], row_to_challenge)
            .map_err(store_error)?
            .map(|r| r.map(|(c, _)| c))
            .collect::<Result<Vec<_>, _>>()
            .map_err(store_error);
        rows
    }

    fn reread(&self, challenge_id: &str) -> Result<Option<Challenge>, ProviderError> {
        get_challenge(&self.store, challenge_id).map_err(store_error)
    }

    /// Save a new challenge, then dispatch its probe once.
    async fn issue(
        &self,
        subject: &str,
        attempt: u32,
        failures: u32,
        role: &RoleToProvision,
        binding: &AgentBinding,
    ) -> Result<Readiness, ProviderError> {
        let challenge_id = format!("rc-{}", uuid::Uuid::new_v4().simple());
        // Two random UUIDv4s: 244 random bits.
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
        let dispatched = self.probe.dispatch(&request).await;
        drop(request); // the nonce is not needed after dispatch
                       // One write records the dispatch outcome: the trusted run (if any)
                       // and that the outcome is known, and moves a still-dispatching
                       // challenge on. A receipt that arrived first keeps its status.
        let (to, trusted, detail, retry_at): (_, Option<&str>, Option<String>, _) =
            match &dispatched {
                Ok(d) => (
                    ChallengeStatus::Dispatched,
                    d.probe_ref.as_deref(),
                    None,
                    None,
                ),
                // May have reached the provider: the challenge stays live until
                // expiry, and a receipt rests on the nonce alone.
                Err(e @ (ProviderError::Unknown(_) | ProviderError::Timeout)) => {
                    tracing::warn!(challenge = %challenge_id, error = %probe_error_kind(e), "readiness probe outcome unknown");
                    (
                        ChallengeStatus::Dispatching,
                        None,
                        Some("probe outcome unknown".into()),
                        None,
                    )
                }
                Err(e) => {
                    let kind = probe_error_kind(e);
                    tracing::warn!(challenge = %challenge_id, error = %kind, "readiness probe dispatch failed");
                    (
                        ChallengeStatus::DispatchFailed,
                        None,
                        Some(kind),
                        Some(Utc::now() + chrono(self.cfg.backoff(failures + 1))),
                    )
                }
            };
        resolve_dispatch(
            &self.store,
            &challenge_id,
            to,
            trusted,
            detail.as_deref(),
            retry_at,
        )
        .map_err(store_error)?;
        let current = self
            .reread(&challenge_id)?
            .ok_or_else(|| store_error("challenge vanished"))?;
        if current.status == ChallengeStatus::Received {
            // The agent answered before dispatch returned.
            return self.judge_receipt(&current);
        }
        Ok(Readiness::Waiting(note(&current)))
    }

    /// A received challenge is readiness only when its dispatch outcome is
    /// durably known and the receipt's run (if any) is the run dispatch
    /// reported (if any). A mismatch retires the challenge so a later pass
    /// issues a new one; an unresolved dispatch keeps waiting (and expires).
    /// Re-evaluated on every pass, so a failed write or a restart can never
    /// turn a provisional receipt into readiness.
    fn judge_receipt(&self, c: &Challenge) -> Result<Readiness, ProviderError> {
        if c.dispatch_resolved_at.is_none() {
            return Ok(Readiness::Waiting(format!(
                "receipt for challenge {} awaits its dispatch outcome",
                c.challenge_id
            )));
        }
        if consistent(c) {
            return Ok(ready(c));
        }
        tracing::warn!(challenge = %c.challenge_id, "readiness receipt named a different run than the dispatched probe");
        transition(
            &self.store,
            &c.challenge_id,
            &[ChallengeStatus::Received],
            ChallengeStatus::Expired,
            None,
            Some(EARLY_RUN_MISMATCH),
            None,
        )
        .map_err(store_error)?;
        Ok(Readiness::Waiting(format!(
            "{EARLY_RUN_MISMATCH} (challenge {}); a new challenge follows",
            c.challenge_id
        )))
    }
}

/// The receipt's run, if it named one, is the trusted dispatched run, if known.
fn consistent(c: &Challenge) -> bool {
    !matches!((&c.probe_ref, &c.receipt_probe_ref), (Some(t), Some(r)) if t != r)
}

/// Record dispatch's outcome in one statement (see `issue`).
fn resolve_dispatch(
    store: &Store,
    challenge_id: &str,
    to: ChallengeStatus,
    trusted: Option<&str>,
    detail: Option<&str>,
    retry_at: Option<DateTime<Utc>>,
) -> rusqlite::Result<()> {
    store.lock().execute(
        "UPDATE readiness_challenges SET probe_ref = COALESCE(?2, probe_ref), \
         dispatch_resolved_at = ?3, detail = COALESCE(?4, detail), \
         status = CASE WHEN status = 'dispatching' THEN ?5 ELSE status END, \
         expires_at = CASE WHEN status = 'dispatching' THEN COALESCE(?6, expires_at) ELSE expires_at END \
         WHERE challenge_id = ?1",
        params![challenge_id, trusted, ts(Utc::now()), detail, to.as_str(), retry_at.map(ts)],
    )?;
    Ok(())
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
        let history = self.history(&subject)?;
        let now = Utc::now();
        if let Some(c) = history.last() {
            match c.status {
                ChallengeStatus::Received if c.dispatch_resolved_at.is_some() => {
                    // Ready, or retired as a mismatch: then issue anew below.
                    if let r @ Readiness::Ready { .. } = self.judge_receipt(c)? {
                        return Ok(r);
                    }
                }
                ChallengeStatus::Received if c.expires_at > now => {
                    return self.judge_receipt(c); // waits for its dispatch outcome
                }
                ChallengeStatus::Received => {
                    // Its dispatch outcome was never recorded (crash or failed
                    // write): fail closed, retire it and issue anew below.
                    transition(
                        &self.store,
                        &c.challenge_id,
                        &[ChallengeStatus::Received],
                        ChallengeStatus::Expired,
                        None,
                        Some("receipt arrived but the dispatch outcome was never recorded"),
                        None,
                    )
                    .map_err(store_error)?;
                }
                _ if c.status != ChallengeStatus::Expired && c.expires_at > now => {
                    return Ok(Readiness::Waiting(note(c)))
                }
                ChallengeStatus::Dispatching | ChallengeStatus::Dispatched => {
                    // Expire it, unless a receipt was accepted meanwhile.
                    let expired = transition(
                        &self.store,
                        &c.challenge_id,
                        &[ChallengeStatus::Dispatching, ChallengeStatus::Dispatched],
                        ChallengeStatus::Expired,
                        None,
                        None,
                        None,
                    )
                    .map_err(store_error)?;
                    if !expired {
                        if let Some(r) = self.reread(&c.challenge_id)? {
                            if r.status == ChallengeStatus::Received {
                                return self.judge_receipt(&r);
                            }
                        }
                    }
                }
                _ => {} // a failed dispatch past its backoff, or already expired
            }
        }
        let reached = history
            .iter()
            .filter(|c| c.status != ChallengeStatus::DispatchFailed)
            .count() as u32;
        let failures = history.len() as u32 - reached;
        if reached >= self.cfg.max_attempts {
            return Ok(Readiness::NotReady(format!(
                "no gateway readiness receipt after {reached} challenge(s)"
            )));
        }
        if failures >= self.cfg.max_dispatch_failures {
            return Ok(Readiness::NotReady(format!(
                "readiness probe could not be dispatched after {failures} attempt(s)"
            )));
        }
        let attempt = history.last().map(|c| c.attempt + 1).unwrap_or(1);
        self.issue(&subject, attempt, failures, role, binding).await
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
         receipt_probe_ref = ?3 WHERE challenge_id = ?1 \
         AND status IN ('dispatching', 'dispatched')",
        params![c.challenge_id, ts(now), receipt.probe_ref],
    )?;
    if updated == 0 {
        return Ok(Err(ReceiptRejection::Expired));
    }
    tracing::info!(challenge = %c.challenge_id, role = %c.role_key, "readiness receipt accepted");
    Ok(Ok(ReceiptAccepted {
        challenge_id: c.challenge_id,
        duplicate: false,
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
