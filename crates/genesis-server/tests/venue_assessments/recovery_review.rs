//! Review regressions for guards that the original recovery cases do not reach.
//! All recoveries come from the real public API over genuine loopback proofs.
//! Direct SQLite or kernel-state edits are explicit tamper/revocation fixtures,
//! never a substitute for recovery evidence.
use super::*;
use genesis_core::lifecycle::{ContractState, RequirementStatus};

const OTHER_PEER: &str = "agent1qwggt4n8xns6fk2e7wxwrsp8a5khlw5h4dewnrm595vqd5ucuhfss02kw06";

async fn status(f: &Fixture, method: Method, path: &str) -> StatusCode {
    let body = (method == Method::POST).then(|| json!({}));
    f.request(method, path, "operator", body).await.0
}

async fn detach_runtime(f: &mut Fixture) {
    f.server.stop().await;
    f.app.fetch_recovery = None;
    f.server = Server::org(f.app.clone(), false).await;
}

/// Replace the configured runtime with explicitly different identities. Each
/// journal file is new, so no saved Fetch evidence is relabeled.
async fn swap_runtime(f: &mut Fixture, bridge: &Bridge, file: &str, address: &str, peer: &str) {
    f.server.stop().await;
    let journal = Arc::new(FetchStore::open(f.directory.path().join(file), ORG, address).unwrap());
    let client = BridgeClient::new(
        &format!("{}/v1", bridge.server.url),
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
        FetchRecoveryRuntime::new(client, journal, peer.into()).unwrap(),
    ));
    f.server = Server::org(f.app.clone(), false).await;
}

