use genesis_verification::*;
use serde_json::{json, Value};

// The oracle uses typed, hand-authored facts. Candidate JSON is separately
// authored below: neither side is generated from the other or from a parser.
fn fixture() -> (OracleSuite, ArtifactPin, Vec<ExecutionEvidence>) {
    let source_a = SourceSnapshot {
        url: "https://campus.example/venues/table".into(),
        sha256: "1".repeat(64),
    };
    let source_b = SourceSnapshot {
        url: "https://campus.example/venues/cards".into(),
        sha256: "2".repeat(64),
    };
    let source_c = SourceSnapshot {
        url: "https://campus.example/venues/incomplete".into(),
        sha256: "3".repeat(64),
    };
    let suite = OracleSuite {
        schema_version: 1,
        suite_id: "venue-acceptance-v1".into(),
        scope: "Synthetic campus fixtures; declared accessibility only".into(),
        cases: vec![
            OracleCase {
                case_id: "table".into(),
                format_id: "html-table-v1".into(),
                input: json!({"url": source_a.url, "date": "2026-10-22"}),
                source: source_a.clone(),
                expected: ExtractionOutcome::Supported {
                    venues: vec![
                        Venue {
                            venue_id: "hall-a".into(),
                            capacity: 160,
                            price: Money {
                                minor_units: 45000,
                                currency: "USD".into(),
                            },
                            window: AvailabilityWindow {
                                start: "2026-10-22T09:00:00-04:00".into(),
                                end: "2026-10-22T13:00:00-04:00".into(),
                            },
                            availability: Availability::Available,
                            step_free: true,
                            evidence: EvidenceRef {
                                source_url: source_a.url.clone(),
                                snapshot_sha256: source_a.sha256.clone(),
                                locator: "#hall-a".into(),
                            },
                        },
                        Venue {
                            venue_id: "hall-b".into(),
                            capacity: 220,
                            price: Money {
                                minor_units: 60000,
                                currency: "USD".into(),
                            },
                            window: AvailabilityWindow {
                                start: "2026-10-22T09:00:00-04:00".into(),
                                end: "2026-10-22T13:00:00-04:00".into(),
                            },
                            availability: Availability::Unavailable,
                            step_free: false,
                            evidence: EvidenceRef {
                                source_url: source_a.url.clone(),
                                snapshot_sha256: source_a.sha256.clone(),
                                locator: "#hall-b".into(),
                            },
                        },
                    ],
                },
            },
            OracleCase {
                case_id: "cards".into(),
                format_id: "html-cards-v1".into(),
                input: json!({"url": source_b.url, "date": "2026-10-23"}),
                source: source_b.clone(),
                expected: ExtractionOutcome::Supported {
                    venues: vec![Venue {
                        venue_id: "garden".into(),
                        capacity: 140,
                        price: Money {
                            minor_units: 12500,
                            currency: "EUR".into(),
                        },
                        window: AvailabilityWindow {
                            start: "2026-10-23T10:00:00+02:00".into(),
                            end: "2026-10-23T14:00:00+02:00".into(),
                        },
                        availability: Availability::Available,
                        step_free: true,
                        evidence: EvidenceRef {
                            source_url: source_b.url.clone(),
                            snapshot_sha256: source_b.sha256.clone(),
                            locator: "#garden".into(),
                        },
                    }],
                },
            },
            OracleCase {
                case_id: "missing-hours".into(),
                format_id: "html-incomplete-v1".into(),
                input: json!({"url": source_c.url, "date": "2026-10-24"}),
                source: source_c.clone(),
                expected: ExtractionOutcome::Unsupported {
                    reason: UnsupportedReason::MissingRequiredData,
                    evidence: EvidenceRef {
                        source_url: source_c.url.clone(),
                        snapshot_sha256: source_c.sha256.clone(),
                        locator: "#missing-hours".into(),
                    },
                },
            },
        ],
    };
    let artifact = ArtifactPin {
        capability_id: "venue-reader".into(),
        capability_version: "1.0.0".into(),
        source_sha256: "a".repeat(64),
        actor_id: "actor-venue-reader".into(),
        build_id: "build-0001".into(),
        build_number: "0.0.1".into(),
    };
    let outputs = [
        json!({
            "status": "supported",
            "venues": [
                {
                    "venue_id": "hall-a", "capacity": 160,
                    "price": {"minor_units": 45000, "currency": "USD"},
                    "window": {"start": "2026-10-22T09:00:00-04:00", "end": "2026-10-22T13:00:00-04:00"},
                    "availability": "available", "step_free": true,
                    "evidence": {"source_url": "https://campus.example/venues/table", "snapshot_sha256": "1".repeat(64), "locator": "#hall-a"}
                },
                {
                    "venue_id": "hall-b", "capacity": 220,
                    "price": {"minor_units": 60000, "currency": "USD"},
                    "window": {"start": "2026-10-22T09:00:00-04:00", "end": "2026-10-22T13:00:00-04:00"},
                    "availability": "unavailable", "step_free": false,
                    "evidence": {"source_url": "https://campus.example/venues/table", "snapshot_sha256": "1".repeat(64), "locator": "#hall-b"}
                }
            ]
        }),
        json!({
            "status": "supported",
            "venues": [{
                "venue_id": "garden", "capacity": 140,
                "price": {"minor_units": 12500, "currency": "EUR"},
                "window": {"start": "2026-10-23T10:00:00+02:00", "end": "2026-10-23T14:00:00+02:00"},
                "availability": "available", "step_free": true,
                "evidence": {"source_url": "https://campus.example/venues/cards", "snapshot_sha256": "2".repeat(64), "locator": "#garden"}
            }]
        }),
        json!({
            "status": "unsupported", "reason": "missing_required_data",
            "evidence": {"source_url": "https://campus.example/venues/incomplete", "snapshot_sha256": "3".repeat(64), "locator": "#missing-hours"}
        }),
    ];
    let evidence = suite
        .cases
        .iter()
        .zip(outputs)
        .enumerate()
        .map(|(i, (case, output))| ExecutionEvidence {
            case_id: case.case_id.clone(),
            artifact: artifact.clone(),
            input_sha256: json_sha256(&case.input),
            source: case.source.clone(),
            run_id: format!("run-{i}"),
            dataset_id: format!("dataset-{i}"),
            status: RunStatus::Succeeded,
            output,
        })
        .collect();
    (suite, artifact, evidence)
}

