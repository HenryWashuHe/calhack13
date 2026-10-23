use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pin supplied by the trusted forge adapter, never by generated Actor output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPin {
    pub capability_id: String,
    pub capability_version: String,
    pub source_sha256: String,
    pub actor_id: String,
    pub build_id: String,
    pub build_number: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    pub url: String,
    pub sha256: String,
}

/// Kept outside generated source and prompts. This is the independent oracle.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleSuite {
    pub schema_version: u32,
    pub suite_id: String,
    pub scope: String,
    pub cases: Vec<OracleCase>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleCase {
    pub case_id: String,
    /// Distinct supported formats are required; a URL change is not a format.
    pub format_id: String,
    pub input: Value,
    pub source: SourceSnapshot,
    pub expected: ExtractionOutcome,
}

/// Metadata must come from an authenticated adapter. Only `output` is Actor data.
/// Deserializing this structure does not authenticate a receipt or prove a run.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEvidence {
    pub case_id: String,
    pub artifact: ArtifactPin,
    pub input_sha256: String,
    pub source: SourceSnapshot,
    pub run_id: String,
    pub dataset_id: String,
    pub status: RunStatus,
    pub output: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionOutcome {
    Supported { venues: Vec<Venue> },
    Unsupported { reason: UnsupportedReason, evidence: EvidenceRef },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    MissingRequiredData,
    UnsupportedFormat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Venue {
    pub venue_id: String,
    pub capacity: u32,
    pub price: Money,
    pub window: AvailabilityWindow,
    pub availability: Availability,
    /// A fixture-declared attribute, not an independently audited accessibility claim.
    pub step_free: bool,
    pub evidence: EvidenceRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Money {
    pub minor_units: u64,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvailabilityWindow {
    pub start: String,
    pub end: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub source_url: String,
    pub snapshot_sha256: String,
    pub locator: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    MissingCase,
    DuplicateCase,
    UnknownCase,
    ReusedReceipt,
    InvalidReceipt,
    ArtifactMismatch,
    InputMismatch,
    SourceMismatch,
    RunNotSucceeded,
    MalformedOutput,
    InvalidOutput,
    OutcomeMismatch,
    MissingVenue,
    UnexpectedVenue,
    DuplicateVenue,
    ValueMismatch,
}

/// Failures disclose field paths, never oracle answers or raw provider output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseReport {
    pub case_id: String,
    pub input_sha256: String,
    pub source: SourceSnapshot,
    pub run_id: Option<String>,
    pub dataset_id: Option<String>,
    pub issues: Vec<Issue>,
}

/// Scoped semantic evidence only. It grants no publication or execution authority.
#[derive(Clone, Debug, Serialize)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub evaluator_version: String,
    pub decision: Decision,
    pub scope: String,
    pub suite_id: String,
    pub suite_sha256: String,
    pub artifact: ArtifactPin,
    pub issues: Vec<Issue>,
    pub cases: Vec<CaseReport>,
}

/// Constructible only by a successful verifier call; not deserializable from a claim.
#[derive(Clone, Debug)]
pub struct AcceptedVerification {
    pub(crate) report: VerificationReport,
}

impl AcceptedVerification {
    pub fn report(&self) -> &VerificationReport { &self.report }
    pub fn artifact(&self) -> &ArtifactPin { &self.report.artifact }
}

#[derive(Clone, Debug)]
pub enum VerificationOutcome {
    Accepted(AcceptedVerification),
    Rejected(VerificationReport),
}

impl VerificationOutcome {
    pub fn report(&self) -> &VerificationReport {
        match self {
            Self::Accepted(accepted) => accepted.report(),
            Self::Rejected(report) => report,
        }
    }
    pub fn is_accepted(&self) -> bool { matches!(self, Self::Accepted(_)) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigurationError {
    pub path: String,
    pub reason: &'static str,
}

impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

impl std::error::Error for ConfigurationError {}
