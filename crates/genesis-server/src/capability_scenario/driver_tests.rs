//! Canned organization responses test driver HTTP behavior, not backend proofs
//! or provider execution. Full acquisition evidence belongs to the integration tests.
use super::*;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::{Json, Router};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const ACQUISITION: &str = "acq-observer-test";
const JOB: &str = "job-observer-test";
const ADOPTION: &str = "adopt_0123456789abcdef0123456789abcdef";
const PRIVATE: &str = "PRIVATE_PROVIDER_BODY_MUST_NOT_REACH_ERROR";

#[derive(Clone)]
struct HttpState {
    replies: BTreeMap<(String, String), Value>,
    seen: Arc<Mutex<Vec<(String, String, Value)>>>,
}

struct Server {
    url: String,
    state: HttpState,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start(replies: Vec<(&str, String, Value)>) -> Self {
        async fn reply(
            State(state): State<HttpState>,
            request: Request,
        ) -> (axum::http::StatusCode, Json<Value>) {
            let method = request.method().to_string();
            let path = request.uri().path().to_owned();
            assert_eq!(
                request.headers()["authorization"],
                "Bearer fixture-operator"
            );
            let bytes = to_bytes(request.into_body(), 128 * 1024).await.unwrap();
            let body = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            state
                .seen
                .lock()
                .unwrap()
                .push((method.clone(), path.clone(), body));
            match state.replies.get(&(method, path)) {
                Some(value) => (axum::http::StatusCode::OK, Json(value.clone())),
                None => (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({"error":PRIVATE})),
                ),
            }
        }
        let state = HttpState {
            replies: replies
                .into_iter()
                .map(|(method, path, body)| ((method.into(), path), body))
                .collect(),
            seen: Arc::default(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(reply).with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, state, task }
    }

    fn requests(&self) -> Vec<(String, String, Value)> {
        self.state.seen.lock().unwrap().clone()
    }

