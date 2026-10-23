//! Audit a stored organization history against the core invariants.
//!
//! Every snapshot is checked with [`genesis_core::audit::audit`], and every
//! consecutive pair with [`genesis_core::audit::audit_transition`], plus the
//! storage-level rules below. Read-only: it never writes to the store.

use crate::store::{Store, StoreResult};
use genesis_core::audit::{audit, audit_transition, INVARIANTS, TRANSITION_INVARIANTS};
use genesis_core::ids::OrgId;
use genesis_core::OrgState;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

/// Storage-level invariants checked in addition to the core ones.
pub const HISTORY_INVARIANTS: &[&str] = &[
    "snapshot_revision_matches",
    "snapshot_history_contiguous",
    "snapshot_org_matches",
    "event_revision_known",
    "event_org_matches",
];

/// Most violations reported in full; the rest are only counted.
const MAX_REPORTED: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct RevisionViolation {
    pub revision: u64,
    pub invariant: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryAudit {
    pub org_id: String,
    pub snapshots_checked: usize,
    pub first_revision: Option<u64>,
    pub last_revision: Option<u64>,
    pub events_checked: u64,
    /// Names of every invariant applied (core state, core transition, storage).
    pub invariants: Vec<&'static str>,
    pub violation_count: usize,
    /// The first [`MAX_REPORTED`] violations.
    pub violations: Vec<RevisionViolation>,
}

impl HistoryAudit {
    pub fn ok(&self) -> bool {
        self.violation_count == 0
    }
}

fn push(report: &mut HistoryAudit, revision: u64, invariant: &'static str, detail: String) {
    report.violation_count += 1;
    if report.violations.len() < MAX_REPORTED {
        report.violations.push(RevisionViolation {
            revision,
            invariant,
            detail,
        });
    }
}

/// Audit every snapshot and event in `store`.
///
/// A file-backed store is audited through its own read-only connection, so
/// the whole-history replay (seconds on a long history) never holds the
/// shared connection that kernel commits, workers and routes wait on. WAL
/// readers neither block nor are blocked by the writer. Only an in-memory
/// store, which has no file to reopen, is audited under the shared lock.
pub fn audit_history(store: &Store) -> StoreResult<HistoryAudit> {
    match store.path() {
        Some(path) => audit_database(path, store.org_id()),
        None => audit_connection(&store.lock(), store.org_id()),
    }
}

/// Audit an organization database file, opened read-only.
pub fn audit_database(path: &std::path::Path, org_id: &OrgId) -> StoreResult<HistoryAudit> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    audit_connection(&conn, org_id)
}

fn audit_connection(conn: &Connection, org_id: &OrgId) -> StoreResult<HistoryAudit> {
    // One read transaction: snapshots and events come from the same database
    // snapshot even while the server keeps committing (WAL readers are not
    // blocked and do not block). Dropped without commit; nothing is written.
    let tx = conn.unchecked_transaction()?;
    audit_snapshot(&tx, org_id)
}

fn audit_snapshot(conn: &Connection, org_id: &OrgId) -> StoreResult<HistoryAudit> {
    let org_id = org_id.to_string();
    let mut report = HistoryAudit {
        org_id: org_id.clone(),
        snapshots_checked: 0,
        first_revision: None,
        last_revision: None,
        events_checked: 0,
        invariants: INVARIANTS
            .iter()
            .chain(TRANSITION_INVARIANTS)
            .chain(HISTORY_INVARIANTS)
            .copied()
            .collect(),
        violation_count: 0,
        violations: vec![],
    };
    let mut revisions = std::collections::BTreeSet::new();
    {
        let mut stmt =
            conn.prepare("SELECT revision, state_json FROM org_snapshots ORDER BY revision")?;
        let mut rows = stmt.query([])?;
        let mut prev: Option<OrgState> = None;
        while let Some(row) = rows.next()? {
            let revision = row.get::<_, i64>(0)? as u64;
            let state: OrgState = serde_json::from_str(&row.get::<_, String>(1)?)?;
            revisions.insert(revision);
            report.snapshots_checked += 1;
            report.first_revision.get_or_insert(revision);
            report.last_revision = Some(revision);
            if state.revision != revision {
                push(
                    &mut report,
                    revision,
                    "snapshot_revision_matches",
                    format!("row {revision} holds state r{}", state.revision),
                );
            }
            if state.org_id.as_str() != org_id {
                push(
                    &mut report,
                    revision,
                    "snapshot_org_matches",
                    format!("snapshot belongs to {}", state.org_id),
                );
            }
            for v in audit(&state) {
                push(&mut report, revision, v.invariant, v.detail);
            }
            if let Some(p) = &prev {
                if revision != p.revision + 1 {
                    push(
                        &mut report,
                        revision,
                        "snapshot_history_contiguous",
                        format!("r{} follows r{}", revision, p.revision),
                    );
                }
                for v in audit_transition(p, &state) {
                    push(&mut report, revision, v.invariant, v.detail);
                }
            }
            prev = Some(state);
        }
    }
    let mut stmt = conn.prepare("SELECT seq, revision, org_id FROM events ORDER BY seq")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let seq = row.get::<_, i64>(0)?;
        let revision = row.get::<_, i64>(1)? as u64;
        let event_org: String = row.get(2)?;
        report.events_checked += 1;
        if !revisions.contains(&revision) {
            push(
                &mut report,
                revision,
                "event_revision_known",
                format!("event {seq} names r{revision}, which has no snapshot"),
            );
        }
        if event_org != org_id {
            push(
                &mut report,
                revision,
                "event_org_matches",
                format!("event {seq} belongs to {event_org}"),
            );
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn file_backed_audit_does_not_wait_for_the_shared_connection() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("org.sqlite"), OrgId::new("org_a")).unwrap();
        // Anything holding the shared connection (a commit, a worker, a route).
        let guard = store.lock();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = store.clone();
        std::thread::spawn(move || {
            let _ = tx.send(audit_history(&reader).map(|r| r.org_id));
        });
        let got = rx.recv_timeout(Duration::from_secs(10));
        drop(guard);
        assert!(
            matches!(&got, Ok(Ok(org)) if org == "org_a"),
            "audit waited on the shared connection: {:?}",
            got.map(|r| r.is_ok())
        );
    }
}
