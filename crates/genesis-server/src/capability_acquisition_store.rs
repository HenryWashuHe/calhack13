//! Trusted organization-owned acquisition records and persistent count quotas.
//! These are effect-count limits, not dollar billing guarantees. Provider
//! outcomes never refund consumed counts, including interrupted/unknown POSTs.
use crate::capability_acquisition_history::{self as history, CheckpointKind, HistoryError};
use crate::store::Store;
use chrono::{DateTime, Duration, Utc};
use genesis_core::engine::{authorize_dispatch, DispatchGrant};
use genesis_core::ids::*;
use genesis_core::lifecycle::{MissionStatus, RequirementStatus, ResponsibilityStatus};
use genesis_core::model::{OrgState, Owner};
use genesis_forge::{
    AcquisitionPhase, AcquisitionPlan, AcquisitionSession, ActionKind, CandidateIdentity,
    ModelConfig, PublicExample, RepairIdentity, SynthesisPhase, SynthesisRequest,
};
use genesis_verification::{decode_json, json_sha256, OracleSuite};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

const MAX_RECORD_BYTES: usize = 9 * 1024 * 1024;
const ACQUIRE_SCOPE: &str = "capability.acquire";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionQuotas {
    pub total_intents: u64,
    pub active: u64,
    pub model_calls: u64,
    pub actor_creates: u64,
    pub builds: u64,
    pub test_submits: u64,
    pub read_actions: u64,
}
impl Default for AcquisitionQuotas {
    fn default() -> Self {
        Self {
            total_intents: 32,
            active: 4,
            model_calls: 64,
            actor_creates: 64,
            builds: 64,
            test_submits: 512,
            read_actions: 2048,
        }
    }
}
impl AcquisitionQuotas {
    fn validate(&self) -> Result<()> {
        if [
            self.total_intents,
            self.active,
            self.model_calls,
            self.actor_creates,
            self.builds,
            self.test_submits,
            self.read_actions,
        ]
        .into_iter()
        .any(|n| n == 0 || n > 1_000_000)
            || self.active > self.total_intents
        {
            return Err(AcquisitionStoreError::InvalidPolicy);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionPolicy {
    pub model: ModelConfig,
    pub quotas: AcquisitionQuotas,
    pub max_duration_seconds: u64,
}
impl AcquisitionPolicy {
    pub fn validate(&self) -> Result<()> {
        self.model
            .validate()
            .map_err(|_| AcquisitionStoreError::InvalidPolicy)?;
        self.quotas.validate()?;
        if !(1..=86_400).contains(&self.max_duration_seconds) {
            return Err(AcquisitionStoreError::InvalidPolicy);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAcquisition {
    pub client_request_id: String,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub suite_id: String,
    pub candidate: CandidateIdentity,
    pub repair: Option<CandidateIdentity>,
    pub allowed_source_urls: Vec<String>,
    pub examples: Vec<PublicExample>,
    pub deadline: Option<DateTime<Utc>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionBinding {
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub grant: DispatchGrant,
    pub deadline: DateTime<Utc>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub requirement_sha256: String,
    pub authority_sha256: String,
    pub mission_sha256: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionFence {
    AuthorityChanged,
    RequirementChanged,
    DeadlineExpired,
    LeaseExpired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionDebit {
    ModelCall,
    ActorCreate,
    Build,
    TestSubmit,
    Read,
}
impl From<ActionKind> for AcquisitionDebit {
    fn from(action: ActionKind) -> Self {
        match action {
            ActionKind::CreateActor => Self::ActorCreate,
            ActionKind::StartBuild => Self::Build,
            ActionKind::SubmitCase => Self::TestSubmit,
            ActionKind::PollBuild | ActionKind::CollectCase => Self::Read,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionCounters {
    pub total_intents: u64,
    pub active: u64,
    pub model_calls: u64,
    pub actor_creates: u64,
    pub builds: u64,
    pub test_submits: u64,
    pub read_actions: u64,
}
#[derive(Debug, thiserror::Error)]
pub enum AcquisitionStoreError {
    #[error("invalid acquisition request")]
    InvalidInput,
    #[error("invalid acquisition record")]
    InvalidRecord,
    #[error("invalid trusted acquisition policy")]
    InvalidPolicy,
    #[error("organization acquisition quotas are immutable; changing them requires an explicit migration")]
    PolicyChanged,
    #[error("acquisition record not found")]
    NotFound,
    #[error("acquisition or organization changed concurrently")]
    Conflict,
    #[error("acquisition idempotency key is bound to a different request")]
    IdempotencyConflict,
    #[error("an active acquisition already holds this requirement and assignment")]
    ActiveConflict,
    #[error("acquisition count quota exhausted: {0}")]
    QuotaExceeded(&'static str),
    #[error("acquisition authority is fenced: {0:?}")]
    Authority(AcquisitionFence),
    #[error("acquisition database operation failed")]
    Db(#[from] rusqlite::Error),
    #[error("cannot encode acquisition state")]
    Encoding,
}
pub type Result<T> = std::result::Result<T, AcquisitionStoreError>;

impl From<HistoryError> for AcquisitionStoreError {
    fn from(error: HistoryError) -> Self {
        match error {
            HistoryError::Db(error) => Self::Db(error),
            HistoryError::Encoding => Self::Encoding,
            _ => Self::InvalidRecord,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionRecord {
    schema_version: u32,
    id: String,
    client_request_id: String,
    intent_sha256: String,
    binding: AcquisitionBinding,
    suite_id: String,
    suite_sha256: String,
    model: ModelConfig,
    session: AcquisitionSession,
    version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    fence_reason: Option<AcquisitionFence>,
    record_sha256: String,
}
impl AcquisitionRecord {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn session(&self) -> &AcquisitionSession {
        &self.session
    }
    pub fn binding(&self) -> &AcquisitionBinding {
        &self.binding
    }
    pub fn suite_id(&self) -> &str {
        &self.suite_id
    }
    pub fn is_fenced(&self) -> bool {
        self.fence_reason.is_some()
    }
    pub fn fence_reason(&self) -> Option<AcquisitionFence> {
        self.fence_reason
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn with_session(&self, session: AcquisitionSession) -> Result<Self> {
        if session.plan() != self.session.plan() {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
        let mut next = self.clone();
        next.session = session;
        next.seal()?;
        Ok(next)
    }
    pub fn fence(&mut self, reason: AcquisitionFence) {
        if self.fence_reason.is_none() {
            self.fence_reason = Some(reason);
        }
        // Every field is serializable; errors are still checked by store.validate
        // rather than making fence capable of panicking on an invalid checkpoint.
        if let Ok(hash) = self.digest() {
            self.record_sha256 = hash;
        }
    }
    pub fn validate(&self, suite: &OracleSuite) -> Result<()> {
        if self.schema_version != 1
            || !label(&self.id, 128)
            || !label(&self.client_request_id, 128)
            || self.version == 0
            || self.version > i64::MAX as u64
            || self.created_at > self.updated_at
            || self.binding.deadline <= self.created_at
            || self.suite_id != suite.suite_id
            || self.suite_sha256 != hash(suite)?
            || self.record_sha256 != self.digest()?
            || self.session.plan().model != self.model
            || !digest_valid(&self.intent_sha256)
            || !digest_valid(&self.binding.requirement_sha256)
            || !digest_valid(&self.binding.authority_sha256)
            || !digest_valid(&self.binding.mission_sha256)
            || self.binding.grant.role_id != self.binding.role_id
            || self.binding.grant.responsibility_id != self.binding.responsibility_id
            || self.session.plan().request.operation_id != format!("{}:generate:0", self.id)
            || self
                .session
                .plan()
                .repair
                .as_ref()
                .is_some_and(|r| r.operation_id != format!("{}:generate:1", self.id))
        {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
        self.session
            .validate(suite)
            .map_err(|_| AcquisitionStoreError::InvalidRecord)?;
        if encoded(self)?.len() > MAX_RECORD_BYTES {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
        Ok(())
    }
    pub fn authorize(&self, state: &OrgState, now: DateTime<Utc>) -> Result<()> {
        if let Some(reason) = self.fence_reason {
            return Err(AcquisitionStoreError::Authority(reason));
        }
        let b = &self.binding;
        if state.org_id != b.org_id {
            return fenced(AcquisitionFence::AuthorityChanged);
        }
        if b.lease_expires_at.is_some_and(|until| until <= now) {
            return fenced(AcquisitionFence::LeaseExpired);
        }
        if b.deadline <= now {
            return fenced(AcquisitionFence::DeadlineExpired);
        }
        let (grant, requirement, authority, mission, lease) = authority_snapshot(
            state,
            &b.mission_id,
            &b.requirement_id,
            &b.role_id,
            &b.responsibility_id,
            now,
        )?;
        if requirement != b.requirement_sha256 {
            return fenced(AcquisitionFence::RequirementChanged);
        }
        if grant != b.grant
            || authority != b.authority_sha256
            || mission != b.mission_sha256
            || lease != b.lease_expires_at
        {
            return fenced(AcquisitionFence::AuthorityChanged);
        }
        Ok(())
    }
    pub(crate) fn active(&self, suite: &OracleSuite) -> Result<bool> {
        if self.is_fenced() {
            return Ok(false);
        }
        match self.session.phase() {
            AcquisitionPhase::Generating | AcquisitionPhase::Forging => Ok(true),
            AcquisitionPhase::Rejected => {
                let mut next = self.session.clone();
                next.advance_local(suite)
                    .map_err(|_| AcquisitionStoreError::InvalidRecord)
            }
            _ => Ok(false),
        }
    }
    fn immutable_digest(&self) -> Result<String> {
        hash(&(
            &self.id,
            &self.client_request_id,
            &self.intent_sha256,
            &self.binding,
            &self.suite_id,
            &self.suite_sha256,
            &self.model,
            self.session.plan(),
            self.created_at,
        ))
    }
    fn digest(&self) -> Result<String> {
        hash(&(
            "genesis-server-acquisition-v1",
            self.schema_version,
            self.immutable_digest()?,
            &self.session,
            self.version,
            self.updated_at,
            self.fence_reason,
        ))
    }
    fn seal(&mut self) -> Result<()> {
        self.record_sha256 = self.digest()?;
        Ok(())
    }
}

impl Store {
    /// Call from trusted startup configuration. Counters persist across restarts;
    /// changing quota ceilings is refused rather than silently resetting budget.
    pub fn init_acquisitions(&self, policy: &AcquisitionPolicy) -> Result<()> {
        policy.validate()?;
        let mut conn = self.lock();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS acquisition_limits (singleton INTEGER PRIMARY KEY CHECK(singleton=1),quota_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS acquisition_counters (singleton INTEGER PRIMARY KEY CHECK(singleton=1),total_intents INTEGER NOT NULL,model_calls INTEGER NOT NULL,actor_creates INTEGER NOT NULL,builds INTEGER NOT NULL,test_submits INTEGER NOT NULL,read_actions INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS capability_acquisitions (id TEXT PRIMARY KEY,client_request_id TEXT NOT NULL UNIQUE,requirement_id TEXT NOT NULL,assignment_id TEXT NOT NULL,epoch INTEGER NOT NULL,suite_id TEXT NOT NULL,version INTEGER NOT NULL,active INTEGER NOT NULL CHECK(active IN(0,1)),record_json TEXT NOT NULL);
            CREATE UNIQUE INDEX IF NOT EXISTS acquisition_active_grant ON capability_acquisitions(requirement_id,assignment_id,epoch) WHERE active=1;")?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT quota_json FROM acquisition_limits WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let quotas: AcquisitionQuotas = decode_json(existing.as_bytes())
                .map_err(|_| AcquisitionStoreError::InvalidRecord)?;
            if quotas != policy.quotas {
                return Err(AcquisitionStoreError::PolicyChanged);
            }
        } else {
            tx.execute(
                "INSERT INTO acquisition_limits(singleton,quota_json) VALUES(1,?1)",
                params![text(&policy.quotas)?],
            )?;
            tx.execute("INSERT INTO acquisition_counters(singleton,total_intents,model_calls,actor_creates,builds,test_submits,read_actions) VALUES(1,0,0,0,0,0,0)",[])?;
        }
        tx.commit()?;
        drop(conn);
        // The journal must exist before any provider intent can be created.
        // Existing unjournaled records remain unavailable until trusted startup
        // validates and baselines them; quota configuration is not a migration.
        self.init_acquisition_history()?;
        Ok(())
    }
    pub fn create_acquisition(
        &self,
        intent: &CreateAcquisition,
        policy: &AcquisitionPolicy,
        suite: &OracleSuite,
        now: DateTime<Utc>,
    ) -> Result<(AcquisitionRecord, bool)> {
        policy.validate()?;
        if !label(&intent.client_request_id, 128)
            || !label(&intent.suite_id, 256)
            || intent.suite_id != suite.suite_id
        {
            return Err(AcquisitionStoreError::InvalidInput);
        }
        let intent_sha256 = hash(intent)?;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let limits = limits(&tx)?;
        if policy.quotas != limits {
            return Err(AcquisitionStoreError::PolicyChanged);
        }
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT id,suite_id FROM capability_acquisitions WHERE client_request_id=?1",
                params![intent.client_request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((id, suite_id)) = existing {
            // A different configured suite is a changed intent, not corrupt
            // evidence. Do not validate the saved session against that suite.
            if suite_id != intent.suite_id {
                return Err(AcquisitionStoreError::IdempotencyConflict);
            }
            let record = load_record(&tx, &id, suite)?;
            if record.intent_sha256 != intent_sha256 {
                return Err(AcquisitionStoreError::IdempotencyConflict);
            }
            return Ok((record, false));
        }
        let state = current_state(&tx)?;
        let (grant, requirement_sha256, authority_sha256, mission_sha256, lease_expires_at) =
            authority_snapshot(
                &state,
                &intent.mission_id,
                &intent.requirement_id,
                &intent.role_id,
                &intent.responsibility_id,
                now,
            )?;
        if grant.epoch > i64::MAX as u64 {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
        let mut deadline = now
            .checked_add_signed(Duration::seconds(policy.max_duration_seconds as i64))
            .ok_or(AcquisitionStoreError::InvalidInput)?;
        for bound in [
            intent.deadline,
            state.missions[&intent.mission_id].deadline,
            lease_expires_at,
        ]
        .into_iter()
        .flatten()
        {
            deadline = deadline.min(bound);
        }
        if deadline <= now {
            return fenced(AcquisitionFence::DeadlineExpired);
        }
        let id = format!("acq_{}", uuid::Uuid::new_v4().simple());
        let requirement = &state.requirements[&intent.requirement_id];
        let request = SynthesisRequest {
            operation_id: format!("{id}:generate:0"),
            candidate: intent.candidate.clone(),
            requirement: format!(
                "Requirement: {}\nAcceptance: {}",
                requirement.summary, requirement.acceptance
            ),
            allowed_source_urls: intent.allowed_source_urls.clone(),
            examples: intent.examples.clone(),
        };
        let plan = AcquisitionPlan {
            request,
            model: policy.model.clone(),
            repair: intent.repair.clone().map(|candidate| RepairIdentity {
                operation_id: format!("{id}:generate:1"),
                candidate,
            }),
        };
        let session = AcquisitionSession::new(plan, suite)
            .map_err(|_| AcquisitionStoreError::InvalidInput)?;
        let mut record = AcquisitionRecord {
            schema_version: 1,
            id,
            client_request_id: intent.client_request_id.clone(),
            intent_sha256,
            binding: AcquisitionBinding {
                org_id: state.org_id.clone(),
                mission_id: intent.mission_id.clone(),
                requirement_id: intent.requirement_id.clone(),
                role_id: intent.role_id.clone(),
                responsibility_id: intent.responsibility_id.clone(),
                grant,
                deadline,
                lease_expires_at,
                requirement_sha256,
                authority_sha256,
                mission_sha256,
            },
            suite_id: suite.suite_id.clone(),
            suite_sha256: hash(suite)?,
            model: policy.model.clone(),
            session,
            version: 1,
            created_at: now,
            updated_at: now,
            fence_reason: None,
            record_sha256: String::new(),
        };
        record.seal()?;
        record.validate(suite)?;
        let b = &record.binding;
        let occupied:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM capability_acquisitions WHERE requirement_id=?1 AND assignment_id=?2 AND epoch=?3 AND active=1)",params![b.requirement_id.as_str(),b.grant.assignment_id.as_str(),b.grant.epoch as i64],|row|row.get(0))?;
        if occupied {
            return Err(AcquisitionStoreError::ActiveConflict);
        }
        let mut counts = counters(&tx)?;
        require_count(counts.total_intents, limits.total_intents, "total_intents")?;
        require_count(counts.active, limits.active, "active")?;
        counts.total_intents += 1;
        persist_counters(&tx, &counts)?;
        tx.execute("INSERT INTO capability_acquisitions(id,client_request_id,requirement_id,assignment_id,epoch,suite_id,version,active,record_json) VALUES(?1,?2,?3,?4,?5,?6,1,1,?7)",params![record.id,record.client_request_id,b.requirement_id.as_str(),b.grant.assignment_id.as_str(),b.grant.epoch as i64,record.suite_id,text(&record)?])?;
        history::append_checkpoint(
            &tx,
            &record,
            state.revision,
            CheckpointKind::Created,
            None,
            true,
            now,
        )?;
        tx.commit()?;
        Ok((record, true))
    }
    pub fn acquisition_suite_id(&self, id: &str) -> Result<String> {
        self.lock()
            .query_row(
                "SELECT suite_id FROM capability_acquisitions WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(AcquisitionStoreError::NotFound)
    }
    pub fn get_acquisition(&self, id: &str, suite: &OracleSuite) -> Result<AcquisitionRecord> {
        let conn = self.lock();
        load_record(&conn, id, suite)
    }
    pub fn acquisition_ids(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id FROM capability_acquisitions ORDER BY id")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(ids)
    }
    pub fn acquisition_counters(&self) -> Result<AcquisitionCounters> {
        counters(&self.lock())
    }
    /// CAS the complete record and its quota debit in one immediate transaction.
    /// The organization revision is checked even for completion writes. Revoked
    /// completions remain recorded, but are permanently fenced from further use.
    pub fn save_acquisition(
        &self,
        expected_version: u64,
        expected_org_revision: u64,
        record: &AcquisitionRecord,
        suite: &OracleSuite,
        debit: Option<AcquisitionDebit>,
        now: DateTime<Utc>,
    ) -> Result<AcquisitionRecord> {
        record.validate(suite)?;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = load_record(&tx, record.id(), suite)?;
        if previous.version != expected_version
            || record.version != expected_version
            || previous.immutable_digest()? != record.immutable_digest()?
        {
            return Err(AcquisitionStoreError::Conflict);
        }
        if previous.is_fenced() && previous.fence_reason != record.fence_reason {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
        let state = current_state(&tx)?;
        if state.revision != expected_org_revision {
            return Err(AcquisitionStoreError::Conflict);
        }
        let mut next = record.clone();
        validate_debit(&previous, &next, debit)?;
        if let Some(debit) = debit {
            if previous.is_fenced() || next.is_fenced() {
                return fenced(AcquisitionFence::AuthorityChanged);
            }
            next.authorize(&state, now)?;
            let limits = limits(&tx)?;
            let mut counts = counters(&tx)?;
            let (count, limit, label) = match debit {
                AcquisitionDebit::ModelCall => {
                    (&mut counts.model_calls, limits.model_calls, "model_calls")
                }
                AcquisitionDebit::ActorCreate => (
                    &mut counts.actor_creates,
                    limits.actor_creates,
                    "actor_creates",
                ),
                AcquisitionDebit::Build => (&mut counts.builds, limits.builds, "builds"),
                AcquisitionDebit::TestSubmit => (
                    &mut counts.test_submits,
                    limits.test_submits,
                    "test_submits",
                ),
                AcquisitionDebit::Read => (
                    &mut counts.read_actions,
                    limits.read_actions,
                    "read_actions",
                ),
            };
            require_count(*count, limit, label)?;
            *count += 1;
            persist_counters(&tx, &counts)?;
        } else if let Err(error) = next.authorize(&state, now) {
            match error {
                AcquisitionStoreError::Authority(reason) => next.fence(reason),
                other => return Err(other),
            }
        }
        next.version = expected_version
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(AcquisitionStoreError::InvalidRecord)?;
        if now < next.updated_at {
            return Err(AcquisitionStoreError::InvalidInput);
        }
        next.updated_at = now;
        next.seal()?;
        next.validate(suite)?;
        let active = next.active(suite)?;
        let updated=tx.execute("UPDATE capability_acquisitions SET version=?1,active=?2,record_json=?3 WHERE id=?4 AND version=?5",params![next.version as i64,active,text(&next)?,next.id,expected_version as i64])?;
        if updated != 1 {
            return Err(AcquisitionStoreError::Conflict);
        }
        history::append_checkpoint(
            &tx,
            &next,
            state.revision,
            CheckpointKind::Transition,
            debit,
            active,
            now,
        )?;
        tx.commit()?;
        Ok(next)
    }
}

fn validate_debit(
    previous: &AcquisitionRecord,
    next: &AcquisitionRecord,
    debit: Option<AcquisitionDebit>,
) -> Result<()> {
    let before = previous.session();
    let after = next.session();
    let model_started = after.generation().phase() == SynthesisPhase::Requesting
        && (before.generation().phase() != SynthesisPhase::Requesting
            || before.attempt() != after.attempt());
    let new_forge = after.forge().and_then(|f| f.pending_action());
    let old_forge = before.forge().and_then(|f| f.pending_action());
    let changed_forge = new_forge.is_some()
        && (old_forge != new_forge
            || before.attempt() != after.attempt()
            || before.forge().is_none());
    let expected = if model_started {
        Some(AcquisitionDebit::ModelCall)
    } else if changed_forge {
        new_forge.map(Into::into)
    } else {
        None
    };
    if let Some(expected) = expected {
        if debit != Some(expected) {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
    } else if let Some(debit) = debit {
        // Reissuing the same saved GET is allowed, but every read is charged.
        if debit != AcquisitionDebit::Read
            || !matches!(
                new_forge,
                Some(ActionKind::PollBuild | ActionKind::CollectCase)
            )
            || old_forge != new_forge
        {
            return Err(AcquisitionStoreError::InvalidRecord);
        }
    }
    Ok(())
}

type AuthoritySnapshot = (DispatchGrant, String, String, String, Option<DateTime<Utc>>);
fn authority_snapshot(
    state: &OrgState,
    mission_id: &MissionId,
    requirement_id: &RequirementId,
    role_id: &RoleId,
    responsibility_id: &ResponsibilityId,
    now: DateTime<Utc>,
) -> Result<AuthoritySnapshot> {
    let mission = state
        .missions
        .get(mission_id)
        .ok_or(AcquisitionStoreError::Authority(
            AcquisitionFence::AuthorityChanged,
        ))?;
    if mission.status != MissionStatus::Active {
        return fenced(AcquisitionFence::AuthorityChanged);
    }
    if mission.deadline.is_some_and(|until| until <= now) {
        return fenced(AcquisitionFence::DeadlineExpired);
    }
    let requirement =
        state
            .requirements
            .get(requirement_id)
            .ok_or(AcquisitionStoreError::Authority(
                AcquisitionFence::RequirementChanged,
            ))?;
    let responsibility =
        state
            .responsibilities
            .get(responsibility_id)
            .ok_or(AcquisitionStoreError::Authority(
                AcquisitionFence::AuthorityChanged,
            ))?;
    if requirement.mission_id != *mission_id
        || responsibility.mission_id != *mission_id
        || responsibility.requirement_id.as_ref() != Some(requirement_id)
        || responsibility.status != ResponsibilityStatus::Open
        || requirement.status != RequirementStatus::Assigned
        || requirement.owner != responsibility.owner
        || matches!(requirement.owner, Owner::MissionController)
        || requirement.triggering_evidence.is_empty()
    {
        return fenced(AcquisitionFence::RequirementChanged);
    }
    let observations = requirement
        .triggering_evidence
        .iter()
        .map(|id| {
            state
                .observations
                .get(id)
                .filter(|observation| observation.mission_id == *mission_id)
                .ok_or(AcquisitionStoreError::Authority(
                    AcquisitionFence::RequirementChanged,
                ))
        })
        .collect::<Result<Vec<_>>>()?;
    let grant = authorize_dispatch(state, role_id, responsibility_id, Some(ACQUIRE_SCOPE))
        .map_err(|_| AcquisitionStoreError::Authority(AcquisitionFence::AuthorityChanged))?;
    let assignment =
        state
            .assignments
            .get(&grant.assignment_id)
            .ok_or(AcquisitionStoreError::Authority(
                AcquisitionFence::AuthorityChanged,
            ))?;
    if assignment
        .lease_expires_at
        .is_some_and(|until| until <= now)
    {
        return fenced(AcquisitionFence::LeaseExpired);
    }
    let role = state
        .roles
        .get(role_id)
        .ok_or(AcquisitionStoreError::Authority(
            AcquisitionFence::AuthorityChanged,
        ))?;
    let authority = hash(&(
        &grant,
        assignment,
        &role.authority,
        responsibility,
        &observations,
    ))?;
    Ok((
        grant,
        hash(requirement)?,
        authority,
        hash(mission)?,
        assignment.lease_expires_at,
    ))
}
fn current_state(conn: &Connection) -> Result<OrgState> {
    let text: String = conn.query_row(
        "SELECT state_json FROM org_snapshots ORDER BY revision DESC LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    decode_json(text.as_bytes()).map_err(|_| AcquisitionStoreError::InvalidRecord)
}
fn limits(conn: &Connection) -> Result<AcquisitionQuotas> {
    let text: String = conn.query_row(
        "SELECT quota_json FROM acquisition_limits WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    let limits: AcquisitionQuotas =
        decode_json(text.as_bytes()).map_err(|_| AcquisitionStoreError::InvalidRecord)?;
    limits.validate()?;
    Ok(limits)
}
fn counters(conn: &Connection) -> Result<AcquisitionCounters> {
    let mut counters=conn.query_row("SELECT total_intents,model_calls,actor_creates,builds,test_submits,read_actions FROM acquisition_counters WHERE singleton=1",[],|row|Ok(AcquisitionCounters{total_intents:row.get(0)?,active:0,model_calls:row.get(1)?,actor_creates:row.get(2)?,builds:row.get(3)?,test_submits:row.get(4)?,read_actions:row.get(5)?}))?;
    counters.active = conn.query_row(
        "SELECT COUNT(*) FROM capability_acquisitions WHERE active=1",
        [],
        |row| row.get(0),
    )?;
    Ok(counters)
}
fn persist_counters(tx: &Transaction<'_>, c: &AcquisitionCounters) -> Result<()> {
    tx.execute("UPDATE acquisition_counters SET total_intents=?1,model_calls=?2,actor_creates=?3,builds=?4,test_submits=?5,read_actions=?6 WHERE singleton=1",params![c.total_intents,c.model_calls,c.actor_creates,c.builds,c.test_submits,c.read_actions])?;
    Ok(())
}
pub(crate) fn load_record(
    conn: &Connection,
    id: &str,
    suite: &OracleSuite,
) -> Result<AcquisitionRecord> {
    let row: Option<(String, u64, bool, String)> = conn
        .query_row(
            "SELECT record_json,version,active,suite_id FROM capability_acquisitions WHERE id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let (text, version, active, suite_id) = row.ok_or(AcquisitionStoreError::NotFound)?;
    let record = decode_record(&text, suite)?;
    if record.id != id
        || record.version != version
        || record.active(suite)? != active
        || record.suite_id != suite_id
        || record.binding.org_id != current_state(conn)?.org_id
    {
        return Err(AcquisitionStoreError::InvalidRecord);
    }
    Ok(record)
}
fn decode_record(text: &str, suite: &OracleSuite) -> Result<AcquisitionRecord> {
    if text.len() > MAX_RECORD_BYTES {
        return Err(AcquisitionStoreError::InvalidRecord);
    }
    // Nested session limit is 8MiB; allow the separately bounded outer envelope.
    // Typed fields reject duplicates; arbitrary provider output is hashed and
    // reverified by AcquisitionSession before any use.
    let record: AcquisitionRecord =
        serde_json::from_str(text).map_err(|_| AcquisitionStoreError::InvalidRecord)?;
    record.validate(suite)?;
    Ok(record)
}
fn hash(value: &impl Serialize) -> Result<String> {
    serde_json::to_value(value)
        .map(|value| json_sha256(&value))
        .map_err(|_| AcquisitionStoreError::Encoding)
}
fn encoded(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| AcquisitionStoreError::Encoding)
}
fn text(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|_| AcquisitionStoreError::Encoding)
}
fn label(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn require_count(current: u64, limit: u64, name: &'static str) -> Result<()> {
    if current >= limit {
        Err(AcquisitionStoreError::QuotaExceeded(name))
    } else {
        Ok(())
    }
}
fn fenced<T>(reason: AcquisitionFence) -> Result<T> {
    Err(AcquisitionStoreError::Authority(reason))
}
