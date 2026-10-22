use genesis_verification::json_sha256;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
use std::{fmt, io::Write, time::Duration};

pub(crate) const MAX_INPUT: usize = 256 * 1024;
pub(crate) const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Limits {
    /// Includes the account quota wait, HTTP transfer, and response decoding.
    pub request_timeout: Duration,
    pub max_input_bytes: usize,
    pub max_response_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_input_bytes: MAX_INPUT,
            max_response_bytes: MAX_RESPONSE,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentRequest {
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub agent_id: String,
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}
impl fmt::Debug for RunRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunRequest")
            .field("agent_id", &self.agent_id)
            .field("input", &"[redacted]")
            .field("model", &self.model)
            .field("conversation_id", &self.conversation_id)
            .field("workspace_id", &self.workspace_id)
            .finish()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "operation",
    content = "request",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OperationRequest {
    CreateAgent(CreateAgentRequest),
    CreateRun(RunRequest),
}
impl OperationRequest {
    pub fn validate(&self) -> Result<(), Error> {
        self.payload(MAX_INPUT).map(|_| ())
    }

    /// Versioned fingerprint of the exact HTTP operation, including forced
    /// notification suppression. A digest is not authentication of its author.
    pub fn request_sha256(&self) -> Result<String, Error> {
        let payload = self.payload(MAX_INPUT)?;
        Ok(json_sha256(&json!({
            "schema_version": 1, "path": self.path(), "payload": payload
        })))
    }

    pub(crate) fn path(&self) -> &'static str {
        match self {
            Self::CreateAgent(_) => "agents",
            Self::CreateRun(_) => "runs",
        }
    }
    pub(crate) fn model(&self) -> Option<&str> {
        match self {
            Self::CreateAgent(r) => r.model.as_deref(),
            Self::CreateRun(r) => r.model.as_deref(),
        }
    }
    pub(crate) fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::CreateAgent(r) => r.conversation_id.as_deref(),
            Self::CreateRun(r) => r.conversation_id.as_deref(),
        }
    }
    pub(crate) fn payload(&self, max: usize) -> Result<Value, Error> {
        let invalid = || error(ErrorKind::InvalidRequest, Stage::Configuration);
        if self.model().is_some_and(|v| !plain(v, 100))
            || self.conversation_id().is_some_and(|v| !uuid(v))
        {
            return Err(invalid());
        }
        let payload = match self {
            Self::CreateAgent(r) => {
                if r.input.trim().is_empty() || r.input.len() > max {
                    return Err(invalid());
                }
                serde_json::to_value(r).map_err(|_| invalid())?
            }
            Self::CreateRun(r) => {
                if !uuid(&r.agent_id)
                    || r.workspace_id.as_deref().is_some_and(|v| !uuid(v))
                    || !bounded_tree(&r.input, max)
                {
                    return Err(invalid());
                }
                let mut value = serde_json::to_value(r).map_err(|_| invalid())?;
                value
                    .as_object_mut()
                    .ok_or_else(invalid)?
                    .insert("should_notify".into(), Value::Bool(false));
                value
            }
        };
        bounded_serialize(&payload, max).map_err(|_| invalid())?;
        Ok(payload)
    }
}

