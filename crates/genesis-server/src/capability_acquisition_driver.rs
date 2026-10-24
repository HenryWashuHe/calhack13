//! Explicitly enabled scheduling of already authorized acquisition intents.
//! The existing runtime owns every external intent, debit, receipt and recovery
//! decision. This module owns only fair scheduling and version-bound attention.

use crate::app::AppState;
use crate::capability_acquisition_runtime::AcquisitionRuntimeError;
use crate::capability_acquisition_store::AcquisitionStoreError;
use crate::store::{Store, StoreError, StoreResult};
use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::time::Duration;
use tokio::task::JoinHandle;

pub const ACQUISITION_BATCH_LIMIT: usize = 4;
const ADVANCE_DELAY_SECONDS: i64 = 1;
const RECONCILE_DELAY_SECONDS: i64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverErrorCode {
    RetryRead,
    Quota,
    Configuration,
    InvalidRecord,
    Conflict,
    Internal,
}
impl DriverErrorCode {
    fn label(self) -> &'static str {
        match self {
            Self::RetryRead => "retry_read",
            Self::Quota => "quota",
            Self::Configuration => "configuration",
            Self::InvalidRecord => "invalid_record",
            Self::Conflict => "conflict",
            Self::Internal => "internal",
        }
    }
    fn parse(value: &str) -> StoreResult<Self> {
        match value {
            "retry_read" => Ok(Self::RetryRead),
            "quota" => Ok(Self::Quota),
            "configuration" => Ok(Self::Configuration),
            "invalid_record" => Ok(Self::InvalidRecord),
            "conflict" => Ok(Self::Conflict),
            "internal" => Ok(Self::Internal),
            _ => Err(invalid("invalid acquisition driver error code")),
        }
    }
    fn reconcile_locally(self) -> bool {
        matches!(self, Self::RetryRead | Self::Quota)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationState {
    Disabled,
    Ready,
    Waiting,
    Attention,
    Inactive,
}

/// Scheduling metadata only. Read the acquisition endpoint for phase and proof.
#[derive(Clone, Debug, Serialize)]
pub struct AutomationStatus {
    pub id: String,
    pub enabled: bool,
    pub current_version: u64,
    pub observed_version: Option<u64>,
    pub state: AutomationState,
    pub error_code: Option<DriverErrorCode>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub next_reconcile_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DrainSummary {
    pub serviced: usize,
    pub advanced: usize,
    pub attention: usize,
    pub stale: usize,
}

#[derive(Clone, Debug)]
struct Schedule {
    version: u64,
    serviced_at: DateTime<Utc>,
    next_check_at: Option<DateTime<Utc>>,
    error: Option<DriverErrorCode>,
}
#[derive(Clone, Debug)]
struct Candidate {
    id: String,
    version: u64,
    error: Option<DriverErrorCode>,
}

fn invalid(message: &'static str) -> StoreError {
    StoreError::Db(rusqlite::Error::InvalidParameterName(message.into()))
}
fn stamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
fn parse_time(text: &str) -> StoreResult<DateTime<Utc>> {
    if text.len() > 40 {
        return Err(invalid("invalid acquisition driver timestamp"));
    }
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| invalid("invalid acquisition driver timestamp"))
}
fn version(value: i64) -> StoreResult<u64> {
    u64::try_from(value).map_err(|_| invalid("invalid acquisition driver version"))
}
fn metadata(conn: &Connection, id: &str) -> StoreResult<(u64, bool)> {
    let row: Option<(i64, i64)> = conn
        .query_row(
            "SELECT version,active FROM capability_acquisitions WHERE id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (current, active) = row.ok_or_else(|| StoreError::NotFound("acquisition".into()))?;
    if !(0..=1).contains(&active) {
        return Err(invalid("invalid acquisition active index"));
    }
    Ok((version(current)?, active == 1))
}
fn schedule(conn: &Connection, id: &str) -> StoreResult<Option<Schedule>> {
    type Row = (i64, String, Option<String>, Option<String>);
    let row: Option<Row> = conn.query_row(
        "SELECT observed_version,last_serviced_at,next_check_at,error_code FROM acquisition_automation WHERE acquisition_id=?1",
        params![id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional()?;
    row.map(|(v, serviced, next, error)| {
        let error = error.as_deref().map(DriverErrorCode::parse).transpose()?;
        let next_check_at = next.as_deref().map(parse_time).transpose()?;
        if next_check_at.is_some() != error.is_none_or(DriverErrorCode::reconcile_locally) {
            return Err(invalid("inconsistent acquisition driver schedule"));
        }
        Ok(Schedule {
            version: version(v)?,
            serviced_at: parse_time(&serviced)?,
            next_check_at,
            error,
        })
    })
    .transpose()
}

impl Store {
    pub fn init_acquisition_driver(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS acquisition_automation (
                acquisition_id TEXT PRIMARY KEY REFERENCES capability_acquisitions(id),
                observed_version INTEGER NOT NULL CHECK(observed_version>=0),
                last_serviced_at TEXT NOT NULL,
                next_check_at TEXT,
                error_code TEXT CHECK(error_code IN ('retry_read','quota','configuration','invalid_record','conflict','internal')));
             CREATE INDEX IF NOT EXISTS acquisition_automation_due ON acquisition_automation(next_check_at,last_serviced_at);",
        )?;
        Ok(())
    }
}

pub fn status(app: &AppState, id: &str) -> StoreResult<AutomationStatus> {
    let conn = app.store.lock();
    let (current, active) = metadata(&conn, id)?;
    let saved = schedule(&conn, id)?;
    let matching = saved.as_ref().filter(|s| s.version == current);
    let error = matching.and_then(|s| s.error);
    let next = matching.and_then(|s| s.next_check_at);
    let state = if !app.automatic_acquisition {
        AutomationState::Disabled
    } else if !active {
        AutomationState::Inactive
    } else if error.is_some() {
        AutomationState::Attention
    } else if next.is_some_and(|due| due > Utc::now()) {
        AutomationState::Waiting
    } else {
        AutomationState::Ready
    };
    Ok(AutomationStatus {
        id: id.into(),
        enabled: app.automatic_acquisition,
        current_version: current,
        observed_version: saved.as_ref().map(|s| s.version),
        state,
        error_code: if active { error } else { None },
        next_attempt_at: if active && error.is_none() {
            next
        } else {
            None
        },
        next_reconcile_at: if active && error.is_some_and(DriverErrorCode::reconcile_locally) {
            next
        } else {
            None
        },
        updated_at: saved.map(|s| s.serviced_at),
    })
}

fn candidates(store: &Store, now: DateTime<Utc>) -> StoreResult<Vec<Candidate>> {
    let conn = store.lock();
    let mut stmt = conn.prepare(
        "SELECT a.id,a.version FROM capability_acquisitions a
         LEFT JOIN acquisition_automation s ON s.acquisition_id=a.id
         WHERE a.active=1 AND (s.acquisition_id IS NULL OR s.observed_version<>a.version OR s.next_check_at<=?1)
         ORDER BY COALESCE(s.last_serviced_at,'0000'),a.id LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![stamp(now), ACQUISITION_BATCH_LIMIT as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.map(|row| {
        let (id, raw) = row?;
        let version = version(raw)?;
        let error = schedule(&conn, &id)?
            .filter(|s| s.version == version)
            .and_then(|s| s.error);
        Ok(Candidate { id, version, error })
    })
    .collect()
}

/// Persist only against the version actually observed by the runtime. If a
/// manual step won a race, its newer version must never inherit an old error.
fn save_schedule(
    store: &Store,
    id: &str,
    observed: u64,
    error: Option<DriverErrorCode>,
    now: DateTime<Utc>,
) -> StoreResult<bool> {
    let mut conn = store.lock();
    let tx = conn.transaction()?;
    let (current, _) = metadata(&tx, id)?;
    if current != observed {
        return Ok(false);
    }
    let delay = match error {
        None => Some(ADVANCE_DELAY_SECONDS),
        Some(code) if code.reconcile_locally() => Some(RECONCILE_DELAY_SECONDS),
        Some(_) => None,
    };
    let next = delay.map(|seconds| stamp(now + ChronoDuration::seconds(seconds)));
    tx.execute(
        "INSERT INTO acquisition_automation(acquisition_id,observed_version,last_serviced_at,next_check_at,error_code)
         VALUES (?1,?2,?3,?4,?5) ON CONFLICT(acquisition_id) DO UPDATE SET
         observed_version=excluded.observed_version,last_serviced_at=excluded.last_serviced_at,
         next_check_at=excluded.next_check_at,error_code=excluded.error_code",
        params![id, i64::try_from(observed).map_err(|_| invalid("acquisition version overflow"))?, stamp(now), next, error.map(DriverErrorCode::label)],
    )?;
    tx.commit()?;
    Ok(true)
}

fn classify(error: &AcquisitionRuntimeError) -> DriverErrorCode {
    use AcquisitionRuntimeError::*;
    match error {
        RetryRequired => DriverErrorCode::RetryRead,
        UnknownSuite | Unconfigured(_) => DriverErrorCode::Configuration,
        Store(AcquisitionStoreError::QuotaExceeded(_)) => DriverErrorCode::Quota,
        Store(AcquisitionStoreError::InvalidPolicy | AcquisitionStoreError::PolicyChanged) => {
            DriverErrorCode::Configuration
        }
        Store(
            AcquisitionStoreError::InvalidRecord
            | AcquisitionStoreError::Encoding
            | AcquisitionStoreError::NotFound,
        )
        | Acquire(_) => DriverErrorCode::InvalidRecord,
        ConcurrentChange
        | StaleSchedule
        | Store(AcquisitionStoreError::Conflict)
        | Persistence(StoreError::RevisionConflict { .. }) => DriverErrorCode::Conflict,
        _ => DriverErrorCode::Internal,
    }
}

/// At most four one-step services. Provider I/O is bounded by the configured
/// clients and never holds the kernel lock. A paused failed read is only locally
/// reconciled for expiry; the worker never supplies retry_read=true.
pub async fn drain_once(app: &AppState) -> StoreResult<DrainSummary> {
    let mut summary = DrainSummary::default();
    if !app.automatic_acquisition {
        return Ok(summary);
    }
    for candidate in candidates(&app.store, Utc::now())? {
        if !save_schedule(
            &app.store,
            &candidate.id,
            candidate.version,
            candidate.error,
            Utc::now(),
        )? {
            summary.stale += 1;
            continue;
        }
        summary.serviced += 1;
        let local_only = candidate.error.is_some();
        let result = if local_only {
            app.acquisition_runtime
                .reconcile_if_current(&candidate.id, candidate.version)
                .await
        } else {
            app.acquisition_runtime
                .step_if_current(&candidate.id, candidate.version)
                .await
        };
        match result {
            Ok(record) => {
                let code = if local_only && record.version() == candidate.version {
                    candidate.error
                } else {
                    None
                };
                if save_schedule(
                    &app.store,
                    &candidate.id,
                    record.version(),
                    code,
                    Utc::now(),
                )? {
                    if record.version() != candidate.version {
                        summary.advanced += 1;
                    }
                } else {
                    summary.stale += 1;
                }
            }
            Err(AcquisitionRuntimeError::StaleSchedule) => {
                summary.stale += 1;
            }
            Err(error) => {
                let code = classify(&error);
                if save_schedule(
                    &app.store,
                    &candidate.id,
                    candidate.version,
                    Some(code),
                    Utc::now(),
                )? {
                    summary.attention += 1;
                    tracing::warn!(acquisition_id=%candidate.id, code=code.label(), "automatic acquisition requires attention");
                } else {
                    summary.stale += 1;
                }
            }
        }
    }
    Ok(summary)
}

pub fn spawn(app: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if drain_once(&app).await.is_err() {
                tracing::error!("automatic acquisition stopped after scheduler storage failure");
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests;
