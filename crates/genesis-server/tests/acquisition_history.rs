//! Acquisition history is observation-only. Fixtures use trusted local mission
//! setup and a canned loopback model; no generated code or cloud Actor executes.
use axum::{extract::State, routing::post, Json, Router};
use chrono::Utc;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::{ObservationId, OrgId, RequirementId};
use genesis_core::lifecycle::{RequirementStatus, RoleState};
use genesis_core::model::{Observation, Owner, Requirement};
use genesis_forge::{AnthropicGenerator, ModelConfig};
use genesis_server::app::AppState;
use genesis_server::capability_acquisition_driver::drain_once;
use genesis_server::capability_acquisition_store::{
    AcquisitionDebit, AcquisitionPolicy, AcquisitionQuotas, AcquisitionRecord,
    AcquisitionStoreError, CreateAcquisition,
};
use genesis_server::error::validation_error;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, AcquisitionConfig, OrgConfig};
use genesis_verification::{decode_json, OracleSuite};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::net::TcpListener;

const BASE: &str = "/capability-acquisitions";
const MODEL: &str = "claude-sonnet-4-6";
const MODEL_KEY: &str = "HISTORY_MODEL_KEY_CANARY";
const NATIVE_ID: &str = "msg_HISTORY_NATIVE_ID_CANARY";
const SOURCE: &str = "HISTORY_SOURCE_CANARY";
const EXAMPLE: &str = "HISTORY_EXAMPLE_CANARY";
const ORACLE: &str = "HISTORY_PRIVATE_ORACLE_CANARY";
const REQUIREMENT: &str = "HISTORY_PRIVATE_REQUIREMENT_CANARY";
const ACTOR_NAME: &str = "history-actor-name-canary";

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, task }
    }
    async fn stop(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn suite() -> OracleSuite {
    let mut suite: OracleSuite = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap();
    suite.scope = ORACLE.into();
    suite
}
fn policy() -> AcquisitionPolicy {
    AcquisitionPolicy {
        model: ModelConfig {
            model: MODEL.into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 3600,
    }
}
async fn model_reply(
    State(calls): State<Arc<AtomicUsize>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(body["model"], MODEL);
    calls.fetch_add(1, Ordering::SeqCst);
    Json(
        json!({"id":NATIVE_ID,"type":"message","role":"assistant","model":MODEL,
        "content":[{"type":"text","text":json!({"extractor_typescript":format!("// {SOURCE}\nexport function extract() {{ throw new Error('canned source, never executed'); }}\n")}).to_string()}],
        "stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":80,"output_tokens":50}}),
    )
}

struct Fixture {
    _directory: tempfile::TempDir,
    _model: Server,
    calls: Arc<AtomicUsize>,
    server: Server,
    app: AppState,
    config: OrgConfig,
    http: reqwest::Client,
}
impl Fixture {
    async fn new(enabled: bool) -> Self {
        Self::with_policy(enabled, policy()).await
    }
    async fn with_policy(enabled: bool, policy: AcquisitionPolicy) -> Self {
        Self::with_org_id(enabled, policy, "history-org").await
    }
    async fn with_org_id(enabled: bool, policy: AcquisitionPolicy, org_id: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let model = Server::start(
            Router::new()
                .route("/v1/messages", post(model_reply))
                .with_state(calls.clone()),
        )
        .await;
        let mut config = OrgConfig::dev("history-org", "History fixture");
        // Operator credentials are independent of the organization's display ID.
        config.org_id = OrgId::new(org_id);
        config.db = Some(directory.path().join("org.sqlite"));
        config.oracle_suites = vec![suite()];
        if enabled {
            let mut acquisition = AcquisitionConfig::offline(policy);
            acquisition.model = Some(Arc::new(
                AnthropicGenerator::with_test_endpoint(
                    &format!("{}/v1/messages", model.url),
                    MODEL_KEY,
                )
                .unwrap(),
            ));
            config.acquisition = Some(acquisition);
        }
        let app = build_org(&config, None).unwrap();
        // History tests exercise explicit transitions; no unrelated workers run.
        let server = Server::start(app.clone().router()).await;
        Self {
            _directory: directory,
            _model: model,
            calls,
            server,
            app,
            config,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }
    async fn reopen(&mut self, config: OrgConfig) {
        self.server.stop().await;
        let app = build_org(&config, None).unwrap();
        self.server = Server::start(app.clone().router()).await;
        self.app = app;
        self.config = config;
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.config.db.as_ref().unwrap()).unwrap()
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        principal: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .http
            .request(method, format!("{}/api/v1{path}", self.server.url));
        if let Some(principal) = principal {
            request = request.bearer_auth(format!("history-org-{principal}"));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
    async fn get(&self, path: &str) -> Value {
        let (status, body) = self
            .request(Method::GET, path, Some("operator"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        redact(&body);
        body
    }
    async fn create(&self, intent: &CreateAcquisition) -> AcquisitionRecord {
        let (status, body) = self
            .request(
                Method::POST,
                BASE,
                Some("operator"),
                Some(serde_json::to_value(intent).unwrap()),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        self.app
            .acquisition_runtime
            .status(body["id"].as_str().unwrap())
            .unwrap()
    }
    fn begin(&self, record: &AcquisitionRecord) -> AcquisitionRecord {
        let oracle = suite();
        let mut session = record.session().clone();
        session.begin_generation(&oracle).unwrap();
        record.with_session(session).unwrap()
    }
    fn save(
        &self,
        record: &AcquisitionRecord,
        debit: Option<AcquisitionDebit>,
    ) -> Result<AcquisitionRecord, AcquisitionStoreError> {
        self.app.store.save_acquisition(
            record.version(),
            self.app.current_state().unwrap().revision,
            record,
            &suite(),
            debit,
            Utc::now(),
        )
    }
    async fn history(&self, id: &str) -> Value {
        self.get(&format!("{BASE}/{id}/history?limit=100")).await
    }
}

fn redact(value: &Value) {
    let text = value.to_string();
    for canary in [
        MODEL,
        MODEL_KEY,
        NATIVE_ID,
        SOURCE,
        EXAMPLE,
        ORACLE,
        REQUIREMENT,
        ACTOR_NAME,
    ] {
        assert!(
            !text.contains(canary),
            "history exposed private field {canary}"
        );
    }
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    ![
                        "extractor_typescript",
                        "examples",
                        "artifact",
                        "actor_id",
                        "build_id",
                        "run_id",
                        "dataset_id",
                        "message_id",
                        "checkpoint_sha256",
                        "record_sha256",
                        "model"
                    ]
                    .contains(&key.as_str()),
                    "private history key {key}"
                );
                redact(value);
            }
        }
        Value::Array(items) => items.iter().for_each(redact),
        _ => {}
    }
}
fn items(page: &Value) -> &[Value] {
    page["items"].as_array().unwrap()
}
fn events(page: &Value) -> Vec<&str> {
    items(page)
        .iter()
        .map(|item| item["kind"].as_str().unwrap())
        .collect()
}

async fn seed(app: &AppState, suffix: &str) -> CreateAcquisition {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Acquire an independently checked parser",
        "roles":[{"key":"owner","name":"Owner","charter":"Own acquisition",
            "authority":{"scopes":["capability.acquire"],"budget_cents":100}}],
        "responsibilities":[{"purpose":"Acquire parser","owner":"owner"}]
    }))
    .unwrap();
    let mission = app
        .mutate(EventMeta::default(), |state| {
            bootstrap_mission(state, &spec, Utc::now()).map_err(validation_error)
        })
        .await
        .unwrap()
        .value;
    let mut state = app.current_state().unwrap();
    let revision = state.revision;
    let responsibility = state
        .responsibilities
        .values()
        .find(|value| value.mission_id == mission)
        .unwrap()
        .clone();
    let role = responsibility.owner.role().unwrap().clone();
    let requirement = RequirementId::new(format!("history-requirement-{suffix}"));
    let observation = ObservationId::new(format!("history-observation-{suffix}"));
    state.observations.insert(
        observation.clone(),
        Observation {
            observation_id: observation.clone(),
            mission_id: mission.clone(),
            source: "fixture".into(),
            summary: "A new website requires a parser".into(),
            evidence_ref: None,
            observed_at: Utc::now(),
        },
    );
    state.requirements.insert(
        requirement.clone(),
        Requirement {
            requirement_id: requirement.clone(),
            mission_id: mission.clone(),
            summary: REQUIREMENT.into(),
            triggering_evidence: vec![observation],
            acceptance: ORACLE.into(),
            status: RequirementStatus::Assigned,
            owner: Owner::Role(role.clone()),
        },
    );
    state
        .responsibilities
        .get_mut(&responsibility.responsibility_id)
        .unwrap()
        .requirement_id = Some(requirement.clone());
    state.revision += 1;
    app.store
        .commit_state(
            revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    let oracle = suite();
    let urls: Vec<_> = oracle
        .cases
        .iter()
        .map(|case| case.source.url.clone())
        .collect();
    serde_json::from_value(json!({"client_request_id":format!("history-key-{suffix}"),
        "mission_id":mission,"requirement_id":requirement,"role_id":role,
        "responsibility_id":responsibility.responsibility_id,"suite_id":oracle.suite_id,
        "candidate":{"capability_id":"history-venue-reader","capability_version":"1.0.0",
            "actor_name":ACTOR_NAME,"source_version":"0.1"},"repair":null,
        "allowed_source_urls":urls,"examples":[{"url":urls[0],"html":format!("<html>{EXAMPLE}</html>")}],
        "deadline":null})).unwrap()
}

