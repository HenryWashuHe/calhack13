//! Review tests for the automatic invocation finalization queue
//! (`capability_finalization`): the persisted automatic-attempt budget and
//! the stored-row consistency checks that fail closed on corrupted rows.
//!
//! Scheduling metadata only. Job/run/binding rows are seeded with deliberately
//! invalid JSON (as the module's own unit tests do), so every finalization
//! attempt fails locally and no provider, evidence import or proof is involved.
use genesis_core::ids::JobId;
use genesis_server::{
    app::AppState,
    build_org,
    capability_finalization::{
        drain_once, recover, DrainSummary, FinalizationErrorCode, FinalizationState,
        MAX_AUTOMATIC_ATTEMPTS,
    },
    OrgConfig,
};
use rusqlite::{params, Connection};

const PAST: &str = "2000-01-01T00:00:00.000000000Z";
const STAMP: &str = "2026-10-21T12:00:00.000000000Z";

struct Fixture {
    _dir: tempfile::TempDir,
    app: AppState,
    db: std::path::PathBuf,
}
impl Fixture {
    fn new(org: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("org.sqlite");
        let mut config = OrgConfig::dev(org, "Finalization review");
        config.db = Some(db.clone());
        let app = build_org(&config, None).unwrap();
        Self { _dir: dir, app, db }
    }
    fn sql(&self) -> Connection {
        Connection::open(&self.db).unwrap()
    }
    /// A bound, collected invocation job awaiting verification.
    fn seed(&self, name: &str) -> JobId {
        let conn = self.sql();
        conn.execute(
            "INSERT INTO jobs(job_id,client_request_id,status,job_json,created_at,updated_at)
             VALUES (?1,?1,'Verifying','{}',?2,?2)",
            params![name, STAMP],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO capability_runs(job_id,client_request_id,phase,record_json)
             VALUES (?1,?1,'collected','{}')",
            params![name],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO capability_invocation_bindings(job_id,binding_json) VALUES (?1,'{}')",
            params![name],
        )
        .unwrap();
        JobId::new(name)
    }
    fn write_row(
        &self,
        id: &JobId,
        state: &str,
        attempts: u32,
        next: Option<&str>,
        error: Option<&str>,
    ) {
        self.sql()
            .execute(
                "INSERT OR REPLACE INTO capability_invocation_attempts(job_id,state,attempts,next_attempt_at,error_code,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![id.as_str(), state, attempts, next, error, STAMP],
            )
            .unwrap();
    }
    fn make_due(&self, id: &JobId) {
        assert_eq!(
            self.sql()
                .execute(
                    "UPDATE capability_invocation_attempts SET next_attempt_at=?1 WHERE job_id=?2 AND state='pending'",
                    params![PAST, id.as_str()],
                )
                .unwrap(),
            1
        );
    }
}

