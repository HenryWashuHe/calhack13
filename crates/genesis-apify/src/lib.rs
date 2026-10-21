//! Executes a backend-pinned Apify build and produces verifier input from cloud
//! metadata. Dataset contents remain untrusted. This is not organization authority.

use genesis_verification::{
    decode_json, json_sha256, ArtifactPin, ExecutionEvidence, RunStatus, SourceSnapshot,
};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::{Client, Method, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::fmt;
use std::time::Duration;
use tokio::time::{sleep, timeout_at, Instant};

#[derive(Clone, Debug)]
pub struct RunLimits {
    pub request_timeout: Duration,
    pub poll_interval: Duration,
    /// Bounds the complete operation, including build lookup and output reading.
    pub deadline: Duration,
    pub actor_timeout_secs: u32,
    pub memory_mbytes: u32,
    pub max_total_charge_usd: f64,
    pub max_input_bytes: usize,
    pub max_response_bytes: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(20),
            poll_interval: Duration::from_secs(2),
            deadline: Duration::from_secs(150),
            actor_timeout_secs: 120,
            memory_mbytes: 256,
            max_total_charge_usd: 1.0,
            max_input_bytes: 256 * 1024,
            max_response_bytes: 1024 * 1024,
        }
    }
}

/// Trusted backend context. Do not construct from generated Actor metadata.
#[derive(Clone, Debug)]
pub struct RunRequest {
    pub case_id: String,
    pub artifact: ArtifactPin,
    pub input: Value,
    /// Backend-captured fixture/source; the cloud does not attest this digest.
    pub source: SourceSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Configuration,
    Build,
    Submission,
    Poll,
    Dataset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidConfiguration,
    InvalidRequest,
    Transport,
    HttpStatus(u16),
    DeadlineExceeded,
    ResponseTooLarge,
    MalformedResponse,
    ProvenanceMismatch,
    BuildNotSucceeded,
    RunFailed,
    UnknownRunStatus,
    InvalidDataset,
}

/// Sanitized errors never retain request URLs, tokens, response bodies, or input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub stage: Stage,
    pub run_id: Option<String>,
    /// A POST may have reached Apify even when its response was lost. Reconcile
    /// before a new submission. A local timeout does not cancel the remote run.
    pub submission_may_have_occurred: bool,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Apify {:?}: {:?}", self.stage, self.kind)
    }
}
impl std::error::Error for Error {}

pub struct ApifyClient {
    client: Client,
    base: Url,
    authorization: HeaderValue,
    limits: RunLimits,
}

impl fmt::Debug for ApifyClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApifyClient")
            .field("authorization", &"[REDACTED]")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Build {
    id: String,
    act_id: String,
    build_number: String,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    id: String,
    act_id: String,
    build_id: String,
    status: String,
    default_dataset_id: Option<String>,
}

struct Response {
    headers: reqwest::header::HeaderMap,
    bytes: Vec<u8>,
}

impl ApifyClient {
    pub fn new(token: &str, limits: RunLimits) -> Result<Self, Error> {
        Self::configured(token, limits, "https://api.apify.com", false)
    }

    /// Only loopback IP endpoints are allowed; never use a production token here.
    pub fn for_local_testing(token: &str, limits: RunLimits, base: &str) -> Result<Self, Error> {
        Self::configured(token, limits, base, true)
    }

