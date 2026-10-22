//! Pure interpretation of separately observed acquisition and scheduler state.
//! A completed observation is not an accepted artifact or publication grant.

use crate::capability_adoption_store::{AdoptionAttention, AdoptionFence, AdoptionPhase};
use chrono::{DateTime, Duration, Utc};
use genesis_verification::{validate_artifact, ArtifactPin};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObservationDecision {
    Wait,
    Complete,
    Attention(&'static str),
}

fn positive_version(value: Option<&Value>) -> Result<u64, &'static str> {
    value
        .and_then(Value::as_u64)
        .filter(|version| *version > 0)
        .ok_or("invalid acquisition observation version")
}

/// Interpret one pair of GET responses without importing any evidence. The
/// caller must separately bind the acquisition to the expected organization,
/// mission, requirement, responsibility and role, and validate its final artifact.
pub(super) fn acquisition_observation(
    status: &Value,
    automation: &Value,
    expected_id: &str,
) -> Result<ObservationDecision, &'static str> {
    if expected_id.is_empty()
        || expected_id.len() > 256
        || !expected_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        || status.get("id").and_then(Value::as_str) != Some(expected_id)
        || automation.get("id").and_then(Value::as_str) != Some(expected_id)
    {
        return Err("acquisition observation identity mismatch");
    }
    let version = positive_version(status.get("version"))?;
    let current = positive_version(automation.get("current_version"))?;
    let observed = match automation.get("observed_version") {
        Some(Value::Null) => None,
        Some(value) => Some(positive_version(Some(value))?),
        None => return Err("invalid acquisition scheduler version"),
    };
    if observed.is_some_and(|observed| observed > current) {
        return Err("invalid acquisition scheduler version");
    }
    let enabled = automation
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or("invalid acquisition automation configuration")?;
    let authorized = status
        .get("currently_authorized")
        .and_then(Value::as_bool)
        .ok_or("invalid acquisition observation authority")?;
    let phase = status
        .get("phase")
        .and_then(Value::as_str)
        .filter(|phase| {
            matches!(
                *phase,
                "generating"
                    | "forging"
                    | "verified"
                    | "rejected"
                    | "failed"
                    | "needs_reconciliation"
                    | "fenced"
            )
        })
        .ok_or("invalid acquisition observation phase")?;
    let state = automation
        .get("state")
        .and_then(Value::as_str)
        .filter(|state| {
            matches!(
                *state,
                "disabled" | "ready" | "waiting" | "attention" | "inactive"
            )
        })
        .ok_or("invalid acquisition automation state")?;
    if !authorized || phase == "fenced" {
        return Ok(ObservationDecision::Attention(
            "acquisition authority changed",
        ));
    }
    if !enabled || state == "disabled" {
        return Ok(ObservationDecision::Attention(
            "automatic acquisition is disabled",
        ));
    }
    // The worker can commit between these GETs. Do not combine a scheduler
    // decision for one version with acquisition semantics from another.
    if current != version {
        return Ok(ObservationDecision::Wait);
    }
    match state {
        "ready" | "waiting" => Ok(ObservationDecision::Wait),
        "attention" => {
            if observed != Some(current) {
                return Err("acquisition attention is not bound to the observed version");
            }
            let reason = match automation.get("error_code").and_then(Value::as_str) {
                Some("retry_read") => "acquisition requires an explicit read retry",
                Some("quota") => "acquisition count quota requires attention",
                Some("configuration") => "acquisition configuration requires attention",
                Some("invalid_record") => "acquisition record requires attention",
                Some("conflict") => "acquisition changed concurrently",
                Some("internal") => "acquisition worker requires attention",
                _ => return Err("invalid acquisition attention code"),
            };
            Ok(ObservationDecision::Attention(reason))
        }
        "inactive" if phase == "verified" => Ok(ObservationDecision::Complete),
        "inactive" => Ok(ObservationDecision::Attention(
            "acquisition stopped without verification",
        )),
        // Disabled was handled above; keeping an exhaustive fallback makes
        // malformed responses fail closed if this vocabulary later changes.
        _ => Err("invalid acquisition automation state"),
    }
}

