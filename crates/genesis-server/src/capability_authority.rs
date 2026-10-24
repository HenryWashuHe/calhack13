//! Current authority for capability effects. Committed semantic acceptance is
//! organization-owned evidence; these checks apply to live work and the initial
//! acceptance decision, not to the lifetime of an already accepted capability.

use crate::error::ApiError;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use genesis_core::engine::{self, DispatchError, DispatchGrant};
use genesis_core::ids::{ResponsibilityId, RoleId};
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{AssignmentState, MissionStatus};
use genesis_core::model::OrgState;

#[derive(Debug, thiserror::Error)]
pub enum CapabilityAuthorityError {
    #[error(transparent)]
    Dispatch(#[from] DispatchError),
    #[error("capability responsibility has no current mission")]
    MissionMissing,
    #[error("capability mission is not active")]
    MissionInactive,
    #[error("capability mission deadline has expired")]
    MissionExpired,
    #[error("capability assignment lease has expired")]
    LeaseExpired,
    #[error("capability job belongs to a different organization or mission")]
    JobBinding,
    #[error("capability assignment changed")]
    GrantChanged,
    #[error("capability job deadline has expired")]
    JobExpired,
}

impl From<CapabilityAuthorityError> for ApiError {
    fn from(error: CapabilityAuthorityError) -> Self {
        let code = match error {
            CapabilityAuthorityError::Dispatch(error) => return error.into(),
            CapabilityAuthorityError::MissionMissing => "mission_missing",
            CapabilityAuthorityError::MissionInactive => "mission_inactive",
            CapabilityAuthorityError::MissionExpired => "mission_expired",
            CapabilityAuthorityError::LeaseExpired => "assignment_expired",
            CapabilityAuthorityError::JobBinding => "job_binding",
            CapabilityAuthorityError::GrantChanged => "stale_epoch",
            CapabilityAuthorityError::JobExpired => "deadline_expired",
        };
        ApiError::new(StatusCode::CONFLICT, code, error.to_string())
    }
}

/// Supplement the kernel's role/scope/epoch grant with time-sensitive authority.
/// `now` is supplied once by the caller so exact deadline boundaries agree.
pub fn authorize_current(
    state: &OrgState,
    role: &RoleId,
    responsibility: &ResponsibilityId,
    scope: Option<&str>,
    now: DateTime<Utc>,
) -> Result<DispatchGrant, CapabilityAuthorityError> {
    let grant = engine::authorize_dispatch(state, role, responsibility, scope)?;
    let responsibility = state
        .responsibilities
        .get(responsibility)
        .ok_or(CapabilityAuthorityError::JobBinding)?;
    let mission = state
        .missions
        .get(&responsibility.mission_id)
        .filter(|mission| mission.mission_id == responsibility.mission_id)
        .ok_or(CapabilityAuthorityError::MissionMissing)?;
    if mission.status != MissionStatus::Active {
        return Err(CapabilityAuthorityError::MissionInactive);
    }
    if mission.deadline.is_some_and(|deadline| deadline <= now) {
        return Err(CapabilityAuthorityError::MissionExpired);
    }
    let assignment = state
        .assignments
        .get(&grant.assignment_id)
        .filter(|assignment| {
            assignment.assignment_id == grant.assignment_id
                && assignment.responsibility_id == grant.responsibility_id
                && assignment.epoch == grant.epoch
                && assignment.state == AssignmentState::Active
        })
        .ok_or(CapabilityAuthorityError::GrantChanged)?;
    if assignment
        .lease_expires_at
        .is_some_and(|expiry| expiry <= now)
    {
        return Err(CapabilityAuthorityError::LeaseExpired);
    }
    Ok(grant)
}

/// Reauthorize a saved job without allowing reappointment to revive its grant.
pub fn authorize_job(
    state: &OrgState,
    job: &Job,
    scope: Option<&str>,
    now: DateTime<Utc>,
) -> Result<DispatchGrant, CapabilityAuthorityError> {
    if job.org_id != state.org_id {
        return Err(CapabilityAuthorityError::JobBinding);
    }
    if job.deadline.is_some_and(|deadline| deadline <= now) {
        return Err(CapabilityAuthorityError::JobExpired);
    }
    let grant = authorize_current(state, &job.role_id, &job.responsibility_id, scope, now)?;
    if state
        .responsibilities
        .get(&job.responsibility_id)
        .is_none_or(|responsibility| responsibility.mission_id != job.mission_id)
    {
        return Err(CapabilityAuthorityError::JobBinding);
    }
    if grant.assignment_id != job.assignment_id || grant.epoch != job.epoch {
        return Err(CapabilityAuthorityError::GrantChanged);
    }
    Ok(grant)
}
