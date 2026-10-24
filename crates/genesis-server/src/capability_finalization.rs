//! Bounded automatic local finalization of backend-bound capability invocations.
//! This queue never calls a provider, imports evidence, or retries an external
//! effect. Its status is scheduling metadata; only the verifier issues a proof.

use crate::app::AppState;
use crate::capability_invocation;
use crate::error::ApiError;
use crate::store::{Store, StoreError, StoreResult};
use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use genesis_core::ids::JobId;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::time::Duration;
use tokio::task::JoinHandle;

pub const MAX_AUTOMATIC_ATTEMPTS: u32 = 3;
pub const FINALIZATION_BATCH_LIMIT: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizationState {
    Pending,
    Running,
    Attention,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizationErrorCode {
    Interrupted,
    RevisionConflict,
    InvalidBinding,
    InvalidRecord,
    MissingRecord,
    Internal,
}

impl FinalizationErrorCode {
    fn label(self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted",
            Self::RevisionConflict => "revision_conflict",
            Self::InvalidBinding => "invalid_binding",
            Self::InvalidRecord => "invalid_record",
            Self::MissingRecord => "missing_record",
            Self::Internal => "internal",
        }
    }

    fn parse(value: &str) -> StoreResult<Self> {
        match value {
            "interrupted" => Ok(Self::Interrupted),
            "revision_conflict" => Ok(Self::RevisionConflict),
            "invalid_binding" => Ok(Self::InvalidBinding),
            "invalid_record" => Ok(Self::InvalidRecord),
            "missing_record" => Ok(Self::MissingRecord),
            "internal" => Ok(Self::Internal),
            _ => Err(invalid("invalid finalization error code")),
        }
    }
}

/// Operator-safe scheduling information, never a semantic verification proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FinalizationStatus {
    pub job_id: JobId,
    pub state: FinalizationState,
    pub attempts: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub error_code: Option<FinalizationErrorCode>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DrainSummary {
    pub claimed: usize,
    pub completed: usize,
    pub attention: usize,
    pub retried: usize,
}

fn invalid(message: &'static str) -> StoreError {
    StoreError::Db(rusqlite::Error::InvalidParameterName(message.into()))
}

fn stamp(now: DateTime<Utc>) -> String {
    // Fixed precision and UTC allow indexed lexical ordering of queue dates.
    now.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_time(value: &str) -> StoreResult<DateTime<Utc>> {
    if value.len() > 40 {
        return Err(invalid("invalid finalization timestamp"));
    }
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| invalid("invalid finalization timestamp"))
}

type AttemptRow = (String, i64, Option<String>, Option<String>, String);

fn read_attempt(conn: &Connection, id: &JobId) -> StoreResult<Option<FinalizationStatus>> {
    let row: Option<AttemptRow> = conn
        .query_row(
            "SELECT state,attempts,next_attempt_at,error_code,updated_at
             FROM capability_invocation_attempts WHERE job_id=?1",
            params![id.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    row.map(|(state, attempts, next, error, updated)| {
        let state = match state.as_str() {
            "pending" => FinalizationState::Pending,
            "running" => FinalizationState::Running,
            "attention" => FinalizationState::Attention,
            "completed" => FinalizationState::Completed,
            _ => return Err(invalid("invalid finalization state")),
        };
        let attempts = u32::try_from(attempts)
            .ok()
            .filter(|count| *count <= MAX_AUTOMATIC_ATTEMPTS)
            .ok_or_else(|| invalid("invalid finalization attempt count"))?;
        let next_attempt_at = next.as_deref().map(parse_time).transpose()?;
        let error_code = error
            .as_deref()
            .map(FinalizationErrorCode::parse)
            .transpose()?;
        let valid = match state {
            FinalizationState::Pending => {
                attempts < MAX_AUTOMATIC_ATTEMPTS
                    && next_attempt_at.is_some()
                    && if attempts == 0 {
                        error_code.is_none()
                    } else {
                        matches!(
                            error_code,
                            Some(
                                FinalizationErrorCode::Interrupted
                                    | FinalizationErrorCode::RevisionConflict
                            )
                        )
                    }
            }
            FinalizationState::Running => {
                attempts > 0 && next_attempt_at.is_none() && error_code.is_none()
            }
            FinalizationState::Attention => {
                attempts > 0 && next_attempt_at.is_none() && error_code.is_some()
            }
            FinalizationState::Completed => next_attempt_at.is_none() && error_code.is_none(),
        };
        if !valid {
            return Err(invalid("inconsistent finalization state"));
        }
        Ok(FinalizationStatus {
            job_id: id.clone(),
            state,
            attempts,
            next_attempt_at,
            error_code,
            updated_at: parse_time(&updated)?,
        })
    })
    .transpose()
}

fn has_proof(conn: &Connection, id: &JobId) -> StoreResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM capability_invocation_verifications WHERE job_id=?1)",
        params![id.as_str()],
        |row| row.get(0),
    )?)
}