/// An account-bound receipt from trusted backend storage. Deserialization does
/// not grant organization authority or prove a caller owns the remote run.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunTicket {
    pub(crate) schema_version: u32,
    pub(crate) account_namespace: String,
    pub(crate) request_sha256: String,
    pub(crate) run_id: String,
    pub(crate) agent_id: Option<String>,
    pub(crate) conversation_id: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) model: Option<String>,
}
impl fmt::Debug for RunTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunTicket")
            .field("schema_version", &self.schema_version)
            .field("account_namespace", &self.account_namespace)
            .field("request_sha256", &self.request_sha256)
            .field("run_id", &self.run_id)
            .field("agent_id", &self.agent_id)
            .field("conversation_id", &self.conversation_id)
            .field("workspace_id", &self.workspace_id)
            .field("model", &"[redacted]")
            .finish()
    }
}
impl RunTicket {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }
    pub fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }
    pub fn account_namespace(&self) -> &str {
        &self.account_namespace
    }
    pub fn validate_for(&self, request: &OperationRequest) -> Result<(), Error> {
        let invalid = || error(ErrorKind::TicketMismatch, Stage::Configuration);
        if self.schema_version != 1
            || !plain(&self.account_namespace, 256)
            || !self.account_namespace.is_ascii()
            || !uuid(&self.run_id)
            || [&self.agent_id, &self.conversation_id, &self.workspace_id]
                .iter()
                .any(|id| id.as_deref().is_some_and(|v| !uuid(v)))
            || self.model.as_deref().is_some_and(|v| !plain(v, 100))
            || self.request_sha256 != account_fingerprint(request, &self.account_namespace)?
            || request
                .model()
                .is_some_and(|v| Some(v) != self.model.as_deref())
            || request
                .conversation_id()
                .is_some_and(|v| Some(v) != self.conversation_id.as_deref())
        {
            return Err(invalid());
        }
        match request {
            OperationRequest::CreateRun(r)
                if self.agent_id.as_deref() != Some(r.agent_id.as_str())
                    || r.workspace_id
                        .as_ref()
                        .is_some_and(|id| self.workspace_id.as_ref() != Some(id)) =>
            {
                Err(invalid())
            }
            OperationRequest::CreateAgent(_) if self.workspace_id.is_some() => Err(invalid()),
            _ => Ok(()),
        }
    }
    pub fn validate_for_account(
        &self,
        request: &OperationRequest,
        namespace: &str,
    ) -> Result<(), Error> {
        self.validate_for(request)?;
        if self.account_namespace != namespace {
            return Err(error(ErrorKind::TicketMismatch, Stage::Configuration));
        }
        Ok(())
    }

    /// Bind identity fields first learned during a validated poll. Persist the
    /// returned ticket atomically with that observation before the next poll;
    /// this prevents later responses from replacing an initially unknown ID.
    pub fn refine(
        &self,
        request: &OperationRequest,
        observation: &RunObservation,
    ) -> Result<Self, Error> {
        observation.validate_for(request, self)?;
        let mut next = self.clone();
        next.agent_id = observation.agent_id.clone();
        next.conversation_id = observation.conversation_id.clone();
        next.workspace_id = observation.workspace_id.clone();
        next.model = observation.model.clone();
        next.validate_for(request)?;
        Ok(next)
    }
}

/// Unknown future statuses remain inspectable and never imply completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Unknown(String),
}
impl NativeRunStatus {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown(s) => s,
        }
    }
}
impl Serialize for NativeRunStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for NativeRunStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if !plain(&value, 128) {
            return Err(serde::de::Error::custom("invalid native status"));
        }
        Ok(match value.as_str() {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Unknown(value),
        })
    }
}

