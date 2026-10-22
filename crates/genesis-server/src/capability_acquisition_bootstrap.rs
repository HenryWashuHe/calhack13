//! Trusted startup migration for acquisition observation metadata.
//! Only previously unjournaled, independently validated current checkpoints get
//! a baseline. A baseline says nothing about unrecorded historical transitions.

use crate::capability_acquisition_history::{
    self as history, CheckpointKind, HistoryError, Result,
};
use crate::capability_acquisition_store::load_record;
use crate::capability_evidence::EvidencePolicy;
use crate::store::Store;
use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};

const BATCH_SIZE: usize = 32;
// Matches the maximum configured total-intent quota. Each baseline commits
// independently, so cancellation or invalid later records preserve prior work.
const MAX_BASELINES_PER_START: usize = 1_000_000;

/// Run before serving requests or starting capability workers. No provider calls,
/// quota changes, checkpoint rewrites, or retrospective proof creation occur.
/// A missing trusted suite fails enabled startup; an operator can restore it and
/// resume the idempotent migration. Disabled startup may serve explicit
/// history-unavailable responses instead of invoking this migration.
pub fn bootstrap(store: &Store, policy: &EvidencePolicy) -> Result<()> {
    store.init_acquisition_history()?;
    {
        let conn = store.lock();
        match history::require_ready(&conn) {
            Ok(()) => return Ok(()),
            Err(HistoryError::Unavailable) => {}
            Err(error) => return Err(error),
        }
    }
    let mut processed = 0;
    loop {
        // Fetch small identities only. Never materialize a batch of multi-MiB
        // source checkpoints in memory; each is read/validated below on its own.
        let missing: Vec<(String, String)> = {
            let conn = store.lock();
            let mut query = conn.prepare(
                "SELECT a.id,a.suite_id FROM capability_acquisitions a
                 LEFT JOIN acquisition_catalog c ON c.id=a.id
                 WHERE c.id IS NULL ORDER BY a.id LIMIT ?1",
            )?;
            let rows =
                query.query_map([BATCH_SIZE as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        if missing.is_empty() {
            let mut conn = store.lock();
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // This also refuses existing catalog/checkpoint version drift. It
            // cannot silently repair a journal damaged by an older writer.
            history::mark_ready(&tx)?;
            tx.commit()?;
            return Ok(());
        }
        for (id, suite_id) in missing {
            if processed >= MAX_BASELINES_PER_START {
                return Err(HistoryError::Unavailable);
            }
            let suite = policy.suite(&suite_id).ok_or(HistoryError::Unavailable)?;
            let mut conn = store.lock();
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Another trusted bootstrap may have completed this row after the
            // identity scan. No baseline or history row is duplicated.
            let indexed: Option<i64> = tx
                .query_row(
                    "SELECT version FROM acquisition_catalog WHERE id=?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?;
            if indexed.is_some() {
                continue;
            }
            let record = load_record(&tx, &id, suite).map_err(|_| HistoryError::InvalidRecord)?;
            let revision: u64 = tx.query_row(
                "SELECT revision FROM org_snapshots ORDER BY revision DESC LIMIT 1",
                [],
                |row| row.get(0),
            )?;
            let active = record
                .active(suite)
                .map_err(|_| HistoryError::InvalidRecord)?;
            history::append_checkpoint(
                &tx,
                &record,
                revision,
                CheckpointKind::BaselineObserved,
                None,
                active,
                Utc::now(),
            )?;
            tx.commit()?;
            processed += 1;
        }
    }
}
