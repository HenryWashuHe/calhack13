//! Henry's Claude: pins for demo-critical planner guards that mutation testing
//! found unobserved. Real in-process kernel; a loopback proxy between the
//! planner and the kernel races commits or injects ambiguous responses.
use genesis_core::events::PlannerVerdict;
use genesis_core::ids::ObservationId;
use genesis_planner::agent::{PlanError, PlanOutcome, Planner};
use genesis_planner::kernel::{DecisionRecord, KernelClient, KernelError};
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Org {
    url: String,
    operator: String,
    agent: String,
    http: reqwest::Client,
}

impl Org {
    async fn start() -> Self {
        let cfg = OrgConfig::dev("org_guards", "Guards");
        let addr = spawn_org(build_org(&cfg, None).unwrap(), "127.0.0.1:0")
            .await
            .unwrap();
        Self {
            url: format!("http://{addr}"),
            operator: cfg.tokens.operator,
            agent: cfg.tokens.agent,
            http: reqwest::Client::new(),
        }
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        self.http
            .post(format!("{}/api/v1{path}", self.url))
            .bearer_auth(&self.operator)
            .json(&body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}/api/v1{path}", self.url))
            .bearer_auth(&self.operator)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn decisions(&self) -> Vec<Value> {
        self.get("/events/log?after=0&limit=5000")
            .await
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "planner_decision")
            .cloned()
            .collect()
    }
    async fn mission(&self) -> String {
        let m = self
            .post(
                "/missions",
                json!({"goal":"Recover from a disruption",
            "roles":[{"key":"lead","name":"Lead","charter":"Own the plan",
                "authority":{"scopes":["mission.coordinate"],"budget_cents":10000}}],
            "responsibilities":[{"purpose":"Coordinate the plan","owner":"lead"}]}),
            )
            .await;
        m["mission_id"].as_str().unwrap().to_owned()
    }
    async fn observe(&self, mission: &str, summary: &str) -> ObservationId {
        let o = self
            .post(
                &format!("/missions/{mission}/observations"),
                json!({"source":"test","summary":summary}),
            )
            .await;
        ObservationId::new(o["observation_id"].as_str().unwrap())
    }
    /// (lead role id, mission id, observation id)
    async fn seed(&self) -> (String, String, ObservationId) {
        let mission = self.mission().await;
        let obs = self
            .observe(&mission, "The planned location is unavailable")
            .await;
        let state = self.get("/organization").await;
        let lead = state["roles"]
            .as_object()
            .unwrap()
            .values()
            .find(|r| r["name"] == "Lead")
            .unwrap()["role_id"]
            .as_str()
            .unwrap()
            .to_owned();
        (lead, mission, obs)
    }
}

fn change_reply(lead: &str) -> String {
    json!({"assessment":"The disruption needs a continuing owner","verdict":"change",
        "requirement":{"summary":"Restore the plan","acceptance":"An alternative is recorded"},
        "rationale":"Independent ongoing work","budget_limit_cents":50000,
        "operations":[{"op":"CreateRole","key":"r","name":"Recovery lead","charter":"Restore the plan",
            "authority":{"scopes":["ledger.reserve_venue"],"budget_cents":20000},
            "reports_to":{"kind":"existing","role_id":lead},"provider":"elastic",
            "justification":"independent_ownership"},
          {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"work",
            "requirement_id":"$REQUIREMENT","purpose":"Restore the plan","trigger":"new evidence",
            "termination":"restored"},"owner":{"kind":"new","key":"r"}}]})
    .to_string()
}

fn no_change_reply(assessment: &str, rationale: &str) -> String {
    json!({"assessment":assessment,"verdict":"no_change","requirement":null,
        "rationale":rationale,"operations":[]})
    .to_string()
}

/// What the proxy does to matching requests.
#[derive(Clone, Copy)]
enum Script {
    /// Pass everything through.
    Forward,
    /// Before forwarding each of the first `n` change submissions, commit an
    /// unrelated observation, so the submission's revision is stale.
    RaceSubmits(usize),
    /// Answer the first `n` decision writes with `status` without forwarding
    /// them (the decision never reaches the kernel).
    FailDecisions(usize, u16),
    /// Answer the first change submission with 503 without forwarding it.
    FailFirstSubmit,
}

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    body: Vec<u8>,
    status: u16,
    response: Vec<u8>,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
    fn response_json(&self) -> Value {
        serde_json::from_slice(&self.response).unwrap_or(Value::Null)
    }
}