pub(super) struct AdoptionExpected<'a> {
    pub id: &'a str,
    pub org_id: &'a str,
    pub mission_id: &'a str,
    pub requirement_id: &'a str,
    pub role_id: &'a str,
    pub responsibility_id: &'a str,
    pub suite_id: &'a str,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ObservedSelection {
    capability_id: String,
    version: String,
    publisher: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedAdoption {
    id: String,
    version: u64,
    org_id: String,
    mission_id: String,
    requirement_id: String,
    role_id: String,
    responsibility_id: String,
    assignment_id: String,
    epoch: u64,
    suite_id: String,
    phase: AdoptionPhase,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
    next_attempt_at: Option<DateTime<Utc>>,
    discovery_attempts: u32,
    selection: Option<ObservedSelection>,
    artifact: Option<ArtifactPin>,
    installation_id: Option<String>,
    job_ids: Vec<String>,
    acceptance_ref: Option<String>,
    rejection_ref: Option<String>,
    attention: Option<AdoptionAttention>,
    fence_reason: Option<AdoptionFence>,
    authority: String,
}

fn opaque_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}

fn minted_id(value: &str) -> bool {
    opaque_id(value)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn observation_ref(value: Option<&str>, prefix: &str) -> bool {
    value
        .and_then(|value| value.strip_prefix(prefix))
        .is_some_and(hex_digest)
}

/// Bound the decoded DTO before cloning strings during typed deserialization.
/// This is metadata only: no arbitrary source, output, or oracle field is valid.
fn bounded_adoption_status(value: &Value) -> bool {
    if value.as_object().is_none_or(|object| object.len() != 25) {
        return false;
    }
    let mut pending = vec![(value, 0)];
    let mut bytes = 0usize;
    let mut nodes = 0;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > 256 || depth > 4 {
            return false;
        }
        match value {
            Value::String(text) => {
                if text.len() > 1024 {
                    return false;
                }
                bytes += text.len();
            }
            Value::Object(object) => {
                if object.len() > 25 {
                    return false;
                }
                for (key, value) in object {
                    if key.len() > 64 {
                        return false;
                    }
                    bytes += key.len();
                    pending.push((value, depth + 1));
                }
            }
            Value::Array(array) => {
                if array.len() > 128 {
                    return false;
                }
                pending.extend(array.iter().map(|value| (value, depth + 1)));
            }
            _ => bytes += 32,
        }
        if bytes > 64 * 1024 || pending.len() > 256 {
            return false;
        }
    }
    true
}

