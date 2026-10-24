//! Stopping retires an uncreated intent only after historical child lookup.
//! These probes use public monitor routes and the existing loopback fixture;
//! journal reads assert durability without manufacturing monitor or child state.
use super::*;

fn request(f: &Fixture, key: &str, source: &str) -> Value {
    json!({
        "client_request_id": key,
        "role_id": f.monitor,
        "responsibility_id": f.responsibility,
        "source_job_id": source,
        "policy": f.request_body("unused-retirement-policy", source)["policy"],
        "expires_at": Utc::now() + ChronoDuration::hours(1),
        "max_assessments": 1,
    })
}

async fn register(f: &Fixture, body: Value) -> (String, Value) {
    let (status, monitor) = f.post("/venue-monitors", body).await;
    assert_eq!(status, StatusCode::CREATED, "{monitor}");
    assert_eq!(monitor["status"], "active");
    assert_eq!(monitor["assessment_count"], 1);
    assert!(monitor["pending_assessment_id"].is_null());
    let path = format!(
        "/venue-monitors/{}",
        monitor["monitor_id"].as_str().unwrap()
    );
    (path, monitor)
}

async fn stop(f: &Fixture, path: &str) -> Value {
    let (status, monitor) = f.post(&format!("{path}/stop"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{monitor}");
    assert_eq!(monitor["status"], "stopped");
    monitor
}

fn stored(f: &Fixture, monitor: &Value) -> Value {
    let text: String = sqlite(f)
        .query_row(
            "SELECT record_json FROM venue_monitors WHERE monitor_id=?1",
            [monitor["monitor_id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn children(f: &Fixture) -> i64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_assessments", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[tokio::test]
async fn stopped_uncreated_intent_settles_once_and_stays_retired_across_restart() {
    let mut f = Fixture::start().await;
    let source = f.source("retirement-uncreated-source", 0).await;
    let body = request(&f, "retirement-uncreated", &source);
    let (path, registered) = register(&f, body.clone()).await;
    let original = stored(&f, &registered);
    let exact_child_request = original["intents"][0]["request"].clone();
    let effects = f.side_effect_counts();
    assert_eq!(registered["pending_source_job_id"], source);
    assert_eq!(children(&f), 0);
    assert!(f.providers.execution_posts().is_empty());
    let stopped = stop(&f, &path).await;
    assert_eq!(stopped["pending_source_job_id"], source);

    // Startup may occur between the operator stop and the first cleanup pass.
    // The fixture resumes dispatch/polling only; monitor drains stay explicit.
    f.restart(false).await;
    assert_eq!(
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap(),
        1
    );
    let settled = f.get(&path).await;
    assert_eq!(settled["status"], "stopped");
    assert_eq!(settled["attention"], "stopped_by_operator");
    assert!(
        settled["pending_source_job_id"].is_null(),
        "historical lookup proved there was no child; a stopped intent must leave the worker queue: {settled}"
    );
    assert!(settled["pending_assessment_id"].is_null());
    assert!(settled["last_assessment_id"].is_null());
    assert!(settled["requirement_id"].is_null());
    assert_eq!(
        settled["assessment_count"], 1,
        "the reserved intent stays charged"
    );
    assert_eq!(settled["request"], registered["request"]);
    let persisted = stored(&f, &settled);
    assert_eq!(persisted["binding"], original["binding"]);
    assert_eq!(persisted["binding_sha256"], original["binding_sha256"]);
    assert_eq!(persisted["intents"][0]["request"], exact_child_request);
    assert_eq!(persisted["intents"][0]["finished"], true);
    assert_eq!(persisted["intents"][0]["skipped"], "monitor_stopped");
    assert!(persisted["intents"][0]["assessment_id"].is_null());
    assert_eq!(children(&f), 0);
    assert!(f.providers.execution_posts().is_empty());
    assert!(f.journal.list().unwrap().is_empty());
    assert_eq!(f.side_effect_counts(), effects);

    f.restart(false).await;
    for _ in 0..3 {
        assert_eq!(
            genesis_server::venue_monitors::drain_once(&f.app)
                .await
                .unwrap(),
            0,
            "a retired uncreated intent must not be selected again after restart"
        );
    }
    assert_eq!(f.get(&path).await, settled);
    let (status, replay) = f.post("/venue-monitors", body).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(
        replay, settled,
        "replay cannot renew the request or its count"
    );
    assert_eq!(stop(&f, &path).await, settled);
    assert_eq!(children(&f), 0);
    assert!(f.providers.execution_posts().is_empty());
    assert_eq!(f.side_effect_counts(), effects);
}

#[tokio::test]
async fn one_cleanup_pass_removes_sixteen_stopped_intents_from_the_worker_batch() {
    let f = Fixture::start().await;
    let source = f.source("retirement-batch-source", 0).await;
    let mut stopped = Vec::new();
    // Each stop frees the responsibility's open slot. No child has consumed
    // this source, so all registrations are real legal public API requests.
    for index in 0..16 {
        let (path, registered) = register(
            &f,
            request(&f, &format!("retirement-batch-{index}"), &source),
        )
        .await;
        stop(&f, &path).await;
        stopped.push((path, registered));
    }
    let (healthy_path, healthy) = register(&f, request(&f, "retirement-healthy", &source)).await;
    assert_eq!(children(&f), 0);
    assert_eq!(
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap(),
        16,
        "the first bounded pass selects the sixteen older stopped records"
    );
    assert_eq!(f.get(&healthy_path).await, healthy);
    assert_eq!(children(&f), 0);
    assert!(f.providers.execution_posts().is_empty());

    // checked_at rotation already lets the healthy monitor advance; the
    // regression is old stopped records continually taking the other slots.
    let selected = genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let progressing = f.get(&healthy_path).await;
    assert!(
        progressing["pending_assessment_id"].is_string(),
        "{progressing}"
    );
    assert_eq!(progressing["status"], "active");
    assert_eq!(progressing["assessment_count"], 1);
    assert_eq!(children(&f), 1);
    assert_eq!(
        selected, 1,
        "cleaned stopped records must no longer occupy the bounded worker batch"
    );
    for (path, registered) in stopped {
        let after = f.get(&path).await;
        assert_eq!(after["status"], "stopped");
        assert_eq!(after["assessment_count"], 1);
        assert_eq!(after["request"], registered["request"]);
        assert!(after["pending_source_job_id"].is_null(), "{after}");
        assert!(after["pending_assessment_id"].is_null());
        assert!(after["last_assessment_id"].is_null());
    }
    let child = f
        .get(&format!(
            "/venue-assessments/{}",
            progressing["pending_assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(child["source_job_id"], source);
    f.collected(&child).await;
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn stopped_child_waits_for_missing_provider_instead_of_retiring_its_intent() {
    let mut f = Fixture::start().await;
    let source = f.source("retirement-existing-source", 0).await;
    let (path, registered) = register(&f, request(&f, "retirement-existing", &source)).await;
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let pending = f.get(&path).await;
    let child_id = pending["pending_assessment_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let child = f.get(&format!("/venue-assessments/{child_id}")).await;
    f.collected(&child).await;
    let accepted = f.verify(&child).await;
    assert_eq!(proof(&accepted)["decision"], "accepted");
    // The assessment is accepted, but the monitor has not observed it yet.
    // Reconstructing this saved proof requires its original CREAO receipt.
    stop(&f, &path).await;
    let original = stored(&f, &registered);
    let effects = f.side_effect_counts();
    assert_eq!(f.providers.execution_posts().len(), 1);

    f.server.stop().await;
    f.config.forge_checkpoints.clear();
    f.app = build_org(
        &f.config,
        Some(ProviderSet::new().with_dev_stub(DevStub::default())),
    )
    .unwrap();
    f.server = Server::router(f.app.clone().router()).await;
    let (status, unavailable_child) = f
        .request(
            Method::GET,
            &format!("/venue-assessments/{child_id}"),
            "operator",
            None,
        )
        .await;
    assert!(status.is_server_error(), "{unavailable_child}");
    assert_eq!(unavailable_child["error"], "provider_configuration");
    for _ in 0..2 {
        assert_eq!(
            genesis_server::venue_monitors::drain_once(&f.app)
                .await
                .unwrap(),
            1,
            "unavailable receipt reconstruction is not proof that no child exists"
        );
    }
    let unavailable = f.get(&path).await;
    assert_eq!(unavailable["status"], "stopped");
    assert_eq!(unavailable["assessment_count"], 1);
    assert_eq!(unavailable["pending_source_job_id"], source);
    assert_eq!(unavailable["pending_assessment_id"], child_id);
    let preserved = stored(&f, &unavailable);
    assert_eq!(preserved["intents"], original["intents"]);
    assert_eq!(preserved["intents"][0]["finished"], false);
    assert!(preserved["intents"][0]["skipped"].is_null());
    assert_eq!(children(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(f.side_effect_counts(), effects);

    f.restart(false).await;
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let restored = f.get(&path).await;
    assert_eq!(restored["status"], "stopped");
    assert_eq!(restored["assessment_count"], 1);
    assert!(restored["pending_source_job_id"].is_null());
    assert!(restored["pending_assessment_id"].is_null());
    assert_eq!(restored["last_assessment_id"], child_id);
    let settled = stored(&f, &restored);
    assert_eq!(
        settled["intents"][0]["request"],
        original["intents"][0]["request"]
    );
    assert_eq!(settled["intents"][0]["assessment_id"], child_id);
    assert_eq!(settled["intents"][0]["finished"], true);
    assert!(settled["intents"][0]["skipped"].is_null());
    assert_eq!(
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap(),
        0
    );
    assert_eq!(children(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(f.side_effect_counts(), effects);
}
