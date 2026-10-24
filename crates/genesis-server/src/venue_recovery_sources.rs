//! Authenticate replacement candidates from existing backend decisions.
//!
//! Callers hold the organization's kernel lock. These functions perform no
//! provider requests or state changes. They establish historical source facts,
//! not the current authority to recruit a contractor or apply its choice.
use crate::app::AppState;
use crate::capability_invocation::{self, InvocationDecision};
use crate::error::{ApiError, ApiResult};
use crate::venue_assessment_domain::{AssessmentVerdict, VenueAssessmentPolicy};
use crate::venue_assessments::{self, AssessmentDecision, AssessmentRecord};
use crate::venue_recovery_domain::{self as domain, RecoveryCandidate, RecoveryContext};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use genesis_core::ids::{InstallationId, JobId, MissionId, ObservationId, OrgId, RequirementId};
use genesis_core::lifecycle::JobStatus;
use genesis_core::model::Provider;
use genesis_verification::{
    decode_json, json_sha256, ArtifactPin, ExtractionOutcome, SourceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_SOURCES: usize = 8;
const MAX_VENUES: usize = 16;

/// Private persisted source facts. Deserialize does not authenticate them:
/// `revalidate` reconstructs each proof before this binding may be used.
/// Deliberately no Debug implementation: extraction facts stay out of logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceSummary {
    pub job_id: JobId,
    pub proof_ref: String,
    pub output_sha256: String,
    pub created_at: DateTime<Utc>,
    pub verified_at: DateTime<Utc>,
    pub source: SourceSnapshot,
    pub extraction: ExtractionOutcome,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceBinding {
    pub reopened_assessment_id: String,
    pub reopened_proof_ref: String,
    pub reopened_job_id: JobId,
    pub replacement_requirement_id: RequirementId,
    pub reopened_observation_id: ObservationId,
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub policy: VenueAssessmentPolicy,
    pub original: SourceSummary,
    pub candidates: Vec<SourceSummary>,
    pub installation_id: InstallationId,
    pub suite_id: String,
    pub suite_sha256: String,
    pub acceptance_report_ref: String,
    pub artifact: ArtifactPin,
    /// Exact original invocation input, with only source_url removed.
    pub event_input: Value,
}

pub(crate) struct ResolvedRecoverySources {
    pub assessment: AssessmentRecord,
    pub candidates: Vec<RecoveryCandidate>,
    pub binding: SourceBinding,
}

fn invalid(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "venue_recovery_source", message)
}

fn candidate_ids_valid(ids: &[JobId]) -> ApiResult<()> {
    if ids.is_empty()
        || ids.len() > MAX_SOURCES
        || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(invalid(
            "recovery requires one to eight distinct source jobs",
        ));
    }
    Ok(())
}

fn event_input(input: &Value, source: &SourceSnapshot) -> ApiResult<Value> {
    let mut object = input
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("source input is not an event object"))?;
    if object.remove("source_url") != Some(Value::String(source.url.clone())) {
        return Err(invalid("source input does not name its verified snapshot"));
    }
    Ok(Value::Object(object))
}

struct Source {
    summary: SourceSummary,
    org_id: OrgId,
    mission_id: MissionId,
    installation_id: InstallationId,
    suite_id: String,
    suite_sha256: String,
    acceptance_report_ref: String,
    artifact: ArtifactPin,
    event_input: Value,
}