impl ObservedAdoption {
    fn read(status: &Value, expected: &AdoptionExpected<'_>) -> Result<Self, &'static str> {
        if !bounded_adoption_status(status) {
            return Err("invalid adoption observation shape");
        }
        let observed =
            Self::deserialize(status).map_err(|_| "invalid adoption observation shape")?;
        let suffix = observed
            .id
            .strip_prefix("adopt_")
            .ok_or("invalid adoption observation identity")?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || observed.id != expected.id
            || [
                (&observed.org_id, expected.org_id),
                (&observed.mission_id, expected.mission_id),
                (&observed.requirement_id, expected.requirement_id),
                (&observed.role_id, expected.role_id),
                (&observed.responsibility_id, expected.responsibility_id),
                (&observed.suite_id, expected.suite_id),
            ]
            .into_iter()
            .any(|(actual, expected)| !opaque_id(actual) || actual != expected)
            || !opaque_id(&observed.assignment_id)
            || observed.version == 0
            || observed.version > i64::MAX as u64
            || observed.epoch > i64::MAX as u64
        {
            return Err("adoption observation binding mismatch");
        }
        let duration = observed.deadline - observed.created_at;
        if observed.created_at > observed.updated_at
            || duration <= Duration::zero()
            || duration > Duration::seconds(1800)
            || observed.discovery_attempts > 3
            || observed.authority.is_empty()
            || observed.authority.len() > 256
            || observed.phase.is_active() != observed.next_attempt_at.is_some()
            || observed.next_attempt_at.is_some_and(|due| {
                due < observed.updated_at
                    || due > observed.deadline
                    || due - observed.updated_at > Duration::seconds(60)
            })
            || observed.selection.is_some() != observed.artifact.is_some()
            || observed.selection.is_some() != observed.installation_id.is_some()
            || observed
                .installation_id
                .as_deref()
                .is_some_and(|id| !minted_id(id))
            || observed.job_ids.iter().any(|id| !minted_id(id))
            || observed.job_ids.iter().collect::<BTreeSet<_>>().len() != observed.job_ids.len()
            || (!observed.job_ids.is_empty()
                && (observed.selection.is_none() || observed.job_ids.len() < 3))
            || (observed.phase == AdoptionPhase::Accepted) != observed.acceptance_ref.is_some()
            || (observed.phase == AdoptionPhase::Rejected) != observed.rejection_ref.is_some()
            || (observed.phase == AdoptionPhase::Attention) != observed.attention.is_some()
            || (observed.phase == AdoptionPhase::Fenced) != observed.fence_reason.is_some()
            || observed
                .acceptance_ref
                .as_deref()
                .is_some_and(|reference| !observation_ref(Some(reference), "verification:"))
            || observed
                .rejection_ref
                .as_deref()
                .is_some_and(|reference| !observation_ref(Some(reference), "adoption-rejection:"))
        {
            return Err("incoherent adoption observation");
        }
        if let (Some(selection), Some(artifact)) = (&observed.selection, &observed.artifact) {
            if observed.discovery_attempts == 0
                || !opaque_id(&selection.publisher)
                || selection.publisher == observed.org_id
                || selection.capability_id != artifact.capability_id
                || selection.version != artifact.capability_version
                || selection.version.eq_ignore_ascii_case("latest")
                || validate_artifact(artifact).is_err()
            {
                return Err("invalid adoption selected artifact");
            }
        }
        match observed.phase {
            AdoptionPhase::Discovering if observed.selection.is_some() => {
                return Err("incoherent adoption discovery")
            }
            AdoptionPhase::Selected
                if observed.selection.is_none() || !observed.job_ids.is_empty() =>
            {
                return Err("incoherent adoption selection")
            }
            AdoptionPhase::Checking | AdoptionPhase::Accepted | AdoptionPhase::Rejected
                if observed.selection.is_none() || observed.job_ids.len() < 3 =>
            {
                return Err("incoherent adoption check batch")
            }
            _ => {}
        }
        Ok(observed)
    }

    fn follows(&self, previous: &Self) -> bool {
        let phase_allowed = match previous.phase {
            AdoptionPhase::Discovering => true,
            AdoptionPhase::Selected => self.phase != AdoptionPhase::Discovering,
            AdoptionPhase::Checking => !matches!(
                self.phase,
                AdoptionPhase::Discovering | AdoptionPhase::Selected
            ),
            AdoptionPhase::Attention => self.phase == AdoptionPhase::Fenced,
            AdoptionPhase::Accepted | AdoptionPhase::Rejected | AdoptionPhase::Fenced => false,
        };
        phase_allowed
            && self.version > previous.version
            && self.assignment_id == previous.assignment_id
            && self.epoch == previous.epoch
            && self.created_at == previous.created_at
            && self.deadline == previous.deadline
            && self.updated_at >= previous.updated_at
            && self.discovery_attempts >= previous.discovery_attempts
            && (previous.selection.is_none()
                || self.discovery_attempts == previous.discovery_attempts)
            && (previous.selection.is_none() || self.selection == previous.selection)
            && (previous.artifact.is_none() || self.artifact == previous.artifact)
            && (previous.installation_id.is_none()
                || self.installation_id == previous.installation_id)
            && (previous.job_ids.is_empty() || self.job_ids == previous.job_ids)
    }
}

/// Observe server-owned adoption without polling its step endpoint or importing
/// an acceptance proof. Completion still requires the caller's independent
/// installation, artifact, child-run and local-evidence checks.
pub(super) fn adoption_observation(
    status: &Value,
    previous: Option<&Value>,
    expected: &AdoptionExpected<'_>,
) -> Result<ObservationDecision, &'static str> {
    let observed = ObservedAdoption::read(status, expected)?;
    if let Some(previous_value) = previous {
        let prior = ObservedAdoption::read(previous_value, expected)?;
        if observed.version == prior.version {
            if status != previous_value {
                return Err("adoption changed without a new version");
            }
        } else if !observed.follows(&prior) {
            return Err("adoption observation regressed or changed its binding");
        }
    }
    match observed.phase {
        AdoptionPhase::Discovering | AdoptionPhase::Selected | AdoptionPhase::Checking => {
            Ok(ObservationDecision::Wait)
        }
        AdoptionPhase::Accepted => Ok(ObservationDecision::Complete),
        AdoptionPhase::Rejected => Ok(ObservationDecision::Attention(
            "independent adoption verification rejected the capability",
        )),
        AdoptionPhase::Attention => Ok(ObservationDecision::Attention(
            "adoption requires operator attention",
        )),
        AdoptionPhase::Fenced => Ok(ObservationDecision::Attention(
            "adoption authority changed or expired",
        )),
    }
}

