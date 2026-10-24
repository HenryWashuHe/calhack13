//! Coordinator composition over real private-journal receipts and genuine
//! loopback invocation/assessment proofs. The bridge models its authenticated
//! sender boundary; it does not claim SDK signatures, cloud execution, native
//! provisioning, live source freshness or a venue booking.
use super::*;
#[path = "recovery/native.rs"]
mod native;
use genesis_fetch::{
    client::{BridgeClient, ClientLimits},
    protocol::{Body as FetchBody, InboxItem, Message, SendRequest, PROTOCOL_DIGEST},
    runtime::RuntimeStore as FetchStore,
};
use genesis_server::venue_recovery::FetchRecoveryRuntime;

const BRIDGE: &str = "agent1qgllj55vfr6ueyktacwgsv9fysg0qtexqrv4l738eq006km8e2hxu8pg5ev";
const PEER: &str = "agent1q2hsltlcv7afefqgreazmw8fce28fql0dnlas6jezt9nzqceru7au62r39n";
const TOKEN: &str = "synthetic-private-bridge-token";

#[derive(Default)]
struct BridgeWorld {
    sends: Vec<SendRequest>,
    inbox: Vec<InboxItem>,
    bad_output: bool,
    offline: bool,
    release_failures_remaining: u32,
}
struct Bridge {
    server: Server,
    world: Arc<Mutex<BridgeWorld>>,
}
impl Bridge {
    async fn start() -> Self {
        let world = Arc::new(Mutex::new(BridgeWorld::default()));
        let server = Server::router(
            Router::new()
                .fallback(bridge_reply)
                .with_state(world.clone()),
        )
        .await;
        Self { server, world }
    }
    // Contract work and terminal cleanup are independently counted: retiring
    // a grant must not be mistaken for re-recruiting or rerunning selection.
    fn sends(&self) -> Vec<SendRequest> {
        self.world
            .lock()
            .unwrap()
            .sends
            .iter()
            .filter(|send| !matches!(send.message.body, FetchBody::ReleaseContract { .. }))
            .cloned()
            .collect()
    }
    fn release_sends(&self) -> Vec<SendRequest> {
        self.world
            .lock()
            .unwrap()
            .sends
            .iter()
            .filter(|send| matches!(send.message.body, FetchBody::ReleaseContract { .. }))
            .cloned()
            .collect()
    }
    async fn attach(&self, f: &mut Fixture) -> Arc<FetchStore> {
        f.server.stop().await;
        let journal = Arc::new(
            FetchStore::open(
                f.directory.path().join("fetch-recovery.sqlite"),
                ORG,
                BRIDGE,
            )
            .unwrap(),
        );
        let client = BridgeClient::new(
            &format!("{}/v1", self.server.url),
            TOKEN,
            BRIDGE,
            PROTOCOL_DIGEST,
            ClientLimits {
                timeout: Duration::from_secs(2),
                ..Default::default()
            },
        )
        .unwrap();
        f.app.fetch_recovery = Some(Arc::new(
            FetchRecoveryRuntime::new(client, journal.clone(), PEER.into()).unwrap(),
        ));
        f.server = Server::org(f.app.clone(), false).await;
        journal
    }
}

