//! Durable, local-only linkage from a committed gap to one acquisition intent.
//! Policy materialization happens once. Creation, count debit, acquisition
//! journal and parent linkage share one transaction; no provider I/O occurs here.
use crate::capability_acquisition_store::{
    create_acquisition_in_tx, current_state, gap_authority_snapshot, load_record,
    AcquisitionBinding, AcquisitionPolicy, AcquisitionRecord, AcquisitionStoreError,
    CreateAcquisition,
};
use crate::capability_gap_policy::{GapAcquisitionPolicy, GapPolicyError};
use crate::store::Store;
use chrono::{DateTime, Duration, Utc};
use genesis_core::{
    digest::canonical_sha256,
    ids::{CapabilityGapId, OrgId},
    lifecycle::RoleState,
    model::{CapabilityGap, CapabilityGapStatus, OrgState},
};
use genesis_verification::{decode_json, json_sha256, validate_suite, OracleSuite};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_BINDING_BYTES: usize = 5 * 1024 * 1024;

#[derive(thiserror::Error)]
pub enum GapAcquisitionError {
    #[error("gap policy cannot materialize this request: {0}")]
    Policy(#[from] GapPolicyError),
    #[error("gap acquisition could not be created: {0}")]
    Acquisition(#[from] AcquisitionStoreError),
    #[error("gap role is awaiting activation")]
    WaitingForRole,
    #[error("gap authority or requirement changed")]
    AuthorityChanged,
    #[error("invalid gap acquisition binding")]
    InvalidRecord,
    #[error("gap acquisition binding not found")]
    NotFound,
    #[error("gap acquisition database operation failed")]
    Db(#[from] rusqlite::Error),
    #[error("cannot encode gap acquisition binding")]
    Encoding,
}
impl std::fmt::Debug for GapAcquisitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not expose raw SQLite errors or public examples in diagnostic output.
        std::fmt::Display::fmt(self, f)
    }
}
pub type Result<T> = std::result::Result<T, GapAcquisitionError>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapAcquisitionBinding {
    schema_version: u32,
    gap: CapabilityGap,
    request: CreateAcquisition,
    policy: AcquisitionPolicy,
    policy_digest: String,
    suite_sha256: String,
    authority: AcquisitionBinding,
    bound_at: DateTime<Utc>,
    acquisition_id: Option<String>,
    binding_sha256: String,
}
impl std::fmt::Debug for GapAcquisitionBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GapAcquisitionBinding")
            .field("gap_id", &self.gap.gap_id)
            .field("acquisition_id", &self.acquisition_id)
            .finish_non_exhaustive()
    }
}
impl GapAcquisitionBinding {
    pub fn request(&self) -> &CreateAcquisition {
        &self.request
    }
    pub fn gap(&self) -> &CapabilityGap {
        &self.gap
    }
    pub fn acquisition_id(&self) -> Option<&str> {
        self.acquisition_id.as_deref()
    }
    pub fn suite_id(&self) -> &str {
        &self.request.suite_id
    }
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }
    pub fn org_id(&self) -> &OrgId {
        &self.authority.org_id
    }
    pub fn suite_matches(&self, suite: &OracleSuite) -> bool {
        suite.suite_id == self.request.suite_id
            && hash(suite).is_ok_and(|digest| digest == self.suite_sha256)
    }

    /// A saved binding is not a dispatch grant. Recheck its exact work and
    /// assignment before creating an intent, even after waiting for activation.
    pub fn validate_current(&self, state: &OrgState, now: DateTime<Utc>) -> Result<()> {
        self.validate()?;
        if state.org_id != self.authority.org_id || self.authority.deadline <= now {
            return Err(GapAcquisitionError::AuthorityChanged);
        }
        let gap = state
            .capability_gaps
            .get(&self.gap.gap_id)
            .ok_or(GapAcquisitionError::AuthorityChanged)?;
        let mut frozen_identity = gap.clone();
        frozen_identity.status = CapabilityGapStatus::Open;
        if frozen_identity != self.gap
            || gap
                .status
                .acquisition_id()
                .is_some_and(|id| Some(id) != self.acquisition_id())
            || matches!(gap.status, CapabilityGapStatus::Refused { .. })
        {
            return Err(GapAcquisitionError::AuthorityChanged);
        }
        validate_requirement(state, gap)?;
        let (grant, requirement, authority, mission, lease) = gap_authority_snapshot(
            state,
            &gap.mission_id,
            &gap.requirement_id,
            &gap.role_id,
            &gap.responsibility_id,
            now,
        )
        .map_err(authority_error)?;
        let b = &self.authority;
        if grant != b.grant
            || requirement != b.requirement_sha256
            || authority != b.authority_sha256
            || mission != b.mission_sha256
            || lease != b.lease_expires_at
        {
            return Err(GapAcquisitionError::AuthorityChanged);
        }
        if state.roles[&gap.role_id].state != RoleState::Active {
            return Err(GapAcquisitionError::WaitingForRole);
        }
        Ok(())
    }

    /// Validate immutable child provenance without requiring present authority.
    /// This supports historical/unknown-outcome reconciliation after revocation.
    pub fn matches_record(&self, record: &AcquisitionRecord) -> Result<()> {
        self.validate()?;
        if self.acquisition_id() != Some(record.id())
            || !record.matches_frozen_request(
                &self.request,
                &self.policy,
                &self.authority,
                &self.suite_sha256,
            )?
        {
            return Err(GapAcquisitionError::InvalidRecord);
        }
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        let b = &self.authority;
        if self.schema_version != 1
            || self.binding_sha256 != self.digest()?
            || self.gap.status != CapabilityGapStatus::Open
            || self.request.client_request_id != self.gap.gap_id.as_str()
            || self.request.mission_id != self.gap.mission_id
            || self.request.requirement_id != self.gap.requirement_id
            || self.request.role_id != self.gap.role_id
            || self.request.responsibility_id != self.gap.responsibility_id
            || self.request.candidate.capability_id != self.gap.capability_id.as_str()
            || b.mission_id != self.gap.mission_id
            || b.requirement_id != self.gap.requirement_id
            || b.role_id != self.gap.role_id
            || b.responsibility_id != self.gap.responsibility_id
            || b.grant.role_id != b.role_id
            || b.grant.responsibility_id != b.responsibility_id
            || b.deadline <= self.bound_at
            || self.request.deadline != Some(b.deadline)
            || !valid_digest(&self.policy_digest)
            || !valid_digest(&self.suite_sha256)
            || self
                .acquisition_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 128)
        {
            return Err(GapAcquisitionError::InvalidRecord);
        }
        self.policy
            .validate()
            .map_err(|_| GapAcquisitionError::InvalidRecord)?;
        encoded(self)?;
        Ok(())
    }
    fn digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self).map_err(|_| GapAcquisitionError::Encoding)?;
        value
            .as_object_mut()
            .ok_or(GapAcquisitionError::Encoding)?
            .remove("binding_sha256");
        Ok(json_sha256(
            &json!({"domain":"genesis-gap-acquisition-binding-v1","binding":value}),
        ))
    }
    fn seal(&mut self) -> Result<()> {
        self.binding_sha256 = self.digest()?;
        self.validate()
    }
}

