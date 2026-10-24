//! Opt-in adopt-before-acquire for capability gaps: `open` → `installed`
//! through Org B's own independent adoption, with synthesis only as an
//! explicit fallback after a definitive empty search.
//!
//! For each committed open gap whose capability has a trusted rule:
//!
//! 1. **Freeze** one intent per gap: the requirement digest, the requesting
//!    role's assignment and epoch, the rule's suite and its digest, and whether
//!    synthesis may follow an empty search. From this moment the gap driver
//!    leaves the gap alone.
//! 2. **Dispatch** exactly one adoption (`gap-adopt:<gap>` key, idempotent in
//!    the adoption store) after rechecking the live requirement and grant, and
//!    record its ID. The existing adoption worker does discovery (which
//!    excludes this organization's own publications), installation and B's
//!    frozen checks; nothing here talks to a provider.
//! 3. **Link**: when that adoption is accepted, the adoption transaction also
//!    records the gap `installed` with the exact selected pin and accepted
//!    installation (`engine::install_capability_gap_by_adoption_at`) and closes
//!    this record. The kernel wakes the planner to attach it.
//!
//! Only a persisted `no_candidate` result may release the gap to the ordinary
//! gap driver, and only when the rule allows it; release is terminal, so it
//! happens at most once. Registry errors, invalid candidates, failed or
//! unknown checks, rejection and lost authority all keep the adoption ID at
//! attention and never trigger synthesis.

use crate::app::AppState;
use crate::auth::Principal;
use crate::capability_adoption_store::{
    AdoptionAttention, AdoptionError, AdoptionPhase, AdoptionRecord, CreateAdoption,
};
use crate::capability_authority::authorize_current;
use crate::error::{ApiError, ApiResult};
use crate::store::{EventMeta, Store, StoreError, StoreResult};
use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use genesis_core::engine::{self, AdoptionInstall, Outcome};
use genesis_core::ids::*;
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{CapabilityGap, CapabilityGapStatus, CapabilityRef, OrgState};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use tokio::task::JoinHandle;

// ---------------------------------------------------------------------------
// Trusted operator policy
// ---------------------------------------------------------------------------

/// Which gaps try adoption first, with which acceptance suite. Loaded from
/// operator configuration; never from HTTP or the planner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapAdoptionPolicy {
    pub rules: Vec<GapAdoptionRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapAdoptionRule {
    /// The gap's requested capability ID. The adopted capability may have a
    /// different ID; B's suite decides whether it serves the need.
    pub capability_id: CapabilityId,
    /// B's own backend oracle suite for the adopted candidate.
    pub suite_id: String,
    /// After a definitive empty search, release the gap to the gap driver.
    #[serde(default)]
    pub synthesis_fallback: bool,
}

impl GapAdoptionPolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.rules.is_empty() && self.rules.len() <= 32,
            "gap adoption policy needs 1-32 rules"
        );
        let mut seen = BTreeSet::new();
        for rule in &self.rules {
            anyhow::ensure!(
                seen.insert(rule.capability_id.clone()),
                "duplicate gap adoption rule for {}",
                rule.capability_id
            );
            anyhow::ensure!(
                !rule.suite_id.trim().is_empty()
                    && rule.suite_id.len() <= 256
                    && !rule.suite_id.chars().any(char::is_control),
                "rule {} needs a suite_id",
                rule.capability_id
            );
        }
        Ok(())
    }

    pub fn rule(&self, capability_id: &CapabilityId) -> Option<&GapAdoptionRule> {
        self.rules
            .iter()
            .find(|r| &r.capability_id == capability_id)
    }
}