    fn assert_only_adoption_requests(&self, spec: &ScenarioSpec) {
        let seen = self.requests();
        let creates: Vec<_> = seen
            .iter()
            .filter(|(method, _, _)| method == "POST")
            .collect();
        assert_eq!(
            creates.len(),
            1,
            "observation must not recreate or fall back"
        );
        assert_eq!(creates[0].1, "/api/v1/capability-adoptions");
        let request = creates[0].2.as_object().unwrap();
        let fields: BTreeSet<_> = request.keys().map(String::as_str).collect();
        let required = BTreeSet::from([
            "client_request_id",
            "mission_id",
            "requirement_id",
            "role_id",
            "responsibility_id",
            "suite_id",
            "provider_access_acknowledged",
        ]);
        assert!(required.is_subset(&fields));
        assert!(fields.is_subset(
            &required
                .union(&BTreeSet::from(["deadline"]))
                .copied()
                .collect()
        ));
        assert_eq!(request["mission_id"], "mission");
        assert_eq!(request["requirement_id"], "requirement");
        assert_eq!(request["role_id"], "role");
        assert_eq!(request["responsibility_id"], "responsibility");
        assert_eq!(request["suite_id"], spec.suite_b_id);
        assert_eq!(request["provider_access_acknowledged"], true);
        assert!(request["client_request_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
        assert!(
            seen.iter().all(|(method, path, _)| {
                (method == "POST" && path == "/api/v1/capability-adoptions")
                    || (method == "GET"
                        && path == &format!("/api/v1/capability-adoptions/{ADOPTION}"))
            }),
            "unexpected request: {seen:?}"
        );
        self.assert_no_manual_effects();
    }

    fn assert_no_manual_effects(&self) {
        assert!(self
            .requests()
            .iter()
            .all(|(_, path, _)| !path.ends_with("/step") && !path.ends_with("/verify")));
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn driver(a: &Server, b: &Server) -> ScenarioDriver {
    ScenarioDriver::new(
        ScenarioEndpoint::new(&a.url, "fixture-operator").unwrap(),
        ScenarioEndpoint::new(&b.url, "fixture-operator").unwrap(),
        ScenarioLimits {
            request_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(1),
            max_polls: 2,
            total_timeout: Duration::from_secs(5),
            ..ScenarioLimits::default()
        },
    )
    .unwrap()
}

fn legacy_spec() -> ScenarioSpec {
    let mut spec: Value =
        serde_json::from_slice(include_bytes!("../../examples/scenario/mission.json")).unwrap();
    spec.as_object_mut().unwrap().remove("progression");
    serde_json::from_value(spec).unwrap()
}

fn work() -> Work {
    Work {
        mission: "mission".into(),
        observation: "observation".into(),
        requirement: "requirement".into(),
        role: "role".into(),
        responsibility: "responsibility".into(),
    }
}

fn acquisition(phase: &str, version: u64) -> Value {
    json!({"id":ACQUISITION,"org_id":"org-a","mission_id":"mission","requirement_id":"requirement",
        "role_id":"role","responsibility_id":"responsibility","currently_authorized":true,
        "phase":phase,"version":version,"attempt":0})
}

fn automation(state: &str, code: Option<&str>) -> Value {
    json!({"id":ACQUISITION,"enabled":true,"current_version":1,"observed_version":1,
        "state":state,"error_code":code,"message":PRIVATE})
}

#[tokio::test]
async fn legacy_manual_acquisition_still_requests_one_step() {
    let a = Server::start(vec![
        (
            "POST",
            "/api/v1/capability-acquisitions".into(),
            acquisition("generating", 1),
        ),
        (
            "POST",
            format!("/api/v1/capability-acquisitions/{ACQUISITION}/step"),
            acquisition("verified", 2),
        ),
    ])
    .await;
    let b = Server::start(vec![]).await;
    let spec = legacy_spec();
    assert_eq!(spec.progression, ScenarioProgression::Manual);
    let result = driver(&a, &b)
        .acquire(&spec, &work(), "org-a")
        .await
        .unwrap();
    assert_eq!(result["phase"], "verified");
    let seen = a.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, "POST");
    assert_eq!(seen[0].1, "/api/v1/capability-acquisitions");
    assert_eq!(
        seen[1].1,
        format!("/api/v1/capability-acquisitions/{ACQUISITION}/step")
    );
    assert_eq!(seen[1].2, json!({"retry_read":false}));
    assert!(b.requests().is_empty());
}

#[tokio::test]
async fn automatic_retry_or_quota_attention_stops_without_manual_effects() {
    for code in ["retry_read", "quota"] {
        let a = Server::start(vec![
            (
                "POST",
                "/api/v1/capability-acquisitions".into(),
                acquisition("forging", 1),
            ),
            (
                "GET",
                format!("/api/v1/capability-acquisitions/{ACQUISITION}/automation"),
                automation("attention", Some(code)),
            ),
        ])
        .await;
        let b = Server::start(vec![]).await;
        let mut spec = legacy_spec();
        spec.progression = ScenarioProgression::Automatic;
        let error = driver(&a, &b)
            .acquire(&spec, &work(), "org-a")
            .await
            .unwrap_err();
        assert!(
            matches!(&error, ScenarioError::Observation { kind:"acquisition", id, .. } if id == ACQUISITION)
        );
        assert!(!error.to_string().contains(PRIVATE));
        assert_eq!(a.requests().len(), 2);
        a.assert_no_manual_effects();
        assert!(b.requests().is_empty());
    }
}

#[tokio::test]
async fn automatic_polling_limits_preserve_ids_without_fallback_or_repeated_create() {
    let a = Server::start(vec![
        (
            "POST",
            "/api/v1/capability-acquisitions".into(),
            acquisition("generating", 1),
        ),
        (
            "GET",
            format!("/api/v1/capability-acquisitions/{ACQUISITION}"),
            acquisition("generating", 1),
        ),
        (
            "GET",
            format!("/api/v1/capability-acquisitions/{ACQUISITION}/automation"),
            automation("waiting", None),
        ),
    ])
    .await;
    let pin = ArtifactPin {
        capability_id: "venue".into(),
        capability_version: "1".into(),
        source_sha256: "a".repeat(64),
        actor_id: "actor".into(),
        build_id: "build".into(),
        build_number: "0.1.0".into(),
    };
    let b = Server::start(vec![("GET", format!("/api/v1/capability-runs/{JOB}"),
        json!({"job_id":JOB,"request":{"artifact":pin},"purpose":{"kind":"invocation"},"phase":"prepared","failure":null}))]).await;
    let driver = driver(&a, &b);
    let mut spec = legacy_spec();
    spec.progression = ScenarioProgression::Automatic;
    let error = driver.acquire(&spec, &work(), "org-a").await.unwrap_err();
    assert!(
        matches!(error, ScenarioError::Observation { kind:"acquisition", id, reason:"observation polling limit reached" } if id == ACQUISITION)
    );
    let error = driver
        .collect(JOB, &pin, None, ScenarioProgression::Automatic)
        .await
        .err()
        .expect("collection must reach its bound");
    assert!(
        matches!(error, ScenarioError::Observation { kind:"capability run", id, reason:"observation polling limit reached" } if id == JOB)
    );
    let seen = a.requests();
    assert_eq!(
        seen.iter()
            .filter(|(method, _, _)| method == "POST")
            .count(),
        1
    );
    assert_eq!(
        seen.iter()
            .filter(|(_, path, _)| path.ends_with("/automation"))
            .count(),
        2
    );
    assert_eq!(b.requests().len(), 2);
    a.assert_no_manual_effects();
    b.assert_no_manual_effects();
}

#[tokio::test]
async fn completed_finalization_cannot_replace_an_absent_or_malformed_proof() {
    for malformed in [false, true] {
        let a = Server::start(vec![]).await;
        let mut replies = vec![(
            "GET",
            format!("/api/v1/capability-runs/{JOB}/finalization"),
            json!({"job_id":JOB,"state":"completed"}),
        )];
        if malformed {
            replies.push((
                "GET",
                format!("/api/v1/capability-runs/{JOB}/verification"),
                json!({"decision":"verified","proof_ref":"untrusted-claim","raw_output":PRIVATE}),
            ));
        }
        let b = Server::start(replies).await;
        let error = driver(&a, &b)
            .observe_invocation(JOB)
            .await
            .err()
            .expect("scheduling completion is not a proof");
        assert!(matches!(
            error,
            ScenarioError::Http { status: 404, .. } | ScenarioError::Response("invocation proof")
        ));
        assert!(!error.to_string().contains(PRIVATE));
        assert_eq!(b.requests().len(), 2);
        assert!(b.requests().iter().all(|(method, _, _)| method == "GET"));
        b.assert_no_manual_effects();
        assert!(a.requests().is_empty());
    }
}

fn adoption_pin() -> ArtifactPin {
    ArtifactPin {
        capability_id: "venue".into(),
        capability_version: "1".into(),
        source_sha256: "a".repeat(64),
        actor_id: "actor".into(),
        build_id: "build".into(),
        build_number: "0.1.0".into(),
    }
}

fn adoption_status(spec: &ScenarioSpec, phase: &str, version: u64) -> Value {
    let mut status = json!({
        "id":ADOPTION,"version":version,"org_id":"org-b","mission_id":"mission",
        "requirement_id":"requirement","role_id":"role","responsibility_id":"responsibility",
        "assignment_id":"assignment","epoch":1,"suite_id":spec.suite_b_id,"phase":phase,
        "created_at":"2026-10-22T00:00:00Z","updated_at":"2026-10-22T00:00:01Z",
        "deadline":"2026-10-22T00:30:00Z","next_attempt_at":null,"discovery_attempts":1,
        "selection":null,"artifact":null,"installation_id":null,"job_ids":[],
        "acceptance_ref":null,"rejection_ref":null,"attention":null,"fence_reason":null,
        "authority":"observational: local acceptance does not attach capabilities or satisfy a mission"
    });
    match phase {
        "discovering" => status["next_attempt_at"] = json!("2026-10-22T00:00:02Z"),
        "attention" => status["attention"] = json!("provider_unavailable"),
        "accepted" => {
            status["selection"] =
                json!({"capability_id":"venue","version":"1","publisher":"org-a"});
            status["artifact"] = json!(adoption_pin());
            status["installation_id"] = json!("installation");
            status["job_ids"] = json!(["check-a", "check-b", "check-c"]);
            status["acceptance_ref"] = json!(format!("verification:{}", "c".repeat(64)));
        }
        _ => panic!("unsupported fixture phase"),
    }
    status
}

#[tokio::test]
async fn automatic_adoption_attention_is_terminal_and_redacted() {
    let mut spec = legacy_spec();
    spec.progression = ScenarioProgression::Automatic;
    spec.provider_access_acknowledged = true;
    for code in ["provider_unavailable", "registry_unavailable", PRIVATE] {
        let a = Server::start(vec![]).await;
        let mut stopped = adoption_status(&spec, "attention", 2);
        stopped["attention"] = json!(code);
        let b = Server::start(vec![
            (
                "POST",
                "/api/v1/capability-adoptions".into(),
                adoption_status(&spec, "discovering", 1),
            ),
            (
                "GET",
                format!("/api/v1/capability-adoptions/{ADOPTION}"),
                stopped,
            ),
        ])
        .await;
        let error = driver(&a, &b)
            .adopt_automatic(&spec, &work(), "org-b", "org-a", &adoption_pin())
            .await
            .err()
            .expect("attention must stop without a replacement intent");
        assert!(
            matches!(&error, ScenarioError::Observation { kind:"adoption", id, .. } if id == ADOPTION),
            "{error}"
        );
        assert!(!error.to_string().contains(PRIVATE));
        assert_eq!(b.requests().len(), 2);
        b.assert_only_adoption_requests(&spec);
        assert!(a.requests().is_empty());
    }
}

#[tokio::test]
async fn automatic_adoption_failed_read_keeps_the_id_without_recreating_work() {
    let mut spec = legacy_spec();
    spec.progression = ScenarioProgression::Automatic;
    spec.provider_access_acknowledged = true;
    let a = Server::start(vec![]).await;
    let b = Server::start(vec![(
        "POST",
        "/api/v1/capability-adoptions".into(),
        adoption_status(&spec, "discovering", 1),
    )])
    .await;
    let error = driver(&a, &b)
        .adopt_automatic(&spec, &work(), "org-b", "org-a", &adoption_pin())
        .await
        .err()
        .expect("a failed observation must retain the committed intent identity");
    assert!(matches!(&error, ScenarioError::Observation {
        kind: "adoption", id, reason: "adoption status could not be observed"
    } if id == ADOPTION));
    assert!(!error.to_string().contains(PRIVATE));
    assert_eq!(b.requests().len(), 2);
    b.assert_only_adoption_requests(&spec);
    assert!(a.requests().is_empty());
}

#[tokio::test]
async fn automatic_adoption_polling_limit_keeps_the_intent_without_manual_fallback() {
    let mut spec = legacy_spec();
    spec.progression = ScenarioProgression::Automatic;
    spec.provider_access_acknowledged = true;
    let a = Server::start(vec![]).await;
    let waiting = adoption_status(&spec, "discovering", 1);
    let b = Server::start(vec![
        (
            "POST",
            "/api/v1/capability-adoptions".into(),
            waiting.clone(),
        ),
        (
            "GET",
            format!("/api/v1/capability-adoptions/{ADOPTION}"),
            waiting,
        ),
    ])
    .await;
    let error = driver(&a, &b)
        .adopt_automatic(&spec, &work(), "org-b", "org-a", &adoption_pin())
        .await
        .err()
        .expect("waiting must hit the observation bound");
    assert!(
        matches!(&error, ScenarioError::Observation {
        kind:"adoption", id, reason:"observation polling limit reached"
    } if id == ADOPTION),
        "{error}"
    );
    assert_eq!(b.requests().len(), 3);
    b.assert_only_adoption_requests(&spec);
    assert!(a.requests().is_empty());
}

#[tokio::test]
async fn automatic_adoption_accepted_label_cannot_hide_a_binding_mismatch() {
    let mut spec = legacy_spec();
    spec.progression = ScenarioProgression::Automatic;
    spec.provider_access_acknowledged = true;
    for mismatched in ["suite", "publisher", "artifact", "reference"] {
        let a = Server::start(vec![]).await;
        let mut accepted = adoption_status(&spec, "accepted", 2);
        match mismatched {
            "suite" => accepted["suite_id"] = json!("wrong-local-suite"),
            "publisher" => accepted["selection"]["publisher"] = json!("unrelated-publisher"),
            "artifact" => accepted["artifact"]["source_sha256"] = json!("b".repeat(64)),
            "reference" => accepted["acceptance_ref"] = json!(PRIVATE),
            _ => unreachable!(),
        }
        let b = Server::start(vec![
            (
                "POST",
                "/api/v1/capability-adoptions".into(),
                adoption_status(&spec, "discovering", 1),
            ),
            (
                "GET",
                format!("/api/v1/capability-adoptions/{ADOPTION}"),
                accepted,
            ),
        ])
        .await;
        let error = driver(&a, &b)
            .adopt_automatic(&spec, &work(), "org-b", "org-a", &adoption_pin())
            .await
            .err()
            .expect("accepted metadata cannot replace exact local proof bindings");
        assert!(
            matches!(&error, ScenarioError::Observation { kind:"adoption", id, .. } if id == ADOPTION),
            "{mismatched}: {error}"
        );
        assert!(!error.to_string().contains(PRIVATE));
        assert_eq!(b.requests().len(), 2);
        b.assert_only_adoption_requests(&spec);
        assert!(a.requests().is_empty());
    }
}
