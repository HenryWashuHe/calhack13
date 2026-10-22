//! Bounded HTTP driver for one fresh, two-organization inheritance scenario.
//! This is an operator-directed integration workflow, not an autonomous planner.
//! It never receives an oracle, executes generated source, or retries a POST.
//! A failed/ambiguous request leaves server-owned durable state for inspection;
//! a rerun refuses the now-nonempty organizations instead of duplicating work.

use crate::capability_acquisition_store::AcquisitionCounters;
use genesis_core::lifecycle::{InstallationStatus, RoleState};
use genesis_core::model::{OrgState, Owner, Provider};
use genesis_forge::{CandidateIdentity, PublicExample, SynthesisRequest};
use genesis_verification::{validate_artifact, ArtifactPin};
use reqwest::{Client, Method, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("invalid scenario configuration: {0}")]
    Invalid(&'static str),
    #[error("scenario preflight refused: {0}")]
    Preflight(&'static str),
    #[error("scenario request failed at {0}; inspect server state before continuing")]
    Transport(&'static str),
    #[error("scenario HTTP request failed at {step} with status {status}; inspect server state before continuing")]
    Http { step: &'static str, status: u16 },
    #[error("scenario response exceeded the configured bound at {0}")]
    ResponseTooLarge(&'static str),
    #[error("scenario received an invalid response at {0}")]
    Response(&'static str),
    #[error("scenario stopped at {0}; inspect server state before continuing")]
    Stopped(&'static str),
    #[error("scenario exceeded its polling or elapsed-time bound; inspect server state before continuing")]
    Limit,
}

type Result<T> = std::result::Result<T, ScenarioError>;

/// Credentials deliberately have no Debug or Serialize implementation.
pub struct ScenarioEndpoint {
    base: Url,
    token: String,
}
impl ScenarioEndpoint {
    pub fn new(base_url: &str, operator_token: &str) -> Result<Self> {
        let base = Url::parse(base_url).map_err(|_| ScenarioError::Invalid("organization URL"))?;
        let loopback = base
            .host_str()
            .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if base.host_str().is_none()
            || !(base.scheme() == "https" || (base.scheme() == "http" && loopback))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(ScenarioError::Invalid(
                "use an HTTPS origin or literal loopback HTTP origin",
            ));
        }
        if operator_token.is_empty()
            || operator_token.len() > 4096
            || !operator_token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(ScenarioError::Invalid("operator token"));
        }
        Ok(Self {
            base,
            token: operator_token.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct ScenarioLimits {
    pub request_timeout: Duration,
    pub max_response_bytes: usize,
    pub poll_interval: Duration,
    pub max_polls: usize,
    pub total_timeout: Duration,
}
impl Default for ScenarioLimits {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(300),
            max_response_bytes: 8 * 1024 * 1024,
            poll_interval: Duration::from_millis(250),
            max_polls: 512,
            total_timeout: Duration::from_secs(1800),
        }
    }
}

/// Only public generation inputs and a local invocation input cross this API.
/// Each organization's oracle remains in its independently configured backend.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    pub run_id: String,
    pub suite_a_id: String,
    pub suite_b_id: String,
    pub candidate: CandidateIdentity,
    pub repair: Option<CandidateIdentity>,
    pub allowed_source_urls: Vec<String>,
    pub examples: Vec<PublicExample>,
    pub invocation_input: Value,
    #[serde(default)]
    pub allow_development_roles: bool,
    /// Both operators have arranged permitted access to the same private Actor.
    /// Registry publication neither shares credentials nor grants provider access.
    #[serde(default)]
    pub provider_access_acknowledged: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioRoleBinding {
    pub org_id: String,
    pub role_id: String,
    pub provider: Option<Provider>,
    pub provider_agent_id: Option<String>,
    pub state: RoleState,
}

/// Redacted evidence of the completed workflow: no source, HTML, oracle answers,
/// execution outputs, credentials, or provider response bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioReport {
    pub org_a_id: String,
    pub org_b_id: String,
    pub mission_a_id: String,
    pub mission_b_id: String,
    pub requirement_a_id: String,
    pub requirement_b_id: String,
    pub acquisition_id: String,
    pub acquisition_attempt: u8,
    pub artifact: ArtifactPin,
    pub acquisition_usage: AcquisitionCounters,
    pub installation_b_id: String,
    pub acceptance_b_ref: String,
    pub acceptance_b_jobs: Vec<String>,
    pub initial_run_b_id: String,
    pub continuation_run_b_id: String,
    pub producer_role_id: String,
    pub producer_successor_id: String,
    pub verifier_role_id: String,
    pub monitor_role_id: String,
    pub responsibility_a_id: String,
    pub responsibility_b_id: String,
    /// Synthesis count returned by this run's B installation operation.
    pub synthesis_calls_b: u64,
    /// This B server reported acquisition disabled before and after the workflow.
    /// This is not an account-wide claim about unrelated processes.
    pub acquisition_disabled_b: bool,
    pub bindings: Vec<ScenarioRoleBinding>,
}

pub struct ScenarioDriver {
    a: ScenarioEndpoint,
    b: ScenarioEndpoint,
    http: Client,
    limits: ScenarioLimits,
}

struct Work {
    mission: String,
    observation: String,
    requirement: String,
    role: String,
    responsibility: String,
}

impl ScenarioDriver {
    pub fn new(a: ScenarioEndpoint, b: ScenarioEndpoint, limits: ScenarioLimits) -> Result<Self> {
        if a.base == b.base {
            return Err(ScenarioError::Invalid(
                "organizations must use distinct origins",
            ));
        }
        if limits.request_timeout.is_zero()
            || limits.request_timeout > Duration::from_secs(300)
            || !(1024..=16 * 1024 * 1024).contains(&limits.max_response_bytes)
            || limits.poll_interval.is_zero()
            || limits.poll_interval > Duration::from_secs(5)
            || !(1..=4096).contains(&limits.max_polls)
            || limits.total_timeout.is_zero()
            || limits.total_timeout > Duration::from_secs(7200)
        {
            return Err(ScenarioError::Invalid("network and polling limits"));
        }
        let http = Client::builder()
            .timeout(limits.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .build()
            .map_err(|_| ScenarioError::Invalid("HTTP client"))?;
        Ok(Self { a, b, http, limits })
    }

    pub async fn run(&self, spec: &ScenarioSpec) -> Result<ScenarioReport> {
        validate_spec(spec)?;
        tokio::time::timeout(self.limits.total_timeout, self.run_inner(spec))
            .await
            .map_err(|_| ScenarioError::Limit)?
    }

    async fn request(
        &self,
        endpoint: &ScenarioEndpoint,
        method: Method,
        path: &str,
        body: Option<Value>,
        step: &'static str,
    ) -> Result<Value> {
        let url = endpoint
            .base
            .join(&format!("/api/v1{path}"))
            .map_err(|_| ScenarioError::Invalid("request path"))?;
        let mut request = self.http.request(method, url).bearer_auth(&endpoint.token);
        if let Some(body) = body {
            let bytes =
                serde_json::to_vec(&body).map_err(|_| ScenarioError::Invalid("request JSON"))?;
            if bytes.len() > self.limits.max_response_bytes {
                return Err(ScenarioError::Invalid(
                    "request exceeds configured byte bound",
                ));
            }
            request = request
                .header("content-type", "application/json")
                .body(bytes);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| ScenarioError::Transport(step))?;
        if !response.status().is_success() {
            return Err(ScenarioError::Http {
                step,
                status: response.status().as_u16(),
            });
        }
        if response
            .content_length()
            .is_some_and(|len| len > self.limits.max_response_bytes as u64)
        {
            return Err(ScenarioError::ResponseTooLarge(step));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ScenarioError::Transport(step))?
        {
            if chunk.len() > self.limits.max_response_bytes.saturating_sub(bytes.len()) {
                return Err(ScenarioError::ResponseTooLarge(step));
            }
            bytes.extend_from_slice(&chunk);
        }
        genesis_verification::decode_json(&bytes).map_err(|_| ScenarioError::Response(step))
    }

    async fn get(
        &self,
        endpoint: &ScenarioEndpoint,
        path: &str,
        step: &'static str,
    ) -> Result<Value> {
        self.request(endpoint, Method::GET, path, None, step).await
    }
    async fn post(
        &self,
        endpoint: &ScenarioEndpoint,
        path: &str,
        body: Value,
        step: &'static str,
    ) -> Result<Value> {
        self.request(endpoint, Method::POST, path, Some(body), step)
            .await
    }
    async fn state(&self, endpoint: &ScenarioEndpoint) -> Result<OrgState> {
        decode(
            self.get(endpoint, "/organization", "organization").await?,
            "organization",
        )
    }

    async fn preflight(&self, spec: &ScenarioSpec) -> Result<(String, String)> {
        let a = self.state(&self.a).await?;
        let b = self.state(&self.b).await?;
        if a.org_id == b.org_id {
            return Err(ScenarioError::Preflight(
                "organization identities must differ",
            ));
        }
        for state in [&a, &b] {
            if state.revision != 0
                || !state.missions.is_empty()
                || !state.roles.is_empty()
                || !state.responsibilities.is_empty()
                || !state.installations.is_empty()
                || !state.observations.is_empty()
                || !state.requirements.is_empty()
                || !state.assignments.is_empty()
                || !state.contracts.is_empty()
            {
                return Err(ScenarioError::Preflight(
                    "both organizations must be fresh and empty",
                ));
            }
            if !state
                .constitution
                .scopes
                .contains("capability.venue_lookup")
            {
                return Err(ScenarioError::Preflight("venue lookup scope is required"));
            }
        }
        if !a.constitution.scopes.contains("capability.acquire") {
            return Err(ScenarioError::Preflight(
                "Org A requires acquisition authority",
            ));
        }
        let ha = self.get(&self.a, "/health", "preflight health").await?;
        let hb = self.get(&self.b, "/health", "preflight health").await?;
        for health in [&ha, &hb] {
            if health["registry_configured"] != true {
                return Err(ScenarioError::Preflight(
                    "both registries must be configured",
                ));
            }
            if !spec.allow_development_roles && health["dev_stub_enabled"] != false {
                return Err(ScenarioError::Preflight(
                    "development role provisioning requires explicit opt-in",
                ));
            }
        }
        if ha["acquisition_enabled"] != true || hb["acquisition_enabled"] != false {
            return Err(ScenarioError::Preflight(
                "acquisition must be enabled only for Org A",
            ));
        }
        Ok((a.org_id.to_string(), b.org_id.to_string()))
    }

    async fn propose(
        &self,
        endpoint: &ScenarioEndpoint,
        key: String,
        observation: &str,
        operations: Value,
        stage: &'static str,
    ) -> Result<Value> {
        let revision = self.state(endpoint).await?.revision;
        let mut change = self.post(endpoint, "/org-changes", json!({
            "client_request_id":key,"proposal":{"expected_org_revision":revision,
            "trigger_evidence_ids":[observation],"rationale":"Keep continuing venue work accountable as requirements change",
            "operations":operations}
        }), stage).await?;
        let id = field_id(&change, "change_id", stage)?;
        for _ in 0..self.limits.max_polls {
            match change["status"].as_str() {
                Some("COMMITTED") => return Ok(change),
                Some("FAILED" | "REJECTED") => return Err(ScenarioError::Stopped(stage)),
                Some(_) => {}
                None => return Err(ScenarioError::Response(stage)),
            }
            tokio::time::sleep(self.limits.poll_interval).await;
            change = self
                .get(endpoint, &format!("/org-changes/{id}"), stage)
                .await?;
        }
        Err(ScenarioError::Limit)
    }

    async fn seed(
        &self,
        endpoint: &ScenarioEndpoint,
        spec: &ScenarioSpec,
        producer: bool,
    ) -> Result<Work> {
        let role_name = if producer {
            "Mission coordinator"
        } else {
            "Local venue verifier"
        };
        let scope = if producer {
            "capability.acquire"
        } else {
            "capability.venue_lookup"
        };
        let mission = self.post(endpoint, "/missions", json!({
            "goal":"Maintain verified venue lookup for a campus event after the main hall becomes unavailable",
            "constraints":["Lookup and monitoring only; no reservation or live availability claim"],
            "success_checks":["An accountable role can run a locally accepted venue capability"],
            "roles":[{"key":"initial","name":role_name,"charter":"Own the initial venue mission",
                "authority":{"scopes":[scope],"budget_cents":0}}],"responsibilities":[]
        }), "create mission").await?;
        let mission = field_id(&mission, "mission_id", "create mission")?;
        let state = self.state(endpoint).await?;
        let initial_role = state
            .roles
            .values()
            .find(|r| r.name == role_name)
            .ok_or(ScenarioError::Response("initial role"))?
            .role_id
            .to_string();
        let observation = self.post(endpoint, &format!("/missions/{mission}/observations"), json!({
            "source":"scenario-operator","summary":"The main hall is unavailable and replacement venues use an unfamiliar portal",
            "evidence_ref":spec.allowed_source_urls[0]
        }), "observe unexpected requirement").await?;
        let observation = field_id(&observation, "observation_id", "observation")?;
        let requirement = self.post(endpoint, &format!("/missions/{mission}/requirements"), json!({
            "summary":"Read venue capacity, price and availability from the unfamiliar public portal and preserve ongoing ownership",
            "evidence":[observation],"acceptance":"Independently verified lookup remains available after responsibility handoff"
        }), "record requirement").await?;
        let requirement = field_id(&requirement, "requirement_id", "requirement")?;
        let mut operations = vec![];
        let owner = if producer {
            operations.push(json!({"op":"CreateRole","key":"specialist","name":"Temporary portal specialist",
                "charter":"Acquire a verified portal capability for this new requirement",
                "authority":{"scopes":["capability.acquire"],"budget_cents":0},
                "reports_to":{"kind":"existing","role_id":initial_role},"provider":"elastic","justification":"independent_ownership"}));
            json!({"kind":"new","key":"specialist"})
        } else {
            json!({"kind":"existing","role_id":initial_role})
        };
        operations.push(json!({"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"lookup",
            "requirement_id":requirement,"purpose":"Keep verified venue lookup available","trigger":"venue portal changes","termination":"event ends"},"owner":owner}));
        let change = self
            .propose(
                endpoint,
                format!("{}-assign", spec.run_id),
                &observation,
                json!(operations),
                "assign unexpected work",
            )
            .await?;
        let role = if producer {
            nested_id(
                &change,
                &["allocation", "roles", "specialist"],
                "specialist allocation",
            )?
        } else {
            initial_role
        };
        let responsibility = nested_id(
            &change,
            &["allocation", "responsibilities", "lookup"],
            "responsibility allocation",
        )?;
        self.check_bindings(endpoint, spec).await?;
        Ok(Work {
            mission,
            observation,
            requirement,
            role,
            responsibility,
        })
    }

    async fn check_bindings(&self, endpoint: &ScenarioEndpoint, spec: &ScenarioSpec) -> Result<()> {
        let state = self.state(endpoint).await?;
        if !spec.allow_development_roles
            && state
                .roles
                .values()
                .any(|r| r.binding.as_ref().is_some_and(|b| b.provider.is_dev_stub()))
        {
            return Err(ScenarioError::Stopped(
                "development binding was not authorized",
            ));
        }
        Ok(())
    }

    async fn acquire(&self, spec: &ScenarioSpec, work: &Work) -> Result<Value> {
        let mut status = self.post(&self.a, "/capability-acquisitions", json!({
            "client_request_id":format!("{}-acquire",spec.run_id),"mission_id":work.mission,
            "requirement_id":work.requirement,"role_id":work.role,"responsibility_id":work.responsibility,
            "suite_id":spec.suite_a_id,"candidate":spec.candidate,"repair":spec.repair,
            "allowed_source_urls":spec.allowed_source_urls,"examples":spec.examples,"deadline":null
        }), "create acquisition").await?;
        let id = field_id(&status, "id", "acquisition identity")?;
        for _ in 0..self.limits.max_polls {
            if status["currently_authorized"] != true {
                return Err(ScenarioError::Stopped("acquisition authority changed"));
            }
            let phase = status["phase"]
                .as_str()
                .ok_or(ScenarioError::Response("acquisition phase"))?;
            match phase {
                "verified" => return Ok(status),
                "generating" | "forging" => {}
                "rejected" if spec.repair.is_some() && status["attempt"] == 0 => {}
                _ => return Err(ScenarioError::Stopped("acquisition did not verify")),
            }
            let previous = status["version"].clone();
            status = self
                .post(
                    &self.a,
                    &format!("/capability-acquisitions/{id}/step"),
                    json!({"retry_read":false}),
                    "advance acquisition",
                )
                .await?;
            if status["phase"] == "rejected" && status["version"] == previous {
                return Err(ScenarioError::Stopped(
                    "acquisition rejected without repair",
                ));
            }
            tokio::time::sleep(self.limits.poll_interval).await;
        }
        Err(ScenarioError::Limit)
    }

    async fn collect(
        &self,
        id: &str,
        artifact: &ArtifactPin,
        acceptance: Option<(&str, &str)>,
    ) -> Result<()> {
        for _ in 0..self.limits.max_polls {
            let status = self
                .get(
                    &self.b,
                    &format!("/capability-runs/{id}"),
                    "inspect capability run",
                )
                .await?;
            let actual: ArtifactPin =
                decode(status["request"]["artifact"].clone(), "run artifact")?;
            if actual != *artifact || status["job_id"] != id {
                return Err(ScenarioError::Response("run artifact mismatch"));
            }
            if let Some((installation, suite)) = acceptance {
                if status["purpose"]["kind"] != "local_acceptance"
                    || status["purpose"]["installation_id"] != installation
                    || status["purpose"]["suite_id"] != suite
                {
                    return Err(ScenarioError::Response("local acceptance purpose mismatch"));
                }
            } else if status["purpose"]["kind"] != "invocation" {
                return Err(ScenarioError::Response("invocation purpose mismatch"));
            }
            match status["phase"].as_str() {
                Some("collected") => {
                    let evidence: ArtifactPin =
                        decode(status["evidence"]["artifact"].clone(), "collected artifact")?;
                    if evidence != *artifact {
                        return Err(ScenarioError::Response("collected artifact mismatch"));
                    }
                    return Ok(());
                }
                Some("prepared" | "running") => {
                    self.post(
                        &self.b,
                        &format!("/capability-runs/{id}/step"),
                        json!({"retry_read":false}),
                        "advance capability run",
                    )
                    .await?;
                }
                // Another server worker may own the saved intent. Poll it; do
                // not force recovery or repeat an uncertain provider POST.
                Some("submitting" | "collecting") => {}
                _ => return Err(ScenarioError::Stopped("capability run did not collect")),
            }
            tokio::time::sleep(self.limits.poll_interval).await;
        }
        Err(ScenarioError::Limit)
    }

    async fn invoke(
        &self,
        spec: &ScenarioSpec,
        work: &Work,
        role: &str,
        installation: &str,
        artifact: &ArtifactPin,
        key: &str,
    ) -> Result<String> {
        let job = self
            .post(
                &self.b,
                "/capability-runs",
                json!({"client_request_id":format!("{}-{key}",spec.run_id),
            "installation_id":installation,"role_id":role,"responsibility_id":work.responsibility,
            "suite_id":spec.suite_b_id,"input":spec.invocation_input}),
                "invoke inherited capability",
            )
            .await?;
        let id = field_id(&job, "job_id", "invocation identity")?;
        self.collect(&id, artifact, None).await?;
        Ok(id)
    }

    async fn retire(
        &self,
        endpoint: &ScenarioEndpoint,
        spec: &ScenarioSpec,
        work: &Work,
        artifact: Option<&ArtifactPin>,
    ) -> Result<String> {
        let scope = if artifact.is_some() {
            "capability.venue_lookup"
        } else {
            "capability.acquire"
        };
        let mut operations = vec![
            json!({"op":"CreateRole","key":"monitor","name":"Persistent venue monitor",
            "charter":"Maintain the inherited responsibility after temporary work ends",
            "authority":{"scopes":[scope],"budget_cents":0},"provider":"creao","justification":"lifecycle"}),
            json!({"op":"RetireRole","role_id":work.role,"handoffs":[{"responsibility_id":work.responsibility,"to":{"kind":"new","key":"monitor"}}]}),
        ];
        if let Some(pin) = artifact {
            operations.push(json!({"op":"AttachCapability","role":{"kind":"new","key":"monitor"},"capability":{"capability_id":pin.capability_id,"version":pin.capability_version}}));
        }
        let change = self
            .propose(
                endpoint,
                format!("{}-retire", spec.run_id),
                &work.observation,
                json!(operations),
                "handoff and retire temporary role",
            )
            .await?;
        let successor = nested_id(
            &change,
            &["allocation", "roles", "monitor"],
            "monitor allocation",
        )?;
        self.check_bindings(endpoint, spec).await?;
        let state = self.state(endpoint).await?;
        let old = state
            .roles
            .values()
            .find(|r| r.role_id.as_str() == work.role)
            .ok_or(ScenarioError::Response("retired role"))?;
        let responsibility = state
            .responsibilities
            .values()
            .find(|r| r.responsibility_id.as_str() == work.responsibility)
            .ok_or(ScenarioError::Response("transferred responsibility"))?;
        if old.state != RoleState::Retired
            || responsibility.owner != Owner::Role(successor.clone().into())
        {
            return Err(ScenarioError::Response("retirement dropped ownership"));
        }
        Ok(successor)
    }

    async fn run_inner(&self, spec: &ScenarioSpec) -> Result<ScenarioReport> {
        let (org_a_id, org_b_id) = self.preflight(spec).await?;
        let a = self.seed(&self.a, spec, true).await?;
        let acquired = self.acquire(spec, &a).await?;
        let acquisition_id = field_id(&acquired, "id", "acquisition identity")?;
        let artifact: ArtifactPin = decode(acquired["artifact"].clone(), "verified artifact")?;
        validate_artifact(&artifact).map_err(|_| ScenarioError::Response("verified artifact"))?;
        let attempt: u8 = decode(acquired["attempt"].clone(), "acquisition attempt")?;
        let candidate = match attempt {
            0 => &spec.candidate,
            1 => spec
                .repair
                .as_ref()
                .ok_or(ScenarioError::Response("unexpected repair"))?,
            _ => return Err(ScenarioError::Response("unexpected repair")),
        };
        if artifact.capability_id != candidate.capability_id
            || artifact.capability_version != candidate.capability_version
        {
            return Err(ScenarioError::Response("acquired identity mismatch"));
        }
        let usage: AcquisitionCounters = decode(
            acquired["organization_usage"].clone(),
            "acquisition counters",
        )?;
        let expected_attempts = u64::from(attempt) + 1;
        if usage.total_intents != 1
            || usage.active != 0
            || usage.model_calls != expected_attempts
            || usage.actor_creates != expected_attempts
            || usage.builds != expected_attempts
            || usage.test_submits < 3 * expected_attempts
        {
            return Err(ScenarioError::Response(
                "unexpected acquisition effect counts",
            ));
        }
        let published = self.post(&self.a,"/capabilities/publish",json!({"verification_ref":format!("acquisition:{acquisition_id}"),"manifest":{
            "capability_id":artifact.capability_id,"version":artifact.capability_version,"publisher":org_a_id,
            "source_digest":artifact.source_sha256,"intent":"Read venue capacity price and availability from public portal fixtures",
            "applicability_conditions":["Only the independently configured oracle scope"],
            "known_limitations":["Lookup and monitoring only; not a reservation or live availability guarantee"],
            "input_schema":{"type":"object"},"output_schema":{"type":"object"},"required_permissions":["capability.venue_lookup"],
            "artifact":{"actor_id":artifact.actor_id,"build_id":artifact.build_id,"build_number":artifact.build_number},
            "role_template":null,"responsibility_template":"Keep verified venue lookup available","workflow_procedure":null,
            "local_configuration_schema":{},"acceptance_suite_ref":spec.suite_a_id,
            "verification":{"passed":false,"cases":[],"tested_environment":"backend proof required","limitations":[],"oracle_ref":"backend proof required","verified_at":"2000-01-01T00:00:00Z"},
            "provenance":[],"state":"APPROVED","published_at":null
        }}),"publish acquired capability").await?;
        if manifest_pin(&published)? != artifact
            || published["verification"]["passed"] != true
            || published["acceptance_suite_ref"] != spec.suite_a_id
        {
            return Err(ScenarioError::Response("publication proof mismatch"));
        }
        let b = self.seed(&self.b, spec, false).await?;
        let hits = self.post(&self.b,"/capabilities/search",json!({"query":"venue capacity price availability portal","limit":100,"available_scopes":["capability.venue_lookup"]}),"discover inherited capability").await?;
        let hits = hits
            .as_array()
            .ok_or(ScenarioError::Response("capability search"))?;
        if !hits.iter().any(|hit| {
            hit["capability_id"] == artifact.capability_id
                && hit["version"] == artifact.capability_version
                && hit["publisher"] == org_a_id
                && hit["source_digest"] == artifact.source_sha256
        }) {
            return Err(ScenarioError::Stopped(
                "exact published capability not discovered",
            ));
        }
        let installed = self.post(&self.b,"/capability-installations",json!({"capability_id":artifact.capability_id,"version":artifact.capability_version}),"install inherited capability").await?;
        let installation = field_id(&installed, "installation_id", "installation identity")?;
        if installed["synthesis_calls"] != 0
            || installed["manifest_source_digest"] != artifact.source_sha256
            || installed["artifact"] != published["artifact"]
        {
            return Err(ScenarioError::Response(
                "installation regenerated or changed artifact",
            ));
        }
        let checks = self.post(&self.b,&format!("/capability-installations/{installation}/checks"),json!({"client_request_id":format!("{}-checks",spec.run_id),"role_id":b.role,"responsibility_id":b.responsibility,"suite_id":spec.suite_b_id}),"start independent local acceptance").await?;
        let jobs: Vec<String> =
            decode(checks["job_ids"].clone(), "local acceptance job identities")?;
        let unique: BTreeSet<_> = jobs.iter().collect();
        if jobs.len() < 3
            || jobs.len() > 64
            || unique.len() != jobs.len()
            || jobs.iter().any(|id| !safe_id(id))
        {
            return Err(ScenarioError::Response("local acceptance job identities"));
        }
        for id in &jobs {
            self.collect(id, &artifact, Some((&installation, &spec.suite_b_id)))
                .await?;
        }
        let accepted = self
            .post(
                &self.b,
                &format!("/capability-installations/{installation}/acceptance"),
                json!({"suite_id":spec.suite_b_id,"job_ids":jobs}),
                "accept independent local proof",
            )
            .await?;
        let reference = accepted["installation"]["acceptance_report_ref"]
            .as_str()
            .filter(|s| !s.is_empty() && !s.contains("dev-assertion"))
            .ok_or(ScenarioError::Response("local acceptance reference"))?
            .to_owned();
        if accepted["installation"]["status"] != "ACCEPTED"
            || accepted["installation"]["source_digest"] != artifact.source_sha256
        {
            return Err(ScenarioError::Response("local acceptance mismatch"));
        }
        self.propose(&self.b,format!("{}-attach",spec.run_id),&b.observation,json!([{"op":"AttachCapability","role":{"kind":"existing","role_id":b.role},"capability":{"capability_id":artifact.capability_id,"version":artifact.capability_version}}]),"attach locally accepted capability").await?;
        let initial_run = self
            .invoke(spec, &b, &b.role, &installation, &artifact, "initial-run")
            .await?;
        let producer_successor = self.retire(&self.a, spec, &a, None).await?;
        let monitor = self.retire(&self.b, spec, &b, Some(&artifact)).await?;
        let continuation = self
            .invoke(spec, &b, &monitor, &installation, &artifact, "continuation")
            .await?;
        let final_a = self.state(&self.a).await?;
        let final_b = self.state(&self.b).await?;
        let final_installation = final_b
            .installations
            .values()
            .find(|i| i.installation_id.as_str() == installation)
            .ok_or(ScenarioError::Response("final installation"))?;
        if final_installation.status != InstallationStatus::Accepted
            || final_installation.acceptance_report_ref.as_deref() != Some(&reference)
            || final_installation.source_digest != artifact.source_sha256
        {
            return Err(ScenarioError::Response(
                "acceptance changed after verifier retirement",
            ));
        }
        let after = self
            .get(
                &self.a,
                &format!("/capability-acquisitions/{acquisition_id}"),
                "final acquisition counts",
            )
            .await?;
        let after_usage: AcquisitionCounters = decode(
            after["organization_usage"].clone(),
            "final acquisition counts",
        )?;
        if after_usage != usage {
            return Err(ScenarioError::Response(
                "inheritance consumed additional acquisition effects",
            ));
        }
        if self
            .get(&self.b, "/health", "final generation-disabled evidence")
            .await?["acquisition_enabled"]
            != false
        {
            return Err(ScenarioError::Response(
                "Org B acquisition configuration changed",
            ));
        }
        let bindings = [&final_a, &final_b]
            .into_iter()
            .flat_map(|state| {
                state.roles.values().map(|role| ScenarioRoleBinding {
                    org_id: state.org_id.to_string(),
                    role_id: role.role_id.to_string(),
                    provider: role.binding.as_ref().map(|b| b.provider),
                    provider_agent_id: role.binding.as_ref().map(|b| b.provider_agent_id.clone()),
                    state: role.state,
                })
            })
            .collect();
        Ok(ScenarioReport {
            org_a_id,
            org_b_id,
            mission_a_id: a.mission,
            mission_b_id: b.mission,
            requirement_a_id: a.requirement,
            requirement_b_id: b.requirement,
            acquisition_id,
            acquisition_attempt: attempt,
            artifact,
            acquisition_usage: usage,
            installation_b_id: installation,
            acceptance_b_ref: reference,
            acceptance_b_jobs: jobs,
            initial_run_b_id: initial_run,
            continuation_run_b_id: continuation,
            producer_role_id: a.role,
            producer_successor_id: producer_successor,
            verifier_role_id: b.role,
            monitor_role_id: monitor,
            responsibility_a_id: a.responsibility,
            responsibility_b_id: b.responsibility,
            synthesis_calls_b: 0,
            acquisition_disabled_b: true,
            bindings,
        })
    }
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}
fn field_id(value: &Value, key: &str, step: &'static str) -> Result<String> {
    nested_id(value, &[key], step)
}
fn nested_id(value: &Value, keys: &[&str], step: &'static str) -> Result<String> {
    let value = keys.iter().fold(value, |value, key| &value[*key]);
    value
        .as_str()
        .filter(|id| safe_id(id))
        .map(str::to_owned)
        .ok_or(ScenarioError::Response(step))
}
fn decode<T: DeserializeOwned>(value: Value, step: &'static str) -> Result<T> {
    serde_json::from_value(value).map_err(|_| ScenarioError::Response(step))
}
fn manifest_pin(value: &Value) -> Result<ArtifactPin> {
    decode(
        json!({"capability_id":value["capability_id"],"capability_version":value["version"],"source_sha256":value["source_digest"],
        "actor_id":value["artifact"]["actor_id"],"build_id":value["artifact"]["build_id"],"build_number":value["artifact"]["build_number"]}),
        "manifest pin",
    )
}
fn validate_spec(spec: &ScenarioSpec) -> Result<()> {
    if !spec.provider_access_acknowledged {
        return Err(ScenarioError::Invalid(
            "permitted shared Actor access must be acknowledged",
        ));
    }
    if !safe_id(&spec.run_id)
        || spec.run_id.len() > 96
        || !safe_id(&spec.suite_a_id)
        || !safe_id(&spec.suite_b_id)
    {
        return Err(ScenarioError::Invalid("run and oracle suite identities"));
    }
    let request = SynthesisRequest {
        operation_id: format!("{}-generation", spec.run_id),
        candidate: spec.candidate.clone(),
        requirement: "Acquire verified venue lookup for an unexpected portal requirement".into(),
        allowed_source_urls: spec.allowed_source_urls.clone(),
        examples: spec.examples.clone(),
    };
    request
        .validate()
        .map_err(|_| ScenarioError::Invalid("public generation inputs"))?;
    if let Some(repair) = &spec.repair {
        let mut repaired = request.clone();
        repaired.candidate = repair.clone();
        repaired
            .validate()
            .map_err(|_| ScenarioError::Invalid("repair identity"))?;
        if repair.capability_id != spec.candidate.capability_id
            || repair.capability_version == spec.candidate.capability_version
            || repair.actor_name == spec.candidate.actor_name
        {
            return Err(ScenarioError::Invalid(
                "repair requires a distinct source and Actor for the same capability",
            ));
        }
    }
    if !spec.invocation_input.is_object()
        || serde_json::to_vec(&spec.invocation_input)
            .map_err(|_| ScenarioError::Invalid("invocation input"))?
            .len()
            > 64 * 1024
    {
        return Err(ScenarioError::Invalid("bounded invocation object"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_origins_and_secrets_are_constrained() {
        for url in [
            "https://organization.example",
            "http://127.0.0.1:7101",
            "http://[::1]:7101",
        ] {
            assert!(ScenarioEndpoint::new(url, "operator").is_ok());
        }
        for url in [
            "http://organization.example",
            "http://localhost:7101",
            "https://user:secret@organization.example",
            "https://organization.example/path",
            "https://organization.example/?token=secret",
            "https://organization.example/#secret",
        ] {
            assert!(ScenarioEndpoint::new(url, "operator").is_err());
        }
        assert!(ScenarioEndpoint::new("https://organization.example", "secret\nvalue").is_err());
        assert!(!safe_id("job/../../secret"));
    }
    #[test]
    fn spec_rejects_overrides_and_requires_explicit_provider_access() {
        let mut value = json!({
            "run_id":"scenario-one","suite_a_id":"suite-a","suite_b_id":"suite-b",
            "candidate":{"capability_id":"venue-reader","capability_version":"1.0.0","actor_name":"venue-reader-one","source_version":"0.1"},
            "repair":null,"allowed_source_urls":["https://example.com/venues"],
            "examples":[{"url":"https://example.com/venues","html":"<main>Public listing</main>"}],
            "invocation_input":{"source_url":"https://example.com/venues"}
        });
        let spec: ScenarioSpec = serde_json::from_value(value.clone()).unwrap();
        assert!(!spec.allow_development_roles);
        assert!(!spec.provider_access_acknowledged);
        assert!(matches!(
            validate_spec(&spec),
            Err(ScenarioError::Invalid(_))
        ));
        value["provider_access_acknowledged"] = json!(true);
        assert!(validate_spec(&serde_json::from_value(value.clone()).unwrap()).is_ok());
        value["model_endpoint"] = json!("http://127.0.0.1:9999");
        assert!(serde_json::from_value::<ScenarioSpec>(value).is_err());
    }

    #[test]
    fn driver_limits_are_bounded_and_errors_never_embed_remote_values() {
        let make = || {
            (
                ScenarioEndpoint::new("http://127.0.0.1:7101", "TOKEN_A").unwrap(),
                ScenarioEndpoint::new("http://127.0.0.1:7102", "TOKEN_B").unwrap(),
            )
        };
        let (a, b) = make();
        assert!(ScenarioDriver::new(
            a,
            b,
            ScenarioLimits {
                max_polls: 0,
                ..ScenarioLimits::default()
            }
        )
        .is_err());
        assert_eq!(ScenarioError::Http {step:"publish acquired capability",status:503}.to_string(),"scenario HTTP request failed at publish acquired capability with status 503; inspect server state before continuing");
    }
}
