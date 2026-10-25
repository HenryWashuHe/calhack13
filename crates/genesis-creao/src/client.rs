use crate::model::{
    account_fingerprint_for, agent_observation, bounded_serialize, bounded_tree, error,
    observation_for, uuid, ApiVersion, MAX_INPUT, MAX_RESPONSE,
};
use crate::{
    AccountLimiter, AgentObservation, Error, ErrorKind, LimiterError, Limits, OperationRequest,
    RequestKind, RunObservation, RunTicket, Stage,
};
use genesis_verification::decode_json;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER},
    Client, Method, Url,
};
use serde_json::Value;
use std::{fmt, future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::time::{timeout_at, Instant};

/// Backend authorization checked after account quota admission, immediately
/// before HTTP. Return false on missing authority or an unavailable authority
/// store. Release any application lock before returning. The client bounds the
/// future by its request deadline; the callback must not advance this runtime.
/// Changes after this check require separate result fencing by the caller.
pub type PreEffectGuard = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Clone shares the HTTP pool and the same durable account quota coordinator.
/// Create separate clients with the SAME limiter namespace for the same account.
#[derive(Clone)]
pub struct CreaoClient {
    client: Client,
    base: Url,
    authorization: HeaderValue,
    limits: Limits,
    limiter: AccountLimiter,
    api_version: ApiVersion,
    trigger_app_id: Option<String>,
}
impl fmt::Debug for CreaoClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreaoClient")
            .field("authorization", &"[REDACTED]")
            .field("limits", &self.limits)
            .field("api_version", &self.api_version)
            .finish_non_exhaustive()
    }
}
impl CreaoClient {
    pub fn new(token: &str, limits: Limits, limiter: AccountLimiter) -> Result<Self, Error> {
        Self::new_for_version(token, limits, limiter, ApiVersion::V1)
    }
    pub fn new_for_version(
        token: &str,
        limits: Limits,
        limiter: AccountLimiter,
        version: ApiVersion,
    ) -> Result<Self, Error> {
        if version == ApiVersion::Trigger {
            return Err(error(ErrorKind::InvalidConfiguration, Stage::Configuration));
        }
        Self::configured(
            token,
            limits,
            limiter,
            &format!("https://developer.creao.ai/{}", version.path()),
            false,
            version,
            None,
        )
    }
    /// Runs one prepublished app with its app-scoped `capi_` credential.
    /// An override must be the exact official runs URL for this app.
    pub fn new_trigger(
        token: &str,
        app_id: &str,
        runs_url: Option<&str>,
        limits: Limits,
        limiter: AccountLimiter,
    ) -> Result<Self, Error> {
        let default_url = format!("https://agent.creao.ai/api/v1/apps/{app_id}/runs");
        // Compare before URL parsing can normalize ports, casing or dot paths.
        if runs_url.is_some_and(|url| url != default_url) {
            return Err(error(ErrorKind::InvalidConfiguration, Stage::Configuration));
        }
        Self::configured(
            token,
            limits,
            limiter,
            runs_url.unwrap_or(&default_url),
            false,
            ApiVersion::Trigger,
            Some(app_id),
        )
    }