    fn configured(token: &str, limits: RunLimits, base: &str, local: bool) -> Result<Self, Error> {
        let invalid = || error(ErrorKind::InvalidConfiguration, Stage::Configuration);
        let base = Url::parse(base).map_err(|_| invalid())?;
        let local_ip = base
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if (local && (base.scheme() != "http" || !local_ip))
            || (!local && (base.scheme() != "https" || base.host_str() != Some("api.apify.com")))
            || base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
            || token.is_empty()
            || token.len() > 4096
            || token.chars().any(char::is_whitespace)
            || limits.request_timeout.is_zero()
            || limits.request_timeout > Duration::from_secs(3600)
            || limits.poll_interval.is_zero()
            || limits.poll_interval > Duration::from_secs(3600)
            || limits.deadline.is_zero()
            || limits.deadline > Duration::from_secs(3600)
            || limits.actor_timeout_secs == 0
            || limits.actor_timeout_secs > 3600
            || !(128..=32768).contains(&limits.memory_mbytes)
            || !limits.memory_mbytes.is_power_of_two()
            || !limits.max_total_charge_usd.is_finite()
            || limits.max_total_charge_usd <= 0.0
            || !(1..=8 * 1024 * 1024).contains(&limits.max_input_bytes)
            || !(1..=8 * 1024 * 1024).contains(&limits.max_response_bytes)
        {
            return Err(invalid());
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| invalid())?;
        authorization.set_sensitive(true);
        let client = Client::builder()
            .timeout(limits.request_timeout)
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| invalid())?;
        Ok(Self {
            client,
            base,
            authorization,
            limits,
        })
    }

    /// Submit once, poll, and fetch one extraction envelope. This does not retry
    /// submissions or grant installation/mission authority. Persist errors with
    /// known run IDs and reconcile ambiguous submissions in the kernel.
    pub async fn execute(&self, request: &RunRequest) -> Result<ExecutionEvidence, Error> {
        let input_bytes = self.validate_request(request)?;
        let deadline = Instant::now() + self.limits.deadline;
        let build: Build = self
            .get_data(
                &["v2", "actor-builds", &request.artifact.build_id],
                Stage::Build,
                deadline,
            )
            .await?;
        if build.id != request.artifact.build_id
            || build.act_id != request.artifact.actor_id
            || build.build_number != request.artifact.build_number
        {
            return Err(error(ErrorKind::ProvenanceMismatch, Stage::Build));
        }
        if build.status != "SUCCEEDED" {
            return Err(error(ErrorKind::BuildNotSucceeded, Stage::Build));
        }
        let mut url = self.url(&["v2", "actors", &request.artifact.actor_id, "runs"])?;
        url.query_pairs_mut()
            .append_pair("build", &request.artifact.build_number)
            .append_pair("timeout", &self.limits.actor_timeout_secs.to_string())
            .append_pair("memory", &self.limits.memory_mbytes.to_string())
            .append_pair(
                "maxTotalChargeUsd",
                &self.limits.max_total_charge_usd.to_string(),
            )
            .append_pair("maxItems", "2")
            .append_pair("forcePermissionLevel", "LIMITED_PERMISSIONS");
        let result = self
            .request(
                Method::POST,
                url,
                Some(input_bytes),
                Stage::Submission,
                deadline,
            )
            .await;
        let mut run: Run = result
            .and_then(|r| decode::<Envelope<Run>>(&r.bytes, Stage::Submission).map(|v| v.data))
            .map_err(|mut e| {
                e.submission_may_have_occurred = true;
                e
            })?;
        let run_id = if valid_id(&run.id) {
            Some(run.id.clone())
        } else {
            None
        };
        let result = self.collect(request, &mut run, deadline).await;
        result.map_err(|mut e| {
            e.run_id = run_id;
            e.submission_may_have_occurred = true;
            e
        })
    }

    async fn collect(
        &self,
        request: &RunRequest,
        run: &mut Run,
        deadline: Instant,
    ) -> Result<ExecutionEvidence, Error> {
        let original_id = run.id.clone();
        let mut dataset_id: Option<String> = None;
        loop {
            if !valid_id(&run.id)
                || run.id != original_id
                || run.act_id != request.artifact.actor_id
                || run.build_id != request.artifact.build_id
            {
                return Err(error(ErrorKind::ProvenanceMismatch, Stage::Poll));
            }
            if let Some(id) = &run.default_dataset_id {
                if !valid_id(id) || dataset_id.as_ref().is_some_and(|previous| previous != id) {
                    return Err(error(ErrorKind::ProvenanceMismatch, Stage::Poll));
                }
                dataset_id = Some(id.clone());
            }
            match run.status.as_str() {
                "SUCCEEDED" => break,
                "READY" | "RUNNING" | "TIMING-OUT" | "ABORTING" => {}
                "FAILED" | "TIMED-OUT" | "ABORTED" => {
                    return Err(error(ErrorKind::RunFailed, Stage::Poll))
                }
                _ => return Err(error(ErrorKind::UnknownRunStatus, Stage::Poll)),
            }
            timeout_at(deadline, sleep(self.limits.poll_interval))
                .await
                .map_err(|_| error(ErrorKind::DeadlineExceeded, Stage::Poll))?;
            *run = self
                .get_data(&["v2", "actor-runs", &original_id], Stage::Poll, deadline)
                .await?;
        }
        // A successful terminal record itself must identify its output store.
        let dataset_id = run
            .default_dataset_id
            .as_ref()
            .ok_or_else(|| error(ErrorKind::MalformedResponse, Stage::Poll))?;
        let mut url = self.url(&["v2", "datasets", dataset_id, "items"])?;
        // Ask for one extra item so overflow is rejected rather than truncated.
        url.query_pairs_mut()
            .append_pair("format", "json")
            .append_pair("limit", "2")
            .append_pair("offset", "0")
            .append_pair("clean", "false");
        let response = self
            .request(Method::GET, url, None, Stage::Dataset, deadline)
            .await?;
        let mut items: Vec<Value> = decode(&response.bytes, Stage::Dataset)?;
        if items.len() != 1 || !items[0].is_object() {
            return Err(error(ErrorKind::InvalidDataset, Stage::Dataset));
        }
        // Headers are documented but optional in intermediated local responses.
        // If provided, they must agree and cannot hide additional records.
        for (name, expected) in [
            ("x-apify-pagination-total", 1),
            ("x-apify-pagination-count", 1),
            ("x-apify-pagination-offset", 0),
        ] {
            if let Some(value) = response.headers.get(name) {
                if value.to_str().ok().and_then(|v| v.parse::<usize>().ok()) != Some(expected) {
                    return Err(error(ErrorKind::InvalidDataset, Stage::Dataset));
                }
            }
        }
        Ok(ExecutionEvidence {
            case_id: request.case_id.clone(),
            artifact: request.artifact.clone(),
            input_sha256: json_sha256(&request.input),
            source: request.source.clone(),
            run_id: original_id,
            dataset_id: dataset_id.clone(),
            status: RunStatus::Succeeded,
            output: items.remove(0),
        })
    }

    fn validate_request(&self, request: &RunRequest) -> Result<Vec<u8>, Error> {
        let invalid = || error(ErrorKind::InvalidRequest, Stage::Configuration);
        let pin = &request.artifact;
        if !valid_label(&request.case_id)
            || !valid_label(&pin.capability_id)
            || !valid_label(&pin.capability_version)
            || !valid_id(&pin.actor_id)
            || !valid_id(&pin.build_id)
            || !digest(&pin.source_sha256)
            || !digest(&request.source.sha256)
            || !request.input.is_object()
            || !bounded_tree(&request.input)
            || !numeric_build(&pin.build_number)
        {
            return Err(invalid());
        }
        let source = Url::parse(&request.source.url).map_err(|_| invalid())?;
        if !matches!(source.scheme(), "http" | "https")
            || source.host_str().is_none()
            || !source.username().is_empty()
            || source.password().is_some()
            || source.fragment().is_some()
            || request.source.url.len() > 4096
        {
            return Err(invalid());
        }
        let bytes = serde_json::to_vec(&request.input).map_err(|_| invalid())?;
        if bytes.len() > self.limits.max_input_bytes {
            return Err(invalid());
        }
        Ok(bytes)
    }

    fn url(&self, segments: &[&str]) -> Result<Url, Error> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| error(ErrorKind::InvalidConfiguration, Stage::Configuration))?
            .clear()
            .extend(segments);
        Ok(url)
    }

    async fn get_data<T: DeserializeOwned>(
        &self,
        path: &[&str],
        stage: Stage,
        deadline: Instant,
    ) -> Result<T, Error> {
        let response = self
            .request(Method::GET, self.url(path)?, None, stage, deadline)
            .await?;
        decode::<Envelope<T>>(&response.bytes, stage).map(|v| v.data)
    }

    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        stage: Stage,
        deadline: Instant,
    ) -> Result<Response, Error> {
        let action = async {
            let mut request = self
                .client
                .request(method, url)
                .header(AUTHORIZATION, self.authorization.clone());
            if let Some(body) = body {
                request = request
                    .header("content-type", "application/json")
                    .body(body);
            }
            let mut response = request
                .send()
                .await
                .map_err(|_| error(ErrorKind::Transport, stage))?;
            if !response.status().is_success() {
                return Err(error(
                    ErrorKind::HttpStatus(response.status().as_u16()),
                    stage,
                ));
            }
            if response
                .content_length()
                .is_some_and(|n| n > self.limits.max_response_bytes as u64)
            {
                return Err(error(ErrorKind::ResponseTooLarge, stage));
            }
            let headers = response.headers().clone();
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| error(ErrorKind::Transport, stage))?
            {
                if chunk.len() > self.limits.max_response_bytes - bytes.len() {
                    return Err(error(ErrorKind::ResponseTooLarge, stage));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(Response { headers, bytes })
        };
        timeout_at(deadline, action)
            .await
            .map_err(|_| error(ErrorKind::DeadlineExceeded, stage))?
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8], stage: Stage) -> Result<T, Error> {
    decode_json(bytes).map_err(|_| error(ErrorKind::MalformedResponse, stage))
}
fn error(kind: ErrorKind, stage: Stage) -> Error {
    Error {
        kind,
        stage,
        run_id: None,
        submission_may_have_occurred: false,
    }
}
fn valid_label(s: &str) -> bool {
    !s.is_empty() && s.len() <= 1024 && s.trim() == s && !s.chars().any(char::is_control)
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn numeric_build(s: &str) -> bool {
    let parts: Vec<_> = s.split('.').collect();
    parts.len() == 3
        && s.len() <= 64
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}
fn bounded_tree(value: &Value) -> bool {
    let mut pending = vec![(value, 0)];
    let mut count = 0;
    while let Some((v, depth)) = pending.pop() {
        count += 1;
        if count > 10000 || depth > 32 {
            return false;
        }
        match v {
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => pending.extend(values.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    true
}
