//! Dispatch, job results and the simulated commitment ledger.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::store::{insert_commitment, put_job, EventMeta, NewOutbox, OutboxItem};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use genesis_core::engine::{self, DispatchError};
use genesis_core::events::EventKind;
use genesis_core::ids::*;
use genesis_core::jobs::{DispatchRequest, Job, JobResultSubmission, ResultOutcome};
use genesis_core::lifecycle::{JobStatus, StateMachine};
use genesis_core::model::Provider;
use serde_json::{json, Value};

/// Build a queued job after authorizing the dispatch against current state.
pub(crate) fn new_job(
    app: &AppState,
    state: &genesis_core::model::OrgState,
    req: &DispatchRequest,
) -> ApiResult<(Job, Vec<EventKind>, Vec<NewOutbox>)> {
    let grant = engine::authorize_dispatch(
        state,
        &req.role_id,
        &req.responsibility_id,
        req.required_scope.as_deref(),
    )?;
    if let Some(cap) = &req.capability {
        engine::authorize_capability(state, &req.role_id, cap)?;
    }
    let resp = &state.responsibilities[&req.responsibility_id];
    let role = &state.roles[&req.role_id];
    let provider = req
        .provider
        .or_else(|| role.binding.as_ref().map(|b| b.provider))
        .unwrap_or(Provider::Internal);
    let now = Utc::now();
    let job = Job {
        job_id: JobId::generate(),
        org_id: app.org_id.clone(),
        mission_id: resp.mission_id.clone(),
        role_id: req.role_id.clone(),
        responsibility_id: req.responsibility_id.clone(),
        assignment_id: grant.assignment_id.clone(),
        epoch: grant.epoch,
        task: req.task.clone(),
        input: req.input.clone(),
        attempt_id: AttemptId::generate(),
        attempt_no: 1,
        correlation_id: CorrelationId::generate(),
        idempotency_key: format!("{}:{}", app.org_id, req.client_request_id),
        deadline: req.deadline,
        capability: req.capability.clone(),
        provider: Some(provider),
        status: JobStatus::Queued,
        provider_refs: Default::default(),
        raw_status: None,
        result: None,
        rejection_reason: None,
        created_at: now,
        updated_at: now,
    };
    let effects = vec![EventKind::JobDispatched {
        job_id: job.job_id.clone(),
        role_id: job.role_id.clone(),
        responsibility_id: job.responsibility_id.clone(),
        epoch: job.epoch,
        task: job.task.clone(),
    }];
    let outbox = vec![NewOutbox {
        destination: format!("{provider:?}").to_lowercase(),
        kind: "job_dispatch".into(),
        payload: json!({
            "job_id": job.job_id,
            "role_id": job.role_id,
            "responsibility_id": job.responsibility_id,
            "assignment_id": job.assignment_id,
            "epoch": job.epoch,
            "attempt_id": job.attempt_id,
            "correlation_id": job.correlation_id,
            "idempotency_key": job.idempotency_key,
            "task": job.task,
            "input": job.input,
            "capability": job.capability,
            "deadline": job.deadline,
        }),
    }];
    Ok((job, effects, outbox))
}

pub async fn dispatch(
    State(app): State<AppState>,
    p: Principal,
    Json(req): Json<DispatchRequest>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    p.require(&[Principal::Operator])?;
    if req.client_request_id.trim().is_empty() {
        return Err(ApiError::bad_request("client_request_id is required"));
    }
    if !super::capabilities::development(&app) && req.capability.is_some() {
        return Err(ApiError::bad_request(
            "use /capability-runs for real capability execution",
        ));
    }
    let _guard = app.kernel.lock().await;
    let state = app.current_state()?;
    let (job, effects, outbox) = new_job(&app, &state, &req)?;
    let (stored, fresh) = app.store.insert_job(&req.client_request_id, &job)?;
    if !fresh {
        return Ok((StatusCode::OK, Json(stored)));
    }
    let meta = EventMeta::mission(job.mission_id.clone()).caused_by(req.client_request_id.clone());
    let events = app
        .store
        .commit_state(state.revision, &state, effects, &meta, outbox, |_| Ok(()))?;
    app.broadcast(&events);
    Ok((StatusCode::CREATED, Json(job)))
}

pub async fn list(State(app): State<AppState>, p: Principal) -> ApiResult<Json<Vec<Job>>> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    Ok(Json(app.store.list_jobs()?))
}

pub async fn get_one(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Json<Job>> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    Ok(Json(app.store.get_job(&JobId::new(id))?))
}

/// Move a job forward through every intermediate state up to `target`.
fn advance(
    job: &mut Job,
    path: &[JobStatus],
    effects: &mut Vec<EventKind>,
) -> Result<(), ApiError> {
    let now = Utc::now();
    for to in path {
        if job.status == *to {
            continue;
        }
        if job.status.can_transition_to(*to) {
            let from = job
                .set_status(*to, now)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            effects.push(EventKind::JobStatusChanged {
                job_id: job.job_id.clone(),
                from,
                to: *to,
            });
        }
    }
    Ok(())
}