struct Proxy {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Proxy {
    fn all(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
    fn posts(&self, path: &str) -> Vec<Seen> {
        self.all()
            .into_iter()
            .filter(|s| s.method == "POST" && s.path == path)
            .collect()
    }
    fn change_keys(&self) -> Vec<String> {
        self.posts("/api/v1/org-changes")
            .iter()
            .map(|s| s.json()["client_request_id"].as_str().unwrap().to_owned())
            .collect()
    }
}

struct Counters {
    submits: usize,
    decisions: usize,
}

async fn proxy(org: &Org, mission: &str, script: Script) -> Proxy {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let counters = Arc::new(Mutex::new(Counters {
        submits: 0,
        decisions: 0,
    }));
    let ctx = Arc::new((
        org.url.clone(),
        org.operator.clone(),
        mission.to_owned(),
        script,
    ));
    let log = seen.clone();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (log, counters, ctx) = (log.clone(), counters.clone(), ctx.clone());
            tokio::spawn(async move { handle(stream, log, counters, ctx).await });
        }
    });
    Proxy { url, seen, task }
}

async fn respond(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    stream.shutdown().await.unwrap();
}

async fn handle(
    mut stream: TcpStream,
    log: Arc<Mutex<Vec<Seen>>>,
    counters: Arc<Mutex<Counters>>,
    ctx: Arc<(String, String, String, Script)>,
) {
    let (backend, operator, mission, script) = (&ctx.0, &ctx.1, &ctx.2, ctx.3);
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut buf).await.unwrap();
        if n == 0 {
            return;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
    let mut words = headers.lines().next().unwrap().split_whitespace();
    let method = words.next().unwrap().to_owned();
    let path = words.next().unwrap().to_owned();
    let length = headers
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| v.trim().parse::<usize>().unwrap())
        .unwrap_or(0);
    while data.len() < header_end + length {
        let n = stream.read(&mut buf).await.unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&buf[..n]);
    }
    let body = data[header_end..header_end + length].to_vec();
    let record = |status: u16, response: &[u8]| {
        log.lock().unwrap().push(Seen {
            method: method.clone(),
            path: path.clone(),
            body: body.clone(),
            status,
            response: response.to_vec(),
        });
    };

    let is_submit = method == "POST" && path == "/api/v1/org-changes";
    let is_decision = method == "POST" && path == "/api/v1/planner/decisions";
    let (submit_n, decision_n) = {
        let mut c = counters.lock().unwrap();
        if is_submit {
            c.submits += 1;
        }
        if is_decision {
            c.decisions += 1;
        }
        (c.submits, c.decisions)
    };
    match script {
        Script::FailDecisions(n, status) if is_decision && decision_n <= n => {
            let reply = br#"{"error":"injected","message":"injected failure"}"#;
            record(status, reply);
            return respond(&mut stream, status, reply).await;
        }
        Script::FailFirstSubmit if is_submit && submit_n == 1 => {
            let reply = br#"{"error":"internal","message":"injected failure"}"#;
            record(503, reply);
            return respond(&mut stream, 503, reply).await;
        }
        Script::RaceSubmits(n) if is_submit && submit_n <= n => {
            // A background worker commits first: the organization moves on
            // while the planner's proposal is in flight.
            reqwest::Client::new()
                .post(format!("{backend}/api/v1/missions/{mission}/observations"))
                .bearer_auth(operator)
                .json(&json!({"source":"background","summary":"Concurrent unrelated update"}))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
        _ => {}
    }

    let mut req = reqwest::Client::new().request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
        format!("{backend}{path}"),
    );
    for (key, value) in headers.lines().skip(1).filter_map(|l| l.split_once(':')) {
        if key.eq_ignore_ascii_case("authorization") || key.eq_ignore_ascii_case("content-type") {
            req = req.header(key, value.trim());
        }
    }
    if length > 0 {
        req = req.body(body.clone());
    }
    let response = req.send().await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap();
    record(status, &bytes);
    respond(&mut stream, status, &bytes).await;
}

fn key(obs: &ObservationId, attempt: u32, submission: u32) -> String {
    format!("planner:{obs}:a{attempt}:s{submission}")
}