async fn drain_until(f: &Fixture, path: &str, done: impl Fn(&Value) -> bool) -> Value {
    let mut last = Value::Null;
    for _ in 0..40 {
        genesis_server::venue_recovery::drain_once(&f.app)
            .await
            .unwrap();
        last = f.get(path).await;
        if done(&last) {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("bounded worker wait failed: {last}")
}

#[tokio::test]
async fn sealed_recovery_digest_and_every_index_column_fail_closed() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, registered) = register(&f, recovery_request(&source, &reopened)).await;
    let id = registered["recovery_id"].as_str().unwrap().to_owned();
    let db = sqlite(&f);
    // Tamper fixture: this connection may write index values that no real
    // job row has; the server's own connection is unaffected.
    db.pragma_update(None, "foreign_keys", false).unwrap();
    let saved: String = db
        .query_row(
            "SELECT data FROM venue_recoveries WHERE recovery_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();

    // A forged completion inside the sealed payload without its digest.
    db.execute(
        "UPDATE venue_recoveries SET data=json_set(data,'$.data.phase','completed',
            '$.data.child_proof_ref','forged-proof') WHERE recovery_id=?1",
        [&id],
    )
    .unwrap();
    assert_eq!(status(&f, Method::GET, &path).await, StatusCode::CONFLICT);
    assert_eq!(
        status(&f, Method::POST, &format!("{path}/step")).await,
        StatusCode::CONFLICT
    );
    db.execute(
        "UPDATE venue_recoveries SET data=?1 WHERE recovery_id=?2",
        rusqlite::params![saved, id],
    )
    .unwrap();
    assert_eq!(f.get(&path).await, registered);

    // Each scheduling/uniqueness index column must keep naming the sealed binding.
    for (column, forged) in [
        ("client_request_id", "'forged-request'"),
        ("requirement_id", "'forged-requirement'"),
        ("contract_id", "'forged-contract'"),
        ("job_id", "'forged-job'"),
        ("revision", "revision+1"),
    ] {
        let original: rusqlite::types::Value = db
            .query_row(
                &format!("SELECT {column} FROM venue_recoveries WHERE recovery_id=?1"),
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        db.execute(
            &format!("UPDATE venue_recoveries SET {column}={forged} WHERE recovery_id=?1"),
            [&id],
        )
        .unwrap();
        assert_eq!(
            status(&f, Method::GET, &path).await,
            StatusCode::CONFLICT,
            "{column} must be bound to the sealed record"
        );
        db.execute(
            &format!("UPDATE venue_recoveries SET {column}=?1 WHERE recovery_id=?2"),
            rusqlite::params![original, id],
        )
        .unwrap();
        assert_eq!(f.get(&path).await, registered);
    }
    db.execute(
        "UPDATE venue_recoveries SET recovery_id='recovery_forged' WHERE recovery_id=?1",
        [&id],
    )
    .unwrap();
    assert_eq!(
        status(&f, Method::GET, "/venue-recoveries/recovery_forged").await,
        StatusCode::CONFLICT
    );
    db.execute(
        "UPDATE venue_recoveries SET recovery_id=?1 WHERE recovery_id='recovery_forged'",
        [&id],
    )
    .unwrap();
    assert_eq!(f.get(&path).await, registered);
    assert!(bridge.sends().is_empty());
}

/// One revocation/drift per fresh registration; each must stop before any
/// Fetch preparation or bridge POST and persist a visible hold.
async fn changed_authority_pauses_before_transport(change: &str) {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    let journal = bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let body = recovery_request(&source, &reopened);
    let (path, registered) = register(&f, body.clone()).await;
    let contract = ContractId::new(registered["contract_id"].as_str().unwrap());
    let requirement = RequirementId::new(registered["requirement_id"].as_str().unwrap());
    let job = registered["job_id"].as_str().unwrap().to_owned();
    match change {
        "scope" => {
            f.mutate(|state| {
                state
                    .roles
                    .get_mut(&RoleId::new(&f.monitor))
                    .unwrap()
                    .authority
                    .scopes
                    .remove("contract.logistics");
            })
            .await
        }
        "contract" => {
            f.mutate(|state| {
                state.contracts.get_mut(&contract).unwrap().state = ContractState::Offered;
            })
            .await
        }
        "requirement" => {
            f.mutate(|state| {
                state.requirements.get_mut(&requirement).unwrap().status =
                    RequirementStatus::Satisfied;
            })
            .await
        }
        "binding" => {
            f.mutate(|state| {
                state
                    .roles
                    .get_mut(&RoleId::new(&f.monitor))
                    .unwrap()
                    .binding
                    .as_mut()
                    .unwrap()
                    .config_digest = "b".repeat(64);
            })
            .await
        }
        "executor" => {
            let responsibility =
                ResponsibilityId::new(registered["responsibility_id"].as_str().unwrap());
            f.mutate(|state| {
                state
                    .responsibilities
                    .get_mut(&responsibility)
                    .unwrap()
                    .executor = None;
            })
            .await
        }
        "peer" => swap_runtime(&mut f, &bridge, "fetch-recovery.sqlite", BRIDGE, OTHER_PEER).await,
        "job" => {
            let _kernel = f.app.kernel.lock().await;
            let changed = sqlite(&f)
                .execute(
                    "UPDATE jobs SET job_json=json_set(job_json,'$.idempotency_key','tampered')
                     WHERE job_id=?1",
                    [&job],
                )
                .unwrap();
            assert_eq!(changed, 1);
        }
        other => panic!("unknown change {other}"),
    }
    let paused = step(&f, &path).await;
    assert!(paused["attention"].is_string(), "{change}: {paused}");
    assert_eq!(paused["phase"], "recruiting");
    assert!(bridge.sends().is_empty(), "{change}");
    assert!(
        journal.pending(Utc::now()).unwrap().is_empty(),
        "{change}: no Fetch intent may be prepared"
    );
    // The hold persists across later ticks; nothing is sent.
    assert!(step(&f, &path).await["attention"].is_string());
    assert!(bridge.sends().is_empty(), "{change}");
    // A held record leaves the per-second worker page but stays scheduled for
    // deadline retirement.
    let due: String = sqlite(&f)
        .query_row(
            "SELECT next_check_at FROM venue_recoveries WHERE recovery_id=?1",
            [registered["recovery_id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let due: chrono::DateTime<Utc> = due.parse().unwrap();
    let deadline: chrono::DateTime<Utc> = serde_json::from_value(body["deadline"].clone()).unwrap();
    assert_eq!(due, deadline, "{change}");
}

#[tokio::test]
async fn revoked_contract_scope_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("scope").await;
}

#[tokio::test]
async fn drifted_contract_state_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("contract").await;
}

#[tokio::test]
async fn changed_requirement_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("requirement").await;
}

#[tokio::test]
async fn changed_creao_binding_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("binding").await;
}

#[tokio::test]
async fn changed_responsibility_executor_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("executor").await;
}

#[tokio::test]
async fn different_configured_peer_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("peer").await;
}

#[tokio::test]
async fn changed_managed_job_pauses_recovery_before_transport() {
    changed_authority_pauses_before_transport("job").await;
}

