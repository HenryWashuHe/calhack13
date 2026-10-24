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
use genesis_core::model::{AgentBinding, Executor};

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
        || responsibility.executor != Some(Executor::Role(role_id.into()))
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
        if attention != "local_verification_pending"
            || record.status != JobStatus::Verifying
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
    spec: &ScenarioSpec,
    work: &Work,
    frozen: &Monitor,
    source: &InvocationProofResponse,
    policy: &VenueAssessmentPolicy,
    verdict: AssessmentVerdict,
) -> Result<ScenarioAssessmentReport> {
    frozen.unchanged(&driver.state(&driver.b).await?, work, Utc::now())?;
    // Generated once, never regenerated for a retry of this exact request key.
    let deadline = Utc::now() + chrono::Duration::minutes(30);
    let key = match verdict {
        AssessmentVerdict::Feasible => "monitor-baseline",
        AssessmentVerdict::Reopen => "monitor-reassessment",
    };
    let initial = driver
        .post(
            &driver.b,
            "/venue-assessments",
            json!({
                "client_request_id": format!("{}-{key}", spec.run_id), "role_id": frozen.role_id,
                "responsibility_id": frozen.responsibility_id, "source_job_id": source.job_id,
                "policy": policy, "deadline": deadline,
            }),
            "request persistent venue assessment",
        )
        .await?;
    let mut record: AssessmentResponse = decode(initial, "venue assessment response")?;
    let id = record.assessment_id.clone();
    let job_id = record.job_id.clone();
    let expected = Expected {
        id: &id,
        job_id: &job_id,
        monitor: frozen,
        policy,
        source,
        verdict,
    };
    for _ in 0..driver.limits.max_polls {
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
    let baseline = assess(
        driver,
        spec,
        work,
        &frozen,
        baseline,
        &config.policy,
        AssessmentVerdict::Feasible,
    )
    .await?;
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
    let updated = assess(
        driver,
        spec,
        work,
        &frozen,
        &updated_source,
        &config.policy,
        AssessmentVerdict::Reopen,
    )
    .await?;
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
        provider_agent_id: frozen.binding.provider_agent_id,
        monitor_role_id: monitor_id.into(),
        policy_sha256: json_sha256(&json!(config.policy)),
        baseline,
        updated,
    })
}
