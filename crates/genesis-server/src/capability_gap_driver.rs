//! Opt-in intake of committed capability gaps into the existing acquisition
//! runtime. This worker performs local transactions only: the acquisition
//! runtime remains the sole owner of provider effects, quotas and recovery.
use crate::app::AppState;
use crate::capability_acquisition_store::AcquisitionStoreError;
use crate::capability_gap_policy::GapPolicyError;
use crate::capability_gap_store::GapAcquisitionError;
use crate::store::{EventMeta, Store, StoreError, StoreResult};
use chrono::{DateTime, Utc};
use genesis_core::engine;
use genesis_core::ids::CapabilityGapId;
use genesis_core::model::{CapabilityGapStatus, CapabilityRef, OrgState};
use genesis_forge::AcquisitionPhase;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapDriverCode {
    Configuration,
    Policy,
    Provisioning,
    Authority,
    Quota,
    Conflict,
    InvalidRecord,
    UnknownOutcome,
    RetryRead,
    VerificationRejected,
    AcquisitionFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapDriverState {
    Disabled,
    Ready,
    Waiting,
    Attention,
    Acquiring,
    Acquired,
    Installed,
    Refused,
}

/// Operational metadata only. The request, public HTML, source bodies, oracle
/// answers and provider responses are deliberately absent from this view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapDriverStatus {
    pub gap_id: CapabilityGapId,
    pub enabled: bool,
    pub state: GapDriverState,
    pub code: Option<GapDriverCode>,
    pub acquisition_id: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DrainSummary {
    pub examined: usize,
    pub bound: usize,
    pub created: usize,
    pub transitioned: usize,
    pub attention: usize,
}

fn invalid() -> StoreError {
    StoreError::Db(rusqlite::Error::InvalidParameterName(
        "capability gap driver storage validation failed".into(),
    ))
}

impl Store {
    pub fn init_gap_driver(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_gap_progress (
                gap_id TEXT PRIMARY KEY,
                status_json TEXT NOT NULL CHECK(length(status_json)<=2048));",
        )?;
        Ok(())
    }
}

fn save_status(conn: &Connection, status: &GapDriverStatus) -> StoreResult<()> {
    let text = serde_json::to_string(status)?;
    if text.len() > 2048 {
        return Err(invalid());
    }
    conn.execute(
        "INSERT INTO capability_gap_progress(gap_id,status_json) VALUES (?1,?2)
         ON CONFLICT(gap_id) DO UPDATE SET status_json=excluded.status_json",
        params![status.gap_id.as_str(), text],
    )?;
    Ok(())
}

