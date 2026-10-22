//! Offline policy tests. Synthetic receipts/checkpoints represent trusted local
//! storage; they are not evidence that an Apify Actor actually ran.
use chrono::Utc;
use genesis_apify::SourceFile;
use genesis_core::capability::CaseKind;
use genesis_forge::{CandidateSpec, ForgeSession, SourcePackage};
use genesis_server::capability_evidence::{EvidencePolicy, EvidencePolicyError};
use genesis_verification::{
    decode_json, ArtifactPin, Decision, ExecutionEvidence, ExtractionOutcome, IssueCode,
    OracleSuite, VerificationOutcome,
};
use serde_json::{json, Value};

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}

fn artifact() -> ArtifactPin {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/artifact.json"
    ))
    .unwrap()
}

fn runs() -> Vec<ExecutionEvidence> {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap()
}

fn spec() -> CandidateSpec {
    CandidateSpec {
        operation_id: "synthetic-backend-forge-proof".into(),
        capability_id: artifact().capability_id,
        capability_version: artifact().capability_version,
        actor_name: "synthetic-policy-fixture".into(),
        source_version: "0.0".into(),
        source: SourcePackage::new(vec![SourceFile {
            name: "main.js".into(),
            content: "// Synthetic policy test source; never uploaded or executed.\n".into(),
        }])
        .unwrap(),
    }
}

/// Build a complete synthetic trusted-storage checkpoint. The policy must still
/// re-run all oracle checks; unlike a raw passed flag, every required receipt
/// and request/source/build binding is present. No network/provider claim.
fn verified_checkpoint_value() -> Value {
    let suite = suite();
    let spec = spec();
    let definition_digest = spec.definition().definition_sha256().unwrap();
    let mut pin = artifact();
    pin.source_sha256 = spec.source.sha256().unwrap();
    let mut receipts = runs();
    for receipt in &mut receipts {
        receipt.artifact = pin.clone();
    }
    let mut checkpoint = serde_json::to_value(ForgeSession::new(spec, &suite).unwrap()).unwrap();
    checkpoint["phase"] = json!("verified");
    checkpoint["actor"] = json!({
        "schema_version":1, "actor_id":pin.actor_id, "version_number":"0.0",
        "definition_sha256":definition_digest,
    });
    checkpoint["build"] = json!({
        "schema_version":1, "actor_id":pin.actor_id, "build_id":pin.build_id,
        "build_number":pin.build_number, "version_number":"0.0",
        "definition_sha256":definition_digest,
    });
    checkpoint["pin"] = json!(pin);
    checkpoint["receipts"] = json!(receipts);
    checkpoint
}

fn verified_checkpoint() -> ForgeSession {
    serde_json::from_value(verified_checkpoint_value()).unwrap()
}

#[test]
fn accepted_fixture_maps_real_case_kinds_scope_and_backend_time() {
    let oracle = suite();
    let policy = EvidencePolicy::new(vec![oracle.clone()], vec![]).unwrap();
    let before = Utc::now();
    let evidence = policy
        .accept_suite(&oracle.suite_id, &artifact(), &runs())
        .unwrap();
    let after = Utc::now();
    assert_eq!(evidence.artifact, artifact());
    assert_eq!(evidence.report.decision, Decision::Accepted);
    assert_eq!(evidence.kernel_report.oracle_ref, oracle.suite_id);
    assert_eq!(evidence.kernel_report.tested_environment, oracle.scope);
    assert!(evidence.kernel_report.verified_at >= before);
    assert!(evidence.kernel_report.verified_at <= after);
    assert_eq!(
        evidence
            .kernel_report
            .cases
            .iter()
            .map(|case| case.kind)
            .collect::<Vec<_>>(),
        vec![CaseKind::Success, CaseKind::Success, CaseKind::Unsupported]
    );
    assert!(evidence.kernel_report.meets_publication_bar().is_ok());
    assert!(!evidence.kernel_report.limitations.is_empty());
    assert!(evidence.reference.starts_with("verification:"));
    let serialized = serde_json::to_string(&evidence).unwrap();
    assert!(!serialized.contains("\"expected\""));
    assert!(!serialized.contains("\"venues\""));
}

#[test]
fn wrong_pin_input_source_and_missing_receipt_are_rejected() {
    let oracle = suite();
    let policy = EvidencePolicy::new(vec![oracle.clone()], vec![]).unwrap();
    for (kind, code) in [
        ("pin", IssueCode::ArtifactMismatch),
        ("input", IssueCode::InputMismatch),
        ("source", IssueCode::SourceMismatch),
        ("missing", IssueCode::MissingCase),
    ] {
        let mut receipts = runs();
        match kind {
            "pin" => receipts[0].artifact.build_id = "foreign-build".into(),
            "input" => receipts[0].input_sha256 = "0".repeat(64),
            "source" => receipts[0].source.sha256 = "0".repeat(64),
            "missing" => {
                receipts.remove(0);
            }
            _ => unreachable!(),
        }
        let outcome = policy
            .check_suite(&oracle.suite_id, &artifact(), &receipts)
            .unwrap();
        assert!(!outcome.is_accepted(), "{kind}");
        assert!(
            outcome
                .report()
                .cases
                .iter()
                .flat_map(|case| &case.issues)
                .any(|issue| issue.code == code),
            "{kind}"
        );
        assert!(matches!(
            policy.accept_suite(&oracle.suite_id, &artifact(), &receipts),
            Err(EvidencePolicyError::Rejected(_))
        ));
    }
}

