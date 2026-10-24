//! Coordinator composition over real private-journal receipts and genuine
//! loopback invocation/assessment proofs. The bridge models its authenticated
//! sender boundary; it does not claim SDK signatures, cloud execution, native
//! provisioning, live source freshness or a venue booking.
use super::*;
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
    fn sends(&self) -> Vec<SendRequest> {
        self.world.lock().unwrap().sends.clone()
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
    } else {
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
    let before = f.side_effect_counts();
    let (path, registered) = register(&f, body.clone()).await;
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
    assert_eq!(f.providers.execution_posts().len(), 2);
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
        assert!(step(&f, &path).await["attention"].is_string());
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