fn read_source(app: &AppState, id: &JobId) -> ApiResult<Source> {
    // read_locked authenticates the exact receipt, full original suite and
    // acceptance, historical authority, and immutable terminal projection.
    let proof = capability_invocation::read_locked(app, id)
        .map_err(|_| invalid("source invocation proof cannot be reconstructed"))?;
    let job = app
        .store
        .get_job(id)
        .map_err(|_| invalid("source job is unavailable"))?;
    if proof.decision != InvocationDecision::Verified
        || job.status != JobStatus::Succeeded
        || job.provider != Some(Provider::Apify)
        || job.org_id != app.org_id
    {
        return Err(invalid(
            "source must be a verified Apify invocation from this organization",
        ));
    }
    let record = app
        .store
        .get_capability_run(id)
        .map_err(|_| invalid("source execution evidence is unavailable"))?;
    let evidence = record
        .evidence
        .as_ref()
        .ok_or_else(|| invalid("source execution evidence is missing"))?;
    let extraction: ExtractionOutcome = decode_json(
        &serde_json::to_vec(&evidence.output)
            .map_err(|_| invalid("source output cannot be encoded"))?,
    )
    .map_err(|_| invalid("source output is not typed venue evidence"))?;
    let input = event_input(&job.input, &record.request.source)?;
    Ok(Source {
        summary: SourceSummary {
            job_id: id.clone(),
            proof_ref: proof.proof_ref,
            output_sha256: json_sha256(&evidence.output),
            created_at: job.created_at,
            verified_at: proof.verified_at,
            source: record.request.source.clone(),
            extraction,
        },
        org_id: job.org_id,
        mission_id: job.mission_id,
        installation_id: proof.installation_id,
        suite_id: proof.suite_id,
        suite_sha256: proof.suite_sha256,
        acceptance_report_ref: proof.acceptance_report_ref,
        artifact: record.request.artifact,
        event_input: input,
    })
}

fn same_origin(a: &Source, b: &Source) -> bool {
    a.org_id == b.org_id
        && a.mission_id == b.mission_id
        && a.installation_id == b.installation_id
        && a.suite_id == b.suite_id
        && a.suite_sha256 == b.suite_sha256
        && a.acceptance_report_ref == b.acceptance_report_ref
        && a.artifact == b.artifact
        && json_sha256(&a.event_input) == json_sha256(&b.event_input)
}

fn reconstruct(
    app: &AppState,
    reopened_assessment_id: &str,
    candidate_ids: &[JobId],
) -> ApiResult<ResolvedRecoverySources> {
    candidate_ids_valid(candidate_ids)?;
    let assessment = venue_assessments::read_locked(app, reopened_assessment_id)
        .map_err(|_| invalid("reopened assessment cannot be reconstructed"))?;
    let proof = assessment
        .proof
        .as_ref()
        .ok_or_else(|| invalid("assessment has no verified decision"))?;
    if assessment.status != JobStatus::Succeeded
        || proof.decision != AssessmentDecision::Accepted
        || proof
            .output
            .as_ref()
            .is_none_or(|o| o.verdict != AssessmentVerdict::Reopen)
    {
        return Err(invalid("recovery requires an accepted reopen assessment"));
    }
    let requirement_id = proof
        .requirement_id
        .clone()
        .ok_or_else(|| invalid("reopened assessment has no replacement requirement"))?;
    let observation_id = proof
        .observation_id
        .clone()
        .ok_or_else(|| invalid("reopened assessment has no observation"))?;
    let original = read_source(app, &assessment.source_job_id)?;
    let assessment_job = app
        .store
        .get_job(&assessment.job_id)
        .map_err(|_| invalid("reopened assessment job is unavailable"))?;
    if assessment_job.org_id != original.org_id
        || assessment_job.mission_id != original.mission_id
        || proof.invocation_proof_ref != original.summary.proof_ref
    {
        return Err(invalid("reopened assessment source binding changed"));
    }
    let mut sources = Vec::with_capacity(candidate_ids.len());
    let mut candidates = Vec::new();
    for id in candidate_ids {
        let source = read_source(app, id)?;
        if !same_origin(&original, &source) {
            return Err(invalid(
                "candidate source differs from the frozen event or capability",
            ));
        }
        let ExtractionOutcome::Supported { venues } = &source.summary.extraction else {
            return Err(invalid(
                "candidate source does not provide supported venue facts",
            ));
        };
        if venues.is_empty() || candidates.len().saturating_add(venues.len()) > MAX_VENUES {
            return Err(invalid("recovery candidates exceed the venue bound"));
        }
        for venue in venues {
            if venue.evidence.source_url != source.summary.source.url
                || venue.evidence.snapshot_sha256 != source.summary.source.sha256
            {
                return Err(invalid(
                    "candidate evidence does not name the verified source",
                ));
            }
            // Keep the failed venue if present. The domain verifier excludes
            // it while proving whether any actual replacement is eligible.
            candidates.push(RecoveryCandidate {
                source_job_id: source.summary.job_id.clone(),
                invocation_proof_ref: source.summary.proof_ref.clone(),
                venue: venue.clone(),
            });
        }
        sources.push(source.summary);
    }
    domain::validate_context(&RecoveryContext {
        recovery_id: "source-validation".into(),
        policy_sha256: json_sha256(&serde_json::json!(assessment.policy)),
        policy: assessment.policy.clone(),
        candidates: candidates.clone(),
    })
    .map_err(|_| invalid("candidate facts do not form a bounded recovery context"))?;
    // The real recovery ID can be longer: the caller also validates its actual
    // domain context before freezing or preparing a protocol message.
    let binding = SourceBinding {
        reopened_assessment_id: assessment.assessment_id.clone(),
        reopened_proof_ref: proof.proof_ref.clone(),
        reopened_job_id: assessment.job_id.clone(),
        replacement_requirement_id: requirement_id,
        reopened_observation_id: observation_id,
        org_id: original.org_id,
        mission_id: original.mission_id,
        policy: assessment.policy.clone(),
        original: original.summary,
        candidates: sources,
        installation_id: original.installation_id,
        suite_id: original.suite_id,
        suite_sha256: original.suite_sha256,
        acceptance_report_ref: original.acceptance_report_ref,
        artifact: original.artifact,
        event_input: original.event_input,
    };
    Ok(ResolvedRecoverySources {
        assessment,
        candidates,
        binding,
    })
}