pub fn status(app: &AppState, id: &CapabilityGapId) -> StoreResult<GapDriverStatus> {
    let state = app.store.current_state()?;
    let gap = state
        .capability_gaps
        .get(id)
        .ok_or_else(|| StoreError::NotFound("capability gap".into()))?;
    let text: Option<String> = app
        .store
        .lock()
        .query_row(
            "SELECT status_json FROM capability_gap_progress WHERE gap_id=?1",
            params![id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let mut status = match text {
        Some(text) if text.len() <= 2048 => {
            let saved: GapDriverStatus = serde_json::from_str(&text).map_err(|_| invalid())?;
            if saved.gap_id != *id
                || saved
                    .acquisition_id
                    .as_ref()
                    .is_some_and(|id| id.len() > 128)
            {
                return Err(invalid());
            }
            saved
        }
        Some(_) => return Err(invalid()),
        None => GapDriverStatus {
            gap_id: id.clone(),
            enabled: false,
            state: GapDriverState::Ready,
            code: None,
            acquisition_id: None,
            updated_at: None,
        },
    };
    status.enabled = app.gap_acquisition_policy.is_some();
    // Kernel lifecycle is authoritative even if the process stopped between a
    // child creation and its next projection. Saved metadata is informational.
    if let Some(id) = gap.status.acquisition_id() {
        status.acquisition_id = Some(id.into());
    }
    status.state = match gap.status {
        CapabilityGapStatus::Acquired { .. } => GapDriverState::Acquired,
        CapabilityGapStatus::Installed { .. } => GapDriverState::Installed,
        CapabilityGapStatus::Refused { .. } => GapDriverState::Refused,
        _ if !status.enabled => GapDriverState::Disabled,
        CapabilityGapStatus::Acquiring { .. } if status.code.is_none() => GapDriverState::Acquiring,
        _ => status.state,
    };
    Ok(status)
}

struct Decision {
    target: Option<CapabilityGapStatus>,
    code: Option<GapDriverCode>,
}
impl Decision {
    fn attention(code: GapDriverCode) -> Self {
        Self {
            target: None,
            code: Some(code),
        }
    }
    fn refuse(code: GapDriverCode, id: Option<String>, reason: &'static str) -> Self {
        Self {
            target: Some(CapabilityGapStatus::Refused {
                reason: reason.into(),
                acquisition_id: id,
            }),
            code: Some(code),
        }
    }
}

fn acquisition_error(error: AcquisitionStoreError, id: Option<String>) -> StoreResult<Decision> {
    use AcquisitionStoreError::*;
    Ok(match error {
        Authority(_) => Decision::refuse(
            GapDriverCode::Authority,
            id,
            "The original acquisition authority or deadline is no longer valid.",
        ),
        QuotaExceeded(_) => Decision::attention(GapDriverCode::Quota),
        Conflict | IdempotencyConflict | ActiveConflict => {
            Decision::attention(GapDriverCode::Conflict)
        }
        InvalidPolicy | PolicyChanged => Decision::attention(GapDriverCode::Configuration),
        InvalidInput | InvalidRecord | NotFound | Encoding => {
            Decision::attention(GapDriverCode::InvalidRecord)
        }
        Db(_) => return Err(invalid()),
    })
}

fn gap_error(error: GapAcquisitionError, id: Option<String>) -> StoreResult<Decision> {
    use GapAcquisitionError::*;
    Ok(match error {
        Policy(GapPolicyError::NoMatchingRule | GapPolicyError::SourceNotAllowed) => Decision::refuse(
            GapDriverCode::Policy, id, "The requested capability or sources are outside the configured acquisition policy."),
        Policy(_) => Decision::attention(GapDriverCode::Configuration),
        Acquisition(error) => return acquisition_error(error, id),
        WaitingForRole => Decision::attention(GapDriverCode::Provisioning),
        AuthorityChanged => Decision::refuse(GapDriverCode::Authority, id, "The original requirement or acquisition authority is no longer valid."),
        InvalidRecord | NotFound | Encoding => Decision::attention(GapDriverCode::InvalidRecord),
        Db(_) => return Err(invalid()),
    })
}

/// Called only while the kernel lock is held. Every creation rechecks the
/// frozen request and current authority inside the acquisition transaction.
fn decide(
    app: &AppState,
    state: &OrgState,
    id: &CapabilityGapId,
    summary: &mut DrainSummary,
) -> StoreResult<Decision> {
    let gap = state.capability_gaps.get(id).ok_or_else(invalid)?;
    let saved = match app.store.gap_acquisition_binding(id.as_str()) {
        Ok(saved) => saved,
        Err(error) => return gap_error(error, gap.status.acquisition_id().map(String::from)),
    };
    let binding = if let Some(saved) = saved {
        saved
    } else {
        if gap.status != CapabilityGapStatus::Open {
            return Ok(Decision::attention(GapDriverCode::InvalidRecord));
        }
        let Some(policy) = app.gap_acquisition_policy.as_deref() else {
            return Ok(Decision::attention(GapDriverCode::Configuration));
        };
        let Some(acquisition_policy) = app.acquisition_policy.as_deref() else {
            return Ok(Decision::attention(GapDriverCode::Configuration));
        };
        // Materialize only to select a suite for a NEW binding. Existing
        // bindings never read a replacement policy or replacement examples.
        let request = match policy.materialize(&state.org_id, gap) {
            Ok(request) => request,
            Err(error) => return gap_error(GapAcquisitionError::Policy(error), None),
        };
        let Some(suite) = app.evidence_policy.suite(&request.suite_id) else {
            return Ok(Decision::attention(GapDriverCode::Configuration));
        };
        match app.store.bind_gap_acquisition(
            id.as_str(),
            policy,
            acquisition_policy,
            suite,
            Utc::now(),
        ) {
            Ok(binding) => {
                summary.bound += 1;
                binding
            }
            Err(error) => return gap_error(error, None),
        }
    };
    let Some(suite) = app.evidence_policy.suite(binding.suite_id()) else {
        return Ok(Decision::attention(GapDriverCode::Configuration));
    };
    if !binding.suite_matches(suite) {
        return Ok(Decision::attention(GapDriverCode::Configuration));
    }
    let (record, created) = if let Some(acquisition_id) = binding.acquisition_id() {
        match app.store.get_acquisition(acquisition_id, suite) {
            Ok(record) => (record, false),
            Err(error) => return acquisition_error(error, Some(acquisition_id.into())),
        }
    } else {
        match app
            .store
            .create_gap_acquisition(id.as_str(), suite, Utc::now())
        {
            Ok(result) => result,
            Err(error) => return gap_error(error, None),
        }
    };
    summary.created += usize::from(created);
    let acquisition_id = record.id().to_owned();
    let binding = match app.store.gap_acquisition_binding(id.as_str()) {
        Ok(Some(binding)) => binding,
        Ok(None) => return Ok(Decision::attention(GapDriverCode::InvalidRecord)),
        Err(error) => return gap_error(error, Some(acquisition_id)),
    };
    // Recovering a child ID alone is insufficient: the request, model, suite
    // and frozen authority must all belong to this exact parent binding.
    if let Err(error) = binding.matches_record(&record) {
        return gap_error(error, Some(acquisition_id));
    }
    if gap.status == CapabilityGapStatus::Open {
        // A crash after atomic child creation is repaired here using the
        // durable link. Never make a second child to repair a projection.
        return Ok(Decision {
            target: Some(CapabilityGapStatus::Acquiring { acquisition_id }),
            code: None,
        });
    }
    if gap.status.acquisition_id() != Some(record.id()) {
        return Ok(Decision::attention(GapDriverCode::InvalidRecord));
    }
    // An ambiguous POST retains its original receipt and intent, even if its
    // authority subsequently expires. Only explicit reconciliation may decide
    // its external outcome; this worker never retries or replaces it.
    if record.session().phase() == AcquisitionPhase::NeedsReconciliation {
        return Ok(Decision::attention(GapDriverCode::UnknownOutcome));
    }
    if let Err(error) = binding.validate_current(state, Utc::now()) {
        return gap_error(error, Some(acquisition_id));
    }
    if let Err(error) = record.authorize(state, Utc::now()) {
        return acquisition_error(error, Some(acquisition_id));
    }
    if crate::capability_acquisition_runtime::retryable_read(&record) {
        return Ok(Decision::attention(GapDriverCode::RetryRead));
    }
    // Reflect the existing scheduler's version-bound pause. This is visibility
    // only; retry_read and all provider effects stay with its original owner.
    if record.active(suite).map_err(|_| invalid())? {
        use crate::capability_acquisition_driver::DriverErrorCode;
        if let Some(code) =
            crate::capability_acquisition_driver::status(app, record.id())?.error_code
        {
            return Ok(Decision::attention(match code {
                DriverErrorCode::RetryRead => GapDriverCode::RetryRead,
                DriverErrorCode::Quota => GapDriverCode::Quota,
                DriverErrorCode::Configuration => GapDriverCode::Configuration,
                DriverErrorCode::Conflict => GapDriverCode::Conflict,
                DriverErrorCode::InvalidRecord | DriverErrorCode::Internal => {
                    GapDriverCode::InvalidRecord
                }
            }));
        }
    }
    Ok(match record.session().phase() {
        AcquisitionPhase::Verified => {
            let proof = record.session().verified(suite).map_err(|_| invalid())?;
            if proof.artifact.capability_id != gap.capability_id.as_str() {
                return Ok(Decision::attention(GapDriverCode::InvalidRecord));
            }
            Decision {
                target: Some(CapabilityGapStatus::Acquired {
                    acquisition_id,
                    capability: CapabilityRef::new(
                        proof.artifact.capability_id,
                        proof.artifact.capability_version,
                    ),
                    // A verifier result is not a confirmed registry publication.
                    publication_ref: None,
                }),
                code: None,
            }
        }
        AcquisitionPhase::Rejected if !record.active(suite).map_err(|_| invalid())? => {
            Decision::refuse(
                GapDriverCode::VerificationRejected,
                Some(acquisition_id),
                "Independent verification rejected the candidate after its allowed repairs.",
            )
        }
        AcquisitionPhase::Failed => Decision::refuse(
            GapDriverCode::AcquisitionFailed,
            Some(acquisition_id),
            "The acquisition ended with a definite failure.",
        ),
        _ => Decision {
            target: None,
            code: None,
        },
    })
}

/// Service the bounded set of committed open/acquiring gaps (at most sixteen
/// live gaps under core policy). No network I/O, provider retries or sleeps.
pub async fn drain_once(app: &AppState) -> StoreResult<DrainSummary> {
    let mut summary = DrainSummary::default();
    if app.gap_acquisition_policy.is_none() {
        return Ok(summary);
    }
    let _kernel = app.kernel.lock().await;
    let mut state = app.store.current_state()?;
    let mut ids: Vec<_> = state
        .capability_gaps
        .values()
        .filter(|gap| {
            matches!(
                gap.status,
                CapabilityGapStatus::Open | CapabilityGapStatus::Acquiring { .. }
            )
        })
        .map(|gap| (gap.opened_in_revision, gap.gap_id.clone()))
        .collect();
    ids.sort();
    for (_, id) in ids {
        summary.examined += 1;
        let decision = decide(app, &state, &id, &mut summary)?;
        let gap = state.capability_gaps.get(&id).ok_or_else(invalid)?;
        let target = decision.target.as_ref().unwrap_or(&gap.status);
        let progress = GapDriverStatus {
            gap_id: id.clone(),
            enabled: true,
            state: match target {
                CapabilityGapStatus::Acquired { .. } => GapDriverState::Acquired,
                CapabilityGapStatus::Refused { .. } => GapDriverState::Refused,
                _ if decision.code == Some(GapDriverCode::Provisioning) => GapDriverState::Waiting,
                _ if decision.code.is_some() => GapDriverState::Attention,
                CapabilityGapStatus::Acquiring { .. } => GapDriverState::Acquiring,
                _ => GapDriverState::Ready,
            },
            code: decision.code,
            acquisition_id: target.acquisition_id().map(String::from),
            updated_at: Some(Utc::now()),
        };
        summary.attention += usize::from(progress.state == GapDriverState::Attention);
        if let Some(target) = decision.target.filter(|target| *target != gap.status) {
            let outcome = engine::update_capability_gap_at(&state, &id, target, Utc::now())
                .map_err(|_| invalid())?;
            let events = app.store.commit_state(
                state.revision,
                &outcome.state,
                outcome.effects,
                &EventMeta::mission(gap.mission_id.clone()).caused_by(id.to_string()),
                Vec::new(),
                |tx| save_status(tx, &progress),
            )?;
            app.broadcast(&events);
            state = outcome.state;
            summary.transitioned += 1;
        } else {
            save_status(&app.store.lock(), &progress)?;
        }
    }
    Ok(summary)
}

pub fn spawn(app: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut failures: u32 = 0;
        loop {
            interval.tick().await;
            match drain_once(&app).await {
                Ok(_) => failures = 0,
                Err(error) => {
                    failures = failures.saturating_add(1);
                    let wait = Duration::from_millis(250u64 << failures.min(7))
                        .min(Duration::from_secs(30));
                    tracing::error!(%error, retry_in_ms = wait.as_millis() as u64, "capability gap intake pass failed; retrying");
                    tokio::time::sleep(wait).await;
                }
            }
        }
    })
}
