//! Observe two server-owned assessment cycles without importing provider proof.
//! HTTP claims are correlated here; private receipt/domain verification remains
//! exclusively in the backend. No failed POST is retried by this driver.
use super::*;
use crate::venue_assessment_domain::{AssessmentOutput, AssessmentVerdict, VenueAssessmentPolicy};
use chrono::{DateTime, Utc};
use genesis_core::jobs::Job;
use genesis_core::lifecycle::{
    AssignmentState, JobStatus, MissionStatus, RequirementStatus, ResponsibilityStatus,
};
use genesis_core::model::AgentBinding;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioVenueMonitoring {
    pub policy: VenueAssessmentPolicy,
    pub updated_invocation_input: Value,
}

/// Identifiers and decisions only. Neither native text nor verified venue facts
/// are copied into this report. Native IDs are not public run share links.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioMonitoringReport {
    pub subscription_id: String,
    pub provider_agent_id: String,
    pub monitor_role_id: String,
    pub policy_sha256: String,
    pub baseline: ScenarioAssessmentReport,
    pub updated: ScenarioAssessmentReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioAssessmentReport {
    pub assessment_id: String,
    pub job_id: String,
    pub source_job_id: String,
    pub invocation_proof_ref: String,
    pub proof_ref: String,
    pub native_run_id: String,
    pub receipt_sha256: String,
    pub observation_id: String,
    pub requirement_id: Option<String>,
    pub verdict: AssessmentVerdict,
}