// Canned answers for the three explicitly configured snapshot facts. Do not
// call the production evaluator to generate the output it will later verify.
fn selector_output(input: &Value, bad: bool) -> Value {
    assert_eq!(input["kind"], "venue_replacement_v1");
    assert_eq!(input["schema_version"], 1);
    let context = &input["context"];
    assert_eq!(input["context_sha256"], json_sha256(context));
    assert_eq!(context["policy"]["selected_venue_id"], "grand-hall");
    assert!(input.get("expected").is_none());
    assert!(!input.to_string().contains(ORACLE_CANARY));
    let candidates = context["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3);
    let evaluations:Vec<_>=candidates.iter().map(|candidate| {
        let id=candidate["venue"]["venue_id"].as_str().unwrap();
        let (capacity,available,step_free,eligible)=match id {
            "grand-hall"=>(false,true,true,false),
            "makers-loft"=>(false,false,false,false),
            "replacement-hall"=>(true,true,true,true),
            _=>panic!("unexpected fixed candidate"),
        };
        json!({"source_job_id":candidate["source_job_id"],"venue_id":id,
            "invocation_proof_ref":candidate["invocation_proof_ref"],"evidence":candidate["venue"]["evidence"],
            "checks":{"capacity":if bad {!capacity}else{capacity},"price":true,"window":true,"availability":available,"step_free":step_free},
            "eligible":eligible})
    }).collect();
    let chosen = candidates
        .iter()
        .find(|c| c["venue"]["venue_id"] == "replacement-hall")
        .unwrap();
    json!({"schema_version":1,"recovery_id":context["recovery_id"],"policy_sha256":context["policy_sha256"],
        "context_sha256":input["context_sha256"],"evaluations":evaluations,
        "selected":{"source_job_id":chosen["source_job_id"],"venue_id":"replacement-hall"}})
}
async fn bridge_reply(
    State(world): State<Arc<Mutex<BridgeWorld>>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    assert_eq!(
        request.headers().get("authorization").unwrap(),
        format!("Bearer {TOKEN}").as_str()
    );
    if request.method() == Method::GET && world.lock().unwrap().offline {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"fixture bridge unavailable"})),
        );
    }
    let method = request.method().clone();
    let uri = request.uri().clone();
    if method == Method::GET && uri.path() == "/v1/identity" {
        return (
            StatusCode::OK,
            Json(json!({"schema_version":1,"address":BRIDGE,"protocol_digest":PROTOCOL_DIGEST})),
        );
    }
    if method == Method::GET && uri.path() == "/v1/inbox" {
        let url = reqwest::Url::parse(&format!("http://127.0.0.1{uri}")).unwrap();
        let after: u64 = url
            .query_pairs()
            .find(|(k, _)| k == "after")
            .unwrap()
            .1
            .parse()
            .unwrap();
        let world = world.lock().unwrap();
        let items: Vec<_> = world
            .inbox
            .iter()
            .filter(|item| item.sequence > after)
            .take(50)
            .cloned()
            .collect();
        let next = items.last().map_or(after, |item| item.sequence);
        return (
            StatusCode::OK,
            Json(json!({"schema_version":1,"address":BRIDGE,"items":items,"next_cursor":next})),
        );
    }
    assert_eq!(method, Method::POST);
    assert_eq!(uri.path(), "/v1/messages");
    let bytes = to_bytes(request.into_body(), 96 * 1024).await.unwrap();
    let send: SendRequest = decode_json(&bytes).unwrap();
    assert_eq!(send.recipient, PEER);
    send.message.validate().unwrap();
    let mut world = world.lock().unwrap();
    if let Some(old) = world
        .sends
        .iter()
        .find(|old| old.message.message_id == send.message.message_id)
    {
        assert_eq!(
            json!(old),
            json!(send),
            "bridge retries must preserve the complete immutable request"
        );
    } else if !matches!(send.message.body, FetchBody::ReleaseContract { .. }) {
        let body = match &send.message.body {
            FetchBody::RecruitRequest {
                capabilities,
                lease_expires_at,
                ..
            } => {
                assert_eq!(capabilities, &["venue.select"]);
                FetchBody::Offer {
                    recruit_message_id: send.message.message_id.clone(),
                    offer_id: "fixed-offer".into(),
                    capabilities: capabilities.clone(),
                    terms: json!({"fixture":"fixed-facts-only"}),
                    lease_expires_at: *lease_expires_at,
                }
            }
            FetchBody::AcceptOffer { offer_id, .. } => FetchBody::ContractAccepted {
                accept_message_id: send.message.message_id.clone(),
                offer_id: offer_id.clone(),
            },
            FetchBody::TaskRequest { task } => FetchBody::TaskResult {
                task_request_id: send.message.message_id.clone(),
                job_id: task.job_id.clone(),
                attempt_id: task.attempt_id.clone(),
                epoch: task.epoch,
                output: selector_output(&task.input, world.bad_output),
            },
            _ => panic!("unexpected outgoing bridge message"),
        };
        let reply = Message {
            message_id: uuid::Uuid::new_v4().to_string(),
            body,
            ..send.message.clone()
        };
        reply.validate().unwrap();
        let sequence = world.inbox.len() as u64 + 1;
        world.inbox.push(InboxItem {
            sequence,
            sender: PEER.into(),
            message: reply,
        });
    }
    world.sends.push(send.clone());
    // A queue response never manufactures a peer ContractReleased message.
    // Failed cleanup responses model uncertain POST outcomes with an exact
    // request retained by this idempotent private bridge fixture.
    if matches!(send.message.body, FetchBody::ReleaseContract { .. })
        && world.release_failures_remaining > 0
    {
        world.release_failures_remaining -= 1;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"unknown cleanup outcome"})),
        );
    }
    (
        StatusCode::ACCEPTED,
        Json(
            json!({"schema_version":1,"message_id":send.message.message_id,"bridge_address":BRIDGE,"recipient":PEER,"state":"queued"}),
        ),
    )
}

