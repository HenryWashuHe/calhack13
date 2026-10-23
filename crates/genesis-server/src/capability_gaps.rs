//! Kernel-side access to committed capability gaps (`RequestCapability`).
//!
//! A gap is the organization's evidence-bound statement that a role needs an
//! executable capability it does not have. This module is the boundary an
//! acquisition driver uses: it lists open gaps and applies the only allowed
//! transitions (`engine::update_capability_gap`) under the kernel lock, with
//! events. It never chooses a suite, oracle, quota or provider; those come
//! from trusted operator policy in the driver.

use crate::app::AppState;
use crate::error::ApiResult;
use crate::store::EventMeta;
use genesis_core::engine;
use genesis_core::ids::CapabilityGapId;
use genesis_core::model::{CapabilityGap, CapabilityGapStatus};

/// Gaps that no driver has claimed yet, oldest revision first.
pub fn open_gaps(app: &AppState) -> ApiResult<Vec<CapabilityGap>> {
    let state = app.current_state()?;
    let mut gaps: Vec<_> = state
        .capability_gaps
        .into_values()
        .filter(|g| g.status == CapabilityGapStatus::Open)
        .collect();
    gaps.sort_by_key(|g| (g.opened_in_revision, g.gap_id.clone()));
    Ok(gaps)
}

/// Apply one gap transition atomically with its event. A transition that
/// already happened (same target state, e.g. repeated after a lost response
/// or by a concurrent caller) returns the gap unchanged with no second event.
pub async fn transition(
    app: &AppState,
    gap_id: &CapabilityGapId,
    to: CapabilityGapStatus,
) -> ApiResult<CapabilityGap> {
    let current = app.current_state()?;
    if let Some(g) = current.capability_gaps.get(gap_id) {
        if g.status == to {
            return Ok(g.clone());
        }
    }
    let meta = match current.capability_gaps.get(gap_id) {
        Some(g) => EventMeta::mission(g.mission_id.clone()),
        None => EventMeta::default(),
    }
    .caused_by(gap_id.to_string());
    let (id, target) = (gap_id.clone(), to.clone());
    let applied = app
        .mutate(meta, move |s| {
            Ok((engine::update_capability_gap(s, &id, target)?, ()))
        })
        .await;
    let now = app.current_state()?;
    match (applied, now.capability_gaps.get(gap_id)) {
        (Ok(_), Some(g)) => Ok(g.clone()),
        // Lost a race to an identical transition: same outcome, one event.
        (Err(_), Some(g)) if g.status == to => Ok(g.clone()),
        (Err(e), _) => Err(e),
        (Ok(_), None) => Err(crate::error::ApiError::internal(
            "gap vanished after commit",
        )),
    }
}
