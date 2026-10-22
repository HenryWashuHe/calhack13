use chrono::{Duration, TimeZone, Utc};
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::*;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{AssignmentState, MissionStatus, RoleState};
use genesis_core::model::{Executor, OrgState};
use genesis_server::capability_authority::{
    authorize_current, authorize_job, CapabilityAuthorityError as Error,
};
use serde_json::json;

fn fixture() -> (OrgState, Job) {
    let now = Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
    let initial = OrgState::new(
        OrgId::new("authority-org"),
        "Authority fixture",
        genesis_server::demo_constitution(),
    );
    let bootstrap: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Use inherited capability",
        "roles":[{"key":"owner","name":"Owner","charter":"Own work",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":0}}],
        "responsibilities":[{"purpose":"Read venue","owner":"owner"}],
    }))
    .unwrap();
    let (outcome, mission) = bootstrap_mission(&initial, &bootstrap, now).unwrap();
    let state = outcome.state;
    let responsibility = state.responsibilities.values().next().unwrap();
    let assignment = state
        .active_assignment(&responsibility.responsibility_id)
        .unwrap();
    let job = serde_json::from_value(json!({
        "job_id":"job-authority", "org_id":state.org_id,"mission_id":mission,
        "role_id":assignment.role_id,"responsibility_id":responsibility.responsibility_id,
        "assignment_id":assignment.assignment_id,"epoch":assignment.epoch,
        "task":"Inspect venue", "input":{},"attempt_id":"attempt-1","attempt_no":1,
        "correlation_id":"correlation-1","idempotency_key":"request-1","deadline":null,
        "capability":null,"provider":"apify","status":"QUEUED","provider_refs":{},
        "raw_status":null,"result":null,"rejection_reason":null,"created_at":now,"updated_at":now,
    }))
    .unwrap();
    (state, job)
}

#[test]
fn current_authority_requires_live_lease_and_mission_at_exact_boundaries() {
    let (state, job) = fixture();
    let now = job.created_at;
    for offset in [-1, 0, 1] {
        let mut current = state.clone();
        current
            .assignments
            .get_mut(&job.assignment_id)
            .unwrap()
            .lease_expires_at = Some(now + Duration::seconds(offset));
        let result = authorize_job(&current, &job, Some("capability.venue_lookup"), now);
        if offset <= 0 {
            assert!(matches!(result, Err(Error::LeaseExpired)));
        } else {
            result.unwrap();
        }
        let mut current = state.clone();
        current.missions.get_mut(&job.mission_id).unwrap().deadline =
            Some(now + Duration::seconds(offset));
        let result = authorize_job(&current, &job, None, now);
        if offset <= 0 {
            assert!(matches!(result, Err(Error::MissionExpired)));
        } else {
            result.unwrap();
        }
        let mut timed_job = job.clone();
        timed_job.deadline = Some(now + Duration::seconds(offset));
        let result = authorize_job(&state, &timed_job, None, now);
        if offset <= 0 {
            assert!(matches!(result, Err(Error::JobExpired)));
        } else {
            result.unwrap();
        }
    }
    authorize_job(&state, &job, None, now).unwrap();
}

#[test]
fn missing_closed_and_changed_missions_cannot_authorize_saved_work() {
    let (state, job) = fixture();
    let now = job.created_at;
    for status in [MissionStatus::Succeeded, MissionStatus::Failed] {
        let mut current = state.clone();
        current.missions.get_mut(&job.mission_id).unwrap().status = status;
        assert!(matches!(
            authorize_job(&current, &job, None, now),
            Err(Error::MissionInactive)
        ));
    }
    let mut current = state.clone();
    current.missions.clear();
    assert!(matches!(
        authorize_job(&current, &job, None, now),
        Err(Error::MissionMissing)
    ));
    let mut current = state.clone();
    let mut other = current.missions[&job.mission_id].clone();
    other.mission_id = MissionId::new("other-mission");
    current
        .responsibilities
        .get_mut(&job.responsibility_id)
        .unwrap()
        .mission_id = other.mission_id.clone();
    current.missions.insert(other.mission_id.clone(), other);
    assert!(matches!(
        authorize_job(&current, &job, None, now),
        Err(Error::JobBinding)
    ));
    let mut foreign_job = job.clone();
    foreign_job.org_id = OrgId::new("foreign-org");
    assert!(matches!(
        authorize_job(&state, &foreign_job, None, now),
        Err(Error::JobBinding)
    ));
}

#[test]
fn reappointment_never_revives_saved_grant_and_delegation_keeps_owners_lease() {
    let (state, job) = fixture();
    let now = job.created_at;
    let mut current = state.clone();
    let mut successor = current.assignments[&job.assignment_id].clone();
    current
        .assignments
        .get_mut(&job.assignment_id)
        .unwrap()
        .state = AssignmentState::Fenced;
    successor.assignment_id = AssignmentId::new("reappointed-owner");
    successor.epoch += 2;
    current
        .responsibilities
        .get_mut(&job.responsibility_id)
        .unwrap()
        .epoch = successor.epoch;
    current
        .assignments
        .insert(successor.assignment_id.clone(), successor);
    authorize_current(&current, &job.role_id, &job.responsibility_id, None, now).unwrap();
    assert!(matches!(
        authorize_job(&current, &job, None, now),
        Err(Error::GrantChanged)
    ));

    let mut delegated = state.clone();
    let mut executor = delegated.roles[&job.role_id].clone();
    executor.role_id = RoleId::new("delegate");
    delegated
        .responsibilities
        .get_mut(&job.responsibility_id)
        .unwrap()
        .executor = Some(Executor::Role(executor.role_id.clone()));
    delegated
        .roles
        .insert(executor.role_id.clone(), executor.clone());
    authorize_current(
        &delegated,
        &executor.role_id,
        &job.responsibility_id,
        None,
        now,
    )
    .unwrap();
    delegated
        .assignments
        .get_mut(&job.assignment_id)
        .unwrap()
        .lease_expires_at = Some(now);
    assert!(matches!(
        authorize_current(
            &delegated,
            &executor.role_id,
            &job.responsibility_id,
            None,
            now
        ),
        Err(Error::LeaseExpired)
    ));
}

#[test]
fn current_checks_preserve_role_and_scope_authority() {
    let (mut state, job) = fixture();
    assert!(matches!(
        authorize_job(&state, &job, Some("not.granted"), job.created_at),
        Err(Error::Dispatch(_))
    ));
    state.roles.get_mut(&job.role_id).unwrap().state = RoleState::Retired;
    assert!(matches!(
        authorize_job(&state, &job, None, job.created_at),
        Err(Error::Dispatch(_))
    ));
}