// ---------------------------------------------------------------------------
// Durable record
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapAdoptionState {
    /// Adoption owns the gap (before or after dispatch).
    Active,
    /// A definitive empty search released the gap to the gap driver.
    Released,
    /// The gap is installed from the accepted adoption.
    Installed,
    /// Stopped for an operator; the adoption ID is kept, nothing falls back.
    Attention,
    /// A definite outcome (rejection, lost authority, or an empty search
    /// without fallback) refused the kernel gap, which wakes the planner.
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapAdoptionCode {
    /// Rule, suite, registry or executor configuration missing or changed.
    Configuration,
    /// The requirement, assignment or epoch changed, or adoption was fenced.
    Authority,
    /// The registry failed or answered ambiguously.
    Registry,
    /// The registry offered an invalid or unusable candidate.
    Candidate,
    /// A check run failed or its provider outcome is unknown.
    Checks,
    /// B's suite rejected the selected candidate.
    Rejected,
    /// Nothing compatible was found and the rule has no synthesis fallback.
    NoCandidate,
    /// Another adoption holds this requirement, a limit was reached, or an
    /// installation conflicted.
    Conflict,
    /// Stored records disagree with the frozen intent.
    InvalidRecord,
}

/// One frozen adopt-first intent and its adoption identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapAdoptionRecord {
    pub schema_version: u32,
    pub gap_id: CapabilityGapId,
    pub capability_id: CapabilityId,
    pub suite_id: String,
    pub suite_sha256: String,
    pub synthesis_fallback: bool,
    pub mission_id: MissionId,
    pub requirement_id: RequirementId,
    pub requirement_digest: String,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub assignment_id: AssignmentId,
    pub epoch: u64,
    pub frozen_at: DateTime<Utc>,
    pub adoption_id: Option<String>,
    /// The exact pin and installation recorded on the gap, once installed.
    pub selected: Option<CapabilityRef>,
    pub installation_id: Option<InstallationId>,
    pub state: GapAdoptionState,
    pub code: Option<GapAdoptionCode>,
    pub updated_at: DateTime<Utc>,
    /// Operator retries so far; each gets a fresh adoption key.
    #[serde(default)]
    pub attempt: u32,
    /// Adoptions of earlier attempts, oldest first (kept for history).
    #[serde(default)]
    pub previous_adoption_ids: Vec<String>,
}

/// Operator retries allowed per gap after the first adoption.
pub const MAX_RETRIES: u32 = 2;

impl GapAdoptionCode {
    /// Stops a fresh adoption may resolve: a registry blip, a bad candidate
    /// that may have been replaced, changed configuration, an installation
    /// conflict, or failed/unknown checks (e.g. a restart during a check
    /// run). Nothing is replayed: the stopped adoption can only be fenced
    /// from here, keeps its runs and stays in history, and the operator's
    /// retry starts new checks under a new key. The cost is at most one
    /// duplicate check run per retry. Inconsistent records are never retried.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Registry | Self::Candidate | Self::Configuration | Self::Conflict | Self::Checks
        )
    }
}

impl GapAdoptionRecord {
    pub fn client_request_id(&self) -> String {
        match self.attempt {
            0 => format!("gap-adopt:{}", self.gap_id),
            n => format!("gap-adopt:{}:{n}", self.gap_id),
        }
    }

    fn intent(&self) -> CreateAdoption {
        CreateAdoption {
            client_request_id: self.client_request_id(),
            mission_id: self.mission_id.clone(),
            requirement_id: self.requirement_id.clone(),
            role_id: self.role_id.clone(),
            responsibility_id: self.responsibility_id.clone(),
            suite_id: self.suite_id.clone(),
            // The operator's trusted policy is the acknowledgement.
            provider_access_acknowledged: true,
            deadline: None,
        }
    }

    /// The adoption is this intent's child: same ID, key, requirement, grant
    /// and suite. A matching ID prefix alone proves nothing.
    fn owns(&self, a: &AdoptionRecord) -> bool {
        self.adoption_id.as_deref() == Some(a.id()) && self.binds(a)
    }

    /// Same key, requirement, grant and suite (used before the ID is saved).
    fn binds(&self, a: &AdoptionRecord) -> bool {
        let b = a.binding();
        a.client_request_id() == self.client_request_id()
            && b.mission_id == self.mission_id
            && b.requirement_id == self.requirement_id
            && b.role_id == self.role_id
            && b.responsibility_id == self.responsibility_id
            && b.grant.assignment_id == self.assignment_id
            && b.grant.epoch == self.epoch
            && a.suite_id() == self.suite_id
    }
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capability_gap_adoptions (
    gap_id TEXT PRIMARY KEY,
    adoption_id TEXT UNIQUE,
    record_json TEXT NOT NULL CHECK(length(record_json) <= 65536));";

fn table_exists(conn: &Connection) -> StoreResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='capability_gap_adoptions')",
        [],
        |r| r.get(0),
    )?)
}

fn decode(gap: Option<&CapabilityGapId>, text: &str) -> StoreResult<GapAdoptionRecord> {
    let record: GapAdoptionRecord = serde_json::from_str(text)?;
    if gap.is_some_and(|g| *g != record.gap_id) {
        return Err(StoreError::NotFound("consistent gap adoption".into()));
    }
    Ok(record)
}

