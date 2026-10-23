use crate::model::*;
use chrono::DateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_CASES: usize = 128;
const MAX_VENUES: usize = 1_000;
const MAX_JSON_NODES: usize = 50_000;
const MAX_JSON_BYTES: usize = 1_048_576;
const MAX_JSON_DEPTH: usize = 32;

/// SHA-256 of compact JSON with recursively sorted object keys. Array order is
/// preserved. This is GENESIS's version-1 JSON digest, not RFC 8785. Callers must
/// bound externally supplied JSON before calling (the verifier does so itself).
pub fn json_sha256(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let ordered: BTreeMap<_, _> = map.iter().collect();
                Value::Object(
                    ordered
                        .into_iter()
                        .map(|(k, v)| (k.clone(), sorted(v)))
                        .collect(),
                )
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    format!("{:x}", Sha256::digest(sorted(value).to_string().as_bytes()))
}

/// Accept only complete, independent fixture evidence for the exact expected
/// artifact. Receipt metadata is trusted adapter input, not Actor self-report.
pub fn verify(
    suite: &OracleSuite,
    expected_artifact: &ArtifactPin,
    runs: &[ExecutionEvidence],
) -> Result<VerificationOutcome, ConfigurationError> {
    validate_artifact(expected_artifact)?;
    validate_suite(suite)?;
    let suite_json =
        serde_json::to_value(suite).map_err(|_| config("suite", "cannot serialize oracle"))?;
    let mut report = VerificationReport {
        schema_version: 1,
        evaluator_version: env!("CARGO_PKG_VERSION").to_owned(),
        decision: Decision::Rejected,
        scope: suite.scope.clone(),
        suite_id: suite.suite_id.clone(),
        suite_sha256: json_sha256(&suite_json),
        artifact: expected_artifact.clone(),
        issues: Vec::new(),
        cases: Vec::new(),
    };

    // Bound receipt count before building maps or cloning untrusted data.
    if runs.len() > MAX_CASES {
        issue(&mut report.issues, IssueCode::InvalidReceipt, "runs");
        return Ok(VerificationOutcome::Rejected(report));
    }
    let known: BTreeSet<_> = suite.cases.iter().map(|c| c.case_id.as_str()).collect();
    let mut by_case: BTreeMap<&str, Vec<&ExecutionEvidence>> = BTreeMap::new();
    let mut run_ids = BTreeSet::new();
    let mut dataset_ids = BTreeSet::new();
    for (index, run) in runs.iter().enumerate() {
        if !known.contains(run.case_id.as_str()) {
            issue(
                &mut report.issues,
                IssueCode::UnknownCase,
                format!("runs/{index}/case_id"),
            );
        }
        if !text_valid(&run.run_id) || !text_valid(&run.dataset_id) {
            issue(
                &mut report.issues,
                IssueCode::InvalidReceipt,
                format!("runs/{index}"),
            );
        }
        if !run_ids.insert(&run.run_id) || !dataset_ids.insert(&run.dataset_id) {
            issue(
                &mut report.issues,
                IssueCode::ReusedReceipt,
                format!("runs/{index}"),
            );
        }
        by_case.entry(&run.case_id).or_default().push(run);
    }
    for case in &suite.cases {
        let mut result = CaseReport {
            case_id: case.case_id.clone(),
            input_sha256: json_sha256(&case.input),
            source: case.source.clone(),
            run_id: None,
            dataset_id: None,
            issues: Vec::new(),
        };
        match by_case.get(case.case_id.as_str()).map(Vec::as_slice) {
            None | Some([]) => issue(&mut result.issues, IssueCode::MissingCase, "case"),
            Some([run]) => {
                if text_valid(&run.run_id) {
                    result.run_id = Some(run.run_id.clone());
                }
                if text_valid(&run.dataset_id) {
                    result.dataset_id = Some(run.dataset_id.clone());
                }
                check_run(case, expected_artifact, run, &mut result);
            }
            Some(_) => issue(&mut result.issues, IssueCode::DuplicateCase, "case"),
        }
        report.cases.push(result);
    }
    if report.issues.is_empty() && report.cases.iter().all(|c| c.issues.is_empty()) {
        report.decision = Decision::Accepted;
        Ok(VerificationOutcome::Accepted(AcceptedVerification {
            report,
        }))
    } else {
        Ok(VerificationOutcome::Rejected(report))
    }
}