#[cfg(test)]
mod adoption_tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "adopt_0123456789abcdef0123456789abcdef";

    fn expected() -> AdoptionExpected<'static> {
        AdoptionExpected {
            id: ID,
            org_id: "Org B 東京",
            mission_id: "mission",
            requirement_id: "requirement",
            role_id: "role",
            responsibility_id: "responsibility",
            suite_id: "suite",
        }
    }

    fn status(phase: &str) -> Value {
        let selection = !matches!(phase, "discovering" | "attention" | "fenced");
        let jobs = matches!(phase, "checking" | "accepted" | "rejected");
        let active = matches!(phase, "discovering" | "selected" | "checking");
        json!({
            "id":ID,"version":8,"org_id":"Org B 東京","mission_id":"mission",
            "requirement_id":"requirement","role_id":"role","responsibility_id":"responsibility",
            "assignment_id":"Assignment 東京","epoch":1,"suite_id":"suite","phase":phase,
            "created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:10Z",
            "deadline":"2026-10-22T10:30:00Z",
            "next_attempt_at":if active {json!("2026-10-22T10:00:11Z")} else {Value::Null},
            "discovery_attempts":1,
            "selection":if selection {json!({"capability_id":"venue-capability","version":"1.0.0","publisher":"Org A 東京"})} else {Value::Null},
            "artifact":if selection {json!({"capability_id":"venue-capability","capability_version":"1.0.0",
                "source_sha256":"a".repeat(64),"actor_id":"actor_1","build_id":"build_1","build_number":"1.0.1"})} else {Value::Null},
            "installation_id":if selection {json!("install_1")} else {Value::Null},
            "job_ids":if jobs {json!(["job_1","job_2","job_3"])} else {json!([])},
            "acceptance_ref":if phase=="accepted" {json!(format!("verification:{}","b".repeat(64)))} else {Value::Null},
            "rejection_ref":if phase=="rejected" {json!(format!("adoption-rejection:{}","c".repeat(64)))} else {Value::Null},
            "attention":if phase=="attention" {json!("no_candidate")} else {Value::Null},
            "fence_reason":if phase=="fenced" {json!("deadline_expired")} else {Value::Null},
            "authority":"observational metadata only"
        })
    }

    #[test]
    fn observes_forward_progress_and_skipped_states_without_step_calls() {
        let mut previous = None;
        for (version, phase) in [
            (1, "discovering"),
            (2, "discovering"),
            (3, "selected"),
            (4, "checking"),
            (5, "accepted"),
        ] {
            let mut current = status(phase);
            current["version"] = json!(version);
            assert_eq!(
                adoption_observation(&current, previous.as_ref(), &expected()),
                Ok(if phase == "accepted" {
                    ObservationDecision::Complete
                } else {
                    ObservationDecision::Wait
                })
            );
            assert_eq!(
                adoption_observation(&current, Some(&current), &expected()),
                Ok(if phase == "accepted" {
                    ObservationDecision::Complete
                } else {
                    ObservationDecision::Wait
                })
            );
            previous = Some(current);
        }
        let mut discovery = status("discovering");
        discovery["version"] = json!(1);
        assert_eq!(
            adoption_observation(&status("accepted"), Some(&discovery), &expected()),
            Ok(ObservationDecision::Complete)
        );
        // Expired wall-clock authority is not part of historical proof lifetime.
        // The timestamps above are intentionally fixed rather than Utc::now().
        assert_eq!(
            adoption_observation(&status("accepted"), None, &expected()),
            Ok(ObservationDecision::Complete)
        );
    }

    #[test]
    fn binds_every_work_identity_and_keeps_assignment_deadline_and_creation_fixed() {
        for key in [
            "id",
            "org_id",
            "mission_id",
            "requirement_id",
            "role_id",
            "responsibility_id",
            "suite_id",
        ] {
            let mut wrong = status("accepted");
            wrong[key] = json!("different");
            assert!(
                adoption_observation(&wrong, None, &expected()).is_err(),
                "{key}"
            );
        }
        let prior = status("checking");
        for (key, value) in [
            ("assignment_id", json!("replacement-assignment")),
            ("epoch", json!(2)),
            ("created_at", json!("2026-10-22T10:00:01Z")),
            ("deadline", json!("2026-10-22T10:29:59Z")),
        ] {
            let mut changed = status("accepted");
            changed["version"] = json!(9);
            changed[key] = value;
            assert!(
                adoption_observation(&changed, Some(&prior), &expected()).is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn same_version_mutation_lower_version_and_terminal_regression_are_errors() {
        let prior = status("checking");
        let mut same_version = prior.clone();
        same_version["authority"] = json!("different even if observational");
        assert!(adoption_observation(&same_version, Some(&prior), &expected()).is_err());
        let mut older = status("accepted");
        older["version"] = json!(7);
        assert!(adoption_observation(&older, Some(&prior), &expected()).is_err());
        for (before, after) in [
            ("checking", "selected"),
            ("selected", "discovering"),
            ("accepted", "checking"),
            ("accepted", "accepted"),
            ("rejected", "accepted"),
            ("attention", "discovering"),
            ("fenced", "checking"),
        ] {
            let before = status(before);
            let mut after = status(after);
            after["version"] = json!(9);
            assert!(adoption_observation(&after, Some(&before), &expected()).is_err());
        }
        let mut fenced = status("fenced");
        fenced["version"] = json!(9);
        assert!(matches!(
            adoption_observation(&fenced, Some(&status("attention")), &expected()),
            Ok(ObservationDecision::Attention(_))
        ));
    }

    #[test]
    fn selected_artifact_installation_and_exact_batch_cannot_change() {
        let prior = status("checking");
        for field in ["selection", "artifact", "installation_id", "job_ids"] {
            let mut changed = status("accepted");
            changed["version"] = json!(9);
            match field {
                "selection" => changed[field]["publisher"] = json!("Different foreign org"),
                "artifact" => changed[field]["actor_id"] = json!("actor_2"),
                "installation_id" => changed[field] = json!("installation_2"),
                "job_ids" => changed[field] = json!(["job_2", "job_1", "job_3"]),
                _ => unreachable!(),
            }
            assert!(
                adoption_observation(&changed, Some(&prior), &expected()).is_err(),
                "{field}"
            );
        }
        let mut changed = status("accepted");
        changed["version"] = json!(9);
        changed["discovery_attempts"] = json!(2);
        assert!(adoption_observation(&changed, Some(&prior), &expected()).is_err());
    }

    #[test]
    fn a_label_or_reference_without_coherent_independent_evidence_does_not_complete() {
        for (key, value) in [
            ("artifact", Value::Null),
            ("selection", Value::Null),
            ("installation_id", Value::Null),
            ("job_ids", json!([])),
            ("job_ids", json!(["job_1", "job_2"])),
            ("job_ids", json!(["job_1", "job_1", "job_3"])),
            ("acceptance_ref", Value::Null),
            ("acceptance_ref", json!("verification:not-a-digest")),
            (
                "acceptance_ref",
                json!(format!("adoption-rejection:{}", "a".repeat(64))),
            ),
            ("attention", json!("checks_failed")),
            ("fence_reason", json!("lease_expired")),
            (
                "rejection_ref",
                json!(format!("adoption-rejection:{}", "a".repeat(64))),
            ),
            ("version", json!(0)),
            ("version", json!(u64::MAX)),
            ("epoch", json!(-1)),
            ("discovery_attempts", json!(0)),
            ("next_attempt_at", json!("2026-10-22T10:00:11Z")),
        ] {
            let mut invalid = status("accepted");
            invalid[key] = value;
            assert!(
                adoption_observation(&invalid, None, &expected()).is_err(),
                "{key}"
            );
        }
        for (field, value) in [
            ("source_sha256", "invalid"),
            ("build_number", "latest"),
            ("capability_id", "other-capability"),
            ("capability_version", "other-version"),
        ] {
            let mut invalid = status("accepted");
            invalid["artifact"][field] = json!(value);
            assert!(adoption_observation(&invalid, None, &expected()).is_err());
        }
        let mut self_published = status("accepted");
        self_published["selection"]["publisher"] = json!("Org B 東京");
        assert!(adoption_observation(&self_published, None, &expected()).is_err());
        let mut extra_proof = status("accepted");
        extra_proof["raw_output"] = json!({"PRIVATE_ORACLE_ANSWER":"not-a-proof"});
        assert!(adoption_observation(&extra_proof, None, &expected()).is_err());
    }

    #[test]
    fn attention_is_closed_redacted_and_bounded() {
        for phase in ["attention", "fenced", "rejected"] {
            assert!(matches!(
                adoption_observation(&status(phase), None, &expected()),
                Ok(ObservationDecision::Attention(_))
            ));
        }
        let mut unknown = status("attention");
        unknown["attention"] = json!("PRIVATE_PROVIDER_BODY");
        let error = adoption_observation(&unknown, None, &expected()).unwrap_err();
        assert!(!error.contains("PRIVATE"));
        for (field, value) in [
            ("assignment_id", json!("x".repeat(257))),
            ("authority", json!("x".repeat(1025))),
            (
                "job_ids",
                json!((0..129).map(|n| format!("job_{n}")).collect::<Vec<_>>()),
            ),
        ] {
            let mut invalid = status("accepted");
            invalid[field] = value;
            assert!(adoption_observation(&invalid, None, &expected()).is_err());
        }
        let mut missing = status("discovering");
        missing.as_object_mut().unwrap().remove("fence_reason");
        assert!(adoption_observation(&missing, None, &expected()).is_err());
        let mut invalid_due = status("discovering");
        invalid_due["next_attempt_at"] = json!("2026-10-22T10:30:01Z");
        assert!(adoption_observation(&invalid_due, None, &expected()).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "acq_observation-test";

    fn status(phase: &str) -> Value {
        json!({"id":ID,"version":8,"currently_authorized":true,"phase":phase,"attempt":0})
    }
    fn automation(state: &str) -> Value {
        json!({"id":ID,"enabled":true,"current_version":8,"observed_version":8,
            "state":state,"error_code":null})
    }

    #[test]
    fn version_skew_waits_before_using_stale_attention_or_completion() {
        for (phase, state) in [("forging", "attention"), ("verified", "inactive")] {
            for current in [7, 9] {
                let mut scheduler = automation(state);
                scheduler["current_version"] = json!(current);
                scheduler["observed_version"] = json!(current);
                scheduler["error_code"] = json!("quota");
                assert_eq!(
                    acquisition_observation(&status(phase), &scheduler, ID),
                    Ok(ObservationDecision::Wait)
                );
            }
        }
    }

    #[test]
    fn only_a_matching_inactive_verified_acquisition_completes() {
        assert_eq!(
            acquisition_observation(&status("verified"), &automation("inactive"), ID),
            Ok(ObservationDecision::Complete)
        );
        for phase in [
            "generating",
            "forging",
            "rejected",
            "failed",
            "needs_reconciliation",
        ] {
            assert!(matches!(
                acquisition_observation(&status(phase), &automation("inactive"), ID),
                Ok(ObservationDecision::Attention(_))
            ));
        }
        // A scheduler does not attest the artifact, and even a candidate's
        // verified label alone cannot complete a still-active observation.
        for state in ["ready", "waiting"] {
            assert_eq!(
                acquisition_observation(&status("verified"), &automation(state), ID),
                Ok(ObservationDecision::Wait)
            );
        }
    }

    #[test]
    fn an_initial_rejection_waits_for_the_server_owned_repair_decision() {
        for state in ["ready", "waiting"] {
            let mut scheduler = automation(state);
            scheduler["observed_version"] = Value::Null;
            assert_eq!(
                acquisition_observation(&status("rejected"), &scheduler, ID),
                Ok(ObservationDecision::Wait)
            );
        }
        assert!(matches!(
            acquisition_observation(&status("rejected"), &automation("inactive"), ID),
            Ok(ObservationDecision::Attention(_))
        ));
    }

    #[test]
    fn attention_uses_only_closed_static_reasons_and_never_claims_a_proof() {
        for code in [
            "retry_read",
            "quota",
            "configuration",
            "invalid_record",
            "conflict",
            "internal",
        ] {
            let mut scheduler = automation("attention");
            scheduler["error_code"] = json!(code);
            scheduler["message"] = json!("PRIVATE_PROVIDER_OR_ORACLE_CANARY");
            let decision = acquisition_observation(&status("verified"), &scheduler, ID).unwrap();
            let ObservationDecision::Attention(reason) = decision else {
                panic!("attention is not completion");
            };
            assert!(!reason.contains("CANARY"));
        }
        for code in [
            Value::Null,
            json!(9),
            json!("PRIVATE_PROVIDER_OR_ORACLE_CANARY"),
        ] {
            let mut scheduler = automation("attention");
            scheduler["error_code"] = code;
            let error = acquisition_observation(&status("forging"), &scheduler, ID).unwrap_err();
            assert!(!error.contains("CANARY"));
        }
        let mut scheduler = automation("attention");
        scheduler["error_code"] = json!("quota");
        for observed in [Value::Null, json!(7)] {
            scheduler["observed_version"] = observed;
            assert!(acquisition_observation(&status("forging"), &scheduler, ID).is_err());
        }
    }

    #[test]
    fn disabled_automation_or_lost_authority_stops_even_during_version_skew() {
        let mut scheduler = automation("ready");
        scheduler["enabled"] = json!(false);
        scheduler["current_version"] = json!(9);
        assert!(matches!(
            acquisition_observation(&status("forging"), &scheduler, ID),
            Ok(ObservationDecision::Attention(_))
        ));
        scheduler["enabled"] = json!(true);
        let mut acquisition = status("verified");
        acquisition["currently_authorized"] = json!(false);
        assert!(matches!(
            acquisition_observation(&acquisition, &scheduler, ID),
            Ok(ObservationDecision::Attention(_))
        ));
        assert!(matches!(
            acquisition_observation(&status("fenced"), &automation("inactive"), ID),
            Ok(ObservationDecision::Attention(_))
        ));
        assert!(matches!(
            acquisition_observation(&status("forging"), &automation("disabled"), ID),
            Ok(ObservationDecision::Attention(_))
        ));
    }

    #[test]
    fn wrong_ids_untyped_versions_and_missing_fields_fail_closed() {
        let mut acquisition = status("verified");
        acquisition["id"] = json!("another-acquisition");
        assert!(acquisition_observation(&acquisition, &automation("inactive"), ID).is_err());
        let mut scheduler = automation("inactive");
        scheduler["id"] = json!("another-acquisition");
        assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
        for version in [Value::Null, json!(0), json!(-1), json!(1.5), json!("8")] {
            let mut acquisition = status("verified");
            acquisition["version"] = version.clone();
            assert!(acquisition_observation(&acquisition, &automation("inactive"), ID).is_err());
            let mut scheduler = automation("inactive");
            scheduler["current_version"] = version;
            assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
        }
        for key in ["enabled", "current_version", "observed_version", "state"] {
            let mut scheduler = automation("inactive");
            scheduler.as_object_mut().unwrap().remove(key);
            assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
        }
        for key in ["currently_authorized", "version", "phase"] {
            let mut acquisition = status("verified");
            acquisition.as_object_mut().unwrap().remove(key);
            assert!(acquisition_observation(&acquisition, &automation("inactive"), ID).is_err());
        }
        let mut scheduler = automation("inactive");
        scheduler["observed_version"] = json!(9);
        assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
    }
}