/// Provider adapters report results here. Stale epochs are archived and
/// rejected; duplicate deliveries with the same operation ID are idempotent;
/// provider success without domain verification never marks success.
pub async fn submit_result(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(sub): Json<JobResultSubmission>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require(&[Principal::Adapter, Principal::Operator])?;
    let _guard = app.kernel.lock().await;
    let state = app.current_state()?;
    let mut job = app.store.get_job(&JobId::new(id))?;
    super::capabilities::reject_managed_result(&app, &job)?;
    if sub.attempt_id != job.attempt_id {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "wrong_attempt",
            format!(
                "result is for attempt {}, job is on {}",
                sub.attempt_id, job.attempt_id
            ),
        ));
    }
    // Duplicate delivery of an already-applied result.
    if job.status.is_terminal() {
        if let Some(op) = &sub.operation_id {
            if app.store.commitment_exists(op)? {
                return Ok((
                    StatusCode::OK,
                    Json(json!({ "duplicate": true, "job": job })),
                ));
            }
        }
        return Ok((
            StatusCode::OK,
            Json(json!({ "duplicate": true, "job": job })),
        ));
    }

    let mut effects = Vec::new();
    let meta = EventMeta::mission(job.mission_id.clone()).caused_by(job.job_id.to_string());
    job.raw_status = Some(sub.provider_status.clone());
    job.provider_refs.extend(sub.provider_refs.clone());
    job.result = Some(sub.output.clone());

    let epoch_check = if sub.epoch != job.epoch {
        Err(DispatchError::StaleEpoch {
            current: job.epoch,
            got: sub.epoch,
        })
    } else {
        engine::check_result_epoch(&state, &job.responsibility_id, sub.epoch)
    };
    if let Err(e) = epoch_check {
        let reason = e.to_string();
        advance(
            &mut job,
            &[
                JobStatus::Submitting,
                JobStatus::Running,
                JobStatus::ResultRejected,
            ],
            &mut effects,
        )?;
        job.rejection_reason = Some(reason.clone());
        effects.push(EventKind::JobResultRejected {
            job_id: job.job_id.clone(),
            reason,
        });
        let events = app
            .store
            .append_events(effects, &meta, |tx| put_job(tx, &job))?;
        app.broadcast(&events);
        return Err(ApiError::from(e).with_details(json!({ "job": job })));
    }

    let mut new_state = state.clone();
    let mut state_effects = Vec::new();
    let mut ledger_written = false;
    let status = match sub.outcome {
        ResultOutcome::Unknown => {
            advance(
                &mut job,
                &[JobStatus::Submitting, JobStatus::Unknown],
                &mut effects,
            )?;
            StatusCode::ACCEPTED
        }
        ResultOutcome::Failed => {
            advance(
                &mut job,
                &[JobStatus::Submitting, JobStatus::Running, JobStatus::Failed],
                &mut effects,
            )?;
            StatusCode::OK
        }
        ResultOutcome::Succeeded if !sub.domain_verified => {
            advance(
                &mut job,
                &[
                    JobStatus::Submitting,
                    JobStatus::Running,
                    JobStatus::Verifying,
                ],
                &mut effects,
            )?;
            StatusCode::ACCEPTED
        }
        ResultOutcome::Succeeded => {
            advance(
                &mut job,
                &[
                    JobStatus::Submitting,
                    JobStatus::Running,
                    JobStatus::Verifying,
                    JobStatus::Succeeded,
                ],
                &mut effects,
            )?;
            if let Some(op) = &sub.operation_id {
                if !app.store.commitment_exists(op)? {
                    ledger_written = true;
                    effects.push(EventKind::CommitmentRecorded {
                        operation_id: op.clone(),
                        job_id: job.job_id.clone(),
                    });
                }
            }
            if sub.completes_responsibility {
                let out = engine::complete_responsibility(&new_state, &job.responsibility_id)?;
                new_state = out.state;
                state_effects = out.effects;
            }
            StatusCode::OK
        }
    };
    effects.extend(state_effects);
    let op = sub.operation_id.clone();
    let payload = json!({ "output": sub.output, "provider_refs": sub.provider_refs });
    let job_for_tx = job.clone();
    let events =
        app.store
            .commit_state(state.revision, &new_state, effects, &meta, vec![], |tx| {
                put_job(tx, &job_for_tx)?;
                if ledger_written {
                    if let Some(op) = &op {
                        insert_commitment(tx, op, &job_for_tx.job_id, &payload)?;
                    }
                }
                Ok(())
            })?;
    app.broadcast(&events);
    Ok((
        status,
        Json(json!({ "duplicate": false, "job": job, "revision": new_state.revision })),
    ))
}

pub async fn ledger(State(app): State<AppState>, p: Principal) -> ApiResult<Json<Vec<Value>>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    Ok(Json(app.store.commitments()?))
}

pub async fn outbox(State(app): State<AppState>, p: Principal) -> ApiResult<Json<Vec<OutboxItem>>> {
    p.require(&[Principal::Operator])?;
    Ok(Json(app.store.list_outbox(200)?))
}