#[tokio::test]
async fn explicit_retry_after_expiry_retires_the_grant_without_new_transport() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (path, registered, deadline) = expiring_recovery(&f).await;
    assert_eq!(bridge.sends().len(), 1);
    wait_until_expired(deadline).await;
    let (code, retired) = f.post(&format!("{path}/retry"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{retired}");
    assert_eq!(retired["phase"], "retired");
    assert_eq!(retired["retirement"]["reason"], "expired");
    assert_eq!(retired["retirement"]["local_contract_state"], "DECLINED");
    assert_eq!(retired["retirement"]["delivery"], "pending");
    assert_eq!(retired["retirement"]["send_attempts"], 0);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(
        f.app.current_state().unwrap().contracts
            [&ContractId::new(registered["contract_id"].as_str().unwrap())]
            .state,
        ContractState::Declined
    );
    // Retry only retires; it neither re-sends work nor posts cleanup itself.
    assert_eq!(bridge.sends().len(), 1);
    assert!(bridge.release_sends().is_empty());
}

#[tokio::test]
async fn retry_reenables_worker_cleanup_held_for_missing_configuration() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (path, _, deadline) = expiring_recovery(&f).await;
    detach_runtime(&mut f).await;
    wait_until_expired(deadline).await;
    let held = drain_until(&f, &path, |r| r["phase"] == "retired").await;
    assert_eq!(held["retirement"]["delivery"], "needs_attention");
    assert_eq!(held["retirement"]["send_attempts"], 0);

    // Configuration returns, but a needs_attention record is not scheduled by
    // the worker until an explicit retry.
    bridge.attach(&mut f).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        genesis_server::venue_recovery::drain_once(&f.app)
            .await
            .unwrap(),
        0
    );
    assert!(bridge.release_sends().is_empty());
    assert_eq!(f.get(&path).await, held);

    let (code, retried) = f.post(&format!("{path}/retry"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{retried}");
    assert_eq!(retried["retirement"]["delivery"], "pending");
    assert!(retried["retirement"]["attention"].is_null());
    assert_eq!(
        retried["retirement"]["release_message_id"],
        held["retirement"]["release_message_id"]
    );
    let queued = drain_until(&f, &path, |r| {
        r["retirement"]["delivery"] == "bridge_queued"
    })
    .await;
    assert_eq!(queued["retirement"]["send_attempts"], 1);
    assert_eq!(bridge.release_sends().len(), 1);
    assert_eq!(
        bridge.release_sends()[0].message.message_id,
        held["retirement"]["release_message_id"].as_str().unwrap()
    );
    assert_eq!(bridge.sends().len(), 1);
}

/// Retire without a runtime, then change what cleanup is bound to before
/// configuration returns. The exact release must not be posted.
async fn changed_release_binding_holds_cleanup(change: &str) {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (path, registered, deadline) = expiring_recovery(&f).await;
    detach_runtime(&mut f).await;
    wait_until_expired(deadline).await;
    let held = drain_until(&f, &path, |r| r["phase"] == "retired").await;
    assert_eq!(held["retirement"]["delivery"], "needs_attention");
    match change {
        "contract" => {
            let contract = ContractId::new(registered["contract_id"].as_str().unwrap());
            f.mutate(|state| {
                state.contracts.get_mut(&contract).unwrap().state = ContractState::Requested;
            })
            .await;
            bridge.attach(&mut f).await;
        }
        "peer" => swap_runtime(&mut f, &bridge, "fetch-recovery.sqlite", BRIDGE, OTHER_PEER).await,
        other => panic!("unknown change {other}"),
    }
    for _ in 0..2 {
        let (code, retried) = f.post(&format!("{path}/retry"), json!({})).await;
        assert_eq!(code, StatusCode::OK, "{retried}");
        let stepped = step(&f, &path).await;
        assert_eq!(
            stepped["retirement"]["delivery"], "needs_attention",
            "{change}: {stepped}"
        );
        assert_eq!(stepped["retirement"]["send_attempts"], 0, "{change}");
        assert_eq!(
            stepped["retirement"]["release_message_id"],
            held["retirement"]["release_message_id"]
        );
    }
    assert!(bridge.release_sends().is_empty(), "{change}");
    assert_eq!(bridge.sends().len(), 1);
}

#[tokio::test]
async fn changed_retired_contract_holds_recovery_cleanup() {
    changed_release_binding_holds_cleanup("contract").await;
}

#[tokio::test]
async fn different_configured_peer_holds_recovery_cleanup() {
    changed_release_binding_holds_cleanup("peer").await;
}

#[tokio::test]
async fn foreign_organization_fetch_journal_refuses_recovery_creation() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    f.server.stop().await;
    let journal = Arc::new(
        FetchStore::open(
            f.directory.path().join("foreign-fetch-recovery.sqlite"),
            "org_foreign",
            BRIDGE,
        )
        .unwrap(),
    );
    let client = BridgeClient::new(
        &format!("{}/v1", bridge.server.url),
        TOKEN,
        BRIDGE,
        PROTOCOL_DIGEST,
        ClientLimits::default(),
    )
    .unwrap();
    f.app.fetch_recovery = Some(Arc::new(
        FetchRecoveryRuntime::new(client, journal, PEER.into()).unwrap(),
    ));
    f.server = Server::org(f.app.clone(), false).await;
    let before = f.app.current_state().unwrap();
    let (code, refused) = f
        .post("/venue-recoveries", recovery_request(&source, &reopened))
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(f.app.current_state().unwrap(), before);
    let rows: u64 = sqlite(&f)
        .query_row("SELECT COUNT(*) FROM venue_recoveries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    assert!(bridge.sends().is_empty());
}

/// Wraps the authenticated bridge fixture: the next `failures` work POSTs are
/// recorded and answered 503 (outcome unknown) or held past the caller's
/// cancellation, without reaching the fixture's queue or producing replies.
#[derive(Clone)]
struct Flaky {
    world: Arc<Mutex<BridgeWorld>>,
    failures: Arc<Mutex<u32>>,
    hang: bool,
    attempted: Arc<Mutex<Vec<SendRequest>>>,
    /// While positive, each identity preflight briefly holds the organization
    /// kernel, modeling ordinary lock contention at the guarded POST check.
    contend: Arc<Mutex<u32>>,
    /// While positive, identity preflights fail before any POST.
    identity_failures: Arc<Mutex<u32>>,
    kernel: Arc<Mutex<Option<Arc<tokio::sync::Mutex<()>>>>>,
}
async fn flaky_reply(State(flaky): State<Flaky>, request: Request) -> (StatusCode, Json<Value>) {
    if request.uri().path() == "/v1/identity" {
        let fail = {
            let mut left = flaky.identity_failures.lock().unwrap();
            let fail = *left > 0;
            *left = left.saturating_sub(1);
            fail
        };
        if fail {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"bridge identity unavailable"})),
            );
        }
        let contend = {
            let mut left = flaky.contend.lock().unwrap();
            let contend = *left > 0;
            *left = left.saturating_sub(1);
            contend
        };
        let kernel = flaky.kernel.lock().unwrap().clone();
        if let (true, Some(kernel)) = (contend, kernel) {
            let guard = kernel.lock_owned().await;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                drop(guard);
            });
        }
    }
    if request.method() != Method::POST {
        return bridge_reply(State(flaky.world), request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 96 * 1024).await.unwrap();
    let fail = {
        let mut left = flaky.failures.lock().unwrap();
        let fail = *left > 0;
        if fail {
            *left -= 1;
            flaky
                .attempted
                .lock()
                .unwrap()
                .push(decode_json(&bytes).unwrap());
        }
        fail
    };
    if !fail {
        let request = Request::from_parts(parts, axum::body::Body::from(bytes));
        return bridge_reply(State(flaky.world), request).await;
    }
    if flaky.hang {
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"unknown delivery outcome"})),
    )
}
async fn flaky_bridge(failures: u32, hang: bool) -> (Bridge, Flaky) {
    let world = Arc::new(Mutex::new(BridgeWorld::default()));
    let flaky = Flaky {
        world: world.clone(),
        failures: Arc::new(Mutex::new(failures)),
        hang,
        attempted: Arc::new(Mutex::new(Vec::new())),
        contend: Arc::new(Mutex::new(0)),
        identity_failures: Arc::new(Mutex::new(0)),
        kernel: Arc::new(Mutex::new(None)),
    };
    let server = Server::router(
        Router::new()
            .fallback(flaky_reply)
            .with_state(flaky.clone()),
    )
    .await;
    (Bridge { server, world }, flaky)
}

