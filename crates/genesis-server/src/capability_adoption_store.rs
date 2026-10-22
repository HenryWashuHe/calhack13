//! Durable, requirement-bound discovery and independent local acceptance.
//! One explicit intent selects at most one foreign capability and one checking
//! batch. Count limits bound local work; they are not monetary reservations.

use crate::capability_authority::{authorize_current, CapabilityAuthorityError};
use crate::store::{Store, StoreError, StoreResult};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use genesis_core::capability::CapabilityManifest;
use genesis_core::engine::DispatchGrant;
use genesis_core::ids::*;
use genesis_core::lifecycle::{RequirementStatus, ResponsibilityStatus};
use genesis_core::model::{OrgState, Owner};
use genesis_verification::{
    decode_json, json_sha256, validate_artifact, validate_suite, ArtifactPin, OracleSuite,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_ADOPTIONS: usize = 32;
pub const MAX_ACTIVE_ADOPTIONS: usize = 4;
pub const MAX_ADOPTION_DURATION_SECONDS: i64 = 1800;
pub const MAX_DISCOVERY_ATTEMPTS: u32 = 3;
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_MANIFEST_BYTES: usize = 128 * 1024;
const MAX_QUERY_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAdoption {
    pub client_request_id: String,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub suite_id: String,
    pub provider_access_acknowledged: bool,
    pub deadline: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionPhase {
    Discovering,
    Selected,
    Checking,
    Accepted,
    Rejected,
    Attention,
    Fenced,
}
impl AdoptionPhase {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Discovering | Self::Selected | Self::Checking)
    }
    fn label(self) -> &'static str {
        match self {
            Self::Discovering => "discovering",
            Self::Selected => "selected",
            Self::Checking => "checking",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Attention => "attention",
            Self::Fenced => "fenced",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionAttention {
    RegistryUnavailable,
    NoCandidate,
    InvalidCandidate,
    ChecksFailed,
    ConfigurationChanged,
    ProviderUnavailable,
    AcceptanceConflict,
    InstallationConflict,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionFence {
    AuthorityChanged,
    RequirementChanged,
    DeadlineExpired,
    LeaseExpired,
}

#[derive(Debug, thiserror::Error)]
pub enum AdoptionError {
    #[error("invalid adoption intent")]
    InvalidInput,
    #[error("invalid adoption record")]
    InvalidRecord,
    #[error("adoption not found")]
    NotFound,
    #[error("adoption or organization changed concurrently")]
    Conflict,
    #[error("adoption idempotency key has a different intent")]
    IdempotencyConflict,
    #[error("an active adoption already holds this requirement and assignment")]
    ActiveConflict,
    #[error("adoption count limit reached")]
    Limit,
    #[error("adoption authority is fenced: {0:?}")]
    Authority(AdoptionFence),
    #[error("adoption storage failed")]
    Db(#[from] rusqlite::Error),
    #[error("cannot encode adoption state")]
    Encoding,
}
pub type Result<T> = std::result::Result<T, AdoptionError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionBinding {
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub grant: DispatchGrant,
    pub deadline: DateTime<Utc>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    requirement_sha256: String,
    mission_sha256: String,
    authority_sha256: String,
}

/// Backend-only record. HTTP handlers return `status()`, never this envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionRecord {
    schema_version: u32,
    id: String,
    client_request_id: String,
    intent_sha256: String,
    binding: AdoptionBinding,
    suite_id: String,
    suite_sha256: String,
    case_count: usize,
    query: String,
    version: u64,
    org_revision: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    next_attempt_at: Option<DateTime<Utc>>,
    discovery_attempts: u32,
    phase: AdoptionPhase,
    selection: Option<CapabilityManifest>,
    installation_id: Option<InstallationId>,
    job_ids: Vec<JobId>,
    acceptance_ref: Option<String>,
    rejection_ref: Option<String>,
    attention: Option<AdoptionAttention>,
    fence_reason: Option<AdoptionFence>,
    record_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SelectedCapability {
    pub capability_id: CapabilityId,
    pub version: String,
    pub publisher: OrgId,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdoptionStatus {
    pub id: String,
    pub version: u64,
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub assignment_id: AssignmentId,
    pub epoch: u64,
    pub suite_id: String,
    pub phase: AdoptionPhase,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub discovery_attempts: u32,
    pub selection: Option<SelectedCapability>,
    pub artifact: Option<ArtifactPin>,
    pub installation_id: Option<InstallationId>,
    pub job_ids: Vec<JobId>,
    pub acceptance_ref: Option<String>,
    pub rejection_ref: Option<String>,
    pub attention: Option<AdoptionAttention>,
    pub fence_reason: Option<AdoptionFence>,
    pub authority: &'static str,
}

impl AdoptionRecord {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn phase(&self) -> AdoptionPhase {
        self.phase
    }
    pub fn binding(&self) -> &AdoptionBinding {
        &self.binding
    }
    pub fn suite_id(&self) -> &str {
        &self.suite_id
    }
    pub fn suite_sha256(&self) -> &str {
        &self.suite_sha256
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn selection(&self) -> Option<&CapabilityManifest> {
        self.selection.as_ref()
    }
    pub fn installation_id(&self) -> Option<&InstallationId> {
        self.installation_id.as_ref()
    }
    pub fn job_ids(&self) -> &[JobId] {
        &self.job_ids
    }
    pub fn acceptance_ref(&self) -> Option<&str> {
        self.acceptance_ref.as_deref()
    }
    pub fn rejection_ref(&self) -> Option<&str> {
        self.rejection_ref.as_deref()
    }
    pub fn attention_code(&self) -> Option<AdoptionAttention> {
        self.attention
    }
    pub fn fence_reason(&self) -> Option<AdoptionFence> {
        self.fence_reason
    }
    pub fn next_attempt_at(&self) -> Option<DateTime<Utc>> {
        self.next_attempt_at
    }
    pub fn discovery_attempts(&self) -> u32 {
        self.discovery_attempts
    }
    pub fn org_revision(&self) -> u64 {
        self.org_revision
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn status(&self) -> AdoptionStatus {
        AdoptionStatus {
            id: self.id.clone(),
            version: self.version,
            org_id: self.binding.org_id.clone(),
            mission_id: self.binding.mission_id.clone(),
            requirement_id: self.binding.requirement_id.clone(),
            role_id: self.binding.role_id.clone(),
            responsibility_id: self.binding.responsibility_id.clone(),
            assignment_id: self.binding.grant.assignment_id.clone(),
            epoch: self.binding.grant.epoch,
            suite_id: self.suite_id.clone(),
            phase: self.phase,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deadline: self.binding.deadline,
            next_attempt_at: self.next_attempt_at,
            discovery_attempts: self.discovery_attempts,
            selection: self.selection.as_ref().map(|m| SelectedCapability {
                capability_id: m.capability_id.clone(),
                version: m.version.clone(),
                publisher: m.publisher.clone(),
            }),
            artifact: self.selection.as_ref().map(manifest_pin),
            installation_id: self.installation_id.clone(),
            job_ids: self.job_ids.clone(),
            acceptance_ref: self.acceptance_ref.clone(),
            rejection_ref: self.rejection_ref.clone(),
            attention: self.attention,
            fence_reason: self.fence_reason,
            authority:
                "observational: local acceptance does not attach capabilities or satisfy a mission",
        }
    }
    pub fn suite_matches(&self, suite: &OracleSuite) -> Result<()> {
        validate_suite(suite).map_err(|_| AdoptionError::InvalidInput)?;
        if self.suite_id != suite.suite_id
            || self.suite_sha256 != suite_digest(suite)?
            || self.case_count != suite.cases.len()
        {
            return Err(AdoptionError::InvalidRecord);
        }
        Ok(())
    }
    pub fn authorize(&self, state: &OrgState, now: DateTime<Utc>) -> Result<()> {
        self.validate()?;
        if let Some(fence) = self.fence_reason {
            return Err(AdoptionError::Authority(fence));
        }
        let b = &self.binding;
        if b.lease_expires_at.is_some_and(|until| until <= now) {
            return Err(AdoptionError::Authority(AdoptionFence::LeaseExpired));
        }
        if b.deadline <= now {
            return Err(AdoptionError::Authority(AdoptionFence::DeadlineExpired));
        }
        if b.org_id != state.org_id {
            return Err(AdoptionError::Authority(AdoptionFence::AuthorityChanged));
        }
        let current = authority_snapshot(
            state,
            &b.mission_id,
            &b.requirement_id,
            &b.role_id,
            &b.responsibility_id,
            now,
        )?;
        if b.requirement_sha256 != current.requirement_sha256 {
            return Err(AdoptionError::Authority(AdoptionFence::RequirementChanged));
        }
        if b.grant != current.grant
            || b.authority_sha256 != current.authority_sha256
            || b.mission_sha256 != current.mission_sha256
            || b.lease_expires_at != current.lease_expires_at
        {
            return Err(AdoptionError::Authority(AdoptionFence::AuthorityChanged));
        }
        if let Some(manifest) = &self.selection {
            manifest
                .check_compatibility(&state.constitution)
                .map_err(|_| AdoptionError::Authority(AdoptionFence::AuthorityChanged))?;
            for scope in &manifest.required_permissions {
                authorize_current(state, &b.role_id, &b.responsibility_id, Some(scope), now)
                    .map_err(authority_error)?;
            }
        }
        Ok(())
    }
    /// Bind a prepared mutation to the new snapshot revision that commit_state
    /// inserts before its transaction callback. This does not change version.
    pub fn for_revision(mut self, revision: u64) -> Result<Self> {
        self.org_revision = revision;
        self.seal()?;
        self.validate()?;
        Ok(self)
    }
    fn next(&self, now: DateTime<Utc>) -> Result<Self> {
        self.validate()?;
        if now < self.updated_at {
            return Err(AdoptionError::InvalidInput);
        }
        let mut next = self.clone();
        next.version = next
            .version
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or(AdoptionError::InvalidRecord)?;
        next.updated_at = now;
        Ok(next)
    }
    fn finish(mut self) -> Result<Self> {
        self.seal()?;
        self.validate()?;
        Ok(self)
    }
    /// Commit this claim before registry reads. Cancellation consumes its count;
    /// restarts may make at most three bounded discovery attempts in total.
    pub fn begin_discovery(&self, now: DateTime<Utc>) -> Result<Self> {
        if self.phase != AdoptionPhase::Discovering
            || self.discovery_attempts >= MAX_DISCOVERY_ATTEMPTS
        {
            return Err(AdoptionError::Limit);
        }
        let mut next = self.next(now)?;
        next.discovery_attempts += 1;
        next.next_attempt_at = Some((now + Duration::seconds(1)).min(self.binding.deadline));
        next.finish()
    }
    pub fn select(
        &self,
        manifest: CapabilityManifest,
        installation_id: InstallationId,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        if self.phase != AdoptionPhase::Discovering || self.discovery_attempts == 0 {
            return Err(AdoptionError::InvalidRecord);
        }
        let mut next = self.next(now)?;
        next.phase = AdoptionPhase::Selected;
        next.selection = Some(manifest);
        next.installation_id = Some(installation_id);
        next.next_attempt_at = Some(now);
        next.finish()
    }
    pub fn checking(&self, job_ids: Vec<JobId>, now: DateTime<Utc>) -> Result<Self> {
        if self.phase != AdoptionPhase::Selected {
            return Err(AdoptionError::InvalidRecord);
        }
        let mut next = self.next(now)?;
        next.phase = AdoptionPhase::Checking;
        next.job_ids = job_ids;
        next.next_attempt_at = Some(now);
        next.finish()
    }
    pub fn accept(&self, reference: String, now: DateTime<Utc>) -> Result<Self> {
        if self.phase != AdoptionPhase::Checking {
            return Err(AdoptionError::InvalidRecord);
        }
        let mut next = self.next(now)?;
        next.phase = AdoptionPhase::Accepted;
        next.acceptance_ref = Some(reference);
        next.next_attempt_at = None;
        next.finish()
    }
    pub fn rejected(&self, reference: String, now: DateTime<Utc>) -> Result<Self> {
        if self.phase != AdoptionPhase::Checking {
            return Err(AdoptionError::InvalidRecord);
        }
        let mut next = self.next(now)?;
        next.phase = AdoptionPhase::Rejected;
        next.rejection_ref = Some(reference);
        next.next_attempt_at = None;
        next.finish()
    }
    pub fn attention(&self, code: AdoptionAttention, now: DateTime<Utc>) -> Result<Self> {
        if !self.phase.is_active() {
            return Err(AdoptionError::InvalidRecord);
        }
        let mut next = self.next(now)?;
        next.phase = AdoptionPhase::Attention;
        next.attention = Some(code);
        next.next_attempt_at = None;
        next.finish()
    }
    pub fn fence(&self, reason: AdoptionFence, now: DateTime<Utc>) -> Result<Self> {
        if !self.phase.is_active() && self.phase != AdoptionPhase::Attention {
            return Err(AdoptionError::InvalidRecord);
        }
        let mut next = self.next(now)?;
        next.phase = AdoptionPhase::Fenced;
        next.attention = None;
        next.fence_reason = Some(reason);
        next.next_attempt_at = None;
        next.finish()
    }
    pub fn wait_until(&self, due: DateTime<Utc>, now: DateTime<Utc>) -> Result<Self> {
        if !self.phase.is_active()
            || due < now
            || due > now + Duration::seconds(60)
            || due > self.binding.deadline
        {
            return Err(AdoptionError::InvalidInput);
        }
        let mut next = self.next(now)?;
        next.next_attempt_at = Some(due);
        next.finish()
    }
    fn immutable_digest(&self) -> Result<String> {
        hash(&(
            &self.id,
            &self.client_request_id,
            &self.intent_sha256,
            &self.binding,
            &self.suite_id,
            &self.suite_sha256,
            self.case_count,
            &self.query,
            self.created_at,
        ))
    }
    fn digest(&self) -> Result<String> {
        hash(&(
            "genesis-capability-adoption-v1",
            self.schema_version,
            self.immutable_digest()?,
            self.version,
            self.org_revision,
            self.updated_at,
            self.next_attempt_at,
            self.discovery_attempts,
            self.phase,
            &self.selection,
            &self.installation_id,
            &self.job_ids,
            &self.acceptance_ref,
            &self.rejection_ref,
            self.attention,
            self.fence_reason,
        ))
    }
    fn seal(&mut self) -> Result<()> {
        self.record_sha256 = self.digest()?;
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        let b = &self.binding;
        if self.schema_version != 1
            || !safe_id(&self.id)
            || !domain_id(&self.client_request_id)
            || [
                &b.org_id.to_string(),
                &b.mission_id.to_string(),
                &b.requirement_id.to_string(),
                &b.role_id.to_string(),
                &b.responsibility_id.to_string(),
                &b.grant.assignment_id.to_string(),
                &self.suite_id,
            ]
            .into_iter()
            .any(|id| !domain_id(id))
            || b.grant.role_id != b.role_id
            || b.grant.responsibility_id != b.responsibility_id
            || b.grant.epoch > i64::MAX as u64
            || self.version == 0
            || self.version > i64::MAX as u64
            || self.org_revision > i64::MAX as u64
            || self.created_at > self.updated_at
            || b.deadline <= self.created_at
            || b.deadline > self.created_at + Duration::seconds(MAX_ADOPTION_DURATION_SECONDS)
            || b.lease_expires_at.is_some_and(|lease| b.deadline > lease)
            || self.query.is_empty()
            || self.query.len() > MAX_QUERY_BYTES
            || !(1..=128).contains(&self.case_count)
            || self.discovery_attempts > MAX_DISCOVERY_ATTEMPTS
            || (self.selection.is_some() && self.discovery_attempts == 0)
            || [
                &self.intent_sha256,
                &self.suite_sha256,
                &b.requirement_sha256,
                &b.mission_sha256,
                &b.authority_sha256,
            ]
            .into_iter()
            .any(|digest| !digest_valid(digest))
            || self.phase.is_active() != self.next_attempt_at.is_some()
            || self.next_attempt_at.is_some_and(|due| {
                due < self.updated_at
                    || due > self.updated_at + Duration::seconds(60)
                    || due > b.deadline
            })
            || self.selection.is_some() != self.installation_id.is_some()
            || self
                .installation_id
                .as_ref()
                .is_some_and(|id| !domain_id(id.as_str()))
            || self.job_ids.len() > self.case_count
            || self.job_ids.iter().any(|id| !domain_id(id.as_str()))
            || self.job_ids.iter().collect::<BTreeSet<_>>().len() != self.job_ids.len()
            || (!self.job_ids.is_empty()
                && (self.selection.is_none() || self.job_ids.len() != self.case_count))
            || self
                .acceptance_ref
                .as_ref()
                .is_some_and(|reference| !proof_ref(reference, "verification:"))
            || self
                .rejection_ref
                .as_ref()
                .is_some_and(|reference| !proof_ref(reference, "adoption-rejection:"))
            || (self.phase == AdoptionPhase::Accepted) != self.acceptance_ref.is_some()
            || (self.phase == AdoptionPhase::Rejected) != self.rejection_ref.is_some()
            || (self.phase == AdoptionPhase::Attention) != self.attention.is_some()
            || (self.phase == AdoptionPhase::Fenced) != self.fence_reason.is_some()
            || (self.phase == AdoptionPhase::Discovering && self.selection.is_some())
            || (self.phase == AdoptionPhase::Selected
                && (self.selection.is_none() || !self.job_ids.is_empty()))
            || (matches!(
                self.phase,
                AdoptionPhase::Checking | AdoptionPhase::Accepted | AdoptionPhase::Rejected
            ) && (self.selection.is_none() || self.job_ids.len() != self.case_count))
        {
            return Err(AdoptionError::InvalidRecord);
        }
        if let Some(manifest) = &self.selection {
            if manifest.publisher == b.org_id
                || !domain_id(manifest.publisher.as_str())
                || manifest.check_publishable().is_err()
                || validate_artifact(&manifest_pin(manifest)).is_err()
                || encoded(manifest)?.len() > MAX_MANIFEST_BYTES
            {
                return Err(AdoptionError::InvalidRecord);
            }
        }
        if self.record_sha256 != self.digest()? || encoded(self)?.len() > MAX_RECORD_BYTES {
            return Err(AdoptionError::InvalidRecord);
        }
        Ok(())
    }
}

fn manifest_pin(manifest: &CapabilityManifest) -> ArtifactPin {
    ArtifactPin {
        capability_id: manifest.capability_id.to_string(),
        capability_version: manifest.version.clone(),
        source_sha256: manifest.source_digest.clone(),
        actor_id: manifest.artifact.actor_id.clone(),
        build_id: manifest.artifact.build_id.clone(),
        build_number: manifest.artifact.build_number.clone(),
    }
}
fn domain_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}
fn safe_id(value: &str) -> bool {
    domain_id(value)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn proof_ref(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(digest_valid)
}
fn encoded<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| AdoptionError::Encoding)
}
fn hash<T: Serialize>(value: &T) -> Result<String> {
    let json = encoded(value)?;
    if json.len() > MAX_RECORD_BYTES {
        return Err(AdoptionError::InvalidRecord);
    }
    let value: serde_json::Value =
        decode_json(json.as_bytes()).map_err(|_| AdoptionError::InvalidRecord)?;
    Ok(json_sha256(&value))
}
// validate_suite bounds case count and every source/input/expected field. Keep
// its full canonical digest without retaining the oracle in the adoption row.
fn suite_digest(suite: &OracleSuite) -> Result<String> {
    serde_json::to_value(suite)
        .map(|value| json_sha256(&value))
        .map_err(|_| AdoptionError::Encoding)
}
fn stamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
fn authority_error(error: CapabilityAuthorityError) -> AdoptionError {
    AdoptionError::Authority(match error {
        CapabilityAuthorityError::LeaseExpired => AdoptionFence::LeaseExpired,
        CapabilityAuthorityError::MissionExpired | CapabilityAuthorityError::JobExpired => {
            AdoptionFence::DeadlineExpired
        }
        _ => AdoptionFence::AuthorityChanged,
    })
}

struct AuthoritySnapshot {
    grant: DispatchGrant,
    requirement_sha256: String,
    mission_sha256: String,
    authority_sha256: String,
    lease_expires_at: Option<DateTime<Utc>>,
}
fn authority_snapshot(
    state: &OrgState,
    mission_id: &MissionId,
    requirement_id: &RequirementId,
    role_id: &RoleId,
    responsibility_id: &ResponsibilityId,
    now: DateTime<Utc>,
) -> Result<AuthoritySnapshot> {
    let fenced = || AdoptionError::Authority(AdoptionFence::RequirementChanged);
    let grant =
        authorize_current(state, role_id, responsibility_id, None, now).map_err(authority_error)?;
    let requirement = state.requirements.get(requirement_id).ok_or_else(fenced)?;
    let responsibility = state
        .responsibilities
        .get(responsibility_id)
        .ok_or_else(fenced)?;
    let mission = state.missions.get(mission_id).ok_or_else(fenced)?;
    if requirement.requirement_id != *requirement_id
        || mission.mission_id != *mission_id
        || responsibility.responsibility_id != *responsibility_id
        || requirement.mission_id != *mission_id
        || responsibility.mission_id != *mission_id
        || responsibility.requirement_id.as_ref() != Some(requirement_id)
        || requirement.status != RequirementStatus::Assigned
        || responsibility.status != ResponsibilityStatus::Open
        || requirement.owner != responsibility.owner
        || matches!(requirement.owner, Owner::MissionController)
        || requirement.summary.is_empty()
        || requirement.summary.len() > MAX_QUERY_BYTES
        || requirement.triggering_evidence.is_empty()
        || requirement.triggering_evidence.len() > 128
    {
        return Err(fenced());
    }
    let observations = requirement
        .triggering_evidence
        .iter()
        .map(|id| {
            state
                .observations
                .get(id)
                .filter(|observation| {
                    observation.observation_id == *id && observation.mission_id == *mission_id
                })
                .ok_or_else(fenced)
        })
        .collect::<Result<Vec<_>>>()?;
    let assignment = state
        .assignments
        .get(&grant.assignment_id)
        .ok_or_else(fenced)?;
    let role = state.roles.get(role_id).ok_or_else(fenced)?;
    Ok(AuthoritySnapshot {
        requirement_sha256: hash(&(requirement, &observations))?,
        mission_sha256: hash(mission)?,
        authority_sha256: hash(&(
            &grant,
            assignment,
            &role.authority,
            responsibility,
            &state.constitution.scopes,
        ))?,
        grant,
        lease_expires_at: assignment.lease_expires_at,
    })
}

fn state(conn: &Connection) -> Result<OrgState> {
    let json: String = conn.query_row(
        "SELECT state_json FROM org_snapshots ORDER BY revision DESC LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    decode_json(json.as_bytes()).map_err(|_| AdoptionError::InvalidRecord)
}
struct StoredRecord {
    json: Option<String>,
    version: u64,
    phase: Option<String>,
    active: bool,
    next: Option<String>,
    client_request_id: Option<String>,
    requirement_id: Option<String>,
    assignment_id: Option<String>,
    epoch: u64,
    installation_id: Option<String>,
}
fn load(conn: &Connection, id: &str) -> Result<AdoptionRecord> {
    let row:Option<StoredRecord>=conn.query_row("SELECT CASE WHEN length(CAST(record_json AS BLOB))<=?2 THEN record_json END,version,CASE WHEN length(CAST(phase AS BLOB))<=32 THEN phase END,active,CASE WHEN next_attempt_at IS NULL OR length(CAST(next_attempt_at AS BLOB))<=64 THEN next_attempt_at ELSE '' END,CASE WHEN length(CAST(client_request_id AS BLOB))<=256 THEN client_request_id END,CASE WHEN length(CAST(requirement_id AS BLOB))<=256 THEN requirement_id END,CASE WHEN length(CAST(assignment_id AS BLOB))<=256 THEN assignment_id END,epoch,CASE WHEN installation_id IS NULL OR length(CAST(installation_id AS BLOB))<=256 THEN installation_id ELSE '' END FROM capability_adoptions WHERE id=?1",params![id,MAX_RECORD_BYTES as i64],|row|Ok(StoredRecord{json:row.get(0)?,version:row.get(1)?,phase:row.get(2)?,active:row.get(3)?,next:row.get(4)?,client_request_id:row.get(5)?,requirement_id:row.get(6)?,assignment_id:row.get(7)?,epoch:row.get(8)?,installation_id:row.get(9)?})).optional()?;
    let row = row.ok_or(AdoptionError::NotFound)?;
    let record: AdoptionRecord =
        decode_json(row.json.ok_or(AdoptionError::InvalidRecord)?.as_bytes())
            .map_err(|_| AdoptionError::InvalidRecord)?;
    record.validate()?;
    let org_id:Option<String>=conn.query_row("SELECT CASE WHEN json_type(state_json,'$.org_id')='text' AND length(CAST(json_extract(state_json,'$.org_id') AS BLOB)) BETWEEN 1 AND 256 THEN json_extract(state_json,'$.org_id') END FROM org_snapshots ORDER BY revision DESC LIMIT 1",[],|row|row.get(0))?;
    if org_id.as_deref() != Some(record.binding.org_id.as_str()) {
        return Err(AdoptionError::InvalidRecord);
    }
    if record.id != id
        || record.version != row.version
        || Some(record.phase.label()) != row.phase.as_deref()
        || record.phase.is_active() != row.active
        || record.next_attempt_at.map(stamp) != row.next
        || Some(record.client_request_id.as_str()) != row.client_request_id.as_deref()
        || Some(record.binding.requirement_id.as_str()) != row.requirement_id.as_deref()
        || Some(record.binding.grant.assignment_id.as_str()) != row.assignment_id.as_deref()
        || record.binding.grant.epoch != row.epoch
        || record.installation_id.as_ref().map(|id| id.as_str()) != row.installation_id.as_deref()
    {
        return Err(AdoptionError::InvalidRecord);
    }
    Ok(record)
}

impl Store {
    pub fn init_capability_adoptions(&self) -> Result<()> {
        self.lock().execute_batch("CREATE TABLE IF NOT EXISTS capability_adoptions(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,client_request_id TEXT NOT NULL UNIQUE,requirement_id TEXT NOT NULL,assignment_id TEXT NOT NULL,epoch INTEGER NOT NULL,version INTEGER NOT NULL,phase TEXT NOT NULL,active INTEGER NOT NULL CHECK(active IN(0,1)),next_attempt_at TEXT,installation_id TEXT UNIQUE,record_json TEXT NOT NULL);CREATE UNIQUE INDEX IF NOT EXISTS adoption_active_requirement ON capability_adoptions(requirement_id,assignment_id,epoch) WHERE active=1;CREATE INDEX IF NOT EXISTS adoption_due ON capability_adoptions(active,next_attempt_at,ordinal);")?;
        Ok(())
    }
    pub fn create_adoption(
        &self,
        intent: &CreateAdoption,
        suite: &OracleSuite,
        now: DateTime<Utc>,
    ) -> Result<(AdoptionRecord, bool)> {
        if !intent.provider_access_acknowledged
            || [
                &intent.client_request_id,
                &intent.suite_id,
                &intent.mission_id.to_string(),
                &intent.requirement_id.to_string(),
                &intent.role_id.to_string(),
                &intent.responsibility_id.to_string(),
            ]
            .into_iter()
            .any(|id| !domain_id(id))
            || intent.suite_id != suite.suite_id
        {
            return Err(AdoptionError::InvalidInput);
        }
        validate_suite(suite).map_err(|_| AdoptionError::InvalidInput)?;
        let digest = hash(intent)?;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(id)=tx.query_row("SELECT CASE WHEN length(CAST(id AS BLOB))<=256 THEN id END FROM capability_adoptions WHERE client_request_id=?1",params![intent.client_request_id],|row|row.get::<_,Option<String>>(0)).optional()? {let id=id.ok_or(AdoptionError::InvalidRecord)?;let previous=load(&tx,&id)?;if previous.intent_sha256!=digest{return Err(AdoptionError::IdempotencyConflict);}return Ok((previous,false));}
        let state = state(&tx)?;
        let authority = authority_snapshot(
            &state,
            &intent.mission_id,
            &intent.requirement_id,
            &intent.role_id,
            &intent.responsibility_id,
            now,
        )?;
        let mut deadline = now + Duration::seconds(MAX_ADOPTION_DURATION_SECONDS);
        for bound in [
            intent.deadline,
            state.missions[&intent.mission_id].deadline,
            authority.lease_expires_at,
        ]
        .into_iter()
        .flatten()
        {
            deadline = deadline.min(bound);
        }
        if deadline <= now {
            return Err(AdoptionError::Authority(AdoptionFence::DeadlineExpired));
        }
        let total: usize =
            tx.query_row("SELECT COUNT(*) FROM capability_adoptions", [], |row| {
                row.get(0)
            })?;
        let active: usize = tx.query_row(
            "SELECT COUNT(*) FROM capability_adoptions WHERE active=1",
            [],
            |row| row.get(0),
        )?;
        let occupied:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM capability_adoptions WHERE requirement_id=?1 AND assignment_id=?2 AND epoch=?3 AND active=1)",params![intent.requirement_id.as_str(),authority.grant.assignment_id.as_str(),authority.grant.epoch as i64],|row|row.get(0))?;
        if occupied {
            return Err(AdoptionError::ActiveConflict);
        }
        if total >= MAX_ADOPTIONS || active >= MAX_ACTIVE_ADOPTIONS {
            return Err(AdoptionError::Limit);
        }
        let mut record = AdoptionRecord {
            schema_version: 1,
            id: format!("adopt_{}", uuid::Uuid::new_v4().simple()),
            client_request_id: intent.client_request_id.clone(),
            intent_sha256: digest,
            binding: AdoptionBinding {
                org_id: state.org_id.clone(),
                mission_id: intent.mission_id.clone(),
                requirement_id: intent.requirement_id.clone(),
                role_id: intent.role_id.clone(),
                responsibility_id: intent.responsibility_id.clone(),
                grant: authority.grant,
                deadline,
                lease_expires_at: authority.lease_expires_at,
                requirement_sha256: authority.requirement_sha256,
                mission_sha256: authority.mission_sha256,
                authority_sha256: authority.authority_sha256,
            },
            suite_id: suite.suite_id.clone(),
            suite_sha256: suite_digest(suite)?,
            case_count: suite.cases.len(),
            query: state.requirements[&intent.requirement_id].summary.clone(),
            version: 1,
            org_revision: state.revision,
            created_at: now,
            updated_at: now,
            next_attempt_at: Some(now),
            discovery_attempts: 0,
            phase: AdoptionPhase::Discovering,
            selection: None,
            installation_id: None,
            job_ids: vec![],
            acceptance_ref: None,
            rejection_ref: None,
            attention: None,
            fence_reason: None,
            record_sha256: String::new(),
        };
        record.seal()?;
        record.validate()?;
        tx.execute("INSERT INTO capability_adoptions(id,client_request_id,requirement_id,assignment_id,epoch,version,phase,active,next_attempt_at,record_json) VALUES(?1,?2,?3,?4,?5,1,'discovering',1,?6,?7)",params![record.id,record.client_request_id,record.binding.requirement_id.as_str(),record.binding.grant.assignment_id.as_str(),record.binding.grant.epoch as i64,stamp(now),encoded(&record)?])?;
        tx.commit()?;
        Ok((record, true))
    }
    pub fn get_adoption(&self, id: &str) -> Result<AdoptionRecord> {
        if !safe_id(id) {
            return Err(AdoptionError::InvalidInput);
        }
        load(&self.lock(), id)
    }
    /// An adoption owns its selected installation even after it terminates.
    /// Ordinary acceptance routes must not replace its frozen checking batch.
    pub fn adoption_owns_installation(&self, installation_id: &InstallationId) -> Result<bool> {
        if !domain_id(installation_id.as_str()) {
            return Err(AdoptionError::InvalidInput);
        }
        let conn = self.lock();
        let id:Option<Option<String>>=conn.query_row("SELECT CASE WHEN length(CAST(id AS BLOB))<=256 THEN id END FROM capability_adoptions WHERE installation_id=?1",params![installation_id.as_str()],|row|row.get(0)).optional()?;
        let Some(id) = id else {
            return Ok(false);
        };
        let record = load(&conn, &id.ok_or(AdoptionError::InvalidRecord)?)?;
        if record.installation_id.as_ref() != Some(installation_id) {
            return Err(AdoptionError::InvalidRecord);
        }
        Ok(true)
    }
    pub fn due_adoption_ids(&self, now: DateTime<Utc>, limit: usize) -> Result<Vec<String>> {
        if !(1..=16).contains(&limit) {
            return Err(AdoptionError::InvalidInput);
        }
        let conn = self.lock();
        let mut stmt=conn.prepare("SELECT CASE WHEN length(CAST(id AS BLOB))<=256 THEN id END FROM capability_adoptions WHERE active=1 AND next_attempt_at<=?1 ORDER BY next_attempt_at,ordinal LIMIT ?2")?;
        let rows = stmt.query_map(params![stamp(now), limit as i64], |row| {
            row.get::<_, Option<String>>(0)
        })?;
        let ids = rows
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|id| id.ok_or(AdoptionError::InvalidRecord))
            .collect::<Result<Vec<_>>>()?;
        if ids.iter().any(|id| !safe_id(id)) {
            return Err(AdoptionError::InvalidRecord);
        }
        Ok(ids)
    }
    pub fn list_adoptions(&self, after_ordinal: u64, limit: usize) -> Result<Vec<AdoptionRecord>> {
        if after_ordinal > i64::MAX as u64 || !(1..=MAX_ADOPTIONS).contains(&limit) {
            return Err(AdoptionError::InvalidInput);
        }
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let mut stmt=tx.prepare("SELECT CASE WHEN length(CAST(id AS BLOB))<=256 THEN id END FROM capability_adoptions WHERE ordinal>?1 ORDER BY ordinal LIMIT ?2")?;
        let ids = stmt
            .query_map(params![after_ordinal as i64, limit as i64], |row| {
                row.get::<_, Option<String>>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|id| id.ok_or(AdoptionError::InvalidRecord))
            .collect::<Result<Vec<_>>>()?;
        ids.iter().map(|id| load(&tx, id)).collect()
    }
    pub fn save_adoption(
        &self,
        expected: &AdoptionRecord,
        next: &AdoptionRecord,
    ) -> Result<AdoptionRecord> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        save(&tx, expected, next)?;
        tx.commit()?;
        Ok(next.clone())
    }
}

fn save(tx: &Transaction<'_>, expected: &AdoptionRecord, next: &AdoptionRecord) -> Result<()> {
    expected.validate()?;
    next.validate()?;
    let saved = load(tx, expected.id())?;
    if saved.record_sha256 != expected.record_sha256
        || next.version != expected.version + 1
        || expected.immutable_digest()? != next.immutable_digest()?
    {
        return Err(AdoptionError::Conflict);
    }
    let current = state(tx)?;
    if current.revision != next.org_revision {
        return Err(AdoptionError::Conflict);
    }
    if next.updated_at < expected.updated_at || next.org_revision < expected.org_revision {
        return Err(AdoptionError::InvalidRecord);
    }
    let allowed = match (expected.phase, next.phase) {
        (AdoptionPhase::Discovering, AdoptionPhase::Selected)
        | (AdoptionPhase::Selected, AdoptionPhase::Checking)
        | (AdoptionPhase::Checking, AdoptionPhase::Accepted | AdoptionPhase::Rejected) => true,
        (before, AdoptionPhase::Attention) => before.is_active(),
        (before, AdoptionPhase::Fenced) => before.is_active() || before == AdoptionPhase::Attention,
        (before, after) => before.is_active() && before == after,
    };
    if next.discovery_attempts < expected.discovery_attempts
        || next.discovery_attempts > expected.discovery_attempts + 1
        || (next.discovery_attempts != expected.discovery_attempts
            && (expected.phase != AdoptionPhase::Discovering
                || next.phase != AdoptionPhase::Discovering))
    {
        return Err(AdoptionError::InvalidRecord);
    }
    if !allowed
        || expected
            .selection
            .as_ref()
            .is_some_and(|selection| next.selection.as_ref() != Some(selection))
        || expected
            .installation_id
            .as_ref()
            .is_some_and(|id| next.installation_id.as_ref() != Some(id))
        || (!expected.job_ids.is_empty() && expected.job_ids != next.job_ids)
    {
        return Err(AdoptionError::InvalidRecord);
    }
    if !matches!(next.phase, AdoptionPhase::Attention | AdoptionPhase::Fenced) {
        next.authorize(&current, next.updated_at)?;
    }
    if tx.execute("UPDATE capability_adoptions SET version=?1,phase=?2,active=?3,next_attempt_at=?4,record_json=?5,installation_id=?8 WHERE id=?6 AND version=?7",params![next.version as i64,next.phase.label(),next.phase.is_active(),next.next_attempt_at.map(stamp),encoded(next)?,next.id,expected.version as i64,next.installation_id.as_ref().map(|id|id.as_str())])?!=1{return Err(AdoptionError::Conflict);}
    Ok(())
}

/// Use inside commit_state's callback, after the new organization revision has
/// been inserted. Any error rolls back installation/jobs/acceptance with adoption.
pub(crate) fn save_in_tx(
    tx: &Transaction<'_>,
    expected: &AdoptionRecord,
    next: &AdoptionRecord,
) -> StoreResult<()> {
    save(tx, expected, next).map_err(|error| match error {
        AdoptionError::Db(error) => StoreError::Db(error),
        _ => StoreError::Db(rusqlite::Error::InvalidParameterName(
            "adoption transaction binding failed".into(),
        )),
    })
}

#[cfg(test)]
mod tests;
