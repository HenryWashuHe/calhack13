//! Backend-owned oracle catalog and verified forge proofs.
//!
//! Construct this policy only from trusted local configuration/checkpoints.
//! Neither oracle answers nor checkpoint source belongs in an HTTP request.
//! Execution receipts passed to this module must come from the authenticated
//! backend adapter: deserialization alone does not establish their provenance.

use chrono::Utc;
use genesis_core::capability::{CaseKind, CaseResult, VerificationReport as KernelReport};
use genesis_forge::{ForgeSession, VerifiedCandidate};
use genesis_verification::{
    json_sha256, validate_suite, verify, ArtifactPin, ConfigurationError, Decision,
    ExecutionEvidence, ExtractionOutcome, OracleCase, OracleSuite, VerificationOutcome,
    VerificationReport,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Evidence for the kernel to evaluate, never an authorization token. This has
/// no Deserialize implementation and is never accepted from client JSON.
#[derive(Clone, Debug, Serialize)]
pub struct VerifiedEvidence {
    pub artifact: ArtifactPin,
    pub kernel_report: KernelReport,
    pub report: VerificationReport,
    pub reference: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EvidencePolicyError {
    #[error("duplicate oracle suite: {0}")]
    DuplicateSuite(String),
    #[error("invalid oracle suite {suite_id}: {source}")]
    InvalidSuite {
        suite_id: String,
        source: ConfigurationError,
    },
    #[error("duplicate forge proof reference: {0}")]
    DuplicateProof(String),
    #[error("forge checkpoint must verify against exactly one configured suite: {0}")]
    UnverifiedForge(String),
    #[error("unknown oracle suite: {0}")]
    UnknownSuite(String),
    #[error("input does not match a configured oracle case in suite: {0}")]
    UnknownCaseInput(String),
    #[error("input ambiguously matches multiple oracle cases in suite: {0}")]
    AmbiguousCaseInput(String),
    #[error("unknown backend forge proof: {0}")]
    UnknownForgeProof(String),
    #[error("invalid verification configuration: {0}")]
    InvalidEvidence(#[from] ConfigurationError),
    #[error("execution evidence did not pass the configured oracle")]
    Rejected(Box<VerificationReport>),
    #[error("verification report binding mismatch: {0}")]
    BindingMismatch(&'static str),
    #[error("cannot encode verification evidence")]
    Encoding,
}

#[derive(Clone)]
struct ForgeProof {
    candidate: VerifiedCandidate,
    evidence: VerifiedEvidence,
}

/// Immutable after construction; private fields cannot be populated by a
/// deserialized passed flag. Cloning a policy preserves its verified catalog.
#[derive(Clone, Default)]
pub struct EvidencePolicy {
    suites: BTreeMap<String, OracleSuite>,
    forge_proofs: BTreeMap<String, ForgeProof>,
}

impl std::fmt::Debug for EvidencePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvidencePolicy")
            .field("suite_count", &self.suites.len())
            .field("forge_proof_count", &self.forge_proofs.len())
            .finish_non_exhaustive()
    }
}

impl EvidencePolicy {
    pub fn new(
        suites: Vec<OracleSuite>,
        sessions: Vec<ForgeSession>,
    ) -> Result<Self, EvidencePolicyError> {
        let mut policy = Self::default();
        for suite in suites {
            validate_suite(&suite).map_err(|source| EvidencePolicyError::InvalidSuite {
                suite_id: suite.suite_id.clone(),
                source,
            })?;
            let id = suite.suite_id.clone();
            if policy.suites.insert(id.clone(), suite).is_some() {
                return Err(EvidencePolicyError::DuplicateSuite(id));
            }
        }
        for session in sessions {
            let reference = session.spec().operation_id.clone();
            if policy.forge_proofs.contains_key(&reference) {
                return Err(EvidencePolicyError::DuplicateProof(reference));
            }
            let mut matched = None;
            for suite in policy.suites.values() {
                // `verified` revalidates the checkpoint and executes the actual
                // verifier again; a stored phase of "verified" is insufficient.
                if session.validate(suite).is_err() {
                    continue;
                }
                let Ok(candidate) = session.verified(suite) else {
                    continue;
                };
                if matched.is_some() {
                    return Err(EvidencePolicyError::UnverifiedForge(reference));
                }
                let evidence = map_verified_report(
                    suite,
                    &candidate.artifact,
                    candidate.verification.clone(),
                    reference.clone(),
                )?;
                matched = Some(ForgeProof {
                    candidate,
                    evidence,
                });
            }
            let proof =
                matched.ok_or_else(|| EvidencePolicyError::UnverifiedForge(reference.clone()))?;
            policy.forge_proofs.insert(reference, proof);
        }
        Ok(policy)
    }

    /// Trusted backend access only. Do not return this oracle through an API.
    pub fn suite(&self, suite_id: &str) -> Option<&OracleSuite> {
        self.suites.get(suite_id)
    }

    /// Resolve only an exact configured input. A URL match alone is insufficient:
    /// attendee count, requested window, and every other input field are pinned.
    pub fn case_for_input(
        &self,
        suite_id: &str,
        input: &serde_json::Value,
    ) -> Result<&OracleCase, EvidencePolicyError> {
        let suite = self
            .suite(suite_id)
            .ok_or_else(|| EvidencePolicyError::UnknownSuite(suite_id.to_owned()))?;
        let mut matched = None;
        let mut input_digest = None;
        for case in &suite.cases {
            // Equality with an already validated, bounded oracle input limits
            // the structure before hashing caller data recursively. The digest
            // also distinguishes JSON numeric encodings such as 0.0 and -0.0.
            if case.input != *input {
                continue;
            }
            let input_digest = input_digest.get_or_insert_with(|| json_sha256(input));
            if json_sha256(&case.input) != *input_digest {
                continue;
            }
            if matched.is_some() {
                return Err(EvidencePolicyError::AmbiguousCaseInput(suite_id.to_owned()));
            }
            matched = Some(case);
        }
        matched.ok_or_else(|| EvidencePolicyError::UnknownCaseInput(suite_id.to_owned()))
    }

    pub fn check_suite(
        &self,
        suite_id: &str,
        artifact: &ArtifactPin,
        runs: &[ExecutionEvidence],
    ) -> Result<VerificationOutcome, EvidencePolicyError> {
        let suite = self
            .suite(suite_id)
            .ok_or_else(|| EvidencePolicyError::UnknownSuite(suite_id.to_owned()))?;
        Ok(verify(suite, artifact, runs)?)
    }

    /// Recompute acceptance from adapter observations. There is intentionally
    /// no public method that converts a caller-supplied report or passed flag.
    pub fn accept_suite(
        &self,
        suite_id: &str,
        artifact: &ArtifactPin,
        runs: &[ExecutionEvidence],
    ) -> Result<VerifiedEvidence, EvidencePolicyError> {
        let accepted = match self.check_suite(suite_id, artifact, runs)? {
            VerificationOutcome::Accepted(accepted) => accepted,
            VerificationOutcome::Rejected(report) => {
                return Err(EvidencePolicyError::Rejected(Box::new(report)));
            }
        };
        let suite = self
            .suite(suite_id)
            .ok_or_else(|| EvidencePolicyError::UnknownSuite(suite_id.to_owned()))?;
        let report = accepted.report().clone();
        let reference = format!("verification:{}", digest(&report)?);
        map_verified_report(suite, artifact, report, reference)
    }

    pub fn forge_candidate(&self, proof_ref: &str) -> Option<&VerifiedCandidate> {
        self.forge_proofs
            .get(proof_ref)
            .map(|proof| &proof.candidate)
    }

    /// Lookup succeeds only for a checkpoint verified during policy creation.
    /// The proof reference is its original CandidateSpec operation_id.
    pub fn verified_forge(&self, proof_ref: &str) -> Result<VerifiedEvidence, EvidencePolicyError> {
        self.forge_proofs
            .get(proof_ref)
            .map(|proof| proof.evidence.clone())
            .ok_or_else(|| EvidencePolicyError::UnknownForgeProof(proof_ref.to_owned()))
    }

    /// Verify a checkpoint loaded from the organization's trusted acquisition
    /// store. The caller must separately check its live mission/grant authority.
    /// Never pass a checkpoint supplied through an HTTP request to this method.
    pub fn verify_session(
        &self,
        suite_id: &str,
        session: &ForgeSession,
    ) -> Result<VerifiedEvidence, EvidencePolicyError> {
        let suite = self
            .suite(suite_id)
            .ok_or_else(|| EvidencePolicyError::UnknownSuite(suite_id.to_owned()))?;
        let reference = session.spec().operation_id.clone();
        let candidate = session
            .verified(suite)
            .map_err(|_| EvidencePolicyError::UnverifiedForge(reference.clone()))?;
        map_verified_report(
            suite,
            &candidate.artifact,
            candidate.verification,
            reference,
        )
    }
}

fn digest(value: &impl Serialize) -> Result<String, EvidencePolicyError> {
    serde_json::to_value(value)
        .map(|value| json_sha256(&value))
        .map_err(|_| EvidencePolicyError::Encoding)
}

// Only called with freshly recomputed verifier output (directly through an
// AcceptedVerification or through ForgeSession::verified). Keep this private:
// matching report metadata is defense in depth, not receipt authentication.
fn map_verified_report(
    suite: &OracleSuite,
    artifact: &ArtifactPin,
    report: VerificationReport,
    reference: String,
) -> Result<VerifiedEvidence, EvidencePolicyError> {
    if report.schema_version != 1
        || report.decision != Decision::Accepted
        || !report.issues.is_empty()
        || report.suite_id != suite.suite_id
        || report.scope != suite.scope
        || report.suite_sha256 != digest(suite)?
    {
        return Err(EvidencePolicyError::BindingMismatch("oracle or decision"));
    }
    if report.artifact != *artifact {
        return Err(EvidencePolicyError::BindingMismatch("artifact"));
    }
    if report.cases.len() != suite.cases.len() {
        return Err(EvidencePolicyError::BindingMismatch("case count"));
    }
    let mut case_ids = BTreeSet::new();
    let mut cases = Vec::with_capacity(suite.cases.len());
    for (expected, actual) in suite.cases.iter().zip(&report.cases) {
        if actual.case_id != expected.case_id
            || !case_ids.insert(&actual.case_id)
            || !actual.issues.is_empty()
            || actual.input_sha256 != json_sha256(&expected.input)
            || actual.source != expected.source
            || actual.run_id.as_deref().is_none_or(str::is_empty)
            || actual.dataset_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(EvidencePolicyError::BindingMismatch("case evidence"));
        }
        cases.push(CaseResult {
            name: expected.case_id.clone(),
            kind: match expected.expected {
                ExtractionOutcome::Supported { .. } => CaseKind::Success,
                ExtractionOutcome::Unsupported { .. } => CaseKind::Unsupported,
            },
            passed: true,
            detail: "Matched the independent oracle for the pinned input and source snapshot."
                .to_owned(),
        });
    }
    let kernel_report = KernelReport {
        passed: true,
        cases,
        tested_environment: suite.scope.clone(),
        limitations: vec![
            "Input-specific fixture acceptance only; no generalization to other inputs, sources, or future source changes.".to_owned(),
            "Execution metadata is trusted backend input; this report is not a provider signature.".to_owned(),
            "No safety, booking, publication, or execution permission is granted.".to_owned(),
        ],
        oracle_ref: suite.suite_id.clone(),
        verified_at: Utc::now(),
    };
    Ok(VerifiedEvidence {
        artifact: artifact.clone(),
        kernel_report,
        report,
        reference,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_verification::decode_json;

    #[test]
    fn mapping_refuses_foreign_oracle_hash_artifact_and_case_bindings() {
        let suite: OracleSuite = decode_json(include_bytes!(
            "../../genesis-verification/examples/fixtures/trusted/suite.json"
        ))
        .unwrap();
        let artifact: ArtifactPin = decode_json(include_bytes!(
            "../../genesis-verification/examples/fixtures/artifact.json"
        ))
        .unwrap();
        let runs: Vec<ExecutionEvidence> = decode_json(include_bytes!(
            "../../genesis-verification/examples/fixtures/runs.json"
        ))
        .unwrap();
        let accepted = verify(&suite, &artifact, &runs).unwrap();
        assert!(accepted.is_accepted());
        for changed in ["suite", "hash", "artifact", "input", "source", "case"] {
            let mut report = accepted.report().clone();
            match changed {
                "suite" => report.suite_id = "foreign-suite".into(),
                "hash" => report.suite_sha256 = "0".repeat(64),
                "artifact" => report.artifact.build_id = "foreign-build".into(),
                "input" => report.cases[0].input_sha256 = "0".repeat(64),
                "source" => report.cases[0].source.sha256 = "0".repeat(64),
                "case" => {
                    report.cases.pop();
                }
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    map_verified_report(&suite, &artifact, report, "test-only".into()),
                    Err(EvidencePolicyError::BindingMismatch(_))
                ),
                "{changed}"
            );
        }
    }
}
