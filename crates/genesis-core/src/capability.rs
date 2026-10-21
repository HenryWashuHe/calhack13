//! Capability manifests, verification evidence and publication rules
//! (TECH_SPEC §8 and §9).

use crate::ids::*;
use crate::lifecycle::CapabilityState;
use crate::model::{CapabilityRef, Constitution};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    /// A fixture variant where extraction must succeed and match the oracle.
    Success,
    /// A fixture where data is missing or unsupported and the Actor must say so.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CaseResult {
    pub name: String,
    pub kind: CaseKind,
    pub passed: bool,
    #[serde(default)]
    pub detail: String,
}

/// Independent acceptance evidence. The oracle lives outside the generated
/// Actor and outside the public portal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VerificationReport {
    pub passed: bool,
    pub cases: Vec<CaseResult>,
    /// What environment was tested, e.g. `"fixture-portal v1, Apify LIMITED_PERMISSIONS"`.
    pub tested_environment: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub oracle_ref: String,
    pub verified_at: DateTime<Utc>,
}

impl VerificationReport {
    /// Minimum evidence before a capability can be approved: every case passed,
    /// at least two successful fixture variants and one unsupported case.
    pub fn meets_publication_bar(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if !self.passed {
            problems.push("verification report is not marked passed".to_owned());
        }
        let failed: Vec<_> = self
            .cases
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.clone())
            .collect();
        if !failed.is_empty() {
            problems.push(format!("failed cases: {}", failed.join(", ")));
        }
        let success = self
            .cases
            .iter()
            .filter(|c| c.kind == CaseKind::Success && c.passed)
            .count();
        let unsupported = self
            .cases
            .iter()
            .filter(|c| c.kind == CaseKind::Unsupported && c.passed)
            .count();
        if success < 2 {
            problems.push(format!(
                "need at least 2 passing success variants, got {success}"
            ));
        }
        if unsupported < 1 {
            problems.push("need at least 1 passing unsupported/missing-data case".to_owned());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// Pinned Apify artifact. Never `latest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActorArtifact {
    pub actor_id: String,
    pub build_id: String,
    pub build_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RoleTemplate {
    pub name: String,
    pub charter: String,
    #[serde(default)]
    pub scopes: BTreeSet<String>,
}

/// Shareable package published after verification (TECH_SPEC §9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityManifest {
    pub capability_id: CapabilityId,
    pub version: String,
    pub publisher: OrgId,
    pub source_digest: String,
    pub intent: String,
    #[serde(default)]
    pub applicability_conditions: Vec<String>,
    #[serde(default)]
    pub known_limitations: Vec<String>,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    /// Scopes the installing organization must be able to grant.
    #[serde(default)]
    pub required_permissions: BTreeSet<String>,
    pub artifact: ActorArtifact,
    pub role_template: Option<RoleTemplate>,
    pub responsibility_template: Option<String>,
    pub workflow_procedure: Option<String>,
    #[serde(default)]
    pub local_configuration_schema: serde_json::Value,
    pub acceptance_suite_ref: String,
    pub verification: VerificationReport,
    #[serde(default)]
    pub provenance: Vec<String>,
    pub state: CapabilityState,
    pub published_at: Option<DateTime<Utc>>,
}

impl CapabilityManifest {
    pub fn reference(&self) -> CapabilityRef {
        CapabilityRef::new(self.capability_id.clone(), self.version.clone())
    }

