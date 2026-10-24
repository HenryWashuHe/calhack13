//! Review-only regressions for PR12. Interrupted planning must not duplicate effects.
use genesis_planner::{agent::Planner, kernel::KernelClient, llm::ReplayClient, runner::Runner};
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct Org {
    url: String,
    operator: String,
    agent: String,
    http: reqwest::Client,
}
impl Org {
    async fn start() -> Self {
        let cfg = OrgConfig::dev("org_review", "Review");
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
    async fn seed(&self) -> String {
        let mission = self.post("/missions", json!({"goal":"Recover from a disruption", "roles":[{"key":"lead","name":"Lead","charter":"Own the plan","authority":{"scopes":["mission.coordinate"],"budget_cents":10000}}],"responsibilities":[{"purpose":"Coordinate the plan","owner":"lead"}]})).await;
        self.post(
            &format!(
                "/missions/{}/observations",
                mission["mission_id"].as_str().unwrap()
            ),
            json!({"source":"test","summary":"The planned location is unavailable"}),
        )
        .await;
        let s = self.get("/organization").await;
        s["roles"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone()
    }
    async fn wait_quiet(&self) {
        for _ in 0..100 {
            let changes = self.get("/org-changes").await;
            if changes.as_array().unwrap().iter().all(|c| {
                ["COMMITTED", "FAILED", "REJECTED", "CONFLICTED"]
                    .contains(&c["status"].as_str().unwrap_or(""))
            }) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("saga did not finish");
    }
}
fn reply(lead: &str) -> String {
    json!({"assessment":"The disruption needs a continuing owner","verdict":"change","requirement":{"summary":"Restore the plan","acceptance":"An alternative is recorded"},"rationale":"Independent ongoing work","budget_limit_cents":50000,"operations":[{"op":"CreateRole","key":"r","name":"Recovery lead","charter":"Restore the plan","authority":{"scopes":["ledger.reserve_venue"],"budget_cents":20000},"reports_to":{"kind":"existing","role_id":lead},"provider":"elastic","justification":"independent_ownership"},{"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"work","requirement_id":"$REQUIREMENT","purpose":"Restore the plan","trigger":"new evidence","termination":"restored"},"owner":{"kind":"new","key":"r"}}]}).to_string()
}
#[derive(Clone, Copy)]
enum Fault {
    RequirementResponse,
    ChangeResponse,
    DecisionRequest,
    DecisionResponse,
}
impl Fault {
    fn matches(self, path: &str) -> bool {
        match self {
            Self::RequirementResponse => path.ends_with("/requirements"),
            Self::ChangeResponse => path == "/api/v1/org-changes",
            Self::DecisionRequest | Self::DecisionResponse => path == "/api/v1/planner/decisions",
        }
    }
}
struct Proxy {
    url: String,
    fired: Arc<AtomicBool>,
    change_keys: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn proxy(org: &Org, fault: Fault) -> Proxy {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let backend = org.url.clone();
    let fired = Arc::new(AtomicBool::new(false));
    let change_keys = Arc::new(Mutex::new(Vec::new()));
    let f = fired.clone();
    let k = change_keys.clone();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let backend = backend.clone();
            let f = f.clone();
            let k = k.clone();
            tokio::spawn(async move {
                forward(stream, &backend, fault, f, k).await;
            });
        }
    });
    Proxy {
        url,
        fired,
        change_keys,
        task,
    }
}
async fn forward(
    mut stream: TcpStream,
    backend: &str,
    fault: Fault,
    fired: Arc<AtomicBool>,
    keys: Arc<Mutex<Vec<String>>>,
) {
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
        assert!(data.len() < 128 * 1024);
    };
    let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
    let mut words = headers.lines().next().unwrap().split_whitespace();
    let method = words.next().unwrap();
    let path = words.next().unwrap();
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
    if method == "POST" && path == "/api/v1/org-changes" {
        let v: Value = serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
        keys.lock()
            .unwrap()
            .push(v["client_request_id"].as_str().unwrap().to_owned());
    }
    let fail = method == "POST" && fault.matches(path) && !fired.swap(true, Ordering::SeqCst);
    if fail && matches!(fault, Fault::DecisionRequest) {
        stream.shutdown().await.unwrap();
        return;
    }
    let client = reqwest::Client::new();
    let mut req = client.request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
        format!("{backend}{path}"),
    );
    for (key, value) in headers.lines().skip(1).filter_map(|l| l.split_once(':')) {
        if key.eq_ignore_ascii_case("authorization") || key.eq_ignore_ascii_case("content-type") {
            req = req.header(key, value.trim());
        }
    }
    if length > 0 {
        req = req.body(data[header_end..header_end + length].to_vec());
    }
    let response = req.send().await.unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    if fail {
        assert!(
            status.is_success(),
            "intercepted action must have committed: {status}"
        );
        stream.shutdown().await.unwrap();
        return;
    }
    let response_header=format!("HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",status.as_u16(),status.canonical_reason().unwrap_or("Unknown"),bytes.len());
    stream.write_all(response_header.as_bytes()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    stream.shutdown().await.unwrap();
}
async fn run_fault(
    fault: Fault,
    expected_requirements: usize,
    expected_roles: usize,
    expected_calls: usize,
    expected_changes: usize,
) {
    let org = Org::start().await;
    let lead = org.seed().await;
    let p = proxy(&org, fault).await;
    let model = Arc::new(ReplayClient::new([reply(&lead), reply(&lead)]));
    let mut runner = Runner::new(
        Planner::new(KernelClient::new(&p.url, &org.agent), model.clone()),
        Duration::from_millis(1),
    );
    assert!(runner.tick().await.unwrap().is_empty());
    assert!(p.fired.load(Ordering::SeqCst));
    org.wait_quiet().await;
    // Restart from durable kernel events, with no previous in-memory planner state.
    let mut restarted = Runner::new(
        Planner::new(KernelClient::new(&p.url, &org.agent), model.clone()),
        Duration::from_millis(1),
    );
    restarted.tick().await.unwrap();
    org.wait_quiet().await;
    let state = org.get("/organization").await;
    assert_eq!(
        state["requirements"].as_object().unwrap().len(),
        expected_requirements
    );
    assert_eq!(state["roles"].as_object().unwrap().len(), expected_roles);
    assert_eq!(model.requests().len(), expected_calls);
    let keys = p.change_keys.lock().unwrap();
    assert_eq!(keys.len(), expected_changes);
}
#[tokio::test]
async fn lost_requirement_response_must_recover_without_an_orphan() {
    run_fault(Fault::RequirementResponse, 1, 2, 1, 1).await;
}
#[tokio::test]
async fn lost_change_response_must_not_duplicate_roles_on_restart() {
    run_fault(Fault::ChangeResponse, 1, 2, 1, 1).await;
}
#[tokio::test]
async fn missing_decision_after_accepted_change_must_not_duplicate_roles() {
    run_fault(Fault::DecisionRequest, 1, 2, 1, 1).await;
}
#[tokio::test]
async fn lost_response_after_decision_is_persisted_is_recovered_without_duplicate() {
    run_fault(Fault::DecisionResponse, 1, 2, 1, 1).await;
}