impl Store {
    pub fn init_capability_finalization(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_invocation_attempts (
                job_id TEXT PRIMARY KEY REFERENCES jobs(job_id),
                state TEXT NOT NULL CHECK(state IN ('pending','running','attention','completed')),
                attempts INTEGER NOT NULL CHECK(attempts BETWEEN 0 AND 3),
                next_attempt_at TEXT,
                error_code TEXT CHECK(error_code IN ('interrupted','revision_conflict','invalid_binding','invalid_record','missing_record','internal')),
                updated_at TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS capability_finalization_due
                ON capability_invocation_attempts(state,next_attempt_at);",
        )?;
        Ok(())
    }

    pub fn capability_finalization_status(&self, id: &JobId) -> StoreResult<FinalizationStatus> {
        let conn = self.lock();
        let updated: Option<String> = conn
            .query_row(
                "SELECT j.updated_at FROM jobs j
                 JOIN capability_invocation_bindings b ON b.job_id=j.job_id
                 WHERE j.job_id=?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let updated =
            updated.ok_or_else(|| StoreError::NotFound("bound capability invocation".into()))?;
        let mut status = read_attempt(&conn, id)?.unwrap_or(FinalizationStatus {
            job_id: id.clone(),
            state: FinalizationState::Pending,
            attempts: 0,
            next_attempt_at: None,
            error_code: None,
            updated_at: parse_time(&updated)?,
        });
        if has_proof(&conn, id)? {
            status.state = FinalizationState::Completed;
            status.next_attempt_at = None;
            status.error_code = None;
            status.updated_at = parse_time(&updated)?;
        } else if status.state == FinalizationState::Completed {
            return Err(invalid("completed finalization lacks a proof"));
        }
        Ok(status)
    }
}

/// Called by the semantic proof transaction; absent automatic rows are normal
/// for manual finalization. A failed update rolls the entire proof commit back.
pub(crate) fn mark_completed(
    tx: &Transaction<'_>,
    id: &JobId,
    now: DateTime<Utc>,
) -> StoreResult<()> {
    if read_attempt(tx, id)?.is_some() {
        if !has_proof(tx, id)? {
            return Err(invalid("cannot complete finalization without a proof"));
        }
        tx.execute(
            "UPDATE capability_invocation_attempts SET state='completed',next_attempt_at=NULL,error_code=NULL,updated_at=?2
             WHERE job_id=?1",
            params![id.as_str(), stamp(now)],
        )?;
    }
    Ok(())
}

fn candidates(store: &Store, now: DateTime<Utc>) -> StoreResult<Vec<JobId>> {
    let conn = store.lock();
    let mut statement = conn.prepare(
        "SELECT b.job_id FROM capability_invocation_bindings b
         JOIN capability_runs r ON r.job_id=b.job_id
         JOIN jobs j ON j.job_id=b.job_id
         LEFT JOIN capability_invocation_attempts a ON a.job_id=b.job_id
         WHERE r.phase='collected' AND j.status='Verifying'
           AND NOT EXISTS(SELECT 1 FROM capability_invocation_verifications p WHERE p.job_id=b.job_id)
           AND (a.job_id IS NULL OR (a.state='pending' AND a.attempts<3 AND a.next_attempt_at<=?1))
         ORDER BY COALESCE(a.next_attempt_at,j.updated_at),b.job_id LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![stamp(now), FINALIZATION_BATCH_LIMIT as i64],
        |row| row.get::<_, String>(0),
    )?;
    rows.map(|row| row.map(JobId::new).map_err(StoreError::from))
        .collect()
}

/// Caller holds the kernel lock. Recheck eligibility inside the claim transaction
/// so a stale scan cannot claim a manually finalized or no-longer-collected job.
fn claim(store: &Store, id: &JobId, now: DateTime<Utc>) -> StoreResult<Option<u32>> {
    let mut conn = store.lock();
    let tx = conn.transaction()?;
    let eligible: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capability_invocation_bindings b
         JOIN capability_runs r ON r.job_id=b.job_id JOIN jobs j ON j.job_id=b.job_id
         WHERE b.job_id=?1 AND r.phase='collected' AND j.status='Verifying'
           AND NOT EXISTS(SELECT 1 FROM capability_invocation_verifications p WHERE p.job_id=b.job_id))",
        params![id.as_str()],
        |row| row.get(0),
    )?;
    if !eligible {
        return Ok(None);
    }
    let previous = read_attempt(&tx, id)?;
    let attempts = match previous {
        None => 1,
        Some(status)
            if status.state == FinalizationState::Pending
                && status.attempts < MAX_AUTOMATIC_ATTEMPTS
                && status.next_attempt_at.is_some_and(|due| due <= now) =>
        {
            status.attempts + 1
        }
        _ => return Ok(None),
    };
    tx.execute(
        "INSERT INTO capability_invocation_attempts(job_id,state,attempts,next_attempt_at,error_code,updated_at)
         VALUES (?1,'running',?2,NULL,NULL,?3)
         ON CONFLICT(job_id) DO UPDATE SET state='running',attempts=?2,next_attempt_at=NULL,error_code=NULL,updated_at=?3",
        params![id.as_str(), attempts, stamp(now)],
    )?;
    tx.commit()?;
    Ok(Some(attempts))
}