fn check_run(
    case: &OracleCase,
    pin: &ArtifactPin,
    run: &ExecutionEvidence,
    result: &mut CaseReport,
) {
    if &run.artifact != pin {
        issue(&mut result.issues, IssueCode::ArtifactMismatch, "artifact");
    }
    if run.input_sha256 != result.input_sha256 {
        issue(&mut result.issues, IssueCode::InputMismatch, "input_sha256");
    }
    if run.source != case.source {
        issue(&mut result.issues, IssueCode::SourceMismatch, "source");
    }
    if run.status != RunStatus::Succeeded {
        issue(&mut result.issues, IssueCode::RunNotSucceeded, "status");
    }
    if !bounded_json(&run.output) {
        issue(&mut result.issues, IssueCode::InvalidOutput, "output");
        return;
    }
    let actual = match serde_json::from_value::<ExtractionOutcome>(run.output.clone()) {
        Ok(actual) => actual,
        Err(_) => {
            issue(&mut result.issues, IssueCode::MalformedOutput, "output");
            return;
        }
    };
    if let Err(error) = validate_output(&actual, &case.source) {
        issue(&mut result.issues, IssueCode::InvalidOutput, error.path);
    }
    compare_output(&case.expected, &actual, &mut result.issues);
}

fn compare_output(
    expected: &ExtractionOutcome,
    actual: &ExtractionOutcome,
    issues: &mut Vec<Issue>,
) {
    match (expected, actual) {
        (
            ExtractionOutcome::Unsupported {
                reason: er,
                evidence: ee,
            },
            ExtractionOutcome::Unsupported {
                reason: ar,
                evidence: ae,
            },
        ) => {
            compare(er, ar, "output/reason", issues);
            compare(ee, ae, "output/evidence", issues);
        }
        (
            ExtractionOutcome::Supported { venues: expected },
            ExtractionOutcome::Supported { venues: actual },
        ) => {
            let expected_ids: BTreeSet<_> = expected.iter().map(|v| &v.venue_id).collect();
            let mut actual_by_id = BTreeMap::new();
            for (index, row) in actual.iter().enumerate() {
                if actual_by_id.insert(&row.venue_id, (index, row)).is_some() {
                    issue(
                        issues,
                        IssueCode::DuplicateVenue,
                        format!("output/venues/{index}/venue_id"),
                    );
                }
                if !expected_ids.contains(&row.venue_id) {
                    issue(
                        issues,
                        IssueCode::UnexpectedVenue,
                        format!("output/venues/{index}/venue_id"),
                    );
                }
            }
            for expected in expected {
                let Some((index, actual)) = actual_by_id.get(&expected.venue_id) else {
                    issue(issues, IssueCode::MissingVenue, "output/venues");
                    continue;
                };
                let path = format!("output/venues/{index}");
                compare(
                    &expected.capacity,
                    &actual.capacity,
                    &format!("{path}/capacity"),
                    issues,
                );
                compare(
                    &expected.price,
                    &actual.price,
                    &format!("{path}/price"),
                    issues,
                );
                compare(
                    &expected.availability,
                    &actual.availability,
                    &format!("{path}/availability"),
                    issues,
                );
                compare(
                    &expected.step_free,
                    &actual.step_free,
                    &format!("{path}/step_free"),
                    issues,
                );
                compare(
                    &expected.evidence,
                    &actual.evidence,
                    &format!("{path}/evidence"),
                    issues,
                );
                // Compare instants: equivalent explicit offsets are valid.
                for (field, exp, act) in [
                    ("start", &expected.window.start, &actual.window.start),
                    ("end", &expected.window.end, &actual.window.end),
                ] {
                    if !matches!((DateTime::parse_from_rfc3339(exp), DateTime::parse_from_rfc3339(act)),
                        (Ok(e), Ok(a)) if e == a)
                    {
                        issue(
                            issues,
                            IssueCode::ValueMismatch,
                            format!("{path}/window/{field}"),
                        );
                    }
                }
            }
        }
        _ => issue(issues, IssueCode::OutcomeMismatch, "output/status"),
    }
}