    /// Literal-loopback fixture transport; never use a live credential here.
    pub fn for_local_testing_trigger(
        token: &str,
        app_id: &str,
        limits: Limits,
        limiter: AccountLimiter,
        runs_url: &str,
    ) -> Result<Self, Error> {
        Self::configured(
            token,
            limits,
            limiter,
            runs_url,
            true,
            ApiVersion::Trigger,
            Some(app_id),
        )
    }
    /// Only literal loopback HTTP URLs ending in `/v1` are permitted. Never use
    /// a real account credential against a test endpoint.
    pub fn for_local_testing(
        token: &str,
        limits: Limits,
        limiter: AccountLimiter,
        base: &str,
    ) -> Result<Self, Error> {
        Self::configured(token, limits, limiter, base, true, ApiVersion::V1, None)
    }
    pub fn for_local_testing_v2(
        token: &str,
        limits: Limits,
        limiter: AccountLimiter,
        base: &str,
    ) -> Result<Self, Error> {
        Self::configured(token, limits, limiter, base, true, ApiVersion::V2, None)
    }
    fn configured(
        token: &str,
        limits: Limits,
        limiter: AccountLimiter,
        base: &str,
        local: bool,
        api_version: ApiVersion,
        trigger_app_id: Option<&str>,
    ) -> Result<Self, Error> {
        let invalid = || error(ErrorKind::InvalidConfiguration, Stage::Configuration);
        let base = Url::parse(base).map_err(|_| invalid())?;
        let trigger = api_version == ApiVersion::Trigger;
        let expected_path = match trigger_app_id {
            Some(id) if trigger && uuid(id) => format!("/api/v1/apps/{id}/runs"),
            None if !trigger => format!("/{}", api_version.path()),
            _ => return Err(invalid()),
        };
        let loopback = base
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback());
        if (local && (base.scheme() != "http" || !loopback))
            || (!local
                && (base.scheme() != "https"
                    || base.host_str()
                        != Some(if trigger {
                            "agent.creao.ai"
                        } else {
                            "developer.creao.ai"
                        })
                    || base.port().is_some()))
            || base.path() != expected_path
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
            || !token.starts_with(if trigger { "capi_" } else { "cr_sk_" })
            || !(7..=4096).contains(&token.len())
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || limiter.namespace().contains(token)
            || limits.request_timeout.is_zero()
            || limits.request_timeout > Duration::from_secs(3600)
            || !(1..=MAX_INPUT).contains(&limits.max_input_bytes)
            || !(1..=MAX_RESPONSE).contains(&limits.max_response_bytes)
        {
            return Err(invalid());
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| invalid())?;
        authorization.set_sensitive(true);
        let mut builder = Client::builder()
            .timeout(limits.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy();
        if trigger {
            // The official GET route currently rejects Node/default clients
            // at its CDN; match the observed, documented live-gate workaround.
            builder = builder.user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36");
        }
        let client = builder.build().map_err(|_| invalid())?;
        Ok(Self {
            client,
            base,
            authorization,
            limits,
            limiter,
            api_version,
            trigger_app_id: trigger_app_id.map(str::to_owned),
        })
    }
    pub fn api_version(&self) -> ApiVersion {
        self.api_version
    }
    pub fn account_namespace(&self) -> &str {
        self.limiter.namespace()
    }
    pub fn trigger_app_id(&self) -> Option<&str> {
        self.trigger_app_id.as_deref()
    }

    pub(crate) fn accepts_request_app(&self, request: &OperationRequest) -> bool {
        self.trigger_app_id.as_deref().is_none_or(
            |app| matches!(request, OperationRequest::CreateRun(run) if run.agent_id == app),
        )
    }

    /// Inspect an existing personal agent. This is a provider readiness input,
    /// not an organizational role activation or a proof that guarded writes ran.
    pub async fn get_agent(&self, agent_id: &str) -> Result<AgentObservation, Error> {
        if self.api_version == ApiVersion::Trigger || !uuid(agent_id) {
            return Err(error(ErrorKind::InvalidRequest, Stage::Configuration));
        }
        let raw = self
            .request(
                Method::GET,
                &format!("agents/{agent_id}"),
                None,
                RequestKind::Read,
                Stage::AgentInspection,
                None,
            )
            .await?;
        agent_observation(raw, agent_id)
    }

    /// Attempt one POST and return its durable receipt without polling. Caller
    /// must persist submission intent BEFORE calling, and the ticket afterward.
    /// An ambiguous error requires reconciliation, never an automatic new POST.
    pub async fn submit(&self, request: &OperationRequest) -> Result<RunTicket, Error> {
        self.submit_with_guard(request, None).await
    }

    /// Submit only if the guard still authorizes the effect after quota waiting.
    /// A denied guard never sends a request and is not an unknown POST outcome.
    /// Its conservative account quota reservation is retained, not refunded.
    pub async fn submit_guarded(
        &self,
        request: &OperationRequest,
        guard: &PreEffectGuard,
    ) -> Result<RunTicket, Error> {
        self.submit_with_guard(request, Some(guard)).await
    }

