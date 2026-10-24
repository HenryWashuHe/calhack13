//! Mutation-guided pins for venue monitor subscriptions. Each test drives the
//! parent fixture's loopback providers and real invocation/assessment proofs;
//! no live provider, page freshness or sponsor evidence is claimed.
use super::*;

fn request(f: &Fixture, key: &str, source: &str, maximum: u32) -> Value {
    json!({
        "client_request_id": key,
        "role_id": f.monitor,
        "responsibility_id": f.responsibility,
        "source_job_id": source,
        "policy": f.request_body("unused-policy-shape", source)["policy"],
        "expires_at": Utc::now() + ChronoDuration::hours(1),
        "max_assessments": maximum,
    })
}

async fn register(f: &Fixture, body: Value) -> String {
    let (status, record) = f.post("/venue-monitors", body).await;
    assert_eq!(status, StatusCode::CREATED, "{record}");
    assert_eq!(record["status"], "active", "{record}");
    format!("/venue-monitors/{}", record["monitor_id"].as_str().unwrap())
}

fn stored(f: &Fixture) -> Value {
    let text: String = sqlite(f)
        .query_row("SELECT record_json FROM venue_monitors", [], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn children(f: &Fixture) -> i64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_assessments", [], |r| r.get(0))
        .unwrap()
}

fn watermark(f: &Fixture) -> u64 {
    sqlite(f)
        .query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))
        .unwrap()
}

fn assessment(id: &Value) -> String {
    format!("/venue-assessments/{}", id.as_str().unwrap())
}

