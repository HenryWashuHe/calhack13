use genesis_creao::{AccountLimiter, LimiterError, QuotaLimits, RequestKind};
use std::time::Duration;
use tokio::time::Instant;

fn one() -> QuotaLimits {
    QuotaLimits {
        requests_per_minute: 1,
        management_writes_per_hour: 1,
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

#[test]
fn namespace_and_quota_configuration_cannot_silently_change_a_shared_budget() {
    for namespace in ["", " leading", "trailing ", "line\nbreak", "💥"] {
        assert!(matches!(
            AccountLimiter::in_memory(namespace, one()),
            Err(LimiterError::InvalidConfiguration)
        ));
    }
    assert!(matches!(
        AccountLimiter::in_memory(&"a".repeat(257), one()),
        Err(LimiterError::InvalidConfiguration)
    ));
    assert!(matches!(
        AccountLimiter::in_memory(
            "account",
            QuotaLimits {
                requests_per_minute: 0,
                ..one()
            }
        ),
        Err(LimiterError::InvalidConfiguration)
    ));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("account-limits.db");
    let _first = AccountLimiter::open(&path, "same-account", one()).unwrap();
    assert!(matches!(
        AccountLimiter::open(&path, "same-account", QuotaLimits::default()),
        Err(LimiterError::InvalidConfiguration)
    ));
    assert!(AccountLimiter::open(&path, "another-account", QuotaLimits::default()).is_ok());
}

#[tokio::test]
async fn clones_and_reopened_handles_share_consumed_quota() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("account-limits.db");
    let first = AccountLimiter::open(&path, "account", one()).unwrap();
    let clone = first.clone();
    first.acquire(RequestKind::Read, deadline()).await.unwrap();
    drop(first);
    assert_eq!(
        clone
            .acquire(RequestKind::Run, Instant::now() + Duration::from_millis(30))
            .await,
        Err(LimiterError::DeadlineExceeded)
    );
    drop(clone);
    let reopened = AccountLimiter::open(&path, "account", one()).unwrap();
    assert_eq!(
        reopened
            .acquire(
                RequestKind::ManagementWrite,
                Instant::now() + Duration::from_millis(30)
            )
            .await,
        Err(LimiterError::DeadlineExceeded)
    );
    let other_account = AccountLimiter::open(&path, "other-account", one()).unwrap();
    other_account
        .acquire(RequestKind::Run, deadline())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_connections_compete_atomically_for_one_account_budget() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("account-limits.db");
    let limits = QuotaLimits {
        requests_per_minute: 4,
        management_writes_per_hour: 1,
    };
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let limiter = AccountLimiter::open(&path, "shared-account", limits).unwrap();
        tasks.push(tokio::spawn(async move {
            limiter
                .acquire(
                    RequestKind::Run,
                    Instant::now() + Duration::from_millis(150),
                )
                .await
        }));
    }
    let mut admitted = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(()) => admitted += 1,
            Err(LimiterError::DeadlineExceeded) => {}
            other => panic!("unexpected limiter result: {other:?}"),
        }
    }
    assert_eq!(admitted, 4);
    let sql = rusqlite::Connection::open(path).unwrap();
    let count: u32 = sql
        .query_row(
            "SELECT COUNT(*) FROM creao_account_reservations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 4);
}

#[tokio::test]
async fn persistent_account_cooldown_applies_to_every_operation_and_never_shortens() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("account-limits.db");
    {
        let first = AccountLimiter::open(&path, "account", QuotaLimits::default()).unwrap();
        first.defer_for(Duration::from_millis(180)).unwrap();
    }
    let second = AccountLimiter::open(&path, "account", QuotaLimits::default()).unwrap();
    second.defer_for(Duration::from_millis(1)).unwrap();
    for kind in [
        RequestKind::Read,
        RequestKind::Run,
        RequestKind::ManagementWrite,
    ] {
        assert_eq!(
            second
                .acquire(kind, Instant::now() + Duration::from_millis(20))
                .await,
            Err(LimiterError::DeadlineExceeded)
        );
    }
    second.acquire(RequestKind::Run, deadline()).await.unwrap();
    let sql = rusqlite::Connection::open(path).unwrap();
    let count: u32 = sql
        .query_row(
            "SELECT COUNT(*) FROM creao_account_reservations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "cooldown waits must not consume request quota");
}

#[tokio::test]
async fn sqlite_contention_obeys_async_deadline_without_resetting_quota() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("account-limits.db");
    let limiter = AccountLimiter::open(&path, "account", one()).unwrap();
    let blocker = rusqlite::Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let started = Instant::now();
    assert_eq!(
        limiter
            .acquire(RequestKind::Read, started + Duration::from_millis(40))
            .await,
        Err(LimiterError::DeadlineExceeded)
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    blocker.execute_batch("ROLLBACK;").unwrap();
    limiter
        .acquire(RequestKind::Read, deadline())
        .await
        .unwrap();
}

#[tokio::test]
async fn expired_deadline_does_not_take_a_reservation_and_errors_are_sanitized() {
    let limiter = AccountLimiter::in_memory("account", one()).unwrap();
    assert_eq!(
        limiter.acquire(RequestKind::Run, Instant::now()).await,
        Err(LimiterError::DeadlineExceeded)
    );
    limiter.acquire(RequestKind::Run, deadline()).await.unwrap();
    let error = AccountLimiter::open("/nonexistent/SECRET/path/quota.sqlite", "account", one())
        .unwrap_err();
    assert_eq!(error, LimiterError::Storage);
    assert!(!format!("{error:?} {error}").contains("SECRET"));
    assert_eq!(
        limiter.defer_for(Duration::MAX),
        Err(LimiterError::InvalidConfiguration)
    );
}

#[tokio::test]
async fn reader_blocking_commit_retries_without_leaking_a_reservation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("account-limits.db");
    let limiter = AccountLimiter::open(&path, "account", one()).unwrap();
    let reader = rusqlite::Connection::open(&path).unwrap();
    // A deferred transaction holding a read lock permits BEGIN IMMEDIATE but
    // blocks its later COMMIT in rollback-journal mode. This is a different
    // contention boundary from another writer blocking BEGIN IMMEDIATE.
    reader
        .execute_batch("BEGIN DEFERRED; SELECT * FROM creao_account_limits;")
        .unwrap();
    assert_eq!(
        limiter
            .acquire(
                RequestKind::Read,
                Instant::now() + Duration::from_millis(40)
            )
            .await,
        Err(LimiterError::DeadlineExceeded)
    );
    reader.execute_batch("ROLLBACK;").unwrap();
    let count: u32 = reader
        .query_row(
            "SELECT COUNT(*) FROM creao_account_reservations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "failed commits must roll back attempted reservations"
    );
    limiter
        .acquire(RequestKind::Read, deadline())
        .await
        .unwrap();
    let count: u32 = reader
        .query_row(
            "SELECT COUNT(*) FROM creao_account_reservations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