    pub(crate) async fn submit_with_guard(
        &self,
        request: &OperationRequest,
        guard: Option<&PreEffectGuard>,
    ) -> Result<RunTicket, Error> {
        if !self.accepts_request_app(request) {
            return Err(error(ErrorKind::InvalidRequest, Stage::Configuration));
        }
        let payload = request.payload_for(self.api_version, self.limits.max_input_bytes)?;
        if self.contains_credential(&payload) {
            return Err(error(ErrorKind::InvalidRequest, Stage::Configuration));
        }
        let bytes = bounded_serialize(&payload, self.limits.max_input_bytes)
            .map_err(|_| error(ErrorKind::InvalidRequest, Stage::Configuration))?;
        let kind = match request {
            OperationRequest::CreateAgent(_) => RequestKind::ManagementWrite,
            OperationRequest::CreateRun(_) => RequestKind::Run,
        };
        let raw = self
            .request(
                Method::POST,
                if self.api_version == ApiVersion::Trigger {
                    ""
                } else {
                    request.path()
                },
                Some(bytes),
                kind,
                Stage::Submission,
                guard,
            )
            .await?;
        let observed = observation_for(raw, request, None, Stage::Submission, self.api_version)
            .map_err(|mut e| {
                e.post_may_have_run = true;
                e
            })?;
        // A v2 creation may return a run ID before its agent ID. Preserve that
        // exact run ID; the runtime blocks polling until an agent-scoped edit
        // GET verifies an operator-supplied candidate ID.
        Ok(RunTicket {
            schema_version: 1,
            api_version: self.api_version,
            account_namespace: self.limiter.namespace().to_owned(),
            request_sha256: account_fingerprint_for(
                request,
                self.limiter.namespace(),
                self.api_version,
            )?,
            run_id: observed.run_id,
            agent_id: observed.agent_id,
            conversation_id: observed.conversation_id,
            workspace_id: observed.workspace_id,
            model: observed.model,
        })
    }

    /// Exactly one GET for a known run. No submission, sleeping poll loop,
    /// implicit retry, or automatic organization-state mutation occurs here.
    /// Persist `ticket.refine(request, &observation)` with each returned
    /// observation so subsequent polls retain identities first learned by GET.
    pub async fn poll(
        &self,
        request: &OperationRequest,
        ticket: &RunTicket,
    ) -> Result<RunObservation, Error> {
        self.poll_with_guard(request, ticket, None).await
    }

    /// Poll only if live authority still permits a read after quota waiting.
    pub async fn poll_guarded(
        &self,
        request: &OperationRequest,
        ticket: &RunTicket,
        guard: &PreEffectGuard,
    ) -> Result<RunObservation, Error> {
        self.poll_with_guard(request, ticket, Some(guard)).await
    }

    pub(crate) async fn poll_with_guard(
        &self,
        request: &OperationRequest,
        ticket: &RunTicket,
        guard: Option<&PreEffectGuard>,
    ) -> Result<RunObservation, Error> {
        if !self.accepts_request_app(request) {
            return Err(error(ErrorKind::TicketMismatch, Stage::Configuration));
        }
        let payload = request.payload_for(self.api_version, self.limits.max_input_bytes)?;
        ticket.validate_for_account(request, self.limiter.namespace())?;
        if ticket.api_version != self.api_version {
            return Err(error(ErrorKind::TicketMismatch, Stage::Configuration));
        }
        if self.contains_credential(&payload)
            || ticket
                .model
                .as_deref()
                .is_some_and(|v| v.contains(self.token()))
        {
            return Err(error(ErrorKind::TicketMismatch, Stage::Configuration));
        }
        let poll_path = if self.api_version == ApiVersion::Trigger {
            ticket.run_id.clone()
        } else if self.api_version == ApiVersion::V2
            && matches!(request, OperationRequest::CreateAgent(_))
        {
            let agent_id = ticket
                .agent_id
                .as_deref()
                .ok_or_else(|| error(ErrorKind::TicketMismatch, Stage::Configuration))?;
            format!("agents/{agent_id}/edits/{}", ticket.run_id)
        } else {
            format!("runs/{}", ticket.run_id)
        };
        let raw = self
            .request(
                Method::GET,
                &poll_path,
                None,
                RequestKind::Read,
                Stage::Poll,
                guard,
            )
            .await?;
        observation_for(raw, request, Some(ticket), Stage::Poll, self.api_version)
    }

