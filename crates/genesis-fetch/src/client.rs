//! Bounded clients for the private, authenticated Fetch SDK bridge.
//! A queue receipt proves durable local acceptance, never remote execution.

use crate::protocol::{address, BridgeIdentity, InboxPage, SendReceipt, SendRequest};
use genesis_verification::decode_json;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER},
    Client, Method, Url,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, time::Duration};
use tokio::time::{timeout, timeout_at, Instant};

pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 96 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct ClientLimits {
    pub timeout: Duration,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
}

impl Default for ClientLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
            max_request_bytes: MAX_REQUEST_BYTES,
            max_response_bytes: MAX_RESPONSE_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientErrorKind {
    InvalidConfiguration,
    InvalidRequest,
    Timeout,
    Transport,
    HttpStatus,
    ResponseTooLarge,
    MalformedResponse,
    IdentityMismatch,
    ReceiptMismatch,
    CredentialEcho,
    NoLongerEligible,
}

/// Safe to log: no URL, token, input, provider body, or nested transport error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("Fetch client error: {kind:?} (HTTP {http_status:?})")]
pub struct ClientError {
    pub kind: ClientErrorKind,
    pub http_status: Option<u16>,
    /// True after attempting a POST, even if its response was lost/rejected.
    /// Bridge retries must reuse the exact message ID, recipient and payload.
    pub post_may_have_run: bool,
    pub retry_after_secs: Option<u64>,
}

impl ClientError {
    pub(crate) fn new(kind: ClientErrorKind) -> Self {
        Self {
            kind,
            http_status: None,
            post_may_have_run: false,
            retry_after_secs: None,
        }
    }
    fn attempted_post(mut self) -> Self {
        self.post_may_have_run = true;
        self
    }
}

#[derive(Clone)]
pub(crate) struct Transport {
    client: Client,
    authorization: HeaderValue,
    pub(crate) limits: ClientLimits,
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transport")
            .field("authorization", &"[REDACTED]")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Transport {
    pub(crate) fn new(token: &str, limits: ClientLimits) -> Result<Self, ClientError> {
        let invalid = || ClientError::new(ClientErrorKind::InvalidConfiguration);
        if token.len() < 16
            || token.len() > 4096
            || !token.bytes().all(|b| b.is_ascii_graphic())
            || limits.timeout.is_zero()
            || limits.timeout > Duration::from_secs(300)
            || !(1..=MAX_REQUEST_BYTES).contains(&limits.max_request_bytes)
            || !(1..=MAX_RESPONSE_BYTES).contains(&limits.max_response_bytes)
        {
            return Err(invalid());
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| invalid())?;
        authorization.set_sensitive(true);
        let client = Client::builder()
            .timeout(limits.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .build()
            .map_err(|_| invalid())?;
        Ok(Self {
            client,
            authorization,
            limits,
        })
    }

    fn token(&self) -> &str {
        self.authorization
            .to_str()
            .expect("validated header")
            .strip_prefix("Bearer ")
            .expect("validated prefix")
    }

    fn contains_credential(&self, value: &Value) -> bool {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                Value::String(s) if s.contains(self.token()) => return true,
                Value::Array(items) => pending.extend(items),
                Value::Object(items) => {
                    if items.keys().any(|key| key.contains(self.token())) {
                        return true;
                    }
                    pending.extend(items.values());
                }
                _ => {}
            }
        }
        false
    }

    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
        expected_status: u16,
    ) -> Result<T, ClientError> {
        let started = Instant::now();
        let post = method == Method::POST;
        let mut request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, self.authorization.clone());
        if let Some(value) = body {
            if self.contains_credential(&value) {
                return Err(ClientError::new(ClientErrorKind::InvalidRequest));
            }
            let bytes = serde_json::to_vec(&value)
                .map_err(|_| ClientError::new(ClientErrorKind::InvalidRequest))?;
            if bytes.len() > self.limits.max_request_bytes {
                return Err(ClientError::new(ClientErrorKind::InvalidRequest));
            }
            request = request.header(CONTENT_TYPE, "application/json").body(bytes);
        }
        let action = async {
            let mut response = request.send().await.map_err(transport_error)?;
            let status = response.status().as_u16();
            if status != expected_status {
                let mut error = ClientError::new(ClientErrorKind::HttpStatus);
                error.http_status = Some(status);
                error.retry_after_secs = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|v| v.parse().ok());
                return Err(error);
            }
            if response
                .content_length()
                .is_some_and(|n| n > self.limits.max_response_bytes as u64)
            {
                return Err(ClientError::new(ClientErrorKind::ResponseTooLarge));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
                if chunk.len() > self.limits.max_response_bytes - bytes.len() {
                    return Err(ClientError::new(ClientErrorKind::ResponseTooLarge));
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes
                .windows(self.token().len())
                .any(|part| part == self.token().as_bytes())
            {
                return Err(ClientError::new(ClientErrorKind::CredentialEcho));
            }
            let value: Value = decode_json(&bytes)
                .map_err(|_| ClientError::new(ClientErrorKind::MalformedResponse))?;
            if self.contains_credential(&value) {
                return Err(ClientError::new(ClientErrorKind::CredentialEcho));
            }
            let decoded = serde_json::from_value(value)
                .map_err(|_| ClientError::new(ClientErrorKind::MalformedResponse))?;
            if started.elapsed() >= self.limits.timeout {
                return Err(ClientError::new(ClientErrorKind::Timeout));
            }
            Ok(decoded)
        };
        let result = timeout(self.limits.timeout, action)
            .await
            .map_err(|_| ClientError::new(ClientErrorKind::Timeout))
            .and_then(|result| result);
        if post {
            result.map_err(ClientError::attempted_post)
        } else {
            result
        }
    }
}