/// Every automatic attempt is interrupted before its disposition commits, as a
/// process crash would leave it. Startup recovery turns each running claim into
/// an `interrupted` retry until the persisted budget is exhausted. The queue
/// must make exactly MAX_AUTOMATIC_ATTEMPTS claims and then stop for attention.
#[tokio::test]
async fn crash_looping_finalization_makes_exactly_the_automatic_budget_then_stops() {
    let f = Fixture::new("review-finalization-budget");
    let id = f.seed("crash-loop");
    // Simulated crash: the worker's own failure disposition never commits.
    // Recovery's `interrupted` disposition (pending or final attention) does.
    f.sql()
        .execute_batch(
            "CREATE TRIGGER crash_before_disposition BEFORE UPDATE ON capability_invocation_attempts
             WHEN NEW.state='completed' OR (NEW.state='attention' AND NEW.error_code<>'interrupted')
             BEGIN SELECT RAISE(ABORT,'simulated crash before disposition'); END;",
        )
        .unwrap();
    let mut claims = 0;
    for attempt in 1..=MAX_AUTOMATIC_ATTEMPTS {
        assert!(
            drain_once(&f.app).await.is_err(),
            "attempt {attempt} should crash"
        );
        claims += 1;
        let running = f.app.store.capability_finalization_status(&id).unwrap();
        assert_eq!(
            (running.state, running.attempts),
            (FinalizationState::Running, attempt)
        );
        assert_eq!(recover(&f.app).await.unwrap(), 1);
        let status = f.app.store.capability_finalization_status(&id).unwrap();
        assert_eq!(status.attempts, attempt);
        assert_eq!(status.error_code, Some(FinalizationErrorCode::Interrupted));
        if attempt < MAX_AUTOMATIC_ATTEMPTS {
            assert_eq!(status.state, FinalizationState::Pending);
            assert!(status.next_attempt_at.is_some());
            // Not yet due: no claim before the backoff elapses.
            assert_eq!(drain_once(&f.app).await.unwrap(), DrainSummary::default());
            f.make_due(&id);
        } else {
            assert_eq!(status.state, FinalizationState::Attention);
            assert!(status.next_attempt_at.is_none());
        }
    }
    assert_eq!(claims, MAX_AUTOMATIC_ATTEMPTS);
    // Budget exhausted: no further claim, ever, and recovery has nothing to do.
    for _ in 0..3 {
        assert_eq!(drain_once(&f.app).await.unwrap(), DrainSummary::default());
    }
    assert_eq!(recover(&f.app).await.unwrap(), 0);
    let last = f.app.store.capability_finalization_status(&id).unwrap();
    assert_eq!(
        (last.state, last.attempts, last.error_code),
        (
            FinalizationState::Attention,
            MAX_AUTOMATIC_ATTEMPTS,
            Some(FinalizationErrorCode::Interrupted)
        )
    );
}

/// Stored-row corruption defenses: a row whose attempt count contradicts its
/// state must fail closed instead of being reported (or scheduled) as valid.
#[tokio::test]
async fn attempt_counts_inconsistent_with_state_fail_closed() {
    let f = Fixture::new("review-finalization-corruption");
    let id = f.seed("stored-row");
    let max = MAX_AUTOMATIC_ATTEMPTS;
    let last_retry = max - 1;
    // Valid boundary controls: each must read back exactly.
    for (state, attempts, next, error, expected) in [
        ("pending", 0, Some(PAST), None, FinalizationState::Pending),
        (
            "pending",
            last_retry,
            Some(PAST),
            Some("interrupted"),
            FinalizationState::Pending,
        ),
        ("running", 1, None, None, FinalizationState::Running),
        ("running", max, None, None, FinalizationState::Running),
        (
            "attention",
            1,
            None,
            Some("internal"),
            FinalizationState::Attention,
        ),
        (
            "attention",
            max,
            None,
            Some("revision_conflict"),
            FinalizationState::Attention,
        ),
    ] {
        f.write_row(&id, state, attempts, next, error);
        let status = f
            .app
            .store
            .capability_finalization_status(&id)
            .unwrap_or_else(|e| panic!("valid {state}/{attempts} rejected: {e}"));
        assert_eq!((status.state, status.attempts), (expected, attempts));
    }
    // Corrupted rows. A pending row may never hold the whole budget (a retry
    // would be attempt MAX+1); running/attention rows imply at least one claim.
    for (state, attempts, next, error) in [
        ("pending", max, Some(PAST), Some("interrupted")),
        ("pending", max, Some(PAST), Some("revision_conflict")),
        ("running", 0, None, None),
        ("attention", 0, None, Some("internal")),
        ("attention", 0, None, Some("interrupted")),
    ] {
        f.write_row(&id, state, attempts, next, error);
        assert!(
            f.app.store.capability_finalization_status(&id).is_err(),
            "corrupt {state}/{attempts} row was accepted"
        );
    }
    // A corrupt exhausted pending row is not silently claimed for attempt MAX+1.
    f.write_row(&id, "pending", max, Some(PAST), Some("interrupted"));
    assert_eq!(drain_once(&f.app).await.unwrap().claimed, 0);
    assert!(f.app.store.capability_finalization_status(&id).is_err());
}
