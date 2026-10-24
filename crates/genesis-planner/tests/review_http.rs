//! Review regressions against PR12 5f03bb5. Real public HTTP clients, loopback
//! servers only. The expected safe assertions intentionally fail on that head.

use chrono::Utc;
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::events::EventKind;
use genesis_core::ids::ObservationId;
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::{AnthropicClient, LlmClient, LlmRequest, Message};
use genesis_server::app::AppState;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, serve_org, OrgConfig};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const KEY_CANARY: &str = "review-fake-anthropic-key-canary";
const CONTEXT_CANARY: &str = "review-private-context-canary";

#[derive(Clone, Debug)]
struct CapturedRequest {
    request_line: String,
    headers: BTreeMap<String, String>,
    body: String,
}

fn receive(stream: &mut TcpStream) -> CapturedRequest {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "client closed before request headers");
        bytes.extend_from_slice(&chunk[..read]);
        assert!(bytes.len() <= 1024 * 1024, "fixture request exceeds cap");
        if let Some(position) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let text = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap().to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(length <= 1024 * 1024, "fixture body exceeds cap");
    while bytes.len() < header_end + length {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "client closed before request body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    CapturedRequest {
        request_line,
        headers,
        body: String::from_utf8(bytes[header_end..header_end + length].to_vec()).unwrap(),
    }
}

struct Loopback {
    url: String,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Loopback {
    fn start(status: &str, extra_headers: &str, body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        );
        let captured = requests.clone();
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = receive(&mut stream);
                        captured.lock().unwrap().push(request);
                        stream.write_all(response.as_bytes()).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback accept failed: {error}"),
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Org {
    app: AppState,
    url: String,
    task: tokio::task::JoinHandle<()>,
    observation: ObservationId,
}

impl Org {
    async fn start() -> Self {
        let config = OrgConfig::dev("review-http", "Review HTTP org");
        let app = build_org(&config, None).unwrap();
        let initial = app.current_state().unwrap();
        let bootstrap: MissionBootstrap = serde_json::from_value(json!({
            "goal":"Review planner HTTP privacy",
            "roles":[{"key":"lead","name":"Lead","charter":"Coordinate review",
                "authority":{"scopes":["mission.coordinate"],"budget_cents":0}}],
            "responsibilities":[{"purpose":"Review behavior","owner":"lead"}],
        }))
        .unwrap();
        let (mission, mission_id) =
            engine::bootstrap_mission(&initial, &bootstrap, Utc::now()).unwrap();
        app.store
            .commit_state(
                initial.revision,
                &mission.state,
                mission.effects,
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        let (observed, observation) = engine::record_observation(
            &mission.state,
            &mission_id,
            "review-fixture",
            CONTEXT_CANARY,
            None,
            Utc::now(),
        )
        .unwrap();
        app.store
            .commit_state(
                mission.state.revision,
                &observed.state,
                observed.effects,
                &EventMeta::default(),
                vec![],
                |_| Ok(()),
            )
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let serving = app.clone();
        let task = tokio::spawn(async move { serve_org(serving, listener).await.unwrap() });
        Self {
            app,
            url,
            task,
            observation,
        }
    }
}

impl Drop for Org {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn provider_error_body_must_not_be_persisted_as_planner_decision() {
    let provider = Loopback::start("401 Unauthorized", "", json!({
        "error":{"message":format!("upstream echoed credential {KEY_CANARY} and context {CONTEXT_CANARY}")}
    }).to_string());
    let org = Org::start().await;
    let planner = Planner::new(
        KernelClient::new(&org.url, "review-http-agent"),
        Arc::new(AnthropicClient::new(
            KEY_CANARY,
            "synthetic-review-model",
            Some(provider.url.clone()),
        )),
    );
    let outcome = tokio::time::timeout(Duration::from_secs(5), planner.plan(&org.observation))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(outcome, PlanOutcome::Escalated { .. }));
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some(KEY_CANARY)
    );
    assert!(requests[0].body.contains(CONTEXT_CANARY));
    let decisions: Vec<String> = org
        .app
        .store
        .events_after(0, 100)
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::PlannerDecision { rationale, .. } => Some(rationale),
            _ => None,
        })
        .collect();
    assert_eq!(
        decisions.len(),
        1,
        "planner must record the attempted model call"
    );
    eprintln!("provider_key_canary_in_persisted_decision={}, provider_context_canary_in_persisted_decision={}",
        decisions[0].contains(KEY_CANARY), decisions[0].contains(CONTEXT_CANARY));
    assert!(!decisions[0].contains(KEY_CANARY) && !decisions[0].contains(CONTEXT_CANARY),
        "raw provider HTTP error copied a credential and private context into the durable PlannerDecision rationale");
}

#[tokio::test]
async fn anthropic_cross_origin_307_must_not_forward_key_or_prompt() {
    let destination = Loopback::start(
        "200 OK",
        "",
        json!({
            "content":[{"type":"text","text":"{}"}]
        })
        .to_string(),
    );
    let source = Loopback::start(
        "307 Temporary Redirect",
        &format!("Location: {}/redirected\r\n", destination.url),
        String::new(),
    );
    let model = AnthropicClient::new(
        KEY_CANARY,
        "synthetic-review-model",
        Some(source.url.clone()),
    );
    let request = LlmRequest {
        system: "Return JSON".into(),
        messages: vec![Message::user(CONTEXT_CANARY)],
        max_tokens: 64,
    };
    let result = tokio::time::timeout(Duration::from_secs(5), model.complete(&request))
        .await
        .unwrap();
    assert_eq!(source.requests().len(), 1);
    let forwarded = destination.requests();
    eprintln!("redirect_target_requests={}, received_key_canary={}, received_prompt_canary={}, forwarded_method={:?}",
        forwarded.len(),
        forwarded.iter().any(|r| r.headers.get("x-api-key").is_some_and(|v| v == KEY_CANARY)),
        forwarded.iter().any(|r| r.body.contains(CONTEXT_CANARY)),
        forwarded.first().map(|r| r.request_line.as_str()));
    assert!(
        forwarded.is_empty(),
        "cross-origin redirect destination received an authenticated model POST"
    );
    assert!(
        result.is_err(),
        "redirect must be surfaced without following it"
    );
}