// Separate HTTP DTOs: no backend proof token can be deserialized here.
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AssessmentResponse {
    assessment_id: String,
    job_id: String,
    source_job_id: String,
    role_id: String,
    responsibility_id: String,
    status: JobStatus,
    policy: VenueAssessmentPolicy,
    proof: Option<ProofResponse>,
    finalization_attempts: u32,
    attention: Option<String>,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProofResponse {
    schema_version: u32,
    assessment_id: String,
    decision: Decision,
    output: Option<AssessmentOutput>,
    invocation_proof_ref: String,
    execution_receipt_sha256: String,
    run_id: String,
    provider_agent_id: String,
    org_revision: u64,
    verified_at: DateTime<Utc>,
    proof_ref: String,
    observation_id: Option<String>,
    requirement_id: Option<String>,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Accepted,
    SemanticRejected,
    AuthorityRejected,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SubscriptionRequest {
    client_request_id: String,
    role_id: String,
    responsibility_id: String,
    source_job_id: String,
    policy: VenueAssessmentPolicy,
    expires_at: DateTime<Utc>,
    max_assessments: u16,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SubscriptionStatus {
    Active,
    Attention,
    Completed,
    Stopped,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionResponse {
    monitor_id: String,
    request: SubscriptionRequest,
    status: SubscriptionStatus,
    attention: Option<String>,
    assessment_count: usize,
    pending_source_job_id: Option<String>,
    pending_assessment_id: Option<String>,
    last_assessment_id: Option<String>,
    requirement_id: Option<String>,
    installation_id: String,
    suite_id: String,
    suite_sha256: String,
    acceptance_report_ref: String,
    event_cursor: u64,
}

struct Subscription {
    id: String,
    request: SubscriptionRequest,
    installation_id: String,
    suite_id: String,
    suite_sha256: String,
    acceptance_report_ref: String,
    cursor: u64,
}

impl Subscription {
    fn stopped(&self, reason: &'static str) -> ScenarioError {
        ScenarioError::Observation {
            kind: "venue monitor subscription",
            id: self.id.clone(),
            reason,
        }
    }

    /// Correlate scheduling metadata only. A child still needs the independent
    /// assessment proof, saved job and historical-effect checks below.
    fn observe(
        &mut self,
        record: &SubscriptionResponse,
        source: &InvocationProofResponse,
        previous: Option<&str>,
    ) -> Result<Option<String>> {
        if record.monitor_id != self.id
            || !safe_id(&record.monitor_id)
            || record.request != self.request
            || record.installation_id != self.installation_id
            || record.suite_id != self.suite_id
            || record.suite_sha256 != self.suite_sha256
            || record.acceptance_report_ref != self.acceptance_report_ref
            || record.event_cursor < self.cursor
            || !(1..=2).contains(&record.assessment_count)
            || [
                record.pending_source_job_id.as_deref(),
                record.pending_assessment_id.as_deref(),
                record.last_assessment_id.as_deref(),
                record.requirement_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|id| !safe_id(id))
        {
            return Err(self.stopped("subscription binding changed"));
        }
        self.cursor = record.event_cursor;
        if matches!(
            record.status,
            SubscriptionStatus::Attention | SubscriptionStatus::Stopped
        ) {
            return Err(self.stopped(
                "subscription stopped or requires operator attention; inspect retained child IDs",
            ));
        }
        if record.status == SubscriptionStatus::Active && record.attention.is_some() {
            return Err(self.stopped("active subscription retains attention"));
        }
        let count = if previous.is_some() { 2 } else { 1 };
        if previous.is_some() && record.assessment_count == 1 {
            if record.status != SubscriptionStatus::Active
                || record.pending_source_job_id.is_some()
                || record.pending_assessment_id.is_some()
                || record.last_assessment_id.as_deref() != previous
                || record.requirement_id.is_some()
            {
                return Err(self.stopped("subscription baseline checkpoint changed"));
            }
            return Ok(None);
        }
        if record.assessment_count != count {
            return Err(self.stopped("subscription consumed an unexpected source"));
        }
        if let Some(pending) = &record.pending_source_job_id {
            if pending != &source.job_id
                || record.status != SubscriptionStatus::Active
                || record.last_assessment_id.as_deref() != previous
                || record.requirement_id.is_some()
                || record
                    .pending_assessment_id
                    .as_deref()
                    .is_some_and(|id| Some(id) == previous)
            {
                return Err(self.stopped("subscription pending source or child changed"));
            }
            return Ok(record.pending_assessment_id.clone());
        }
        if record.pending_assessment_id.is_some()
            || record.last_assessment_id.is_none()
            || record.last_assessment_id.as_deref() == previous
            || match previous {
                None => {
                    record.status != SubscriptionStatus::Active || record.requirement_id.is_some()
                }
                Some(_) => {
                    record.status != SubscriptionStatus::Completed
                        || record.attention.as_deref() != Some("replacement_requirement_created")
                        || record.requirement_id.is_none()
                }
            }
        {
            return Err(self.stopped("subscription completion is inconsistent"));
        }
        Ok(record.last_assessment_id.clone())
    }
}

async fn subscription_record(
    driver: &ScenarioDriver,
    subscription: &Subscription,
) -> Result<SubscriptionResponse> {
    decode(
        driver
            .get(
                &driver.b,
                &format!("/venue-monitors/{}", subscription.id),
                "observe venue monitor subscription",
            )
            .await
            .map_err(|_| {
                subscription
                    .stopped("subscription status read failed; authorized server work may continue")
            })?,
        "venue monitor subscription",
    )
}

async fn await_child(
    driver: &ScenarioDriver,
    subscription: &mut Subscription,
    source: &InvocationProofResponse,
    previous: Option<&str>,
) -> Result<String> {
    for poll in 0..driver.limits.max_polls {
        if poll > 0 {
            tokio::time::sleep(driver.limits.poll_interval).await;
        }
        let record = subscription_record(driver, subscription).await?;
        if let Some(id) = subscription.observe(&record, source, previous)? {
            return Ok(id);
        }
    }
    Err(subscription
        .stopped("subscription child polling limit reached; authorized server work may continue"))
}

async fn await_settled(
    driver: &ScenarioDriver,
    subscription: &mut Subscription,
    source: &InvocationProofResponse,
    previous: Option<&str>,
    assessment: &ScenarioAssessmentReport,
) -> Result<()> {
    for poll in 0..driver.limits.max_polls {
        if poll > 0 {
            tokio::time::sleep(driver.limits.poll_interval).await;
        }
        let record = subscription_record(driver, subscription).await?;
        let child = subscription.observe(&record, source, previous)?;
        if child
            .as_deref()
            .is_some_and(|id| id != assessment.assessment_id)
        {
            return Err(subscription.stopped("subscription selected a different assessment"));
        }
        if record.pending_source_job_id.is_none() && child.is_some() {
            if record.last_assessment_id.as_deref() != Some(assessment.assessment_id.as_str())
                || record.requirement_id != assessment.requirement_id
            {
                return Err(
                    subscription.stopped("subscription effects differ from the accepted proof")
                );
            }
            return Ok(());
        }
    }
    Err(subscription.stopped(
        "subscription completion polling limit reached; authorized server work may continue",
    ))
}

struct AssessmentTarget {
    id: String,
    deadline: DateTime<Utc>,
}

struct Monitor {
    org_id: String,
    mission_id: String,
    role_id: String,
    responsibility_id: String,
    assignment_id: String,
    epoch: u64,
    binding: AgentBinding,
}

fn bad(message: &'static str) -> ScenarioError {
    ScenarioError::Response(message)
}
fn opaque_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn monitor(state: &OrgState, work: &Work, role_id: &str, now: DateTime<Utc>) -> Result<Monitor> {
    let role = state
        .roles
        .get(&role_id.into())
        .ok_or_else(|| bad("monitor role missing"))?;
    let binding = role
        .binding
        .as_ref()
        .ok_or_else(|| bad("monitor binding missing"))?;
    let responsibility = state
        .responsibilities
        .get(&work.responsibility.clone().into())
        .ok_or_else(|| bad("monitor responsibility missing"))?;
    let mission = state
        .missions
        .get(&work.mission.clone().into())
        .ok_or_else(|| bad("monitor mission missing"))?;
    let assignments: Vec<_> = state
        .assignments
        .values()
        .filter(|a| {
            a.role_id.as_str() == role_id
                && a.responsibility_id.as_str() == work.responsibility
                && a.epoch == responsibility.epoch
                && a.state == AssignmentState::Active
        })
        .collect();
    if role.state != RoleState::Active
        || binding.provider != Provider::Creao
        || !opaque_id(&binding.provider_agent_id)
        || !opaque_id(&binding.config_digest)
        || !role.authority.allows("monitor.venue")
        || !role.authority.allows("capability.venue_lookup")
        || !state.constitution.scopes.contains("monitor.venue")
        || !state
            .constitution
            .scopes
            .contains("capability.venue_lookup")
        || responsibility.mission_id.as_str() != work.mission
        || responsibility.owner != Owner::Role(role_id.into())
        || responsibility.status != ResponsibilityStatus::Open
        || mission.status != MissionStatus::Active
        || mission.deadline.is_some_and(|deadline| deadline <= now)
        || assignments.len() != 1
        || assignments[0]
            .lease_expires_at
            .is_some_and(|deadline| deadline <= now)
    {
        return Err(ScenarioError::Stopped(
            "persistent CREAO monitor authority is unavailable",
        ));
    }
    Ok(Monitor {
        org_id: state.org_id.to_string(),
        mission_id: work.mission.clone(),
        role_id: role_id.into(),
        responsibility_id: work.responsibility.clone(),
        assignment_id: assignments[0].assignment_id.to_string(),
        epoch: responsibility.epoch,
        binding: binding.clone(),
    })
}
impl Monitor {
    fn unchanged(&self, state: &OrgState, work: &Work, now: DateTime<Utc>) -> Result<()> {
        let current = monitor(state, work, &self.role_id, now)?;
        if current.org_id != self.org_id
            || current.assignment_id != self.assignment_id
            || current.epoch != self.epoch
            || current.binding != self.binding
        {
            return Err(ScenarioError::Stopped(
                "persistent CREAO monitor binding changed",
            ));
        }
        Ok(())
    }
}

struct Expected<'a> {
    id: &'a str,
    job_id: &'a str,
    monitor: &'a Monitor,
    policy: &'a VenueAssessmentPolicy,
    source: &'a InvocationProofResponse,
    verdict: AssessmentVerdict,
}

/// Only a completed backend proof can finish observation. A temporary local
/// claim is not failure, while Unknown must never cause a replacement run.
fn observe(record: &AssessmentResponse, expected: &Expected<'_>) -> Result<bool> {
    if record.assessment_id != expected.id
        || record.job_id != expected.job_id
        || !safe_id(&record.assessment_id)
        || !safe_id(&record.job_id)
        || record.source_job_id != expected.source.job_id
        || record.role_id != expected.monitor.role_id
        || record.responsibility_id != expected.monitor.responsibility_id
        || record.policy != *expected.policy
        || record.finalization_attempts > 3
    {
        return Err(bad("assessment observation binding mismatch"));
    }
    if record.status == JobStatus::Succeeded {
        if record.attention.is_some() {
            return Err(bad("completed assessment retains attention"));
        }
        validate_proof(
            record
                .proof
                .as_ref()
                .ok_or_else(|| bad("missing assessment proof"))?,
            expected,
        )?;
        return Ok(true);
    }
    if record.proof.is_some()
        || matches!(
            record.status,
            JobStatus::Failed | JobStatus::ResultRejected | JobStatus::Unknown
        )
    {
        return Err(ScenarioError::Observation {
            kind: "venue assessment",
            id: record.assessment_id.clone(),
            reason: "assessment stopped without an accepted result",
        });
    }
    if let Some(attention) = &record.attention {
        // The backend retries either local checkpoint while attempts remain.
        // Continuing to observe does not authorize another native request.
        if !matches!(
            attention.as_str(),
            "local_verification_pending" | "local_verification_failed"
        ) || record.status != JobStatus::Verifying
            || !(1..3).contains(&record.finalization_attempts)
        {
            return Err(ScenarioError::Observation {
                kind: "venue assessment",
                id: record.assessment_id.clone(),
                reason: "assessment requires operator attention",
            });
        }
    }
    Ok(false)
}

fn validate_proof(proof: &ProofResponse, expected: &Expected<'_>) -> Result<()> {
    let output = proof
        .output
        .as_ref()
        .ok_or_else(|| bad("missing accepted assessment output"))?;
    let source = expected
        .source
        .case_report
        .as_ref()
        .ok_or_else(|| bad("missing source case proof"))?;
    let all_checks = output.checks.capacity
        && output.checks.price
        && output.checks.window
        && output.checks.availability
        && output.checks.step_free;
    if proof.schema_version != 1
        || proof.assessment_id != expected.id
        || proof.decision != Decision::Accepted
        || proof.invocation_proof_ref != expected.source.proof_ref
        || proof.provider_agent_id != expected.monitor.binding.provider_agent_id
        || !opaque_id(&proof.run_id)
        || !valid_hash(&proof.execution_receipt_sha256)
        || !proof
            .proof_ref
            .strip_prefix("venue-assessment:")
            .is_some_and(valid_hash)
        || proof.verified_at < expected.source.verified_at
        || proof.org_revision < expected.source.org_revision
        || !proof.observation_id.as_deref().is_some_and(safe_id)
        || output.schema_version != 1
        || output.assessment_id != expected.id
        || output.source_job_id.as_str() != expected.source.job_id
        || output.invocation_proof_ref != expected.source.proof_ref
        || output.policy_sha256 != json_sha256(&json!(expected.policy))
        || output.selected_venue_id != expected.policy.selected_venue_id
        || output.evidence.source_url != source.source.url
        || output.evidence.snapshot_sha256 != source.source.sha256
        || output.evidence.locator.is_empty()
        || output.evidence.locator.len() > 1024
        || output.verdict != expected.verdict
        || all_checks != (expected.verdict == AssessmentVerdict::Feasible)
        || match expected.verdict {
            AssessmentVerdict::Feasible => proof.requirement_id.is_some(),
            AssessmentVerdict::Reopen => !proof.requirement_id.as_deref().is_some_and(safe_id),
        }
    {
        return Err(bad("assessment proof binding mismatch"));
    }
    Ok(())
}

fn validate_job(
    job: &Job,
    record: &AssessmentResponse,
    expected: &Expected<'_>,
    deadline: DateTime<Utc>,
) -> Result<()> {
    let proof = record
        .proof
        .as_ref()
        .ok_or_else(|| bad("missing assessment job proof"))?;
    let saved: ProofResponse = decode(
        job.result
            .as_ref()
            .and_then(|v| v.get("proof"))
            .cloned()
            .unwrap_or(Value::Null),
        "assessment job proof",
    )?;
    if job.job_id.as_str() != expected.job_id
        || job.org_id.as_str() != expected.monitor.org_id
        || job.mission_id.as_str() != expected.monitor.mission_id
        || job.role_id.as_str() != expected.monitor.role_id
        || job.responsibility_id.as_str() != expected.monitor.responsibility_id
        || job.assignment_id.as_str() != expected.monitor.assignment_id
        || job.epoch != expected.monitor.epoch
        || job.provider != Some(Provider::Creao)
        || job.status != JobStatus::Succeeded
        || job.deadline != Some(deadline)
        || job.created_at > proof.verified_at
        || job.provider_refs.get("run_id") != Some(&proof.run_id)
        || job.provider_refs.get("agent_id") != Some(&proof.provider_agent_id)
        || job
            .result
            .as_ref()
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str)
            != Some("venue_assessment")
        || saved != *proof
    {
        return Err(bad("assessment job binding mismatch"));
    }
    Ok(())
}

fn validate_effects(
    state: &OrgState,
    proof: &ProofResponse,
    expected: &Expected<'_>,
) -> Result<()> {
    if state.org_id.as_str() != expected.monitor.org_id
        || Some(state.revision) != proof.org_revision.checked_add(1)
    {
        return Err(bad("assessment committed revision mismatch"));
    }
    let observation = proof
        .observation_id
        .as_ref()
        .and_then(|id| state.observations.get(&id.clone().into()))
        .ok_or_else(|| bad("assessment observation missing"))?;
    if observation.mission_id.as_str() != expected.monitor.mission_id
        || observation.source != "creao-venue-assessment"
        || observation.evidence_ref.as_ref() != Some(&proof.proof_ref)
        || observation.observed_at != proof.verified_at
        || observation.summary.is_empty()
    {
        return Err(bad("assessment observation effect mismatch"));
    }
    let triggered: Vec<_> = state
        .requirements
        .values()
        .filter(|r| r.triggering_evidence.contains(&observation.observation_id))
        .collect();
    match expected.verdict {
        AssessmentVerdict::Feasible if !triggered.is_empty() => {
            return Err(bad("feasible assessment created replacement work"))
        }
        AssessmentVerdict::Reopen => {
            let requirement = proof
                .requirement_id
                .as_ref()
                .and_then(|id| state.requirements.get(&id.clone().into()))
                .ok_or_else(|| bad("replacement requirement missing"))?;
            if triggered.len() != 1
                || requirement.mission_id.as_str() != expected.monitor.mission_id
                || requirement.status != RequirementStatus::Unmet
                || requirement.owner != Owner::MissionController
                || requirement.triggering_evidence != vec![observation.observation_id.clone()]
                || requirement.summary
                    != "Find a replacement venue satisfying the verified event constraints."
                || requirement.acceptance.is_empty()
            {
                return Err(bad("replacement requirement effect mismatch"));
            }
        }
        _ => {}
    }
    Ok(())
}

async fn assess(
    driver: &ScenarioDriver,
    work: &Work,
    frozen: &Monitor,
    source: &InvocationProofResponse,
    policy: &VenueAssessmentPolicy,
    verdict: AssessmentVerdict,
    target: &AssessmentTarget,
) -> Result<ScenarioAssessmentReport> {
    frozen.unchanged(&driver.state(&driver.b).await?, work, Utc::now())?;
    let deadline = target.deadline;
    let initial = driver
        .get(
            &driver.b,
            &format!("/venue-assessments/{}", target.id),
            "read subscription assessment",
        )
        .await?;
    let mut record: AssessmentResponse = decode(initial, "venue assessment response")?;
    let id = target.id.clone();
    let job_id = record.job_id.clone();
    let expected = Expected {
        id: &id,
        job_id: &job_id,
        monitor: frozen,
        policy,
        source,
        verdict,
    };
    // The first child GET uses the first poll. Observe every permitted GET,
    // including the last one, before reporting exhaustion.
    for polls in 1..=driver.limits.max_polls {
        if observe(&record, &expected)? {
            let persisted: AssessmentResponse = decode(
                driver
                    .get(
                        &driver.b,
                        &format!("/venue-assessments/{id}"),
                        "read persisted assessment",
                    )
                    .await?,
                "persisted assessment",
            )?;
            if record != persisted {
                return Err(bad("persisted assessment changed"));
            }
            let proof = record
                .proof
                .as_ref()
                .ok_or_else(|| bad("missing persisted assessment proof"))?;
            let job: Job = decode(
                driver
                    .get(&driver.b, &format!("/jobs/{job_id}"), "read assessment job")
                    .await?,
                "assessment job",
            )?;
            validate_job(&job, &record, &expected, deadline)?;
            let revision = proof
                .org_revision
                .checked_add(1)
                .ok_or_else(|| bad("assessment revision overflow"))?;
            let committed: OrgState = decode(
                driver
                    .get(
                        &driver.b,
                        &format!("/organization/revisions/{revision}"),
                        "read assessment effects",
                    )
                    .await?,
                "assessment effects",
            )?;
            validate_effects(&committed, proof, &expected)?;
            frozen.unchanged(&committed, work, proof.verified_at)?;
            frozen.unchanged(&driver.state(&driver.b).await?, work, Utc::now())?;
            return Ok(ScenarioAssessmentReport {
                assessment_id: id,
                job_id,
                source_job_id: source.job_id.clone(),
                invocation_proof_ref: proof.invocation_proof_ref.clone(),
                proof_ref: proof.proof_ref.clone(),
                native_run_id: proof.run_id.clone(),
                receipt_sha256: proof.execution_receipt_sha256.clone(),
                observation_id: proof
                    .observation_id
                    .clone()
                    .ok_or_else(|| bad("missing assessment observation"))?,
                requirement_id: proof.requirement_id.clone(),
                verdict,
            });
        }
        if polls == driver.limits.max_polls {
            break;
        }
        tokio::time::sleep(driver.limits.poll_interval).await;
        record = decode(
            driver
                .get(
                    &driver.b,
                    &format!("/venue-assessments/{id}"),
                    "observe venue assessment",
                )
                .await
                .map_err(|_| ScenarioError::Observation {
                    kind: "venue assessment",
                    id: id.clone(),
                    reason: "assessment status read failed",
                })?,
            "venue assessment observation",
        )?;
    }
    Err(ScenarioError::Observation {
        kind: "venue assessment",
        id,
        reason: "assessment polling limit reached",
    })
}

/// The child deadline the server freezes for a source (`venue_monitors::
/// new_intent`, since #99): the subscription expiry, capped by the source
/// job's creation and the source proof's verification, each plus 30 minutes.
/// The job is read from this backend, so no client clock is involved.
async fn child_deadline(
    driver: &ScenarioDriver,
    source: &InvocationProofResponse,
    expires_at: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let job: Job = decode(
        driver
            .get(
                &driver.b,
                &format!("/jobs/{}", source.job_id),
                "read monitoring source job",
            )
            .await?,
        "monitoring source job",
    )?;
    Ok(expires_at
        .min(job.created_at + chrono::Duration::minutes(30))
        .min(source.verified_at + chrono::Duration::minutes(30)))
}

pub(super) async fn run(
    driver: &ScenarioDriver,
    spec: &ScenarioSpec,
    work: &Work,
    monitor_id: &str,
    baseline: &InvocationProofResponse,
    invocation: &InvocationContext<'_>,
) -> Result<ScenarioMonitoringReport> {
    let config = spec
        .venue_monitoring
        .as_ref()
        .ok_or(ScenarioError::Invalid(
            "missing venue monitoring configuration",
        ))?;
    let frozen = monitor(
        &driver.state(&driver.b).await?,
        work,
        monitor_id,
        Utc::now(),
    )?;
    let suite_sha256 = baseline.suite_sha256.clone();
    let ledger = driver
        .get(&driver.b, "/ledger", "read pre-assessment commitments")
        .await?;
    // A timestamp from this backend avoids using client clock skew to infer
    // the child deadline. Each child's deadline is this expiration capped by
    // its own source (see `child_deadline`).
    let expires_at = baseline.verified_at + chrono::Duration::minutes(30);
    if expires_at <= Utc::now() {
        return Err(bad("baseline is too old to register monitoring"));
    }
    let request = SubscriptionRequest {
        client_request_id: format!("{}-venue-monitor", spec.run_id),
        role_id: frozen.role_id.clone(),
        responsibility_id: frozen.responsibility_id.clone(),
        source_job_id: baseline.job_id.clone(),
        policy: config.policy.clone(),
        expires_at,
        max_assessments: 2,
    };
    // A lost registration response is ambiguous. Never retry this POST or
    // restart the fresh-only scenario against populated organizations.
    let registered: SubscriptionResponse = decode(
        driver
            .post(
                &driver.b,
                "/venue-monitors",
                json!(request),
                "register persistent venue monitor",
            )
            .await?,
        "venue monitor registration",
    )?;
    let mut subscription = Subscription {
        id: registered.monitor_id.clone(),
        request,
        installation_id: baseline.installation_id.clone(),
        suite_id: baseline.suite_id.clone(),
        suite_sha256: baseline.suite_sha256.clone(),
        acceptance_report_ref: baseline.acceptance_report_ref.clone(),
        cursor: registered.event_cursor,
    };
    subscription.observe(&registered, baseline, None)?;
    let baseline_id = await_child(driver, &mut subscription, baseline, None).await?;
    let baseline_report = assess(
        driver,
        work,
        &frozen,
        baseline,
        &config.policy,
        AssessmentVerdict::Feasible,
        &AssessmentTarget {
            id: baseline_id,
            deadline: child_deadline(driver, baseline, expires_at).await?,
        },
    )
    .await?;
    await_settled(driver, &mut subscription, baseline, None, &baseline_report).await?;
    let updated_context = InvocationContext {
        input: &config.updated_invocation_input,
        ..*invocation
    };
    frozen.unchanged(&driver.state(&driver.b).await?, work, Utc::now())?;
    let updated_source = driver
        .invoke(
            &updated_context,
            monitor_id,
            "monitor-update",
            Some(&suite_sha256),
        )
        .await?;
    let updated_id = await_child(
        driver,
        &mut subscription,
        &updated_source,
        Some(&baseline_report.assessment_id),
    )
    .await?;
    let updated = assess(
        driver,
        work,
        &frozen,
        &updated_source,
        &config.policy,
        AssessmentVerdict::Reopen,
        &AssessmentTarget {
            id: updated_id,
            deadline: child_deadline(driver, &updated_source, expires_at).await?,
        },
    )
    .await?;
    await_settled(
        driver,
        &mut subscription,
        &updated_source,
        Some(&baseline_report.assessment_id),
        &updated,
    )
    .await?;
    let baseline = baseline_report;
    if baseline.assessment_id == updated.assessment_id
        || baseline.job_id == updated.job_id
        || baseline.source_job_id == updated.source_job_id
        || baseline.invocation_proof_ref == updated.invocation_proof_ref
        || baseline.proof_ref == updated.proof_ref
        || baseline.native_run_id == updated.native_run_id
        || baseline.receipt_sha256 == updated.receipt_sha256
        || baseline.observation_id == updated.observation_id
    {
        return Err(bad("monitor updates reused execution evidence"));
    }
    if driver
        .get(&driver.b, "/ledger", "read post-assessment commitments")
        .await?
        != ledger
    {
        return Err(bad("monitoring unexpectedly changed commitments"));
    }
    Ok(ScenarioMonitoringReport {
        subscription_id: subscription.id,
        provider_agent_id: frozen.binding.provider_agent_id,
        monitor_role_id: monitor_id.into(),
        policy_sha256: json_sha256(&json!(config.policy)),
        baseline,
        updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::model::{
        Assignment, AuthorityGrant, Constitution, Executor, Mission, Observation, Requirement,
        Responsibility, Role,
    };

    struct Fixture {
        state: OrgState,
        work: Work,
        source: InvocationProofResponse,
        policy: VenueAssessmentPolicy,
        record: Value,
        now: DateTime<Utc>,
    }
    impl Fixture {
        fn new(verdict: AssessmentVerdict) -> Self {
            let now: DateTime<Utc> = "2026-10-22T12:00:00Z".parse().unwrap();
            let policy: VenueAssessmentPolicy = serde_json::from_value(json!({
                "selected_venue_id":"hall", "attendees":150,"budget":{"minor_units":100000,"currency":"USD"},
                "window":{"start":"2026-11-01T18:00:00Z","end":"2026-11-01T20:00:00Z"},"step_free_required":true,
            })).unwrap();
            let source: InvocationProofResponse = serde_json::from_value(json!({
                "schema_version":1,"job_id":"source_job","installation_id":"installation","request_sha256":"a".repeat(64),
                "suite_id":"suite-b","suite_sha256":"b".repeat(64),"acceptance_report_ref":"acceptance:original",
                "org_revision":7,"verified_at":now,"decision":"verified","proof_ref":format!("invocation:{}","c".repeat(64)),
                "case_report":{"schema_version":1,"evaluator_version":"v1","purpose":"invocation_case","decision":"accepted",
                    "suite_id":"suite-b","suite_sha256":"b".repeat(64),"case_id":"portal","input_sha256":"d".repeat(64),
                    "source":{"url":"https://venue.example/one","sha256":"e".repeat(64)},
                    "artifact":{"capability_id":"venue","capability_version":"1.0.0","source_sha256":"f".repeat(64),"actor_id":"actor","build_id":"build","build_number":"1"},
                    "run_id":"apify-run","dataset_id":"dataset","evidence_sha256":"0".repeat(64),"extraction":"supported","issues":[]}
            })).unwrap();
            let record = json!({
                "assessment_id":"assessment_test","job_id":"assessment_job","source_job_id":"source_job","role_id":"monitor",
                "responsibility_id":"responsibility","status":"SUCCEEDED","policy":policy,"finalization_attempts":1,"attention":null,
                "proof":{"schema_version":1,"assessment_id":"assessment_test","decision":"accepted",
                    "output":{"schema_version":1,"assessment_id":"assessment_test","source_job_id":"source_job",
                        "invocation_proof_ref":source.proof_ref,"policy_sha256":json_sha256(&json!(policy)),"selected_venue_id":"hall",
                        "evidence":{"source_url":"https://venue.example/one","snapshot_sha256":"e".repeat(64),"locator":"#hall"},
                        "checks":{"capacity":verdict == AssessmentVerdict::Feasible,"price":true,"window":true,"availability":true,"step_free":true},"verdict":verdict},
                    "invocation_proof_ref":source.proof_ref,"execution_receipt_sha256":"1".repeat(64),"run_id":"native-run",
                    "provider_agent_id":"native-agent","org_revision":10,"verified_at":now+chrono::Duration::seconds(1),
                    "proof_ref":format!("venue-assessment:{}","2".repeat(64)),"observation_id":"observed",
                    "requirement_id": if verdict == AssessmentVerdict::Reopen {Some("replacement")} else {None}}
            });
            let mut state = OrgState::new(
                "org_b".into(),
                "B",
                Constitution {
                    scopes: ["monitor.venue".into(), "capability.venue_lookup".into()]
                        .into_iter()
                        .collect(),
                    ..Default::default()
                },
            );
            state.revision = 11;
            state.missions.insert(
                "mission".into(),
                Mission {
                    mission_id: "mission".into(),
                    goal: "Venue".into(),
                    constraints: vec![],
                    deadline: None,
                    success_checks: vec![],
                    status: MissionStatus::Active,
                    created_at: now,
                },
            );
            state.roles.insert(
                "monitor".into(),
                Role {
                    role_id: "monitor".into(),
                    name: "Monitor".into(),
                    charter: "Maintain venue".into(),
                    authority: AuthorityGrant::new(["monitor.venue", "capability.venue_lookup"], 0),
                    reports_to: None,
                    state: RoleState::Active,
                    binding: Some(AgentBinding {
                        provider: Provider::Creao,
                        provider_agent_id: "native-agent".into(),
                        config_digest: "config".into(),
                        conversation_ref: None,
                    }),
                    capabilities: vec![],
                    created_in_revision: 3,
                    retired_in_revision: None,
                },
            );
            state.responsibilities.insert(
                "responsibility".into(),
                Responsibility {
                    responsibility_id: "responsibility".into(),
                    mission_id: "mission".into(),
                    purpose: "Monitor venue".into(),
                    requirement_id: None,
                    owner: Owner::Role("monitor".into()),
                    executor: Some(Executor::Role("monitor".into())),
                    trigger: "event".into(),
                    termination: "explicit".into(),
                    status: ResponsibilityStatus::Open,
                    epoch: 2,
                },
            );
            state.assignments.insert(
                "assignment".into(),
                Assignment {
                    assignment_id: "assignment".into(),
                    role_id: "monitor".into(),
                    responsibility_id: "responsibility".into(),
                    epoch: 2,
                    state: AssignmentState::Active,
                    lease_expires_at: None,
                },
            );
            state.observations.insert(
                "observed".into(),
                Observation {
                    observation_id: "observed".into(),
                    mission_id: "mission".into(),
                    source: "creao-venue-assessment".into(),
                    summary: "Verified venue assessment".into(),
                    evidence_ref: Some(format!("venue-assessment:{}", "2".repeat(64))),
                    observed_at: now + chrono::Duration::seconds(1),
                },
            );
            if verdict == AssessmentVerdict::Reopen {
                state.requirements.insert(
                    "replacement".into(),
                    Requirement {
                        requirement_id: "replacement".into(),
                        mission_id: "mission".into(),
                        summary:
                            "Find a replacement venue satisfying the verified event constraints."
                                .into(),
                        triggering_evidence: vec!["observed".into()],
                        acceptance: "Verified replacement meets frozen policy".into(),
                        status: RequirementStatus::Unmet,
                        owner: Owner::MissionController,
                    },
                );
            }
            Self {
                state,
                work: Work {
                    mission: "mission".into(),
                    observation: "initial".into(),
                    requirement: "original".into(),
                    role: "old".into(),
                    responsibility: "responsibility".into(),
                },
                source,
                policy,
                record,
                now,
            }
        }
        fn frozen(&self) -> Monitor {
            monitor(&self.state, &self.work, "monitor", self.now).unwrap()
        }
        fn expected<'a>(&'a self, frozen: &'a Monitor, verdict: AssessmentVerdict) -> Expected<'a> {
            Expected {
                id: "assessment_test",
                job_id: "assessment_job",
                monitor: frozen,
                policy: &self.policy,
                source: &self.source,
                verdict,
            }
        }
        fn response(&self) -> AssessmentResponse {
            serde_json::from_value(self.record.clone()).unwrap()
        }
        fn job(&self) -> Job {
            serde_json::from_value(json!({
                "job_id":"assessment_job","org_id":"org_b","mission_id":"mission","role_id":"monitor","responsibility_id":"responsibility",
                "assignment_id":"assignment","epoch":2,"task":"Assess venue","input":{},"attempt_id":"attempt","attempt_no":1,
                "correlation_id":"correlation","idempotency_key":"request","deadline":self.now+chrono::Duration::minutes(30),
                "capability":null,"provider":"creao","status":"SUCCEEDED","provider_refs":{"agent_id":"native-agent","run_id":"native-run"},
                "raw_status":"CREAO_ASSESSMENT_VERIFIED","result":{"kind":"venue_assessment","proof":self.record["proof"]},
                "rejection_reason":null,"created_at":self.now,"updated_at":self.now+chrono::Duration::seconds(1),
            })).unwrap()
        }
    }

    #[test]
    fn exact_backend_cycles_validate_without_importing_a_receipt() {
        for verdict in [AssessmentVerdict::Feasible, AssessmentVerdict::Reopen] {
            let f = Fixture::new(verdict);
            let frozen = f.frozen();
            let expected = f.expected(&frozen, verdict);
            let record = f.response();
            assert!(observe(&record, &expected).unwrap());
            validate_job(
                &f.job(),
                &record,
                &expected,
                f.now + chrono::Duration::minutes(30),
            )
            .unwrap();
            validate_effects(&f.state, record.proof.as_ref().unwrap(), &expected).unwrap();
        }
    }

    #[test]
    fn foreign_source_agent_policy_or_claim_cannot_complete_observation() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let frozen = f.frozen();
        let expected = f.expected(&frozen, AssessmentVerdict::Feasible);
        for (pointer, value) in [
            ("/source_job_id", json!("foreign")),
            ("/policy/attendees", json!(151)),
            ("/proof/provider_agent_id", json!("foreign")),
            ("/proof/execution_receipt_sha256", json!("not-a-digest")),
            ("/proof/output/policy_sha256", json!("9".repeat(64))),
            (
                "/proof/output/evidence/snapshot_sha256",
                json!("9".repeat(64)),
            ),
            ("/proof/output/checks/price", json!(false)),
            ("/proof/decision", json!("authority_rejected")),
            ("/proof/requirement_id", json!("unexpected")),
            ("/proof/verified_at", json!("2026-10-22T11:00:00Z")),
        ] {
            let mut bad_record = f.record.clone();
            *bad_record.pointer_mut(pointer).unwrap() = value;
            let parsed = serde_json::from_value(bad_record).unwrap();
            assert!(observe(&parsed, &expected).is_err(), "{pointer}");
        }
        let mut unknown = f.record.clone();
        unknown["proof"]["caller_verified"] = json!(true);
        assert!(serde_json::from_value::<AssessmentResponse>(unknown).is_err());
    }

    #[test]
    fn unknown_and_exhausted_attention_stop_but_transient_local_claim_waits() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let frozen = f.frozen();
        let expected = f.expected(&frozen, AssessmentVerdict::Feasible);
        let mut record = f.response();
        record.proof = None;
        record.status = JobStatus::Verifying;
        for attention in ["local_verification_pending", "local_verification_failed"] {
            record.attention = Some(attention.into());
            for attempt in [1, 2] {
                record.finalization_attempts = attempt;
                assert!(!observe(&record, &expected).unwrap());
            }
            record.finalization_attempts = 3;
            assert!(observe(&record, &expected).is_err());
        }
        record.finalization_attempts = 0;
        record.attention = None;
        for status in [
            JobStatus::Unknown,
            JobStatus::Failed,
            JobStatus::ResultRejected,
            JobStatus::Succeeded,
        ] {
            record.status = status;
            assert!(observe(&record, &expected).is_err());
        }
    }

    #[test]
    fn retired_role_handoff_authorizes_the_new_owner_without_an_executor() {
        use genesis_core::capability::StaticCatalog;
        use genesis_core::change::{
            Handoff, KeyAllocation, OrgChangeProposal, OrgOp, RoleJustification, RoleRef,
        };
        use genesis_core::engine::{authorize_dispatch, check_job_result, commit};

        let f = Fixture::new(AssessmentVerdict::Feasible);
        let now = f.now + chrono::Duration::seconds(2);
        let proposal = OrgChangeProposal {
            expected_org_revision: f.state.revision,
            trigger_evidence_ids: vec!["observed".into()],
            rationale: "Retire the temporary role and hand monitoring to a persistent role".into(),
            budget_limit_cents: Some(0),
            operations: vec![
                OrgOp::CreateRole {
                    key: "successor".into(),
                    name: "Persistent venue monitor".into(),
                    charter: "Monitor the inherited venue responsibility".into(),
                    authority: AuthorityGrant::new(["monitor.venue", "capability.venue_lookup"], 0),
                    reports_to: None,
                    provider: Provider::Creao,
                    justification: RoleJustification::Lifecycle,
                },
                OrgOp::RetireRole {
                    role_id: "monitor".into(),
                    handoffs: vec![Handoff {
                        responsibility_id: "responsibility".into(),
                        to: RoleRef::new_key("successor"),
                    }],
                },
            ],
        };
        let allocation = KeyAllocation::allocate(&proposal);
        let bindings = std::collections::BTreeMap::from([(
            "successor".into(),
            AgentBinding {
                provider: Provider::Creao,
                provider_agent_id: "native-successor".into(),
                config_digest: "successor-config".into(),
                conversation_ref: None,
            },
        )]);
        let state = commit(
            &f.state,
            &proposal,
            &allocation,
            &bindings,
            &StaticCatalog::default(),
            now,
        )
        .unwrap()
        .state;
        let successor = &allocation.roles["successor"];
        let responsibility_id = "responsibility".into();
        let responsibility = &state.responsibilities[&responsibility_id];
        assert_eq!(state.roles[&"monitor".into()].state, RoleState::Retired);
        assert_eq!(responsibility.owner, Owner::Role(successor.clone()));
        assert_eq!(responsibility.executor, None);
        assert_eq!(responsibility.epoch, 3);
        let grant =
            authorize_dispatch(&state, successor, &responsibility_id, Some("monitor.venue"))
                .unwrap();
        check_job_result(&state, successor, &responsibility_id, grant.epoch).unwrap();
        let frozen = monitor(&state, &f.work, successor.as_str(), now).unwrap();
        assert_eq!(frozen.assignment_id, grant.assignment_id.to_string());
        frozen.unchanged(&state, &f.work, now).unwrap();
        assert!(monitor(&state, &f.work, "monitor", now).is_err());

        let mut wrong_owner = state.clone();
        wrong_owner
            .responsibilities
            .get_mut(&responsibility_id)
            .unwrap()
            .owner = Owner::MissionController;
        assert!(monitor(&wrong_owner, &f.work, successor.as_str(), now).is_err());
        let mut stale_assignment = state;
        stale_assignment
            .assignments
            .get_mut(&grant.assignment_id)
            .unwrap()
            .epoch -= 1;
        assert!(monitor(&stale_assignment, &f.work, successor.as_str(), now).is_err());
    }

    #[test]
    fn development_override_cannot_replace_monitor_or_expired_assignment() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let frozen = f.frozen();
        let mut state = f.state.clone();
        state
            .roles
            .get_mut(&"monitor".into())
            .unwrap()
            .binding
            .as_mut()
            .unwrap()
            .provider = Provider::DevStub;
        assert!(monitor(&state, &f.work, "monitor", f.now).is_err());
        let mut state = f.state.clone();
        state
            .assignments
            .get_mut(&"assignment".into())
            .unwrap()
            .lease_expires_at = Some(f.now);
        assert!(frozen.unchanged(&state, &f.work, f.now).is_err());
        let mut state = f.state.clone();
        state.missions.get_mut(&"mission".into()).unwrap().status = MissionStatus::Failed;
        assert!(frozen.unchanged(&state, &f.work, f.now).is_err());
        let mut state = f.state.clone();
        state
            .roles
            .get_mut(&"monitor".into())
            .unwrap()
            .binding
            .as_mut()
            .unwrap()
            .config_digest = "changed".into();
        assert!(frozen.unchanged(&state, &f.work, f.now).is_err());
        let mut state = f.state.clone();
        state
            .roles
            .get_mut(&"monitor".into())
            .unwrap()
            .authority
            .scopes
            .remove("monitor.venue");
        assert!(frozen.unchanged(&state, &f.work, f.now).is_err());
    }

    #[test]
    fn saved_job_must_match_native_run_assignment_and_exact_proof() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let frozen = f.frozen();
        let expected = f.expected(&frozen, AssessmentVerdict::Feasible);
        let record = f.response();
        let deadline = f.now + chrono::Duration::minutes(30);
        let mut job = f.job();
        job.epoch += 1;
        assert!(validate_job(&job, &record, &expected, deadline).is_err());
        let mut job = f.job();
        job.provider_refs.insert("run_id".into(), "foreign".into());
        assert!(validate_job(&job, &record, &expected, deadline).is_err());
        let mut job = f.job();
        job.result.as_mut().unwrap()["proof"]["execution_receipt_sha256"] = json!("9".repeat(64));
        assert!(validate_job(&job, &record, &expected, deadline).is_err());
    }

    #[test]
    fn replacement_effect_requires_exact_trigger_owner_and_committed_revision() {
        let f = Fixture::new(AssessmentVerdict::Reopen);
        let frozen = f.frozen();
        let expected = f.expected(&frozen, AssessmentVerdict::Reopen);
        let record = f.response();
        let proof = record.proof.as_ref().unwrap();
        let mut state = f.state.clone();
        state
            .requirements
            .get_mut(&"replacement".into())
            .unwrap()
            .owner = Owner::Role("monitor".into());
        assert!(validate_effects(&state, proof, &expected).is_err());
        let mut state = f.state.clone();
        state
            .requirements
            .get_mut(&"replacement".into())
            .unwrap()
            .triggering_evidence
            .push("unrelated".into());
        assert!(validate_effects(&state, proof, &expected).is_err());
        let mut state = f.state.clone();
        state
            .observations
            .get_mut(&"observed".into())
            .unwrap()
            .evidence_ref = Some("foreign-proof".into());
        assert!(validate_effects(&state, proof, &expected).is_err());
        let mut state = f.state.clone();
        state.revision += 1;
        assert!(validate_effects(&state, proof, &expected).is_err());
    }

    #[test]
    fn feasible_proof_cannot_hide_a_linked_replacement_requirement() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let reopened = Fixture::new(AssessmentVerdict::Reopen);
        let frozen = f.frozen();
        let expected = f.expected(&frozen, AssessmentVerdict::Feasible);
        let mut state = f.state.clone();
        state.requirements = reopened.state.requirements;
        assert!(validate_effects(&state, f.response().proof.as_ref().unwrap(), &expected).is_err());
    }

    fn subscription_fixture(f: &Fixture) -> (Subscription, Value) {
        let request = SubscriptionRequest {
            client_request_id: "scenario-monitor".into(),
            role_id: "monitor".into(),
            responsibility_id: "responsibility".into(),
            source_job_id: f.source.job_id.clone(),
            policy: f.policy.clone(),
            expires_at: f.now + chrono::Duration::minutes(30),
            max_assessments: 2,
        };
        let subscription = Subscription {
            id: "monitor_subscription".into(),
            request: request.clone(),
            installation_id: f.source.installation_id.clone(),
            suite_id: f.source.suite_id.clone(),
            suite_sha256: f.source.suite_sha256.clone(),
            acceptance_report_ref: f.source.acceptance_report_ref.clone(),
            cursor: 20,
        };
        let record = json!({
            "monitor_id":subscription.id,"request":request,"status":"active","attention":null,"assessment_count":1,
            "pending_source_job_id":f.source.job_id,"pending_assessment_id":null,"last_assessment_id":null,"requirement_id":null,
            "installation_id":f.source.installation_id,"suite_id":f.source.suite_id,"suite_sha256":f.source.suite_sha256,
            "acceptance_report_ref":f.source.acceptance_report_ref,"event_cursor":20,
        });
        (subscription, record)
    }

    #[test]
    fn subscription_preserves_both_source_cycles_and_requires_replacement_completion() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let (mut subscription, mut value) = subscription_fixture(&f);
        let response =
            |value: &Value| serde_json::from_value::<SubscriptionResponse>(value.clone()).unwrap();
        assert_eq!(
            subscription
                .observe(&response(&value), &f.source, None)
                .unwrap(),
            None
        );
        value["pending_assessment_id"] = json!("assessment_baseline");
        assert_eq!(
            subscription
                .observe(&response(&value), &f.source, None)
                .unwrap()
                .as_deref(),
            Some("assessment_baseline")
        );
        value["pending_source_job_id"] = Value::Null;
        value["pending_assessment_id"] = Value::Null;
        value["last_assessment_id"] = json!("assessment_baseline");
        assert_eq!(
            subscription
                .observe(&response(&value), &f.source, None)
                .unwrap()
                .as_deref(),
            Some("assessment_baseline")
        );
        let mut updated = Fixture::new(AssessmentVerdict::Reopen).source;
        updated.job_id = "source_updated".into();
        assert_eq!(
            subscription
                .observe(&response(&value), &updated, Some("assessment_baseline"))
                .unwrap(),
            None
        );
        value["assessment_count"] = json!(2);
        value["event_cursor"] = json!(30);
        value["pending_source_job_id"] = json!("source_updated");
        value["pending_assessment_id"] = json!("assessment_updated");
        assert_eq!(
            subscription
                .observe(&response(&value), &updated, Some("assessment_baseline"))
                .unwrap()
                .as_deref(),
            Some("assessment_updated")
        );
        let mut rebound = value.clone();
        rebound["pending_assessment_id"] = json!("assessment_baseline");
        assert!(subscription
            .observe(&response(&rebound), &updated, Some("assessment_baseline"))
            .is_err());
        value["pending_source_job_id"] = Value::Null;
        value["pending_assessment_id"] = Value::Null;
        value["last_assessment_id"] = json!("assessment_updated");
        value["status"] = json!("completed");
        value["attention"] = json!("replacement_requirement_created");
        value["requirement_id"] = json!("replacement");
        assert_eq!(
            subscription
                .observe(&response(&value), &updated, Some("assessment_baseline"))
                .unwrap()
                .as_deref(),
            Some("assessment_updated")
        );
        value["requirement_id"] = Value::Null;
        value["attention"] = json!("assessment_limit_reached");
        assert!(subscription
            .observe(&response(&value), &updated, Some("assessment_baseline"))
            .is_err());
    }

    /// Exercise the production subscription GET loop, including its final poll.
    async fn subscription_sequence(
        f: &Fixture,
        replies: Vec<Value>,
        max_polls: usize,
    ) -> (Result<String>, Vec<String>) {
        use axum::extract::{Request, State};
        use axum::{Json, Router};
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};
        type Replies = Arc<Mutex<(VecDeque<Value>, Vec<String>)>>;
        async fn reply(State(state): State<Replies>, request: Request) -> Json<Value> {
            assert_eq!(request.method(), "GET");
            assert_eq!(
                request.uri().path(),
                "/api/v1/venue-monitors/monitor_subscription"
            );
            let mut state = state.lock().unwrap();
            state.1.push(request.method().to_string());
            Json(
                state
                    .0
                    .pop_front()
                    .expect("unexpected extra subscription poll"),
            )
        }
        let state = Arc::new(Mutex::new((replies.into(), vec![])));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().fallback(reply).with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let driver = ScenarioDriver::new(
            ScenarioEndpoint::new("https://unused.example", "fixture-operator").unwrap(),
            ScenarioEndpoint::new(&format!("http://{address}"), "fixture-operator").unwrap(),
            ScenarioLimits {
                request_timeout: Duration::from_secs(2),
                poll_interval: Duration::from_millis(1),
                max_polls,
                total_timeout: Duration::from_secs(5),
                ..ScenarioLimits::default()
            },
        )
        .unwrap();
        let (mut subscription, _) = subscription_fixture(f);
        let result = await_child(&driver, &mut subscription, &f.source, None).await;
        server.abort();
        let seen = state.lock().unwrap().1.clone();
        (result, seen)
    }

    #[tokio::test]
    async fn subscription_last_poll_returns_child_without_an_assessment_post() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let (_, waiting) = subscription_fixture(&f);
        let mut ready = waiting.clone();
        ready["pending_assessment_id"] = json!("assessment_test");
        let (result, seen) = subscription_sequence(&f, vec![waiting, ready], 2).await;
        assert_eq!(result.unwrap(), "assessment_test");
        assert_eq!(seen, vec!["GET", "GET"]);
    }

    #[tokio::test]
    async fn subscription_attention_corruption_and_exhaustion_stop_without_replay() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        for (pointer, replacement) in [
            ("/status", json!("attention")),
            ("/status", json!("stopped")),
            ("/assessment_count", json!(3)),
            ("/event_cursor", json!(19)),
            ("/request/source_job_id", json!("foreign")),
            ("/suite_sha256", json!("9".repeat(64))),
            ("/pending_source_job_id", json!("foreign")),
            ("/installation_id", json!("foreign")),
        ] {
            let (_, mut record) = subscription_fixture(&f);
            *record.pointer_mut(pointer).unwrap() = replacement;
            let (result, seen) = subscription_sequence(&f, vec![record], 3).await;
            assert!(result.is_err(), "{pointer} was accepted");
            assert_eq!(
                seen,
                vec!["GET"],
                "invalid subscription must not be retried"
            );
        }
        let (_, waiting) = subscription_fixture(&f);
        let (result, seen) = subscription_sequence(&f, vec![waiting.clone()], 1).await;
        assert!(result.is_err());
        assert_eq!(seen, vec!["GET"]);
        let mut malformed = waiting;
        malformed["caller_verified"] = json!(true);
        let (result, seen) = subscription_sequence(&f, vec![malformed], 3).await;
        assert!(result.is_err());
        assert_eq!(seen, vec!["GET"]);
    }

    /// Script only the organization HTTP API. This exercises the production
    /// assess loop and its persisted job/history reads, not native verification.
    async fn poll_sequence(
        f: &Fixture,
        observations: Vec<Value>,
        max_polls: usize,
    ) -> (Result<ScenarioAssessmentReport>, Vec<(String, String)>) {
        use axum::body::to_bytes;
        use axum::extract::{Request, State};
        use axum::{Json, Router};
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};
        struct Replies {
            observations: VecDeque<Value>,
            last: Value,
            job: Value,
            org: Value,
            seen: Vec<(String, String)>,
        }
        async fn reply(
            State(shared): State<Arc<Mutex<Replies>>>,
            request: Request,
        ) -> (axum::http::StatusCode, Json<Value>) {
            let method = request.method().to_string();
            let path = request.uri().path().to_owned();
            assert_eq!(
                request.headers()["authorization"],
                "Bearer fixture-operator"
            );
            let bytes = to_bytes(request.into_body(), 64 * 1024).await.unwrap();
            assert!(bytes.is_empty(), "child observation sends no body or POST");
            let mut state = shared.lock().unwrap();
            state.seen.push((method.clone(), path.clone()));
            let value = match (method.as_str(), path.as_str()) {
                ("GET", "/api/v1/venue-assessments/assessment_test") => {
                    if let Some(next) = state.observations.pop_front() {
                        state.last = next;
                    }
                    state.last.clone()
                }
                ("GET", "/api/v1/jobs/assessment_job") => state.job.clone(),
                ("GET", "/api/v1/organization" | "/api/v1/organization/revisions/11") => {
                    state.org.clone()
                }
                _ => return (axum::http::StatusCode::NOT_FOUND, Json(Value::Null)),
            };
            (axum::http::StatusCode::OK, Json(value))
        }
        let mut queued = f.record.clone();
        queued["status"] = json!("QUEUED");
        queued["proof"] = Value::Null;
        queued["finalization_attempts"] = json!(0);
        let shared = Arc::new(Mutex::new(Replies {
            last: queued.clone(),
            observations: observations.into(),
            job: json!(f.job()),
            org: json!(f.state),
            seen: vec![],
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().fallback(reply).with_state(shared.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let driver = ScenarioDriver::new(
            ScenarioEndpoint::new("https://unused.example", "fixture-operator").unwrap(),
            ScenarioEndpoint::new(&format!("http://{address}"), "fixture-operator").unwrap(),
            ScenarioLimits {
                request_timeout: Duration::from_secs(2),
                poll_interval: Duration::from_millis(1),
                max_polls,
                total_timeout: Duration::from_secs(5),
                ..ScenarioLimits::default()
            },
        )
        .unwrap();
        let frozen = f.frozen();
        let result = assess(
            &driver,
            &f.work,
            &frozen,
            &f.source,
            &f.policy,
            AssessmentVerdict::Feasible,
            &AssessmentTarget {
                id: "assessment_test".into(),
                deadline: f.now + chrono::Duration::minutes(30),
            },
        )
        .await;
        server.abort();
        let seen = shared.lock().unwrap().seen.clone();
        assert_eq!(
            seen.iter().filter(|(method, _)| method == "POST").count(),
            0,
            "observation must never recreate the assessment or manually verify it"
        );
        (result, seen)
    }

    #[tokio::test]
    async fn success_on_the_last_allowed_poll_checks_persisted_job_and_effects() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let (result, seen) = poll_sequence(&f, vec![f.record.clone()], 1).await;
        assert!(
            result.is_ok(),
            "last allowed observation was discarded: {result:?}"
        );
        assert_eq!(
            seen.iter()
                .filter(|(method, path)| method == "GET"
                    && path == "/api/v1/venue-assessments/assessment_test")
                .count(),
            2,
            "one observation GET plus a persisted-proof reread"
        );
        assert!(seen
            .iter()
            .any(|(_, path)| path == "/api/v1/jobs/assessment_job"));
        assert!(seen
            .iter()
            .any(|(_, path)| path == "/api/v1/organization/revisions/11"));
    }

    #[tokio::test]
    async fn local_failures_recover_within_the_backend_retry_budget_without_another_post() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        let failures: Vec<_> = [1, 2]
            .into_iter()
            .map(|attempt| {
                let mut record = f.record.clone();
                record["status"] = json!("VERIFYING");
                record["proof"] = Value::Null;
                record["attention"] = json!("local_verification_failed");
                record["finalization_attempts"] = json!(attempt);
                record
            })
            .collect();
        let mut success = f.record.clone();
        success["finalization_attempts"] = json!(3);
        let (result, seen) =
            poll_sequence(&f, failures.into_iter().chain([success]).collect(), 3).await;
        assert!(
            result.is_ok(),
            "backend still has automatic retries: {result:?}"
        );
        assert_eq!(
            seen.iter()
                .filter(|(method, path)| method == "GET"
                    && path == "/api/v1/venue-assessments/assessment_test")
                .count(),
            4
        );
    }

    #[tokio::test]
    async fn unknown_malformed_and_exhausted_responses_stop_without_further_polls() {
        let f = Fixture::new(AssessmentVerdict::Feasible);
        for case in ["unknown", "malformed", "exhausted", "still_running"] {
            let mut record = f.record.clone();
            record["status"] = json!("VERIFYING");
            record["proof"] = Value::Null;
            match case {
                "unknown" => record["status"] = json!("UNKNOWN"),
                "malformed" => record["caller_verified"] = json!(true),
                "exhausted" => {
                    record["attention"] = json!("local_verification_failed");
                    record["finalization_attempts"] = json!(3);
                }
                _ => {}
            }
            let budget = if case == "still_running" { 1 } else { 3 };
            let (result, seen) = poll_sequence(&f, vec![record, f.record.clone()], budget).await;
            assert!(result.is_err(), "{case} must not be reported as completed");
            assert_eq!(
                seen.iter()
                    .filter(|(method, path)| method == "GET"
                        && path == "/api/v1/venue-assessments/assessment_test")
                    .count(),
                1,
                "{case}: no extra poll, retry or fabricated completion"
            );
            assert!(!seen
                .iter()
                .any(|(_, path)| path == "/api/v1/jobs/assessment_job"));
        }
    }
}