#[tokio::test]
async fn third_uncertain_work_post_holds_recovery_until_explicit_exact_retry() {
    let mut f = Fixture::start().await;
    let (bridge, flaky) = flaky_bridge(3, false).await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, registered) = register(&f, recovery_request(&source, &reopened)).await;
    for attempt in 1..=2 {
        let record = step(&f, &path).await;
        assert!(record["attention"].is_null(), "attempt {attempt}: {record}");
    }
    let held = step(&f, &path).await;
    assert!(held["attention"].is_string(), "{held}");
    assert_eq!(held["phase"], "recruiting");
    for _ in 0..2 {
        assert!(step(&f, &path).await["attention"].is_string());
    }
    let attempted = flaky.attempted.lock().unwrap().clone();
    assert_eq!(attempted.len(), 3, "automatic delivery is bounded");
    assert!(attempted
        .iter()
        .all(|send| json!(send) == json!(attempted[0])));
    assert_eq!(
        attempted[0].message.message_id,
        registered["recruit_message_id"].as_str().unwrap()
    );
    assert!(bridge.sends().is_empty());
    let (code, retried) = f.post(&format!("{path}/retry"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{retried}");
    assert!(retried["attention"].is_null());
    let resumed = step(&f, &path).await;
    assert!(resumed["attention"].is_null(), "{resumed}");
    let sends = bridge.sends();
    assert_eq!(sends.len(), 1);
    assert_eq!(json!(sends[0]), json!(attempted[0]));
}

#[tokio::test]
async fn cancelled_in_flight_work_posts_still_consume_the_recovery_send_bound() {
    let mut f = Fixture::start().await;
    let (bridge, flaky) = flaky_bridge(3, true).await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, registered) = register(&f, recovery_request(&source, &reopened)).await;
    let id = registered["recovery_id"].as_str().unwrap().to_owned();
    // Model a process stop while the POST is in flight: the attempt was saved
    // before transport, and its response is never observed.
    for attempt in 1..=3 {
        let cancelled = tokio::time::timeout(
            Duration::from_millis(700),
            genesis_server::venue_recovery::step(&f.app, &id),
        )
        .await;
        assert!(cancelled.is_err(), "attempt {attempt} should be in flight");
        assert_eq!(flaky.attempted.lock().unwrap().len(), attempt);
        assert!(f.get(&path).await["attention"].is_null());
    }
    tokio::time::sleep(Duration::from_millis(1600)).await;
    let held = step(&f, &path).await;
    assert!(held["attention"].is_string(), "{held}");
    assert_eq!(flaky.attempted.lock().unwrap().len(), 3);
    assert!(bridge.sends().is_empty(), "no fourth automatic POST");
}