fn validate_freshness(
    binding: &SourceBinding,
    deadline: DateTime<Utc>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    if deadline <= now
        || now
            .checked_add_signed(Duration::minutes(30))
            .is_none_or(|limit| deadline > limit)
    {
        return Err(invalid(
            "recovery deadline must be within the next thirty minutes",
        ));
    }
    for source in std::iter::once(&binding.original).chain(binding.candidates.iter()) {
        for timestamp in [source.created_at, source.verified_at] {
            validate_timestamp(timestamp, deadline, now)?;
        }
    }
    Ok(())
}

fn validate_timestamp(
    timestamp: DateTime<Utc>,
    deadline: DateTime<Utc>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    if timestamp > now
        || timestamp
            .checked_add_signed(Duration::minutes(30))
            .is_none_or(|limit| deadline > limit)
    {
        return Err(invalid(
            "recovery deadline exceeds a source freshness window",
        ));
    }
    Ok(())
}

fn maximum_deadline(
    timestamps: impl IntoIterator<Item = DateTime<Utc>>,
    now: DateTime<Utc>,
) -> ApiResult<DateTime<Utc>> {
    let mut deadline = now
        .checked_add_signed(Duration::minutes(30))
        .ok_or_else(|| invalid("recovery freshness window cannot be represented"))?;
    for timestamp in timestamps {
        if timestamp > now {
            return Err(invalid("recovery source timestamp is from the future"));
        }
        let limit = timestamp
            .checked_add_signed(Duration::minutes(30))
            .ok_or_else(|| invalid("source freshness window cannot be represented"))?;
        deadline = deadline.min(limit);
    }
    // Fetch wire messages retain microseconds. Round down so normalization can
    // never extend a source's exact freshness bound, even by a nanosecond.
    let deadline = DateTime::from_timestamp_micros(deadline.timestamp_micros())
        .ok_or_else(|| invalid("recovery deadline cannot be represented"))?;
    if deadline <= now {
        return Err(invalid("recovery source freshness window has expired"));
    }
    Ok(deadline)
}

