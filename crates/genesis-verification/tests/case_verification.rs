use genesis_verification::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;

// Oracle answers and candidate outputs are separately authored fixtures. These
// tests do not generate an extraction result from the expected oracle values.
fn fixture() -> (OracleSuite, ArtifactPin, Vec<ExecutionEvidence>) {
    let suite = decode_json(include_bytes!("../examples/fixtures/trusted/suite.json")).unwrap();
    let runs: Vec<ExecutionEvidence> =
        decode_json(include_bytes!("../examples/fixtures/runs.json")).unwrap();
    let pin = runs[0].artifact.clone();
    (suite, pin, runs)
}

fn checked(
    suite: &OracleSuite,
    pin: &ArtifactPin,
    run: &ExecutionEvidence,
) -> CaseVerificationOutcome {
    verify_case(suite, pin, &run.case_id, run).unwrap()
}

#[test]
fn supported_and_unsupported_are_distinct_successful_case_verifications() {
    let (suite, pin, runs) = fixture();
    for (index, run) in runs.iter().enumerate() {
        let result = checked(&suite, &pin, run);
        let CaseVerificationOutcome::Accepted(accepted) = result else {
            panic!("independent fixture rejected: {:?}", result.report());
        };
        let report = accepted.report();
        assert_eq!(accepted.artifact(), &pin);
        assert_eq!(report.decision, Decision::Accepted);
        assert_eq!(report.purpose, CaseVerificationPurpose::InvocationCase);
        assert_eq!(
            report.extraction,
            Some(if index == 2 {
                ExtractionKind::Unsupported
            } else {
                ExtractionKind::Supported
            })
        );
        assert_eq!(report.suite_id, suite.suite_id);
        assert_eq!(
            report.suite_sha256,
            json_sha256(&serde_json::to_value(&suite).unwrap())
        );
        assert_eq!(report.case_id, run.case_id);
        assert_eq!(report.source, run.source);
        assert_eq!(report.input_sha256, run.input_sha256);
        assert_eq!(report.run_id.as_deref(), Some(run.run_id.as_str()));
        assert_eq!(report.dataset_id.as_deref(), Some(run.dataset_id.as_str()));
        assert_eq!(
            report.evidence_sha256,
            Some(json_sha256(&serde_json::to_value(run).unwrap()))
        );
        assert!(report.issues.is_empty());
        // The separate suite verifier still requires all cases.
        assert!(!verify(&suite, &pin, std::slice::from_ref(run))
            .unwrap()
            .is_accepted());
    }
}

#[test]
fn selected_case_is_exact_and_full_suite_is_validated_before_selection() {
    let (suite, pin, runs) = fixture();
    assert!(verify_case(&suite, &pin, "absent", &runs[0]).is_err());
    assert!(verify_case(&suite, &pin, &"x".repeat(1025), &runs[0]).is_err());
    let wrong = verify_case(&suite, &pin, &runs[0].case_id, &runs[1]).unwrap();
    assert!(!wrong.is_accepted());
    assert!(wrong.report().issues.contains(&IssueCode::UnknownCase));
    assert_eq!(wrong.report().case_id, runs[0].case_id);
    let mut substituted = runs[0].clone();
    substituted.case_id = runs[1].case_id.clone();
    let result = verify_case(&suite, &pin, &runs[0].case_id, &substituted).unwrap();
    assert_eq!(result.report().issues, vec![IssueCode::UnknownCase]);
    assert!(!result.is_accepted());

    let edits: Vec<fn(&mut OracleSuite)> = vec![
        |s| s.cases.truncate(1),
        |s| s.cases[1].case_id = s.cases[0].case_id.clone(),
        |s| s.cases[1].input = s.cases[0].input.clone(),
        |s| s.cases[1].source.url = "not-a-url".into(),
        |s| {
            if let ExtractionOutcome::Supported { venues } = &mut s.cases[1].expected {
                venues[0].price.currency = "invalid".into();
            }
        },
        |s| s.cases[1].input = json!({"oversized":"x".repeat(1_048_577)}),
    ];
    for (index, edit) in edits.into_iter().enumerate() {
        let mut invalid = suite.clone();
        edit(&mut invalid);
        assert!(
            verify_case(&invalid, &pin, &runs[0].case_id, &runs[0]).is_err(),
            "case {index}"
        );
    }
}

