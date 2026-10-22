//! Durable account quotas shared by every organization using that CREAO account.
//! The namespace identifies an account, not an API key: two keys for one account
//! must use the same namespace and SQLite file. Reservations are never refunded.

use rusqlite::{params, Connection, ErrorCode, TransactionBehavior};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

const REQUEST_WINDOW_MS: i64 = 60_000;
const MANAGEMENT_WINDOW_MS: i64 = 3_600_000;
const BUSY_RETRY: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind {
    Read,
    Run,
    ManagementWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaLimits {
    pub requests_per_minute: u32,
    pub management_writes_per_hour: u32,
}

impl Default for QuotaLimits {
    fn default() -> Self {
        Self {
            requests_per_minute: 30,
            management_writes_per_hour: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LimiterError {
    #[error("invalid account limiter namespace, quota, or cooldown")]
    InvalidConfiguration,
    #[error("account limiter deadline exceeded")]
    DeadlineExceeded,
    #[error("account limiter storage is unavailable")]
    Storage,
    #[error("account limiter clock is unavailable")]
    Clock,
}

#[derive(Clone)]
pub struct AccountLimiter {
    inner: Arc<Inner>,
}

struct Inner {
    namespace: String,
    connection: Mutex<Connection>,
}

impl std::fmt::Debug for AccountLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountLimiter")
            .field("namespace", &self.namespace())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Reservation {
    Granted,
    WaitUntil(i64),
    Busy,
}

impl AccountLimiter {
    /// The parent directory must be a trusted backend-owned directory. Every
    /// process sharing the account must open the same file and namespace.
    pub fn open(
        path: impl AsRef<Path>,
        namespace: &str,
        limits: QuotaLimits,
    ) -> Result<Self, LimiterError> {
        Self::initialize(
            Connection::open(path).map_err(|_| LimiterError::Storage)?,
            namespace,
            limits,
        )
    }

    /// In-memory storage is shared by clones only. Use a file for independent
    /// handles, separate organizations, or any execution that must survive restart.
    pub fn in_memory(namespace: &str, limits: QuotaLimits) -> Result<Self, LimiterError> {
        Self::initialize(
            Connection::open_in_memory().map_err(|_| LimiterError::Storage)?,
            namespace,
            limits,
        )
    }

    fn initialize(
        mut connection: Connection,
        namespace: &str,
        limits: QuotaLimits,
    ) -> Result<Self, LimiterError> {
        if namespace.is_empty()
            || namespace.len() > 256
            || namespace.trim() != namespace
            || !namespace.is_ascii()
            || namespace.bytes().any(|byte| byte.is_ascii_control())
            || limits.requests_per_minute == 0
            || limits.requests_per_minute > 1_000_000
            || limits.management_writes_per_hour == 0
            || limits.management_writes_per_hour > 1_000_000
        {
            return Err(LimiterError::InvalidConfiguration);
        }
        // Construction is synchronous. HTTP request acquisition below runs all
        // SQLite operations off the async executor and obeys its caller's deadline.
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| LimiterError::Storage)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS creao_account_limits (
            namespace TEXT PRIMARY KEY, requests_per_minute INTEGER NOT NULL,
            management_writes_per_hour INTEGER NOT NULL, last_seen_ms INTEGER NOT NULL,
            cooldown_until_ms INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS creao_account_reservations (
            namespace TEXT NOT NULL, reserved_at_ms INTEGER NOT NULL, management INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS creao_account_reservations_window
            ON creao_account_reservations(namespace,reserved_at_ms);",
            )
            .map_err(|_| LimiterError::Storage)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| LimiterError::Storage)?;
        tx.execute(
            "INSERT OR IGNORE INTO creao_account_limits VALUES (?1,?2,?3,0,0)",
            params![
                namespace,
                limits.requests_per_minute,
                limits.management_writes_per_hour
            ],
        )
        .map_err(|_| LimiterError::Storage)?;
        let existing: (u32, u32) = tx.query_row("SELECT requests_per_minute,management_writes_per_hour FROM creao_account_limits WHERE namespace=?1",
            params![namespace], |row| Ok((row.get(0)?,row.get(1)?))).map_err(|_| LimiterError::Storage)?;
        if existing
            != (
                limits.requests_per_minute,
                limits.management_writes_per_hour,
            )
        {
            return Err(LimiterError::InvalidConfiguration);
        }
        tx.commit().map_err(|_| LimiterError::Storage)?;
        // Busy acquisition is retried asynchronously; it must not block an
        // executor thread or overrun an HTTP operation's deadline.
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(|_| LimiterError::Storage)?;
        Ok(Self {
            inner: Arc::new(Inner {
                namespace: namespace.to_owned(),
                connection: Mutex::new(connection),
            }),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    /// Reserve one request before sending it. Management writes additionally
    /// consume the hourly budget. A cancelled/deadline-expired caller may have
    /// consumed a reservation; this is deliberately conservative and never refunded.
    pub async fn acquire(&self, kind: RequestKind, deadline: Instant) -> Result<(), LimiterError> {
        loop {
            if Instant::now() >= deadline {
                return Err(LimiterError::DeadlineExceeded);
            }
            let limiter = self.clone();
            let task = tokio::task::spawn_blocking(move || {
                let now = now_ms()?;
                limiter
                    .reserve_at(kind, now)
                    .map(|reservation| (reservation, now))
            });
            let (reservation, observed_now) = tokio::time::timeout_at(deadline, task)
                .await
                .map_err(|_| LimiterError::DeadlineExceeded)?
                .map_err(|_| LimiterError::Storage)??;
            if Instant::now() >= deadline {
                return Err(LimiterError::DeadlineExceeded);
            }
            let delay = match reservation {
                Reservation::Granted => return Ok(()),
                Reservation::Busy => BUSY_RETRY,
                Reservation::WaitUntil(next) => {
                    Duration::from_millis(next.saturating_sub(observed_now).max(1) as u64)
                }
            };
            // Cap addition through the remaining deadline to avoid Instant overflow.
            let wake =
                Instant::now() + delay.min(deadline.saturating_duration_since(Instant::now()));
            tokio::time::sleep_until(wake).await;
        }
    }

    /// Persist a provider Retry-After cooldown for the entire account. Extends an
    /// existing cooldown; a shorter later response can never shorten it. No token,
    /// response body, or organization-private content is stored in this database.
    pub fn defer_for(&self, duration: Duration) -> Result<(), LimiterError> {
        self.defer_at(now_ms()?, duration)
    }

    fn defer_at(&self, observed_now: i64, duration: Duration) -> Result<(), LimiterError> {
        let milliseconds = duration
            .as_millis()
            .checked_add(u128::from(
                !duration.subsec_nanos().is_multiple_of(1_000_000),
            ))
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(LimiterError::InvalidConfiguration)?;
        let mut connection = self
            .inner
            .connection
            .lock()
            .map_err(|_| LimiterError::Storage)?;
        // This short synchronous operation is also usable by synchronous callers.
        // Normal reservations never hold this transaction while waiting or doing HTTP.
        connection
            .busy_timeout(Duration::from_millis(250))
            .map_err(|_| LimiterError::Storage)?;
        let result = (|| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| LimiterError::Storage)?;
            let (last_seen, cooldown): (i64, i64) = tx.query_row("SELECT last_seen_ms,cooldown_until_ms FROM creao_account_limits WHERE namespace=?1",
                params![self.namespace()], |row| Ok((row.get(0)?,row.get(1)?))).map_err(|_| LimiterError::Storage)?;
            let now = observed_now.max(last_seen);
            let next = now
                .checked_add(milliseconds)
                .ok_or(LimiterError::InvalidConfiguration)?
                .max(cooldown);
            tx.execute("UPDATE creao_account_limits SET last_seen_ms=?2,cooldown_until_ms=?3 WHERE namespace=?1",
                params![self.namespace(),now,next]).map_err(|_| LimiterError::Storage)?;
            tx.commit().map_err(|_| LimiterError::Storage)
        })();
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(|_| LimiterError::Storage)?;
        result
    }

    fn reserve_at(
        &self,
        kind: RequestKind,
        observed_now: i64,
    ) -> Result<Reservation, LimiterError> {
        let mut connection = match self.inner.connection.try_lock() {
            Ok(connection) => connection,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(Reservation::Busy),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(LimiterError::Storage),
        };
        let tx = match connection.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(error) if busy(&error) => return Ok(Reservation::Busy),
            Err(_) => return Err(LimiterError::Storage),
        };
        let (request_limit, management_limit, last_seen, cooldown): (u32,u32,i64,i64) = tx.query_row(
            "SELECT requests_per_minute,management_writes_per_hour,last_seen_ms,cooldown_until_ms FROM creao_account_limits WHERE namespace=?1",
            params![self.namespace()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(|_| LimiterError::Storage)?;
        // Rollback never makes reservations or cooldowns expire early. Time may
        // effectively freeze until UTC catches up with the persisted high-water mark.
        let now = observed_now.max(last_seen);
        tx.execute(
            "DELETE FROM creao_account_reservations WHERE namespace=?1 AND reserved_at_ms<=?2",
            params![self.namespace(), now.saturating_sub(MANAGEMENT_WINDOW_MS)],
        )
        .map_err(|_| LimiterError::Storage)?;
        let (requests, oldest_request): (u32,Option<i64>) = tx.query_row(
            "SELECT COUNT(*),MIN(reserved_at_ms) FROM creao_account_reservations WHERE namespace=?1 AND reserved_at_ms>?2",
            params![self.namespace(),now.saturating_sub(REQUEST_WINDOW_MS)], |row| Ok((row.get(0)?,row.get(1)?))).map_err(|_| LimiterError::Storage)?;
        let mut next = cooldown;
        if requests >= request_limit {
            next = next.max(
                oldest_request
                    .ok_or(LimiterError::Storage)?
                    .saturating_add(REQUEST_WINDOW_MS),
            );
        }
        if kind == RequestKind::ManagementWrite {
            let (writes, oldest_write): (u32,Option<i64>) = tx.query_row(
                "SELECT COUNT(*),MIN(reserved_at_ms) FROM creao_account_reservations WHERE namespace=?1 AND management=1",
                params![self.namespace()], |row| Ok((row.get(0)?,row.get(1)?))).map_err(|_| LimiterError::Storage)?;
            if writes >= management_limit {
                next = next.max(
                    oldest_write
                        .ok_or(LimiterError::Storage)?
                        .saturating_add(MANAGEMENT_WINDOW_MS),
                );
            }
        }
        tx.execute(
            "UPDATE creao_account_limits SET last_seen_ms=?2 WHERE namespace=?1",
            params![self.namespace(), now],
        )
        .map_err(|_| LimiterError::Storage)?;
        let reservation = if next > now {
            Reservation::WaitUntil(next)
        } else {
            tx.execute(
                "INSERT INTO creao_account_reservations VALUES (?1,?2,?3)",
                params![self.namespace(), now, kind == RequestKind::ManagementWrite],
            )
            .map_err(|_| LimiterError::Storage)?;
            Reservation::Granted
        };
        match tx.commit() {
            Ok(()) => Ok(reservation),
            // In rollback-journal mode BEGIN IMMEDIATE can succeed while a
            // reader still holds a shared lock; COMMIT then needs that reader
            // to finish. rusqlite rolls back the consumed transaction on error.
            // Retry only once rollback has restored autocommit, so a failed
            // commit cannot leave a reservation behind or be counted twice.
            Err(error) if busy(&error) && connection.is_autocommit() => Ok(Reservation::Busy),
            Err(_) => Err(LimiterError::Storage),
        }
    }
}

fn now_ms() -> Result<i64, LimiterError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LimiterError::Clock)
        .and_then(|duration| i64::try_from(duration.as_millis()).map_err(|_| LimiterError::Clock))
}

fn busy(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(failure, _)
        if matches!(failure.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> QuotaLimits {
        QuotaLimits {
            requests_per_minute: 2,
            management_writes_per_hour: 1,
        }
    }

    #[test]
    fn rolling_request_window_counts_reads_runs_and_writes_at_exact_boundary() {
        let limiter = AccountLimiter::in_memory("shared-account", limits()).unwrap();
        assert_eq!(
            limiter.reserve_at(RequestKind::Read, 100).unwrap(),
            Reservation::Granted
        );
        assert_eq!(
            limiter.reserve_at(RequestKind::Run, 200).unwrap(),
            Reservation::Granted
        );
        assert_eq!(
            limiter
                .reserve_at(RequestKind::ManagementWrite, 300)
                .unwrap(),
            Reservation::WaitUntil(60_100)
        );
        assert_eq!(
            limiter.reserve_at(RequestKind::Read, 60_099).unwrap(),
            Reservation::WaitUntil(60_100)
        );
        assert_eq!(
            limiter
                .reserve_at(RequestKind::ManagementWrite, 60_100)
                .unwrap(),
            Reservation::Granted
        );
        assert_eq!(
            limiter.reserve_at(RequestKind::Read, 60_200).unwrap(),
            Reservation::Granted
        );
    }

    #[test]
    fn hourly_management_budget_does_not_block_reads_after_minute_rolls() {
        let limiter = AccountLimiter::in_memory("shared-account", limits()).unwrap();
        assert_eq!(
            limiter
                .reserve_at(RequestKind::ManagementWrite, 1_000)
                .unwrap(),
            Reservation::Granted
        );
        assert_eq!(
            limiter
                .reserve_at(RequestKind::ManagementWrite, 62_000)
                .unwrap(),
            Reservation::WaitUntil(3_601_000)
        );
        assert_eq!(
            limiter.reserve_at(RequestKind::Run, 62_000).unwrap(),
            Reservation::Granted
        );
        assert_eq!(
            limiter
                .reserve_at(RequestKind::ManagementWrite, 3_600_999)
                .unwrap(),
            Reservation::WaitUntil(3_601_000)
        );
        assert_eq!(
            limiter
                .reserve_at(RequestKind::ManagementWrite, 3_601_000)
                .unwrap(),
            Reservation::Granted
        );
    }

    #[test]
    fn clock_rollback_cannot_expire_quota_and_cooldown_never_shortens() {
        let limiter = AccountLimiter::in_memory("shared-account", limits()).unwrap();
        limiter.reserve_at(RequestKind::Read, 100_000).unwrap();
        limiter.reserve_at(RequestKind::Run, 100_000).unwrap();
        assert_eq!(
            limiter.reserve_at(RequestKind::Read, 1).unwrap(),
            Reservation::WaitUntil(160_000)
        );
        limiter.defer_at(1, Duration::from_secs(120)).unwrap();
        limiter.defer_at(1, Duration::from_secs(1)).unwrap();
        assert_eq!(
            limiter.reserve_at(RequestKind::Read, 160_000).unwrap(),
            Reservation::WaitUntil(220_000)
        );
        assert_eq!(
            limiter.reserve_at(RequestKind::Read, 220_000).unwrap(),
            Reservation::Granted
        );
    }

    #[test]
    fn quota_and_cooldown_survive_reopening_independent_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.db");
        {
            let first = AccountLimiter::open(&path, "account", limits()).unwrap();
            assert_eq!(
                first.reserve_at(RequestKind::ManagementWrite, 10).unwrap(),
                Reservation::Granted
            );
            first.defer_at(10, Duration::from_secs(2)).unwrap();
        }
        let second = AccountLimiter::open(&path, "account", limits()).unwrap();
        assert_eq!(
            second.reserve_at(RequestKind::Read, 11).unwrap(),
            Reservation::WaitUntil(2_010)
        );
        assert_eq!(
            second
                .reserve_at(RequestKind::ManagementWrite, 62_000)
                .unwrap(),
            Reservation::WaitUntil(3_600_010)
        );
    }
}
