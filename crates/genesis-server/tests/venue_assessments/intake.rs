//! Intake regressions use genuine loopback invocation and assessment proofs.
//! The suite fixes snapshot facts in advance; no live freshness is claimed.
use super::*;

async fn register(f: &Fixture, source: &str, key: &str) -> String {
    let (status, record) = f
        .post(
            "/venue-monitors",
            json!({
                "client_request_id":key,"role_id":f.monitor,"responsibility_id":f.responsibility,
                "source_job_id":source,"policy":f.request_body("policy",source)["policy"],
                "expires_at":Utc::now()+ChronoDuration::hours(1),"max_assessments":4,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{record}");
    format!("/venue-monitors/{}", record["monitor_id"].as_str().unwrap())
}
fn saved(f: &Fixture) -> Value {
    let text: String = sqlite(f)
        .query_row("SELECT record_json FROM venue_monitors", [], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&text).unwrap()
}
fn children(f: &Fixture) -> u64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_assessments", [], |r| r.get(0))
        .unwrap()
}
async fn drain(f: &Fixture, path: &str, complete: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..500 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        genesis_server::venue_assessments::drain_once(&f.app)
            .await
            .unwrap();
        let record = f.get(path).await;
        if complete(&record) {
            return record;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("bounded intake wait: {}", f.get(path).await);
}
fn idle(record: &Value) -> bool {
    record["last_assessment_id"].is_string() && record["pending_source_job_id"].is_null()
}

#[tokio::test]
async fn newest_backlog_candidate_survives_pages_and_restart_without_spending_the_stale_intent() {
    let mut f = Fixture::start().await;
    let baseline = f.source("page-baseline", 0).await;
    let path = register(&f, &baseline, "page-monitor").await;
    drain(&f, &path, idle).await;
    let old = f.source("page-older-cut", 3).await;
    for index in 0..205 {
        let (status, body) = f
            .post(
                &format!("/missions/{}/observations", f.mission),
                json!({
                    "source":"intake-pagination","summary":format!("Unrelated event {index}")
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let newest = f.source("page-newest-feasible", 0).await;
    let before = f.side_effect_counts();
    let mut candidate = None;
    for _ in 0..4 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        let stored = saved(&f);
        assert_eq!(stored["intents"].as_array().unwrap().len(), 1);
        assert_eq!(children(&f), 1);
        if stored["scan"]["candidate"]["source_job_id"] == old {
            candidate = Some(stored);
            break;
        }
    }
    let first =
        candidate.expect("older eligible source must be remembered before newer source's page");
    assert!(first["cursor"].as_u64().unwrap() < first["scan"]["through"].as_u64().unwrap());
    f.restart(false).await;
    assert_eq!(saved(&f)["scan"], first["scan"]);
    assert_eq!(saved(&f)["cursor"], first["cursor"]);
    let mut selected = None;
    for _ in 0..32 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        let record = f.get(&path).await;
        assert_ne!(record["pending_source_job_id"], old);
        assert_eq!(record["status"], "active", "{record}");
        if record["pending_source_job_id"] == newest {
            selected = Some(record);
            break;
        }
    }
    assert_eq!(
        selected.expect("newest source at the end of the captured backlog")["assessment_count"],
        2
    );
    let done = drain(&f, &path, |v| v["assessment_count"] == 2 && idle(v)).await;
    let last = f
        .get(&format!(
            "/venue-assessments/{}",
            done["last_assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(last["source_job_id"], newest);
    assert_eq!(proof(&last)["output"]["verdict"], "feasible");
    assert_eq!(children(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);
    assert_eq!(f.side_effect_counts(), (before.0 + 1, before.1));
}

#[tokio::test]
async fn unresolved_manual_assessment_retains_newer_candidate_without_spending_an_intent() {
    let f = Fixture::start().await;
    let baseline = f.source("busy-baseline", 0).await;
    let path = register(&f, &baseline, "busy-monitor").await;
    drain(&f, &path, idle).await;
    let manual_source = f.source("busy-manual-source", 0).await;
    let manual = f
        .create(f.request_body("busy-manual", &manual_source))
        .await;
    f.collected(&manual).await; // Native completion alone still occupies the cycle.
    let update = f.source("busy-newer-cut", 3).await;
    for _ in 0..8 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    let waiting = f.get(&path).await;
    assert_eq!(waiting["status"], "active");
    assert_eq!(waiting["assessment_count"], 1);
    assert_eq!(children(&f), 2);
    assert_eq!(saved(&f)["scan"]["candidate"]["source_job_id"], update);
    assert!(waiting["pending_source_job_id"].is_null());
    assert_eq!(f.providers.execution_posts().len(), 2);
    f.verify(&manual).await;
    let done = drain(&f, &path, |v| v["status"] == "completed").await;
    assert_eq!(done["assessment_count"], 2);
    let last = f
        .get(&format!(
            "/venue-assessments/{}",
            done["last_assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(last["source_job_id"], update);
    assert_eq!(proof(&last)["output"]["verdict"], "reopen");
    assert_eq!(children(&f), 3);
    assert_eq!(f.providers.execution_posts().len(), 3);
}

#[tokio::test]
async fn missing_provider_preserves_uncreated_intent_and_restores_it_once() {
    let mut f = Fixture::start().await;
    let baseline = f.source("pending-outage-baseline", 0).await;
    let path = register(&f, &baseline, "pending-outage-monitor").await;
    let original = saved(&f)["intents"][0].clone();
    f.server.stop().await;
    f.config.forge_checkpoints.clear();
    f.app = build_org(
        &f.config,
        Some(ProviderSet::new().with_dev_stub(DevStub::default())),
    )
    .unwrap();
    f.server = Server::router(f.app.clone().router()).await;
    for _ in 0..3 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    let waiting = f.get(&path).await;
    assert_eq!(waiting["status"], "active");
    assert_eq!(waiting["assessment_count"], 1);
    assert_eq!(waiting["pending_source_job_id"], baseline);
    assert_eq!(children(&f), 0);
    assert_eq!(saved(&f)["intents"][0], original);
    assert!(f.providers.execution_posts().is_empty());
    f.restart(false).await;
    let done = drain(&f, &path, idle).await;
    assert_eq!(done["assessment_count"], 1);
    assert_eq!(children(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(saved(&f)["intents"][0]["request"], original["request"]);
}

#[tokio::test]
async fn manual_consumption_after_intent_commit_retains_budget_and_allows_newer_work() {
    let f = Fixture::start().await;
    let baseline = f.source("intent-race-baseline", 0).await;
    let path = register(&f, &baseline, "intent-race-monitor").await;
    let original = saved(&f)["intents"][0]["request"].clone();
    let manual = f
        .create(f.request_body("intent-race-manual", &baseline))
        .await;
    f.collected(&manual).await;
    f.verify(&manual).await;
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let waiting = f.get(&path).await;
    assert_eq!(waiting["status"], "active");
    assert_eq!(
        waiting["assessment_count"], 1,
        "a committed reservation is never refunded"
    );
    assert!(waiting["pending_source_job_id"].is_null());
    assert_eq!(saved(&f)["intents"][0]["skipped"], "source_consumed");
    assert_eq!(saved(&f)["intents"][0]["request"], original);
    let update = f.source("intent-race-newer", 0).await;
    let done = drain(&f, &path, |v| v["assessment_count"] == 2 && idle(v)).await;
    let last = f
        .get(&format!(
            "/venue-assessments/{}",
            done["last_assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(last["source_job_id"], update);
    assert_eq!(children(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);
}