// agent.rs: `Err(e) if e.code() == Some("revision_conflict") => ...`
#[tokio::test]
async fn revision_conflict_is_rebuilt_against_fresh_state_without_a_model_call() {
    let org = Org::start().await;
    let (lead, mission, obs) = org.seed().await;
    let p = proxy(&org, &mission, Script::RaceSubmits(1)).await;
    let model = Arc::new(ReplayClient::new([change_reply(&lead)]));
    let planner = Planner::new(KernelClient::new(&p.url, &org.agent), model.clone());

    let outcome = planner.plan(&obs).await.unwrap();

    let submits = p.posts("/api/v1/org-changes");
    assert_eq!(submits.len(), 2, "{submits:?}");
    // The race really produced a kernel revision conflict.
    assert_eq!(submits[0].status, 409);
    assert_eq!(submits[0].response_json()["error"], "revision_conflict");
    // Rebuilt with its own deterministic request ID and the fresh revision.
    assert_eq!(p.change_keys(), vec![key(&obs, 1, 0), key(&obs, 1, 1)]);
    let rev0 = submits[0].json()["proposal"]["expected_org_revision"]
        .as_u64()
        .unwrap();
    let rev1 = submits[1].json()["proposal"]["expected_org_revision"]
        .as_u64()
        .unwrap();
    assert!(rev1 > rev0, "resubmission must use the fresh revision");
    assert_eq!(
        submits[0].json()["proposal"]["operations"],
        submits[1].json()["proposal"]["operations"],
        "the same model proposal is rebuilt, not re-planned"
    );
    // No second model call, no rejection, no escalation.
    assert_eq!(model.requests().len(), 1);
    match &outcome {
        PlanOutcome::Proposed { change, attempts } => {
            assert_eq!(*attempts, 1);
            assert_ne!(
                change.status,
                genesis_core::lifecycle::ChangeStatus::Rejected,
                "{change:?}"
            );
            assert_eq!(
                change.proposal.expected_org_revision, rev1,
                "the accepted change is the rebuilt one"
            );
        }
        other => panic!("revision conflict was not rebuilt: {other:?}"),
    }
    let decisions = org.decisions().await;
    assert_eq!(decisions.len(), 1, "{decisions:?}");
    assert_eq!(decisions[0]["payload"]["verdict"], "change_proposed");
    assert!(decisions[0]["payload"]["kernel_errors"]
        .as_array()
        .unwrap()
        .is_empty());
}

// agent.rs `decide`: `Err(e) if e.is_ambiguous() && tries < 3 => retry`, and
// kernel.rs `KernelError::Api { status, .. } => *status >= 500`.
#[tokio::test]
async fn ambiguous_500_decision_write_is_retried_identically_until_it_lands() {
    let org = Org::start().await;
    let (_, mission, obs) = org.seed().await;
    let p = proxy(&org, &mission, Script::FailDecisions(2, 500)).await;
    let model = Arc::new(ReplayClient::new([no_change_reply(
        "Nothing changes",
        "The plan still holds",
    )]));
    let planner = Planner::new(KernelClient::new(&p.url, &org.agent), model.clone());

    let outcome = planner.plan(&obs).await.unwrap();
    assert_eq!(outcome, PlanOutcome::NoChange { attempts: 1 });

    let writes = p.posts("/api/v1/planner/decisions");
    assert_eq!(
        writes.iter().map(|w| w.status).collect::<Vec<_>>(),
        vec![500, 500, 201]
    );
    assert!(
        writes.iter().all(|w| w.body == writes[0].body),
        "a retried decision must be the identical record"
    );
    assert_eq!(writes[0].json()["observation_id"], obs.to_string());
    assert_eq!(org.decisions().await.len(), 1);
    // agent.rs `if prior.spent > 0`: a fresh observation has no rejection note.
    let requests = model.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages.len(), 1, "{:?}", requests[0].messages);
    assert!(!requests[0].messages[0]
        .content
        .contains("An earlier attempt"));
}

#[tokio::test]
async fn decision_write_gives_up_after_exactly_three_ambiguous_tries() {
    let org = Org::start().await;
    let (_, mission, obs) = org.seed().await;
    let p = proxy(&org, &mission, Script::FailDecisions(3, 500)).await;
    let model = Arc::new(ReplayClient::new([no_change_reply("a", "r")]));
    let planner = Planner::new(KernelClient::new(&p.url, &org.agent), model.clone());

    let err = planner.plan(&obs).await.unwrap_err();
    assert!(
        matches!(err, PlanError::Kernel(KernelError::Api { status: 500, .. })),
        "{err:?}"
    );
    // Three tries, not four: the fourth would have reached the kernel.
    assert_eq!(p.posts("/api/v1/planner/decisions").len(), 3);
    assert!(org.decisions().await.is_empty());
    assert_eq!(model.requests().len(), 1);
}

#[tokio::test]
async fn definite_4xx_decision_failure_is_not_retried() {
    let org = Org::start().await;
    let (_, mission, obs) = org.seed().await;
    let p = proxy(&org, &mission, Script::FailDecisions(1, 422)).await;
    let model = Arc::new(ReplayClient::new([no_change_reply("a", "r")]));
    let planner = Planner::new(KernelClient::new(&p.url, &org.agent), model);
    let err = planner.plan(&obs).await.unwrap_err();
    assert!(
        matches!(err, PlanError::Kernel(KernelError::Api { status: 422, .. })),
        "{err:?}"
    );
    assert_eq!(p.posts("/api/v1/planner/decisions").len(), 1);
}

