//! Durable outbox delivery (TECH_SPEC §6).
//!
//! Items are written in the same transaction as the state change that caused
//! them. This worker delivers them with bounded exponential backoff. Delivery
//! is at least once; receivers must deduplicate.
//!
//! Real provider adapters (Elastic, CREAO, Fetch bridge) are owned by other
//! workstreams. Until one is registered, items for that provider stay pending
//! with a visible `last_error`, unless the development stub is enabled.

use crate::app::AppState;
use crate::store::{put_job, EventMeta, OutboxItem};
use chrono::Utc;
use genesis_core::events::EventKind;
use genesis_core::ids::JobId;
use genesis_core::lifecycle::JobStatus;
use std::time::Duration;

const MAX_ATTEMPTS: u32 = 20;

pub fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tick.tick().await;
            if let Err(e) = drain_once(&app).await {
                tracing::warn!(error = %e, "outbox drain failed");
            }
        }
    })
}

pub async fn drain_once(app: &AppState) -> anyhow::Result<usize> {
    let items = app.store.due_outbox(Utc::now(), 50)?;
    let n = items.len();
    for item in items {
        match deliver(app, &item).await {
            Ok(()) => app.store.mark_outbox_delivered(item.id)?,
            Err(err) => {
                let attempts = item.attempts + 1;
                let backoff = Duration::from_millis((500u64 << attempts.min(7)).min(60_000));
                let next = Utc::now() + chrono::Duration::from_std(backoff).unwrap_or_default();
                app.store
                    .mark_outbox_retry(item.id, &err, next, attempts >= MAX_ATTEMPTS)?;
            }
        }
    }
    Ok(n)
}

async fn deliver(app: &AppState, item: &OutboxItem) -> Result<(), String> {
    match (item.destination.as_str(), item.kind.as_str()) {
        (_, "job_dispatch")
            if item.payload["job_id"]
                .as_str()
                .and_then(|id| app.store.get_job(&JobId::new(id)).ok())
                .is_some_and(|job| app.store.recovery_manages_job(&job).unwrap_or(true)) =>
        {
            Err("Fetch recovery is delivered only by its durable exchange worker".into())
        }
        (_, "job_dispatch") if crate::specialist_jobs::handles(app, item) => {
            crate::specialist_jobs::deliver(app, item).await
        }
        // A restart without CREAO configuration must keep the same assessment
        // dispatch pending. A development acknowledgement would consume the
        // outbox item and strand a Running job without its native operation.
        (_, "job_dispatch")
            if item.payload["job_id"]
                .as_str()
                .and_then(|id| app.store.get_job(&JobId::new(id)).ok())
                .is_some_and(|job| crate::venue_assessments::managed(&job)) =>
        {
            Err("CREAO runtime unavailable for the saved venue assessment".into())
        }
        ("internal", _) => Ok(()),
        ("elastic", "job_dispatch") if app.elastic_agents.is_some() => {
            crate::elastic_jobs::deliver(app, item).await
        }
        (_, "assignment_activated")
            if app.providers.dev_stub_enabled() || item.destination == "dev_stub" =>
        {
            Ok(())
        }
        (_, "job_dispatch")
            if app.providers.dev_stub_enabled() || item.destination == "dev_stub" =>
        {
            dev_stub_accept_job(app, item).map_err(|e| e.to_string())
        }
        (dest, kind) => Err(format!("no adapter registered for `{dest}` ({kind})")),
    }
}

/// Development stub: acknowledge a dispatched job as running. Results still
/// have to be posted to `/api/v1/jobs/{id}/results` like any adapter would.
fn dev_stub_accept_job(app: &AppState, item: &OutboxItem) -> anyhow::Result<()> {
    let job_id = item.payload["job_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("job_dispatch without job_id"))?;
    let mut job = app.store.get_job(&JobId::new(job_id))?;
    let mut effects = Vec::new();
    let now = Utc::now();
    for to in [JobStatus::Submitting, JobStatus::Running] {
        if job.status.can_transition(to) {
            let from = job.set_status(to, now)?;
            effects.push(EventKind::JobStatusChanged {
                job_id: job.job_id.clone(),
                from,
                to,
            });
        }
    }
    if effects.is_empty() {
        return Ok(()); // already past running: duplicate delivery
    }
    job.raw_status = Some("DEVSTUB_ACCEPTED".into());
    job.provider_refs
        .insert("delivery".into(), format!("devstub-outbox-{}", item.id));
    let meta = EventMeta::mission(job.mission_id.clone()).caused_by(job.job_id.to_string());
    let events = app
        .store
        .append_events(effects, &meta, |tx| put_job(tx, &job))?;
    app.broadcast(&events);
    Ok(())
}

trait CanTransition {
    fn can_transition(self, to: Self) -> bool;
}

impl CanTransition for JobStatus {
    fn can_transition(self, to: Self) -> bool {
        use genesis_core::lifecycle::StateMachine;
        self.can_transition_to(to)
    }
}