/// Provider metadata and untrusted output, never a verified domain outcome.
/// `raw_response` preserves future fields; do not log it or feed it into an
/// authority decision without validating the responsibility's output contract.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunObservation {
    pub run_id: String,
    pub status: NativeRunStatus,
    pub agent_id: Option<String>,
    pub conversation_id: Option<String>,
    pub workspace_id: Option<String>,
    pub model: Option<String>,
    pub result: Option<Value>,
    pub raw_response: Value,
}
impl fmt::Debug for RunObservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if matches!(self.status, NativeRunStatus::Unknown(_)) {
            "unknown"
        } else {
            self.status.as_str()
        };
        f.debug_struct("RunObservation")
            .field("run_id", &self.run_id)
            .field("status", &status)
            .field("agent_id", &self.agent_id)
            .field("conversation_id", &self.conversation_id)
            .field("workspace_id", &self.workspace_id)
            .field("model", &"[redacted]")
            .field("result", &"[redacted]")
            .field("raw_response", &"[redacted]")
            .finish()
    }
}
impl RunObservation {
    pub fn validate_for(
        &self,
        request: &OperationRequest,
        ticket: &RunTicket,
    ) -> Result<(), Error> {
        ticket.validate_for(request)?;
        if !bounded_tree(&self.raw_response, MAX_RESPONSE)
            || bounded_serialize(&self.raw_response, MAX_RESPONSE).is_err()
            || observation(
                self.raw_response.clone(),
                request,
                Some(ticket),
                Stage::Poll,
            )? != *self
        {
            return Err(error(ErrorKind::ProvenanceMismatch, Stage::Poll));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Configuration,
    Submission,
    Poll,
    AgentInspection,
}

/// Provider guard policy only. `full_auto` is not a GENESIS permission grant.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteAutonomy {
    FullAuto,
    ApprovalRequired,
    Blocked,
    /// The exact future value remains inspectable in `raw_response`.
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentObservation {
    pub agent_id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub status: String,
    pub write_autonomy: WriteAutonomy,
    pub created_at: String,
    pub updated_at: String,
    /// Includes effective skill/config content when returned by the provider.
    /// Treat it as untrusted and do not log credentials, signed URLs, or content.
    pub raw_response: Value,
}

pub(crate) fn agent_observation(raw: Value, expected_id: &str) -> Result<AgentObservation, Error> {
    #[derive(Deserialize)]
    struct Fields {
        agent_id: String,
        name: String,
        description: Option<String>,
        version: String,
        status: String,
        write_autonomy: WriteAutonomy,
        created_at: String,
        updated_at: String,
    }
    let malformed = || error(ErrorKind::MalformedResponse, Stage::AgentInspection);
    // Option fields otherwise silently accept omission; the documented agent
    // schema requires description even when its value is null.
    if raw.get("description").is_none() {
        return Err(malformed());
    }
    let fields: Fields = serde_json::from_value(raw.clone()).map_err(|_| malformed())?;
    if !uuid(&fields.agent_id)
        || !plain(&fields.name, 1024)
        || !plain(&fields.version, 128)
        || !plain(&fields.status, 128)
        || chrono::DateTime::parse_from_rfc3339(&fields.created_at).is_err()
        || chrono::DateTime::parse_from_rfc3339(&fields.updated_at).is_err()
    {
        return Err(malformed());
    }
    if fields.agent_id != expected_id {
        return Err(error(ErrorKind::ProvenanceMismatch, Stage::AgentInspection));
    }
    Ok(AgentObservation {
        agent_id: fields.agent_id,
        name: fields.name,
        description: fields.description,
        version: fields.version,
        status: fields.status,
        write_autonomy: fields.write_autonomy,
        created_at: fields.created_at,
        updated_at: fields.updated_at,
        raw_response: raw,
    })
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidConfiguration,
    InvalidRequest,
    TicketMismatch,
    LimiterUnavailable,
    DeadlineExceeded,
    Transport,
    HttpStatus,
    ResponseTooLarge,
    MalformedResponse,
    ProvenanceMismatch,
    CredentialEcho,
}
/// Contains no provider text, request contents, URL, or credentials.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub kind: ErrorKind,
    pub stage: Stage,
    pub http_status: Option<u16>,
    /// False when rejected locally or while waiting for quota. True once a
    /// submission is attempted, even if its response was lost or rejected.
    pub post_may_have_run: bool,
    pub retry_after_secs: Option<u64>,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CREAO {:?}: {:?}", self.stage, self.kind)
    }
}
impl std::error::Error for Error {}
pub(crate) fn error(kind: ErrorKind, stage: Stage) -> Error {
    Error {
        kind,
        stage,
        http_status: None,
        post_may_have_run: false,
        retry_after_secs: None,
    }
}

pub(crate) fn account_fingerprint(
    request: &OperationRequest,
    namespace: &str,
) -> Result<String, Error> {
    Ok(json_sha256(
        &json!({"schema_version":1,"account_namespace":namespace,"request_sha256":request.request_sha256()?}),
    ))
}

