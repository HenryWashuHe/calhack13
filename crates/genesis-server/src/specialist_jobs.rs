//! Jobs for roles implemented by an asynchronous specialist runtime (CREAO).
//!
//! Follows CREAO_RUNTIME.md steps 5–6 on the kernel side:
//!
//! - Delivery prepares one durable job operation (`prepare_job`, deterministic
//!   per job attempt) after checking live authority. Outbox redelivery finds
//!   the same operation; it never causes a second POST.
//! - The specialist poller advances each running job at most once per pass.
//!   Live authority (active role, open responsibility, current epoch,
//!   deadline, matching binding) is checked before **each** advance and again
//!   before a collected result is applied; a late result is archived as
//!   `RESULT_REJECTED`.
//! - Native completion is not task success: collected output is stored with
//!   `domain_verified: false` and the job moves to `VERIFYING`. An unknown
//!   submission outcome moves the job to `UNKNOWN` for reconciliation.

use crate::app::AppState;
use crate::store::{put_job, EventMeta, OutboxItem};
use chrono::Utc;
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::JobId;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{JobStatus, StateMachine};
use genesis_core::model::AgentBinding;
use genesis_providers::{SpecialistProgress, SpecialistRuntime};
use serde_json::json;
use std::sync::Arc;

const OPERATION: &str = "operation_id";

fn advance(job: &mut Job, path: &[JobStatus], effects: &mut Vec<EventKind>) {
    let now = Utc::now();
    for to in path {
        if job.status != *to && job.status.can_transition_to(*to) {
            if let Ok(from) = job.set_status(*to, now) {
                effects.push(EventKind::JobStatusChanged {
                    job_id: job.job_id.clone(),
                    from,
                    to: *to,
                });
            }
        }
    }
}

fn save(app: &AppState, job: &Job, effects: Vec<EventKind>) -> Result<(), String> {
    let meta = EventMeta::mission(job.mission_id.clone()).caused_by(job.job_id.to_string());
    let events = app
        .store
        .append_events(effects, &meta, |tx| put_job(tx, job))
        .map_err(|e| e.to_string())?;
    app.broadcast(&events);
    Ok(())
}

/// End a job that may not proceed, with a reason.
fn refuse(app: &AppState, job: &mut Job, reason: String, rejected: bool) -> Result<(), String> {
    let mut effects = Vec::new();
    if rejected {
        advance(
            job,
            &[JobStatus::Running, JobStatus::ResultRejected],
            &mut effects,
        );
        effects.push(EventKind::JobResultRejected {
            job_id: job.job_id.clone(),
            reason: reason.clone(),
        });
    } else {
        advance(job, &[JobStatus::Failed], &mut effects);
    }
    job.rejection_reason = Some(reason);
    save(app, job, effects)
}

/// Live authority for a job; returns the role's binding.
fn authorize(app: &AppState, job: &Job) -> Result<AgentBinding, String> {
    let state = app.current_state().map_err(|e| e.message)?;
    engine::check_job_result(&state, &job.role_id, &job.responsibility_id, job.epoch)
        .map_err(|e| e.to_string())?;
    if job.deadline.is_some_and(|d| d <= Utc::now()) {
        return Err("job deadline has passed".into());
    }
    let binding = state
        .roles
        .get(&job.role_id)
        .and_then(|r| r.binding.clone())
        .ok_or_else(|| "role has no provider binding".to_owned())?;
    if Some(binding.provider) != job.provider {
        return Err("role binding does not match the job's provider".into());
    }
    Ok(binding)
}

fn runtime_for(app: &AppState, job: &Job) -> Option<Arc<dyn SpecialistRuntime>> {
    job.provider.and_then(|p| app.providers.specialist(p).ok())
}

/// Whether an outbox item is a job for a role served by a specialist runtime.
pub(crate) fn handles(app: &AppState, item: &OutboxItem) -> bool {
    item.kind == "job_dispatch"
        && item.payload["job_id"]
            .as_str()
            .and_then(|id| app.store.get_job(&JobId::new(id)).ok())
            .is_some_and(|job| runtime_for(app, &job).is_some())
}

/// Prepare the durable job operation. Redelivery is a no-op.
pub(crate) async fn deliver(app: &AppState, item: &OutboxItem) -> Result<(), String> {
    let job_id = item.payload["job_id"]
        .as_str()
        .ok_or_else(|| "job_dispatch without job_id".to_owned())?;
    let _guard = app.kernel.lock().await;
    let mut job = app
        .store
        .get_job(&JobId::new(job_id))
        .map_err(|e| e.to_string())?;
    if job.status != JobStatus::Queued || job.provider_refs.contains_key(OPERATION) {
        return Ok(());
    }
    let runtime = runtime_for(app, &job).ok_or_else(|| "no specialist runtime".to_owned())?;
    let binding = match authorize(app, &job) {
        Ok(b) => b,
        Err(reason) => return refuse(app, &mut job, format!("not dispatched: {reason}"), false),
    };
    match runtime.prepare_job(&job, &binding) {
        Ok(progress) => {
            let mut effects = Vec::new();
            job.provider_refs
                .insert(OPERATION.into(), progress.operation_id().to_owned());
            job.provider_refs
                .insert("agent_id".into(), binding.provider_agent_id.clone());
            advance(&mut job, &[JobStatus::Submitting], &mut effects);
            save(app, &job, effects)
        }
        Err(e) => refuse(
            app,
            &mut job,
            format!("could not prepare provider operation: {e}"),
            false,
        ),
    }
}