async fn monitors(f: &Fixture, passes: usize) {
    for _ in 0..passes {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
}

async fn drive(f: &Fixture, path: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let mut last = Value::Null;
    for _ in 0..400 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        genesis_server::venue_assessments::drain_once(&f.app)
            .await
            .unwrap();
        last = f.get(path).await;
        if predicate(&last) {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("bounded drive failed at {path}: {last}");
}

fn idle(v: &Value) -> bool {
    v["last_assessment_id"].is_string() && v["pending_source_job_id"].is_null()
}

fn without_creao(f: &mut Fixture) {
    f.config.forge_checkpoints.clear();
    f.app = build_org(
        &f.config,
        Some(ProviderSet::new().with_dev_stub(DevStub::default())),
    )
    .unwrap();
}

fn set_scope(f: &Fixture, state: &mut OrgState, present: bool) {
    let scopes = &mut state
        .roles
        .get_mut(&RoleId::new(&f.monitor))
        .unwrap()
        .authority
        .scopes;
    if present {
        scopes.insert("monitor.venue".into());
    } else {
        scopes.remove("monitor.venue");
    }
}

/// Registration refuses a responsibility whose assessment cycle is still
/// unresolved; the same request succeeds once that cycle resolves.
#[tokio::test]
async fn registration_refuses_an_unresolved_manual_cycle_then_accepts_after_it_resolves() {
    let f = Fixture::start().await;
    let older = f.source("busy-registration-older", 0).await;
    let baseline = f.source("busy-registration-baseline", 0).await;
    let manual = f
        .create(f.request_body("busy-registration-manual", &older))
        .await;
    f.collected(&manual).await;
    let body = request(&f, "busy-registration", &baseline, 4);
    let (status, refused) = f.post("/venue-monitors", body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    let rows: i64 = sqlite(&f)
        .query_row("SELECT COUNT(*) FROM venue_monitors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "a refused registration persists nothing");
    // Positive control: the baseline is newer than the manual source, so only
    // the unresolved cycle caused the refusal.
    f.verify(&manual).await;
    let path = register(&f, body).await;
    let done = drive(&f, &path, idle).await;
    assert_eq!(done["status"], "active", "{done}");
    let child = f.get(&assessment(&done["last_assessment_id"])).await;
    assert_eq!(child["source_job_id"], baseline);
}

/// Only a native CREAO binding can own a subscription.
#[tokio::test]
async fn registration_requires_a_native_creao_binding() {
    let f = Fixture::start().await;
    let source = f.source("non-creao-source", 0).await;
    let monitor = RoleId::new(&f.monitor);
    f.mutate(|state| {
        state
            .roles
            .get_mut(&monitor)
            .unwrap()
            .binding
            .as_mut()
            .unwrap()
            .provider = Provider::Elastic;
    })
    .await;
    let body = request(&f, "non-creao-monitor", &source, 4);
    let (status, refused) = f.post("/venue-monitors", body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    f.mutate(|state| {
        state.roles.get_mut(&monitor).unwrap().binding = Some(binding());
    })
    .await;
    register(&f, body).await;
}

/// The saved intent deadline is the earliest of expiry and source age; a
/// stop request cannot rewrite an already completed subscription.
#[tokio::test]
async fn intent_deadline_follows_source_age_and_stop_keeps_completion() {
    let f = Fixture::start().await;
    let source = f.source("deadline-source", 0).await;
    let path = register(&f, request(&f, "deadline-monitor", &source, 1)).await;
    let job = f.app.store.get_job(&JobId::new(&source)).unwrap();
    let deadline: chrono::DateTime<Utc> =
        serde_json::from_value(stored(&f)["intents"][0]["request"]["deadline"].clone()).unwrap();
    assert_eq!(
        deadline,
        job.created_at + ChronoDuration::minutes(30),
        "an hour-long subscription cannot extend the source creation window"
    );
    let completed = drive(&f, &path, |v| v["status"] == "completed").await;
    assert_eq!(completed["attention"], "assessment_limit_reached");
    let (status, after) = f.post(&format!("{path}/stop"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after, completed, "stop is a no-op after completion");
    assert_eq!(f.get(&path).await, completed);

    let f = Fixture::start().await;
    let source = f.source("short-expiry-source", 0).await;
    let mut body = request(&f, "short-expiry-monitor", &source, 1);
    let expires_at = Utc::now() + ChronoDuration::minutes(5);
    body["expires_at"] = json!(expires_at);
    register(&f, body).await;
    let deadline: chrono::DateTime<Utc> =
        serde_json::from_value(stored(&f)["intents"][0]["request"]["deadline"].clone()).unwrap();
    assert_eq!(deadline, expires_at);
}

/// A stopped subscription stays stopped (and closed) when its retained child
/// later needs attention; observing that child cannot reopen intake.
#[tokio::test]
async fn stopped_subscription_stays_stopped_when_its_retained_child_becomes_unknown() {
    let f = Fixture::start().await;
    f.providers.mode(ResponseMode::Ambiguous);
    let source = f.source("stopped-unknown-source", 0).await;
    let path = register(&f, request(&f, "stopped-unknown", &source, 4)).await;
    monitors(&f, 1).await;
    let pending = f.get(&path).await;
    let child_id = pending["pending_assessment_id"].clone();
    let child = f.get(&assessment(&child_id)).await;
    let (status, stopped) = f.post(&format!("{path}/stop"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["status"], "stopped");
    f.wait(
        &format!("/jobs/{}", child["job_id"].as_str().unwrap()),
        |v| v["status"] == "UNKNOWN",
    )
    .await;
    monitors(&f, 2).await;
    let after = f.get(&path).await;
    assert_eq!(after["status"], "stopped", "{after}");
    assert_eq!(after["pending_assessment_id"], child_id);
    let open: i64 = sqlite(&f)
        .query_row(
            "SELECT COUNT(*) FROM venue_monitors WHERE open=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 0, "a stopped record must not occupy an open slot");
    assert_eq!(f.providers.execution_posts().len(), 1);
}

/// A rejected child retains attention and its identity; it is never reported
/// as a monitoring outcome and never releases a later source.
#[tokio::test]
async fn rejected_child_retains_attention_and_blocks_later_sources() {
    let f = Fixture::start().await;
    f.providers.mode(ResponseMode::WrongCapacity);
    let source = f.source("rejected-child-source", 0).await;
    let path = register(&f, request(&f, "rejected-child", &source, 4)).await;
    let effects = f.side_effect_counts();
    let attention = drive(&f, &path, |v| v["status"] != "active").await;
    assert_eq!(attention["status"], "attention", "{attention}");
    assert_eq!(attention["attention"], "assessment_needs_attention");
    let child_id = attention["pending_assessment_id"].clone();
    let child = f.get(&assessment(&child_id)).await;
    assert_eq!(child["status"], "RESULT_REJECTED", "{child}");
    assert!(attention["last_assessment_id"].is_null());
    f.providers.mode(ResponseMode::Correct);
    f.source("rejected-child-later", 3).await;
    monitors(&f, 4).await;
    let after = f.get(&path).await;
    assert_eq!(after["status"], "attention");
    assert_eq!(after["pending_assessment_id"], child_id);
    assert_eq!(after["assessment_count"], 1);
    assert_eq!(children(&f), 1);
    assert_eq!(f.side_effect_counts(), effects);
}

/// Exhausted local verification puts the subscription in attention under the
/// same child; fewer attempts do not. Operator verification after repair lets
/// the same child be accepted and monitoring continue.
#[tokio::test]
async fn exhausted_child_verification_needs_attention_then_same_child_can_resume() {
    let mut f = Fixture::start().await;
    let source = f.source("exhausted-source", 0).await;
    let path = register(&f, request(&f, "exhausted-monitor", &source, 4)).await;
    monitors(&f, 1).await;
    let child_id = f.get(&path).await["pending_assessment_id"].clone();
    let child = f.get(&assessment(&child_id)).await;
    f.collected(&child).await;
    f.server.stop().await;
    without_creao(&mut f);
    f.server = Server::router(f.app.clone().router()).await;
    for attempt in 1..=3u64 {
        genesis_server::venue_assessments::drain_once(&f.app)
            .await
            .unwrap();
        monitors(&f, 1).await;
        let record = f.get(&assessment(&child_id)).await;
        assert_eq!(record["finalization_attempts"], attempt, "{record}");
        assert!(record["proof"].is_null());
        let monitor = f.get(&path).await;
        assert_eq!(monitor["pending_assessment_id"], child_id);
        if attempt < 3 {
            assert_eq!(monitor["status"], "active", "attempt {attempt}: {monitor}");
        } else {
            assert_eq!(monitor["status"], "attention", "{monitor}");
            assert_eq!(
                monitor["attention"],
                "assessment_verification_needs_attention"
            );
        }
    }
    f.restart(false).await;
    let verified = f.verify(&child).await;
    assert_eq!(proof(&verified)["decision"], "accepted");
    monitors(&f, 1).await;
    let resumed = f.get(&path).await;
    assert_eq!(resumed["status"], "active", "{resumed}");
    assert!(resumed["attention"].is_null());
    assert_eq!(resumed["last_assessment_id"], child_id);
    assert!(resumed["pending_source_job_id"].is_null());
    assert_eq!(f.providers.execution_posts().len(), 1);
}

/// An uncreated intent waits behind an unrelated unresolved manual cycle and is
/// created once that cycle resolves, without spending another intent.
#[tokio::test]
async fn uncreated_intent_waits_behind_unresolved_manual_cycle_then_creates_once() {
    let f = Fixture::start().await;
    let older = f.source("waiting-intent-older", 0).await;
    let baseline = f.source("waiting-intent-baseline", 0).await;
    let path = register(&f, request(&f, "waiting-intent", &baseline, 4)).await;
    let original = stored(&f)["intents"][0].clone();
    let manual = f
        .create(f.request_body("waiting-intent-manual", &older))
        .await;
    f.collected(&manual).await;
    monitors(&f, 4).await;
    let waiting = f.get(&path).await;
    assert_eq!(waiting["status"], "active", "{waiting}");
    assert_eq!(waiting["pending_source_job_id"], baseline);
    assert!(waiting["pending_assessment_id"].is_null());
    assert_eq!(stored(&f)["intents"][0], original);
    assert_eq!(children(&f), 1, "only the manual child exists");
    f.verify(&manual).await;
    let done = drive(&f, &path, idle).await;
    assert_eq!(done["status"], "active", "{done}");
    assert_eq!(done["assessment_count"], 1);
    let child = f.get(&assessment(&done["last_assessment_id"])).await;
    assert_eq!(child["source_job_id"], baseline);
    assert_eq!(stored(&f)["intents"][0]["request"], original["request"]);
    assert_eq!(children(&f), 2);
}

/// After a preparation failure puts the record in attention, repairing the
/// authority alone neither creates the saved child nor scans for new work.
#[tokio::test]
async fn repaired_authority_does_not_resume_a_subscription_in_attention() {
    for phase in ["uncreated", "idle"] {
        let f = Fixture::start().await;
        let baseline = f.source("repair-baseline", 0).await;
        let path = register(&f, request(&f, "repair-monitor", &baseline, 4)).await;
        if phase == "idle" {
            drive(&f, &path, idle).await;
        }
        let before = children(&f);
        f.mutate(|state| set_scope(&f, state, false)).await;
        monitors(&f, 1).await;
        let fenced = f.get(&path).await;
        assert_eq!(fenced["status"], "attention", "{phase}: {fenced}");
        f.mutate(|state| set_scope(&f, state, true)).await;
        if phase == "idle" {
            f.source("repair-later-source", 3).await;
        }
        monitors(&f, 4).await;
        let after = f.get(&path).await;
        assert_eq!(after["status"], "attention", "{phase}: {after}");
        assert_eq!(after["assessment_count"], 1, "{phase}: {after}");
        assert!(after["pending_assessment_id"].is_null(), "{phase}: {after}");
        assert_eq!(children(&f), before, "{phase}");
        if phase == "uncreated" {
            assert_eq!(after["pending_source_job_id"], baseline);
        } else {
            assert!(after["pending_source_job_id"].is_null(), "{after}");
        }
    }
}

/// While delivery is blocked, a saved candidate is reconsidered against events
/// that arrive after its captured watermark; the scan never reads past that
/// watermark before reaching it.
#[tokio::test]
async fn blocked_scan_extends_its_watermark_before_spending_an_older_candidate() {
    let f = Fixture::start().await;
    let baseline = f.source("extend-baseline", 0).await;
    let path = register(&f, request(&f, "extend-monitor", &baseline, 4)).await;
    drive(&f, &path, idle).await;
    let manual_source = f.source("extend-manual-source", 0).await;
    let manual = f
        .create(f.request_body("extend-manual", &manual_source))
        .await;
    f.collected(&manual).await;
    let older = f.source("extend-older-cut", 3).await;
    // Make the captured backlog span pages and end mid-page, so the page that
    // reaches the watermark also contains events created after it.
    let cursor = stored(&f)["cursor"].as_u64().unwrap();
    while watermark(&f) - cursor < 20 || (watermark(&f) - cursor).is_multiple_of(16) {
        let (status, body) = f
            .post(
                &format!("/missions/{}/observations", f.mission),
                json!({"source":"extend-fixture","summary":"Unrelated event"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    monitors(&f, 1).await;
    let captured = stored(&f)["scan"]["through"].as_u64().unwrap();
    assert!(stored(&f)["cursor"].as_u64().unwrap() < captured);
    let newer = f.source("extend-newer-feasible", 0).await;
    let effects = f.side_effect_counts();
    let mut reached = false;
    for _ in 0..16 {
        monitors(&f, 1).await;
        let saved = stored(&f);
        assert_eq!(saved["status"], "active", "{saved}");
        assert_eq!(saved["intents"].as_array().unwrap().len(), 1);
        if saved["scan"]["candidate"]["source_job_id"] == newer {
            reached = true;
            break;
        }
    }
    assert!(reached, "blocked scan never considered the newer source");
    f.verify(&manual).await;
    let done = drive(&f, &path, |v| v["assessment_count"] == 2 && idle(v)).await;
    assert_eq!(done["status"], "active", "{done}");
    let last = f.get(&assessment(&done["last_assessment_id"])).await;
    assert_ne!(last["source_job_id"], older);
    assert_eq!(last["source_job_id"], newer);
    assert_eq!(proof(&last)["output"]["verdict"], "feasible");
    assert_eq!(f.side_effect_counts().1, effects.1, "no replacement work");
}

/// A second responsibility in the same mission, held by the same monitor role,
/// produces genuinely verified invocations of the same event. They can neither
/// seed nor feed this responsibility's subscription.
#[tokio::test]
async fn other_responsibility_sources_neither_register_nor_feed_the_subscription() {
    let f = Fixture::start().await;
    let first = ResponsibilityId::new(&f.responsibility);
    let other = ResponsibilityId::generate();
    f.mutate(|state| {
        let mut responsibility = state.responsibilities[&first].clone();
        responsibility.responsibility_id = other.clone();
        state.responsibilities.insert(other.clone(), responsibility);
        let mut assignment = state.active_assignment(&first).unwrap().clone();
        assignment.assignment_id = AssignmentId::generate();
        assignment.responsibility_id = other.clone();
        state
            .assignments
            .insert(assignment.assignment_id.clone(), assignment);
    })
    .await;
    let other_source = |key: &'static str| {
        let f = &f;
        let other = other.clone();
        async move {
            let (status, job) = f
                .post(
                    "/capability-runs",
                    json!({"client_request_id":key,"installation_id":f.installation,
                        "role_id":f.monitor,"responsibility_id":other,"suite_id":suite().suite_id,
                        "input":suite().cases[0].input}),
                )
                .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{job}");
            let id = job["job_id"].as_str().unwrap().to_owned();
            f.collect(&id).await;
            let (status, verified) = f
                .post(&format!("/capability-runs/{id}/verify"), json!({}))
                .await;
            assert_eq!(status, StatusCode::OK, "{verified}");
            assert_eq!(verified["decision"], "verified");
            id
        }
    };
    let foreign = other_source("other-responsibility-seed").await;
    let (status, refused) = f
        .post(
            "/venue-monitors",
            request(&f, "foreign-seed-monitor", &foreign, 4),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    let baseline = f.source("own-responsibility-baseline", 0).await;
    let path = register(&f, request(&f, "own-responsibility-monitor", &baseline, 4)).await;
    drive(&f, &path, idle).await;
    let later = other_source("other-responsibility-later").await;
    monitors(&f, 4).await;
    let after = f.get(&path).await;
    assert_eq!(after["status"], "active", "{after}");
    assert_eq!(after["assessment_count"], 1);
    assert!(after["pending_source_job_id"].is_null(), "{after}");
    assert_ne!(stored(&f)["scan"]["candidate"]["source_job_id"], later);
    assert_eq!(children(&f), 1);
}

/// Additional frozen-authority fences checked before any new child: the
/// installation must still be accepted and the frozen assignment must remain
/// the active one, even when the epoch is unchanged.
#[tokio::test]
async fn idle_installation_status_and_assignment_identity_fence_new_children() {
    for mutation in ["installation_rejected", "assignment_rotated"] {
        let f = Fixture::start().await;
        let baseline = f.source("identity-fence-baseline", 0).await;
        let path = register(&f, request(&f, "identity-fence", &baseline, 4)).await;
        drive(&f, &path, idle).await;
        f.source("identity-fence-update", 3).await;
        let installation = InstallationId::new(&f.installation);
        let responsibility = ResponsibilityId::new(&f.responsibility);
        f.mutate(|state| match mutation {
            "installation_rejected" => {
                state.installations.get_mut(&installation).unwrap().status =
                    genesis_core::lifecycle::InstallationStatus::Rejected;
            }
            "assignment_rotated" => {
                let old = state.active_assignment(&responsibility).unwrap().clone();
                let mut rotated = old.clone();
                rotated.assignment_id = AssignmentId::generate();
                state.assignments.remove(&old.assignment_id);
                state
                    .assignments
                    .insert(rotated.assignment_id.clone(), rotated);
            }
            _ => unreachable!(),
        })
        .await;
        monitors(&f, 4).await;
        let after = f.get(&path).await;
        assert_ne!(after["status"], "active", "{mutation}: {after}");
        assert_eq!(after["assessment_count"], 1, "{mutation}: {after}");
        assert_eq!(children(&f), 1, "{mutation}");
        assert_eq!(f.providers.execution_posts().len(), 1, "{mutation}");
    }
}

/// While CREAO is absent but invocations continue, the subscription keeps its
/// scan instead of reserving an intent, so a newer source that arrives during
/// the outage wins once CREAO is restored.
#[tokio::test]
async fn creao_absence_defers_reservation_so_a_newer_source_wins_after_restore() {
    let mut f = Fixture::start().await;
    let baseline = f.source("outage-scan-baseline", 0).await;
    let path = register(&f, request(&f, "outage-scan", &baseline, 4)).await;
    drive(&f, &path, idle).await;
    f.server.stop().await;
    f.config.forge_checkpoints.clear();
    let apify = ApifyClient::for_local_testing(
        "synthetic-apify-token",
        RunLimits {
            request_timeout: Duration::from_secs(2),
            deadline: Duration::from_secs(4),
            poll_interval: Duration::from_millis(1),
            ..Default::default()
        },
        &f.providers.server.url,
    )
    .unwrap();
    f.app = build_org(
        &f.config,
        Some(
            ProviderSet::new()
                .with_dev_stub(DevStub::default())
                .with_executor(Arc::new(ApifyCapabilityExecutor::new(apify))),
        ),
    )
    .unwrap();
    f.server = Server::router(f.app.clone().router()).await;
    let older = f.source("outage-scan-older-cut", 3).await;
    monitors(&f, 3).await;
    let waiting = f.get(&path).await;
    assert_eq!(waiting["status"], "active", "{waiting}");
    assert_eq!(
        waiting["assessment_count"], 1,
        "no intent is reserved without CREAO"
    );
    assert!(waiting["pending_source_job_id"].is_null(), "{waiting}");
    assert_eq!(stored(&f)["scan"]["candidate"]["source_job_id"], older);
    let newer = f.source("outage-scan-newer-feasible", 0).await;
    monitors(&f, 3).await;
    assert_eq!(stored(&f)["scan"]["candidate"]["source_job_id"], newer);
    assert_eq!(f.get(&path).await["assessment_count"], 1);
    f.restart(false).await;
    let done = drive(&f, &path, |v| v["assessment_count"] == 2 && idle(v)).await;
    assert_eq!(done["status"], "active", "{done}");
    let last = f.get(&assessment(&done["last_assessment_id"])).await;
    assert_eq!(last["source_job_id"], newer);
    assert_eq!(proof(&last)["output"]["verdict"], "feasible");
    assert_eq!(children(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);
}