/// Authenticate one potential source before intake freezes its exact request.
/// None means authenticated fresh evidence is unrelated to this event or has
/// no feasible alternative to the failed venue. Unavailable, stale or
/// inconsistent evidence remains an error, never permission to discard
/// uncertainty or choose a different prepared request.
/// The caller holds the kernel lock and checks current work authority separately.
pub(crate) fn prepare_candidate(
    app: &AppState,
    reopened_assessment_id: &str,
    job_id: &JobId,
    now: DateTime<Utc>,
) -> ApiResult<Option<(SourceBinding, DateTime<Utc>)>> {
    // Discovery can encounter genuine invocations for another event, or a
    // supported invocation with no replacement facts. Authenticate both sides
    // before classifying those as absent candidates; a missing proof or receipt
    // must remain retryable uncertainty instead of permitting an older winner.
    let assessment = venue_assessments::read_locked(app, reopened_assessment_id)
        .map_err(|_| invalid("reopened assessment cannot be reconstructed"))?;
    let proof = assessment
        .proof
        .as_ref()
        .ok_or_else(|| invalid("assessment has no verified decision"))?;
    if assessment.status != JobStatus::Succeeded
        || proof.decision != AssessmentDecision::Accepted
        || proof
            .output
            .as_ref()
            .is_none_or(|output| output.verdict != AssessmentVerdict::Reopen)
    {
        return Err(invalid("recovery requires an accepted reopen assessment"));
    }
    if proof.verified_at > now {
        return Err(invalid("reopened assessment decision is from the future"));
    }
    let original = read_source(app, &assessment.source_job_id)?;
    let candidate = read_source(app, job_id)?;
    maximum_deadline(
        [&original.summary, &candidate.summary]
            .into_iter()
            .flat_map(|source| [source.created_at, source.verified_at]),
        now,
    )?;
    if !same_origin(&original, &candidate)
        || !matches!(&candidate.summary.extraction,
            ExtractionOutcome::Supported { venues } if !venues.is_empty())
    {
        return Ok(None);
    }
    // Keep the manual reconstruction and all exact source/domain checks intact.
    // These repeated historical reads remain fallible under the same lock.
    let resolved = reconstruct(app, reopened_assessment_id, std::slice::from_ref(job_id))?;
    if resolved
        .assessment
        .proof
        .as_ref()
        .is_none_or(|proof| proof.verified_at > now)
    {
        return Err(invalid("reopened assessment decision is from the future"));
    }
    let deadline = maximum_deadline(
        std::iter::once(&resolved.binding.original)
            .chain(resolved.binding.candidates.iter())
            .flat_map(|source| [source.created_at, source.verified_at]),
        now,
    )?;
    validate_freshness(&resolved.binding, deadline, now)?;
    let context = RecoveryContext {
        // Match the real UUID recovery ID's wire length when enforcing bounds.
        recovery_id: "recovery_00000000-0000-0000-0000-000000000000".into(),
        policy_sha256: json_sha256(&serde_json::json!(resolved.binding.policy)),
        policy: resolved.binding.policy.clone(),
        candidates: resolved.candidates,
    };
    if !domain::has_feasible_alternative(&context)
        .map_err(|_| invalid("candidate facts do not form a bounded recovery context"))?
    {
        return Ok(None);
    }
    Ok(Some((resolved.binding, deadline)))
}

/// Freeze independently authenticated sources while all can still be used by
/// the existing assessment API. Current role/requirement/contract authority is
/// checked separately by the recovery coordinator under this same kernel lock.
pub(crate) fn resolve(
    app: &AppState,
    reopened_assessment_id: &str,
    candidate_ids: &[JobId],
    deadline: DateTime<Utc>,
) -> ApiResult<ResolvedRecoverySources> {
    let resolved = reconstruct(app, reopened_assessment_id, candidate_ids)?;
    let now = Utc::now();
    if resolved
        .assessment
        .proof
        .as_ref()
        .is_none_or(|proof| proof.verified_at > now)
    {
        return Err(invalid("reopened assessment decision is from the future"));
    }
    validate_freshness(&resolved.binding, deadline, now)?;
    Ok(resolved)
}

