//! Local grant retirement and bounded delivery of an exact cleanup intent.
//! A bridge queue acknowledgment is not proof that the peer released anything.

use super::*;
use genesis_fetch::protocol::SendRequest;

const RELEASE_WINDOW_MINUTES: i64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetirementReason {
    Completed,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseDelivery {
    Pending,
    /// The authenticated private bridge accepted the exact message into its
    /// durable queue. This does not assert peer receipt or native release.
    BridgeQueued,
    NeedsAttention,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Retirement {
    reason: RetirementReason,
    retired_at: DateTime<Utc>,
    local_contract_state: ContractState,
    release: Message,
    pub(super) delivery: ReleaseDelivery,
    send_attempts: u32,
    attention: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RetirementRecord {
    pub reason: RetirementReason,
    pub retired_at: DateTime<Utc>,
    pub local_contract_state: ContractState,
    pub release_message_id: String,
    pub release_expires_at: DateTime<Utc>,
    pub delivery: ReleaseDelivery,
    pub send_attempts: u32,
    pub attention: Option<String>,
}

impl Retirement {
    pub(super) fn record(&self) -> RetirementRecord {
        RetirementRecord {
            reason: self.reason,
            retired_at: self.retired_at,
            local_contract_state: self.local_contract_state,
            release_message_id: self.release.message_id.clone(),
            release_expires_at: self.release.expires_at,
            delivery: self.delivery,
            send_attempts: self.send_attempts,
            attention: self.attention.clone(),
        }
    }
    fn pause(&mut self, reason: &'static str) {
        self.delivery = ReleaseDelivery::NeedsAttention;
        self.attention = Some(reason.into());
    }
}

fn reason_text(reason: RetirementReason) -> &'static str {
    match reason {
        RetirementReason::Completed => {
            "GENESIS venue recovery completed; retire the Fetch contract"
        }
        RetirementReason::Expired => {
            "GENESIS venue recovery deadline elapsed; retire the Fetch contract"
        }
    }
}

pub(super) fn validate(record: &Recovery) -> ApiResult<()> {
    let Some(retired) = &record.retirement else {
        return if record.phase == RecoveryPhase::Retired {
            Err(conflict("retired recovery is missing its release intent"))
        } else {
            Ok(())
        };
    };
    let b = &record.binding;
    let release = &retired.release;
    let valid_phase = match retired.reason {
        RetirementReason::Completed => {
            record.phase == RecoveryPhase::Completed
                && retired.local_contract_state == ContractState::Released
        }
        RetirementReason::Expired => {
            record.phase == RecoveryPhase::Retired && retired.retired_at >= b.request.deadline
        }
    };
    if !valid_phase
        || !matches!(
            retired.local_contract_state,
            ContractState::Declined | ContractState::Released
        )
        || retired.retired_at.timestamp_subsec_nanos() % 1000 != 0
        || retired.send_attempts > MAX_SEND_ATTEMPTS
        || release.validate().is_err()
        || release.org_id != b.job.org_id.as_str()
        || release.mission_id != b.job.mission_id.as_str()
        || release.contract_id != b.contract.contract_id.as_str()
        || release.correlation_id != b.job.correlation_id.as_str()
        || release.expires_at != retired.retired_at + Duration::minutes(RELEASE_WINDOW_MINUTES)
        || release.body
            != (Body::ReleaseContract {
                reason: reason_text(retired.reason).into(),
            })
        || release.message_id == b.recruit.message_id
        || release.message_id == b.task.message_id
        || record
            .accept
            .as_ref()
            .is_some_and(|m| m.message_id == release.message_id)
    {
        return Err(conflict("recovery retirement binding changed"));
    }
    Ok(())
}

/// Called under the kernel lock. Retiring the local grant and freezing the
/// cleanup message share one transaction; neither requires new work authority.
pub(super) fn begin(app: &AppState, record: &mut Recovery) -> ApiResult<()> {
    if record.retirement.is_some() {
        return Ok(());
    }
    let now = DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or_else(|| conflict("invalid retirement time"))?;
    let reason = if record.phase == RecoveryPhase::Completed {
        RetirementReason::Completed
    } else if now >= record.binding.request.deadline {
        RetirementReason::Expired
    } else {
        return Err(conflict("recovery work is not terminal"));
    };
    let state = app.current_state()?;
    let mut expected = record.binding.contract.clone();
    expected.state = match record.phase {
        RecoveryPhase::Recruiting => ContractState::Requested,
        RecoveryPhase::Accepting => ContractState::Offered,
        _ => ContractState::Accepted,
    };
    if state.contracts.get(&expected.contract_id) != Some(&expected) {
        return Err(conflict("recovery contract changed before retirement"));
    }
    let terminal = match expected.state {
        ContractState::Requested | ContractState::Offered => ContractState::Declined,
        _ => ContractState::Released,
    };
    let mut outcome = engine::update_contract(&state, &expected.contract_id, terminal)?;
    let mut job = app.store.get_job(&record.binding.job.job_id)?;
    if job_spec(&job) != record.binding.job {
        return Err(conflict("recovery job changed before retirement"));
    }
    if matches!(
        job.status,
        JobStatus::Queued | JobStatus::Submitting | JobStatus::Running | JobStatus::Verifying
    ) {
        let from = job
            .set_status(JobStatus::Failed, now)
            .map_err(|_| conflict("recovery job cannot retire"))?;
        job.rejection_reason = Some("venue recovery work deadline elapsed".into());
        outcome.effects.push(EventKind::JobStatusChanged {
            job_id: job.job_id.clone(),
            from,
            to: JobStatus::Failed,
        });
    }
    let mut release = message(
        &record.binding,
        Body::ReleaseContract {
            reason: reason_text(reason).into(),
        },
    );
    release.expires_at = now + Duration::minutes(RELEASE_WINDOW_MINUTES);
    record.retirement = Some(Retirement {
        reason,
        retired_at: now,
        local_contract_state: terminal,
        release,
        delivery: ReleaseDelivery::Pending,
        send_attempts: 0,
        attention: None,
    });
    if reason == RetirementReason::Expired {
        record.phase = RecoveryPhase::Retired;
        record.pause("recovery work deadline elapsed; local grant retired");
    }
    validate(record)?;
    commit_record(app, record, state.revision, outcome, Some(&job))
}

/// Cleanup is authorized by the exact retired grant, not by renewed task or
/// role authority. Generic managed-state APIs remain fenced.
fn authorize_release(
    app: &AppState,
    runtime: &FetchRecoveryRuntime,
    record: &Recovery,
) -> ApiResult<()> {
    validate(record)?;
    let retired = record
        .retirement
        .as_ref()
        .ok_or_else(|| conflict("missing retirement"))?;
    let b = &record.binding;
    if runtime.journal.org_id() != app.org_id.as_str()
        || runtime.journal.org_id() != b.job.org_id.as_str()
        || runtime.journal.bridge_address() != b.bridge_address
        || runtime.peer != b.peer_address
    {
        return Err(conflict("release runtime identity changed"));
    }
    let mut expected = b.contract.clone();
    expected.state = retired.local_contract_state;
    if app.current_state()?.contracts.get(&expected.contract_id) != Some(&expected) {
        return Err(conflict("retired recovery contract changed"));
    }
    Ok(())
}

fn exact_message(left: &Message, right: &Message) -> bool {
    left.canonical_sha256()
        .ok()
        .zip(right.canonical_sha256().ok())
        .is_some_and(|(left, right)| left == right)
}

/// Called with the runtime gate held, but never with the kernel lock held
/// across transport. Explicit calls may reconcile a hold; they cannot renew
/// the release deadline or exceed the lifetime send-attempt budget.
pub(super) async fn step(
    app: &AppState,
    runtime: Option<&FetchRecoveryRuntime>,
    id: &str,
) -> ApiResult<RecoveryRecord> {
    let (request, expected_revision) = {
        let _kernel = app.kernel.lock().await;
        let mut record = app.store.recovery(id)?;
        if record
            .retirement
            .as_ref()
            .is_some_and(|retired| retired.delivery == ReleaseDelivery::BridgeQueued)
        {
            return Ok(record.record());
        }
        let Some(runtime) = runtime else {
            record
                .retirement
                .as_mut()
                .ok_or_else(|| conflict("missing retirement"))?
                .pause("Fetch cleanup is not configured; exact release intent retained");
            save(app, &mut record)?;
            return Ok(record.record());
        };
        if authorize_release(app, runtime, &record).is_err() {
            record
                .retirement
                .as_mut()
                .ok_or_else(|| conflict("missing retirement"))?
                .pause("release runtime or retired grant changed; intent retained");
            save(app, &mut record)?;
            return Ok(record.record());
        }
        let retired = record
            .retirement
            .as_mut()
            .ok_or_else(|| conflict("missing retirement"))?;
        // Recover the cross-journal interruption even after expiry or the last
        // allowed attempt. This authenticates the full frozen outgoing intent.
        match runtime
            .journal
            .outgoing_queued(&record.binding.peer_address, &retired.release)
        {
            Ok(Some(true)) => {
                retired.delivery = ReleaseDelivery::BridgeQueued;
                retired.attention = None;
                save(app, &mut record)?;
                return Ok(record.record());
            }
            Err(_) => {
                retired.pause("release queue evidence conflicts with the frozen intent");
                save(app, &mut record)?;
                return Ok(record.record());
            }
            _ => {}
        }
        if retired.release.expires_at <= Utc::now() || retired.send_attempts >= MAX_SEND_ATTEMPTS {
            retired.pause("release cleanup bound reached; peer release is unconfirmed");
            save(app, &mut record)?;
            return Ok(record.record());
        }
        // Never recreate recruitment solely to clean up a missing journal.
        let prepared = runtime
            .journal
            .get(record.binding.contract.contract_id.as_str())
            .ok()
            .filter(|contract| {
                contract.peer == record.binding.peer_address
                    && exact_message(&contract.recruit, &record.binding.recruit)
            })
            .and_then(|_| {
                runtime
                    .journal
                    .prepare(
                        &record.binding.peer_address,
                        retired.release.clone(),
                        Utc::now(),
                    )
                    .ok()
            });
        if prepared.as_ref().is_none_or(|contract| {
            contract.phase != ContractPhase::Released
                || contract
                    .release
                    .as_ref()
                    .is_none_or(|release| !exact_message(release, &retired.release))
        }) || runtime
            .journal
            .outgoing_queued(&record.binding.peer_address, &retired.release)
            .ok()
            != Some(Some(false))
        {
            retired.pause("release preparation needs reconciliation; exact intent retained");
            save(app, &mut record)?;
            return Ok(record.record());
        }
        retired.send_attempts += 1;
        retired.delivery = ReleaseDelivery::Pending;
        retired.attention = None;
        let request = SendRequest {
            schema_version: 1,
            recipient: record.binding.peer_address.clone(),
            message: retired.release.clone(),
        };
        save(app, &mut record)?;
        (request, record.revision)
    };
    let runtime = runtime.ok_or_else(|| conflict("Fetch cleanup is not configured"))?;
    let response = runtime
        .client
        .send_guarded(&request, || {
            let Ok(_kernel) = app.kernel.try_lock() else {
                return false;
            };
            app.store
                .recovery(id)
                .and_then(|saved| {
                    if saved.revision != expected_revision
                        || saved.retirement.as_ref().is_none_or(|retired| {
                            retired.delivery != ReleaseDelivery::Pending
                                || retired.release.expires_at <= Utc::now()
                                || !exact_message(&retired.release, &request.message)
                        })
                    {
                        return Err(conflict("release checkpoint changed"));
                    }
                    authorize_release(app, runtime, &saved)
                })
                .is_ok()
        })
        .await;
    let _kernel = app.kernel.lock().await;
    let mut saved = app.store.recovery(id)?;
    if saved.revision != expected_revision {
        return Err(conflict("release checkpoint changed while sending"));
    }
    let retired = saved
        .retirement
        .as_mut()
        .ok_or_else(|| conflict("missing retirement"))?;
    match response {
        Ok(receipt) => {
            runtime
                .journal
                .acknowledge(&receipt)
                .map_err(|_| conflict("release queue acknowledgment could not be saved"))?;
            retired.delivery = ReleaseDelivery::BridgeQueued;
            retired.attention = None;
        }
        Err(error)
            if !error.post_may_have_run
                && error.kind == genesis_fetch::client::ClientErrorKind::NoLongerEligible =>
        {
            retired.send_attempts = retired.send_attempts.saturating_sub(1);
        }
        Err(_) if retired.send_attempts >= MAX_SEND_ATTEMPTS => {
            retired.pause("release delivery outcome uncertain; peer release is unconfirmed");
        }
        Err(_) => {}
    }
    save(app, &mut saved)?;
    Ok(saved.record())
}

/// Re-enable a configuration/preparation hold without resetting the immutable
/// message, its cleanup deadline, or its lifetime attempt count.
pub(super) fn retry(app: &AppState, record: &mut Recovery) -> ApiResult<()> {
    let retired = record
        .retirement
        .as_mut()
        .ok_or_else(|| conflict("missing retirement"))?;
    if retired.delivery != ReleaseDelivery::BridgeQueued {
        retired.delivery = ReleaseDelivery::Pending;
        retired.attention = None;
        save(app, record)?;
    }
    Ok(())
}