    fn token(&self) -> &str {
        // Construction checked ASCII and the literal Bearer prefix.
        self.authorization
            .to_str()
            .expect("validated header")
            .strip_prefix("Bearer ")
            .expect("validated prefix")
    }
    fn contains_credential(&self, value: &Value) -> bool {
        let token = self.token();
        let mut pending = vec![value];
        while let Some(item) = pending.pop() {
            match item {
                Value::String(s) if s.contains(token) => return true,
                Value::Array(items) => pending.extend(items),
                Value::Object(items) => {
                    if items.keys().any(|key| key.contains(token)) {
                        return true;
                    }
                    pending.extend(items.values());
                }
                _ => {}
            }
        }
        false
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        kind: RequestKind,
        stage: Stage,
        guard: Option<&PreEffectGuard>,
    ) -> Result<Value, Error> {
        let deadline = Instant::now() + self.limits.request_timeout;
        self.limiter.acquire(kind, deadline).await.map_err(|e| {
            error(
                if matches!(e, LimiterError::DeadlineExceeded) {
                    ErrorKind::DeadlineExceeded
                } else {
                    ErrorKind::LimiterUnavailable
                },
                stage,
            )
        })?;
        if Instant::now() >= deadline {
            return Err(error(ErrorKind::DeadlineExceeded, stage));
        }
        let mut url = self.base.clone();
        if self.api_version == ApiVersion::Trigger {
            if !path.is_empty() {
                url.set_path(&format!("{}/{path}", self.base.path()));
            }
        } else {
            url.set_path(&format!("/{}/{path}", self.api_version.path()));
        }
        let post = method == Method::POST;
        let mut request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, self.authorization.clone());
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        // Keep this outside the dispatched-action error mapping: a denied or
        // timed-out guard proves no request was attempted. No await separates
        // successful authorization from beginning the HTTP action below.
        if let Some(guard) = guard {
            if !timeout_at(deadline, guard())
                .await
                .map_err(|_| error(ErrorKind::DeadlineExceeded, stage))?
            {
                return Err(error(ErrorKind::GuardRejected, stage));
            }
            if Instant::now() >= deadline {
                return Err(error(ErrorKind::DeadlineExceeded, stage));
            }
        }
        let action = async {
            let mut response = request.send().await.map_err(|e| {
                error(
                    if e.is_timeout() {
                        ErrorKind::DeadlineExceeded
                    } else {
                        ErrorKind::Transport
                    },
                    stage,
                )
            })?;
            let status = response.status().as_u16();
            let expected = if post { 202 } else { 200 };
            if status != expected {
                let mut e = error(ErrorKind::HttpStatus, stage);
                e.http_status = Some(status);
                if status == 429 {
                    let seconds = response
                        .headers()
                        .get(RETRY_AFTER)
                        .and_then(|h| h.to_str().ok())
                        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(60)
                        .min(86400);
                    e.retry_after_secs = Some(seconds);
                    let limiter = self.limiter.clone();
                    tokio::task::spawn_blocking(move || {
                        limiter.defer_for(Duration::from_secs(seconds))
                    })
                    .await
                    .map_err(|_| error(ErrorKind::LimiterUnavailable, stage))?
                    .map_err(|_| {
                        e.kind = ErrorKind::LimiterUnavailable;
                        e.clone()
                    })?;
                }
                return Err(e);
            }
            if response
                .content_length()
                .is_some_and(|n| n > self.limits.max_response_bytes as u64)
            {
                return Err(error(ErrorKind::ResponseTooLarge, stage));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| {
                error(
                    if e.is_timeout() {
                        ErrorKind::DeadlineExceeded
                    } else {
                        ErrorKind::Transport
                    },
                    stage,
                )
            })? {
                if chunk.len() > self.limits.max_response_bytes - bytes.len() {
                    return Err(error(ErrorKind::ResponseTooLarge, stage));
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes
                .windows(self.token().len())
                .any(|window| window == self.token().as_bytes())
            {
                return Err(error(ErrorKind::CredentialEcho, stage));
            }
            let raw: Value =
                decode_json(&bytes).map_err(|_| error(ErrorKind::MalformedResponse, stage))?;
            if !bounded_tree(&raw, self.limits.max_response_bytes) {
                return Err(error(ErrorKind::ResponseTooLarge, stage));
            }
            if self.contains_credential(&raw) {
                return Err(error(ErrorKind::CredentialEcho, stage));
            }
            if Instant::now() >= deadline {
                return Err(error(ErrorKind::DeadlineExceeded, stage));
            }
            Ok(raw)
        };
        timeout_at(deadline, action)
            .await
            .unwrap_or_else(|_| Err(error(ErrorKind::DeadlineExceeded, stage)))
            .map_err(|mut e| {
                e.post_may_have_run = post;
                e
            })
    }
}