#[test]
fn case_verification_reuses_all_receipt_and_extraction_checks() {
    let (suite, pin, runs) = fixture();
    type Edit = (IssueCode, fn(&mut ExecutionEvidence));
    let edits: Vec<Edit> = vec![
        (IssueCode::ArtifactMismatch, |r| {
            r.artifact.build_id = "different-build".into()
        }),
        (IssueCode::InputMismatch, |r| {
            r.input_sha256 = "0".repeat(64)
        }),
        (IssueCode::SourceMismatch, |r| {
            r.source.sha256 = "0".repeat(64)
        }),
        (IssueCode::RunNotSucceeded, |r| {
            r.status = RunStatus::Unknown
        }),
        (IssueCode::RunNotSucceeded, |r| r.status = RunStatus::Failed),
        (IssueCode::MalformedOutput, |r| {
            r.output["unexpected"] = json!(true)
        }),
        (IssueCode::InvalidOutput, |r| {
            r.output["venues"][0]["window"]["end"] = json!("invalid-time")
        }),
        (IssueCode::OutcomeMismatch, |r| {
            r.output = json!({"status":"unsupported","reason":"unsupported_format",
            "evidence":{"source_url":r.source.url,"snapshot_sha256":r.source.sha256,"locator":"body"}})
        }),
        (IssueCode::MissingVenue, |r| {
            r.output["venues"].as_array_mut().unwrap().pop();
        }),
        (IssueCode::UnexpectedVenue, |r| {
            r.output["venues"][0]["venue_id"] = json!("invented")
        }),
        (IssueCode::DuplicateVenue, |r| {
            let duplicate = r.output["venues"][0].clone();
            r.output["venues"].as_array_mut().unwrap().push(duplicate);
        }),
        (IssueCode::ValueMismatch, |r| {
            r.output["venues"][0]["capacity"] = json!(999999)
        }),
    ];
    for (expected, edit) in edits {
        let mut run = runs[0].clone();
        edit(&mut run);
        let result = checked(&suite, &pin, &run);
        assert!(!result.is_accepted(), "{expected:?}");
        assert!(
            result.report().issues.contains(&expected),
            "{expected:?}: {:?}",
            result.report()
        );
        let mut full = runs.clone();
        full[0] = run;
        let full_result = verify(&suite, &pin, &full).unwrap();
        assert!(!full_result.is_accepted());
        let issues: BTreeSet<_> = full_result.report().cases[0]
            .issues
            .iter()
            .map(|issue| issue.code)
            .collect();
        assert_eq!(
            result
                .report()
                .issues
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            issues
        );
    }
}