async fn reopened(f: &Fixture) -> (String, Value) {
    // Explicit trusted setup grants the existing CREAO role contractor scope.
    // The recovery endpoint must still create and bind a real new responsibility.
    f.mutate(|state| {
        state
            .roles
            .get_mut(&RoleId::new(&f.monitor))
            .unwrap()
            .authority
            .scopes
            .insert("contract.logistics".into());
    })
    .await;
    let source = f.source("recovery-frozen-cut-and-replacement", 3).await;
    let assessment = f.create(f.request_body("recovery-reopen", &source)).await;
    f.collected(&assessment).await;
    let accepted = f.verify(&assessment).await;
    assert_eq!(proof(&accepted)["output"]["verdict"], "reopen");
    assert!(proof(&accepted)["requirement_id"].is_string());
    (source, accepted)
}
fn recovery_request(source: &str, assessment: &Value) -> Value {
    let deadline = chrono::DateTime::<Utc>::from_timestamp_micros(
        (Utc::now() + ChronoDuration::minutes(10)).timestamp_micros(),
    )
    .unwrap();
    json!({"client_request_id":"fixed-recovery","reopened_assessment_id":assessment["assessment_id"],
        "candidate_job_ids":[source],"deadline":deadline})
}
async fn step(f: &Fixture, path: &str) -> Value {
    let (status, record) = f.post(&format!("{path}/step"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{record}");
    record
}
async fn register(f: &Fixture, body: Value) -> (String, Value) {
    let (status, record) = f.post("/venue-recoveries", body).await;
    assert_eq!(status, StatusCode::CREATED, "{record}");
    (
        format!(
            "/venue-recoveries/{}",
            record["recovery_id"].as_str().unwrap()
        ),
        record,
    )
}
fn receipt(journal: &FetchStore, bridge: &Bridge) -> genesis_fetch::TaskReceipt {
    let sends = bridge.sends();
    assert_eq!(sends.len(), 3);
    journal
        .task_receipt(
            PEER,
            &sends[0].message,
            &sends[1].message,
            &sends[2].message,
        )
        .unwrap()
}
fn assessment_count(f: &Fixture) -> u64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_assessments", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn recovery_uses_exact_fetch_receipt_then_one_creao_child_across_restart() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let body = recovery_request(&source, &reopened);
    f.mutate(|state| state.constitution.max_active_contracts = 1)
        .await;
    let before = f.side_effect_counts();
    let (path, registered) = register(&f, body.clone()).await;
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 1);
    assert_eq!(registered["phase"], "recruiting");
    assert!(bridge.sends().is_empty());
    assert_ne!(registered["responsibility_id"], f.responsibility);
    assert_eq!(
        registered["requirement_id"],
        proof(&reopened)["requirement_id"]
    );
    let job = f
        .app
        .store
        .get_job(&JobId::new(registered["job_id"].as_str().unwrap()))
        .unwrap();
    assert_eq!(job.provider, Some(Provider::Fetch));
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(job.role_id.as_str(), f.monitor);
    assert_eq!(
        job.responsibility_id.as_str(),
        registered["responsibility_id"].as_str().unwrap()
    );
    let state = f.app.current_state().unwrap();
    assert_eq!(
        state.responsibilities[&job.responsibility_id].executor,
        Some(genesis_core::model::Executor::Contractor(ContractId::new(
            registered["contract_id"].as_str().unwrap()
        )))
    );
    let (status, replay) = f.post("/venue-recoveries", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, registered);
    let mut changed = body.clone();
    changed["deadline"] = json!(Utc::now());
    assert_eq!(
        f.post("/venue-recoveries", changed).await.0,
        StatusCode::CONFLICT
    );

    for _ in 0..10 {
        let record = step(&f, &path).await;
        assert!(record["attention"].is_null(), "{record}");
        if bridge.sends().len() == 3 {
            break;
        }
    }
    assert_eq!(bridge.sends().len(), 3);
    assert_eq!(
        f.app.store.get_job(&job.job_id).unwrap().status,
        JobStatus::Queued,
        "queued transport receipt cannot establish a verified selection"
    );
    let frozen = f.get(&path).await;
    f.restart(false).await;
    let journal = bridge.attach(&mut f).await;
    assert_eq!(f.get(&path).await, frozen);
    let selected = step(&f, &path).await;
    assert_eq!(selected["phase"], "assessing", "{selected}");
    assert_eq!(selected["selected_venue_id"], "replacement-hall");
    assert!(selected["assessment_id"].is_null());
    let native = receipt(&journal, &bridge);
    assert_eq!(selected["receipt_sha256"], native.receipt_sha256());
    assert_eq!(native.job_id(), job.job_id.as_str());
    assert_eq!(native.assignment_id(), job.assignment_id.as_str());
    assert_eq!(native.epoch(), job.epoch);
    assert_eq!(native.recruitment().org_id(), ORG);
    assert_eq!(native.recruitment().peer_address(), PEER);
    assert_eq!(
        native.recruitment().contract_id(),
        registered["contract_id"].as_str().unwrap()
    );
    assert_eq!(
        f.app.store.get_job(&job.job_id).unwrap().status,
        JobStatus::Succeeded
    );
    assert_eq!(f.side_effect_counts(), (before.0 + 1, before.1));
    let prepared = step(&f, &path).await;
    assert!(prepared["attention"].is_null(), "{prepared}");
    let child_path = format!(
        "/venue-assessments/{}",
        prepared["assessment_id"].as_str().unwrap()
    );
    let child = f.get(&child_path).await;
    assert_eq!(child["source_job_id"], source);
    assert_eq!(child["responsibility_id"], registered["responsibility_id"]);
    assert_eq!(child["role_id"], f.monitor);
    assert_eq!(child["policy"]["selected_venue_id"], "replacement-hall");
    f.collected(&child).await;
    assert!(
        f.get(&child_path).await["proof"].is_null(),
        "native completion is not semantic acceptance"
    );
    let accepted = f.verify(&child).await;
    assert_eq!(proof(&accepted)["decision"], "accepted");
    assert_eq!(proof(&accepted)["output"]["verdict"], "feasible");
    assert_eq!(proof(&accepted)["provider_agent_id"], AGENT);
    let completed = step(&f, &path).await;
    assert_eq!(completed["phase"], "completed");
    assert_eq!(completed["retirement"]["reason"], "completed");
    assert_eq!(completed["retirement"]["local_contract_state"], "RELEASED");
    assert_eq!(completed["retirement"]["delivery"], "bridge_queued");
    assert_eq!(bridge.release_sends().len(), 1);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(
        f.app.current_state().unwrap().contracts
            [&ContractId::new(registered["contract_id"].as_str().unwrap())]
            .state,
        genesis_core::lifecycle::ContractState::Released
    );
    assert!(!bridge
        .world
        .lock()
        .unwrap()
        .inbox
        .iter()
        .any(|item| matches!(item.message.body, FetchBody::ContractReleased { .. })));
    assert_eq!(
        completed["assessment_proof_ref"],
        proof(&accepted)["proof_ref"]
    );
    assert_eq!(f.side_effect_counts(), (before.0 + 2, before.1));
    assert_eq!(assessment_count(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);
    let native_hash = native.receipt_sha256().to_owned();
    f.restart(false).await;
    let journal = bridge.attach(&mut f).await;
    assert_eq!(f.get(&path).await, completed);
    assert_eq!(step(&f, &path).await, completed);
    assert_eq!(
        f.post("/venue-recoveries", body).await,
        (StatusCode::OK, completed)
    );
    assert_eq!(receipt(&journal, &bridge).receipt_sha256(), native_hash);
    assert_eq!(bridge.sends().len(), 3);
    assert_eq!(bridge.release_sends().len(), 1);
    assert_eq!(f.providers.execution_posts().len(), 2);

    // A genuinely new verified invocation and accepted Reopen produce a second
    // requirement; no old proof or requirement is relabeled to test capacity.
    let second_source = f.source("second-recovery-cut", 3).await;
    let second_assessment = f
        .create(f.request_body("second-recovery-reopen", &second_source))
        .await;
    f.collected(&second_assessment).await;
    let second_reopen = f.verify(&second_assessment).await;
    assert_eq!(proof(&second_reopen)["output"]["verdict"], "reopen");
    assert_ne!(
        proof(&second_reopen)["requirement_id"],
        proof(&reopened)["requirement_id"]
    );
    let mut second_request = recovery_request(&second_source, &second_reopen);
    second_request["client_request_id"] = json!("second-genuine-recovery");
    let (_, second) = register(&f, second_request).await;
    assert_ne!(second["contract_id"], registered["contract_id"]);
    assert_ne!(second["responsibility_id"], registered["responsibility_id"]);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 1);
    assert_eq!(
        bridge.sends().len(),
        3,
        "registration must not dispatch the second contract"
    );
    assert_eq!(bridge.release_sends().len(), 1);
}