fn classify(error: &ApiError) -> FinalizationErrorCode {
    match error.error.as_str() {
        "revision_conflict" => FinalizationErrorCode::RevisionConflict,
        "invocation_binding" => FinalizationErrorCode::InvalidBinding,
        "capability_run_state" => FinalizationErrorCode::InvalidRecord,
        "not_found" => FinalizationErrorCode::MissingRecord,
        _ => FinalizationErrorCode::Internal,
    }
}

fn disposition(
    tx: &Transaction<'_>,
    id: &JobId,
    attempts: u32,
    code: FinalizationErrorCode,
    now: DateTime<Utc>,
) -> StoreResult<bool> {
    let retry = matches!(
        code,
        FinalizationErrorCode::Interrupted | FinalizationErrorCode::RevisionConflict
    ) && attempts < MAX_AUTOMATIC_ATTEMPTS;
    let next =
        retry.then(|| stamp(now + ChronoDuration::seconds(if attempts == 1 { 1 } else { 5 })));
    let count = tx.execute(
        "UPDATE capability_invocation_attempts SET state=?3,next_attempt_at=?4,error_code=?5,updated_at=?6
         WHERE job_id=?1 AND state='running' AND attempts=?2",
        params![id.as_str(), attempts, if retry { "pending" } else { "attention" }, next, code.label(), stamp(now)],
    )?;
    if count != 1 {
        return Err(invalid("finalization claim changed"));
    }
    Ok(retry)
}

