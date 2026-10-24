//! Henry's Claude: transient model errors are retried before the planner
//! escalates an observation for good; anything else is returned at once.
use async_trait::async_trait;
use genesis_planner::llm::{transient, LlmClient, LlmError, LlmRequest, Message, Retrying};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Scripted {
    replies: Mutex<VecDeque<Result<String, LlmError>>>,
    calls: Mutex<usize>,
}

impl Scripted {
    fn new(replies: Vec<Result<String, LlmError>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(0),
        })
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

#[async_trait]
impl LlmClient for Scripted {
    fn model(&self) -> String {
        "scripted/test".into()
    }
    async fn complete(&self, _: &LlmRequest) -> Result<String, LlmError> {
        *self.calls.lock().unwrap() += 1;
        self.replies.lock().unwrap().pop_front().unwrap()
    }
}

fn req() -> LlmRequest {
    LlmRequest {
        system: "s".into(),
        messages: vec![Message::user("evidence")],
        max_tokens: 10,
    }
}

fn http(status: u16) -> LlmError {
    LlmError::Http {
        status,
        kind: "test".into(),
    }
}

fn fast(inner: Arc<Scripted>) -> Retrying {
    Retrying::with_delays(
        inner,
        vec![Duration::from_millis(1), Duration::from_millis(1)],
    )
}

#[test]
fn only_busy_or_unreached_providers_count_as_transient() {
    for s in [429, 500, 502, 503, 504, 529] {
        assert!(transient(&http(s)), "{s}");
    }
    for s in [400, 401, 403, 404, 413, 422] {
        assert!(!transient(&http(s)), "{s}");
    }
    assert!(transient(&LlmError::Transport("connect".into())));
    // A timeout may have produced (and billed) a reply; not retried here.
    assert!(!transient(&LlmError::Transport("timeout".into())));
    assert!(!transient(&LlmError::Malformed("x".into())));
    assert!(!transient(&LlmError::NotConfigured("x".into())));
}

#[tokio::test]
async fn an_overloaded_provider_is_retried_until_it_answers() {
    let inner = Scripted::new(vec![Err(http(529)), Err(http(429)), Ok("{}".into())]);
    let client = fast(inner.clone());
    assert_eq!(client.complete(&req()).await.unwrap(), "{}");
    assert_eq!(inner.calls(), 3);
    assert_eq!(client.model(), "scripted/test");
}

#[tokio::test]
async fn retries_are_bounded_and_the_last_error_is_returned() {
    let inner = Scripted::new(vec![Err(http(503)), Err(http(503)), Err(http(500))]);
    let err = fast(inner.clone()).complete(&req()).await.unwrap_err();
    assert!(matches!(err, LlmError::Http { status: 500, .. }));
    assert_eq!(inner.calls(), 3);
}

#[tokio::test]
async fn a_definite_error_is_returned_without_retrying() {
    let inner = Scripted::new(vec![Err(http(401))]);
    let err = fast(inner.clone()).complete(&req()).await.unwrap_err();
    assert!(matches!(err, LlmError::Http { status: 401, .. }));
    assert_eq!(inner.calls(), 1);
}