#[tokio::test]
async fn authenticated_but_wrong_selection_cannot_create_an_assessment_or_mission_effect() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.world.lock().unwrap().bad_output = true;
    let journal = bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, registered) = register(&f, recovery_request(&source, &reopened)).await;
    let before = f.side_effect_counts();
    let mut rejected = Value::Null;
    for _ in 0..10 {
        rejected = step(&f, &path).await;
        if rejected["attention"].is_string() {
            break;
        }
    }
    assert!(rejected["attention"].is_string(), "{rejected}");
    let native = receipt(&journal, &bridge);
    assert_eq!(native.job_id(), registered["job_id"].as_str().unwrap());
    assert_eq!(rejected["receipt_sha256"], native.receipt_sha256(),
        "semantic rejection must retain the authenticated native receipt without claiming a verified selection");
    assert!(rejected["proof_ref"].is_null());
    assert!(rejected["assessment_id"].is_null());
    assert_ne!(
        f.app
            .store
            .get_job(&JobId::new(native.job_id()))
            .unwrap()
            .status,
        JobStatus::Succeeded
    );
    assert_eq!(f.side_effect_counts(), before);
    assert_eq!(assessment_count(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    f.restart(false).await;
    bridge.attach(&mut f).await;
    for _ in 0..3 {
        let saved = step(&f, &path).await;
        assert!(saved["attention"].is_string());
        assert_eq!(saved["receipt_sha256"], native.receipt_sha256());
        assert!(saved["proof_ref"].is_null());
    }
    assert_eq!(bridge.sends().len(), 3);
    assert_eq!(f.side_effect_counts(), before);
}

#[tokio::test]
async fn recovery_rejects_foreign_event_sources_and_unprivileged_creation_before_transport() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let body = recovery_request(&source, &reopened);
    let before = f.app.current_state().unwrap();
    let (status, _) = f
        .request(
            Method::POST,
            "/venue-recoveries",
            "agent",
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(f.app.current_state().unwrap(), before);
    let different_event = f.source("recovery-wrong-event", 1).await;
    let mut invalid = body;
    invalid["candidate_job_ids"] = json!([different_event]);
    let before = f.app.current_state().unwrap();
    let (status, rejected) = f.post("/venue-recoveries", invalid).await;
    assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
    assert_eq!(f.app.current_state().unwrap(), before);
    assert!(bridge.sends().is_empty());
    let rows: u64 = sqlite(&f)
        .query_row("SELECT COUNT(*) FROM venue_recoveries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(assessment_count(&f), 1);
}

async fn followup_without_bridge(interrupt_before_child: bool) {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, _) = register(&f, recovery_request(&source, &reopened)).await;
    let mut selected = Value::Null;
    for _ in 0..10 {
        selected = step(&f, &path).await;
        assert!(selected["attention"].is_null(), "{selected}");
        if selected["phase"] == "assessing" {
            break;
        }
    }
    assert_eq!(selected["phase"], "assessing");
    assert!(selected["receipt_sha256"].is_string());
    if !interrupt_before_child {
        selected = step(&f, &path).await;
        assert!(selected["assessment_id"].is_string(), "{selected}");
    }
    bridge.world.lock().unwrap().offline = true;
    let prepared = step(&f, &path).await;
    assert!(prepared["attention"].is_null(),
            "verified selection cannot need another remote Fetch response (before_child={interrupt_before_child}): {prepared}");
    assert!(prepared["assessment_id"].is_string(), "{prepared}");
    let child = f
        .get(&format!(
            "/venue-assessments/{}",
            prepared["assessment_id"].as_str().unwrap()
        ))
        .await;
    f.collected(&child).await;
    let accepted = f.verify(&child).await;
    assert_eq!(proof(&accepted)["output"]["verdict"], "feasible");
    let mut completed = Value::Null;
    for _ in 0..30 {
        genesis_server::venue_recovery::drain_once(&f.app)
            .await
            .unwrap();
        completed = f.get(&path).await;
        if completed["phase"] == "completed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(completed["phase"], "completed", "{completed}");
    assert_eq!(completed["assessment_id"], prepared["assessment_id"]);
    assert_eq!(bridge.sends().len(), 3);
    assert_eq!(f.providers.execution_posts().len(), 2);
    assert_eq!(assessment_count(&f), 2);
}

#[tokio::test]
async fn verified_selection_prepares_creao_child_despite_fetch_inbox_outage() {
    followup_without_bridge(true).await;
}

#[tokio::test]
async fn pending_creao_child_completes_parent_despite_fetch_inbox_outage() {
    followup_without_bridge(false).await;
}

#[tokio::test]
async fn corrupt_full_worker_page_is_deferred_without_starving_the_next_genuine_recovery() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    let journal = bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, healthy) = register(&f, recovery_request(&source, &reopened)).await;
    let before = f.side_effect_counts();
    let db = sqlite(&f);
    // Deliberate private-storage corruption fixture: these are NOT valid
    // recoveries, native receipts or source proofs. Bypass only this connection's
    // FK checks to model unreadable legacy/copied rows. The healthy seventeenth
    // record above came entirely from the real public API and verified evidence.
    db.pragma_update(None, "foreign_keys", false).unwrap();
    let bad_bytes = "{this is intentionally not a recovery checkpoint}";
    for index in 0..16 {
        db.execute("INSERT INTO venue_recoveries
            (recovery_id,client_request_id,requirement_id,contract_id,job_id,revision,active,next_check_at,data)
            VALUES (?1,?2,?3,?4,?5,0,1,'2000-01-01T00:00:00Z',?6)",rusqlite::params![
                format!("corrupt-recovery-{index:02}"),format!("corrupt-request-{index:02}"),
                format!("corrupt-requirement-{index:02}"),format!("corrupt-contract-{index:02}"),
                format!("corrupt-job-{index:02}"),bad_bytes]).unwrap();
    }
    let first = genesis_server::venue_recovery::drain_once(&f.app).await;
    assert!(
        first.is_ok(),
        "one unreadable row must not abort the batch: {first:?}"
    );
    assert_eq!(first.unwrap(), 16, "the pass remains bounded");
    assert!(
        bridge.sends().is_empty(),
        "healthy record is beyond the first full page"
    );
    assert_eq!(f.get(&path).await, healthy);
    for index in 0..16 {
        let (data,revision,active,due):(String,u64,bool,String)=db.query_row(
            "SELECT data,revision,active,next_check_at FROM venue_recoveries WHERE recovery_id=?1",
            [format!("corrupt-recovery-{index:02}")],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!(
            data, bad_bytes,
            "advisory scheduling cannot rewrite corrupt evidence"
        );
        assert_eq!(revision, 0);
        assert!(active);
        let due = chrono::DateTime::parse_from_rfc3339(&due).unwrap();
        assert!(
            due > Utc::now() + ChronoDuration::seconds(30),
            "unreadable rows must leave the next bounded page"
        );
    }
    let second = genesis_server::venue_recovery::drain_once(&f.app)
        .await
        .unwrap();
    assert_eq!(second, 1);
    let sends = bridge.sends();
    assert_eq!(
        sends.len(),
        1,
        "the healthy seventeenth record must enqueue recruitment on the second pass"
    );
    assert_eq!(
        sends[0].message.message_id,
        healthy["recruit_message_id"].as_str().unwrap()
    );
    assert_eq!(
        sends[0].message.contract_id,
        healthy["contract_id"].as_str().unwrap()
    );
    assert!(
        journal.pending(Utc::now()).unwrap().is_empty(),
        "the genuine bridge queue receipt was durably acknowledged"
    );
    assert_eq!(
        assessment_count(&f),
        1,
        "no extra CREAO assessment before verified selection"
    );
    assert_eq!(f.side_effect_counts(), before);
    assert!(
        f.get(&path).await["receipt_sha256"].is_null(),
        "a queue receipt is not native task completion"
    );
}