#[tokio::test]
async fn verified_selection_rechecks_recovery_contract_before_creao_child() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, registered) = register(&f, recovery_request(&source, &reopened)).await;
    let mut selected = Value::Null;
    for _ in 0..10 {
        selected = step(&f, &path).await;
        assert!(selected["attention"].is_null(), "{selected}");
        if selected["phase"] == "assessing" {
            break;
        }
    }
    assert_eq!(selected["phase"], "assessing");
    assert!(selected["assessment_id"].is_null());
    // The accepted contract drifts outside the coordinator before the child
    // intent is dispatched. The assessment API alone does not see this grant.
    let contract = ContractId::new(registered["contract_id"].as_str().unwrap());
    f.mutate(|state| {
        state.contracts.get_mut(&contract).unwrap().state = ContractState::Released;
    })
    .await;
    for _ in 0..2 {
        let held = step(&f, &path).await;
        assert!(held["attention"].is_string(), "{held}");
        assert!(held["assessment_id"].is_null(), "{held}");
        assert_eq!(held["phase"], "assessing");
    }
    assert_eq!(assessment_count(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(bridge.sends().len(), 3);
}

#[tokio::test]
async fn kernel_contention_at_the_guarded_post_refunds_the_recovery_attempt() {
    let mut f = Fixture::start().await;
    let (bridge, flaky) = flaky_bridge(0, false).await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, registered) = register(&f, recovery_request(&source, &reopened)).await;
    *flaky.kernel.lock().unwrap() = Some(f.app.kernel.clone());
    *flaky.contend.lock().unwrap() = 4;
    // More refused pre-POST checks than the automatic bound: none consumes an
    // attempt, so none creates a delivery hold.
    for refused in 1..=4 {
        let record = step(&f, &path).await;
        assert!(record["attention"].is_null(), "refusal {refused}: {record}");
        assert!(bridge.sends().is_empty(), "refusal {refused}");
    }
    assert_eq!(*flaky.contend.lock().unwrap(), 0);
    let sent = step(&f, &path).await;
    assert!(sent["attention"].is_null(), "{sent}");
    let sends = bridge.sends();
    assert_eq!(sends.len(), 1);
    assert_eq!(
        sends[0].message.message_id,
        registered["recruit_message_id"].as_str().unwrap()
    );
}