pub(crate) fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
pub(crate) fn plain(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

pub(crate) fn bounded_tree(value: &Value, max_bytes: usize) -> bool {
    let mut pending = vec![(value, 0usize)];
    let mut nodes = 0usize;
    let mut bytes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 64 || nodes > 10000 {
            return false;
        }
        match value {
            Value::Array(items) => {
                if items.len() > 10000 - nodes - pending.len() {
                    return false;
                }
                pending.extend(items.iter().map(|v| (v, depth + 1)));
            }
            Value::Object(items) => {
                if items.len() > 10000 - nodes - pending.len() {
                    return false;
                }
                for (key, item) in items {
                    bytes = bytes.saturating_add(key.len());
                    pending.push((item, depth + 1));
                }
            }
            Value::String(value) => bytes = bytes.saturating_add(value.len()),
            _ => {}
        }
        if bytes > max_bytes {
            return false;
        }
    }
    true
}

pub(crate) fn bounded_serialize(value: &Value, max: usize) -> Result<Vec<u8>, ()> {
    struct Writer {
        bytes: Vec<u8>,
        max: usize,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.max - self.bytes.len() {
                return Err(std::io::Error::other("JSON limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        max,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| ())?;
    Ok(writer.bytes)
}

pub(crate) fn observation(
    raw: Value,
    request: &OperationRequest,
    ticket: Option<&RunTicket>,
    stage: Stage,
) -> Result<RunObservation, Error> {
    let malformed = || error(ErrorKind::MalformedResponse, stage);
    let mismatch = || error(ErrorKind::ProvenanceMismatch, stage);
    let object = raw.as_object().ok_or_else(malformed)?;
    let run_id = object
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|v| uuid(v))
        .ok_or_else(malformed)?
        .to_owned();
    let status: NativeRunStatus =
        serde_json::from_value(object.get("status").ok_or_else(malformed)?.clone())
            .map_err(|_| malformed())?;
    let nullable = |key: &str, required: bool, is_id: bool| -> Result<Option<String>, Error> {
        match object.get(key) {
            Some(Value::Null) => Ok(None),
            Some(Value::String(v)) if if is_id { uuid(v) } else { plain(v, 100) } => {
                Ok(Some(v.clone()))
            }
            None if !required => Ok(None),
            _ => Err(malformed()),
        }
    };
    let mut agent_id = nullable("agent_id", true, true)?;
    let conversation_id = nullable("conversation_id", true, true)?;
    let workspace_id = nullable("workspace_id", false, true)?;
    let model = nullable("model", true, false)?;
    let result = match object.get("result") {
        Some(Value::Object(_)) => object.get("result").cloned(),
        None => None,
        _ => return Err(malformed()),
    };
    match request {
        OperationRequest::CreateAgent(_) => {
            if let Some(value) = result.as_ref().and_then(|r| r.get("agent_id")) {
                let nested_id = value.as_str().filter(|s| uuid(s)).ok_or_else(malformed)?;
                if agent_id.as_deref().is_some_and(|id| id != nested_id) {
                    return Err(mismatch());
                }
                agent_id = Some(nested_id.to_owned());
            }
            if (status.is_completed() && agent_id.is_none()) || workspace_id.is_some() {
                return Err(mismatch());
            }
        }
        OperationRequest::CreateRun(r) => {
            if agent_id.as_deref() != Some(r.agent_id.as_str())
                || r.workspace_id
                    .as_ref()
                    .is_some_and(|v| workspace_id.as_ref() != Some(v))
            {
                return Err(mismatch());
            }
        }
    }
    if request
        .conversation_id()
        .is_some_and(|v| conversation_id.as_deref() != Some(v))
        || request.model().is_some_and(|v| model.as_deref() != Some(v))
    {
        return Err(mismatch());
    }
    if let Some(ticket) = ticket {
        if run_id != ticket.run_id
            || [
                &ticket.agent_id,
                &ticket.conversation_id,
                &ticket.workspace_id,
                &ticket.model,
            ]
            .into_iter()
            .zip([&agent_id, &conversation_id, &workspace_id, &model])
            .any(|(expected, actual)| expected.is_some() && expected != actual)
        {
            return Err(mismatch());
        }
    }
    Ok(RunObservation {
        run_id,
        status,
        agent_id,
        conversation_id,
        workspace_id,
        model,
        result,
        raw_response: raw,
    })
}
