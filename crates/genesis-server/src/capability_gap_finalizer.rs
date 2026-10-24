//! Opt-in finalization of verified capability gaps: `acquired` → `installed`.
//!
//! After the gap driver (`capability_gap_driver`) records an independently
//! verified acquisition, this coordinator takes that exact artifact the rest of
//! the way inside the same organization:
//!
//! 1. **Freeze** one finalization intent per gap: the acquired artifact pin,
//!    the operator's trusted publication text and acceptance suite, and the
//!    requesting role's current requirement/assignment grant.
//! 2. **Publish** through the existing trusted path (`verification_ref:
//!    acquisition:<id>`; the server recomputes the proof) and confirm the
//!    registry holds exactly that pin before recording `publication_ref`.
//! 3. **Install** the confirmed manifest locally (one kernel transaction).
//! 4. **Check** it with the existing local-acceptance jobs; the capability
//!    worker owns every provider effect.
//! 5. **Accept** from the persisted run evidence and, in the same transaction,
//!    record the gap `installed`, which wakes the planner to attach it.
//!
//! The exact acquired pin is required at every step; adoption's independent
//! discovery (which excludes the local publisher) is not used. Current
//! authority is rechecked before each effect and before applying results. A
//! run whose outcome is unknown, or any other uncertainty, stops at attention
//! with its IDs kept; nothing is replayed or turned into a refusal. Only a
//! semantic rejection by the local verifier, or lost authority, refuses the
//! gap, and then the still-checking installation is closed in the same
//! transaction. Publication is safe to repeat: the registry stores an
//! immutable version and returns the stored manifest for an identical one.

use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_authority::{authorize_current, authorize_job};
use crate::capability_evidence::EvidencePolicyError;
use crate::capability_runtime::CapabilityRunPhase;
use crate::capability_store::{self, LocalAcceptance};
use crate::error::{ApiError, ApiResult};
use crate::routes::capabilities::{self as caps, ChecksBody, PublishBody};
use crate::store::{EventMeta, Store, StoreError, StoreResult};
use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use genesis_core::capability::{ActorArtifact, CapabilityManifest, VerificationReport};
use genesis_core::engine::{self, Outcome};
use genesis_core::ids::*;
use genesis_core::lifecycle::{CapabilityState, InstallationStatus, RequirementStatus};
use genesis_core::model::{CapabilityGap, CapabilityGapStatus, CapabilityRef, OrgState};
use genesis_verification::ArtifactPin;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use tokio::task::JoinHandle;

// ---------------------------------------------------------------------------
// Trusted operator policy
// ---------------------------------------------------------------------------

/// Trusted publication text and local acceptance suite per capability ID.
/// Loaded from operator configuration; never from HTTP or the planner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapFinalizationPolicy {
    pub rules: Vec<GapFinalizationRule>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapFinalizationRule {
    pub capability_id: CapabilityId,
    /// Backend oracle suite for local acceptance of the installed artifact.
    pub suite_id: String,
    pub intent: String,
    #[serde(default)]
    pub applicability_conditions: Vec<String>,
    #[serde(default)]
    pub known_limitations: Vec<String>,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    /// Scopes the requesting role must hold to evaluate and later use it.
    pub required_permissions: BTreeSet<String>,
    #[serde(default)]
    pub responsibility_template: Option<String>,
    #[serde(default)]
    pub workflow_procedure: Option<String>,
    #[serde(default = "empty_object")]
    pub local_configuration_schema: serde_json::Value,
}

fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

fn text_ok(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(|c| c.is_control() && c != '\n')
}

impl GapFinalizationPolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.rules.is_empty() && self.rules.len() <= 32,
            "gap finalization policy needs 1-32 rules"
        );
        let mut seen = BTreeSet::new();
        for rule in &self.rules {
            anyhow::ensure!(
                seen.insert(rule.capability_id.clone()),
                "duplicate gap finalization rule for {}",
                rule.capability_id
            );
            anyhow::ensure!(
                text_ok(&rule.suite_id, 256) && text_ok(&rule.intent, 2000),
                "rule {} needs a suite_id and an intent",
                rule.capability_id
            );
            anyhow::ensure!(
                rule.required_permissions.iter().all(|p| text_ok(p, 128))
                    && rule
                        .applicability_conditions
                        .iter()
                        .chain(&rule.known_limitations)
                        .all(|t| text_ok(t, 1000)),
                "rule {} has an empty or oversized entry",
                rule.capability_id
            );
        }
        Ok(())
    }

    pub fn rule(&self, capability_id: &CapabilityId) -> Option<&GapFinalizationRule> {
        self.rules
            .iter()
            .find(|r| &r.capability_id == capability_id)
    }
}