/// Reconstruct historical evidence exactly without renewing freshness, creating
/// work, or requiring the original producing roles to remain active.
pub(crate) fn revalidate(app: &AppState, frozen: &SourceBinding) -> ApiResult<()> {
    let ids: Vec<_> = frozen.candidates.iter().map(|s| s.job_id.clone()).collect();
    let resolved = reconstruct(app, &frozen.reopened_assessment_id, &ids)?;
    if resolved.binding != *frozen
        // Value equality treats signed zero as equal. Invocation identity uses
        // canonical JSON, so keep the same distinction in historical binding.
        || json_sha256(&resolved.binding.event_input) != json_sha256(&frozen.event_input)
    {
        return Err(invalid("recovery source binding changed after preparation"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_identity_preserves_signed_zero_but_ignores_object_key_order() {
        // Pure origin comparison fixture, not a fabricated invocation proof.
        let source = |input| Source {
            summary: SourceSummary {
                job_id: JobId::new("job"),
                proof_ref: format!("invocation:{}", "a".repeat(64)),
                output_sha256: "b".repeat(64),
                created_at: Utc::now(),
                verified_at: Utc::now(),
                source: SourceSnapshot {
                    url: "https://example.test/venue".into(),
                    sha256: "c".repeat(64),
                },
                extraction: ExtractionOutcome::Supported { venues: vec![] },
            },
            org_id: OrgId::new("org"),
            mission_id: MissionId::new("mission"),
            installation_id: InstallationId::new("installation"),
            suite_id: "suite".into(),
            suite_sha256: "d".repeat(64),
            acceptance_report_ref: "acceptance".into(),
            artifact: ArtifactPin {
                capability_id: "reader".into(),
                capability_version: "1".into(),
                source_sha256: "e".repeat(64),
                actor_id: "actor".into(),
                build_id: "build".into(),
                build_number: "1".into(),
            },
            event_input: input,
        };
        let original = source(json!({"event":"same", "ratio":0.0}));
        let changed = source(json!({"event":"same", "ratio":-0.0}));
        assert_eq!(original.event_input, changed.event_input);
        assert!(!same_origin(&original, &changed));
        let reordered = source(serde_json::from_str(r#"{"ratio":0.0,"event":"same"}"#).unwrap());
        assert!(same_origin(&original, &reordered));
    }

    #[test]
    fn only_the_exact_source_url_is_removed_from_the_event_identity() {
        let source = SourceSnapshot {
            url: "https://example.test/replacement.html".into(),
            sha256: "a".repeat(64),
        };
        let input = json!({"source_url": source.url, "attendees": 150,
            "event_id": "frozen-event", "window": {"start":"2030-01-01T09:00:00Z"}});
        let event = event_input(&input, &source).unwrap();
        assert_eq!(
            event,
            json!({"attendees":150, "event_id":"frozen-event", "window":{"start":"2030-01-01T09:00:00Z"}})
        );
        let mut other = input.clone();
        other["attendees"] = json!(151);
        assert_ne!(event, event_input(&other, &source).unwrap());
        for wrong in [
            json!({"source_url":"https://other.test/"}),
            json!({}),
            json!([]),
            json!({"source_url":null}),
        ] {
            assert!(event_input(&wrong, &source).is_err());
        }
    }

    #[test]
    fn candidate_source_ids_are_bounded_and_cannot_repeat() {
        assert!(candidate_ids_valid(&[]).is_err());
        let one = JobId::new("one");
        assert!(candidate_ids_valid(std::slice::from_ref(&one)).is_ok());
        assert!(candidate_ids_valid(&[one.clone(), one]).is_err());
        let eight: Vec<_> = (0..8).map(|i| JobId::new(format!("job-{i}"))).collect();
        assert!(candidate_ids_valid(&eight).is_ok());
        let mut nine = eight;
        nine.push(JobId::new("ninth"));
        assert!(candidate_ids_valid(&nine).is_err());
    }

    #[test]
    fn deadline_cannot_extend_an_older_source_or_future_proof() {
        let now: DateTime<Utc> = "2030-01-01T10:00:00Z".parse().unwrap();
        let created = now - Duration::minutes(20);
        let verified = now - Duration::minutes(10);
        let deadline = now + Duration::minutes(10);
        assert!(validate_timestamp(created, deadline, now).is_ok());
        assert!(validate_timestamp(verified, deadline, now).is_ok());
        assert!(validate_timestamp(created, deadline + Duration::nanoseconds(1), now).is_err());
        assert!(validate_timestamp(now + Duration::nanoseconds(1), deadline, now).is_err());
        // Reversing which timestamp is older cannot silently choose a newer
        // deadline: creation and verification each constrain the same bound.
        assert!(validate_timestamp(verified - Duration::minutes(11), deadline, now).is_err());
    }

    #[test]
    fn prepared_deadline_respects_every_source_timestamp_and_never_rounds_up() {
        let now: DateTime<Utc> = "2030-01-01T10:00:00.123456789Z".parse().unwrap();
        let full_window: DateTime<Utc> = "2030-01-01T10:30:00.123456Z".parse().unwrap();
        assert_eq!(maximum_deadline([now; 4], now).unwrap(), full_window);
        // Original creation/proof and candidate creation/proof must each cap
        // the request, independently of which timestamp happens to be oldest.
        for position in 0..4 {
            let mut timestamps = [now; 4];
            timestamps[position] = now - Duration::minutes(20);
            let deadline = maximum_deadline(timestamps, now).unwrap();
            let expected: DateTime<Utc> = "2030-01-01T10:10:00.123456Z".parse().unwrap();
            assert_eq!(deadline, expected);
            assert_eq!(deadline.timestamp_subsec_nanos() % 1000, 0);
            for timestamp in timestamps {
                validate_timestamp(timestamp, deadline, now).unwrap();
            }
            assert!(validate_timestamp(
                timestamps[position],
                deadline + Duration::microseconds(1),
                now
            )
            .is_err());
        }
    }

    #[test]
    fn candidate_deadline_rejects_future_expired_and_unrepresentable_windows() {
        let now: DateTime<Utc> = "2030-01-01T10:00:00Z".parse().unwrap();
        for position in 0..4 {
            let mut timestamps = [now - Duration::minutes(20); 4];
            timestamps[position] = now + Duration::nanoseconds(1);
            assert!(maximum_deadline(timestamps, now).is_err());
            timestamps[position] = now - Duration::minutes(30);
            assert!(maximum_deadline(timestamps, now).is_err());
            timestamps[position] = now - Duration::minutes(31);
            assert!(maximum_deadline(timestamps, now).is_err());
        }
        assert!(maximum_deadline([DateTime::<Utc>::MAX_UTC], DateTime::<Utc>::MAX_UTC).is_err());
    }

    #[test]
    fn submicrosecond_remaining_freshness_cannot_prepare_an_expired_request() {
        let now: DateTime<Utc> = "2030-01-01T10:00:00.123456400Z".parse().unwrap();
        let nearly_expired = now - Duration::minutes(30) + Duration::nanoseconds(500);
        assert!(nearly_expired + Duration::minutes(30) > now);
        assert!(maximum_deadline([nearly_expired, now, now, now], now).is_err());
        let enough_time = now - Duration::minutes(30) + Duration::microseconds(1);
        let deadline = maximum_deadline([enough_time, now, now, now], now).unwrap();
        assert_eq!(deadline.to_rfc3339(), "2030-01-01T10:00:00.123457+00:00");
        assert!(deadline > now);
    }
}
