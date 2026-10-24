//! PR74 review: real model HTTP classification and Elastic reconciliation under
//! retry. All endpoints are loopback; no account, model or provider is called.
use genesis_elastic::{AgentBuilder, AgentBuilderConfig};
use genesis_planner::elastic::ElasticAgentLlm;
use genesis_planner::llm::{
    AnthropicClient, LlmClient, LlmError, LlmRequest, Message, OpenAiCompatClient, Retrying,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, PartialEq)]
struct Request {
    method: String,
    path: String,
    body: Value,
}
struct Http {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Http {
    async fn start(mut reply: impl FnMut(&Request) -> (u16, Value) + Send + 'static) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let capture = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() < 1024 * 1024);
                    if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let mut line = headers.lines().next().unwrap().split_whitespace();
                let method = line.next().unwrap().to_owned();
                let path = line.next().unwrap().to_owned();
                let length = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                assert!(length < 1024 * 1024);
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                }
                let request = Request {
                    method,
                    path,
                    body: if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                    },
                };
                capture.lock().unwrap().push(request.clone());
                let (status, body) = reply(&request);
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }
    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Http {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn req() -> LlmRequest {
    LlmRequest {
        system: "Only propose scoped changes".into(),
        messages: vec![Message::user("PRIVATE_MISSION_CONTEXT")],
        max_tokens: 100,
    }
}
fn retry(client: Arc<dyn LlmClient>) -> Retrying {
    Retrying::with_delays(client, vec![Duration::from_millis(1); 2])
}
fn model(url: &str, anthropic: bool) -> Arc<dyn LlmClient> {
    if anthropic {
        Arc::new(AnthropicClient::new(
            "fixture-key",
            "fixture-model",
            Some(url.into()),
        ))
    } else {
        Arc::new(OpenAiCompatClient::new(
            "fixture-key",
            "fixture-model",
            url,
            "fixture",
        ))
    }
}
fn success(anthropic: bool) -> Value {
    if anthropic {
        json!({"content":[{"type":"text","text":"reply"}]})
    } else {
        json!({"choices":[{"message":{"content":"reply"}}]})
    }
}

#[tokio::test]
async fn busy_http_retries_identical_requests_through_both_real_model_clients() {
    for anthropic in [true, false] {
        let mut replies = VecDeque::from([
            (429, json!({"error":{"type":"rate_limit_error"}})),
            (529, json!({"error":{"type":"overloaded_error"}})),
            (200, success(anthropic)),
        ]);
        let server = Http::start(move |_| replies.pop_front().expect("no fourth call")).await;
        assert_eq!(
            retry(model(&server.url, anthropic))
                .complete(&req())
                .await
                .unwrap(),
            "reply"
        );
        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0], requests[1]);
        assert_eq!(requests[0], requests[2]);
    }
}

#[tokio::test]
async fn definite_errors_and_malformed_success_are_not_retried() {
    for anthropic in [true, false] {
        for (status, body) in [
            (
                401,
                json!({"error":{"type":"authentication_error","message":"SECRET_CANARY"}}),
            ),
            (200, json!({"wrong_response_shape":true})),
        ] {
            let server = Http::start(move |_| (status, body.clone())).await;
            let error = retry(model(&server.url, anthropic))
                .complete(&req())
                .await
                .unwrap_err();
            assert_eq!(server.requests().len(), 1);
            assert!(!error.to_string().contains("SECRET_CANARY"));
            if status == 401 {
                assert!(matches!(error, LlmError::Http { status: 401, .. }));
            } else {
                assert!(matches!(error, LlmError::Malformed(_)));
            }
        }
    }
}

async fn elastic(drift: bool) -> (Http, Result<String, LlmError>) {
    let mut objects = BTreeMap::<String, Value>::new();
    let mut conversations = 0;
    let server = Http::start(move |r| {
        let path = r.path.strip_prefix("/api/agent_builder/").unwrap();
        if r.method == "GET" {
            return objects.get(path).map(|v| (200,v.clone())).unwrap_or((404,json!({})));
        }
        if path == "converse" {
            conversations += 1;
            if conversations == 1 {
                if drift {
                    objects.get_mut(&format!("agents/{}",r.body["agent_id"].as_str().unwrap())).unwrap()["configuration"]["instructions"] = json!("Changed remote instructions");
                }
                return (503,json!({"message":"busy PRIVATE_RESPONSE_CANARY"}));
            }
            return (200,json!({"conversation_id":"conversation","status":"completed","response":{"message":"reply"}}));
        }
        assert!(matches!(path,"tools"|"agents"));
        let key = format!("{path}/{}",r.body["id"].as_str().unwrap());
        assert!(objects.insert(key,r.body.clone()).is_none(),"retry created a duplicate resource");
        (200,r.body.clone())
    }).await;
    let builder = AgentBuilder::new(AgentBuilderConfig {
        kibana_url: server.url.clone(),
        api_key: "fixture".into(),
        space: None,
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(3),
    })
    .unwrap();
    let result = retry(Arc::new(ElasticAgentLlm::new(builder, "org_b")))
        .complete(&req())
        .await;
    (server, result)
}

#[tokio::test]
async fn elastic_busy_retry_reconciles_existing_agent_and_tool_before_conversing() {
    let (server, result) = elastic(false).await;
    assert_eq!(result.unwrap(), "reply");
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.method == "POST" && r.path.ends_with("/agents"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.method == "POST" && r.path.ends_with("/tools"))
            .count(),
        1
    );
    let calls: Vec<_> = requests
        .iter()
        .filter(|r| r.path.ends_with("/converse"))
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], calls[1]);
    let first = requests
        .iter()
        .position(|r| r.path.ends_with("/converse"))
        .unwrap();
    assert!(requests[first + 1..requests.len() - 1]
        .iter()
        .all(|r| r.method == "GET"));
    assert_eq!(requests[first + 1..requests.len() - 1].len(), 2);
}

#[tokio::test]
async fn elastic_busy_retry_refuses_drift_before_resending_mission_context() {
    let (server, result) = elastic(true).await;
    let error = result.unwrap_err();
    assert!(matches!(error, LlmError::Malformed(_)));
    assert!(!error.to_string().contains("PRIVATE_RESPONSE_CANARY"));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path.ends_with("/converse"))
            .count(),
        1
    );
}