fn cleanup_window(record: &Value) {
    let retired: chrono::DateTime<Utc> =
        serde_json::from_value(record["retirement"]["retired_at"].clone()).unwrap();
    let expires: chrono::DateTime<Utc> =
        serde_json::from_value(record["retirement"]["release_expires_at"].clone()).unwrap();
    assert_eq!(expires - retired, ChronoDuration::minutes(30));
    assert_ne!(
        record["retirement"]["release_message_id"],
        record["recruit_message_id"]
    );
    assert_ne!(
        record["retirement"]["release_message_id"],
        record["task_message_id"]
    );
}
async fn expiring_recovery(f: &Fixture) -> (String, Value, chrono::DateTime<Utc>) {
    let (source, reopened) = reopened(f).await;
    f.mutate(|state| state.constitution.max_active_contracts = 1)
        .await;
    let deadline = chrono::DateTime::from_timestamp_micros(
        (Utc::now() + ChronoDuration::seconds(3)).timestamp_micros(),
    )
    .unwrap();
    let mut body = recovery_request(&source, &reopened);
    body["deadline"] = json!(deadline);
    let (path, record) = register(f, body).await;
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 1);
    let pending = step(f, &path).await; // Persist and queue exactly one recruit.
    assert_eq!(pending["phase"], "recruiting");
    assert!(pending["attention"].is_null(), "{pending}");
    (path, record, deadline)
}
async fn wait_until_expired(deadline: chrono::DateTime<Utc>) {
    let wait = (deadline - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(20);
    tokio::time::sleep(wait).await;
}

#[tokio::test]
async fn paused_expired_recovery_retires_without_runtime_then_retries_exact_cleanup_after_restart()
{
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (path, registered, deadline) = expiring_recovery(&f).await;
    bridge.world.lock().unwrap().offline = true;
    let paused = step(&f, &path).await;
    assert!(paused["attention"].is_string(), "{paused}");
    assert_eq!(bridge.sends().len(), 1);
    // Expired cleanup is not a fresh delegation and must not need renewed role
    // authority or provider configuration merely to retire the local grant.
    f.mutate(|state| {
        let authority = &mut state
            .roles
            .get_mut(&RoleId::new(&f.monitor))
            .unwrap()
            .authority;
        authority.scopes.remove("contract.logistics");
        authority.scopes.remove("monitor.venue");
    })
    .await;
    f.server.stop().await;
    f.app.fetch_recovery = None;
    f.server = Server::org(f.app.clone(), false).await;
    let effects = f.side_effect_counts();
    wait_until_expired(deadline).await;
    genesis_server::venue_recovery::drain_once(&f.app)
        .await
        .unwrap();
    let retired = f.get(&path).await;
    assert_eq!(retired["phase"], "retired");
    assert_eq!(retired["retirement"]["reason"], "expired");
    assert_eq!(retired["retirement"]["local_contract_state"], "DECLINED");
    assert_eq!(retired["retirement"]["delivery"], "needs_attention");
    assert_eq!(retired["retirement"]["send_attempts"], 0);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(
        f.app.current_state().unwrap().contracts
            [&ContractId::new(registered["contract_id"].as_str().unwrap())]
            .state,
        genesis_core::lifecycle::ContractState::Declined
    );
    assert_eq!(
        f.app
            .store
            .get_job(&JobId::new(registered["job_id"].as_str().unwrap()))
            .unwrap()
            .status,
        JobStatus::Failed
    );
    assert!(retired["assessment_id"].is_null());
    cleanup_window(&retired);
    assert_eq!(assessment_count(&f), 1);
    assert_eq!(bridge.sends().len(), 1);
    assert!(bridge.release_sends().is_empty());
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(f.side_effect_counts(), effects);

    f.restart(false).await;
    bridge.world.lock().unwrap().offline = false;
    bridge.world.lock().unwrap().release_failures_remaining = 1;
    bridge.attach(&mut f).await;
    let uncertain = step(&f, &path).await;
    assert_eq!(uncertain["retirement"]["delivery"], "pending");
    assert_eq!(uncertain["retirement"]["send_attempts"], 1);
    assert_eq!(
        uncertain["retirement"]["release_message_id"],
        retired["retirement"]["release_message_id"]
    );
    let exact = bridge.release_sends()[0].clone();
    assert_eq!(
        json!(exact.message.expires_at),
        retired["retirement"]["release_expires_at"]
    );
    f.restart(false).await;
    let journal = bridge.attach(&mut f).await;
    let queued = step(&f, &path).await;
    assert_eq!(queued["retirement"]["delivery"], "bridge_queued");
    assert_eq!(queued["retirement"]["send_attempts"], 2);
    assert_eq!(
        queued["retirement"]["release_message_id"],
        retired["retirement"]["release_message_id"]
    );
    assert_eq!(
        queued["retirement"]["release_expires_at"],
        retired["retirement"]["release_expires_at"]
    );
    assert_eq!(json!(bridge.release_sends()[1]), json!(exact));
    assert_eq!(
        journal.outgoing_queued(PEER, &exact.message).unwrap(),
        Some(true)
    );
    assert!(
        !bridge
            .world
            .lock()
            .unwrap()
            .inbox
            .iter()
            .any(|item| matches!(item.message.body, FetchBody::ContractReleased { .. })),
        "bridge queue acknowledgment must not be mistaken for peer release evidence"
    );
    assert_eq!(step(&f, &path).await, queued);
    assert_eq!(bridge.release_sends().len(), 2);
    assert_eq!(bridge.sends().len(), 1);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(f.side_effect_counts(), effects);
}

#[tokio::test]
async fn cleanup_lifetime_attempt_bound_survives_explicit_retry_and_restart() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (path, _, deadline) = expiring_recovery(&f).await;
    bridge.world.lock().unwrap().release_failures_remaining = 10;
    wait_until_expired(deadline).await;
    let first = step(&f, &path).await;
    assert_eq!(first["phase"], "retired");
    assert_eq!(first["retirement"]["send_attempts"], 1);
    assert_eq!(first["retirement"]["delivery"], "pending");
    cleanup_window(&first);
    let second = step(&f, &path).await;
    assert_eq!(second["retirement"]["send_attempts"], 2);
    let third = step(&f, &path).await;
    assert_eq!(third["retirement"]["send_attempts"], 3);
    assert_eq!(third["retirement"]["delivery"], "needs_attention");
    assert_eq!(bridge.release_sends().len(), 3);
    let exact = bridge.release_sends()[0].clone();
    assert!(bridge
        .release_sends()
        .iter()
        .all(|request| json!(request) == json!(exact)));
    f.restart(false).await;
    bridge.attach(&mut f).await;
    for _ in 0..3 {
        let (status, retried) = f.post(&format!("{path}/retry"), json!({})).await;
        assert_eq!(status, StatusCode::OK, "{retried}");
        let bounded = step(&f, &path).await;
        assert_eq!(bounded["retirement"]["send_attempts"], 3);
        assert_eq!(bounded["retirement"]["delivery"], "needs_attention");
        assert_eq!(
            bounded["retirement"]["release_message_id"],
            first["retirement"]["release_message_id"]
        );
        assert_eq!(
            bounded["retirement"]["release_expires_at"],
            first["retirement"]["release_expires_at"]
        );
    }
    assert_eq!(bridge.release_sends().len(), 3);
    assert_eq!(bridge.sends().len(), 1);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(assessment_count(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[path = "recovery_review.rs"]
mod review;
