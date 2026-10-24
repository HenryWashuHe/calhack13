//! Bounded replacement selection over backend-frozen venue facts.
//!
//! This verifier authenticates neither sources nor the Fetch sender. The caller
//! must reconstruct an immutable native task receipt, resolve each invocation
//! through the private verifier, and check current authority before effects.
//! A valid `selected: null` means no supplied candidate meets this policy; it
//! does not establish that no replacement exists outside the supplied snapshots.

use crate::venue_assessment_domain::{select_venue, validate_policy, VenueAssessmentPolicy};
use chrono::{DateTime, FixedOffset};
use genesis_core::ids::JobId;
use genesis_verification::{
    decode_json, json_sha256, Availability, AvailabilityWindow, EvidenceRef, ExtractionOutcome,
    Venue,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Write};

pub const TASK_KIND: &str = "venue_replacement_v1";
pub const REQUIRED_CAPABILITY: &str = "venue.select";
pub const MAX_CANDIDATES: usize = 16;
pub const MAX_SOURCE_JOBS: usize = 8;
pub const MAX_WIRE_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCandidate {
    pub source_job_id: JobId,
    pub invocation_proof_ref: String,
    pub venue: Venue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryContext {
    pub recovery_id: String,
    pub policy_sha256: String,
    /// The original selected venue is excluded even if a candidate says it fits.
    pub policy: VenueAssessmentPolicy,
    pub candidates: Vec<RecoveryCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateKey {
    pub source_job_id: JobId,
    pub venue_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryChecks {
    pub capacity: bool,
    pub price: bool,
    pub window: bool,
    pub availability: bool,
    pub step_free: bool,
}

impl RecoveryChecks {
    fn all(&self) -> bool {
        self.capacity && self.price && self.window && self.availability && self.step_free
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateEvaluation {
    pub source_job_id: JobId,
    pub venue_id: String,
    pub invocation_proof_ref: String,
    pub evidence: EvidenceRef,
    pub checks: RecoveryChecks,
    pub eligible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryOutput {
    pub schema_version: u32,
    pub recovery_id: String,
    pub policy_sha256: String,
    pub context_sha256: String,
    /// Exactly one evaluation per candidate, in the original input order.
    pub evaluations: Vec<CandidateEvaluation>,
    // A missing field is not an explicit no-match decision.
    #[serde(deserialize_with = "required_option")]
    pub selected: Option<CandidateKey>,
}

fn required_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CandidateKey>, D::Error> {
    Option::<CandidateKey>::deserialize(deserializer)
}

/// A private, serialize-only semantic result. It grants no dispatch authority.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct VerifiedRecoverySelection {
    output: RecoveryOutput,
    output_sha256: String,
}

impl VerifiedRecoverySelection {
    pub fn output(&self) -> &RecoveryOutput {
        &self.output
    }
    pub fn selected(&self) -> Option<&CandidateKey> {
        self.output.selected.as_ref()
    }
    pub fn context_sha256(&self) -> &str {
        &self.output.context_sha256
    }
    pub fn output_sha256(&self) -> &str {
        &self.output_sha256
    }
}

impl fmt::Debug for VerifiedRecoverySelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedRecoverySelection")
            .field("context_sha256", &self.context_sha256())
            .field("output_sha256", &self.output_sha256)
            .field("output", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryError {
    #[error("invalid replacement selection context")]
    InvalidContext,
    #[error("replacement selection input exceeds 32 KiB")]
    InputTooLarge,
    #[error("invalid replacement selection output")]
    InvalidOutput,
    #[error("replacement selection output exceeds 32 KiB")]
    OutputTooLarge,
    #[error("replacement selection binding mismatch")]
    BindingMismatch,
    #[error("replacement evaluations do not match the frozen facts")]
    EvaluationMismatch,
    #[error("replacement selection does not match the deterministic policy")]
    SelectionMismatch,
}

fn text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn recovery_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
}

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Common Rust/Python RFC3339 subset: four-digit nonzero year, seconds, up to
/// nine fractional digits, explicit timezone, and no leap seconds. Comparing
/// chrono instants preserves nanoseconds and accepts equivalent UTC offsets.
fn timestamp(value: &str) -> Option<DateTime<FixedOffset>> {
    let bytes = value.as_bytes();
    if !(20..=64).contains(&bytes.len())
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    for range in [0..4, 5..7, 8..10, 11..13, 14..16, 17..19] {
        if !bytes[range].iter().all(u8::is_ascii_digit) {
            return None;
        }
    }
    if &bytes[0..4] == b"0000"
        || &bytes[11..13] > b"23".as_slice()
        || &bytes[14..16] > b"59".as_slice()
        || &bytes[17..19] > b"59".as_slice()
    {
        return None;
    }
    let mut zone = 19;
    if bytes[zone] == b'.' {
        zone += 1;
        let start = zone;
        while bytes.get(zone).is_some_and(u8::is_ascii_digit) {
            zone += 1;
        }
        if !(1..=9).contains(&(zone - start)) {
            return None;
        }
    }
    match bytes.get(zone..) {
        Some([b'Z' | b'z']) => {}
        Some([b'+' | b'-', h1, h2, b':', m1, m2])
            if [h1, h2, m1, m2].iter().all(|byte| byte.is_ascii_digit())
                && [*h1, *h2] <= *b"23"
                && [*m1, *m2] <= *b"59" => {}
        _ => return None,
    }
    DateTime::parse_from_rfc3339(value).ok()
}

fn interval(value: &AvailabilityWindow) -> Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let start = timestamp(&value.start)?;
    let end = timestamp(&value.end)?;
    (start < end).then_some((start, end))
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_WIRE_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("bounded JSON exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encoded<T: Serialize>(value: &T, error: RecoveryError) -> Result<Vec<u8>, RecoveryError> {
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(&mut bytes, value).map_err(|_| error)?;
    Ok(bytes.0)
}

fn output_depth_valid(value: &Value) -> bool {
    let mut pending = vec![(value, 0)];
    let mut visited = 0;
    while let Some((value, depth)) = pending.pop() {
        visited += 1;
        if depth > 32 || visited > MAX_WIRE_BYTES {
            return false;
        }
        match value {
            Value::Array(items) => {
                if items.len() > MAX_WIRE_BYTES.saturating_sub(visited + pending.len()) {
                    return false;
                }
                pending.extend(items.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(items) => {
                if items.len() > MAX_WIRE_BYTES.saturating_sub(visited + pending.len()) {
                    return false;
                }
                pending.extend(items.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    true
}

fn fields_valid(context: &RecoveryContext) -> Result<(), RecoveryError> {
    let invalid = RecoveryError::InvalidContext;
    if !recovery_id(&context.recovery_id)
        || !hash(&context.policy_sha256)
        || validate_policy(&context.policy).is_err()
        || interval(&context.policy.window).is_none()
        || !(1..=MAX_CANDIDATES).contains(&context.candidates.len())
    {
        return Err(invalid);
    }
    let mut venues = BTreeSet::new();
    let mut sources = BTreeMap::new();
    for candidate in &context.candidates {
        let evidence = &candidate.venue.evidence;
        let mut selected_policy = context.policy.clone();
        selected_policy.selected_venue_id = candidate.venue.venue_id.clone();
        if !text(candidate.source_job_id.as_str(), 256)
            || !candidate
                .invocation_proof_ref
                .strip_prefix("invocation:")
                .is_some_and(hash)
            || !venues.insert(&candidate.venue.venue_id)
            || interval(&candidate.venue.window).is_none()
            || select_venue(
                &ExtractionOutcome::Supported {
                    venues: vec![candidate.venue.clone()],
                },
                &selected_policy,
            )
            .is_err()
        {
            return Err(invalid);
        }
        let source = (
            &candidate.invocation_proof_ref,
            &evidence.source_url,
            &evidence.snapshot_sha256,
        );
        if sources
            .insert(&candidate.source_job_id, source)
            .is_some_and(|previous| previous != source)
            || sources.len() > MAX_SOURCE_JOBS
        {
            return Err(invalid);
        }
    }
    // All metadata is bounded before hashing or serializing a prompt.
    if context.policy_sha256 != json_sha256(&json!(context.policy)) {
        return Err(invalid);
    }
    Ok(())
}

pub fn validate_context(context: &RecoveryContext) -> Result<(), RecoveryError> {
    provider_input(context).map(|_| ())
}

/// Exact specialist contract. Facts and policy only: no oracle, computed
/// evaluation, ranking, selected venue or expected result is sent.
pub fn provider_input(context: &RecoveryContext) -> Result<Value, RecoveryError> {
    fields_valid(context)?;
    let bytes = encoded(context, RecoveryError::InputTooLarge)?;
    let context: Value =
        serde_json::from_slice(&bytes).map_err(|_| RecoveryError::InvalidContext)?;
    let input = json!({
        "schema_version": 1,
        "kind": TASK_KIND,
        "context_sha256": json_sha256(&context),
        "context": context,
    });
    encoded(&input, RecoveryError::InputTooLarge)?;
    Ok(input)
}

fn evaluations(context: &RecoveryContext) -> Result<Vec<CandidateEvaluation>, RecoveryError> {
    let policy = &context.policy;
    let (wanted_start, wanted_end) =
        interval(&policy.window).ok_or(RecoveryError::InvalidContext)?;
    context
        .candidates
        .iter()
        .map(|candidate| {
            let venue = &candidate.venue;
            let (offered_start, offered_end) =
                interval(&venue.window).ok_or(RecoveryError::InvalidContext)?;
            let checks = RecoveryChecks {
                capacity: venue.capacity >= policy.attendees,
                price: venue.price.currency == policy.budget.currency
                    && venue.price.minor_units <= policy.budget.minor_units,
                window: offered_start <= wanted_start && offered_end >= wanted_end,
                availability: venue.availability == Availability::Available,
                step_free: !policy.step_free_required || venue.step_free,
            };
            Ok(CandidateEvaluation {
                source_job_id: candidate.source_job_id.clone(),
                venue_id: venue.venue_id.clone(),
                invocation_proof_ref: candidate.invocation_proof_ref.clone(),
                evidence: venue.evidence.clone(),
                eligible: checks.all() && venue.venue_id != policy.selected_venue_id,
                checks,
            })
        })
        .collect()
}

/// Validate the actual immutable Fetch task receipt's output. A caller must not
/// substitute generic job.result, a copied HTTP body, or an unauthenticated reply.
pub fn verify_output(
    context: &RecoveryContext,
    output: &Value,
) -> Result<VerifiedRecoverySelection, RecoveryError> {
    let input = provider_input(context)?;
    if !output_depth_valid(output) {
        return Err(RecoveryError::InvalidOutput);
    }
    let bytes = encoded(output, RecoveryError::OutputTooLarge)?;
    let parsed: RecoveryOutput = decode_json(&bytes).map_err(|_| RecoveryError::InvalidOutput)?;
    if parsed.schema_version != 1
        || parsed.recovery_id != context.recovery_id
        || parsed.policy_sha256 != context.policy_sha256
        || input["context_sha256"].as_str() != Some(parsed.context_sha256.as_str())
    {
        return Err(RecoveryError::BindingMismatch);
    }
    let expected = evaluations(context)?;
    if parsed.evaluations != expected {
        return Err(RecoveryError::EvaluationMismatch);
    }
    let selected = context
        .candidates
        .iter()
        .zip(&expected)
        .filter(|(_, evaluation)| evaluation.eligible)
        .map(|(candidate, _)| candidate)
        .min_by(|left, right| {
            (
                left.venue.price.minor_units,
                left.venue.venue_id.as_bytes(),
                left.source_job_id.as_str().as_bytes(),
            )
                .cmp(&(
                    right.venue.price.minor_units,
                    right.venue.venue_id.as_bytes(),
                    right.source_job_id.as_str().as_bytes(),
                ))
        })
        .map(|candidate| CandidateKey {
            source_job_id: candidate.source_job_id.clone(),
            venue_id: candidate.venue.venue_id.clone(),
        });
    if parsed.selected != selected {
        return Err(RecoveryError::SelectionMismatch);
    }
    Ok(VerifiedRecoverySelection {
        output: parsed,
        output_sha256: json_sha256(output),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector() -> (RecoveryContext, Value, Value) {
        let value: Value = decode_json(include_bytes!(
            "../../../contracts/fixtures/fetch_venue_replacement_v1.json"
        ))
        .unwrap();
        (
            serde_json::from_value(value["input"]["context"].clone()).unwrap(),
            value["input"].clone(),
            value["output"].clone(),
        )
    }

    fn rebind(context: &RecoveryContext, output: &mut Value) {
        let input = provider_input(context).unwrap();
        output["context_sha256"] = input["context_sha256"].clone();
    }

    #[test]
    fn frozen_cross_language_vector_checks_real_filtering_and_utf8_ranking() {
        let (context, input, output) = vector();
        assert_eq!(provider_input(&context).unwrap(), input);
        let verified = verify_output(&context, &output).unwrap();
        assert_eq!(verified.selected().unwrap().venue_id, "alpha-hall");
        assert_eq!(verified.output_sha256(), json_sha256(&output));
        assert!(verified.output().evaluations[0].checks.all());
        assert!(!verified.output().evaluations[0].eligible);
        assert!(!verified.output().evaluations[7].checks.window);
        // No answer list or selected answer appears in the provider contract.
        assert_eq!(input.as_object().unwrap().len(), 4);
        assert!(input.get("evaluations").is_none());
        assert!(input.get("selected").is_none());
        let debug = format!("{verified:?}");
        assert!(!debug.contains("alpha-hall"));
        assert!(!debug.contains("replacements.html"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn every_predicate_and_eligibility_must_be_recomputed() {
        let (context, _, output) = vector();
        for field in ["capacity", "price", "window", "availability", "step_free"] {
            let mut changed = output.clone();
            changed["evaluations"][2]["checks"][field] = json!(false);
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::EvaluationMismatch),
                "{field}"
            );
        }
        let mut changed = output;
        changed["evaluations"][0]["eligible"] = json!(true);
        assert_eq!(
            verify_output(&context, &changed),
            Err(RecoveryError::EvaluationMismatch)
        );
    }

    #[test]
    fn source_citations_are_exact_and_cannot_be_substituted() {
        let (context, _, output) = vector();
        for field in ["source_job_id", "venue_id", "invocation_proof_ref"] {
            let mut changed = output.clone();
            changed["evaluations"][2][field] = json!("foreign");
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::EvaluationMismatch)
            );
        }
        for field in ["source_url", "snapshot_sha256", "locator"] {
            let mut changed = output.clone();
            changed["evaluations"][2]["evidence"][field] = json!("foreign");
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::EvaluationMismatch)
            );
        }
    }

    #[test]
    fn exact_context_and_policy_binding_is_required() {
        let (context, _, output) = vector();
        for field in ["recovery_id", "policy_sha256", "context_sha256"] {
            let mut changed = output.clone();
            changed[field] = json!("different");
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::BindingMismatch)
            );
        }
        let mut changed = output;
        changed["schema_version"] = json!(2);
        assert_eq!(
            verify_output(&context, &changed),
            Err(RecoveryError::BindingMismatch)
        );
    }

    #[test]
    fn no_match_requires_complete_correct_negative_evidence() {
        let (mut context, _, mut output) = vector();
        let retained = [0, 4, 5, 6, 7, 8, 9];
        context.candidates = retained
            .iter()
            .map(|index| context.candidates[*index].clone())
            .collect();
        output["evaluations"] = Value::Array(
            retained
                .iter()
                .map(|index| output["evaluations"][*index].clone())
                .collect(),
        );
        output["selected"] = Value::Null;
        rebind(&context, &mut output);
        assert!(verify_output(&context, &output)
            .unwrap()
            .selected()
            .is_none());
        output["selected"] = json!({"source_job_id":"job_replacements","venue_id":"grand-hall"});
        assert_eq!(
            verify_output(&context, &output),
            Err(RecoveryError::SelectionMismatch)
        );
    }

    #[test]
    fn a_feasible_but_nonminimal_selection_or_false_no_match_is_rejected() {
        let (context, _, output) = vector();
        for selected in [
            json!({"source_job_id":"job_replacements","venue_id":"zeta-hall"}),
            json!({"source_job_id":"job_replacements","venue_id":"éclair"}),
            json!({"source_job_id":"foreign_job","venue_id":"alpha-hall"}),
            Value::Null,
        ] {
            let mut changed = output.clone();
            changed["selected"] = selected;
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::SelectionMismatch)
            );
        }
    }

    #[test]
    fn evaluations_cannot_be_missing_duplicated_or_reordered() {
        let (context, _, output) = vector();
        let mut missing = output.clone();
        missing["evaluations"].as_array_mut().unwrap().pop();
        let mut duplicate = output.clone();
        duplicate["evaluations"][1] = duplicate["evaluations"][2].clone();
        let mut reordered = output;
        reordered["evaluations"].as_array_mut().unwrap().swap(1, 2);
        for changed in [missing, duplicate, reordered] {
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::EvaluationMismatch)
            );
        }
    }

    #[test]
    fn exact_schema_is_required_at_every_output_level() {
        let (context, _, output) = vector();
        for pointer in [
            "",
            "/evaluations/0",
            "/evaluations/0/evidence",
            "/evaluations/0/checks",
            "/selected",
        ] {
            let mut changed = output.clone();
            changed
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("extra".into(), json!(true));
            assert_eq!(
                verify_output(&context, &changed),
                Err(RecoveryError::InvalidOutput),
                "{pointer}"
            );
        }
        let mut missing = output.clone();
        missing.as_object_mut().unwrap().remove("selected");
        assert_eq!(
            verify_output(&context, &missing),
            Err(RecoveryError::InvalidOutput)
        );
        let mut wrong_type = output;
        wrong_type["evaluations"][0]["checks"]["capacity"] = json!(1);
        for invalid in [wrong_type, Value::Null, json!("native prose"), json!({})] {
            assert_eq!(
                verify_output(&context, &invalid),
                Err(RecoveryError::InvalidOutput)
            );
        }
    }

    #[test]
    fn invalid_or_conflicting_candidate_sets_fail_before_prompt_creation() {
        let (original, _, _) = vector();
        type Mutation = fn(&mut RecoveryContext);
        let mutations: Vec<Mutation> = vec![
            |context| context.candidates.clear(),
            |context| context.candidates.push(context.candidates[0].clone()),
            |context| context.policy.attendees = 0,
            |context| context.policy.attendees = 1_000_001,
            |context| context.policy.budget.minor_units = 1_000_000_000_001,
            |context| context.policy_sha256 = "A".repeat(64),
            |context| context.recovery_id = "bad id".into(),
            |context| context.candidates[0].source_job_id = JobId::new(" bad"),
            |context| context.candidates[0].venue.venue_id = "bad\nvenue".into(),
            |context| context.candidates[0].venue.evidence.locator = "x".repeat(1025),
            |context| {
                context.candidates[0].venue.evidence.source_url =
                    "https://user:pass@example.com/".into()
            },
            |context| {
                context.candidates[0].invocation_proof_ref =
                    format!("invocation:{}", "c".repeat(64))
            },
            |context| context.candidates[0].venue.evidence.snapshot_sha256 = "c".repeat(64),
            |context| {
                context.candidates[0].venue.evidence.source_url =
                    "https://different.example/source".into()
            },
        ];
        for mutate in mutations {
            let mut context = original.clone();
            mutate(&mut context);
            assert_eq!(
                validate_context(&context),
                Err(RecoveryError::InvalidContext)
            );
        }
        let mut too_many_jobs = original;
        for (index, candidate) in too_many_jobs.candidates.iter_mut().enumerate() {
            candidate.source_job_id = JobId::new(format!("job_{index}"));
        }
        assert_eq!(
            validate_context(&too_many_jobs),
            Err(RecoveryError::InvalidContext)
        );
    }

    #[test]
    fn timestamps_have_identical_nanosecond_and_offset_semantics() {
        let (mut context, _, mut output) = vector();
        // One nanosecond excludes the previous best venue; the next UTF8 key wins.
        context.candidates[2].venue.window.start = "2026-11-15T02:00:00.000000002Z".into();
        output["evaluations"][2]["checks"]["window"] = json!(false);
        output["evaluations"][2]["eligible"] = json!(false);
        output["selected"]["venue_id"] = json!("zeta-hall");
        rebind(&context, &mut output);
        assert!(verify_output(&context, &output).is_ok());
        for invalid in [
            "0000-11-15T02:00:00Z",
            "2026-11-15 02:00:00Z",
            "2026-11-15T02:00Z",
            "2026-11-15T02:00:60Z",
            "2026-11-15T02:00:00.0000000001Z",
            "2026-11-15T02:00:00+24:00",
            "2026-02-30T02:00:00Z",
            "2026-11-15T02:00:00",
        ] {
            let mut invalid_context = context.clone();
            invalid_context.candidates[0].venue.window.start = invalid.into();
            assert_eq!(
                validate_context(&invalid_context),
                Err(RecoveryError::InvalidContext),
                "{invalid}"
            );
        }
    }

    #[test]
    fn unsigned_integer_facts_do_not_round_or_overflow() {
        let (mut context, _, mut output) = vector();
        context.candidates[1].venue.capacity = u32::MAX;
        context.candidates[1].venue.price.minor_units = u64::MAX;
        output["evaluations"][1]["checks"]["price"] = json!(false);
        output["evaluations"][1]["eligible"] = json!(false);
        rebind(&context, &mut output);
        assert!(verify_output(&context, &output).is_ok());
    }

    #[test]
    fn serialized_utf8_wire_size_and_arbitrary_output_depth_are_bounded() {
        let (mut context, _, output) = vector();
        let candidate = context.candidates[0].clone();
        context.candidates = (0..16)
            .map(|index| {
                let mut candidate = candidate.clone();
                candidate.venue.venue_id = format!("venue_{index}");
                candidate.venue.evidence.source_url =
                    format!("https://example.com/{}", "a".repeat(990));
                candidate.venue.evidence.locator = "é".repeat(500);
                candidate
            })
            .collect();
        assert_eq!(provider_input(&context), Err(RecoveryError::InputTooLarge));
        let (context, _, _) = vector();
        let mut oversized = output;
        oversized["extra"] = json!("é".repeat(MAX_WIRE_BYTES / 2));
        assert_eq!(
            verify_output(&context, &oversized),
            Err(RecoveryError::OutputTooLarge)
        );
        let mut deep = Value::Null;
        for _ in 0..40 {
            deep = json!([deep]);
        }
        assert_eq!(
            verify_output(&context, &deep),
            Err(RecoveryError::InvalidOutput)
        );
    }

    #[test]
    fn errors_never_echo_provider_or_source_data() {
        let (context, _, mut output) = vector();
        let private = "PRIVATE_NATIVE_OR_SOURCE_CANARY";
        output["evaluations"][0]["evidence"]["locator"] = json!(private);
        let error = verify_output(&context, &output).unwrap_err();
        assert!(!error.to_string().contains(private));
        assert!(!format!("{error:?}").contains(private));
        assert!(!serde_json::to_string(&error).unwrap().contains(private));
    }
}
