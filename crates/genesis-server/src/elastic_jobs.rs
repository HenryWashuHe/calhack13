//! Delivery of jobs to Elastic Agent Builder specialists.
//!
//! An outbox `job_dispatch` item for provider `elastic` becomes one
//! synchronous converse turn with the role's agent. Authority is checked
//! against the live organization immediately before the call and again before
//! the answer is applied: the role must still be active, the responsibility
//! open, and the job's epoch current. A late answer is archived as
//! `RESULT_REJECTED`.
//!
//! The agent's answer is untrusted analysis. It is stored on the job with
//! `domain_verified: false` and the job moves to `VERIFYING`, never
//! `SUCCEEDED`; no commitment is recorded and any proposal in the answer is
//! not executed. Specialists only hold a read-only evidence search tool, so a
//! repeated turn after a lost response costs a model call but has no external
//! effect.

use crate::app::AppState;
use crate::store::{put_job, EventMeta, OutboxItem};
use chrono::Utc;
use genesis_core::engine;
use genesis_core::events::EventKind;
use genesis_core::ids::JobId;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{JobStatus, StateMachine};
use genesis_core::model::Provider;
use genesis_providers::ProviderError;
use serde_json::json;
use std::time::Duration;

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

fn save(
    app: &AppState,
    job: &Job,
    mut effects: Vec<EventKind>,
    extra: Option<EventKind>,
) -> Result<(), String> {
    if let Some(e) = extra {
        effects.push(e);
    }
    let meta = EventMeta::mission(job.mission_id.clone()).caused_by(job.job_id.to_string());
    let events = app
        .store
        .append_events(effects, &meta, |tx| put_job(tx, job))
        .map_err(|e| e.to_string())?;
    app.broadcast(&events);
    Ok(())
}

/// The prompt for one job. Context IDs let the agent cite evidence and let
/// reviewers correlate the conversation with the job.
fn job_input(job: &Job, purpose: &str) -> String {
    format!(
        "GENESIS job {job_id} (attempt {attempt}) for responsibility \"{purpose}\".\n\
         Task: {task}\n\
         Input (JSON): {input}\n\n\
         Search the organization's evidence, answer the task, and cite the evidence IDs you used. \
         Do not claim any action was taken.",
        job_id = job.job_id,
        attempt = job.attempt_no,
        task = job.task,
        input = serde_json::to_string(&job.input).unwrap_or_else(|_| "null".into()),
    )
}

/// Check live authority for `job`. Returns the agent ID on success.
fn authorize(app: &AppState, job: &Job) -> Result<(String, String), String> {
    let state = app.current_state().map_err(|e| e.message)?;
    engine::check_job_result(&state, &job.role_id, &job.responsibility_id, job.epoch)
        .map_err(|e| e.to_string())?;
    if job.deadline.is_some_and(|d| d <= Utc::now()) {
        return Err("job deadline has passed".into());
    }
    let role = state
        .roles
        .get(&job.role_id)
        .ok_or_else(|| "role not found".to_owned())?;
    let binding = role
        .binding
        .as_ref()
        .filter(|b| b.provider == Provider::Elastic)
        .ok_or_else(|| "role has no Elastic agent binding".to_owned())?;
    let purpose = state
        .responsibilities
        .get(&job.responsibility_id)
        .map(|r| r.purpose.clone())
        .unwrap_or_default();
    Ok((binding.provider_agent_id.clone(), purpose))
}

/// Deliver one `job_dispatch` item. `Ok` means the item is done (including
/// terminal refusals); `Err` asks the outbox to retry later.
pub(crate) async fn deliver(app: &AppState, item: &OutboxItem) -> Result<(), String> {
    let Some(agents) = app.elastic_agents.clone() else {
        return Err("no Elastic Agent Builder configured".into());
    };
    let job_id = item.payload["job_id"]
        .as_str()
        .ok_or_else(|| "job_dispatch without job_id".to_owned())?;
    let job_id = JobId::new(job_id);

    // 1. Live authority before the call.
    let (agent_id, input) = {
        let _guard = app.kernel.lock().await;
        let mut job = app.store.get_job(&job_id).map_err(|e| e.to_string())?;
        if job.status.is_terminal() || job.status == JobStatus::Verifying {
            return Ok(()); // duplicate delivery
        }
        match authorize(app, &job) {
            Ok((agent_id, purpose)) => {
                let mut effects = Vec::new();
                advance(
                    &mut job,
                    &[JobStatus::Submitting, JobStatus::Running],
                    &mut effects,
                );
                job.provider_refs
                    .insert("agent_id".into(), agent_id.clone());
                let input = job_input(&job, &purpose);
                save(app, &job, effects, None)?;
                (agent_id, input)
            }
            Err(reason) => {
                let mut effects = Vec::new();
                advance(&mut job, &[JobStatus::Failed], &mut effects);
                if job.status != JobStatus::Failed {
                    advance(&mut job, &[JobStatus::ResultRejected], &mut effects);
                }
                job.rejection_reason = Some(format!("not dispatched: {reason}"));
                save(app, &job, effects, None)?;
                return Ok(());
            }
        }
    };

    // 2. The provider call, outside the kernel lock.
    let turn = tokio::time::timeout(
        app.provision_timeout.max(Duration::from_secs(30)),
        agents.converse(&agent_id, &input, None),
    )
    .await
    .map_err(|_| ProviderError::Timeout)
    .and_then(|r| r);

    // 3. Re-check authority, then apply.
    let _guard = app.kernel.lock().await;
    let mut job = app.store.get_job(&job_id).map_err(|e| e.to_string())?;
    if job.status.is_terminal() || job.status == JobStatus::Verifying {
        return Ok(());
    }
    let turn = match turn {
        Ok(t) => t,
        Err(ProviderError::Rejected(reason)) => {
            let mut effects = Vec::new();
            advance(&mut job, &[JobStatus::Failed], &mut effects);
            job.rejection_reason = Some(reason);
            save(app, &job, effects, None)?;
            return Ok(());
        }
        // Transport, timeout or server errors: retry the delivery later.
        Err(e) => return Err(e.to_string()),
    };
    job.provider_refs
        .insert("conversation_id".into(), turn.conversation_id.clone());
    if let Some(round) = &turn.round_id {
        job.provider_refs.insert("round_id".into(), round.clone());
    }
    job.raw_status = Some(turn.status.clone());
    let mut effects = Vec::new();
    if let Err(reason) = authorize(app, &job) {
        advance(&mut job, &[JobStatus::ResultRejected], &mut effects);
        job.rejection_reason = Some(format!("late result fenced: {reason}"));
        let ev = EventKind::JobResultRejected {
            job_id: job.job_id.clone(),
            reason: format!("late result fenced: {reason}"),
        };
        return save(app, &job, effects, Some(ev));
    }
    job.result = Some(json!({
        "kind": "elastic_agent_answer",
        "message": turn.message,
        "tool_calls": turn.tool_calls,
        "conversation_id": turn.conversation_id,
        "round_id": turn.round_id,
        "domain_verified": false,
    }));
    advance(&mut job, &[JobStatus::Verifying], &mut effects);
    save(app, &job, effects, None)
}