#[test]
fn only_transport_decode_and_5xx_kernel_errors_are_ambiguous() {
    let api = |status| KernelError::Api {
        status,
        code: "c".into(),
        message: "m".into(),
        details: Value::Null,
    };
    for status in [500, 501, 502, 503, 504, 599] {
        assert!(api(status).is_ambiguous(), "{status}");
        assert!(!api(status).is_rejection(), "{status}");
    }
    for status in [400, 404, 409, 422, 499] {
        assert!(!api(status).is_ambiguous(), "{status}");
        assert!(api(status).is_rejection(), "{status}");
    }
    assert!(KernelError::Transport("x".into()).is_ambiguous());
    assert!(KernelError::Decode("x".into()).is_ambiguous());
}

// An ambiguous submission is reconciled by reading, never resubmitted under a
// new identity; a later pass reuses the identical request ID.
#[tokio::test]
async fn ambiguous_submit_is_reconciled_by_reading_and_retried_under_the_same_request_id() {
    let org = Org::start().await;
    let (lead, mission, obs) = org.seed().await;
    let p = proxy(&org, &mission, Script::FailFirstSubmit).await;
    let model = Arc::new(ReplayClient::new([
        change_reply(&lead),
        change_reply(&lead),
    ]));
    let planner = Planner::new(KernelClient::new(&p.url, &org.agent), model.clone());

    let err = planner.plan(&obs).await.unwrap_err();
    assert!(
        matches!(err, PlanError::Kernel(KernelError::Api { status: 503, .. })),
        "{err:?}"
    );
    assert_eq!(p.change_keys(), vec![key(&obs, 1, 0)]);
    let encoded = key(&obs, 1, 0).replace(':', "%3A");
    assert!(
        p.all()
            .iter()
            .any(|s| s.method == "GET"
                && s.path == format!("/api/v1/org-changes/requests/{encoded}")),
        "the unknown outcome must be read back by its request ID"
    );
    assert!(org.decisions().await.is_empty(), "no terminal decision");

    // The runner's next pass: the same request identity, not a new mutation.
    let outcome = planner.plan(&obs).await.unwrap();
    assert!(matches!(outcome, PlanOutcome::Proposed { attempts: 1, .. }));
    assert_eq!(p.change_keys(), vec![key(&obs, 1, 0), key(&obs, 1, 0)]);
    let state = org.get("/organization").await;
    assert_eq!(state["requirements"].as_object().unwrap().len(), 1);
}

// agent.rs `truncate`: `if s.chars().count() <= n`.
#[tokio::test]
async fn decision_text_at_the_limit_is_kept_whole_and_longer_text_is_cut() {
    let org = Org::start().await;
    let (_, mission, obs) = org.seed().await;
    let exact_summary = "s".repeat(400);
    let long_rationale = "r".repeat(1201);
    let model = Arc::new(ReplayClient::new([no_change_reply(
        &exact_summary,
        &long_rationale,
    )]));
    let p = proxy(&org, &mission, Script::Forward).await;
    let planner = Planner::new(KernelClient::new(&p.url, &org.agent), model);
    planner.plan(&obs).await.unwrap();
    let d = &org.decisions().await[0]["payload"];
    assert_eq!(d["summary"], exact_summary);
    assert_eq!(d["rationale"], format!("{}…", "r".repeat(1200)));
}

// runner.rs: `self.pending.retain(|o| o != observation_id)`.
#[tokio::test]
async fn a_decision_for_one_observation_does_not_drop_other_pending_observations() {
    let org = Org::start().await;
    let (_, mission, first) = org.seed().await;
    let second = org.observe(&mission, "A second, separate report").await;
    // `first` is decided after `second` was recorded, so one event batch
    // holds both observations and then the decision.
    let kernel = KernelClient::new(&org.url, &org.agent);
    kernel
        .record_decision(&DecisionRecord {
            planner: "earlier".into(),
            model: "replay".into(),
            observation_id: first.clone(),
            verdict: PlannerVerdict::NoChange,
            summary: "s".into(),
            rationale: "r".into(),
            attempt: 1,
            latency_ms: 0,
            requirement_id: None,
            change_id: None,
            kernel_errors: vec![],
        })
        .await
        .unwrap();
    let model = Arc::new(ReplayClient::new([no_change_reply("a", "r")]));
    let mut runner = Runner::new(
        Planner::new(kernel, model.clone()),
        Duration::from_millis(1),
    );
    let outcomes = runner.tick().await.unwrap();
    assert_eq!(
        outcomes,
        vec![(second.clone(), PlanOutcome::NoChange { attempts: 1 })]
    );
    assert_eq!(model.requests().len(), 1);
    assert!(model.requests()[0].messages[0]
        .content
        .contains(&format!("- id: {second}")));
}