#[tokio::test]
async fn unreadable_child_receipt_cannot_delay_expired_recovery_retirement() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let deadline = chrono::DateTime::from_timestamp_micros(
        (Utc::now() + ChronoDuration::seconds(6)).timestamp_micros(),
    )
    .unwrap();
    let mut body = recovery_request(&source, &reopened);
    body["deadline"] = json!(deadline);
    let (path, registered) = register(&f, body).await;
    let mut prepared = Value::Null;
    for _ in 0..10 {
        prepared = step(&f, &path).await;
        assert!(prepared["attention"].is_null(), "{prepared}");
        if prepared["assessment_id"].is_string() {
            break;
        }
    }
    let child = prepared["assessment_id"].as_str().unwrap().to_owned();
    assert!(
        Utc::now() < deadline,
        "fixture must reach the child in time"
    );
    // Tamper fixture: the child's sealed binding no longer authenticates.
    let changed = sqlite(&f)
        .execute(
            "UPDATE venue_assessments SET binding_json=json_set(binding_json,'$.sha256',?1)
             WHERE assessment_id=?2",
            rusqlite::params!["0".repeat(64), child],
        )
        .unwrap();
    assert_eq!(changed, 1);
    if Utc::now() < deadline {
        assert_eq!(
            status(&f, Method::POST, &format!("{path}/step")).await,
            StatusCode::CONFLICT,
            "before expiry an unreadable child is surfaced, not replaced"
        );
    }
    wait_until_expired(deadline).await;
    let retired = step(&f, &path).await;
    assert_eq!(retired["phase"], "retired", "{retired}");
    assert_eq!(retired["retirement"]["reason"], "expired");
    assert_eq!(retired["retirement"]["local_contract_state"], "RELEASED");
    assert_eq!(retired["assessment_id"], child);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(
        f.app.current_state().unwrap().contracts
            [&ContractId::new(registered["contract_id"].as_str().unwrap())]
            .state,
        ContractState::Released
    );
    assert_eq!(assessment_count(&f), 2, "no replacement child");
    assert_eq!(bridge.sends().len(), 3);
}

#[tokio::test]
async fn failed_bridge_identity_preflights_consume_the_recovery_send_bound() {
    let mut f = Fixture::start().await;
    let (bridge, flaky) = flaky_bridge(0, false).await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, _) = register(&f, recovery_request(&source, &reopened)).await;
    *flaky.identity_failures.lock().unwrap() = 10;
    for attempt in 1..=2 {
        let record = step(&f, &path).await;
        assert!(record["attention"].is_null(), "attempt {attempt}: {record}");
    }
    // Only a local eligibility refusal is refunded; an unavailable or
    // unverified bridge identity is still a bounded delivery attempt.
    let held = step(&f, &path).await;
    assert!(held["attention"].is_string(), "{held}");
    assert_eq!(*flaky.identity_failures.lock().unwrap(), 7);
    assert!(step(&f, &path).await["attention"].is_string());
    assert_eq!(
        *flaky.identity_failures.lock().unwrap(),
        7,
        "no further preflight"
    );
    assert!(bridge.sends().is_empty());
    assert!(flaky.attempted.lock().unwrap().is_empty());
}

#[test]
fn recovery_runtime_refuses_its_own_bridge_as_the_specialist_peer() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = |peer: &str| {
        let journal =
            Arc::new(FetchStore::open(directory.path().join("fetch.sqlite"), ORG, BRIDGE).unwrap());
        let client = BridgeClient::new(
            "http://127.0.0.1:9/v1",
            TOKEN,
            BRIDGE,
            PROTOCOL_DIGEST,
            ClientLimits::default(),
        )
        .unwrap();
        FetchRecoveryRuntime::new(client, journal, peer.into())
    };
    assert!(runtime(PEER).is_ok());
    assert!(runtime(BRIDGE).is_err());
    assert!(runtime("not-an-agent-address").is_err());
}