fn transport_error(error: reqwest::Error) -> ClientError {
    ClientError::new(if error.is_timeout() {
        ClientErrorKind::Timeout
    } else {
        ClientErrorKind::Transport
    })
}

pub(crate) fn loopback_base(value: &str, path: &str) -> Result<Url, ClientError> {
    let invalid = || ClientError::new(ClientErrorKind::InvalidConfiguration);
    let url = Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "http"
        || !url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback())
        || url.path() != path
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    Ok(url)
}

pub(crate) fn valid_protocol_digest(value: &str) -> bool {
    value.strip_prefix("proto:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Private bridge address is operator-configured literal loopback HTTP.
/// Discovered remote endpoints are never accepted or followed by this client.
#[derive(Clone)]
pub struct BridgeClient {
    transport: Transport,
    base: Url,
    expected_address: String,
    expected_protocol_digest: String,
}

impl fmt::Debug for BridgeClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BridgeClient")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl BridgeClient {
    pub fn new(
        base: &str,
        bearer_token: &str,
        expected_address: &str,
        expected_protocol_digest: &str,
        limits: ClientLimits,
    ) -> Result<Self, ClientError> {
        let base = loopback_base(base, "/v1")?;
        if !address(expected_address)
            || !valid_protocol_digest(expected_protocol_digest)
            || expected_address.contains(bearer_token)
            || expected_protocol_digest.contains(bearer_token)
        {
            return Err(ClientError::new(ClientErrorKind::InvalidConfiguration));
        }
        Ok(Self {
            transport: Transport::new(bearer_token, limits)?,
            base,
            expected_address: expected_address.to_owned(),
            expected_protocol_digest: expected_protocol_digest.to_owned(),
        })
    }

    fn url(&self, path: &str) -> Url {
        let mut url = self.base.clone();
        url.set_path(&format!("/v1/{path}"));
        url
    }

    pub async fn identity(&self) -> Result<BridgeIdentity, ClientError> {
        let identity: BridgeIdentity = self
            .transport
            .json(Method::GET, self.url("identity"), None, 200)
            .await?;
        if identity.schema_version != 1
            || identity.address != self.expected_address
            || identity.protocol_digest != self.expected_protocol_digest
        {
            return Err(ClientError::new(ClientErrorKind::IdentityMismatch));
        }
        Ok(identity)
    }

    /// Exactly one POST. Caller persists intent first; an ambiguous outcome
    /// may only be retried with the same request (bridge deduplicates it).
    /// Runtime integrations should use [`Self::send_guarded`] to recheck live
    /// contract eligibility after the awaited identity preflight.
    pub async fn send(&self, request: &SendRequest) -> Result<SendReceipt, ClientError> {
        self.send_guarded(request, || true).await
    }

    /// Recheck the trusted runtime's current eligibility after the identity
    /// GET, immediately before starting the POST. A false guard sends nothing.
    /// This does not cancel a request whose POST has already started; receivers
    /// and the kernel must independently fence late results and expired work.
    pub async fn send_guarded(
        &self,
        request: &SendRequest,
        authorize: impl FnOnce() -> bool,
    ) -> Result<SendReceipt, ClientError> {
        let deadline = Instant::now() + self.transport.limits.timeout;
        if request.schema_version != 1
            || !address(&request.recipient)
            || request.message.validate().is_err()
        {
            return Err(ClientError::new(ClientErrorKind::InvalidRequest));
        }
        let body = serde_json::to_value(request)
            .map_err(|_| ClientError::new(ClientErrorKind::InvalidRequest))?;
        // Inspect the configured bridge before disclosing payload or enqueuing
        // effects. Creation and identity are stable for a persisted bridge DB.
        // One deadline includes this preflight and the subsequent POST.
        timeout_at(deadline, self.identity())
            .await
            .map_err(|_| ClientError::new(ClientErrorKind::Timeout))??;
        if !authorize() {
            return Err(ClientError::new(ClientErrorKind::NoLongerEligible));
        }
        if Instant::now() >= deadline {
            return Err(ClientError::new(ClientErrorKind::Timeout));
        }
        let receipt: SendReceipt = timeout_at(
            deadline,
            self.transport
                .json(Method::POST, self.url("messages"), Some(body), 202),
        )
        .await
        .map_err(|_| ClientError::new(ClientErrorKind::Timeout).attempted_post())??;
        if receipt.schema_version != 1
            || receipt.message_id != request.message.message_id
            || receipt.bridge_address != self.expected_address
            || receipt.recipient != request.recipient
            || receipt.state != "queued"
        {
            return Err(ClientError::new(ClientErrorKind::ReceiptMismatch).attempted_post());
        }
        Ok(receipt)
    }

    /// Read one bounded page. SDK-authenticated sender is retained separately
    /// from the payload. Matching it to a contract belongs to the caller.
    pub async fn inbox(&self, after: u64, limit: u32) -> Result<InboxPage, ClientError> {
        if !(1..=50).contains(&limit) {
            return Err(ClientError::new(ClientErrorKind::InvalidRequest));
        }
        let mut url = self.url("inbox");
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("limit", &limit.to_string());
        let page: InboxPage = self.transport.json(Method::GET, url, None, 200).await?;
        if page.schema_version != 1 || page.address != self.expected_address {
            return Err(ClientError::new(ClientErrorKind::IdentityMismatch));
        }
        if page.items.len() > limit as usize {
            return Err(ClientError::new(ClientErrorKind::MalformedResponse));
        }
        let mut last = after;
        for item in &page.items {
            if item.sequence <= last || !address(&item.sender) || item.message.validate().is_err() {
                return Err(ClientError::new(ClientErrorKind::MalformedResponse));
            }
            last = item.sequence;
        }
        if page.next_cursor != last {
            return Err(ClientError::new(ClientErrorKind::MalformedResponse));
        }
        Ok(page)
    }
}
