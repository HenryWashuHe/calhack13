//! Planner evaluation: the real planner against a fresh in-process kernel per
//! run, scored on the decision it gets committed.
//!
//! Each scenario seeds a mission, posts earlier evidence and one new
//! observation, then runs [`Planner::plan`] unchanged. The model is either a
//! live client from the planner's own `GENESIS_LLM_*` configuration or a
//! replay of recorded responses. The kernel, validation and saga are real, so
//! "valid" means the kernel committed it, not that the JSON parsed.
//!
//! Recorded responses use placeholders instead of generated IDs
//! (`{{role:<name>}}`, `{{resp:<purpose>}}`), so a recording made against one
//! organization replays against any fresh one.

use async_trait::async_trait;
use genesis_core::ids::ObservationId;
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::{LlmClient, LlmError, LlmRequest};
use genesis_planner::prompt::{KnownSpecialist, SYSTEM_PROMPT};
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One evaluation case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Operations the system prompt must offer for this case to run, e.g.
    /// `RequestCapability`. Missing ones make the case `skipped`, not failed.
    #[serde(default)]
    pub requires_ops: Vec<String>,
    /// `POST /missions` body (goal, constraints, roles, responsibilities).
    pub mission: Value,
    #[serde(default)]
    pub prior_observations: Vec<ObservationSpec>,
    pub observation: ObservationSpec,
    #[serde(default)]
    pub specialists: Vec<KnownSpecialist>,
    pub expect: Expect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationSpec {
    pub source: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Change,
    NoChange,
    Escalate,
}

/// What a good decision looks like. Every field is a separate check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Acceptable outcomes.
    pub verdicts: Vec<Verdict>,
    /// For a change: at least one of these operations appears.
    #[serde(default)]
    pub require_any_op: Vec<String>,
    /// None of these operations may appear.
    #[serde(default)]
    pub forbid_ops: Vec<String>,
    #[serde(default)]
    pub max_new_roles: Option<usize>,
    /// Total budget granted to new roles.
    #[serde(default)]
    pub max_new_budget_cents: Option<u64>,
    /// New roles must not be granted these scopes.
    #[serde(default)]
    pub forbid_scopes: Vec<String>,
    /// A proposed change must be committed by the kernel. Default true.
    #[serde(default = "yes")]
    pub must_commit: bool,
    /// Model calls allowed, including repairs.
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// Name of the existing role that must own the work: every
    /// AssignResponsibility owner, ReassignResponsibility target and
    /// RequestCapability role in a change must be this role, and a change
    /// must assign it something.
    #[serde(default)]
    pub owner_role: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub scenario: String,
    pub run: u32,
    pub model: String,
    /// Why the case did not run (an operation the prompt does not offer).
    pub skipped: Option<String>,
    /// Planner or harness failure; the run counts as failed.
    pub error: Option<String>,
    pub verdict: Option<Verdict>,
    pub change_status: Option<String>,
    pub ops: Vec<String>,
    pub escalation: Option<String>,
    /// For an escalation: true when the model's last reply parsed as an
    /// `escalate` verdict, false when the planner fell back to escalating
    /// (model unavailable, unusable output or repeated kernel rejection).
    pub escalation_by_model: Option<bool>,
    pub attempts: u32,
    pub latency_ms: u64,
    pub checks: Vec<Check>,
    pub passed: bool,
    /// Raw model responses with IDs replaced by placeholders.
    pub responses: Vec<String>,
}

/// Load every `*.json` scenario in `dir`, sorted by id.
pub fn load_scenarios(dir: &Path) -> anyhow::Result<Vec<Scenario>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let s: Scenario =
            serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        out.push(s);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen = std::collections::BTreeSet::new();
    for s in &out {
        anyhow::ensure!(seen.insert(s.id.clone()), "duplicate scenario id {}", s.id);
    }
    Ok(out)
}

/// Whether the planner's system prompt offers operation `op`.
pub fn prompt_offers(op: &str) -> bool {
    SYSTEM_PROMPT.contains(&format!("{{\"op\":\"{op}\""))
}