/// The manifest this organization publishes for `pin`. Verification fields
/// are placeholders: publication replaces them with recomputed evidence.
pub fn manifest_for(
    org: &OrgId,
    rule: &GapFinalizationRule,
    pin: &ArtifactPin,
) -> CapabilityManifest {
    CapabilityManifest {
        capability_id: CapabilityId::new(pin.capability_id.clone()),
        version: pin.capability_version.clone(),
        publisher: org.clone(),
        source_digest: pin.source_sha256.clone(),
        intent: rule.intent.clone(),
        applicability_conditions: rule.applicability_conditions.clone(),
        known_limitations: rule.known_limitations.clone(),
        input_schema: rule.input_schema.clone(),
        output_schema: rule.output_schema.clone(),
        required_permissions: rule.required_permissions.clone(),
        artifact: ActorArtifact {
            actor_id: pin.actor_id.clone(),
            build_id: pin.build_id.clone(),
            build_number: pin.build_number.clone(),
        },
        role_template: None,
        responsibility_template: rule.responsibility_template.clone(),
        workflow_procedure: rule.workflow_procedure.clone(),
        local_configuration_schema: rule.local_configuration_schema.clone(),
        acceptance_suite_ref: rule.suite_id.clone(),
        verification: VerificationReport {
            passed: false,
            cases: vec![],
            tested_environment: "backend proof required".into(),
            limitations: vec![],
            oracle_ref: "backend proof required".into(),
            verified_at: DateTime::<Utc>::UNIX_EPOCH,
        },
        provenance: vec![],
        state: CapabilityState::Approved,
        published_at: None,
    }
}

// ---------------------------------------------------------------------------
// Durable record
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizerState {
    /// Progressing; the stage follows from which child IDs are recorded.
    Active,
    /// Stopped for an operator; IDs are retained and nothing is replayed.
    Attention,
    Installed,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizerCode {
    /// Policy rule, suite or registry/executor configuration missing or changed.
    Configuration,
    /// The acquisition is no longer verified for this gap.
    Acquisition,
    /// Requirement/assignment authority or the role's permissions changed.
    Authority,
    /// Registry publication failed or did not confirm the exact pin.
    Publication,
    /// Local installation could not be created or does not match the pin.
    Installation,
    /// A check run failed or its provider outcome is unknown.
    Checks,
    /// Stored records disagree with the frozen intent.
    InvalidRecord,
}

/// One frozen finalization intent and its child identities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizationRecord {
    pub schema_version: u32,
    pub gap_id: CapabilityGapId,
    pub acquisition_id: String,
    pub capability: CapabilityRef,
    pub artifact: ArtifactPin,
    /// Exact pre-publication manifest; its digest binds every later step.
    pub manifest: CapabilityManifest,
    pub manifest_sha256: String,
    pub suite_id: String,
    pub suite_sha256: String,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub requirement_digest: String,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub assignment_id: AssignmentId,
    pub epoch: u64,
    pub frozen_at: DateTime<Utc>,
    pub publication_ref: Option<String>,
    pub installation_id: Option<InstallationId>,
    pub job_ids: Vec<JobId>,
    pub state: FinalizerState,
    pub code: Option<FinalizerCode>,
    pub updated_at: DateTime<Utc>,
}

impl FinalizationRecord {
    pub fn client_request_id(&self) -> String {
        format!("gap-finalize:{}", self.gap_id)
    }
    fn publication_ref(&self) -> String {
        format!(
            "registry:{}@{}",
            self.capability.capability_id, self.capability.version
        )
    }
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capability_gap_finalizations (
    gap_id TEXT PRIMARY KEY,
    record_json TEXT NOT NULL CHECK(length(record_json) <= 262144));";

impl Store {
    pub fn init_gap_finalizer(&self) -> StoreResult<()> {
        self.lock().execute_batch(SCHEMA)?;
        Ok(())
    }

