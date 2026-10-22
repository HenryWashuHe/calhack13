//! Trusted, offline materialization of committed capability gaps.
//!
//! A rule supplies public synthesis examples and selects an independently
//! configured suite by ID. This module never reads that suite, fetches a page,
//! or accepts examples from the gap. Materialization is not authorization: the
//! driver must still check the frozen gap and current organization authority.
use crate::capability_acquisition_store::CreateAcquisition;
use genesis_core::{digest::canonical_sha256, ids::OrgId, model::CapabilityGap};
use genesis_forge::{CandidateIdentity, PublicExample, SynthesisRequest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

const MAX_RULES: usize = 32;
const MAX_POLICY_BYTES: usize = 4 * 1024 * 1024;
// 29 + separator + 32 hex digits + the two-byte attempt suffix = 64.
const MAX_ACTOR_PREFIX_BYTES: usize = 29;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapAcquisitionPolicy {
    pub rules: Vec<GapAcquisitionRule>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapAcquisitionRule {
    pub capability_id: String,
    pub suite_id: String,
    pub actor_name_prefix: String,
    pub allowed_source_urls: Vec<String>,
    pub examples: Vec<PublicExample>,
    pub repair: bool,
}

/// Closed, persistable reasons. Error formatting never includes policy HTML,
/// a source URL, or planner-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum GapPolicyError {
    #[error("invalid trusted gap acquisition policy")]
    InvalidPolicy,
    #[error("invalid committed capability gap")]
    InvalidGap,
    #[error("no trusted rule for the requested capability")]
    NoMatchingRule,
    #[error("requested source is outside the trusted rule")]
    SourceNotAllowed,
    #[error("no fixed public example matches the requested sources")]
    NoMatchingExample,
}

impl GapAcquisitionPolicy {
    pub fn validate(&self) -> Result<(), GapPolicyError> {
        if self.rules.is_empty() || self.rules.len() > MAX_RULES {
            return Err(GapPolicyError::InvalidPolicy);
        }
        let mut capabilities = BTreeSet::new();
        for rule in &self.rules {
            if !capabilities.insert(&rule.capability_id)
                || !label(&rule.suite_id, 256)
                || rule.actor_name_prefix.is_empty()
                || rule.actor_name_prefix.len() > MAX_ACTOR_PREFIX_BYTES
                || rule.actor_name_prefix.starts_with('-')
                || rule.actor_name_prefix.ends_with('-')
                || !rule
                    .actor_name_prefix
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            {
                return Err(GapPolicyError::InvalidPolicy);
            }
            // SynthesisRequest delegates candidate identity validation to the
            // exact CandidateSpec rules, and also bounds URLs, HTML and JSON.
            for attempt in 0..=u8::from(rule.repair) {
                public_request(
                    candidate(rule, &"0".repeat(64), attempt),
                    rule.allowed_source_urls.clone(),
                    rule.examples.clone(),
                )
                .validate()
                .map_err(|_| GapPolicyError::InvalidPolicy)?;
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| GapPolicyError::InvalidPolicy)?
            .len()
            > MAX_POLICY_BYTES
        {
            return Err(GapPolicyError::InvalidPolicy);
        }
        Ok(())
    }

    /// Canonical across JSON object-key order and serde feature selection.
    /// Array order remains part of this exact trusted policy snapshot.
    pub fn digest(&self) -> Result<String, GapPolicyError> {
        self.validate()?;
        Ok(canonical_sha256(&json!({
            "format": "genesis-gap-acquisition-policy-v1",
            "policy": self,
        })))
    }

    /// Build an idempotent intent from the exact frozen IDs. Restart/recovery
    /// can materialize the same intent after the gap leaves `open`; lifecycle
    /// eligibility and authority remain the driver's responsibility.
    pub fn materialize(
        &self,
        org_id: &OrgId,
        gap: &CapabilityGap,
    ) -> Result<CreateAcquisition, GapPolicyError> {
        self.validate()?;
        if !label(org_id.as_str(), 256)
            || !label(gap.gap_id.as_str(), 128)
            || [
                gap.mission_id.as_str(),
                gap.requirement_id.as_str(),
                gap.role_id.as_str(),
                gap.responsibility_id.as_str(),
                gap.capability_id.as_str(),
            ]
            .iter()
            .any(|id| !label(id, 256))
            || !(1..=100).contains(&gap.observed_sources.len())
        {
            return Err(GapPolicyError::InvalidGap);
        }
        let rule = self
            .rules
            .iter()
            .find(|rule| rule.capability_id == gap.capability_id.as_str())
            .ok_or(GapPolicyError::NoMatchingRule)?;
        let requested: BTreeSet<_> = gap.observed_sources.iter().cloned().collect();
        if requested.len() != gap.observed_sources.len() {
            return Err(GapPolicyError::InvalidGap);
        }
        if requested
            .iter()
            .any(|url| !rule.allowed_source_urls.contains(url))
        {
            return Err(GapPolicyError::SourceNotAllowed);
        }
        let examples: Vec<_> = rule
            .examples
            .iter()
            .filter(|example| requested.contains(&example.url))
            .cloned()
            .collect();
        if examples.is_empty() {
            return Err(GapPolicyError::NoMatchingExample);
        }
        let identity = canonical_sha256(&json!({
            "format": "genesis-gap-candidate-v1",
            "org_id": org_id,
            "gap_id": gap.gap_id,
        }));
        let initial = candidate(rule, &identity, 0);
        let repair = rule.repair.then(|| candidate(rule, &identity, 1));
        let allowed_source_urls: Vec<_> = requested.into_iter().collect();
        // Validate the narrowed input as well. There is no requirement text
        // from the gap here: the existing acquisition store loads the current
        // authoritative requirement under its transaction.
        for identity in std::iter::once(&initial).chain(repair.iter()) {
            public_request(
                identity.clone(),
                allowed_source_urls.clone(),
                examples.clone(),
            )
            .validate()
            .map_err(|_| GapPolicyError::InvalidGap)?;
        }
        Ok(CreateAcquisition {
            client_request_id: gap.gap_id.as_str().into(),
            mission_id: gap.mission_id.clone(),
            requirement_id: gap.requirement_id.clone(),
            role_id: gap.role_id.clone(),
            responsibility_id: gap.responsibility_id.clone(),
            suite_id: rule.suite_id.clone(),
            candidate: initial,
            repair,
            allowed_source_urls,
            examples,
            deadline: None,
        })
    }
}

fn label(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn candidate(rule: &GapAcquisitionRule, identity: &str, attempt: u8) -> CandidateIdentity {
    // Source versions are scoped to an Actor and have only 10,000 valid
    // values. Deterministic even/odd pairs distinguish primary from repair;
    // the full hash in the capability version and 128 bits in the Actor name
    // distinguish independent org/gap operations.
    let slot = identity.as_bytes()[..8]
        .iter()
        .fold(0u32, |n, byte| (n * 16 + hex_digit(*byte)) % 5000);
    CandidateIdentity {
        capability_id: rule.capability_id.clone(),
        capability_version: format!("0.0.1-{identity}.{attempt}"),
        actor_name: format!("{}-{}-{attempt}", rule.actor_name_prefix, &identity[..32]),
        source_version: format!("{}.{}", slot / 50, (slot % 50) * 2 + u32::from(attempt)),
    }
}

fn hex_digit(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => u32::from(byte - b'0'),
        b'a'..=b'f' => u32::from(byte - b'a' + 10),
        _ => unreachable!("candidate identity is generated SHA-256 hex"),
    }
}

fn public_request(
    candidate: CandidateIdentity,
    allowed_source_urls: Vec<String>,
    examples: Vec<PublicExample>,
) -> SynthesisRequest {
    SynthesisRequest {
        operation_id: "gap-policy-validation".into(),
        candidate,
        requirement: "Validate trusted public synthesis inputs".into(),
        allowed_source_urls,
        examples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::{ids::*, model::CapabilityGapStatus};

    fn policy() -> GapAcquisitionPolicy {
        GapAcquisitionPolicy {
            rules: vec![GapAcquisitionRule {
                capability_id: "venue-reader".into(),
                suite_id: "trusted-suite".into(),
                actor_name_prefix: "genesis-venue".into(),
                allowed_source_urls: vec![
                    "https://venues.example/a".into(),
                    "https://venues.example/b".into(),
                    "https://venues.example/c".into(),
                ],
                examples: vec![
                    PublicExample {
                        url: "https://venues.example/a".into(),
                        html: "<html>fixed public A</html>".into(),
                    },
                    PublicExample {
                        url: "https://venues.example/b".into(),
                        html: "<html>fixed public B</html>".into(),
                    },
                ],
                repair: true,
            }],
        }
    }

    fn gap() -> CapabilityGap {
        CapabilityGap {
            gap_id: CapabilityGapId::new("gap-a"),
            mission_id: MissionId::new("mission-a"),
            requirement_id: RequirementId::new("requirement-a"),
            requirement_digest: "1".repeat(64),
            triggering_evidence: vec![ObservationId::new("observation-a")],
            role_id: RoleId::new("role-a"),
            responsibility_id: ResponsibilityId::new("responsibility-a"),
            capability_id: CapabilityId::new("venue-reader"),
            purpose: "Read the changed venue page".into(),
            observed_sources: vec!["https://venues.example/a".into()],
            status: CapabilityGapStatus::Open,
            opened_in_revision: 2,
        }
    }

    #[test]
    fn rejects_unknown_fields_at_every_policy_level() {
        for path in [0, 1, 2] {
            let mut value = serde_json::to_value(policy()).unwrap();
            let object = match path {
                0 => &mut value,
                1 => &mut value["rules"][0],
                _ => &mut value["rules"][0]["examples"][0],
            };
            object["oracle"] = json!({"expected": "never accepted"});
            assert!(serde_json::from_value::<GapAcquisitionPolicy>(value).is_err());
        }
    }

    #[test]
    fn rejects_duplicate_and_unbounded_rules() {
        let mut duplicate = policy();
        duplicate.rules.push(duplicate.rules[0].clone());
        assert_eq!(duplicate.validate(), Err(GapPolicyError::InvalidPolicy));
        assert_eq!(
            GapAcquisitionPolicy { rules: vec![] }.validate(),
            Err(GapPolicyError::InvalidPolicy)
        );
        let mut too_many = policy();
        too_many.rules = (0..=MAX_RULES)
            .map(|i| {
                let mut rule = policy().rules.remove(0);
                rule.capability_id = format!("cap-{i}");
                rule
            })
            .collect();
        assert_eq!(too_many.validate(), Err(GapPolicyError::InvalidPolicy));
    }

    #[test]
    fn existing_synthesis_contract_rejects_invalid_rule_inputs() {
        let mutations: Vec<fn(&mut GapAcquisitionRule)> = vec![
            |r| r.suite_id = " ".into(),
            |r| r.capability_id = "\ninvalid".into(),
            |r| r.actor_name_prefix = "Uppercase".into(),
            |r| r.actor_name_prefix = "a".repeat(MAX_ACTOR_PREFIX_BYTES + 1),
            |r| r.allowed_source_urls[0] = "http://venues.example/a".into(),
            |r| r.allowed_source_urls.push(r.allowed_source_urls[0].clone()),
            |r| r.examples[0].url = "https://foreign.example/".into(),
            |r| r.examples[0].html = " ".into(),
            |r| r.examples[0].html = "x".repeat(128 * 1024 + 1),
            |r| r.examples.clear(),
        ];
        for mutate in mutations {
            let mut invalid = policy();
            mutate(&mut invalid.rules[0]);
            assert_eq!(invalid.validate(), Err(GapPolicyError::InvalidPolicy));
        }
    }

    #[test]
    fn bounds_total_policy_size_even_when_each_rule_is_valid() {
        let mut large = policy();
        large.rules = (0..32)
            .map(|i| {
                let mut rule = policy().rules.remove(0);
                rule.capability_id = format!("cap-{i}");
                for example in &mut rule.examples {
                    example.html = "x".repeat(128 * 1024);
                }
                rule
            })
            .collect();
        assert_eq!(large.validate(), Err(GapPolicyError::InvalidPolicy));
    }

    #[test]
    fn materialization_is_stable_across_restart_and_uses_exact_frozen_ids() {
        let policy = policy();
        let gap = gap();
        let org = OrgId::new("org-a");
        let before = policy.materialize(&org, &gap).unwrap();
        let restored: GapAcquisitionPolicy =
            serde_json::from_slice(&serde_json::to_vec(&policy).unwrap()).unwrap();
        assert_eq!(before, restored.materialize(&org, &gap).unwrap());
        assert_eq!(before.client_request_id, gap.gap_id.as_str());
        assert_eq!(before.mission_id, gap.mission_id);
        assert_eq!(before.requirement_id, gap.requirement_id);
        assert_eq!(before.role_id, gap.role_id);
        assert_eq!(before.responsibility_id, gap.responsibility_id);
        assert_eq!(before.suite_id, "trusted-suite");
        assert_eq!(before.deadline, None);
        assert_eq!(policy.digest().unwrap(), restored.digest().unwrap());
    }

    #[test]
    fn identities_separate_org_gap_and_repair_with_valid_maximum_names() {
        let mut policy = policy();
        policy.rules[0].actor_name_prefix = "a".repeat(MAX_ACTOR_PREFIX_BYTES);
        let original = policy.materialize(&OrgId::new("org-a"), &gap()).unwrap();
        let other_org = policy.materialize(&OrgId::new("org-b"), &gap()).unwrap();
        let mut other_gap = gap();
        other_gap.gap_id = CapabilityGapId::new("gap-b");
        let other_gap = policy
            .materialize(&OrgId::new("org-a"), &other_gap)
            .unwrap();
        for other in [
            &other_org.candidate,
            &other_gap.candidate,
            original.repair.as_ref().unwrap(),
        ] {
            assert_ne!(original.candidate.actor_name, other.actor_name);
            assert_ne!(
                original.candidate.capability_version,
                other.capability_version
            );
            assert_eq!(original.candidate.capability_id, other.capability_id);
        }
        assert_ne!(
            original.candidate.source_version,
            original.repair.as_ref().unwrap().source_version
        );
        assert_eq!(original.candidate.actor_name.len(), 64);
        policy.rules[0].repair = false;
        assert!(policy
            .materialize(&OrgId::new("org-a"), &gap())
            .unwrap()
            .repair
            .is_none());
    }

    #[test]
    fn only_requested_sources_and_matching_fixed_examples_are_materialized() {
        let policy = policy();
        let request = policy.materialize(&OrgId::new("org-a"), &gap()).unwrap();
        assert_eq!(request.allowed_source_urls, gap().observed_sources);
        assert_eq!(request.examples, vec![policy.rules[0].examples[0].clone()]);
        let mut changed = gap();
        changed
            .observed_sources
            .push("https://foreign.example/".into());
        assert_eq!(
            policy.materialize(&OrgId::new("org-a"), &changed),
            Err(GapPolicyError::SourceNotAllowed)
        );
        changed.observed_sources = vec!["https://venues.example/c".into()];
        assert_eq!(
            policy.materialize(&OrgId::new("org-a"), &changed),
            Err(GapPolicyError::NoMatchingExample)
        );
        changed.observed_sources.clear();
        assert_eq!(
            policy.materialize(&OrgId::new("org-a"), &changed),
            Err(GapPolicyError::InvalidGap)
        );
    }

    #[test]
    fn rejects_missing_rule_and_unbounded_or_ambiguous_gap_ids() {
        let policy = policy();
        let org = OrgId::new("org-a");
        let mut changed = gap();
        changed.capability_id = CapabilityId::new("unknown-capability");
        assert_eq!(
            policy.materialize(&org, &changed),
            Err(GapPolicyError::NoMatchingRule)
        );
        changed = gap();
        changed.gap_id = CapabilityGapId::new("x".repeat(129));
        assert_eq!(
            policy.materialize(&org, &changed),
            Err(GapPolicyError::InvalidGap)
        );
        changed = gap();
        changed
            .observed_sources
            .push(changed.observed_sources[0].clone());
        assert_eq!(
            policy.materialize(&org, &changed),
            Err(GapPolicyError::InvalidGap)
        );
    }

    #[test]
    fn gap_content_and_lifecycle_cannot_supply_examples_or_oracle_data() {
        let policy = policy();
        let org = OrgId::new("org-a");
        let expected = policy.materialize(&org, &gap()).unwrap();
        let mut changed = gap();
        changed.purpose = "INJECTED_HTML_AND_ORACLE_CONTENT".into();
        changed.requirement_digest = "INJECTED_HTML_AND_ORACLE_CONTENT".into();
        changed.triggering_evidence = vec![ObservationId::new("INJECTED_HTML_AND_ORACLE_CONTENT")];
        changed.status = CapabilityGapStatus::Acquiring {
            acquisition_id: "acq-existing".into(),
        };
        changed.opened_in_revision += 1;
        let actual = policy.materialize(&org, &changed).unwrap();
        assert_eq!(actual, expected);
        assert!(!serde_json::to_string(&actual).unwrap().contains("INJECTED"));
        assert!(!GapPolicyError::NoMatchingExample
            .to_string()
            .contains(&policy.rules[0].examples[0].html));
    }

    #[test]
    fn digest_canonicalizes_object_keys_and_detects_example_changes() {
        let policy = policy();
        let encoded = serde_json::to_string(&policy).unwrap();
        let reordered: GapAcquisitionPolicy = serde_json::from_str(&encoded.replace(
            "\"url\":\"https://venues.example/a\",\"html\":\"<html>fixed public A</html>\"",
            "\"html\":\"<html>fixed public A</html>\",\"url\":\"https://venues.example/a\"",
        ))
        .unwrap();
        assert_eq!(policy.digest().unwrap(), reordered.digest().unwrap());
        let mut changed = policy.clone();
        changed.rules[0].examples[0].html.push_str(" changed");
        assert_ne!(policy.digest().unwrap(), changed.digest().unwrap());
    }
}