/// Generated IDs of one seeded organization, by placeholder.
#[derive(Debug, Clone, Default)]
pub struct Ids {
    /// placeholder (e.g. `{{role:Lead}}`) → id
    pub by_placeholder: BTreeMap<String, String>,
}

impl Ids {
    fn from_state(state: &Value) -> Ids {
        let mut by_placeholder = BTreeMap::new();
        for r in state["roles"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
        {
            if let (Some(name), Some(id)) = (r["name"].as_str(), r["role_id"].as_str()) {
                by_placeholder.insert(format!("{{{{role:{name}}}}}"), id.to_owned());
            }
        }
        for r in state["responsibilities"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
        {
            if let (Some(p), Some(id)) = (r["purpose"].as_str(), r["responsibility_id"].as_str()) {
                by_placeholder.insert(format!("{{{{resp:{p}}}}}"), id.to_owned());
            }
        }
        Ids { by_placeholder }
    }
    /// Replace placeholders with this organization's IDs.
    pub fn fill(&self, text: &str) -> String {
        let mut s = text.to_owned();
        for (k, v) in &self.by_placeholder {
            s = s.replace(k, v);
        }
        s
    }
    /// Replace this organization's IDs with placeholders.
    pub fn abstract_ids(&self, text: &str) -> String {
        let mut s = text.to_owned();
        for (k, v) in &self.by_placeholder {
            s = s.replace(v, k);
        }
        s
    }
}

/// Replays recorded responses for one run, filling placeholders.
pub struct Replay {
    responses: Mutex<VecDeque<String>>,
}

impl Replay {
    pub fn new(responses: Vec<String>, ids: &Ids) -> Self {
        Replay {
            responses: Mutex::new(responses.iter().map(|r| ids.fill(r)).collect()),
        }
    }
}

#[async_trait]
impl LlmClient for Replay {
    fn model(&self) -> String {
        "replay".into()
    }
    async fn complete(&self, _req: &LlmRequest) -> Result<String, LlmError> {
        self.responses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front()
            .ok_or(LlmError::ReplayExhausted)
    }
}

/// Wraps a client and keeps its responses.
struct Recording {
    inner: Arc<dyn LlmClient>,
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl LlmClient for Recording {
    fn model(&self) -> String {
        self.inner.model()
    }
    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError> {
        let out = self.inner.complete(req).await?;
        self.seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(out.clone());
        Ok(out)
    }
}

/// Supplies the model for one run, given the seeded organization's IDs.
pub type ModelFactory<'a> =
    dyn Fn(&Scenario, u32, &Ids) -> anyhow::Result<Arc<dyn LlmClient>> + Send + Sync + 'a;

pub struct EvalOptions {
    pub max_attempts: u32,
    /// How long to wait for a proposed change to reach a terminal status.
    pub commit_timeout: Duration,
}

impl Default for EvalOptions {
    fn default() -> Self {
        EvalOptions {
            max_attempts: 3,
            commit_timeout: Duration::from_secs(15),
        }
    }
}

struct Org {
    url: String,
    operator: String,
    agent: String,
    http: reqwest::Client,
}

impl Org {
    async fn start() -> anyhow::Result<Org> {
        let cfg = OrgConfig::dev("org_eval", "Evaluation org");
        let app = build_org(&cfg, None)?;
        let addr = spawn_org(app, "127.0.0.1:0").await?;
        Ok(Org {
            url: format!("http://{addr}"),
            operator: cfg.tokens.operator.clone(),
            agent: cfg.tokens.agent.clone(),
            http: reqwest::Client::new(),
        })
    }
    async fn post(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        let r = self
            .http
            .post(format!("{}/api/v1{path}", self.url))
            .bearer_auth(&self.operator)
            .json(body)
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        anyhow::ensure!(status.is_success(), "POST {path}: {status} {v}");
        Ok(v)
    }
    async fn get(&self, path: &str) -> anyhow::Result<Value> {
        let r = self
            .http
            .get(format!("{}/api/v1{path}", self.url))
            .bearer_auth(&self.operator)
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        anyhow::ensure!(status.is_success(), "GET {path}: {status} {v}");
        Ok(v)
    }
    async fn wait_terminal(&self, change_id: &str, timeout: Duration) -> anyhow::Result<String> {
        let start = Instant::now();
        loop {
            let c = self.get(&format!("/org-changes/{change_id}")).await?;
            let status = c["status"].as_str().unwrap_or("").to_owned();
            if ["COMMITTED", "FAILED", "REJECTED", "ABANDONED"].contains(&status.as_str()) {
                return Ok(status);
            }
            if start.elapsed() > timeout {
                return Ok(format!("{status} (timed out)"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// Seed the scenario into `org`. Returns the new observation and the IDs.
async fn seed(org: &Org, s: &Scenario) -> anyhow::Result<(ObservationId, Ids)> {
    let m = org.post("/missions", &s.mission).await?;
    let mission = m["mission_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no mission_id in {m}"))?
        .to_owned();
    let post_obs = |o: &ObservationSpec| {
        let mut body = json!({"source": o.source, "summary": o.summary});
        if let Some(r) = &o.evidence_ref {
            body["evidence_ref"] = json!(r);
        }
        body
    };
    for o in &s.prior_observations {
        org.post(&format!("/missions/{mission}/observations"), &post_obs(o))
            .await?;
    }
    let obs = org
        .post(
            &format!("/missions/{mission}/observations"),
            &post_obs(&s.observation),
        )
        .await?;
    let id = obs["observation_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no observation_id in {obs}"))?;
    let state = org.get("/organization").await?;
    Ok((ObservationId::new(id), Ids::from_state(&state)))
}

/// Run one scenario once.
pub async fn run_scenario(
    s: &Scenario,
    run: u32,
    model: &ModelFactory<'_>,
    opts: &EvalOptions,
) -> RunResult {
    let mut r = RunResult {
        scenario: s.id.clone(),
        run,
        model: String::new(),
        skipped: None,
        error: None,
        verdict: None,
        change_status: None,
        ops: vec![],
        escalation: None,
        escalation_by_model: None,
        attempts: 0,
        latency_ms: 0,
        checks: vec![],
        passed: false,
        responses: vec![],
    };
    let missing: Vec<_> = s
        .requires_ops
        .iter()
        .filter(|op| !prompt_offers(op))
        .cloned()
        .collect();
    if !missing.is_empty() {
        r.skipped = Some(format!(
            "planner prompt does not offer {}",
            missing.join(", ")
        ));
        return r;
    }
    if let Err(e) = execute(s, run, model, opts, &mut r).await {
        r.error = Some(format!("{e:#}"));
        r.passed = false;
    }
    r
}

async fn execute(
    s: &Scenario,
    run: u32,
    model: &ModelFactory<'_>,
    opts: &EvalOptions,
    r: &mut RunResult,
) -> anyhow::Result<()> {
    let org = Org::start().await?;
    let (obs, ids) = seed(&org, s).await?;
    let inner = model(s, run, &ids)?;
    let recording = Arc::new(Recording {
        inner,
        seen: Mutex::new(vec![]),
    });
    r.model = recording.model();
    let mut planner = Planner::new(KernelClient::new(&org.url, &org.agent), recording.clone());
    planner.specialists = s.specialists.clone();
    planner.max_attempts = opts.max_attempts;
    let start = Instant::now();
    let outcome = planner.plan(&obs).await;
    r.latency_ms = start.elapsed().as_millis() as u64;
    r.responses = recording
        .seen
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|t| ids.abstract_ids(t))
        .collect();
    let mut proposal: Option<Value> = None;
    match outcome? {
        PlanOutcome::Proposed { change, attempts } => {
            r.verdict = Some(Verdict::Change);
            r.attempts = attempts;
            r.change_status = Some(
                org.wait_terminal(&change.change_id.to_string(), opts.commit_timeout)
                    .await?,
            );
            proposal = Some(serde_json::to_value(&change.proposal)?);
        }
        PlanOutcome::NoChange { attempts } => {
            r.verdict = Some(Verdict::NoChange);
            r.attempts = attempts;
        }
        PlanOutcome::Escalated { reason, attempts } => {
            r.verdict = Some(Verdict::Escalate);
            r.attempts = attempts;
            let by_model = r.responses.last().is_some_and(|t| {
                genesis_planner::output::parse(t)
                    .is_ok_and(|o| o.verdict == genesis_planner::output::Verdict::Escalate)
            });
            r.escalation_by_model = Some(by_model);
            if r.responses.is_empty() {
                // No model reply at all: an evaluation error, not a decision.
                r.error = Some(format!("model produced no response: {reason}"));
            }
            r.escalation = Some(reason);
        }
    }
    let ops: Vec<Value> = proposal
        .as_ref()
        .and_then(|p| p["operations"].as_array().cloned())
        .unwrap_or_default();
    r.ops = ops
        .iter()
        .map(|o| o["op"].as_str().unwrap_or("?").to_owned())
        .collect();
    let roles: BTreeMap<String, String> = ids
        .by_placeholder
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("{{role:")
                .and_then(|n| n.strip_suffix("}}"))
                .map(|name| (v.clone(), name.to_owned()))
        })
        .collect();
    r.checks = score(s, r, &ops, &roles);
    r.passed = r.error.is_none() && r.checks.iter().all(|c| c.ok);
    Ok(())
}

/// `scheme://host[:port]` of an http(s) URL, lowercased.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// Score a finished run. Pure: depends only on the scenario, the outcome, the
/// proposed ops and the seeded roles (`role_id` → name).
pub fn score(
    s: &Scenario,
    r: &RunResult,
    ops: &[Value],
    roles: &BTreeMap<String, String>,
) -> Vec<Check> {
    let e = &s.expect;
    let mut checks = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        checks.push(Check {
            name: name.into(),
            ok,
            detail,
        })
    };
    let verdict_ok = r.verdict.is_some_and(|v| e.verdicts.contains(&v));
    check(
        "verdict",
        verdict_ok,
        format!("got {:?}, accept {:?}", r.verdict, e.verdicts),
    );
    if let Some(reason) = &r.escalation {
        // An escalation counts only when the model chose it. The planner's own
        // fallback (no reply, unusable output, rejections exhausted) is not a
        // decision about the evidence.
        check(
            "model_decision",
            r.escalation_by_model == Some(true),
            if r.escalation_by_model == Some(true) {
                "the model's reply was an escalate verdict".into()
            } else {
                format!(
                    "planner fallback after {} attempt(s): {}",
                    r.attempts,
                    reason.chars().take(160).collect::<String>()
                )
            },
        );
    }
    let names: Vec<&str> = ops.iter().filter_map(|o| o["op"].as_str()).collect();
    if r.verdict == Some(Verdict::Change) {
        if e.must_commit {
            let status = r.change_status.clone().unwrap_or_default();
            check("committed", status == "COMMITTED", status);
        }
        if !e.require_any_op.is_empty() {
            let hit = e
                .require_any_op
                .iter()
                .any(|op| names.contains(&op.as_str()));
            check(
                "require_any_op",
                hit,
                format!("ops {names:?}, need one of {:?}", e.require_any_op),
            );
        }
    }
    if !e.forbid_ops.is_empty() {
        let bad: Vec<_> = names
            .iter()
            .filter(|n| e.forbid_ops.iter().any(|f| f == *n))
            .collect();
        check(
            "forbid_ops",
            bad.is_empty(),
            format!("forbidden present: {bad:?}"),
        );
    }
    let new_roles: Vec<&Value> = ops.iter().filter(|o| o["op"] == "CreateRole").collect();
    if let Some(max) = e.max_new_roles {
        check(
            "max_new_roles",
            new_roles.len() <= max,
            format!("{} new, max {max}", new_roles.len()),
        );
    }
    if let Some(max) = e.max_new_budget_cents {
        let total: u64 = new_roles
            .iter()
            .filter_map(|o| o["authority"]["budget_cents"].as_u64())
            .sum();
        check(
            "max_new_budget_cents",
            total <= max,
            format!("{total} granted, max {max}"),
        );
    }
    if !e.forbid_scopes.is_empty() {
        let bad: Vec<String> = new_roles
            .iter()
            .flat_map(|o| {
                o["authority"]["scopes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .filter_map(|v| v.as_str().map(str::to_owned))
            .filter(|sc| e.forbid_scopes.contains(sc))
            .collect();
        check("forbid_scopes", bad.is_empty(), format!("granted: {bad:?}"));
    }
    if let (Some(owner), Some(Verdict::Change)) = (&e.owner_role, r.verdict) {
        let named = |v: &Value| -> String {
            match (v["kind"].as_str(), v["role_id"].as_str(), v["key"].as_str()) {
                (Some("existing"), Some(id), _) => roles
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("unknown role {id}")),
                (Some("new"), _, Some(key)) => format!("new role `{key}`"),
                _ => "unrecognized role reference".into(),
            }
        };
        let targets: Vec<(String, String)> = ops
            .iter()
            .filter_map(|o| match o["op"].as_str() {
                Some("AssignResponsibility") => {
                    Some(("AssignResponsibility".into(), named(&o["owner"])))
                }
                Some("ReassignResponsibility") => {
                    Some(("ReassignResponsibility".into(), named(&o["to"])))
                }
                Some("RequestCapability") => Some(("RequestCapability".into(), named(&o["role"]))),
                _ => None,
            })
            .collect();
        let wrong: Vec<_> = targets.iter().filter(|(_, n)| n != owner).collect();
        let assigned = targets
            .iter()
            .any(|(op, n)| n == owner && op != "RequestCapability");
        check(
            "owner_role",
            wrong.is_empty() && assigned,
            if wrong.is_empty() && !assigned {
                format!("nothing assigned to {owner}")
            } else {
                format!("expected {owner}; got {targets:?}")
            },
        );
    }
    if let Some(max) = e.max_attempts {
        check(
            "max_attempts",
            r.attempts <= max,
            format!("{} used, max {max}", r.attempts),
        );
    }
    // A requested capability must point at the site the evidence came from.
    let evidence = s.observation.evidence_ref.as_deref().and_then(origin);
    for o in ops.iter().filter(|o| o["op"] == "RequestCapability") {
        let sources: Vec<&str> = o["observed_sources"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let ok = !sources.is_empty()
            && evidence.is_some()
            && sources.iter().all(|u| origin(u) == evidence);
        check(
            "capability_sources_on_evidence_origin",
            ok,
            format!("sources {sources:?}, evidence origin {evidence:?}"),
        );
    }
    checks
}

/// Aggregate figures over many runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub model: String,
    pub runs: usize,
    pub skipped: usize,
    pub passed: usize,
    pub errors: usize,
    /// Passed / (runs - skipped).
    pub pass_rate: f64,
    /// Proposed changes the kernel committed / proposed changes.
    pub commit_rate: Option<f64>,
    pub mean_attempts: f64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    /// scenario → (passed, ran)
    pub by_scenario: BTreeMap<String, (usize, usize)>,
}

pub fn summarize(results: &[RunResult]) -> Summary {
    let ran: Vec<&RunResult> = results.iter().filter(|r| r.skipped.is_none()).collect();
    let passed = ran.iter().filter(|r| r.passed).count();
    let proposed: Vec<_> = ran
        .iter()
        .filter(|r| r.verdict == Some(Verdict::Change))
        .collect();
    let committed = proposed
        .iter()
        .filter(|r| r.change_status.as_deref() == Some("COMMITTED"))
        .count();
    let mut lat: Vec<u64> = ran.iter().map(|r| r.latency_ms).collect();
    lat.sort_unstable();
    let pct = |p: f64| -> u64 {
        if lat.is_empty() {
            0
        } else {
            lat[((lat.len() as f64 - 1.0) * p).round() as usize]
        }
    };
    let mut by_scenario: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in &ran {
        let e = by_scenario.entry(r.scenario.clone()).or_default();
        e.1 += 1;
        if r.passed {
            e.0 += 1;
        }
    }
    let mut models: Vec<&str> = ran
        .iter()
        .map(|r| r.model.as_str())
        .filter(|m| !m.is_empty())
        .collect();
    models.sort_unstable();
    models.dedup();
    Summary {
        model: models.join(", "),
        runs: results.len(),
        skipped: results.len() - ran.len(),
        passed,
        errors: ran.iter().filter(|r| r.error.is_some()).count(),
        pass_rate: if ran.is_empty() {
            0.0
        } else {
            passed as f64 / ran.len() as f64
        },
        commit_rate: (!proposed.is_empty()).then(|| committed as f64 / proposed.len() as f64),
        mean_attempts: if ran.is_empty() {
            0.0
        } else {
            ran.iter().map(|r| r.attempts as f64).sum::<f64>() / ran.len() as f64
        },
        p50_ms: pct(0.5),
        p95_ms: pct(0.95),
        by_scenario,
    }
}

/// A Markdown report: summary line, then one row per run.
pub fn markdown(summary: &Summary, results: &[RunResult]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "**{}**: {}/{} passed ({:.0}%), {} skipped, {} errors, commit rate {}, mean attempts {:.2}, p50 {} ms, p95 {} ms\n\n",
        if summary.model.is_empty() { "no model" } else { &summary.model },
        summary.passed,
        summary.runs - summary.skipped,
        summary.pass_rate * 100.0,
        summary.skipped,
        summary.errors,
        summary
            .commit_rate
            .map(|c| format!("{:.0}%", c * 100.0))
            .unwrap_or_else(|| "n/a".into()),
        summary.mean_attempts,
        summary.p50_ms,
        summary.p95_ms,
    ));
    s.push_str(
        "| scenario | run | result | verdict | ops | failed checks |\n|---|---|---|---|---|---|\n",
    );
    for r in results {
        let result = if r.skipped.is_some() {
            "skipped"
        } else if r.passed {
            "pass"
        } else {
            "FAIL"
        };
        let failed: Vec<String> = match (&r.skipped, &r.error) {
            (Some(why), _) => vec![why.clone()],
            (_, Some(err)) => vec![format!(
                "error: {}",
                err.chars().take(120).collect::<String>()
            )],
            _ => r
                .checks
                .iter()
                .filter(|c| !c.ok)
                .map(|c| format!("{}: {}", c.name, c.detail))
                .collect(),
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            r.scenario,
            r.run,
            result,
            r.verdict
                .map(|v| format!("{v:?}"))
                .unwrap_or_else(|| "-".into()),
            if r.ops.is_empty() {
                "-".into()
            } else {
                r.ops.join(", ")
            },
            failed.join("; ").replace('|', "\\|"),
        ));
    }
    s
}

/// Recorded responses for `scenario`/`run` in `dir`: `<id>.json` for run 0,
/// `<id>.r<run>.json` for later runs, falling back to `<id>.json`.
pub fn replay_file(dir: &Path, scenario: &str, run: u32) -> std::path::PathBuf {
    let specific = dir.join(format!("{scenario}.r{run}.json"));
    if run > 0 && specific.exists() {
        specific
    } else {
        dir.join(format!("{scenario}.json"))
    }
}

pub fn record_file(dir: &Path, scenario: &str, run: u32) -> std::path::PathBuf {
    if run == 0 {
        dir.join(format!("{scenario}.json"))
    } else {
        dir.join(format!("{scenario}.r{run}.json"))
    }
}

/// Model factory that replays `<dir>/<scenario>.json` (an array of strings).
pub fn replay_factory(
    dir: std::path::PathBuf,
) -> impl Fn(&Scenario, u32, &Ids) -> anyhow::Result<Arc<dyn LlmClient>> + Send + Sync {
    move |s, run, ids| {
        let path = replay_file(&dir, &s.id, run);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        let responses: Vec<String> =
            serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        Ok(Arc::new(Replay::new(responses, ids)) as Arc<dyn LlmClient>)
    }
}