#[test]
fn expected_artifact_is_pinned_independently_of_receipt_claims() {
    let oracle = suite();
    let policy = EvidencePolicy::new(vec![oracle.clone()], vec![]).unwrap();
    let mut foreign = artifact();
    foreign.build_number = "0.0.2".into();
    assert!(matches!(
        policy
            .check_suite(&oracle.suite_id, &foreign, &runs())
            .unwrap(),
        VerificationOutcome::Rejected(_)
    ));
}

#[test]
fn changed_oracle_recomputes_instead_of_inheriting_old_acceptance() {
    let mut oracle = suite();
    if let ExtractionOutcome::Supported { venues } = &mut oracle.cases[0].expected {
        venues[0].capacity += 1;
    } else {
        panic!("fixture must have a supported first case");
    }
    let policy = EvidencePolicy::new(vec![oracle.clone()], vec![]).unwrap();
    assert!(matches!(
        policy.accept_suite(&oracle.suite_id, &artifact(), &runs()),
        Err(EvidencePolicyError::Rejected(_))
    ));
}

#[test]
fn input_lookup_requires_full_canonical_input_and_unique_catalog_cases() {
    let oracle = suite();
    let policy = EvidencePolicy::new(vec![oracle.clone()], vec![]).unwrap();
    let case = &oracle.cases[0];
    let reordered = json!({
        "requested_window":case.input["requested_window"],
        "attendees":case.input["attendees"],
        "source_url":case.input["source_url"],
    });
    assert_eq!(
        policy
            .case_for_input(&oracle.suite_id, &reordered)
            .unwrap()
            .case_id,
        case.case_id
    );
    for field in ["attendees", "source_url", "requested_window", "extra"] {
        let mut input = reordered.clone();
        input[field] = json!("unconfigured-value");
        assert!(
            matches!(
                policy.case_for_input(&oracle.suite_id, &input),
                Err(EvidencePolicyError::UnknownCaseInput(_))
            ),
            "{field}"
        );
    }
    let mut ambiguous = oracle;
    let mut duplicate = ambiguous.cases[0].clone();
    duplicate.case_id = "duplicate-input-with-another-id".into();
    ambiguous.cases.push(duplicate);
    assert!(matches!(
        EvidencePolicy::new(vec![ambiguous], vec![]),
        Err(EvidencePolicyError::InvalidSuite { .. })
    ));
}

#[test]
fn invalid_duplicate_and_unknown_suites_fail_closed() {
    let oracle = suite();
    assert!(matches!(
        EvidencePolicy::new(vec![oracle.clone(), oracle.clone()], vec![]),
        Err(EvidencePolicyError::DuplicateSuite(_))
    ));
    let mut incomplete = oracle;
    incomplete.cases.pop();
    assert!(matches!(
        EvidencePolicy::new(vec![incomplete], vec![]),
        Err(EvidencePolicyError::InvalidSuite { .. })
    ));
    let policy = EvidencePolicy::default();
    assert!(matches!(
        policy.check_suite("caller-invented-suite", &artifact(), &runs()),
        Err(EvidencePolicyError::UnknownSuite(_))
    ));
    assert!(matches!(
        policy.verified_forge("caller-invented-proof"),
        Err(EvidencePolicyError::UnknownForgeProof(_))
    ));
}

#[test]
fn known_forge_proof_is_bound_to_exact_catalog_suite_and_operation() {
    let checkpoint = verified_checkpoint();
    let operation = checkpoint.spec().operation_id.clone();
    let pin = checkpoint.artifact().unwrap().clone();
    let mut other = suite();
    other.suite_id = "another-independent-organization".into();
    let policy = EvidencePolicy::new(vec![other, suite()], vec![checkpoint]).unwrap();
    let evidence = policy.verified_forge(&operation).unwrap();
    assert_eq!(evidence.reference, operation);
    assert_eq!(evidence.artifact, pin);
    assert_eq!(evidence.kernel_report.oracle_ref, suite().suite_id);
    assert_eq!(policy.forge_candidate(&operation).unwrap().artifact, pin);
    assert!(policy.forge_candidate("unknown").is_none());
}

#[test]
fn duplicate_forge_references_and_foreign_oracles_are_rejected() {
    assert!(matches!(
        EvidencePolicy::new(
            vec![suite()],
            vec![verified_checkpoint(), verified_checkpoint()]
        ),
        Err(EvidencePolicyError::DuplicateProof(_))
    ));
    let mut foreign = suite();
    foreign.scope.push_str(" Changed oracle scope.");
    assert!(matches!(
        EvidencePolicy::new(vec![foreign], vec![verified_checkpoint()]),
        Err(EvidencePolicyError::UnverifiedForge(_))
    ));
}

#[test]
fn verified_phase_without_receipts_and_tampered_completed_checkpoint_fail() {
    let mut label_only =
        serde_json::to_value(ForgeSession::new(spec(), &suite()).unwrap()).unwrap();
    label_only["phase"] = json!("verified");
    let mut bad_output = verified_checkpoint_value();
    bad_output["receipts"][0]["output"]["venues"][0]["capacity"] = json!(999_999);
    let mut bad_source = verified_checkpoint_value();
    bad_source["spec"]["source"]["files"][0]["content"] = json!("replacement source");
    for value in [label_only, bad_output, bad_source] {
        let checkpoint: ForgeSession = serde_json::from_value(value).unwrap();
        assert!(matches!(
            EvidencePolicy::new(vec![suite()], vec![checkpoint]),
            Err(EvidencePolicyError::UnverifiedForge(_))
        ));
    }
}
