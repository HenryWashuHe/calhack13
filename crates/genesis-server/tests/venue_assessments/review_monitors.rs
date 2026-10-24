//! Review probes for durable venue monitor subscriptions. Each test reuses the
//! parent fixture's loopback providers; failing assertions document a defect.
use super::*;

fn monitor_request(f: &Fixture, key: &str, source: &str, maximum: u32) -> Value {
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

async fn register(f: &Fixture, request: Value) -> String {
    let (status, record) = f.post("/venue-monitors", request).await;
    assert_eq!(status, StatusCode::CREATED, "{record}");
    format!("/venue-monitors/{}", record["monitor_id"].as_str().unwrap())
}

fn children(f: &Fixture) -> i64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_assessments", [], |row| {
            row.get(0)
        })
        .unwrap()
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

async fn passes(f: &Fixture, count: usize) {
    for _ in 0..count {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        genesis_server::venue_assessments::drain_once(&f.app)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn idle_baseline(v: &Value) -> bool {
    v["last_assessment_id"].is_string() && v["pending_assessment_id"].is_null()
}

/// A source that another (operator) assessment already used is discovered by
/// the event scan, charged as an intent, and then fails child creation. The
/// subscription halts permanently and later genuine updates are never assessed.
#[tokio::test]
async fn review_already_assessed_update_burns_an_intent_and_halts_the_subscription() {
    let f = Fixture::start().await;
    let baseline = f.source("review-used-baseline", 0).await;
    let path = register(&f, monitor_request(&f, "review-used", &baseline, 4)).await;
    let idle = drive(&f, &path, idle_baseline).await;
    assert_eq!(idle["status"], "active", "{idle}");

    let update = f.source("review-used-update", 3).await;
    // An operator assessment for the same responsibility and update is legal
    // under the assessment API because the monitor's child already resolved.
    let manual = f
        .create(f.request_body("review-manual-assessment", &update))
        .await;
    f.collected(&manual).await;
    f.verify(&manual).await;
    passes(&f, 6).await;
    let after = f.get(&path).await;
    // Diagnostic only: replay the saved child request to expose the refusal.
    let text: String = sqlite(&f)
        .query_row(
            "SELECT record_json FROM venue_monitors WHERE monitor_id=?1",
            [after["monitor_id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let saved: Value = serde_json::from_str(&text).unwrap();
    let refusal = match saved["intents"].get(1) {
        Some(intent) => {
            f.post("/venue-assessments", intent["request"].clone())
                .await
                .1
        }
        None => Value::Null,
    };
    assert_eq!(
        after["status"], "active",
        "an already-assessed source must be skipped, not halt the subscription: status={} attention={} assessment_count={} pending_source_job_id={} pending_assessment_id={} saved-intent refusal={refusal}",
        after["status"], after["attention"], after["assessment_count"], after["pending_source_job_id"], after["pending_assessment_id"]
    );
    assert_eq!(
        after["assessment_count"], 1,
        "an un-creatable source must not consume a bounded intent: {after}"
    );
}

/// Registration accepts an initial source that the assessment runtime will
/// always refuse (an equal-or-newer source was already assessed for the
/// responsibility). The operator gets 201 and an `active` record, but the
/// subscription can only ever fall into attention.
#[tokio::test]
async fn review_registration_accepts_an_initial_source_that_can_never_become_a_child() {
    let f = Fixture::start().await;
    let source = f.source("review-doomed-source", 0).await;
    let manual = f
        .create(f.request_body("review-doomed-manual", &source))
        .await;
    f.collected(&manual).await;
    f.verify(&manual).await;
    let (status, record) = f
        .post(
            "/venue-monitors",
            monitor_request(&f, "review-doomed", &source, 4),
        )
        .await;
    if status == StatusCode::CREATED {
        let path = format!("/venue-monitors/{}", record["monitor_id"].as_str().unwrap());
        passes(&f, 3).await;
        let after = f.get(&path).await;
        panic!(
            "registration returned 201 for a source already assessed; after one pass: status={} attention={} children={}",
            after["status"], after["attention"], children(&f)
        );
    }
    assert!(status.is_client_error(), "{record}");
}

/// Two verified updates arrive while the baseline child is unresolved. The
/// older capacity-cut snapshot is verified first; the newer snapshot shows the
/// venue feasible again. The monitor follows verification order, assesses the
/// superseded snapshot and completes with a replacement requirement although
/// newer verified evidence for the same event already exists.
#[tokio::test]
async fn review_superseded_update_verified_first_creates_replacement_work() {
    let f = Fixture::start().await;
    let baseline = f.source("review-order-baseline", 0).await;
    let path = register(&f, monitor_request(&f, "review-order", &baseline, 4)).await;
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let pending = f.get(&path).await;
    assert!(pending["pending_assessment_id"].is_string(), "{pending}");

    // Older update (capacity cut) is created first, newer (restored) second;
    // the older one finishes verification first.
    let mut ids = Vec::new();
    for (key, case) in [("review-order-older", 3), ("review-order-newer", 0)] {
        let (status, job) = f
            .post(
                "/capability-runs",
                json!({"client_request_id":key,"installation_id":f.installation,
                    "role_id":f.monitor,"responsibility_id":f.responsibility,"suite_id":suite().suite_id,
                    "input":suite().cases[case].input}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{job}");
        ids.push(job["job_id"].as_str().unwrap().to_owned());
    }
    let (older, newer) = (ids[0].clone(), ids[1].clone());
    for id in [&older, &newer] {
        f.collect(id).await;
        let (status, verified) = f
            .post(&format!("/capability-runs/{id}/verify"), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK, "{verified}");
    }
    let older_created = f.app.store.get_job(&JobId::new(&older)).unwrap().created_at;
    let newer_created = f.app.store.get_job(&JobId::new(&newer)).unwrap().created_at;
    assert!(older_created < newer_created);

    let done = drive(&f, &path, |v| {
        // Correct coalescing spends only baseline + newest source. Keep the
        // original superseded-source assertion below; do not wait for a third
        // intent that the fix must never create.
        v["status"] != "active"
            || (v["assessment_count"] == 2
                && idle_baseline(v)
                && v["pending_source_job_id"].is_null())
    })
    .await;
    let last = f
        .get(&format!(
            "/venue-assessments/{}",
            done["last_assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_ne!(
        last["source_job_id"], older,
        "monitor assessed the superseded snapshot (created {older_created}) although a newer verified snapshot (created {newer_created}) existed; monitor={done} requirements={}",
        f.side_effect_counts().1
    );
    assert_eq!(last["source_job_id"], newer);
    assert_eq!(done["assessment_count"], 2);
}

/// An idle subscription (no pending child, no candidate events) re-checks the
/// CREAO provider on every pass. One pass during a restart without the CREAO
/// runtime halts it permanently, and restoring the provider does not resume it.
#[tokio::test]
async fn review_idle_pass_during_provider_outage_permanently_halts_subscription() {
    let mut f = Fixture::start().await;
    let baseline = f.source("review-outage-baseline", 0).await;
    let path = register(&f, monitor_request(&f, "review-outage", &baseline, 4)).await;
    drive(&f, &path, idle_baseline).await;

    f.server.stop().await;
    f.config.forge_checkpoints.clear();
    f.app = build_org(
        &f.config,
        Some(ProviderSet::new().with_dev_stub(DevStub::default())),
    )
    .unwrap();
    f.server = Server::router(f.app.clone().router()).await;
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let during = f.get(&path).await;
    assert_eq!(children(&f), 1, "no work was prepared during the outage");

    f.restart(false).await;
    let update = f.source("review-outage-update", 3).await;
    passes(&f, 6).await;
    let after = f.get(&path).await;
    assert_eq!(
        after["pending_source_job_id"], update,
        "an idle pass with no candidate work halted the subscription: during-outage={during} after={after}"
    );
}

/// Two AppState instances over one database drain concurrently; exactly one
/// child must be created and the shared record must stay readable.
#[tokio::test]
async fn review_two_app_states_create_one_child() {
    let f = Fixture::start().await;
    let baseline = f.source("review-two-apps", 0).await;
    let path = register(&f, monitor_request(&f, "review-two-apps", &baseline, 4)).await;
    let (providers, _specialist, _journal) = f.providers.configure(f.directory.path());
    let mut config = f.config.clone();
    config.forge_checkpoints.clear();
    let other = build_org(&config, Some(providers)).unwrap();
    let (left, right) = tokio::join!(
        genesis_server::venue_monitors::drain_once(&f.app),
        genesis_server::venue_monitors::drain_once(&other)
    );
    let _ = (left, right);
    passes(&f, 2).await;
    let record = f.get(&path).await;
    assert_eq!(children(&f), 1, "{record}");
    assert!(record["pending_assessment_id"].is_string(), "{record}");
}

/// Authority changes while idle (installation no longer accepted, capability
/// detached, agent rebound, lease expired) must stop intake before a child.
#[tokio::test]
async fn review_idle_authority_changes_fence_new_children() {
    for mutation in ["installation", "detach", "binding", "lease"] {
        let f = Fixture::start().await;
        let baseline = f.source("review-fence-baseline", 0).await;
        let path = register(&f, monitor_request(&f, "review-fence", &baseline, 4)).await;
        drive(&f, &path, idle_baseline).await;
        f.source("review-fence-update", 3).await;
        let installation = InstallationId::new(&f.installation);
        let monitor = RoleId::new(&f.monitor);
        let responsibility = ResponsibilityId::new(&f.responsibility);
        f.mutate(|state| match mutation {
            "installation" => {
                state
                    .installations
                    .get_mut(&installation)
                    .unwrap()
                    .acceptance_report_ref = Some("other-report".into());
            }
            "detach" => {
                state.roles.get_mut(&monitor).unwrap().capabilities.clear();
            }
            "binding" => {
                state
                    .roles
                    .get_mut(&monitor)
                    .unwrap()
                    .binding
                    .as_mut()
                    .unwrap()
                    .config_digest = "c".repeat(64);
            }
            "lease" => {
                let id = state
                    .active_assignment(&responsibility)
                    .unwrap()
                    .assignment_id
                    .clone();
                state.assignments.get_mut(&id).unwrap().lease_expires_at =
                    Some(Utc::now() - ChronoDuration::seconds(1));
            }
            _ => unreachable!(),
        })
        .await;
        passes(&f, 4).await;
        let after = f.get(&path).await;
        assert_eq!(children(&f), 1, "{mutation}: {after}");
        assert_ne!(after["status"], "active", "{mutation}: {after}");
        assert_eq!(after["assessment_count"], 1, "{mutation}: {after}");
    }
}

/// max_assessments=2 permits exactly the initial source plus one update.
#[tokio::test]
async fn review_limit_two_allows_exactly_one_update() {
    let f = Fixture::start().await;
    let baseline = f.source("review-limit-baseline", 0).await;
    let path = register(&f, monitor_request(&f, "review-limit", &baseline, 2)).await;
    drive(&f, &path, idle_baseline).await;
    let second = f.source("review-limit-second", 0).await;
    let done = drive(&f, &path, |v| v["status"] == "completed").await;
    assert_eq!(done["attention"], "assessment_limit_reached", "{done}");
    assert_eq!(done["assessment_count"], 2);
    let last = f
        .get(&format!(
            "/venue-assessments/{}",
            done["last_assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(last["source_job_id"], second);
    f.source("review-limit-third", 3).await;
    passes(&f, 3).await;
    assert_eq!(children(&f), 2);
    assert_eq!(f.side_effect_counts().1, 0, "no replacement requirement");
}

/// Expiry while idle stops intake; a later verified update creates nothing.
#[tokio::test]
async fn review_expiry_boundary_stops_intake() {
    let f = Fixture::start().await;
    let baseline = f.source("review-expiry-baseline", 0).await;
    let mut request = monitor_request(&f, "review-expiry", &baseline, 4);
    request["expires_at"] = json!(Utc::now() + ChronoDuration::seconds(4));
    let path = register(&f, request).await;
    drive(&f, &path, idle_baseline).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    f.source("review-expiry-update", 3).await;
    passes(&f, 3).await;
    let after = f.get(&path).await;
    assert_eq!(after["status"], "stopped", "{after}");
    assert_eq!(after["attention"], "expired", "{after}");
    assert_eq!(children(&f), 1);
}