impl Store {
    pub fn init_gap_adoption(&self) -> StoreResult<()> {
        self.lock().execute_batch(SCHEMA)?;
        Ok(())
    }

    pub fn gap_adoption(&self, gap: &CapabilityGapId) -> StoreResult<Option<GapAdoptionRecord>> {
        let conn = self.lock();
        if !table_exists(&conn)? {
            return Ok(None);
        }
        let text: Option<String> = conn
            .query_row(
                "SELECT record_json FROM capability_gap_adoptions WHERE gap_id=?1",
                params![gap.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|t| decode(Some(gap), &t)).transpose()
    }

    fn gap_adoption_by_child(&self, adoption_id: &str) -> StoreResult<Option<GapAdoptionRecord>> {
        let conn = self.lock();
        if !table_exists(&conn)? {
            return Ok(None);
        }
        let text: Option<String> = conn
            .query_row(
                "SELECT record_json FROM capability_gap_adoptions WHERE adoption_id=?1",
                params![adoption_id],
                |r| r.get(0),
            )
            .optional()?;
        let record = text.map(|t| decode(None, &t)).transpose()?;
        if record
            .as_ref()
            .is_some_and(|r| r.adoption_id.as_deref() != Some(adoption_id))
        {
            return Err(StoreError::NotFound("consistent gap adoption".into()));
        }
        Ok(record)
    }

    fn gap_adoptions_active(&self) -> StoreResult<Vec<GapAdoptionRecord>> {
        let conn = self.lock();
        if !table_exists(&conn)? {
            return Ok(vec![]);
        }
        let mut stmt = conn.prepare("SELECT record_json FROM capability_gap_adoptions")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = vec![];
        for row in rows {
            // One unreadable record must not stop every other gap.
            match decode(None, &row?) {
                Ok(r) if r.state == GapAdoptionState::Active => out.push(r),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "skipping unreadable gap adoption record"),
            }
        }
        Ok(out)
    }