/// Retirement acts only on the exact frozen grant and job. Drift is surfaced
/// for repair instead of being retired from whatever state it reached.
async fn drifted_grant_is_not_retired(change: &str) {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (path, registered, deadline) = expiring_recovery(&f).await;
    let contract = ContractId::new(registered["contract_id"].as_str().unwrap());
    match change {
        "contract" => {
            f.mutate(|state| {
                state.contracts.get_mut(&contract).unwrap().state = ContractState::Offered;
            })
            .await
        }
        "job" => {
            let _kernel = f.app.kernel.lock().await;
            let changed = sqlite(&f)
                .execute(
                    "UPDATE jobs SET job_json=json_set(job_json,'$.idempotency_key','tampered')
                     WHERE job_id=?1",
                    [registered["job_id"].as_str().unwrap()],
                )
                .unwrap();
            assert_eq!(changed, 1);
        }
        other => panic!("unknown change {other}"),
    }
    let before = f.app.current_state().unwrap();
    wait_until_expired(deadline).await;
    for _ in 0..2 {
        assert_eq!(
            status(&f, Method::POST, &format!("{path}/step")).await,
            StatusCode::CONFLICT,
            "{change}"
        );
    }
    let saved = f.get(&path).await;
    assert!(saved["retirement"].is_null(), "{change}: {saved}");
    assert_eq!(saved["phase"], "recruiting");
    assert_eq!(f.app.current_state().unwrap(), before, "{change}");
    assert!(bridge.release_sends().is_empty());
    assert_eq!(bridge.sends().len(), 1);
}

#[tokio::test]
async fn drifted_contract_state_is_not_retired_by_recovery() {
    drifted_grant_is_not_retired("contract").await;
}

#[tokio::test]
async fn drifted_managed_job_is_not_retired_by_recovery() {
    drifted_grant_is_not_retired("job").await;
}

#[tokio::test]
async fn failed_bridge_identity_preflights_consume_lifetime_release_attempts() {
    let mut f = Fixture::start().await;
    let (bridge, flaky) = flaky_bridge(0, false).await;
    bridge.attach(&mut f).await;
    let (path, _, deadline) = expiring_recovery(&f).await;
    *flaky.identity_failures.lock().unwrap() = 10;
    wait_until_expired(deadline).await;
    let first = step(&f, &path).await;
    assert_eq!(first["phase"], "retired");
    assert_eq!(first["retirement"]["send_attempts"], 1);
    assert_eq!(first["retirement"]["delivery"], "pending");
    assert_eq!(step(&f, &path).await["retirement"]["send_attempts"], 2);
    let third = step(&f, &path).await;
    assert_eq!(third["retirement"]["send_attempts"], 3);
    assert_eq!(third["retirement"]["delivery"], "needs_attention");
    // The lifetime bound holds: retry cannot renew it, and no POST was made.
    let (code, _) = f.post(&format!("{path}/retry"), json!({})).await;
    assert_eq!(code, StatusCode::OK);
    let bounded = step(&f, &path).await;
    assert_eq!(bounded["retirement"]["send_attempts"], 3);
    assert_eq!(bounded["retirement"]["delivery"], "needs_attention");
    assert_eq!(*flaky.identity_failures.lock().unwrap(), 7);
    assert!(bridge.release_sends().is_empty());
    assert!(flaky.attempted.lock().unwrap().is_empty());
}