#[tokio::test]
async fn history_http_requires_operator_and_rejects_ambiguous_or_unbounded_queries() {
    let fixture = Fixture::new(true).await;
    let intent = seed(&fixture.app, "acl").await;
    let record = fixture.create(&intent).await;
    let history = format!("{BASE}/{}/history", record.id());
    for path in [BASE, &history] {
        for principal in [None, Some("unknown"), Some("agent"), Some("adapter")] {
            let (status, body) = fixture.request(Method::GET, path, principal, None).await;
            assert_eq!(
                status,
                if matches!(principal, None | Some("unknown")) {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::FORBIDDEN
                },
                "{body}"
            );
            redact(&body);
        }
        for query in [
            "limit=0",
            "limit=101",
            "limit=-1",
            "limit=1.5",
            "limit=18446744073709551616",
            "after=-1",
            "after=1.5",
            "after=9223372036854775808",
            "after=18446744073709551616",
            "after=",
            "after=0&after=1",
            "limit=1&limit=2",
            "unknown=PRIVATE_QUERY_CANARY",
        ] {
            let (status, body) = fixture
                .request(
                    Method::GET,
                    &format!("{path}?{query}"),
                    Some("operator"),
                    None,
                )
                .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "query {query}: {body}");
            assert_eq!(body["error"], "invalid_history_query");
            assert!(!body.to_string().contains("PRIVATE_QUERY_CANARY"));
        }
        let page = fixture
            .get(&format!("{path}?after=9223372036854775807&limit=100"))
            .await;
        assert!(items(&page).is_empty());
        assert!(page["next_after"].is_null());
    }
    for query in [
        "mission_id=",
        "requirement_id=",
        "mission_id=a&mission_id=b",
        "requirement_id=a&requirement_id=b",
    ] {
        let (status, body) = fixture
            .request(
                Method::GET,
                &format!("{BASE}?{query}"),
                Some("operator"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }
    // Existing domain IDs are opaque UTF-8 values, not ASCII slugs. Their
    // exact bounded filter values are valid even when no row matches them.
    for query in ["mission_id=%20bad", "requirement_id=bad%0Avalue"] {
        let page = fixture.get(&format!("{BASE}?{query}")).await;
        assert!(items(&page).is_empty());
    }
    for query in ["mission_id=one", "requirement_id=one"] {
        let (status, _) = fixture
            .request(
                Method::GET,
                &format!("{history}?{query}"),
                Some("operator"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = fixture
        .request(
            Method::GET,
            &format!("{BASE}?mission_id={}", "a".repeat(257)),
            Some("operator"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        1
    );
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn catalog_cursor_is_stable_across_real_generation_and_history_is_redacted() {
    let fixture = Fixture::new(true).await;
    let first_intent = seed(&fixture.app, "first").await;
    let first = fixture.create(&first_intent).await;
    let second_intent = seed(&fixture.app, "second").await;
    let second = fixture.create(&second_intent).await;
    let first_page = fixture.get(&format!("{BASE}?limit=1")).await;
    assert_eq!(items(&first_page).len(), 1);
    assert_eq!(items(&first_page)[0]["snapshot"]["id"], first.id());
    let cursor = first_page["next_after"].as_u64().unwrap();
    assert_eq!(items(&first_page)[0]["ordinal"], cursor);
    let original = fixture.history(first.id()).await;
    assert_eq!(events(&original), vec!["created"]);

    // The actual HTTP model adapter supplies a source-bearing native receipt;
    // the journal must expose neither the receipt nor the generated package.
    let generated = fixture
        .app
        .acquisition_runtime
        .step(first.id(), false)
        .await
        .unwrap();
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    let private = serde_json::to_string(&generated).unwrap();
    assert!(private.contains(SOURCE));
    assert!(private.contains(NATIVE_ID));
    let third_intent = seed(&fixture.app, "third").await;
    let third = fixture.create(&third_intent).await;
    let remaining = fixture
        .get(&format!("{BASE}?after={cursor}&limit=100"))
        .await;
    let ids: Vec<_> = items(&remaining)
        .iter()
        .map(|item| item["snapshot"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![second.id(), third.id()]);
    assert!(remaining["next_after"].is_null());
    let fresh_first = fixture.get(&format!("{BASE}?limit=1")).await;
    assert_eq!(items(&fresh_first)[0]["ordinal"], cursor);
    assert_eq!(
        items(&fresh_first)[0]["snapshot"]["version"],
        generated.version()
    );
    assert_eq!(
        fixture.history(first.id()).await["items"][0],
        original["items"][0],
        "past entries are immutable"
    );

    let history = fixture.history(first.id()).await;
    assert_eq!(
        events(&history),
        vec!["created", "transition", "transition"]
    );
    let page = fixture
        .get(&format!("{BASE}/{}/history?limit=1", first.id()))
        .await;
    let after = page["next_after"].as_u64().unwrap();
    let tail = fixture
        .get(&format!(
            "{BASE}/{}/history?after={after}&limit=100",
            first.id()
        ))
        .await;
    assert_eq!(items(&tail), &items(&history)[1..]);
    assert!(items(&history)
        .windows(2)
        .all(|pair| pair[0]["seq"].as_u64() < pair[1]["seq"].as_u64()));
    for filters in [
        format!("mission_id={}", second_intent.mission_id),
        format!("requirement_id={}", second_intent.requirement_id),
        format!(
            "mission_id={}&requirement_id={}",
            second_intent.mission_id, second_intent.requirement_id
        ),
    ] {
        let page = fixture.get(&format!("{BASE}?{filters}")).await;
        assert_eq!(items(&page).len(), 1);
        assert_eq!(items(&page)[0]["snapshot"]["id"], second.id());
    }
    let no_match = fixture
        .get(&format!(
            "{BASE}?mission_id={}&requirement_id={}",
            first_intent.mission_id, second_intent.requirement_id
        ))
        .await;
    assert!(items(&no_match).is_empty());
    let (status, _) = fixture
        .request(
            Method::POST,
            BASE,
            Some("operator"),
            Some(serde_json::to_value(&first_intent).unwrap()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fixture.history(first.id()).await,
        history,
        "idempotent creation adds no journal entry"
    );
}

#[tokio::test]
async fn initialized_history_survives_removing_acquisition_clients_and_oracles() {
    let mut fixture = Fixture::new(true).await;
    let intent = seed(&fixture.app, "retained").await;
    let record = fixture.create(&intent).await;
    fixture
        .app
        .acquisition_runtime
        .step(record.id(), false)
        .await
        .unwrap();
    let catalog = fixture.get(BASE).await;
    let history = fixture.history(record.id()).await;
    let mut disabled = fixture.config.clone();
    disabled.acquisition = None;
    disabled.oracle_suites.clear();
    fixture.reopen(disabled).await;
    assert_eq!(fixture.get(BASE).await, catalog);
    assert_eq!(fixture.history(record.id()).await, history);
    let (status, _) = fixture
        .request(
            Method::POST,
            &format!("{BASE}/{}/step", record.id()),
            Some("operator"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);

    let other = Fixture::new(false).await;
    assert_eq!(other.get(BASE).await, json!({"items":[],"next_after":null}));
    let (status, body) = other
        .request(
            Method::GET,
            &format!("{BASE}/{}/history", record.id()),
            Some("operator"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(other.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn organization_ids_with_spaces_and_unicode_survive_journaling_restart_and_bootstrap() {
    const ORG: &str = "Org A 東京";
    let mut fixture = Fixture::with_org_id(true, policy(), ORG).await;
    let intent = seed(&fixture.app, "unicode-org").await;
    let record = fixture.create(&intent).await;
    let catalog = fixture.get(BASE).await;
    let history = fixture.history(record.id()).await;
    assert_eq!(items(&catalog)[0]["snapshot"]["org_id"], ORG);
    assert_eq!(items(&history)[0]["snapshot"]["org_id"], ORG);
    assert_eq!(events(&history), vec!["created"]);
    fixture.reopen(fixture.config.clone()).await;
    assert_eq!(fixture.get(BASE).await, catalog);
    assert_eq!(fixture.history(record.id()).await, history);
    let quota = fixture.app.store.acquisition_counters().unwrap();
    fixture
        .sql()
        .execute_batch(
            "DROP TABLE acquisition_history;
        DROP TABLE acquisition_catalog; DROP TABLE acquisition_history_meta;",
        )
        .unwrap();
    fixture.reopen(fixture.config.clone()).await;
    let migrated = fixture.history(record.id()).await;
    assert_eq!(events(&migrated), vec!["baseline_observed"]);
    assert_eq!(items(&migrated)[0]["snapshot"]["org_id"], ORG);
    assert_eq!(
        items(&fixture.get(BASE).await)[0]["snapshot"]["org_id"],
        ORG
    );
    fixture.reopen(fixture.config.clone()).await;
    assert_eq!(fixture.history(record.id()).await, migrated);
    assert_eq!(fixture.app.store.acquisition_counters().unwrap(), quota);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn journal_failure_rolls_back_actual_checkpoint_and_quota_and_cas_adds_no_ghost_entry() {
    let mut policy = policy();
    policy.quotas.model_calls = 1;
    let fixture = Fixture::with_policy(true, policy).await;
    let intent = seed(&fixture.app, "atomic").await;
    let sql = fixture.sql();
    sql.execute_batch(
        "CREATE TRIGGER fail_created_history BEFORE INSERT ON acquisition_history
        WHEN NEW.event_kind='created' BEGIN SELECT RAISE(ABORT,'PRIVATE_SQL_FAILURE_CANARY'); END;",
    )
    .unwrap();
    let (status, body) = fixture
        .request(
            Method::POST,
            BASE,
            Some("operator"),
            Some(serde_json::to_value(&intent).unwrap()),
        )
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(!body.to_string().contains("PRIVATE_SQL_FAILURE_CANARY"));
    assert!(fixture.app.store.acquisition_ids().unwrap().is_empty());
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        0
    );
    assert!(items(&fixture.get(BASE).await).is_empty());
    sql.execute_batch("DROP TRIGGER fail_created_history;")
        .unwrap();
    let record = fixture.create(&intent).await;
    let created = fixture.history(record.id()).await;
    let pending = fixture.begin(&record);
    sql.execute_batch("CREATE TRIGGER fail_transition_history BEFORE INSERT ON acquisition_history
        WHEN NEW.event_kind='transition' BEGIN SELECT RAISE(ABORT,'PRIVATE_TRANSITION_CANARY'); END;").unwrap();
    assert!(matches!(
        fixture.save(&pending, Some(AcquisitionDebit::ModelCall)),
        Err(AcquisitionStoreError::Db(_))
    ));
    assert_eq!(
        fixture
            .app
            .acquisition_runtime
            .status(record.id())
            .unwrap()
            .version(),
        1
    );
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .model_calls,
        0
    );
    assert_eq!(fixture.history(record.id()).await, created);
    sql.execute_batch("DROP TRIGGER fail_transition_history;")
        .unwrap();
    let saved = fixture
        .save(&pending, Some(AcquisitionDebit::ModelCall))
        .unwrap();
    let started = fixture.history(record.id()).await;
    assert_eq!(events(&started), vec!["created", "transition"]);
    assert_eq!(items(&started)[1]["debit"], "model_call");
    assert!(matches!(
        fixture.save(&pending, Some(AcquisitionDebit::ModelCall)),
        Err(AcquisitionStoreError::Conflict)
    ));
    assert!(matches!(
        fixture.app.store.save_acquisition(
            saved.version(),
            fixture.app.current_state().unwrap().revision - 1,
            &saved,
            &suite(),
            None,
            Utc::now()
        ),
        Err(AcquisitionStoreError::Conflict)
    ));
    assert_eq!(fixture.history(record.id()).await, started);
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .model_calls,
        1
    );

    let mut session = saved.session().clone();
    assert!(session.recover_interrupted(&suite()).unwrap());
    fixture
        .save(&saved.with_session(session).unwrap(), None)
        .unwrap();
    let mut another = intent.clone();
    another.client_request_id = "history-quota-exhausted".into();
    let second = fixture.create(&another).await;
    let before = fixture.history(second.id()).await;
    assert!(matches!(
        fixture.save(&fixture.begin(&second), Some(AcquisitionDebit::ModelCall)),
        Err(AcquisitionStoreError::QuotaExceeded("model_calls"))
    ));
    assert_eq!(
        fixture
            .app
            .acquisition_runtime
            .status(second.id())
            .unwrap()
            .version(),
        1
    );
    assert_eq!(fixture.history(second.id()).await, before);
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .model_calls,
        1
    );
    assert_eq!(
        fixture.calls.load(Ordering::SeqCst),
        0,
        "saved intents do not execute model calls"
    );
}

#[tokio::test]
async fn legacy_records_require_trusted_bootstrap_and_get_one_honest_current_baseline() {
    let mut fixture = Fixture::new(true).await;
    let intent = seed(&fixture.app, "legacy").await;
    let record = fixture.create(&intent).await;
    let started = fixture
        .save(&fixture.begin(&record), Some(AcquisitionDebit::ModelCall))
        .unwrap();
    let mut session = started.session().clone();
    assert!(session.recover_interrupted(&suite()).unwrap());
    let ended = fixture
        .save(&started.with_session(session).unwrap(), None)
        .unwrap();
    assert_eq!(
        events(&fixture.history(record.id()).await),
        vec!["created", "transition", "transition"]
    );
    let enabled_config = fixture.config.clone();
    // Only feature tables are removed: the actual prior checkpoint and quota
    // remain intact, as in a database written before history existed.
    fixture
        .sql()
        .execute_batch(
            "DROP TABLE acquisition_history;
        DROP TABLE acquisition_catalog; DROP TABLE acquisition_history_meta;",
        )
        .unwrap();
    let mut disabled = fixture.config.clone();
    disabled.acquisition = None;
    disabled.oracle_suites.clear();
    fixture.reopen(disabled.clone()).await;
    for path in [BASE.to_owned(), format!("{BASE}/{}/history", record.id())] {
        let (status, body) = fixture
            .request(Method::GET, &path, Some("operator"), None)
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["error"], "acquisition_history_unavailable");
    }
    let mut missing_suite = enabled_config.clone();
    missing_suite.oracle_suites.clear();
    assert!(
        build_org(&missing_suite, None).is_err(),
        "an unverified baseline cannot be invented"
    );
    fixture.reopen(enabled_config.clone()).await;
    let baseline = fixture.history(record.id()).await;
    assert_eq!(events(&baseline), vec!["baseline_observed"]);
    let entry = &items(&baseline)[0];
    assert_eq!(entry["snapshot"]["version"], ended.version());
    assert_eq!(entry["snapshot"]["phase"], "needs_reconciliation");
    assert_eq!(
        entry["checkpoint_org_revision"],
        fixture.app.current_state().unwrap().revision
    );
    assert!(entry["debit"].is_null());
    assert!(entry["attention"].is_null());
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .model_calls,
        1
    );
    fixture.reopen(enabled_config).await;
    assert_eq!(
        fixture.history(record.id()).await,
        baseline,
        "startup must not duplicate an observation"
    );
    fixture.reopen(disabled).await;
    assert_eq!(fixture.history(record.id()).await, baseline);
    assert_eq!(items(&fixture.get(BASE).await).len(), 1);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn bootstrap_resumes_after_later_invalid_record_without_duplicating_committed_baselines() {
    let mut fixture = Fixture::new(true).await;
    let first_intent = seed(&fixture.app, "partial-first").await;
    let first = fixture.create(&first_intent).await;
    let second_intent = seed(&fixture.app, "partial-second").await;
    let second = fixture.create(&second_intent).await;
    let mut ids = [first.id(), second.id()];
    ids.sort();
    let quota = fixture.app.store.acquisition_counters().unwrap();
    let sql = fixture.sql();
    let original: String = sql
        .query_row(
            "SELECT record_json FROM capability_acquisitions WHERE id=?1",
            [ids[1]],
            |row| row.get(0),
        )
        .unwrap();
    sql.execute_batch(
        "DROP TABLE acquisition_history;
        DROP TABLE acquisition_catalog; DROP TABLE acquisition_history_meta;",
    )
    .unwrap();
    // Bootstrap explicitly scans lexically: the first valid record commits a
    // baseline, then the later invalid checkpoint prevents global readiness.
    sql.execute(
        "UPDATE capability_acquisitions SET record_json='{}' WHERE id=?1",
        [ids[1]],
    )
    .unwrap();
    assert!(build_org(&fixture.config, None).is_err());
    assert_eq!(
        sql.query_row(
            "SELECT ready FROM acquisition_history_meta WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM acquisition_catalog", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let committed: String = sql
        .query_row(
            "SELECT payload_json FROM acquisition_history WHERE acquisition_id=?1",
            [ids[0]],
            |row| row.get(0),
        )
        .unwrap();
    let committed: Value = serde_json::from_str(&committed).unwrap();
    assert_eq!(committed["kind"], "baseline_observed");
    assert_eq!(committed["snapshot"]["id"], ids[0]);
    assert_eq!(committed["snapshot"]["version"], 1);
    for path in [BASE.to_owned(), format!("{BASE}/{}/history", ids[0])] {
        let (status, body) = fixture
            .request(Method::GET, &path, Some("operator"), None)
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["error"], "acquisition_history_unavailable");
    }
    assert_eq!(fixture.app.store.acquisition_counters().unwrap(), quota);
    sql.execute(
        "UPDATE capability_acquisitions SET record_json=?1 WHERE id=?2",
        rusqlite::params![original, ids[1]],
    )
    .unwrap();
    fixture.reopen(fixture.config.clone()).await;
    assert_eq!(
        sql.query_row(
            "SELECT ready FROM acquisition_history_meta WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let catalog = fixture.get(BASE).await;
    assert_eq!(items(&catalog).len(), 2);
    assert_eq!(items(&catalog)[0]["snapshot"]["id"], ids[0]);
    assert_eq!(items(&catalog)[1]["snapshot"]["id"], ids[1]);
    for id in ids {
        let page = fixture.history(id).await;
        assert_eq!(events(&page), vec!["baseline_observed"]);
        if id == committed["snapshot"]["id"] {
            assert_eq!(
                items(&page)[0],
                committed,
                "already committed baseline is immutable"
            );
        }
    }
    fixture.reopen(fixture.config.clone()).await;
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(fixture.app.store.acquisition_counters().unwrap(), quota);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn existing_catalog_version_drift_is_not_repaired_as_a_missing_baseline() {
    let mut fixture = Fixture::new(true).await;
    let intent = seed(&fixture.app, "drift").await;
    let record = fixture.create(&intent).await;
    let history = fixture.history(record.id()).await;
    let quota = fixture.app.store.acquisition_counters().unwrap();
    let sql = fixture.sql();
    let checkpoint: String = sql
        .query_row(
            "SELECT record_json FROM capability_acquisitions WHERE id=?1",
            [record.id()],
            |row| row.get(0),
        )
        .unwrap();
    // Damage only the persisted checkpoint index. A migration must neither
    // fabricate the missing transition nor rewrite a previously observed row.
    sql.execute(
        "UPDATE capability_acquisitions SET version=version+1 WHERE id=?1",
        [record.id()],
    )
    .unwrap();
    assert!(build_org(&fixture.config, None).is_err());
    assert_eq!(
        sql.query_row(
            "SELECT ready FROM acquisition_history_meta WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        sql.query_row("SELECT event_kind FROM acquisition_history", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "created"
    );
    assert_eq!(
        sql.query_row(
            "SELECT version FROM acquisition_catalog WHERE id=?1",
            [record.id()],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        sql.query_row(
            "SELECT record_json FROM capability_acquisitions WHERE id=?1",
            [record.id()],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        checkpoint
    );
    for path in [BASE.to_owned(), format!("{BASE}/{}/history", record.id())] {
        let (status, body) = fixture
            .request(Method::GET, &path, Some("operator"), None)
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    }
    assert_eq!(fixture.app.store.acquisition_counters().unwrap(), quota);
    sql.execute(
        "UPDATE capability_acquisitions SET version=?1 WHERE id=?2",
        rusqlite::params![record.version(), record.id()],
    )
    .unwrap();
    fixture.reopen(fixture.config.clone()).await;
    assert_eq!(fixture.history(record.id()).await, history);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn scheduler_attention_is_journaled_once_and_local_fencing_clears_it_without_effects() {
    let mut policy = policy();
    policy.quotas.model_calls = 1;
    let fixture = Fixture::with_policy(true, policy).await;
    let intent = seed(&fixture.app, "attention").await;
    let first = fixture.create(&intent).await;
    let started = fixture
        .save(&fixture.begin(&first), Some(AcquisitionDebit::ModelCall))
        .unwrap();
    let mut session = started.session().clone();
    session.recover_interrupted(&suite()).unwrap();
    fixture
        .save(&started.with_session(session).unwrap(), None)
        .unwrap();
    let mut next_intent = intent.clone();
    next_intent.client_request_id = "history-second-quota-intent".into();
    let record = fixture.create(&next_intent).await;
    let mut automatic = fixture.app.clone();
    automatic.automatic_acquisition = true;
    assert_eq!(drain_once(&automatic).await.unwrap().attention, 1);
    let attention = fixture.history(record.id()).await;
    assert_eq!(events(&attention), vec!["created", "attention"]);
    assert_eq!(items(&attention)[1]["attention"], "quota");
    assert_eq!(items(&attention)[1]["snapshot"]["version"], 1);
    let second = items(&fixture.get(BASE).await)
        .iter()
        .find(|item| item["snapshot"]["id"] == record.id())
        .unwrap()
        .clone();
    assert_eq!(second["attention"], "quota");
    // Repeated observation of the same blocked version must not grow history.
    fixture.sql().execute("UPDATE acquisition_automation SET next_check_at='2000-01-01T00:00:00.000000000Z' WHERE acquisition_id=?1", [record.id()]).unwrap();
    drain_once(&automatic).await.unwrap();
    assert_eq!(fixture.history(record.id()).await, attention);
    let counts = fixture.app.store.acquisition_counters().unwrap();
    let mut state = fixture.app.current_state().unwrap();
    let revision = state.revision;
    state.roles.get_mut(&intent.role_id).unwrap().state = RoleState::Draining;
    state.revision += 1;
    fixture
        .app
        .store
        .commit_state(
            revision,
            &state,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    fixture.sql().execute("UPDATE acquisition_automation SET next_check_at='2000-01-01T00:00:00.000000000Z' WHERE acquisition_id=?1", [record.id()]).unwrap();
    drain_once(&automatic).await.unwrap();
    let history = fixture.history(record.id()).await;
    // A new checkpoint version supersedes prior attention in its transition;
    // attention_cleared is reserved for clearing an error at the same version.
    assert_eq!(events(&history), vec!["created", "attention", "transition"]);
    assert_eq!(&items(&history)[..2], items(&attention));
    let catalog = fixture.get(BASE).await;
    let latest = items(&catalog)
        .iter()
        .find(|item| item["snapshot"]["id"] == record.id())
        .unwrap();
    assert!(latest["attention"].is_null());
    assert_eq!(latest["snapshot"]["active"], false);
    assert_eq!(fixture.app.store.acquisition_counters().unwrap().active, 0);
    assert_eq!(
        fixture
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .model_calls,
        counts.model_calls
    );
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}