fn rejected_with(
    suite: &OracleSuite,
    artifact: &ArtifactPin,
    runs: &[ExecutionEvidence],
    code: IssueCode,
) {
    let result =
        verify(suite, artifact, runs).expect("trusted fixture configuration must remain valid");
    assert!(
        !result.is_accepted(),
        "mutated evidence was accepted: {code:?}"
    );
    assert_eq!(result.report().decision, Decision::Rejected);
    let issues = result.report().issues.iter().chain(
        result
            .report()
            .cases
            .iter()
            .flat_map(|case| case.issues.iter()),
    );
    assert!(
        issues.into_iter().any(|issue| issue.code == code),
        "expected {code:?}, got {:?}",
        result.report()
    );
}

#[test]
fn accepts_independently_authored_outputs_and_scopes_report_to_exact_evidence() {
    let (suite, artifact, runs) = fixture();
    let result = verify(&suite, &artifact, &runs).unwrap();
    assert!(result.is_accepted(), "{:?}", result.report());
    let report = result.report();
    assert_eq!(report.decision, Decision::Accepted);
    assert_eq!(report.artifact, artifact);
    assert_eq!(report.suite_id, suite.suite_id);
    assert_eq!(report.scope, suite.scope);
    assert_eq!(
        report.suite_sha256,
        json_sha256(&serde_json::to_value(&suite).unwrap())
    );
    assert!(!report.evaluator_version.is_empty());
    assert!(report.issues.is_empty());
    assert_eq!(report.cases.len(), 3);
    for expected in &runs {
        let case = report
            .cases
            .iter()
            .find(|case| case.case_id == expected.case_id)
            .unwrap();
        assert_eq!(case.input_sha256, expected.input_sha256);
        assert_eq!(case.source, expected.source);
        assert_eq!(case.run_id.as_deref(), Some(expected.run_id.as_str()));
        assert_eq!(
            case.dataset_id.as_deref(),
            Some(expected.dataset_id.as_str())
        );
        assert!(case.issues.is_empty());
    }
}