/// Startup-only recovery. No provider effects occurred in an interrupted local
/// claim, so it may be recomputed within the original durable attempt budget.
pub async fn recover(app: &AppState) -> StoreResult<usize> {
    let _kernel = app.kernel.lock().await;
    let now = Utc::now();
    let mut changed = 0;
    loop {
        let ids = {
            let conn = app.store.lock();
            let mut statement = conn.prepare(
                "SELECT job_id FROM capability_invocation_attempts WHERE state='running' ORDER BY job_id LIMIT ?1",
            )?;
            let rows = statement.query_map(params![FINALIZATION_BATCH_LIMIT as i64], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if ids.is_empty() {
            break;
        }
        for id in ids {
            let id = JobId::new(id);
            let mut conn = app.store.lock();
            let tx = conn.transaction()?;
            let status =
                read_attempt(&tx, &id)?.ok_or_else(|| invalid("missing finalization claim"))?;
            if has_proof(&tx, &id)? {
                mark_completed(&tx, &id, now)?;
            } else {
                disposition(
                    &tx,
                    &id,
                    status.attempts,
                    FinalizationErrorCode::Interrupted,
                    now,
                )?;
            }
            tx.commit()?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// One bounded local pass. Each claim, computation, and durable disposition is
/// serialized with manual finalization and organization mutations by the kernel.
pub async fn drain_once(app: &AppState) -> StoreResult<DrainSummary> {
    let ids = candidates(&app.store, Utc::now())?;
    let mut summary = DrainSummary::default();
    for id in ids {
        let _kernel = app.kernel.lock().await;
        let Some(attempts) = claim(&app.store, &id, Utc::now())? else {
            continue;
        };
        summary.claimed += 1;
        match capability_invocation::finalize_locked(app, &id) {
            Ok(_) => {
                summary.completed += 1;
            }
            Err(error) => {
                let mut conn = app.store.lock();
                let tx = conn.transaction()?;
                let retry = disposition(&tx, &id, attempts, classify(&error), Utc::now())?;
                tx.commit()?;
                if retry {
                    summary.retried += 1;
                } else {
                    summary.attention += 1;
                }
                tracing::warn!(job_id=%id, code=classify(&error).label(), "automatic invocation finalization requires attention");
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
            if let Err(error) = drain_once(&app).await {
                tracing::error!(%error, "automatic invocation finalization stopped after a storage failure");
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_org, OrgConfig};
    use axum::http::StatusCode;
    use genesis_core::ids::OrgId;

    fn now() -> DateTime<Utc> {
        "2026-10-21T12:00:00Z".parse().unwrap()
    }

    fn store() -> Store {
        let store = Store::open_in_memory(OrgId::new("finalization-test")).unwrap();
        store.init_capability_runs().unwrap();
        store.init_capability_invocations().unwrap();
        store.init_capability_finalization().unwrap();
        store
    }

    // Only scheduling metadata is under test. Intentionally invalid job/record
    // JSON ensures the ID-only selector does not import or deserialize payloads.
    fn seed(store: &Store, name: &str, bound: bool) -> JobId {
        let id = JobId::new(name);
        let conn = store.lock();
        conn.execute(
            "INSERT INTO jobs(job_id,client_request_id,status,job_json,created_at,updated_at)
             VALUES (?1,?1,'Verifying','{}',?2,?2)",
            params![name, stamp(now())],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO capability_runs(job_id,client_request_id,phase,record_json)
             VALUES (?1,?1,'collected','{}')",
            params![name],
        )
        .unwrap();
        if bound {
            conn.execute(
                "INSERT INTO capability_invocation_bindings(job_id,binding_json) VALUES (?1,'{}')",
                params![name],
            )
            .unwrap();
        }
        id
    }

    fn proof(conn: &Connection, id: &JobId) {
        conn.execute(
            "INSERT INTO capability_invocation_verifications(job_id,proof_json) VALUES (?1,'{}')",
            params![id.as_str()],
        )
        .unwrap();
    }

    fn fail(
        store: &Store,
        id: &JobId,
        attempts: u32,
        code: FinalizationErrorCode,
        time: DateTime<Utc>,
    ) -> bool {
        let mut conn = store.lock();
        let tx = conn.transaction().unwrap();
        let retry = disposition(&tx, id, attempts, code, time).unwrap();
        tx.commit().unwrap();
        retry
    }

    #[test]
    fn candidates_are_bounded_ids_and_exclude_unbound_incomplete_and_finished_jobs() {
        let store = store();
        seed(&store, "00-unbound", false);
        let prepared = seed(&store, "01-prepared", true);
        store
            .lock()
            .execute(
                "UPDATE capability_runs SET phase='prepared' WHERE job_id=?1",
                params![prepared.as_str()],
            )
            .unwrap();
        let terminal = seed(&store, "02-terminal", true);
        store
            .lock()
            .execute(
                "UPDATE jobs SET status='Succeeded' WHERE job_id=?1",
                params![terminal.as_str()],
            )
            .unwrap();
        let finished = seed(&store, "03-finished", true);
        proof(&store.lock(), &finished);
        for i in 0..20 {
            seed(&store, &format!("eligible-{i:02}"), true);
        }
        let selected = candidates(&store, now()).unwrap();
        assert_eq!(selected.len(), FINALIZATION_BATCH_LIMIT);
        assert_eq!(selected.first().unwrap().as_str(), "eligible-00");
        assert_eq!(selected.last().unwrap().as_str(), "eligible-15");
    }

    #[test]
    fn claim_is_exclusive_rechecks_proof_and_never_exceeds_persisted_budget() {
        let store = store();
        let id = seed(&store, "retry", true);
        assert_eq!(claim(&store, &id, now()).unwrap(), Some(1));
        assert_eq!(claim(&store, &id, now()).unwrap(), None);
        assert!(fail(
            &store,
            &id,
            1,
            FinalizationErrorCode::RevisionConflict,
            now()
        ));
        assert!(
            candidates(&store, now() + ChronoDuration::milliseconds(999))
                .unwrap()
                .is_empty()
        );
        let due = now() + ChronoDuration::seconds(1);
        assert_eq!(candidates(&store, due).unwrap(), vec![id.clone()]);
        assert_eq!(claim(&store, &id, due).unwrap(), Some(2));
        assert!(fail(
            &store,
            &id,
            2,
            FinalizationErrorCode::Interrupted,
            due
        ));
        let due = due + ChronoDuration::seconds(5);
        assert_eq!(claim(&store, &id, due).unwrap(), Some(3));
        assert!(!fail(
            &store,
            &id,
            3,
            FinalizationErrorCode::RevisionConflict,
            due
        ));
        let status = store.capability_finalization_status(&id).unwrap();
        assert_eq!(status.state, FinalizationState::Attention);
        assert_eq!(status.attempts, MAX_AUTOMATIC_ATTEMPTS);
        assert_eq!(
            status.error_code,
            Some(FinalizationErrorCode::RevisionConflict)
        );
        assert_eq!(
            claim(&store, &id, due + ChronoDuration::days(1)).unwrap(),
            None
        );

        let done = seed(&store, "raced-manual-proof", true);
        assert!(candidates(&store, now()).unwrap().contains(&done));
        proof(&store.lock(), &done);
        assert_eq!(claim(&store, &done, now()).unwrap(), None);
    }

    #[test]
    fn status_and_completion_do_not_create_proof_authority() {
        let store = store();
        let id = seed(&store, "pending", true);
        let unbound = seed(&store, "unbound", false);
        assert!(matches!(
            store.capability_finalization_status(&unbound),
            Err(StoreError::NotFound(_))
        ));
        let initial = store.capability_finalization_status(&id).unwrap();
        assert_eq!(initial.state, FinalizationState::Pending);
        assert_eq!(initial.attempts, 0);
        assert_eq!(initial.next_attempt_at, None);
        claim(&store, &id, now()).unwrap();
        {
            let mut conn = store.lock();
            let tx = conn.transaction().unwrap();
            assert!(mark_completed(&tx, &id, now()).is_err());
        }
        assert_eq!(
            store.capability_finalization_status(&id).unwrap().state,
            FinalizationState::Running
        );
        {
            let mut conn = store.lock();
            let tx = conn.transaction().unwrap();
            proof(&tx, &id);
            mark_completed(&tx, &id, now()).unwrap();
            // Rolling back a failed proof transaction rolls back completion too.
        }
        assert!(!has_proof(&store.lock(), &id).unwrap());
        assert_eq!(
            store.capability_finalization_status(&id).unwrap().state,
            FinalizationState::Running
        );
        {
            let mut conn = store.lock();
            let tx = conn.transaction().unwrap();
            proof(&tx, &id);
            mark_completed(&tx, &id, now()).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            store.capability_finalization_status(&id).unwrap().state,
            FinalizationState::Completed
        );
        let manual = seed(&store, "manual", true);
        {
            let mut conn = store.lock();
            let tx = conn.transaction().unwrap();
            proof(&tx, &manual);
            mark_completed(&tx, &manual, now()).unwrap();
            tx.commit().unwrap();
        }
        let status = store.capability_finalization_status(&manual).unwrap();
        assert_eq!(
            (status.state, status.attempts),
            (FinalizationState::Completed, 0)
        );
        assert!(read_attempt(&store.lock(), &manual).unwrap().is_none());
    }

    #[test]
    fn errors_are_closed_codes_and_unknown_failures_never_auto_retry() {
        let store = store();
        let cases = [
            (
                "revision_conflict",
                FinalizationErrorCode::RevisionConflict,
                true,
            ),
            (
                "invocation_binding",
                FinalizationErrorCode::InvalidBinding,
                false,
            ),
            (
                "capability_run_state",
                FinalizationErrorCode::InvalidRecord,
                false,
            ),
            ("not_found", FinalizationErrorCode::MissingRecord, false),
            (
                "private-error-canary",
                FinalizationErrorCode::Internal,
                false,
            ),
        ];
        for (index, (code, expected, retries)) in cases.into_iter().enumerate() {
            let error = ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                code,
                "private-message-canary",
            )
            .with_details(serde_json::json!({"oracle":"private-oracle-canary"}));
            assert_eq!(classify(&error), expected);
            let id = seed(&store, &format!("error-{index}"), true);
            claim(&store, &id, now()).unwrap();
            assert_eq!(fail(&store, &id, 1, classify(&error), now()), retries);
            let status =
                serde_json::to_string(&store.capability_finalization_status(&id).unwrap()).unwrap();
            assert!(!status.contains("canary"));
        }
    }

    #[tokio::test]
    async fn interrupted_claims_recover_once_with_original_attempts_and_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = OrgConfig::dev("recovery", "Recovery");
        config.db = Some(dir.path().join("org.sqlite"));
        let app = build_org(&config, None).unwrap();
        let first = seed(&app.store, "first", true);
        let last = seed(&app.store, "last", true);
        let completed = seed(&app.store, "completed", true);
        for id in [&first, &last, &completed] {
            claim(&app.store, id, now()).unwrap();
        }
        app.store
            .lock()
            .execute(
                "UPDATE capability_invocation_attempts SET attempts=3 WHERE job_id=?1",
                params![last.as_str()],
            )
            .unwrap();
        proof(&app.store.lock(), &completed);
        drop(app);
        let app = build_org(&config, None).unwrap();
        assert_eq!(recover(&app).await.unwrap(), 3);
        assert_eq!(recover(&app).await.unwrap(), 0);
        let first_status = app.store.capability_finalization_status(&first).unwrap();
        assert_eq!(
            (first_status.state, first_status.attempts),
            (FinalizationState::Pending, 1)
        );
        assert_eq!(
            first_status.error_code,
            Some(FinalizationErrorCode::Interrupted)
        );
        assert!(first_status.next_attempt_at.is_some());
        let last_status = app.store.capability_finalization_status(&last).unwrap();
        assert_eq!(
            (last_status.state, last_status.attempts),
            (FinalizationState::Attention, 3)
        );
        assert_eq!(
            app.store
                .capability_finalization_status(&completed)
                .unwrap()
                .state,
            FinalizationState::Completed
        );
        drop(app);
        let reopened = build_org(&config, None).unwrap();
        assert_eq!(
            reopened
                .store
                .capability_finalization_status(&first)
                .unwrap(),
            first_status
        );
        assert_eq!(
            reopened
                .store
                .capability_finalization_status(&last)
                .unwrap(),
            last_status
        );
    }

    #[tokio::test]
    async fn bad_records_get_one_durable_attention_disposition_without_stopping_later_claims() {
        let app = build_org(&OrgConfig::dev("attention", "Attention"), None).unwrap();
        for i in 0..18 {
            seed(&app.store, &format!("bad-record-{i:02}"), true);
        }
        let first = drain_once(&app).await.unwrap();
        assert_eq!(
            first,
            DrainSummary {
                claimed: 16,
                attention: 16,
                completed: 0,
                retried: 0
            }
        );
        let second = drain_once(&app).await.unwrap();
        assert_eq!(
            second,
            DrainSummary {
                claimed: 2,
                attention: 2,
                completed: 0,
                retried: 0
            }
        );
        assert_eq!(drain_once(&app).await.unwrap(), DrainSummary::default());
        for i in 0..18 {
            let status = app
                .store
                .capability_finalization_status(&JobId::new(format!("bad-record-{i:02}")))
                .unwrap();
            assert_eq!(
                (status.state, status.attempts),
                (FinalizationState::Attention, 1)
            );
        }
    }

    #[tokio::test]
    async fn failure_to_persist_disposition_preserves_claim_and_stops_worker() {
        let app = build_org(&OrgConfig::dev("storage", "Storage"), None).unwrap();
        let id = seed(&app.store, "failed-write", true);
        app.store
            .lock()
            .execute_batch(
                "CREATE TRIGGER reject_disposition BEFORE UPDATE ON capability_invocation_attempts
             BEGIN SELECT RAISE(ABORT,'test storage fault'); END;",
            )
            .unwrap();
        let worker = spawn(app.clone());
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
        let status = app.store.capability_finalization_status(&id).unwrap();
        assert_eq!(
            (status.state, status.attempts),
            (FinalizationState::Running, 1)
        );
        assert!(!has_proof(&app.store.lock(), &id).unwrap());
        app.store
            .lock()
            .execute_batch("DROP TRIGGER reject_disposition")
            .unwrap();
        assert_eq!(recover(&app).await.unwrap(), 1);
        assert_eq!(
            app.store
                .capability_finalization_status(&id)
                .unwrap()
                .attempts,
            1
        );
    }

    #[test]
    fn inconsistent_attempt_rows_fail_closed() {
        let store = store();
        let id = seed(&store, "bad-state", true);
        claim(&store, &id, now()).unwrap();
        store.lock().execute("UPDATE capability_invocation_attempts SET state='attention',error_code=NULL WHERE job_id=?1", params![id.as_str()]).unwrap();
        assert!(store.capability_finalization_status(&id).is_err());
        let mut conn = store.lock();
        let tx = conn.transaction().unwrap();
        proof(&tx, &id);
        assert!(mark_completed(&tx, &id, now()).is_err());
    }
}