impl Store {
    /// Safe to initialize before acquisition configuration/tables exist.
    pub fn init_gap_acquisitions(&self) -> Result<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_gap_acquisitions(
                gap_id TEXT PRIMARY KEY,
                acquisition_id TEXT UNIQUE,
                binding_json TEXT NOT NULL
             );",
        )?;
        Ok(())
    }
    pub fn gap_acquisition_binding(&self, gap_id: &str) -> Result<Option<GapAcquisitionBinding>> {
        load_binding(&self.lock(), gap_id)
    }
    pub fn bind_gap_acquisition(
        &self,
        gap_id: &str,
        gap_policy: &GapAcquisitionPolicy,
        policy: &AcquisitionPolicy,
        suite: &OracleSuite,
        now: DateTime<Utc>,
    ) -> Result<GapAcquisitionBinding> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Frozen policy wins even if current configuration no longer matches.
        if let Some(binding) = load_binding(&tx, gap_id)? {
            return Ok(binding);
        }
        policy.validate()?;
        let state = current_state(&tx)?;
        let gap = state
            .capability_gaps
            .get(&CapabilityGapId::new(gap_id))
            .ok_or(GapAcquisitionError::NotFound)?;
        if gap.status != CapabilityGapStatus::Open {
            return Err(GapAcquisitionError::AuthorityChanged);
        }
        validate_requirement(&state, gap)?;
        let mut request = gap_policy.materialize(&state.org_id, gap)?;
        if suite.suite_id != request.suite_id || validate_suite(suite).is_err() {
            return Err(GapAcquisitionError::InvalidRecord);
        }
        let (grant, requirement_sha256, authority_sha256, mission_sha256, lease_expires_at) =
            gap_authority_snapshot(
                &state,
                &gap.mission_id,
                &gap.requirement_id,
                &gap.role_id,
                &gap.responsibility_id,
                now,
            )
            .map_err(authority_error)?;
        let mut deadline = now
            .checked_add_signed(Duration::seconds(policy.max_duration_seconds as i64))
            .ok_or(GapAcquisitionError::InvalidRecord)?;
        for bound in [state.missions[&gap.mission_id].deadline, lease_expires_at]
            .into_iter()
            .flatten()
        {
            deadline = deadline.min(bound);
        }
        if deadline <= now {
            return Err(GapAcquisitionError::AuthorityChanged);
        }
        request.deadline = Some(deadline);
        let mut binding = GapAcquisitionBinding {
            schema_version: 1,
            gap: gap.clone(),
            request,
            policy: policy.clone(),
            policy_digest: gap_policy.digest()?,
            suite_sha256: hash(suite)?,
            authority: AcquisitionBinding {
                org_id: state.org_id.clone(),
                mission_id: gap.mission_id.clone(),
                requirement_id: gap.requirement_id.clone(),
                role_id: gap.role_id.clone(),
                responsibility_id: gap.responsibility_id.clone(),
                grant,
                deadline,
                lease_expires_at,
                requirement_sha256,
                authority_sha256,
                mission_sha256,
            },
            bound_at: now,
            acquisition_id: None,
            binding_sha256: String::new(),
        };
        binding.seal()?;
        tx.execute("INSERT INTO capability_gap_acquisitions(gap_id,acquisition_id,binding_json) VALUES(?1,NULL,?2)",
            params![gap_id, encoded(&binding)?])?;
        tx.commit()?;
        Ok(binding)
    }
    pub fn create_gap_acquisition(
        &self,
        gap_id: &str,
        suite: &OracleSuite,
        now: DateTime<Utc>,
    ) -> Result<(AcquisitionRecord, bool)> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut binding = load_binding(&tx, gap_id)?.ok_or(GapAcquisitionError::NotFound)?;
        if !binding.suite_matches(suite) {
            return Err(GapAcquisitionError::InvalidRecord);
        }
        if let Some(id) = binding.acquisition_id() {
            let record = load_record(&tx, id, suite)?;
            binding.matches_record(&record)?;
            // Historical lookup is idempotent and never reauthorizes effects.
            return Ok((record, false));
        }
        let state = current_state(&tx)?;
        binding.validate_current(&state, now)?;
        let (record, created) =
            create_acquisition_in_tx(&tx, &binding.request, &binding.policy, suite, now)?;
        if !created {
            // A true child can never be committed without its link. An existing
            // unlinked request belongs to a different caller, even if identical.
            return Err(AcquisitionStoreError::IdempotencyConflict.into());
        }
        binding.acquisition_id = Some(record.id().to_owned());
        binding.seal()?;
        binding.matches_record(&record)?;
        tx.execute("UPDATE capability_gap_acquisitions SET acquisition_id=?1,binding_json=?2 WHERE gap_id=?3 AND acquisition_id IS NULL",
            params![record.id(), encoded(&binding)?, gap_id])?;
        tx.commit()?;
        Ok((record, true))
    }
}
fn load_binding(conn: &Connection, gap_id: &str) -> Result<Option<GapAcquisitionBinding>> {
    let row: Option<(Option<String>, Option<String>)> = conn.query_row(
        "SELECT acquisition_id, CASE WHEN length(CAST(binding_json AS BLOB))<=?2 THEN binding_json END
         FROM capability_gap_acquisitions WHERE gap_id=?1", params![gap_id, MAX_BINDING_BYTES as i64],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let Some((id, text)) = row else {
        return Ok(None);
    };
    let binding: GapAcquisitionBinding =
        decode_json(text.ok_or(GapAcquisitionError::InvalidRecord)?.as_bytes())
            .map_err(|_| GapAcquisitionError::InvalidRecord)?;
    binding.validate()?;
    if binding.gap.gap_id.as_str() != gap_id || binding.acquisition_id != id {
        return Err(GapAcquisitionError::InvalidRecord);
    }
    Ok(Some(binding))
}
fn validate_requirement(state: &OrgState, gap: &CapabilityGap) -> Result<()> {
    let requirement = state
        .requirements
        .get(&gap.requirement_id)
        .ok_or(GapAcquisitionError::AuthorityChanged)?;
    let digest = canonical_sha256(&json!({
        "requirement_id":requirement.requirement_id,"mission_id":requirement.mission_id,
        "summary":requirement.summary,"acceptance":requirement.acceptance,
        "triggering_evidence":requirement.triggering_evidence,
    }));
    if requirement.mission_id != gap.mission_id
        || digest != gap.requirement_digest
        || requirement.triggering_evidence != gap.triggering_evidence
    {
        return Err(GapAcquisitionError::AuthorityChanged);
    }
    Ok(())
}
fn authority_error(error: AcquisitionStoreError) -> GapAcquisitionError {
    match error {
        AcquisitionStoreError::Authority(_) => GapAcquisitionError::AuthorityChanged,
        other => other.into(),
    }
}
fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hash(value: &impl Serialize) -> Result<String> {
    serde_json::to_value(value)
        .map(|value| json_sha256(&value))
        .map_err(|_| GapAcquisitionError::Encoding)
}
fn encoded(value: &impl Serialize) -> Result<String> {
    let text = serde_json::to_string(value).map_err(|_| GapAcquisitionError::Encoding)?;
    if text.len() > MAX_BINDING_BYTES {
        return Err(GapAcquisitionError::InvalidRecord);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_sha256, CapabilityGap, CapabilityGapStatus, GapAcquisitionError,
        GapAcquisitionPolicy,
    };
    use crate::capability_acquisition_store::{
        AcquisitionPolicy, AcquisitionQuotas, AcquisitionStoreError, CreateAcquisition,
    };
    use crate::capability_gap_policy::GapAcquisitionRule;
    use crate::store::{EventMeta, Store};
    use chrono::{DateTime, Duration, Utc};
    use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
    use genesis_core::ids::*;
    use genesis_core::lifecycle::{RequirementStatus, RoleState};
    use genesis_core::model::{Observation, OrgState, Owner, Requirement};
    use genesis_forge::{CandidateIdentity, ModelConfig, PublicExample};
    use genesis_verification::{decode_json, OracleSuite};
    use serde_json::json;
    use std::path::PathBuf;

    fn suite() -> OracleSuite {
        decode_json(include_bytes!(
            "../../genesis-verification/examples/fixtures/trusted/suite.json"
        ))
        .unwrap()
    }
    fn policy() -> AcquisitionPolicy {
        AcquisitionPolicy {
            model: ModelConfig {
                model: "claude-sonnet-4-6".into(),
                max_tokens: 1024,
            },
            quotas: AcquisitionQuotas::default(),
            max_duration_seconds: 600,
        }
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
        store: Store,
        state: OrgState,
        intent: CreateAcquisition,
        now: DateTime<Utc>,
    }
    impl Fixture {
        fn new(policy: &AcquisitionPolicy) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("org.sqlite");
            let now = Utc::now();
            let store = Store::open(&path, OrgId::new("org-acquisition")).unwrap();
            let mut constitution = crate::demo_constitution();
            constitution.scopes.insert("capability.acquire".into());
            let initial = OrgState::new(
                OrgId::new("org-acquisition"),
                "Acquisition org",
                constitution,
            );
            store.init(&initial).unwrap();
            store.init_acquisitions(policy).unwrap();
            store.init_gap_acquisitions().unwrap();
            let bootstrap:MissionBootstrap=serde_json::from_value(json!({"goal":"Acquire required extraction capability","roles":[{"key":"lead","name":"Lead","charter":"Acquire tools","authority":{"scopes":["capability.acquire"],"budget_cents":100}}],"responsibilities":[{"purpose":"Acquire parser","owner":"lead"}]})).unwrap();
            let (outcome, mission_id) = bootstrap_mission(&initial, &bootstrap, now).unwrap();
            let mut state = outcome.state;
            let role_id = state.roles.keys().next().unwrap().clone();
            let responsibility_id = state.responsibilities.keys().next().unwrap().clone();
            let requirement_id = RequirementId::new("req-extraction");
            let observation_id = ObservationId::new("obs-public");
            state.observations.insert(
                observation_id.clone(),
                Observation {
                    observation_id: observation_id.clone(),
                    mission_id: mission_id.clone(),
                    source: "public-web".into(),
                    summary: "Venue format changed".into(),
                    evidence_ref: None,
                    observed_at: now,
                },
            );
            state.requirements.insert(
                requirement_id.clone(),
                Requirement {
                    requirement_id: requirement_id.clone(),
                    mission_id: mission_id.clone(),
                    summary: "Extract public venues".into(),
                    triggering_evidence: vec![observation_id],
                    acceptance: "Return validated venue facts".into(),
                    status: RequirementStatus::Assigned,
                    owner: Owner::Role(role_id.clone()),
                },
            );
            state
                .responsibilities
                .get_mut(&responsibility_id)
                .unwrap()
                .requirement_id = Some(requirement_id.clone());
            store
                .commit_state(
                    0,
                    &state,
                    outcome.effects,
                    &EventMeta::default(),
                    vec![],
                    |_| Ok(()),
                )
                .unwrap();
            let oracle = suite();
            let urls: Vec<String> = oracle.cases.iter().map(|c| c.source.url.clone()).collect();
            let intent = CreateAcquisition {
                client_request_id: "acquire-1".into(),
                mission_id,
                requirement_id,
                role_id,
                responsibility_id,
                suite_id: oracle.suite_id.clone(),
                candidate: CandidateIdentity {
                    capability_id: "venue-extraction".into(),
                    capability_version: "1".into(),
                    actor_name: "acquisition-venue".into(),
                    source_version: "1.0".into(),
                },
                repair: None,
                examples: vec![PublicExample {
                    url: urls[0].clone(),
                    html: "<h1>Public venue structure</h1>".into(),
                }],
                allowed_source_urls: urls,
                deadline: None,
            };
            Self {
                _dir: dir,
                path,
                store,
                state,
                intent,
                now,
            }
        }
        fn mutate(&mut self, f: impl FnOnce(&mut OrgState)) {
            let base = self.state.revision;
            f(&mut self.state);
            self.state.revision += 1;
            self.store
                .commit_state(
                    base,
                    &self.state,
                    vec![],
                    &EventMeta::default(),
                    vec![],
                    |_| Ok(()),
                )
                .unwrap();
        }
    }

    fn gap_fixture() -> (Fixture, GapAcquisitionPolicy, CapabilityGapId) {
        let mut f = Fixture::new(&policy());
        let gap_id = CapabilityGapId::new("gap-test");
        let requirement = &f.state.requirements[&f.intent.requirement_id];
        let gap = CapabilityGap {
            gap_id: gap_id.clone(),
            mission_id: f.intent.mission_id.clone(),
            requirement_id: f.intent.requirement_id.clone(),
            requirement_digest: canonical_sha256(&json!({
                "requirement_id":requirement.requirement_id,"mission_id":requirement.mission_id,
                "summary":requirement.summary,"acceptance":requirement.acceptance,
                "triggering_evidence":requirement.triggering_evidence,
            })),
            triggering_evidence: requirement.triggering_evidence.clone(),
            role_id: f.intent.role_id.clone(),
            responsibility_id: f.intent.responsibility_id.clone(),
            capability_id: CapabilityId::new("venue-extraction"),
            purpose: "Extract public venues".into(),
            observed_sources: f.intent.allowed_source_urls.clone(),
            status: CapabilityGapStatus::Open,
            opened_in_revision: f.state.revision + 1,
        };
        f.mutate(|state| {
            state.capability_gaps.insert(gap_id.clone(), gap);
        });
        let gap_policy = GapAcquisitionPolicy {
            rules: vec![GapAcquisitionRule {
                capability_id: "venue-extraction".into(),
                suite_id: f.intent.suite_id.clone(),
                actor_name_prefix: "gap-venue".into(),
                allowed_source_urls: f.intent.allowed_source_urls.clone(),
                examples: f.intent.examples.clone(),
                repair: true,
            }],
        };
        (f, gap_policy, gap_id)
    }

    #[test]
    fn first_binding_survives_policy_change_and_creates_only_one_child_after_reopen() {
        let (f, gap_policy, id) = gap_fixture();
        let oracle = suite();
        let bound = f
            .store
            .bind_gap_acquisition(id.as_str(), &gap_policy, &policy(), &oracle, f.now)
            .unwrap();
        let reopened = Store::open(&f.path, f.state.org_id.clone()).unwrap();
        reopened.init_gap_acquisitions().unwrap();
        let mut changed = policy();
        changed.model.model = "other-model".into();
        changed.max_duration_seconds = 1;
        // Even an invalid replacement policy must not rematerialize an existing intent.
        let same = reopened
            .bind_gap_acquisition(
                id.as_str(),
                &GapAcquisitionPolicy { rules: vec![] },
                &changed,
                &oracle,
                f.now + Duration::seconds(10),
            )
            .unwrap();
        assert_eq!(same.request(), bound.request());
        assert_eq!(same.policy_digest(), bound.policy_digest());
        let (record, created) = reopened
            .create_gap_acquisition(id.as_str(), &oracle, f.now + Duration::seconds(10))
            .unwrap();
        assert!(created);
        assert_eq!(record.session().plan().model, policy().model);
        assert_eq!(record.binding().deadline, f.now + Duration::seconds(600));
        let (replayed, created) = reopened
            .create_gap_acquisition(id.as_str(), &oracle, f.now + Duration::seconds(700))
            .unwrap();
        assert!(!created);
        assert_eq!(replayed.id(), record.id());
        assert_eq!(reopened.acquisition_counters().unwrap().total_intents, 1);
        reopened
            .gap_acquisition_binding(id.as_str())
            .unwrap()
            .unwrap()
            .matches_record(&record)
            .unwrap();
        assert!(!format!("{bound:?}").contains("Public venue structure"));
    }

    #[test]
    fn link_write_failure_rolls_back_child_quota_and_journal() {
        let (f, p, id) = gap_fixture();
        f.store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        f.store.lock().execute_batch("CREATE TRIGGER abort_link BEFORE UPDATE ON capability_gap_acquisitions BEGIN SELECT RAISE(ABORT,'stop link'); END;").unwrap();
        assert!(f
            .store
            .create_gap_acquisition(id.as_str(), &suite(), f.now)
            .is_err());
        assert!(f.store.acquisition_ids().unwrap().is_empty());
        assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 0);
        assert_eq!(
            f.store
                .lock()
                .query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| row
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert!(f
            .store
            .gap_acquisition_binding(id.as_str())
            .unwrap()
            .unwrap()
            .acquisition_id()
            .is_none());
        f.store
            .lock()
            .execute_batch("DROP TRIGGER abort_link;")
            .unwrap();
        assert!(
            f.store
                .create_gap_acquisition(id.as_str(), &suite(), f.now)
                .unwrap()
                .1
        );
    }

    #[test]
    fn manually_created_identical_request_cannot_become_a_gap_child() {
        let (f, p, id) = gap_fixture();
        let bound = f
            .store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        let (manual, _) = f
            .store
            .create_acquisition(bound.request(), &policy(), &suite(), f.now)
            .unwrap();
        assert!(matches!(
            f.store.create_gap_acquisition(id.as_str(), &suite(), f.now),
            Err(GapAcquisitionError::Acquisition(
                AcquisitionStoreError::IdempotencyConflict
            ))
        ));
        assert!(bound.matches_record(&manual).is_err());
        assert!(f
            .store
            .gap_acquisition_binding(id.as_str())
            .unwrap()
            .unwrap()
            .acquisition_id()
            .is_none());
        assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 1);
    }

    #[test]
    fn provisioning_wait_preserves_grant_and_never_debits_until_activation() {
        let (mut f, p, id) = gap_fixture();
        let role = f.intent.role_id.clone();
        f.mutate(|state| state.roles.get_mut(&role).unwrap().state = RoleState::Provisioning);
        let bound = f
            .store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        assert!(matches!(
            bound.validate_current(&f.state, f.now),
            Err(GapAcquisitionError::WaitingForRole)
        ));
        assert!(matches!(
            f.store.create_gap_acquisition(id.as_str(), &suite(), f.now),
            Err(GapAcquisitionError::WaitingForRole)
        ));
        assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 0);
        f.mutate(|state| state.roles.get_mut(&role).unwrap().state = RoleState::Active);
        let (child, _) = f
            .store
            .create_gap_acquisition(id.as_str(), &suite(), f.now)
            .unwrap();
        assert_eq!(child.binding().grant, bound.authority.grant);
    }

    #[test]
    fn changed_epoch_while_waiting_cannot_rebind_authority() {
        let (mut f, p, id) = gap_fixture();
        let role = f.intent.role_id.clone();
        let responsibility = f.intent.responsibility_id.clone();
        f.mutate(|state| state.roles.get_mut(&role).unwrap().state = RoleState::Provisioning);
        f.store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        f.mutate(|state| {
            state.roles.get_mut(&role).unwrap().state = RoleState::Active;
            state
                .responsibilities
                .get_mut(&responsibility)
                .unwrap()
                .epoch += 1;
            for assignment in state.assignments.values_mut() {
                assignment.epoch += 1;
            }
        });
        assert!(matches!(
            f.store.create_gap_acquisition(id.as_str(), &suite(), f.now),
            Err(GapAcquisitionError::AuthorityChanged)
        ));
        assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 0);
    }

    #[test]
    fn requirement_drift_is_refused_before_first_binding_and_before_creation() {
        let (mut f, p, id) = gap_fixture();
        let req = f.intent.requirement_id.clone();
        let original = f.state.requirements[&req].summary.clone();
        f.mutate(|state| {
            state
                .requirements
                .get_mut(&req)
                .unwrap()
                .summary
                .push_str(" changed")
        });
        assert!(matches!(
            f.store
                .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now),
            Err(GapAcquisitionError::AuthorityChanged)
        ));
        f.mutate(|state| state.requirements.get_mut(&req).unwrap().summary = original);
        f.store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        f.mutate(|state| {
            state
                .requirements
                .get_mut(&req)
                .unwrap()
                .acceptance
                .push_str(" changed")
        });
        assert!(matches!(
            f.store.create_gap_acquisition(id.as_str(), &suite(), f.now),
            Err(GapAcquisitionError::AuthorityChanged)
        ));
        assert_eq!(f.store.acquisition_counters().unwrap().total_intents, 0);
    }

    #[test]
    fn suite_drift_and_wrong_parent_child_link_are_refused() {
        let (mut f, p, id) = gap_fixture();
        let bound = f
            .store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        let mut changed = suite();
        changed.scope.push_str(" changed");
        assert!(!bound.suite_matches(&changed));
        assert!(matches!(
            f.store.create_gap_acquisition(id.as_str(), &changed, f.now),
            Err(GapAcquisitionError::InvalidRecord)
        ));
        let (child, _) = f
            .store
            .create_gap_acquisition(id.as_str(), &suite(), f.now)
            .unwrap();
        let linked = f
            .store
            .gap_acquisition_binding(id.as_str())
            .unwrap()
            .unwrap();
        f.mutate(|state| {
            state.capability_gaps.get_mut(&id).unwrap().status = CapabilityGapStatus::Acquiring {
                acquisition_id: child.id().into(),
            }
        });
        linked.validate_current(&f.state, f.now).unwrap();
        f.mutate(|state| {
            state.capability_gaps.get_mut(&id).unwrap().status = CapabilityGapStatus::Acquiring {
                acquisition_id: "another-child".into(),
            }
        });
        assert!(matches!(
            linked.validate_current(&f.state, f.now),
            Err(GapAcquisitionError::AuthorityChanged)
        ));
    }

    #[test]
    fn tampered_binding_and_oversized_database_rows_are_rejected() {
        let (f, p, id) = gap_fixture();
        f.store
            .bind_gap_acquisition(id.as_str(), &p, &policy(), &suite(), f.now)
            .unwrap();
        f.store.lock().execute("UPDATE capability_gap_acquisitions SET acquisition_id='another-child' WHERE gap_id=?1",[id.as_str()]).unwrap();
        assert!(matches!(
            f.store.gap_acquisition_binding(id.as_str()),
            Err(GapAcquisitionError::InvalidRecord)
        ));
        f.store.lock().execute("UPDATE capability_gap_acquisitions SET acquisition_id=NULL,binding_json=?1 WHERE gap_id=?2",rusqlite::params!["x".repeat(super::MAX_BINDING_BYTES+1),id.as_str()]).unwrap();
        assert!(matches!(
            f.store.gap_acquisition_binding(id.as_str()),
            Err(GapAcquisitionError::InvalidRecord)
        ));
    }

    #[test]
    fn initialization_requires_no_acquisition_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            Store::open(dir.path().join("disabled.sqlite"), OrgId::new("disabled")).unwrap();
        store.init_gap_acquisitions().unwrap();
        assert!(store.gap_acquisition_binding("absent").unwrap().is_none());
    }
}