    /// Checks that must pass before the registry accepts the manifest.
    pub fn check_publishable(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.version.trim().is_empty() || self.version.eq_ignore_ascii_case("latest") {
            problems.push("version must be an exact version, not empty or `latest`".to_owned());
        }
        if self.artifact.build_number.eq_ignore_ascii_case("latest")
            || self.artifact.build_number.is_empty()
        {
            problems.push("artifact must pin an exact build number".to_owned());
        }
        if self.source_digest.is_empty() {
            problems.push("source_digest is required".to_owned());
        }
        if !self.state.is_usable() {
            problems.push(format!("state {:?} is not approved", self.state));
        }
        if let Err(mut more) = self.verification.meets_publication_bar() {
            problems.append(&mut more);
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// Whether an installing organization can adopt this version as-is.
    pub fn check_compatibility(&self, constitution: &Constitution) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if !self.state.is_usable() {
            problems.push(format!("capability state {:?} is not usable", self.state));
        }
        let missing: Vec<_> = self
            .required_permissions
            .iter()
            .filter(|p| !constitution.scopes.contains(*p))
            .cloned()
            .collect();
        if !missing.is_empty() {
            problems.push(format!(
                "constitution lacks required scopes: {}",
                missing.join(", ")
            ));
        }
        if !self.verification.passed {
            problems.push("publisher verification did not pass".to_owned());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// Lookup used by validation to check that attached capabilities exist and
/// are approved.
pub trait CapabilityCatalog {
    fn state_of(&self, capability: &CapabilityRef) -> Option<CapabilityState>;
}

/// Simple in-memory catalog, handy for tests and for the org-local view of
/// installed versions.
#[derive(Debug, Default, Clone)]
pub struct StaticCatalog(pub std::collections::BTreeMap<CapabilityRef, CapabilityState>);

impl StaticCatalog {
    pub fn with(mut self, capability: CapabilityRef, state: CapabilityState) -> Self {
        self.0.insert(capability, state);
        self
    }
}

impl CapabilityCatalog for StaticCatalog {
    fn state_of(&self, capability: &CapabilityRef) -> Option<CapabilityState> {
        self.0.get(capability).copied()
    }
}

/// SHA-256 hex digest helper for source and configuration digests.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn report(cases: &[(&str, CaseKind, bool)]) -> VerificationReport {
        VerificationReport {
            passed: cases.iter().all(|c| c.2),
            cases: cases
                .iter()
                .map(|(n, k, p)| CaseResult {
                    name: (*n).to_owned(),
                    kind: *k,
                    passed: *p,
                    detail: String::new(),
                })
                .collect(),
            tested_environment: "fixture-portal test".to_owned(),
            limitations: vec![],
            oracle_ref: "oracle://test".to_owned(),
            verified_at: Utc::now(),
        }
    }

    pub fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            capability_id: CapabilityId::new("cap_venue_availability"),
            version: "1.0.0".to_owned(),
            publisher: OrgId::new("org_a"),
            source_digest: sha256_hex(b"source"),
            intent: "Read venue availability, capacity and price from a venue portal".to_owned(),
            applicability_conditions: vec!["portal exposes listing pages".to_owned()],
            known_limitations: vec![],
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: serde_json::json!({"type": "array"}),
            required_permissions: ["capability.venue_lookup".to_owned()].into(),
            artifact: ActorArtifact {
                actor_id: "actor123".to_owned(),
                build_id: "build456".to_owned(),
                build_number: "0.0.3".to_owned(),
            },
            role_template: None,
            responsibility_template: None,
            workflow_procedure: None,
            local_configuration_schema: serde_json::Value::Null,
            acceptance_suite_ref: "suite://venue".to_owned(),
            verification: report(&[
                ("variant-a", CaseKind::Success, true),
                ("variant-b", CaseKind::Success, true),
                ("missing-data", CaseKind::Unsupported, true),
            ]),
            provenance: vec![],
            state: CapabilityState::Approved,
            published_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn publication_requires_two_success_and_one_unsupported_case() {
        let ok = report(&[
            ("a", CaseKind::Success, true),
            ("b", CaseKind::Success, true),
            ("c", CaseKind::Unsupported, true),
        ]);
        assert!(ok.meets_publication_bar().is_ok());

        let too_few = report(&[
            ("a", CaseKind::Success, true),
            ("c", CaseKind::Unsupported, true),
        ]);
        assert!(too_few.meets_publication_bar().is_err());

        let failed = report(&[
            ("a", CaseKind::Success, true),
            ("b", CaseKind::Success, false),
            ("c", CaseKind::Unsupported, true),
        ]);
        let problems = failed.meets_publication_bar().unwrap_err();
        assert!(problems.iter().any(|p| p.contains("b")));
    }

    #[test]
    fn latest_is_never_publishable() {
        let mut m = manifest();
        m.version = "latest".to_owned();
        assert!(m.check_publishable().is_err());
        let mut m = manifest();
        m.artifact.build_number = "latest".to_owned();
        assert!(m.check_publishable().is_err());
        assert!(manifest().check_publishable().is_ok());
    }

    #[test]
    fn successful_build_without_verification_is_not_publishable() {
        let mut m = manifest();
        m.state = CapabilityState::Testing;
        assert!(m.check_publishable().is_err());
    }

    #[test]
    fn compatibility_requires_local_scopes() {
        let m = manifest();
        let mut constitution = Constitution::default();
        assert!(m.check_compatibility(&constitution).is_err());
        constitution
            .scopes
            .insert("capability.venue_lookup".to_owned());
        assert!(m.check_compatibility(&constitution).is_ok());
    }
}