#[test]
fn evidence_order_and_venue_order_do_not_change_acceptance() {
    let (suite, artifact, mut runs) = fixture();
    runs[0].output["venues"].as_array_mut().unwrap().reverse();
    runs.reverse();
    assert!(verify(&suite, &artifact, &runs).unwrap().is_accepted());
}

#[test]
fn equivalent_offset_timestamps_are_the_same_instant() {
    let (suite, artifact, mut runs) = fixture();
    runs[0].output["venues"][0]["window"]["start"] = json!("2026-10-22T13:00:00Z");
    runs[0].output["venues"][0]["window"]["end"] = json!("2026-10-22T17:00:00.000+00:00");
    assert!(verify(&suite, &artifact, &runs).unwrap().is_accepted());
}

#[test]
fn every_artifact_identity_field_is_bound() {
    for field in [
        "capability_id",
        "capability_version",
        "source_sha256",
        "actor_id",
        "build_id",
        "build_number",
    ] {
        let (suite, artifact, mut runs) = fixture();
        let mut pin = serde_json::to_value(&artifact).unwrap();
        pin[field] = if field == "source_sha256" {
            json!("b".repeat(64))
        } else {
            json!("different-value")
        };
        runs[1].artifact = serde_json::from_value(pin).unwrap();
        rejected_with(&suite, &artifact, &runs, IssueCode::ArtifactMismatch);
    }
}

#[test]
fn output_claims_cannot_override_the_trusted_artifact_pin() {
    let (suite, artifact, mut runs) = fixture();
    runs[0].artifact.build_id = "unapproved-build".into();
    runs[0].output["artifact"] = serde_json::to_value(&artifact).unwrap();
    rejected_with(&suite, &artifact, &runs, IssueCode::ArtifactMismatch);
}

#[test]
fn binds_input_and_both_source_snapshot_fields() {
    let (suite, artifact, original) = fixture();
    let mut runs = original.clone();
    runs[0].input_sha256 = "f".repeat(64);
    rejected_with(&suite, &artifact, &runs, IssueCode::InputMismatch);
    let mut runs = original.clone();
    runs[0].source.url = "https://other.example/venues".into();
    rejected_with(&suite, &artifact, &runs, IssueCode::SourceMismatch);
    let mut runs = original;
    runs[0].source.sha256 = "f".repeat(64);
    rejected_with(&suite, &artifact, &runs, IssueCode::SourceMismatch);
}

#[test]
fn failed_and_unknown_runs_do_not_pass_even_with_correct_output() {
    for status in [RunStatus::Failed, RunStatus::Unknown] {
        let (suite, artifact, mut runs) = fixture();
        runs[0].status = status;
        rejected_with(&suite, &artifact, &runs, IssueCode::RunNotSucceeded);
    }
}

#[test]
fn missing_duplicate_and_unknown_cases_are_rejected() {
    let (suite, artifact, original) = fixture();
    let mut runs = original.clone();
    runs.remove(0);
    rejected_with(&suite, &artifact, &runs, IssueCode::MissingCase);
    let mut runs = original.clone();
    let mut duplicate = runs[0].clone();
    duplicate.run_id = "extra-run".into();
    duplicate.dataset_id = "extra-dataset".into();
    runs.push(duplicate);
    rejected_with(&suite, &artifact, &runs, IssueCode::DuplicateCase);
    let mut runs = original;
    runs[0].case_id = "not-in-oracle".into();
    rejected_with(&suite, &artifact, &runs, IssueCode::UnknownCase);
}

#[test]
fn empty_evidence_never_vacuously_passes() {
    let (suite, artifact, _) = fixture();
    rejected_with(&suite, &artifact, &[], IssueCode::MissingCase);
}