    /// The active record whose adoption was created but whose ID was not yet
    /// saved (a crash between the two writes), matched by key and binding.
    fn gap_adoption_unlinked(
        &self,
        adoption: &AdoptionRecord,
    ) -> StoreResult<Option<GapAdoptionRecord>> {
        let conn = self.lock();
        if !table_exists(&conn)? {
            return Ok(None);
        }
        let mut stmt = conn.prepare(
            "SELECT record_json FROM capability_gap_adoptions WHERE adoption_id IS NULL",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            let r = decode(None, &row?)?;
            if r.state == GapAdoptionState::Active && r.binds(adoption) {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }
}

/// Insert a new record, or replace `prev` exactly (compare-and-swap on the
/// stored JSON), inside the caller's transaction.
fn put(
    tx: &Transaction<'_>,
    prev: Option<&GapAdoptionRecord>,
    next: &GapAdoptionRecord,
) -> StoreResult<()> {
    let text = serde_json::to_string(next)?;
    let changed = match prev {
        None => tx.execute(
            "INSERT OR IGNORE INTO capability_gap_adoptions(gap_id, adoption_id, record_json) VALUES (?1, ?2, ?3)",
            params![next.gap_id.as_str(), next.adoption_id, text],
        )?,
        Some(prev) => tx.execute(
            "UPDATE capability_gap_adoptions SET record_json=?2, adoption_id=?3 WHERE gap_id=?1 AND record_json=?4",
            params![next.gap_id.as_str(), text, next.adoption_id, serde_json::to_string(prev)?],
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

fn save(
    app: &AppState,
    prev: Option<&GapAdoptionRecord>,
    next: &GapAdoptionRecord,
) -> StoreResult<()> {
    let mut conn = app.store.lock();
    let tx = conn.transaction()?;
    put(&tx, prev, next)?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Authority and ownership
// ---------------------------------------------------------------------------

/// The frozen grant is current: the same role holds the same assignment and
/// epoch for the responsibility, and the live requirement is still assigned
/// and still the work the gap was opened for.
fn authorized(state: &OrgState, r: &GapAdoptionRecord) -> bool {
    let Ok(grant) = authorize_current(state, &r.role_id, &r.responsibility_id, None, Utc::now())
    else {
        return false;
    };
    grant.assignment_id == r.assignment_id
        && grant.epoch == r.epoch
        && state.capability_gaps.get(&r.gap_id).is_some_and(|g| {
            g.requirement_digest == r.requirement_digest
                && g.requirement_id == r.requirement_id
                && g.role_id == r.role_id
                && g.responsibility_id == r.responsibility_id
        })
        && state.requirements.get(&r.requirement_id).is_some_and(|q| {
            q.status == RequirementStatus::Assigned
                && q.mission_id == r.mission_id
                && engine::requirement_digest(q) == r.requirement_digest
        })
}

/// Gap-driver guard: true while adoption owns this open gap. Adoption owns a
/// gap whose capability has a rule and that the driver has not already bound,
/// and keeps it until its record is released. Unreadable state defers.
pub fn defers_driver(app: &AppState, gap: &CapabilityGap) -> bool {
    if gap.status != CapabilityGapStatus::Open {
        return false;
    }
    match app.store.gap_adoption(&gap.gap_id) {
        Err(_) => true,
        Ok(Some(r)) => r.state != GapAdoptionState::Released,
        Ok(None) => {
            let ruled = app
                .gap_adoption_policy
                .as_deref()
                .is_some_and(|p| p.rule(&gap.capability_id).is_some());
            ruled
                && match app.store.gap_acquisition_binding(gap.gap_id.as_str()) {
                    Ok(binding) => binding.is_none(),
                    Err(_) => true,
                }
        }
    }
}

/// Adoption-acceptance hook. `accepted` is the adoption's outcome with the
/// installation already accepted; when this adoption belongs to an active
/// gap intent whose authority is still current, the same outcome also records
/// the gap installed with the exact selected pin. A definite mismatch returns
/// the outcome unchanged (the coordinator then reports it); a storage error
/// fails the whole acceptance so it is retried rather than split.
pub(crate) fn link_accepted(
    store: &Store,
    accepted: Outcome,
    adoption: &AdoptionRecord,
) -> StoreResult<(Outcome, Option<(GapAdoptionRecord, GapAdoptionRecord)>)> {
    let r = match store.gap_adoption_by_child(adoption.id())? {
        Some(r) => r,
        None => match store.gap_adoption_unlinked(adoption)? {
            Some(r) => r,
            None => return Ok((accepted, None)),
        },
    };
    let (Some(manifest), Some(installation_id)) =
        (adoption.selection(), adoption.installation_id())
    else {
        return Ok((accepted, None));
    };
    let mut linked = r.clone();
    linked.adoption_id = Some(adoption.id().to_owned());
    if r.state != GapAdoptionState::Active
        || r.adoption_id
            .as_deref()
            .is_some_and(|id| id != adoption.id())
        || !linked.owns(adoption)
        || !authorized(&accepted.state, &r)
    {
        return Ok((accepted, None));
    }
    let install = AdoptionInstall {
        adoption_id: adoption.id().to_owned(),
        capability: manifest.reference(),
        installation_id: installation_id.clone(),
    };
    let Ok(done) = engine::install_capability_gap_by_adoption_at(
        &accepted.state,
        &r.gap_id,
        &install,
        Utc::now(),
    ) else {
        return Ok((accepted, None));
    };
    linked.state = GapAdoptionState::Installed;
    linked.code = None;
    linked.selected = Some(install.capability);
    linked.installation_id = Some(install.installation_id);
    linked.updated_at = Utc::now();
    let mut effects = accepted.effects;
    effects.extend(done.effects);
    Ok((
        Outcome {
            state: done.state,
            effects,
        },
        Some((r, linked)),
    ))
}

/// Write the linked record inside the adoption's acceptance transaction.
pub(crate) fn save_link(
    tx: &Transaction<'_>,
    link: &(GapAdoptionRecord, GapAdoptionRecord),
) -> StoreResult<()> {
    put(tx, Some(&link.0), &link.1)
}

// ---------------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------------

/// What one step did, for tests and logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Frozen,
    Dispatched,
    Waiting,
    Installed,
    Released,
    Refused(GapAdoptionCode),
    Attention(GapAdoptionCode),
}

fn stop(
    app: &AppState,
    r: &GapAdoptionRecord,
    state: GapAdoptionState,
    code: Option<GapAdoptionCode>,
) -> StoreResult<Step> {
    let mut next = r.clone();
    next.state = state;
    next.code = code;
    next.updated_at = Utc::now();
    save(app, Some(r), &next)?;
    Ok(match (state, code) {
        (GapAdoptionState::Released, _) => Step::Released,
        (GapAdoptionState::Installed, _) => Step::Installed,
        (_, Some(code)) => Step::Attention(code),
        _ => Step::Idle,
    })
}

fn attention(app: &AppState, r: &GapAdoptionRecord, code: GapAdoptionCode) -> StoreResult<Step> {
    stop(app, r, GapAdoptionState::Attention, Some(code))
}

fn reason(code: GapAdoptionCode) -> &'static str {
    match code {
        GapAdoptionCode::Rejected => {
            "This organization's own acceptance suite rejected the adopted capability."
        }
        GapAdoptionCode::NoCandidate => {
            "No compatible published capability was found and synthesis is not allowed for it."
        }
        _ => "The original requirement or its assignment is no longer valid.",
    }
}

/// A definite outcome: refuse the open kernel gap (which wakes the planner)
/// and close the record in the same transaction, keeping the adoption ID.
fn refuse(
    app: &AppState,
    state: &OrgState,
    gap: &CapabilityGap,
    r: Option<&GapAdoptionRecord>,
    code: GapAdoptionCode,
) -> StoreResult<Step> {
    if gap.status != CapabilityGapStatus::Open {
        return match r {
            Some(r) => attention(app, r, code),
            None => Ok(Step::Idle),
        };
    }
    let outcome = engine::update_capability_gap_at(
        state,
        &gap.gap_id,
        CapabilityGapStatus::Refused {
            reason: reason(code).into(),
            acquisition_id: r.and_then(|r| r.adoption_id.clone()),
        },
        Utc::now(),
    )
    .map_err(|e| StoreError::NotFound(format!("gap refusal: {e}")))?;
    let next = r.map(|r| {
        let mut next = r.clone();
        next.state = GapAdoptionState::Refused;
        next.code = Some(code);
        next.updated_at = Utc::now();
        (r.clone(), next)
    });
    let events = app.store.commit_state(
        state.revision,
        &outcome.state,
        outcome.effects,
        &EventMeta::mission(gap.mission_id.clone()).caused_by(gap.gap_id.to_string()),
        vec![],
        |tx| match &next {
            Some((prev, next)) => put(tx, Some(prev), next),
            None => Ok(()),
        },
    )?;
    app.broadcast(&events);
    Ok(Step::Refused(code))
}

/// The gap's requirement itself changed (not merely a role still waiting to
/// be activated): a definite reason to refuse before anything is frozen.
fn requirement_changed(state: &OrgState, gap: &CapabilityGap) -> bool {
    state.requirements.get(&gap.requirement_id).is_none_or(|q| {
        q.status != RequirementStatus::Assigned
            || q.mission_id != gap.mission_id
            || engine::requirement_digest(q) != gap.requirement_digest
    })
}

fn suite_sha256(suite: &genesis_verification::OracleSuite) -> Option<String> {
    serde_json::to_value(suite)
        .ok()
        .map(|v| genesis_verification::json_sha256(&v))
}

fn suite_current(app: &AppState, r: &GapAdoptionRecord) -> bool {
    app.evidence_policy
        .suite(&r.suite_id)
        .is_some_and(|s| suite_sha256(s).as_deref() == Some(&r.suite_sha256))
}

/// One local decision for one gap. Caller holds the kernel lock.
fn step_locked(app: &AppState, state: &OrgState, gap_id: &CapabilityGapId) -> StoreResult<Step> {
    let Some(gap) = state.capability_gaps.get(gap_id) else {
        return Ok(Step::Idle);
    };
    let Some(r) = app.store.gap_adoption(gap_id)? else {
        return freeze(app, state, gap);
    };
    if r.state != GapAdoptionState::Active {
        return Ok(Step::Idle);
    }
    let Some(adoption_id) = r.adoption_id.clone() else {
        return dispatch(app, state, gap, &r);
    };
    let adoption = match app.store.get_adoption(&adoption_id) {
        Ok(a) => a,
        Err(AdoptionError::Db(e)) => return Err(StoreError::Db(e)),
        Err(_) => return attention(app, &r, GapAdoptionCode::InvalidRecord),
    };
    if !r.owns(&adoption) {
        return attention(app, &r, GapAdoptionCode::InvalidRecord);
    }
    use AdoptionAttention as A;
    match adoption.phase() {
        AdoptionPhase::Discovering | AdoptionPhase::Selected | AdoptionPhase::Checking => {
            // The adoption fences itself on lost authority; mirror it early so
            // an acceptance can never link to a changed requirement.
            if !authorized(state, &r) {
                return refuse(app, state, gap, Some(&r), GapAdoptionCode::Authority);
            }
            Ok(Step::Waiting)
        }
        AdoptionPhase::Accepted => {
            // Normally linked atomically by the acceptance transaction; reaching
            // here means the link was declined.
            let linked = matches!(&gap.status, CapabilityGapStatus::Installed { acquisition_id, installation_id, .. }
                if *acquisition_id == adoption_id && Some(installation_id) == adoption.installation_id());
            if linked {
                return Ok(Step::Idle);
            }
            if authorized(state, &r) {
                attention(app, &r, GapAdoptionCode::InvalidRecord)
            } else {
                refuse(app, state, gap, Some(&r), GapAdoptionCode::Authority)
            }
        }
        AdoptionPhase::Rejected => refuse(app, state, gap, Some(&r), GapAdoptionCode::Rejected),
        AdoptionPhase::Fenced => refuse(app, state, gap, Some(&r), GapAdoptionCode::Authority),
        AdoptionPhase::Attention => match adoption.attention_code() {
            // An empty search answered the frozen grant only. A changed grant
            // (even the same role under a new assignment or epoch) must not
            // inherit it: neither the synthesis fallback nor a NoCandidate
            // refusal, so refuse on authority before either.
            Some(A::NoCandidate) if !authorized(state, &r) => {
                refuse(app, state, gap, Some(&r), GapAdoptionCode::Authority)
            }
            Some(A::NoCandidate)
                if r.synthesis_fallback && gap.status == CapabilityGapStatus::Open =>
            {
                stop(
                    app,
                    &r,
                    GapAdoptionState::Released,
                    Some(GapAdoptionCode::NoCandidate),
                )
            }
            Some(A::NoCandidate) => refuse(app, state, gap, Some(&r), GapAdoptionCode::NoCandidate),
            Some(A::RegistryUnavailable) => attention(app, &r, GapAdoptionCode::Registry),
            Some(A::InvalidCandidate) => attention(app, &r, GapAdoptionCode::Candidate),
            Some(A::ChecksFailed | A::ProviderUnavailable) => {
                attention(app, &r, GapAdoptionCode::Checks)
            }
            Some(A::ConfigurationChanged) => attention(app, &r, GapAdoptionCode::Configuration),
            Some(A::AcceptanceConflict | A::InstallationConflict) => {
                attention(app, &r, GapAdoptionCode::Conflict)
            }
            Some(A::Internal) | None => attention(app, &r, GapAdoptionCode::InvalidRecord),
        },
    }
}

fn freeze(app: &AppState, state: &OrgState, gap: &CapabilityGap) -> StoreResult<Step> {
    if gap.status != CapabilityGapStatus::Open {
        return Ok(Step::Idle);
    }
    let Some(rule) = app
        .gap_adoption_policy
        .as_deref()
        .and_then(|p| p.rule(&gap.capability_id))
    else {
        return Ok(Step::Idle);
    };
    // The gap driver bound it first (e.g. adoption enabled later): it owns it.
    // Unreadable bindings also leave the gap untouched (the driver defers it).
    if !matches!(
        app.store.gap_acquisition_binding(gap.gap_id.as_str()),
        Ok(None)
    ) {
        return Ok(Step::Idle);
    }
    let Some(suite) = app.evidence_policy.suite(&rule.suite_id) else {
        return Ok(Step::Idle);
    };
    let Some(suite_sha256) = suite_sha256(suite) else {
        return Ok(Step::Idle);
    };
    if requirement_changed(state, gap) {
        return refuse(app, state, gap, None, GapAdoptionCode::Authority);
    }
    // Wait (still deferring the driver) until the role holds the work.
    let Ok(grant) = authorize_current(
        state,
        &gap.role_id,
        &gap.responsibility_id,
        None,
        Utc::now(),
    ) else {
        return Ok(Step::Idle);
    };
    let now = Utc::now();
    let record = GapAdoptionRecord {
        schema_version: 1,
        gap_id: gap.gap_id.clone(),
        capability_id: gap.capability_id.clone(),
        suite_id: rule.suite_id.clone(),
        suite_sha256,
        synthesis_fallback: rule.synthesis_fallback,
        mission_id: gap.mission_id.clone(),
        requirement_id: gap.requirement_id.clone(),
        requirement_digest: gap.requirement_digest.clone(),
        role_id: gap.role_id.clone(),
        responsibility_id: gap.responsibility_id.clone(),
        assignment_id: grant.assignment_id,
        epoch: grant.epoch,
        frozen_at: now,
        adoption_id: None,
        selected: None,
        installation_id: None,
        state: GapAdoptionState::Active,
        code: None,
        updated_at: now,
        attempt: 0,
        previous_adoption_ids: vec![],
    };
    if !authorized(state, &record) {
        return Ok(Step::Idle);
    }
    save(app, None, &record)?;
    Ok(Step::Frozen)
}

fn dispatch(
    app: &AppState,
    state: &OrgState,
    gap: &CapabilityGap,
    r: &GapAdoptionRecord,
) -> StoreResult<Step> {
    if gap.status != CapabilityGapStatus::Open {
        return attention(app, r, GapAdoptionCode::InvalidRecord);
    }
    if !authorized(state, r) {
        return refuse(app, state, gap, Some(r), GapAdoptionCode::Authority);
    }
    if !suite_current(app, r) || crate::capability_adoption::configured(app).is_err() {
        return attention(app, r, GapAdoptionCode::Configuration);
    }
    let suite = app.evidence_policy.suite(&r.suite_id).expect("checked");
    // Idempotent by key: a crash after creation recovers the same adoption.
    let adoption = match app.store.create_adoption(&r.intent(), suite, Utc::now()) {
        Ok((a, _)) => a,
        Err(AdoptionError::Db(e)) => return Err(StoreError::Db(e)),
        Err(AdoptionError::Authority(_)) => {
            return refuse(app, state, gap, Some(r), GapAdoptionCode::Authority)
        }
        // Another adoption holds this requirement and grant, or the store is
        // at capacity: both clear when that work ends, so wait (visibly).
        Err(AdoptionError::ActiveConflict | AdoptionError::Limit) => {
            if r.code != Some(GapAdoptionCode::Conflict) {
                let mut next = r.clone();
                next.code = Some(GapAdoptionCode::Conflict);
                next.updated_at = Utc::now();
                save(app, Some(r), &next)?;
            }
            return Ok(Step::Waiting);
        }
        Err(AdoptionError::InvalidInput) => {
            return attention(app, r, GapAdoptionCode::Configuration)
        }
        Err(_) => return attention(app, r, GapAdoptionCode::InvalidRecord),
    };
    let mut next = r.clone();
    next.adoption_id = Some(adoption.id().to_owned());
    next.code = None;
    next.updated_at = Utc::now();
    if !next.owns(&adoption) {
        return attention(app, r, GapAdoptionCode::InvalidRecord);
    }
    save(app, Some(r), &next)?;
    Ok(Step::Dispatched)
}

/// Step one gap under the kernel lock.
pub async fn step(app: &AppState, gap_id: &CapabilityGapId) -> StoreResult<Step> {
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    step_locked(app, &state, gap_id)
}

/// Step every open gap and every active record once.
pub async fn drain_once(app: &AppState) -> StoreResult<BTreeMap<CapabilityGapId, Step>> {
    let mut out = BTreeMap::new();
    // The worker runs in every organization; without a policy it only has
    // frozen intents to finish, so don't take the kernel lock for nothing.
    if app.gap_adoption_policy.is_none() && app.store.gap_adoptions_active()?.is_empty() {
        return Ok(out);
    }
    let _kernel = app.kernel.lock().await;
    let state = app.store.current_state()?;
    let mut ids: BTreeSet<(u64, CapabilityGapId)> = state
        .capability_gaps
        .values()
        .filter(|g| g.status == CapabilityGapStatus::Open)
        .map(|g| (g.opened_in_revision, g.gap_id.clone()))
        .collect();
    for r in app.store.gap_adoptions_active()? {
        if let Some(g) = state.capability_gaps.get(&r.gap_id) {
            ids.insert((g.opened_in_revision, r.gap_id));
        }
    }
    for (_, id) in ids {
        // A step may commit (a refusal), so each gap reads the latest state.
        let state = app.store.current_state()?;
        match step_locked(app, &state, &id) {
            Ok(step) => {
                out.insert(id, step);
            }
            Err(e) => tracing::warn!(gap_id = %id, error = %e, "gap adoption step failed"),
        }
    }
    Ok(out)
}

pub fn spawn(app: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut delay = Duration::from_millis(500);
        loop {
            tokio::time::sleep(delay).await;
            match drain_once(&app).await {
                Ok(_) => delay = Duration::from_millis(500),
                Err(e) => {
                    delay = (delay * 2).min(Duration::from_secs(60));
                    tracing::error!(error = %e, retry_in_ms = delay.as_millis() as u64, "gap adoption pass failed; retrying");
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Operator status
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GapAdoptionStatus {
    pub gap_id: CapabilityGapId,
    pub enabled: bool,
    pub state: Option<GapAdoptionState>,
    pub code: Option<GapAdoptionCode>,
    pub adoption_id: Option<String>,
    pub synthesis_fallback: Option<bool>,
    pub selected: Option<CapabilityRef>,
    pub installation_id: Option<InstallationId>,
    pub updated_at: Option<DateTime<Utc>>,
    pub attempt: Option<u32>,
    pub previous_adoption_ids: Vec<String>,
    /// Whether `POST .../adoption/retry` would start a fresh adoption now.
    pub retryable: bool,
}

pub async fn status_route(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<GapAdoptionStatus>> {
    p.require(&[Principal::Operator])?;
    let id = CapabilityGapId::new(id);
    if !app.current_state()?.capability_gaps.contains_key(&id) {
        return Err(ApiError::not_found("capability gap"));
    }
    let r = app.store.gap_adoption(&id)?;
    Ok(Json(GapAdoptionStatus {
        gap_id: id,
        enabled: app.gap_adoption_policy.is_some(),
        state: r.as_ref().map(|r| r.state),
        code: r.as_ref().and_then(|r| r.code),
        adoption_id: r.as_ref().and_then(|r| r.adoption_id.clone()),
        synthesis_fallback: r.as_ref().map(|r| r.synthesis_fallback),
        selected: r.as_ref().and_then(|r| r.selected.clone()),
        installation_id: r.as_ref().and_then(|r| r.installation_id.clone()),
        updated_at: r.as_ref().map(|r| r.updated_at),
        attempt: r.as_ref().map(|r| r.attempt),
        previous_adoption_ids: r
            .as_ref()
            .map(|r| r.previous_adoption_ids.clone())
            .unwrap_or_default(),
        retryable: r.as_ref().is_some_and(can_retry),
    }))
}

fn can_retry(r: &GapAdoptionRecord) -> bool {
    r.state == GapAdoptionState::Attention
        && r.code.is_some_and(GapAdoptionCode::retryable)
        && r.adoption_id.is_some()
        && r.attempt < MAX_RETRIES
}

/// Operator recovery for a retryable stop: keep the stopped adoption in
/// history and let the coordinator dispatch one fresh adoption under a new
/// key, rechecking the requirement and grant first. Bounded per gap.
/// Synthesis fallback is unchanged: only a definitive empty search can
/// release the gap.
pub async fn retry_route(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<GapAdoptionStatus>> {
    p.require(&[Principal::Operator])?;
    let gap = CapabilityGapId::new(id.clone());
    {
        let _kernel = app.kernel.lock().await;
        let r = app
            .store
            .gap_adoption(&gap)?
            .ok_or_else(|| ApiError::not_found("gap adoption"))?;
        if !can_retry(&r) {
            return Err(ApiError::new(
                axum::http::StatusCode::CONFLICT,
                "adoption_not_retryable",
                "this gap adoption is not stopped for a retryable reason, or its retries are used up",
            ));
        }
        let open = app
            .current_state()?
            .capability_gaps
            .get(&gap)
            .is_some_and(|g| g.status == CapabilityGapStatus::Open);
        if !open {
            return Err(ApiError::new(
                axum::http::StatusCode::CONFLICT,
                "adoption_not_retryable",
                "the gap is no longer open",
            ));
        }
        let mut next = r.clone();
        next.previous_adoption_ids.extend(next.adoption_id.take());
        next.attempt += 1;
        next.state = GapAdoptionState::Active;
        next.code = None;
        next.updated_at = Utc::now();
        save(&app, Some(&r), &next)?;
    }
    status_route(State(app), p, Path(id)).await
}