fn compare<T: PartialEq>(expected: &T, actual: &T, path: &str, issues: &mut Vec<Issue>) {
    if expected != actual {
        issue(issues, IssueCode::ValueMismatch, path);
    }
}

/// Validate a trusted exact artifact pin before dispatch.
pub fn validate_artifact(pin: &ArtifactPin) -> Result<(), ConfigurationError> {
    for (field, value) in [
        ("capability_id", &pin.capability_id),
        ("capability_version", &pin.capability_version),
        ("actor_id", &pin.actor_id),
        ("build_id", &pin.build_id),
    ] {
        if !text_valid(value) {
            return Err(config(
                format!("artifact/{field}"),
                "must be nonempty bounded text",
            ));
        }
    }
    if !digest_valid(&pin.source_sha256) {
        return Err(config(
            "artifact/source_sha256",
            "must be lowercase SHA-256 hex",
        ));
    }
    let parts: Vec<_> = pin.build_number.split('.').collect();
    if pin.build_number.len() > 64
        || parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(config(
            "artifact/build_number",
            "must pin a numeric major.minor.build, not a mutable tag",
        ));
    }
    Ok(())
}

/// Validate trusted oracle coverage and bounds before any provider side effect.
pub fn validate_suite(suite: &OracleSuite) -> Result<(), ConfigurationError> {
    if suite.schema_version != 1 {
        return Err(config("suite/schema_version", "unsupported schema version"));
    }
    if !text_valid(&suite.suite_id) || !text_valid(&suite.scope) {
        return Err(config(
            "suite",
            "suite ID and scope must be nonempty bounded text",
        ));
    }
    if suite.cases.is_empty() || suite.cases.len() > MAX_CASES {
        return Err(config(
            "suite/cases",
            "requires 1 to 128 cases, including required coverage",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    let mut supported_formats = BTreeSet::new();
    let mut supported_snapshots = BTreeSet::new();
    let mut unsupported = false;
    for (index, case) in suite.cases.iter().enumerate() {
        let path = format!("suite/cases/{index}");
        if !text_valid(&case.case_id) || !text_valid(&case.format_id) || !ids.insert(&case.case_id)
        {
            return Err(config(
                path,
                "case IDs must be valid and unique; format ID is required",
            ));
        }
        if !case.input.is_object() || !bounded_json(&case.input) {
            return Err(config(
                format!("{path}/input"),
                "requires a bounded JSON object",
            ));
        }
        if !inputs.insert(json_sha256(&case.input)) {
            return Err(config(format!("{path}/input"), "duplicate case input"));
        }
        if !url_valid(&case.source.url) || !digest_valid(&case.source.sha256) {
            return Err(config(
                format!("{path}/source"),
                "requires HTTP(S) URL and lowercase SHA-256 hex",
            ));
        }
        validate_output(&case.expected, &case.source)
            .map_err(|error| config(format!("{path}/expected/{}", error.path), error.reason))?;
        let expected_json = serde_json::to_value(&case.expected)
            .map_err(|_| config(format!("{path}/expected"), "cannot serialize oracle output"))?;
        if !bounded_json(&expected_json) {
            return Err(config(
                format!("{path}/expected"),
                "oracle output exceeds the same limits applied to candidates",
            ));
        }
        match &case.expected {
            ExtractionOutcome::Supported { .. } => {
                supported_formats.insert(&case.format_id);
                supported_snapshots.insert(&case.source.sha256);
            }
            ExtractionOutcome::Unsupported { .. } => unsupported = true,
        }
    }
    if supported_formats.len() < 2 || supported_snapshots.len() < 2 || !unsupported {
        return Err(config(
            "suite/cases",
            "requires two distinct supported formats/snapshots and an unsupported case",
        ));
    }
    Ok(())
}

fn validate_output(
    output: &ExtractionOutcome,
    source: &SourceSnapshot,
) -> Result<(), ConfigurationError> {
    match output {
        ExtractionOutcome::Unsupported { evidence, .. } => {
            validate_evidence(evidence, source, "output/evidence")
        }
        ExtractionOutcome::Supported { venues } => {
            if venues.is_empty() || venues.len() > MAX_VENUES {
                return Err(config("output/venues", "requires 1 to 1000 venues"));
            }
            let mut ids = BTreeSet::new();
            for (index, row) in venues.iter().enumerate() {
                let path = format!("output/venues/{index}");
                if !text_valid(&row.venue_id) || !ids.insert(&row.venue_id) {
                    return Err(config(
                        format!("{path}/venue_id"),
                        "venue IDs must be valid and unique",
                    ));
                }
                if row.price.currency.len() != 3
                    || !row.price.currency.bytes().all(|b| b.is_ascii_uppercase())
                {
                    return Err(config(
                        format!("{path}/price/currency"),
                        "requires a three-letter uppercase currency",
                    ));
                }
                if row.window.start.len() > 64 || row.window.end.len() > 64 {
                    return Err(config(
                        format!("{path}/window"),
                        "timestamp exceeds 64 bytes",
                    ));
                }
                let start = DateTime::parse_from_rfc3339(&row.window.start).map_err(|_| {
                    config(
                        format!("{path}/window/start"),
                        "requires RFC3339 with an explicit offset",
                    )
                })?;
                let end = DateTime::parse_from_rfc3339(&row.window.end).map_err(|_| {
                    config(
                        format!("{path}/window/end"),
                        "requires RFC3339 with an explicit offset",
                    )
                })?;
                if end <= start {
                    return Err(config(format!("{path}/window"), "end must be after start"));
                }
                validate_evidence(&row.evidence, source, &format!("{path}/evidence"))?;
            }
            Ok(())
        }
    }
}

fn validate_evidence(
    evidence: &EvidenceRef,
    source: &SourceSnapshot,
    path: &str,
) -> Result<(), ConfigurationError> {
    if evidence.source_url != source.url
        || evidence.snapshot_sha256 != source.sha256
        || !text_valid(&evidence.locator)
    {
        return Err(config(
            path,
            "evidence must reference this source snapshot and a nonempty locator",
        ));
    }
    Ok(())
}

fn text_valid(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 1024
        && text.trim() == text
        && !text.chars().any(char::is_control)
}

fn digest_valid(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn url_valid(value: &str) -> bool {
    if !text_valid(value) {
        return false;
    }
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
    })
}

fn bounded_json(value: &Value) -> bool {
    let mut pending = vec![(value, 0)];
    let mut nodes = 0;
    let mut bytes = 0;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_JSON_NODES || depth > MAX_JSON_DEPTH {
            return false;
        }
        match value {
            Value::Object(map) => {
                if map.len() > MAX_JSON_NODES - nodes - pending.len() {
                    return false;
                }
                bytes += map.keys().map(String::len).sum::<usize>();
                pending.extend(map.values().map(|v| (v, depth + 1)));
            }
            Value::Array(items) => {
                if items.len() > MAX_JSON_NODES - nodes - pending.len() {
                    return false;
                }
                pending.extend(items.iter().map(|v| (v, depth + 1)));
            }
            Value::String(s) => bytes += s.len(),
            _ => bytes += 16,
        }
        if bytes > MAX_JSON_BYTES || pending.len() > MAX_JSON_NODES {
            return false;
        }
    }
    true
}

fn config(path: impl Into<String>, reason: &'static str) -> ConfigurationError {
    ConfigurationError {
        path: path.into(),
        reason,
    }
}

fn issue(issues: &mut Vec<Issue>, code: IssueCode, path: impl Into<String>) {
    issues.push(Issue {
        code,
        path: path.into(),
    });
}