    pub fn gap_finalization(
        &self,
        gap: &CapabilityGapId,
    ) -> StoreResult<Option<FinalizationRecord>> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT record_json FROM capability_gap_finalizations WHERE gap_id=?1",
                params![gap.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(text) = text else { return Ok(None) };
        let record: FinalizationRecord = serde_json::from_str(&text)?;
        if record.gap_id != *gap {
            return Err(StoreError::NotFound("consistent gap finalization".into()));
        }
        Ok(Some(record))
    }
}

/// Insert a new record, or replace `prev` exactly (compare-and-swap on the
/// stored JSON), inside the caller's transaction.
fn put(
    tx: &Transaction<'_>,
    prev: Option<&FinalizationRecord>,
    next: &FinalizationRecord,
) -> StoreResult<()> {
    let text = serde_json::to_string(next)?;
    let changed = match prev {
        None => tx.execute(
            "INSERT OR IGNORE INTO capability_gap_finalizations(gap_id, record_json) VALUES (?1, ?2)",
            params![next.gap_id.as_str(), text],
        )?,
        Some(prev) => tx.execute(
            "UPDATE capability_gap_finalizations SET record_json=?2 WHERE gap_id=?1 AND record_json=?3",
            params![next.gap_id.as_str(), text, serde_json::to_string(prev)?],
        )?,
    };
    if changed != 1 {
        return Err(StoreError::RevisionConflict {
            expected: 0,
            actual: 0,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------------

/// What one step did, for tests and logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Frozen,
    Published,
    Installing,
    Checking,
    Waiting,
    Installed,
    Refused,
    Attention(FinalizerCode),
}

#[derive(Debug, thiserror::Error)]
pub enum FinalizerError {
    #[error("gap finalization is not configured")]
    Disabled,
    #[error("storage: {0}")]
    Store(#[from] StoreError),
    #[error("api: {0}")]
    Api(String),
}

impl From<ApiError> for FinalizerError {
    fn from(e: ApiError) -> Self {
        FinalizerError::Api(format!("{e:?}"))
    }
}

type Result<T> = std::result::Result<T, FinalizerError>;

fn digest<T: Serialize>(value: &T) -> String {
    genesis_verification::json_sha256(&serde_json::to_value(value).expect("serializable"))
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn gap_acquired(gap: &CapabilityGap) -> Option<(&str, &CapabilityRef, &Option<String>)> {
    match &gap.status {
        CapabilityGapStatus::Acquired {
            acquisition_id,
            capability,
            publication_ref,
        } => Some((acquisition_id, capability, publication_ref)),
        _ => None,
    }
}

/// Current authority for the frozen grant: the same role holds the same
/// assignment and epoch for the responsibility, the mission is active, and the
/// role holds every manifest permission.
fn authorized(state: &OrgState, r: &FinalizationRecord) -> bool {
    let now = now();
    let Ok(grant) = authorize_current(state, &r.role_id, &r.responsibility_id, None, now) else {
        return false;
    };
    grant.assignment_id == r.assignment_id
        && grant.epoch == r.epoch
        && r.manifest.required_permissions.iter().all(|s| {
            authorize_current(state, &r.role_id, &r.responsibility_id, Some(s), now).is_ok()
        })
        && state
            .capability_gaps
            .get(&r.gap_id)
            .is_some_and(|g| g.requirement_digest == r.requirement_digest && g.role_id == r.role_id)
        // The live requirement, not the gap's frozen copy: still assigned and
        // still the same work.
        && state.requirements.get(&r.requirement_id).is_some_and(|q| {
            q.status == RequirementStatus::Assigned
                && q.mission_id == r.mission_id
                && engine::requirement_digest(q) == r.requirement_digest
        })
}

/// Records whose installation is `installation`, if a finalizer owns it.
fn owner_of(
    store: &Store,
    installation: &InstallationId,
) -> StoreResult<Option<FinalizationRecord>> {
    let conn = store.lock();
    // A store that never enabled the finalizer has no table and no owners.
    let enabled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='capability_gap_finalizations')",
        [],
        |r| r.get(0),
    )?;
    if !enabled {
        return Ok(None);
    }
    let mut stmt = conn.prepare("SELECT record_json FROM capability_gap_finalizations")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for row in rows {
        let record: FinalizationRecord = serde_json::from_str(&row?)?;
        if record.installation_id.as_ref() == Some(installation) {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

/// Whether a finalizer owns `installation` (its public checks and acceptance
/// are then refused: the finalizer makes that decision atomically).
pub fn owns_installation(store: &Store, installation: &InstallationId) -> bool {
    // Unreadable records fail closed.
    owner_of(store, installation).map_or(true, |r| r.is_some())
}

/// Fence for the capability worker: a local-acceptance run on a
/// finalizer-owned installation may run, or have its result applied, only
/// while that finalizer's frozen grant and live requirement are current.
pub fn run_allowed(store: &Store, state: &OrgState, installation: &InstallationId) -> bool {
    match owner_of(store, installation) {
        Ok(None) => true,
        Ok(Some(r)) => r.state == FinalizerState::Active && authorized(state, &r),
        Err(_) => false,
    }
}

/// An accepted installation may complete the gap only with this record's
/// exact suite, the stored proof behind its report, the frozen pin, and
/// every proof job still authorized now.
fn accepted_matches(
    app: &AppState,
    state: &OrgState,
    r: &FinalizationRecord,
    id: &InstallationId,
) -> Acceptance {
    let Some(installation) = state.installations.get(id) else {
        return Acceptance::Mismatch;
    };
    let Ok(Some(proof)) = app.store.capability_acceptance(id) else {
        return Acceptance::Mismatch;
    };
    let pinned = app.store.capability_manifest(id).ok();
    if installation.status != InstallationStatus::Accepted
        || installation.acceptance_report_ref.as_deref() != Some(proof.report_ref.as_str())
        || proof.suite_id != r.suite_id
        || pinned.is_none_or(|m| pin_of(&m) != r.artifact)
    {
        return Acceptance::Mismatch;
    }
    let Some(manifest) = app.store.capability_manifest(id).ok() else {
        return Acceptance::Mismatch;
    };
    let at = now();
    for job_id in &proof.job_ids {
        let (Ok(job), Ok(run)) = (
            app.store.get_job(job_id),
            app.store.get_capability_run(job_id),
        ) else {
            return Acceptance::Mismatch;
        };
        if job.role_id != r.role_id || job.responsibility_id != r.responsibility_id {
            return Acceptance::Mismatch;
        }
        let ok = authorize_job(state, &job, run.required_scope.as_deref(), at).is_ok()
            && run
                .required_permissions
                .iter()
                .all(|s| authorize_job(state, &job, Some(s), at).is_ok())
            && caps::authorize_permissions(state, &job.role_id, &job.responsibility_id, &manifest)
                .is_ok();
        if !ok {
            return Acceptance::Unauthorized;
        }
    }
    // Recompute the verdict from the persisted runs; never trust the label.
    match caps::local_runs(app, id, &r.suite_id, &proof.job_ids)
        .ok()
        .and_then(|runs| {
            app.evidence_policy
                .accept_suite(&r.suite_id, &r.artifact, &runs)
                .ok()
        }) {
        Some(evidence) if evidence.reference == proof.report_ref => Acceptance::Matches,
        _ => Acceptance::Mismatch,
    }
}

enum Acceptance {
    Matches,
    Mismatch,
    Unauthorized,
}

/// Whether the registry's stored manifest is this record's frozen manifest,
/// apart from what publication itself sets (recomputed proof, its suite
/// reference and provenance, state and timestamp).
fn same_contract(stored: &CapabilityManifest, frozen: &CapabilityManifest) -> bool {
    let normalize = |m: &CapabilityManifest| {
        let mut m = m.clone();
        m.verification = frozen.verification.clone();
        m.acceptance_suite_ref = frozen.acceptance_suite_ref.clone();
        m.provenance = vec![];
        m.state = CapabilityState::Approved;
        m.published_at = None;
        m
    };
    normalize(stored) == normalize(frozen) && stored.verification.passed
}

/// The gap still names the frozen acquisition and exact capability.
fn gap_matches(state: &OrgState, r: &FinalizationRecord) -> bool {
    state
        .capability_gaps
        .get(&r.gap_id)
        .and_then(gap_acquired)
        .is_some_and(|(acq, cap, _)| acq == r.acquisition_id && *cap == r.capability)
}

fn save(app: &AppState, prev: &FinalizationRecord, next: &FinalizationRecord) -> Result<()> {
    let conn = app.store.lock();
    let tx = conn.unchecked_transaction().map_err(StoreError::from)?;
    put(&tx, Some(prev), next)?;
    tx.commit().map_err(StoreError::from)?;
    Ok(())
}

fn attention(app: &AppState, r: &FinalizationRecord, code: FinalizerCode) -> Result<Step> {
    if r.state == FinalizerState::Attention && r.code == Some(code) {
        return Ok(Step::Attention(code));
    }
    let mut next = r.clone();
    next.state = FinalizerState::Attention;
    next.code = Some(code);
    next.updated_at = now();
    save(app, r, &next)?;
    tracing::warn!(gap = %r.gap_id, ?code, "gap finalization needs attention");
    Ok(Step::Attention(code))
}

/// Refuse the gap and close a still-checking installation of ours, atomically.
/// Kernel lock held by the caller.
fn refuse(
    app: &AppState,
    state: &OrgState,
    r: &FinalizationRecord,
    reason: &str,
    code: FinalizerCode,
) -> Result<Step> {
    let mut outcome = Outcome {
        state: state.clone(),
        effects: vec![],
    };
    if let Some(id) = &r.installation_id {
        if state
            .installations
            .get(id)
            .is_some_and(|i| i.status == InstallationStatus::Checking)
        {
            let closed = engine::finish_installation(
                &outcome.state,
                id,
                false,
                &format!("gap-finalization-stopped:{}", r.gap_id),
                vec![reason.to_owned()],
            )
            .map_err(|e| FinalizerError::Api(e.to_string()))?;
            outcome.state = closed.state;
            outcome.effects.extend(closed.effects);
        }
    }
    let refused = engine::update_capability_gap_at(
        &outcome.state,
        &r.gap_id,
        CapabilityGapStatus::Refused {
            reason: reason.to_owned(),
            acquisition_id: Some(r.acquisition_id.clone()),
        },
        now(),
    )
    .map_err(|e| FinalizerError::Api(e.to_string()))?;
    outcome.state = refused.state;
    outcome.effects.extend(refused.effects);
    let mut next = r.clone();
    next.state = FinalizerState::Refused;
    next.code = Some(code);
    next.updated_at = now();
    commit(app, state, outcome, r, &next, None)?;
    Ok(Step::Refused)
}

/// Extra writes committed with a stage change.
type ExtraWrite = dyn Fn(&Transaction<'_>) -> StoreResult<()>;

fn commit(
    app: &AppState,
    base: &OrgState,
    outcome: Outcome,
    prev: &FinalizationRecord,
    next: &FinalizationRecord,
    extra: Option<&ExtraWrite>,
) -> Result<()> {
    let events = app.store.commit_state(
        base.revision,
        &outcome.state,
        outcome.effects,
        &EventMeta::mission(prev.mission_id.clone()).caused_by(prev.gap_id.to_string()),
        vec![],
        |tx| {
            if let Some(extra) = extra {
                extra(tx)?;
            }
            put(tx, Some(prev), next)
        },
    )?;
    app.broadcast(&events);
    Ok(())
}

/// Advance one gap by at most one stage. Provider and registry I/O happens
/// without the kernel lock; every state change re-reads state under it.
pub async fn step(app: &AppState, gap_id: &CapabilityGapId) -> Result<Step> {
    let policy = app
        .gap_finalization_policy
        .clone()
        .ok_or(FinalizerError::Disabled)?;
    let Some(record) = app.store.gap_finalization(gap_id)? else {
        return freeze(app, &policy, gap_id).await;
    };
    if matches!(
        record.state,
        FinalizerState::Installed | FinalizerState::Refused | FinalizerState::Attention
    ) {
        return Ok(match record.state {
            FinalizerState::Installed => Step::Installed,
            FinalizerState::Refused => Step::Refused,
            _ => Step::Attention(record.code.unwrap_or(FinalizerCode::InvalidRecord)),
        });
    }
    let Some(suite) = app.evidence_policy.suite(&record.suite_id) else {
        return attention(app, &record, FinalizerCode::Configuration);
    };
    if digest(suite) != record.suite_sha256
        || policy
            .rule(&record.capability.capability_id)
            .is_none_or(|rule| manifest_for(&app.org_id, rule, &record.artifact) != record.manifest)
        || digest(&record.manifest) != record.manifest_sha256
    {
        return attention(app, &record, FinalizerCode::Configuration);
    }
    if record.publication_ref.is_none() {
        return publish(app, &record).await;
    }
    if record.installation_id.is_none() {
        return install(app, &record).await;
    }
    if record.job_ids.is_empty() {
        return checks(app, &record).await;
    }
    accept(app, &record).await
}

async fn freeze(
    app: &AppState,
    policy: &GapFinalizationPolicy,
    gap_id: &CapabilityGapId,
) -> Result<Step> {
    let state = app.store.current_state()?;
    let Some(gap) = state.capability_gaps.get(gap_id) else {
        return Ok(Step::Idle);
    };
    let Some((acquisition_id, capability, _)) = gap_acquired(gap) else {
        return Ok(Step::Idle);
    };
    let Some(rule) = policy.rule(&gap.capability_id) else {
        return Ok(Step::Idle); // not ours to finalize; stays acquired for an operator
    };
    let Some(suite) = app.evidence_policy.suite(&rule.suite_id) else {
        return Ok(Step::Idle);
    };
    // Takes the kernel lock itself; the proof is recomputed from the record.
    let Ok(verified) = app.acquisition_runtime.verified(acquisition_id).await else {
        return Ok(Step::Idle);
    };
    let pin = verified.artifact;
    if pin.capability_id != capability.capability_id.as_str()
        || pin.capability_version != capability.version
    {
        return Ok(Step::Idle);
    }
    let manifest = manifest_for(&app.org_id, rule, &pin);
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    let Some(gap) = state.capability_gaps.get(gap_id) else {
        return Ok(Step::Idle);
    };
    if gap_acquired(gap).is_none_or(|(acq, cap, _)| acq != acquisition_id || cap != capability) {
        return Ok(Step::Idle);
    }
    let Ok(grant) = authorize_current(&state, &gap.role_id, &gap.responsibility_id, None, now())
    else {
        return Ok(Step::Idle);
    };
    let at = now();
    let record = FinalizationRecord {
        schema_version: 1,
        gap_id: gap_id.clone(),
        acquisition_id: acquisition_id.to_owned(),
        capability: capability.clone(),
        artifact: pin,
        manifest_sha256: digest(&manifest),
        manifest,
        suite_id: rule.suite_id.clone(),
        suite_sha256: digest(suite),
        mission_id: gap.mission_id.clone(),
        requirement_id: gap.requirement_id.clone(),
        requirement_digest: gap.requirement_digest.clone(),
        role_id: gap.role_id.clone(),
        responsibility_id: gap.responsibility_id.clone(),
        assignment_id: grant.assignment_id,
        epoch: grant.epoch,
        frozen_at: at,
        publication_ref: None,
        installation_id: None,
        job_ids: vec![],
        state: FinalizerState::Active,
        code: None,
        updated_at: at,
    };
    let conn = app.store.lock();
    let tx = conn.unchecked_transaction().map_err(StoreError::from)?;
    // A concurrent freeze already won: keep the first record unchanged.
    let _ = put(&tx, None, &record);
    tx.commit().map_err(StoreError::from)?;
    Ok(Step::Frozen)
}

fn pin_of(manifest: &CapabilityManifest) -> ArtifactPin {
    caps::artifact(manifest)
}

async fn publish(app: &AppState, r: &FinalizationRecord) -> Result<Step> {
    let state = app.store.current_state()?;
    if !gap_matches(&state, r) {
        return attention(app, r, FinalizerCode::Acquisition);
    }
    if !authorized(&state, r) {
        let _kernel = app.kernel.lock().await;
        let state = app.store.current_state()?;
        if !authorized(&state, r) {
            return refuse(
                app,
                &state,
                r,
                "The requesting role's authority changed before publication.",
                FinalizerCode::Authority,
            );
        }
        return Ok(Step::Waiting);
    }
    let Some(registry) = app.registry.as_ref() else {
        return attention(app, r, FinalizerCode::Configuration);
    };
    let body: PublishBody = serde_json::from_value(serde_json::json!({
        "manifest": r.manifest,
        "verification_ref": format!("acquisition:{}", r.acquisition_id),
    }))
    .map_err(|e| FinalizerError::Api(e.to_string()))?;
    // Idempotent: an identical immutable version returns the stored manifest.
    let published = caps::publish(State(app.clone()), Principal::Operator, Json(body)).await;
    if published.is_err() {
        // Ambiguous or definite, only a confirming read can tell. Publication
        // is repeated only after an operator retry (`/finalization/retry`).
        if !matches!(registry.manifest(&r.capability.capability_id, &r.capability.version).await,
            Ok(m) if same_contract(&m, &r.manifest))
        {
            return attention(app, r, FinalizerCode::Publication);
        }
    }
    // Confirm the registry holds exactly this pin from this organization.
    let confirmed = registry
        .manifest(&r.capability.capability_id, &r.capability.version)
        .await;
    let Ok(confirmed) = confirmed else {
        return attention(app, r, FinalizerCode::Publication);
    };
    if pin_of(&confirmed) != r.artifact
        || confirmed.publisher != app.org_id
        || !same_contract(&confirmed, &r.manifest)
    {
        return attention(app, r, FinalizerCode::Publication);
    }
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    let Some((_, _, existing)) = state.capability_gaps.get(&r.gap_id).and_then(gap_acquired) else {
        return attention(app, r, FinalizerCode::Acquisition);
    };
    let reference = r.publication_ref();
    let outcome = match existing {
        Some(other) if *other != reference => return attention(app, r, FinalizerCode::Publication),
        Some(_) => Outcome {
            state: state.clone(),
            effects: vec![],
        },
        None => engine::update_capability_gap_at(
            &state,
            &r.gap_id,
            CapabilityGapStatus::Acquired {
                acquisition_id: r.acquisition_id.clone(),
                capability: r.capability.clone(),
                publication_ref: Some(reference.clone()),
            },
            now(),
        )
        .map_err(|e| FinalizerError::Api(e.to_string()))?,
    };
    let mut next = r.clone();
    next.publication_ref = Some(reference);
    next.updated_at = now();
    commit(app, &state, outcome, r, &next, None)?;
    Ok(Step::Published)
}

async fn install(app: &AppState, r: &FinalizationRecord) -> Result<Step> {
    let Some(registry) = app.registry.as_ref() else {
        return attention(app, r, FinalizerCode::Configuration);
    };
    let Ok(manifest) = registry
        .manifest(&r.capability.capability_id, &r.capability.version)
        .await
    else {
        return attention(app, r, FinalizerCode::Publication);
    };
    if pin_of(&manifest) != r.artifact || manifest.publisher != app.org_id {
        return attention(app, r, FinalizerCode::Installation);
    }
    if caps::check_manifest(app, &manifest).is_err() {
        return attention(app, r, FinalizerCode::Installation);
    }
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    if !gap_matches(&state, r) {
        return attention(app, r, FinalizerCode::Acquisition);
    }
    if !authorized(&state, r) {
        return refuse(
            app,
            &state,
            r,
            "The requesting role's authority changed before installation.",
            FinalizerCode::Authority,
        );
    }
    // Recover an installation of exactly this pin left by an interrupted pass
    // (or made by an operator), never one owned by an adoption intent.
    let reference = manifest.reference();
    let existing = state.installations.values().find(|i| {
        i.capability == reference
            && matches!(
                i.status,
                InstallationStatus::Checking | InstallationStatus::Accepted
            )
    });
    let mut next = r.clone();
    next.updated_at = now();
    if let Some(existing) = existing {
        let pinned = app
            .store
            .capability_manifest(&existing.installation_id)
            .ok();
        let owned = app
            .store
            .adoption_owns_installation(&existing.installation_id)
            .unwrap_or(true);
        if owned || pinned.is_none_or(|m| pin_of(&m) != r.artifact) {
            return attention(app, r, FinalizerCode::Installation);
        }
        // An accepted one is reused only with this record's exact suite,
        // proof and current authority; anything weaker stops for an operator.
        if existing.status == InstallationStatus::Accepted {
            match accepted_matches(app, &state, r, &existing.installation_id) {
                Acceptance::Matches => {}
                Acceptance::Mismatch => return attention(app, r, FinalizerCode::Installation),
                Acceptance::Unauthorized => return attention(app, r, FinalizerCode::Authority),
            }
        }
        next.installation_id = Some(existing.installation_id.clone());
        let unchanged = Outcome {
            state: state.clone(),
            effects: vec![],
        };
        commit(app, &state, unchanged, r, &next, None)?;
        return Ok(Step::Installing);
    }
    let (outcome, id) =
        match engine::begin_installation(&state, &manifest, serde_json::json!({}), now()) {
            Ok(v) => v,
            Err(_) => return attention(app, r, FinalizerCode::Installation),
        };
    next.installation_id = Some(id.clone());
    let pin_manifest = manifest.clone();
    commit(
        app,
        &state,
        outcome,
        r,
        &next,
        Some(&move |tx| capability_store::insert_pin(tx, &id, &pin_manifest)),
    )?;
    Ok(Step::Installing)
}

async fn checks(app: &AppState, r: &FinalizationRecord) -> Result<Step> {
    let installation_id = r.installation_id.clone().expect("checked by caller");
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    if !gap_matches(&state, r) {
        return attention(app, r, FinalizerCode::Acquisition);
    }
    if !authorized(&state, r) {
        return refuse(
            app,
            &state,
            r,
            "The requesting role's authority changed before local checks.",
            FinalizerCode::Authority,
        );
    }
    match state.installations.get(&installation_id).map(|i| i.status) {
        Some(InstallationStatus::Checking) => {}
        Some(InstallationStatus::Accepted) => {
            return accepted_shortcut(app, &state, r, &installation_id)
        }
        _ => return attention(app, r, FinalizerCode::Installation),
    }
    let body = ChecksBody {
        client_request_id: r.client_request_id(),
        role_id: r.role_id.clone(),
        responsibility_id: r.responsibility_id.clone(),
        suite_id: r.suite_id.clone(),
    };
    let plan = match caps::plan_checks(app, &state, &installation_id, &body, None) {
        Ok(plan) => plan,
        Err(_) => return attention(app, r, FinalizerCode::Checks),
    };
    let mut next = r.clone();
    next.job_ids = plan.job_ids.clone();
    next.updated_at = now();
    let events = app.store.commit_state(
        state.revision,
        &state,
        plan.effects.clone(),
        &EventMeta::mission(r.mission_id.clone()).caused_by(r.client_request_id()),
        vec![],
        |tx| {
            caps::persist_checks(tx, &plan)?;
            put(tx, Some(r), &next)
        },
    )?;
    app.broadcast(&events);
    Ok(Step::Checking)
}

async fn accept(app: &AppState, r: &FinalizationRecord) -> Result<Step> {
    let installation_id = r.installation_id.clone().expect("checked by caller");
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    if !gap_matches(&state, r) {
        return attention(app, r, FinalizerCode::Acquisition);
    }
    if !authorized(&state, r) {
        return refuse(
            app,
            &state,
            r,
            "The requesting role's authority or requirement changed during local checks.",
            FinalizerCode::Authority,
        );
    }
    match state.installations.get(&installation_id).map(|i| i.status) {
        Some(InstallationStatus::Checking) => {}
        Some(InstallationStatus::Accepted) => {
            return accepted_shortcut(app, &state, r, &installation_id)
        }
        _ => return attention(app, r, FinalizerCode::Installation),
    }
    let mut pending = false;
    for id in &r.job_ids {
        let (Ok(run), Ok(job)) = (app.store.get_capability_run(id), app.store.get_job(id)) else {
            return attention(app, r, FinalizerCode::InvalidRecord);
        };
        if run.validate(&job).is_err()
            || !run.purpose.is_acceptance_for(&installation_id, &r.suite_id)
        {
            return attention(app, r, FinalizerCode::InvalidRecord);
        }
        if run.failure.is_some()
            || matches!(
                run.phase,
                CapabilityRunPhase::Failed
                    | CapabilityRunPhase::NeedsReconciliation
                    | CapabilityRunPhase::Rejected
            )
        {
            // Includes unknown provider outcomes: kept for reconciliation.
            return attention(app, r, FinalizerCode::Checks);
        }
        pending |= run.phase != CapabilityRunPhase::Collected;
    }
    if pending {
        return Ok(Step::Waiting);
    }
    // Same authority checks as the acceptance endpoint, before applying.
    let manifest = match app.store.capability_manifest(&installation_id) {
        Ok(m) if pin_of(&m) == r.artifact => m,
        _ => return attention(app, r, FinalizerCode::InvalidRecord),
    };
    let at = now();
    for id in &r.job_ids {
        let job = app.store.get_job(id)?;
        let run = app
            .store
            .get_capability_run(id)
            .map_err(|e| FinalizerError::Api(e.to_string()))?;
        let ok = authorize_job(&state, &job, run.required_scope.as_deref(), at).is_ok()
            && run
                .required_permissions
                .iter()
                .all(|s| authorize_job(&state, &job, Some(s), at).is_ok())
            && caps::authorize_permissions(&state, &job.role_id, &job.responsibility_id, &manifest)
                .is_ok();
        if !ok || !authorized(&state, r) {
            return refuse(
                app,
                &state,
                r,
                "The requesting role's authority changed before local acceptance.",
                FinalizerCode::Authority,
            );
        }
    }
    let runs = match caps::local_runs(app, &installation_id, &r.suite_id, &r.job_ids) {
        Ok(runs) => runs,
        Err(_) => return attention(app, r, FinalizerCode::InvalidRecord),
    };
    match app
        .evidence_policy
        .accept_suite(&r.suite_id, &r.artifact, &runs)
    {
        Ok(evidence) => {
            let proof = LocalAcceptance {
                suite_id: r.suite_id.clone(),
                job_ids: r.job_ids.clone(),
                report_ref: evidence.reference.clone(),
            };
            installed(app, &state, r, Some((evidence.reference, proof)))
        }
        Err(EvidencePolicyError::Rejected(_)) => refuse(
            app,
            &state,
            r,
            "Independent local verification rejected the acquired capability.",
            FinalizerCode::Checks,
        ),
        Err(_) => attention(app, r, FinalizerCode::Configuration),
    }
}

/// An installation of ours is already accepted: complete only if its proof is
/// this record's exact suite and every proof job is still authorized.
fn accepted_shortcut(
    app: &AppState,
    state: &OrgState,
    r: &FinalizationRecord,
    id: &InstallationId,
) -> Result<Step> {
    if !authorized(state, r) {
        return refuse(
            app,
            state,
            r,
            "The requesting role's authority changed before completion.",
            FinalizerCode::Authority,
        );
    }
    match accepted_matches(app, state, r, id) {
        Acceptance::Matches => installed(app, state, r, None),
        Acceptance::Mismatch => attention(app, r, FinalizerCode::Installation),
        Acceptance::Unauthorized => refuse(
            app,
            state,
            r,
            "The requesting role's authority changed before completion.",
            FinalizerCode::Authority,
        ),
    }
}

/// Accept (when `proof` is given) and record the gap installed, atomically.
fn installed(
    app: &AppState,
    state: &OrgState,
    r: &FinalizationRecord,
    proof: Option<(String, LocalAcceptance)>,
) -> Result<Step> {
    let installation_id = r.installation_id.clone().expect("installation recorded");
    let mut outcome = Outcome {
        state: state.clone(),
        effects: vec![],
    };
    if let Some((reference, _)) = &proof {
        let accepted =
            engine::finish_installation(&outcome.state, &installation_id, true, reference, vec![])
                .map_err(|e| FinalizerError::Api(e.to_string()))?;
        outcome.state = accepted.state;
        outcome.effects.extend(accepted.effects);
    }
    // The kernel checks the installation is accepted and matches exactly.
    let done = match engine::update_capability_gap_at(
        &outcome.state,
        &r.gap_id,
        CapabilityGapStatus::Installed {
            acquisition_id: r.acquisition_id.clone(),
            capability: r.capability.clone(),
            installation_id: installation_id.clone(),
        },
        now(),
    ) {
        Ok(done) => done,
        Err(_) => return attention(app, r, FinalizerCode::Installation),
    };
    outcome.state = done.state;
    outcome.effects.extend(done.effects);
    let mut next = r.clone();
    next.state = FinalizerState::Installed;
    next.code = None;
    next.updated_at = now();
    let save_proof = proof.map(|(_, p)| p);
    let id = installation_id.clone();
    commit(
        app,
        state,
        outcome,
        r,
        &next,
        Some(&move |tx| match &save_proof {
            Some(p) => capability_store::save_acceptance(tx, &id, p),
            None => Ok(()),
        }),
    )?;
    Ok(Step::Installed)
}

/// Step every verified gap once (at most 16 live gaps exist).
pub async fn drain_once(app: &AppState) -> Result<BTreeMap<CapabilityGapId, Step>> {
    let mut out = BTreeMap::new();
    if app.gap_finalization_policy.is_none() {
        return Ok(out);
    }
    let state = app.store.current_state()?;
    let mut ids: Vec<_> = state
        .capability_gaps
        .values()
        .filter(|g| matches!(g.status, CapabilityGapStatus::Acquired { .. }))
        .map(|g| (g.opened_in_revision, g.gap_id.clone()))
        .collect();
    ids.sort();
    for (_, id) in ids {
        // One gap's failure must not hold up the others; it is retried on the
        // next pass from its durable record.
        match step(app, &id).await {
            Ok(step) => {
                out.insert(id, step);
            }
            Err(e) => {
                tracing::warn!(gap = %id, error = %e, "gap finalization step failed; will retry")
            }
        }
    }
    Ok(out)
}

/// Background worker. A failed pass is logged and retried after a backoff
/// (each stage is an atomic local transaction or an idempotent read), so a
/// transient storage fault never stops finalization for good.
pub fn spawn(app: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut delay = Duration::from_secs(1);
        loop {
            tokio::time::sleep(delay).await;
            match drain_once(&app).await {
                Ok(_) => delay = Duration::from_secs(1),
                Err(e) => {
                    delay = (delay * 2).min(Duration::from_secs(60));
                    tracing::error!(error = %e, retry_in_s = delay.as_secs(), "gap finalization pass failed; retrying");
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Operator status
// ---------------------------------------------------------------------------

/// Operational view; the manifest, suite and evidence stay server-side.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FinalizationStatus {
    pub gap_id: CapabilityGapId,
    pub enabled: bool,
    pub state: Option<FinalizerState>,
    pub code: Option<FinalizerCode>,
    pub capability: Option<CapabilityRef>,
    pub publication_ref: Option<String>,
    pub installation_id: Option<InstallationId>,
    pub job_ids: Vec<JobId>,
    pub updated_at: Option<DateTime<Utc>>,
}

pub async fn status_route(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<FinalizationStatus>> {
    p.require(&[Principal::Operator])?;
    let id = CapabilityGapId::new(id);
    if !app.current_state()?.capability_gaps.contains_key(&id) {
        return Err(ApiError::not_found("capability gap"));
    }
    let record = app.store.gap_finalization(&id)?;
    Ok(Json(FinalizationStatus {
        gap_id: id,
        enabled: app.gap_finalization_policy.is_some(),
        state: record.as_ref().map(|r| r.state),
        code: record.as_ref().and_then(|r| r.code),
        capability: record.as_ref().map(|r| r.capability.clone()),
        publication_ref: record.as_ref().and_then(|r| r.publication_ref.clone()),
        installation_id: record.as_ref().and_then(|r| r.installation_id.clone()),
        job_ids: record
            .as_ref()
            .map(|r| r.job_ids.clone())
            .unwrap_or_default(),
        updated_at: record.map(|r| r.updated_at),
    }))
}

/// Operator recovery: return an attention record to `active` so the next pass
/// re-reads every child and decides again. Nothing is replayed by this call:
/// check jobs keep their keys, a failed or unknown run stops again at
/// attention, and a repeated publication can only return the stored version.
pub async fn retry_route(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<FinalizationStatus>> {
    p.require(&[Principal::Operator])?;
    let gap = CapabilityGapId::new(id.clone());
    let record = app
        .store
        .gap_finalization(&gap)?
        .ok_or_else(|| ApiError::not_found("gap finalization"))?;
    if record.state == FinalizerState::Attention {
        let mut next = record.clone();
        next.state = FinalizerState::Active;
        next.code = None;
        next.updated_at = now();
        save(&app, &record, &next).map_err(|e| ApiError::internal(e.to_string()))?;
    }
    status_route(State(app), p, Path(id)).await
}