/// A genuine verified invocation of the same installation, suite, acceptance,
/// artifact and event input, made by a role in a second mission of this
/// organization. Only the job's mission differs from the frozen original.
async fn second_mission_source(f: &Fixture, key: &str, case: usize) -> (String, String) {
    let (status, mission) = f.post("/missions", json!({"goal":"An unrelated second mission using the same capability",
        "roles":[{"key":"other","name":"Second mission reader","charter":"Read venue snapshots for another mission",
            "authority":{"scopes":["mission.coordinate","capability.venue_lookup"],"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Read venues for the second mission","owner":"other"}]})).await;
    assert_eq!(status, StatusCode::CREATED, "{mission}");
    let mission = mission["mission_id"].as_str().unwrap().to_owned();
    assert_ne!(mission, f.mission);
    let state = f.app.current_state().unwrap();
    let role = state
        .roles
        .values()
        .find(|r| r.name == "Second mission reader")
        .unwrap()
        .role_id
        .to_string();
    let responsibility = state
        .responsibilities
        .values()
        .find(|r| r.mission_id.as_str() == mission)
        .unwrap()
        .responsibility_id
        .to_string();
    let (status, observation) = f
        .post(
            &format!("/missions/{mission}/observations"),
            json!({"source":"fixture","summary":"The second mission also needs the accepted reader"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{observation}");
    let (status, change) = f.post("/org-changes", json!({"client_request_id":format!("{key}-attach"),"proposal":{
        "expected_org_revision":f.app.current_state().unwrap().revision,"trigger_evidence_ids":[observation["observation_id"]],
        "rationale":"Attach the existing accepted capability in the second mission","operations":[
            {"op":"AttachCapability","role":{"kind":"existing","role_id":role},"capability":{"capability_id":CAPABILITY,"version":VERSION}}]}})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{change}");
    f.wait(
        &format!("/org-changes/{}", change["change_id"].as_str().unwrap()),
        |v| v["status"] == "COMMITTED",
    )
    .await;
    let (status, job) = f.post("/capability-runs", json!({"client_request_id":key,"installation_id":f.installation,
        "role_id":role,"responsibility_id":responsibility,"suite_id":suite().suite_id,"input":suite().cases[case].input})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["job_id"].as_str().unwrap().to_owned();
    f.collect(&id).await;
    let (status, proof) = f
        .post(&format!("/capability-runs/{id}/verify"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{proof}");
    assert_eq!(proof["decision"], "verified");
    (id, mission)
}

#[tokio::test]
async fn candidate_from_another_mission_is_refused_while_same_mission_control_registers() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    // Case 0 is another page of the same frozen event: its input differs from
    // the original's only in source_url.
    let same_mission = f.source("recovery-same-mission-page", 0).await;
    let (other_mission, mission) =
        second_mission_source(&f, "recovery-other-mission-page", 0).await;
    let read = |id: &str| {
        let job = f.app.store.get_job(&JobId::new(id)).unwrap();
        let run = f.app.store.get_capability_run(&JobId::new(id)).unwrap();
        (job, run)
    };
    let (original_job, original_run) = read(&source);
    let (control_job, control_run) = read(&same_mission);
    let (foreign_job, foreign_run) = read(&other_mission);
    assert_eq!(foreign_job.mission_id.as_str(), mission);
    assert_ne!(foreign_job.mission_id, original_job.mission_id);
    assert_eq!(control_job.mission_id, original_job.mission_id);
    for (job, run) in [(&control_job, &control_run), (&foreign_job, &foreign_run)] {
        assert_eq!(job.org_id, original_job.org_id);
        assert_eq!(run.request.artifact, original_run.request.artifact);
        let mut input = job.input.clone();
        input["source_url"] = original_job.input["source_url"].clone();
        assert_eq!(json_sha256(&input), json_sha256(&original_job.input));
        let proof = genesis_server::capability_invocation::read(&f.app, &job.job_id)
            .await
            .unwrap();
        let original = genesis_server::capability_invocation::read(&f.app, &original_job.job_id)
            .await
            .unwrap();
        assert_eq!(proof.installation_id, original.installation_id);
        assert_eq!(proof.suite_id, original.suite_id);
        assert_eq!(proof.suite_sha256, original.suite_sha256);
        assert_eq!(proof.acceptance_report_ref, original.acceptance_report_ref);
    }

    // The other origin dimensions cannot vary alone: the kernel refuses a second
    // live installation of this capability, and installation, suite, acceptance
    // and artifact are each fixed per installation by the invocation proof.
    let installations = f.app.current_state().unwrap().installations.len();
    let (status, duplicate) = f
        .post(
            "/capability-installations",
            json!({"capability_id":CAPABILITY,"version":VERSION}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{duplicate}");
    assert_eq!(
        f.app.current_state().unwrap().installations.len(),
        installations
    );

    let mut body = recovery_request(&source, &reopened);
    body["candidate_job_ids"] = json!([other_mission]);
    let before = f.app.current_state().unwrap();
    let (status, refused) = f.post("/venue-recoveries", body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"], "venue_recovery_source");
    assert_eq!(
        refused["message"],
        "candidate source differs from the frozen event or capability"
    );
    assert_eq!(f.app.current_state().unwrap(), before);
    let rows: u64 = sqlite(&f)
        .query_row("SELECT COUNT(*) FROM venue_recoveries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);

    // Control: the same page, verified in the original mission, registers.
    body["candidate_job_ids"] = json!([same_mission]);
    let (_, registered) = register(&f, body).await;
    assert_eq!(registered["phase"], "recruiting");
    assert!(bridge.sends().is_empty());
}