#[test]
fn report_is_redacted_and_issue_categories_are_unique_and_deterministic() {
    let (mut suite, pin, mut runs) = fixture();
    suite.scope = "ORACLE_SCOPE_CANARY".into();
    suite.cases[0].input["private"] = json!("PRIVATE_INPUT_CANARY");
    runs[0].input_sha256 = json_sha256(&suite.cases[0].input);
    if let ExtractionOutcome::Supported { venues } = &mut suite.cases[0].expected {
        venues[0].venue_id = "ORACLE_EXPECTED_CANARY".into();
    }
    runs[0].output["venues"][0]["capacity"] = json!(999999);
    runs[0].output["venues"][0]["price"]["minor_units"] = json!(999999);
    let result = checked(&suite, &pin, &runs[0]);
    let report = result.report();
    assert_eq!(
        report
            .issues
            .iter()
            .filter(|issue| **issue == IssueCode::ValueMismatch)
            .count(),
        1
    );
    assert!(report.issues.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(report, checked(&suite, &pin, &runs[0]).report());
    let encoded = serde_json::to_string(report).unwrap();
    for private in [
        "ORACLE_SCOPE_CANARY",
        "PRIVATE_INPUT_CANARY",
        "ORACLE_EXPECTED_CANARY",
        "output/venues",
    ] {
        assert!(!encoded.contains(private), "leaked {private}");
    }
    let object = serde_json::to_value(report).unwrap();
    for forbidden in ["scope", "input", "output", "expected", "paths", "cases"] {
        assert!(object.get(forbidden).is_none(), "unexpected {forbidden}");
    }
    runs[0].output["private"] = json!("RAW_OUTPUT_CANARY");
    assert!(
        !serde_json::to_string(checked(&suite, &pin, &runs[0]).report())
            .unwrap()
            .contains("RAW_OUTPUT_CANARY")
    );
}

#[test]
fn canonical_proof_binds_complete_suite_pin_and_exact_received_evidence() {
    let (suite, pin, runs) = fixture();
    let original = checked(&suite, &pin, &runs[0]);
    let report = original.report();
    let digest = report.proof_sha256().unwrap();
    assert_eq!(
        digest,
        json_sha256(&json!({"domain":"genesis-invocation-case-v1","report":report}))
    );
    assert_ne!(digest, json_sha256(&serde_json::to_value(report).unwrap()));
    assert_eq!(report.proof_sha256().unwrap(), digest);

    let mut changed = suite.clone();
    changed.scope.push_str(" changed policy");
    let scoped = checked(&changed, &pin, &runs[0]);
    assert!(scoped.is_accepted());
    assert_ne!(scoped.report().suite_sha256, report.suite_sha256);
    assert_ne!(scoped.report().proof_sha256().unwrap(), digest);
    changed = suite.clone();
    changed.cases[1].input["extra"] = json!("unrelated valid input change");
    let unrelated = checked(&changed, &pin, &runs[0]);
    assert!(unrelated.is_accepted());
    assert_ne!(unrelated.report().proof_sha256().unwrap(), digest);

    for edit in [
        (|r: &mut ExecutionEvidence| r.run_id.push_str("-different")) as fn(&mut ExecutionEvidence),
        |r| r.dataset_id.push_str("-different"),
        |r| r.output["venues"].as_array_mut().unwrap().reverse(),
    ] {
        let mut run = runs[0].clone();
        edit(&mut run);
        let changed = checked(&suite, &pin, &run);
        assert!(changed.is_accepted());
        assert_ne!(changed.report().evidence_sha256, report.evidence_sha256);
        assert_ne!(changed.report().proof_sha256().unwrap(), digest);
    }
    let mut newer = pin.clone();
    newer.build_id = "newer-build".into();
    let mut run = runs[0].clone();
    run.artifact = newer.clone();
    let changed = checked(&suite, &newer, &run);
    assert!(changed.is_accepted());
    assert_ne!(changed.report().proof_sha256().unwrap(), digest);
}

#[test]
fn malformed_or_oversized_metadata_is_rejected_without_hashing_or_echoing_it() {
    let (suite, pin, runs) = fixture();
    let edits: Vec<fn(&mut ExecutionEvidence)> = vec![
        |r| r.case_id = "X".repeat(1025),
        |r| r.run_id = "X".repeat(1025),
        |r| r.dataset_id = "X".repeat(1025),
        |r| r.artifact.actor_id = "X".repeat(1025),
        |r| r.artifact.build_number = "latest".into(),
        |r| r.source.url = format!("https://example.com/{}", "X".repeat(1025)),
        |r| r.source.sha256 = "X".repeat(1025),
        |r| r.input_sha256 = "X".repeat(1025),
        |r| r.run_id = "\ninvalid".into(),
        |r| r.dataset_id.clear(),
    ];
    for edit in edits {
        let mut run = runs[0].clone();
        edit(&mut run);
        let result = verify_case(&suite, &pin, &runs[0].case_id, &run).unwrap();
        assert!(!result.is_accepted());
        assert!(result.report().issues.contains(&IssueCode::InvalidReceipt));
        assert!(result.report().evidence_sha256.is_none());
        assert!(!serde_json::to_string(result.report())
            .unwrap()
            .contains(&"X".repeat(1025)));
        result.report().proof_sha256().unwrap();
    }
}

#[test]
fn over_limit_json_is_rejected_before_recursive_hashing() {
    let (suite, pin, runs) = fixture();
    let mut deep = Value::Null;
    for _ in 0..64 {
        deep = json!({"next":deep});
    }
    for output in [
        json!({"large":"x".repeat(1_048_577)}),
        Value::Array(vec![Value::Null; 50_001]),
        deep,
    ] {
        let mut run = runs[0].clone();
        run.output = output;
        let result = checked(&suite, &pin, &run);
        assert!(!result.is_accepted());
        assert!(result.report().issues.contains(&IssueCode::InvalidOutput));
        assert!(result.report().evidence_sha256.is_none());
        assert!(result.report().extraction.is_none());
        result.report().proof_sha256().unwrap();
    }
}

#[test]
fn report_digest_refuses_unbounded_or_inconsistent_mutated_reports() {
    let (suite, pin, runs) = fixture();
    let result = checked(&suite, &pin, &runs[0]);
    let mut report = result.report().clone();
    report.suite_id = "X".repeat(1025);
    assert!(report.proof_sha256().is_err());
    let mut report = result.report().clone();
    report.evidence_sha256 = None;
    assert!(report.proof_sha256().is_err());
    let mut report = result.report().clone();
    report.issues = vec![IssueCode::ValueMismatch; 17];
    assert!(report.proof_sha256().is_err());
}