/// Advance every running specialist job once. Called by the specialist poller.
pub(crate) async fn advance_jobs(app: &AppState) {
    let jobs = match app.store.list_jobs() {
        Ok(j) => j,
        Err(error) => {
            tracing::warn!(%error, "specialist job scan failed");
            return;
        }
    };
    for job in jobs {
        if !matches!(job.status, JobStatus::Submitting | JobStatus::Running) {
            continue;
        }
        let (Some(op), Some(runtime)) = (
            job.provider_refs.get(OPERATION).cloned(),
            runtime_for(app, &job),
        ) else {
            continue;
        };
        if let Err(error) = advance_job(app, &job.job_id, &op, runtime).await {
            tracing::warn!(job = %job.job_id, %error, "specialist job pass failed");
        }
    }
}

async fn advance_job(
    app: &AppState,
    job_id: &JobId,
    op: &str,
    runtime: Arc<dyn SpecialistRuntime>,
) -> Result<(), String> {
    // Authority before each advance.
    {
        let _guard = app.kernel.lock().await;
        let mut job = app.store.get_job(job_id).map_err(|e| e.to_string())?;
        if !matches!(job.status, JobStatus::Submitting | JobStatus::Running) {
            return Ok(());
        }
        if let Err(reason) = authorize(app, &job) {
            return refuse(
                app,
                &mut job,
                format!("authority lost before advance: {reason}"),
                true,
            );
        }
    }
    let timeout = app.provision_timeout;
    let once = |retry| {
        let runtime = runtime.clone();
        async move {
            tokio::time::timeout(timeout, runtime.advance(op, retry))
                .await
                .map_err(|_| "provider call timed out".to_owned())?
                .map_err(|e| e.to_string())
        }
    };
    let progress = match once(false).await {
        Ok(p) => p,
        Err(_) => once(true).await?, // a failed read may be retried; a POST never is
    };

    let _guard = app.kernel.lock().await;
    let mut job = app.store.get_job(job_id).map_err(|e| e.to_string())?;
    if !matches!(job.status, JobStatus::Submitting | JobStatus::Running) {
        return Ok(());
    }
    let mut effects = Vec::new();
    match progress {
        SpecialistProgress::Pending {
            run_id,
            native_status,
            ..
        } => {
            let before = (
                job.status,
                job.raw_status.clone(),
                job.provider_refs.get("run_id").cloned(),
            );
            advance(&mut job, &[JobStatus::Running], &mut effects);
            if let Some(r) = run_id {
                job.provider_refs.insert("run_id".into(), r);
            }
            job.raw_status = native_status.or(job.raw_status.take());
            let after = (
                job.status,
                job.raw_status.clone(),
                job.provider_refs.get("run_id").cloned(),
            );
            if before != after {
                save(app, &job, effects)?;
            }
            Ok(())
        }
        SpecialistProgress::NeedsReconciliation { run_id, .. } => {
            if let Some(r) = run_id {
                job.provider_refs.insert("run_id".into(), r);
            }
            advance(&mut job, &[JobStatus::Unknown], &mut effects);
            job.rejection_reason = Some(
                "provider outcome unknown; reconcile the saved operation before any retry".into(),
            );
            save(app, &job, effects)
        }
        SpecialistProgress::Failed { run_id, .. } => {
            if let Some(r) = run_id {
                job.provider_refs.insert("run_id".into(), r);
            }
            refuse(
                app,
                &mut job,
                format!("provider operation {op} failed"),
                false,
            )
        }
        SpecialistProgress::Created { .. } => refuse(
            app,
            &mut job,
            "job operation returned an agent creation".into(),
            false,
        ),
        SpecialistProgress::Collected {
            run_id,
            provider_agent_id,
            output,
            ..
        } => {
            job.provider_refs.insert("run_id".into(), run_id.clone());
            // Authority again before applying the result.
            if let Err(reason) = authorize(app, &job) {
                return refuse(app, &mut job, format!("late result fenced: {reason}"), true);
            }
            job.raw_status = Some("completed".into());
            job.result = Some(json!({
                "kind": "specialist_run_output",
                "provider": job.provider,
                "run_id": run_id,
                "agent_id": provider_agent_id,
                "output": output,
                "domain_verified": false,
            }));
            advance(
                &mut job,
                &[JobStatus::Running, JobStatus::Verifying],
                &mut effects,
            );
            save(app, &job, effects)
        }
    }
}