#[test]
fn run_and_dataset_receipts_must_each_be_nonempty_and_distinct() {
    let (suite, artifact, original) = fixture();
    for field in ["run_id", "dataset_id"] {
        for empty in ["", "   "] {
            let mut runs = original.clone();
            if field == "run_id" {
                runs[0].run_id = empty.into();
            } else {
                runs[0].dataset_id = empty.into();
            }
            rejected_with(&suite, &artifact, &runs, IssueCode::InvalidReceipt);
        }
        let mut runs = original.clone();
        if field == "run_id" {
            runs[1].run_id = runs[0].run_id.clone();
        } else {
            runs[1].dataset_id = runs[0].dataset_id.clone();
        }
        rejected_with(&suite, &artifact, &runs, IssueCode::ReusedReceipt);
    }
}

#[test]
fn exact_venue_set_rejects_missing_invented_and_duplicate_rows() {
    let (suite, artifact, original) = fixture();
    let mut runs = original.clone();
    runs[0].output["venues"].as_array_mut().unwrap().pop();
    rejected_with(&suite, &artifact, &runs, IssueCode::MissingVenue);
    let mut runs = original.clone();
    runs[0].output["venues"][1]["venue_id"] = json!("invented-hall");
    rejected_with(&suite, &artifact, &runs, IssueCode::UnexpectedVenue);
    let mut runs = original;
    let duplicate = runs[0].output["venues"][0].clone();
    runs[0].output["venues"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    rejected_with(&suite, &artifact, &runs, IssueCode::DuplicateVenue);
}

#[test]
fn empty_supported_output_is_not_a_missing_data_answer() {
    let (suite, artifact, mut runs) = fixture();
    runs[2].output = json!({"status": "supported", "venues": []});
    let result = verify(&suite, &artifact, &runs).unwrap();
    assert!(!result.is_accepted());
}

#[test]
fn plausible_but_wrong_values_cannot_pass_mission_thresholds() {
    let mutations = [
        ("/venues/0/capacity", json!(170)),
        ("/venues/0/price/minor_units", json!(450)),
        ("/venues/0/price/currency", json!("EUR")),
        ("/venues/0/window/start", json!("2026-10-23T09:00:00-04:00")),
        ("/venues/0/window/start", json!("2026-10-22T09:00:00+00:00")),
        ("/venues/0/window/end", json!("2026-10-22T12:59:59-04:00")),
        ("/venues/1/availability", json!("available")),
        ("/venues/0/step_free", json!(false)),
    ];
    for (path, replacement) in mutations {
        let (suite, artifact, mut runs) = fixture();
        *runs[0].output.pointer_mut(path).unwrap() = replacement;
        // A moved start may also make the window invalid, which is still a
        // substantive rejection rather than requiring one implementation path.
        let result = verify(&suite, &artifact, &runs).unwrap();
        assert!(!result.is_accepted(), "accepted incorrect field {path}");
    }
}

#[test]
fn swapping_row_values_does_not_pass_as_a_bag_of_facts() {
    let (suite, artifact, mut runs) = fixture();
    runs[0].output["venues"][0]["capacity"] = json!(220);
    runs[0].output["venues"][1]["capacity"] = json!(160);
    rejected_with(&suite, &artifact, &runs, IssueCode::ValueMismatch);
}

#[test]
fn every_source_evidence_component_is_checked_for_supported_and_unsupported_cases() {
    for case_index in [0, 2] {
        for (field, value) in [
            ("source_url", json!("https://different.example/venues")),
            ("snapshot_sha256", json!("f".repeat(64))),
            ("locator", json!("#different-row")),
        ] {
            let (suite, artifact, mut runs) = fixture();
            let evidence = if case_index == 0 {
                &mut runs[0].output["venues"][0]["evidence"]
            } else {
                &mut runs[2].output["evidence"]
            };
            evidence[field] = value;
            let result = verify(&suite, &artifact, &runs).unwrap();
            assert!(
                !result.is_accepted(),
                "accepted changed evidence {field} in case {case_index}"
            );
        }
    }
}

#[test]
fn unsupported_reason_is_a_fact_and_unavailable_is_not_unsupported() {
    let (suite, artifact, original) = fixture();
    let mut runs = original.clone();
    runs[2].output["reason"] = json!("unsupported_format");
    let result = verify(&suite, &artifact, &runs).unwrap();
    assert!(!result.is_accepted());
    let mut runs = original;
    runs[0].output = json!({
        "status": "unsupported", "reason": "missing_required_data",
        "evidence": {"source_url": "https://campus.example/venues/table", "snapshot_sha256": "1".repeat(64), "locator": "#hall-b"}
    });
    rejected_with(&suite, &artifact, &runs, IssueCode::OutcomeMismatch);
}

#[test]
fn invalid_scalar_types_and_missing_fields_are_malformed_not_coerced() {
    let mutations = [
        ("/venues/0/capacity", json!("160")),
        ("/venues/0/capacity", json!(-1)),
        ("/venues/0/capacity", json!(4_294_967_296_u64)),
        ("/venues/0/price/minor_units", json!(45000.5)),
        ("/venues/0/price/minor_units", json!(-1)),
        ("/venues/0/availability", json!("probably")),
        ("/venues/0/step_free", Value::Null),
    ];
    for (path, replacement) in mutations {
        let (suite, artifact, mut runs) = fixture();
        *runs[0].output.pointer_mut(path).unwrap() = replacement;
        rejected_with(&suite, &artifact, &runs, IssueCode::MalformedOutput);
    }
    let (suite, artifact, mut runs) = fixture();
    runs[0].output["venues"][0]
        .as_object_mut()
        .unwrap()
        .remove("evidence");
    rejected_with(&suite, &artifact, &runs, IssueCode::MalformedOutput);
}

#[test]
fn unknown_output_fields_are_rejected_at_each_object_boundary() {
    for pointer in [
        "",
        "/venues/0",
        "/venues/0/price",
        "/venues/0/window",
        "/venues/0/evidence",
    ] {
        let (suite, artifact, mut runs) = fixture();
        runs[0]
            .output
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("safe".into(), json!(true));
        rejected_with(&suite, &artifact, &runs, IssueCode::MalformedOutput);
    }
    let (suite, artifact, mut runs) = fixture();
    runs[2].output["approved"] = json!(true);
    rejected_with(&suite, &artifact, &runs, IssueCode::MalformedOutput);
}

#[test]
fn invalid_time_windows_and_unzoned_times_are_rejected() {
    for (start, end) in [
        ("2026-10-22T09:00:00", "2026-10-22T13:00:00-04:00"),
        ("not-a-date", "2026-10-22T13:00:00-04:00"),
        ("2026-10-22T13:00:00-04:00", "2026-10-22T09:00:00-04:00"),
        ("2026-10-22T09:00:00-04:00", "2026-10-22T09:00:00-04:00"),
    ] {
        let (suite, artifact, mut runs) = fixture();
        runs[0].output["venues"][0]["window"] = json!({"start": start, "end": end});
        rejected_with(&suite, &artifact, &runs, IssueCode::InvalidOutput);
    }
}

#[test]
fn malformed_untrusted_output_remains_rejection_not_configuration_error() {
    for output in [
        Value::Null,
        json!(true),
        json!([]),
        json!("success"),
        json!({}),
    ] {
        let (suite, artifact, mut runs) = fixture();
        runs[0].output = output;
        rejected_with(&suite, &artifact, &runs, IssueCode::MalformedOutput);
    }
}

#[test]
fn suite_requires_both_format_diversity_and_an_unsupported_case() {
    let (suite, artifact, runs) = fixture();
    let mut changed = suite.clone();
    changed.cases.clear();
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut changed = suite.clone();
    changed.schema_version = 2;
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut changed = suite.clone();
    changed.cases.pop();
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut changed = suite.clone();
    changed.cases.remove(0);
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut changed = suite;
    changed.cases[1].format_id = changed.cases[0].format_id.clone();
    assert!(verify(&changed, &artifact, &runs).is_err());
}

#[test]
fn duplicate_case_identifiers_and_duplicate_inputs_invalidate_the_oracle() {
    let (suite, artifact, runs) = fixture();
    let mut changed = suite.clone();
    changed.cases[1].case_id = changed.cases[0].case_id.clone();
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut changed = suite;
    changed.cases[1].input = changed.cases[0].input.clone();
    assert!(verify(&changed, &artifact, &runs).is_err());
}

#[test]
fn invalid_trusted_source_hash_and_inconsistent_oracle_evidence_are_configuration_errors() {
    for bad_digest in ["", "abc", &"g".repeat(64)] {
        let (mut suite, artifact, runs) = fixture();
        suite.cases[0].source.sha256 = bad_digest.into();
        assert!(verify(&suite, &artifact, &runs).is_err());
    }
    for unsupported in [false, true] {
        let (mut suite, artifact, runs) = fixture();
        let case_index = if unsupported { 2 } else { 0 };
        match &mut suite.cases[case_index].expected {
            ExtractionOutcome::Supported { venues } => {
                venues[0].evidence.snapshot_sha256 = "b".repeat(64)
            }
            ExtractionOutcome::Unsupported { evidence, .. } => {
                evidence.snapshot_sha256 = "b".repeat(64)
            }
        }
        assert!(verify(&suite, &artifact, &runs).is_err());
    }
}

#[test]
fn impossible_and_duplicate_oracle_facts_cannot_define_acceptance() {
    let (suite, artifact, runs) = fixture();
    let mut changed = suite.clone();
    if let ExtractionOutcome::Supported { venues } = &mut changed.cases[0].expected {
        venues[0].window.end = venues[0].window.start.clone();
    }
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut changed = suite;
    if let ExtractionOutcome::Supported { venues } = &mut changed.cases[0].expected {
        venues.push(venues[0].clone());
    }
    assert!(verify(&changed, &artifact, &runs).is_err());
}

#[test]
fn suite_digest_changes_when_acceptance_facts_or_scope_change() {
    let (suite, artifact, runs) = fixture();
    let report = verify(&suite, &artifact, &runs).unwrap().report().clone();
    let mut changed = suite.clone();
    changed.scope = "Different scope".into();
    let changed_report = verify(&changed, &artifact, &runs).unwrap().report().clone();
    assert_ne!(report.suite_sha256, changed_report.suite_sha256);
    let mut changed = suite;
    if let ExtractionOutcome::Supported { venues } = &mut changed.cases[0].expected {
        venues[0].capacity = 161;
    }
    let changed_report = verify(&changed, &artifact, &runs).unwrap().report().clone();
    assert_ne!(report.suite_sha256, changed_report.suite_sha256);
}

#[test]
fn diagnostics_do_not_leak_oracle_answers_or_raw_candidate_text() {
    let (mut suite, artifact, mut runs) = fixture();
    if let ExtractionOutcome::Supported { venues } = &mut suite.cases[0].expected {
        venues[0].evidence.locator = "private-oracle-answer-canary".into();
    }
    runs[0].output["venues"][0]["evidence"]["locator"] = json!("raw-candidate-canary");
    let result = verify(&suite, &artifact, &runs).unwrap();
    assert!(!result.is_accepted());
    let serialized = serde_json::to_string(result.report()).unwrap();
    assert!(!serialized.contains("private-oracle-answer-canary"));
    assert!(!serialized.contains("raw-candidate-canary"));
}

#[test]
fn envelope_unknown_fields_do_not_silently_extend_the_contract() {
    let (suite, artifact, runs) = fixture();
    let mut value = serde_json::to_value(suite).unwrap();
    value["safe"] = json!(true);
    assert!(serde_json::from_value::<OracleSuite>(value).is_err());
    let mut value = serde_json::to_value(artifact).unwrap();
    value["latest"] = json!(true);
    assert!(serde_json::from_value::<ArtifactPin>(value).is_err());
    let mut value = serde_json::to_value(&runs[0]).unwrap();
    value["approved"] = json!(true);
    assert!(serde_json::from_value::<ExecutionEvidence>(value).is_err());
}

#[test]
fn json_hash_is_object_order_independent_but_array_order_sensitive() {
    let left: Value = serde_json::from_str(r#"{"a":1,"nested":{"z":3,"y":2}}"#).unwrap();
    let right: Value = serde_json::from_str(r#"{"nested":{"y":2,"z":3},"a":1}"#).unwrap();
    assert_eq!(json_sha256(&left), json_sha256(&right));
    assert_ne!(json_sha256(&json!([1, 2])), json_sha256(&json!([2, 1])));
}

#[test]
fn mutable_build_tags_and_invalid_trusted_source_digests_are_configuration_errors() {
    for build in ["latest", "default", "0.1", "0.0.1-beta", "0.0.-1"] {
        let (suite, mut artifact, runs) = fixture();
        artifact.build_number = build.into();
        assert!(
            verify(&suite, &artifact, &runs).is_err(),
            "accepted mutable/invalid build {build}"
        );
    }
    for digest in ["", "no-source", &"A".repeat(64)] {
        let (suite, mut artifact, runs) = fixture();
        artifact.source_sha256 = digest.into();
        assert!(verify(&suite, &artifact, &runs).is_err());
    }
}

#[test]
fn naming_the_same_snapshot_as_two_formats_does_not_establish_coverage() {
    let (mut suite, artifact, runs) = fixture();
    suite.cases[1].source.sha256 = suite.cases[0].source.sha256.clone();
    let digest = suite.cases[1].source.sha256.clone();
    if let ExtractionOutcome::Supported { venues } = &mut suite.cases[1].expected {
        venues[0].evidence.snapshot_sha256 = digest;
    }
    assert!(verify(&suite, &artifact, &runs).is_err());
}

#[test]
fn oversized_wide_and_deep_outputs_fail_closed_without_panicking() {
    let (suite, artifact, original) = fixture();
    let mut runs = original.clone();
    runs[0].output["venues"][0]["evidence"]["locator"] = json!("x".repeat(1_048_577));
    rejected_with(&suite, &artifact, &runs, IssueCode::InvalidOutput);
    let mut runs = original.clone();
    runs[0].output = Value::Array(vec![Value::Null; 50_001]);
    rejected_with(&suite, &artifact, &runs, IssueCode::InvalidOutput);
    let mut nested = Value::Null;
    for _ in 0..40 {
        nested = Value::Array(vec![nested]);
    }
    let mut runs = original;
    runs[0].output = nested;
    rejected_with(&suite, &artifact, &runs, IssueCode::InvalidOutput);
}

#[test]
fn excessive_receipt_batches_are_rejected_before_per_case_work() {
    let (suite, artifact, runs) = fixture();
    rejected_with(
        &suite,
        &artifact,
        &vec![runs[0].clone(); 129],
        IssueCode::InvalidReceipt,
    );
}

#[test]
fn excessive_or_deep_trusted_inputs_are_configuration_errors() {
    let (suite, artifact, runs) = fixture();
    let mut changed = suite.clone();
    changed.cases[0].input = json!({"data": "x".repeat(1_048_577)});
    assert!(verify(&changed, &artifact, &runs).is_err());
    let mut nested = Value::Null;
    for _ in 0..40 {
        nested = json!({"next": nested});
    }
    let mut changed = suite;
    changed.cases[0].input = nested;
    assert!(verify(&changed, &artifact, &runs).is_err());
}

#[test]
fn oracle_cannot_require_output_that_exceeds_candidate_limits() {
    let (mut suite, artifact, runs) = fixture();
    if let ExtractionOutcome::Supported { venues } = &mut suite.cases[0].expected {
        let base = venues[0].clone();
        *venues = (0..1_000)
            .map(|index| {
                let mut row = base.clone();
                row.venue_id = format!("{index:04}{}", "x".repeat(1020));
                row.evidence.locator = "y".repeat(1024);
                row
            })
            .collect();
    }
    let error = verify(&suite, &artifact, &runs).unwrap_err();
    assert!(error.path.ends_with("expected"));
}

#[test]
fn rejection_paths_point_to_candidate_rows_after_reordering() {
    let (suite, artifact, mut runs) = fixture();
    runs[0].output["venues"].as_array_mut().unwrap().reverse();
    runs[0].output["venues"][1]["capacity"] = json!(161);
    let outcome = verify(&suite, &artifact, &runs).unwrap();
    assert_eq!(
        outcome.report().cases[0].issues,
        vec![Issue {
            code: IssueCode::ValueMismatch,
            path: "output/venues/1/capacity".into(),
        }]
    );
}
