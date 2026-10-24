//! Opt-in subscriptions reuse the parent fixture's real Rust clients and private
//! CREAO journal. Publication/provisioning remain explicitly synthetic setup;
//! no native account, agent creation, readiness, booking or live page is claimed.
use super::*;

fn subscription_request(f: &Fixture, key: &str, source: &str, maximum: u32) -> Value {
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

async fn subscribe(f: &Fixture, request: Value) -> Value {
    let (status, record) = f.post("/venue-monitors", request).await;
    assert_eq!(status, StatusCode::CREATED, "{record}");
    assert!(record["monitor_id"].is_string(), "{record}");
    assert_eq!(record["status"], "active");
    record
}

fn monitor_path(record: &Value) -> String {
    format!("/venue-monitors/{}", record["monitor_id"].as_str().unwrap())
}

fn assessment_path(id: &Value) -> String {
    format!("/venue-assessments/{}", id.as_str().unwrap())
}

fn assessment_rows(f: &Fixture) -> i64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_assessments", [], |row| {
            row.get(0)
        })
        .unwrap()
}

async fn wait_monitor(f: &Fixture, path: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let mut last = Value::Null;
    for _ in 0..500 {
        last = f.get(path).await;
        if predicate(&last) {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("bounded monitor observation failed at {path}: {last}");
}

// Only local coordinators are advanced here; the parent fixture's genuine
// outbox/specialist workers alone prepare/send/poll the native operation.
async fn drain_until(f: &Fixture, path: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let mut last = Value::Null;
    for _ in 0..500 {
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
    panic!("bounded local monitor drain failed at {path}: {last}");
}

#[tokio::test]
async fn subscription_workers_assess_two_updates_across_restart_without_per_cycle_requests() {
    let mut f = Fixture::start().await;
    let role_before = f.app.current_state().unwrap().roles[&RoleId::new(&f.monitor)].clone();
    let initial_source = f.source("subscription-baseline-source", 0).await;
    let request = subscription_request(&f, "persistent-monitor", &initial_source, 4);
    let monitor = subscribe(&f, request.clone()).await;
    let path = monitor_path(&monitor);
    assert_eq!(
        monitor["assessment_count"], 1,
        "registration freezes one bounded initial intent"
    );
    assert_eq!(
        assessment_rows(&f),
        0,
        "registration alone does not create a native job"
    );
    let before_effects = f.side_effect_counts();

    // Real serve_org startup must resume the saved intent. No monitor drain,
    // assessment create, assessment verify, or native step is called by this
    // test during the following two cycles.
    f.restart(true).await;
    let first = wait_monitor(&f, &path, |value| {
        value["assessment_count"] == 1
            && value["last_assessment_id"].is_string()
            && value["pending_assessment_id"].is_null()
    })
    .await;
    assert_eq!(first["status"], "active", "{first}");
    let baseline = f.get(&assessment_path(&first["last_assessment_id"])).await;
    assert_eq!(baseline["source_job_id"], initial_source);
    assert_eq!(proof(&baseline)["decision"], "accepted");
    assert_eq!(proof(&baseline)["output"]["verdict"], "feasible");
    assert!(proof(&baseline)["requirement_id"].is_null());
    assert_eq!(
        f.side_effect_counts(),
        (before_effects.0 + 1, before_effects.1)
    );

    // The only new caller action is supplying a genuine verified invocation.
    // The persistent monitor discovers its committed proof and runs CREAO.
    let updated_source = f.source("subscription-capacity-cut-source", 3).await;
    let completed = wait_monitor(&f, &path, |value| value["status"] == "completed").await;
    assert_eq!(completed["assessment_count"], 2);
    let updated = f
        .get(&assessment_path(&completed["last_assessment_id"]))
        .await;
    assert_eq!(updated["source_job_id"], updated_source);
    assert_eq!(proof(&updated)["decision"], "accepted");
    assert_eq!(proof(&updated)["output"]["verdict"], "reopen");
    assert!(proof(&updated)["requirement_id"].is_string());
    assert_eq!(
        f.side_effect_counts(),
        (before_effects.0 + 2, before_effects.1 + 1)
    );
    assert_ne!(baseline["assessment_id"], updated["assessment_id"]);
    assert_ne!(baseline["job_id"], updated["job_id"]);
    assert_ne!(proof(&baseline)["run_id"], proof(&updated)["run_id"]);
    assert_ne!(
        proof(&baseline)["execution_receipt_sha256"],
        proof(&updated)["execution_receipt_sha256"]
    );
    for record in [&baseline, &updated] {
        assert_eq!(proof(record)["provider_agent_id"], AGENT);
        let job = f
            .app
            .store
            .get_job(&JobId::new(record["job_id"].as_str().unwrap()))
            .unwrap();
        let native = f.specialist.execution_receipt(&job, &binding()).unwrap();
        assert_eq!(native.run_id, proof(record)["run_id"].as_str().unwrap());
        assert_eq!(job.status, JobStatus::Succeeded);
    }
    let state_after = f.app.current_state().unwrap();
    let role_after = &state_after.roles[&RoleId::new(&f.monitor)];
    assert_eq!(role_after.binding, role_before.binding);
    assert_eq!(role_after.state, RoleState::Active);
    assert_eq!(f.providers.execution_posts().len(), 2);
    assert!(f.app.store.commitments().unwrap().is_empty());
    assert!(f
        .providers
        .world
        .lock()
        .unwrap()
        .seen
        .iter()
        .all(|r| !r.path.contains("agents")));

    let effects = f.side_effect_counts();
    f.restart(true).await;
    assert_eq!(
        f.get(&path).await,
        completed,
        "completed monitor survives restart unchanged"
    );
    assert_eq!(
        f.get(&assessment_path(&first["last_assessment_id"])).await,
        baseline
    );
    assert_eq!(
        f.get(&assessment_path(&completed["last_assessment_id"]))
            .await,
        updated
    );
    let (status, replay) = f.post("/venue-monitors", request).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, completed);
    // Reopen completes this subscription even though its count budget was four.
    f.source("post-completion-source", 0).await;
    for _ in 0..3 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    assert_eq!(f.providers.execution_posts().len(), 2);
    assert_eq!(assessment_rows(&f), 2);
    assert_eq!(f.side_effect_counts(), effects);
}

#[tokio::test]
async fn concurrent_drains_create_one_child_and_unresolved_work_blocks_later_updates() {
    let f = Fixture::start().await;
    let source = f.source("concurrent-baseline", 0).await;
    let monitor = subscribe(
        &f,
        subscription_request(&f, "concurrent-monitor", &source, 4),
    )
    .await;
    let path = monitor_path(&monitor);
    let (left, right) = tokio::join!(
        genesis_server::venue_monitors::drain_once(&f.app),
        genesis_server::venue_monitors::drain_once(&f.app)
    );
    left.unwrap();
    right.unwrap();
    let pending = f.get(&path).await;
    assert_eq!(pending["assessment_count"], 1);
    assert_eq!(pending["pending_source_job_id"], source);
    let child = f
        .get(&assessment_path(&pending["pending_assessment_id"]))
        .await;
    assert_eq!(assessment_rows(&f), 1);
    f.collected(&child).await;
    assert!(
        proof(
            &f.get(&assessment_path(&pending["pending_assessment_id"]))
                .await
        )
        .is_null(),
        "native completion alone cannot release the next intent"
    );

    let cut = f.source("concurrent-cut", 3).await;
    for _ in 0..4 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    let still_pending = f.get(&path).await;
    assert_eq!(
        still_pending["pending_assessment_id"],
        child["assessment_id"]
    );
    assert_eq!(still_pending["assessment_count"], 1);
    assert_eq!(assessment_rows(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);

    let done = drain_until(&f, &path, |v| v["status"] == "completed").await;
    assert_eq!(done["assessment_count"], 2);
    let second = f.get(&assessment_path(&done["last_assessment_id"])).await;
    assert_eq!(second["source_job_id"], cut);
    assert_eq!(proof(&second)["output"]["verdict"], "reopen");
    assert_eq!(assessment_rows(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);
}

#[tokio::test]
async fn subscription_acl_strict_bounds_and_idempotency_fail_before_dispatch() {
    let f = Fixture::start().await;
    let source = f.source("registration-source", 0).await;
    let request = subscription_request(&f, "subscription-identity", &source, 4);
    let jobs_before = f.app.store.list_jobs().unwrap().len();
    for principal in ["agent", "adapter"] {
        assert_eq!(
            f.request(
                Method::POST,
                "/venue-monitors",
                principal,
                Some(request.clone())
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    for (field, bad) in [
        ("max_assessments", json!(0)),
        ("max_assessments", json!(17)),
        ("expires_at", json!(Utc::now() - ChronoDuration::seconds(1))),
        ("expires_at", json!(Utc::now() + ChronoDuration::hours(25))),
        ("unexpected", json!(true)),
    ] {
        let mut invalid = request.clone();
        invalid[field] = bad;
        let (status, _) = f.post("/venue-monitors", invalid).await;
        assert!(status.is_client_error(), "invalid {field} was accepted");
    }
    assert_eq!(assessment_rows(&f), 0);
    assert_eq!(f.app.store.list_jobs().unwrap().len(), jobs_before);
    let saved = subscribe(&f, request.clone()).await;
    let path = monitor_path(&saved);
    let (status, replay) = f.post("/venue-monitors", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(saved, replay);
    let mut changed = request.clone();
    changed["policy"]["attendees"] = json!(121);
    assert_eq!(
        f.post("/venue-monitors", changed).await.0,
        StatusCode::CONFLICT
    );
    let mut changed = request.clone();
    changed["max_assessments"] = json!(5);
    assert_eq!(
        f.post("/venue-monitors", changed).await.0,
        StatusCode::CONFLICT
    );
    let mut changed = request.clone();
    changed["expires_at"] = json!(Utc::now() + ChronoDuration::minutes(30));
    assert_eq!(
        f.post("/venue-monitors", changed).await.0,
        StatusCode::CONFLICT
    );
    let mut second = request.clone();
    second["client_request_id"] = json!("another-monitor-same-owner");
    assert_eq!(
        f.post("/venue-monitors", second).await.0,
        StatusCode::CONFLICT,
        "one active subscription owns this responsibility"
    );
    for principal in ["agent", "adapter"] {
        assert_eq!(
            f.request(Method::GET, &path, principal, None).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.request(
                Method::POST,
                &format!("{path}/stop"),
                principal,
                Some(json!({}))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let text = saved.to_string();
    for private in [
        ORACLE_CANARY,
        "PRIVATE_UPSTREAM_ERROR_CANARY",
        "<html",
        "cr_sk_",
        "synthetic-apify-token",
    ] {
        assert!(
            !text.contains(private),
            "monitor status leaked private fixture data"
        );
    }
    let (status, stopped) = f.post(&format!("{path}/stop"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["status"], "stopped");
    for _ in 0..3 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    assert_eq!(
        assessment_rows(&f),
        0,
        "stopping before first delivery must not prepare a child"
    );
    assert!(f.providers.execution_posts().is_empty());
    let (status, replay) = f.post("/venue-monitors", request).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(
        replay["status"], "stopped",
        "replay cannot re-enable the old subscription"
    );
}

#[tokio::test]
async fn subscription_fences_scope_epoch_and_mission_changes_before_new_preparation() {
    for mutation in ["scope", "epoch", "mission"] {
        let f = Fixture::start().await;
        let source = f.source("fence-source", 0).await;
        let saved = subscribe(&f, subscription_request(&f, "frozen-monitor", &source, 4)).await;
        let path = monitor_path(&saved);
        let source_job = f.app.store.get_job(&JobId::new(&source)).unwrap();
        match mutation {
            "scope" => {
                f.mutate(|state| {
                    state
                        .roles
                        .get_mut(&RoleId::new(&f.monitor))
                        .unwrap()
                        .authority
                        .scopes
                        .remove("monitor.venue");
                })
                .await
            }
            "mission" => {
                f.mutate(|state| {
                    state
                        .missions
                        .get_mut(&MissionId::new(&f.mission))
                        .unwrap()
                        .deadline = Some(Utc::now());
                })
                .await
            }
            "epoch" => {
                f.change("subscription-handoff-away", json!([{"op":"ReassignResponsibility","responsibility_id":f.responsibility,"from":f.monitor,"to":{"kind":"existing","role_id":f.backup}}])).await;
                f.change("subscription-handoff-back", json!([{"op":"ReassignResponsibility","responsibility_id":f.responsibility,"from":f.backup,"to":{"kind":"existing","role_id":f.monitor}}])).await;
                let current = f.app.current_state().unwrap();
                assert!(
                    current.responsibilities[&ResponsibilityId::new(&f.responsibility)].epoch
                        > source_job.epoch
                );
                assert_eq!(
                    current.roles[&RoleId::new(&f.monitor)].binding,
                    Some(binding()),
                    "same agent is not sufficient authority for the old subscription"
                );
            }
            _ => unreachable!(),
        }
        let effects = f.side_effect_counts();
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        let fenced = f.get(&path).await;
        assert_ne!(fenced["status"], "active", "{mutation}: {fenced}");
        assert!(
            matches!(fenced["status"].as_str(), Some("attention" | "stopped")),
            "{mutation}: {fenced}"
        );
        assert_eq!(
            fenced["assessment_count"], 1,
            "existing intent stays charged"
        );
        assert_eq!(
            assessment_rows(&f),
            0,
            "{mutation} must stop before child insertion"
        );
        assert!(f.providers.execution_posts().is_empty());
        assert!(f.journal.list().unwrap().is_empty());
        assert_eq!(f.side_effect_counts(), effects);
    }
}

#[tokio::test]
async fn unknown_subscription_child_is_preserved_across_restart_without_native_replay() {
    let mut f = Fixture::start().await;
    f.providers.mode(ResponseMode::Ambiguous);
    let source = f.source("unknown-monitor-source", 0).await;
    let saved = subscribe(&f, subscription_request(&f, "unknown-monitor", &source, 4)).await;
    let path = monitor_path(&saved);
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let pending = f.get(&path).await;
    let child_id = pending["pending_assessment_id"].clone();
    let child = f.get(&assessment_path(&child_id)).await;
    f.wait(
        &format!("/jobs/{}", child["job_id"].as_str().unwrap()),
        |v| v["status"] == "UNKNOWN",
    )
    .await;
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let attention = f.get(&path).await;
    assert_eq!(attention["status"], "attention");
    assert_eq!(attention["pending_assessment_id"], child_id);
    assert_eq!(attention["assessment_count"], 1);
    assert!(!attention
        .to_string()
        .contains("PRIVATE_UPSTREAM_ERROR_CANARY"));
    let effects = f.side_effect_counts();
    assert_eq!(f.providers.execution_posts().len(), 1);
    f.providers.mode(ResponseMode::Correct);
    f.restart(false).await;
    f.source("new-proof-while-native-unknown", 3).await;
    let calls = f.providers.world.lock().unwrap().seen.len();
    for _ in 0..5 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    let restored = f.get(&path).await;
    assert_eq!(restored["status"], "attention");
    assert_eq!(restored["pending_assessment_id"], child_id);
    assert_eq!(restored["assessment_count"], 1);
    assert_eq!(assessment_rows(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert_eq!(
        f.providers.world.lock().unwrap().seen.len(),
        calls,
        "subscription reconciliation performs no provider request"
    );
    assert_eq!(f.side_effect_counts(), effects);
    assert!(proof(&f.get(&assessment_path(&child_id)).await).is_null());
}

#[tokio::test]
async fn stop_keeps_an_existing_child_but_never_schedules_another_cycle() {
    let f = Fixture::start().await;
    let source = f.source("stop-monitor-source", 0).await;
    let saved = subscribe(&f, subscription_request(&f, "stop-monitor", &source, 4)).await;
    let path = monitor_path(&saved);
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let pending = f.get(&path).await;
    let child = f
        .get(&assessment_path(&pending["pending_assessment_id"]))
        .await;
    f.collected(&child).await;
    let (status, stopped) = f.post(&format!("{path}/stop"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["status"], "stopped");
    assert_eq!(stopped["pending_assessment_id"], child["assessment_id"]);
    genesis_server::venue_assessments::drain_once(&f.app)
        .await
        .unwrap();
    let accepted = f.get(&assessment_path(&child["assessment_id"])).await;
    assert_eq!(
        proof(&accepted)["decision"],
        "accepted",
        "stop is not cancellation of existing authorized work"
    );
    f.source("post-stop-cut", 3).await;
    for _ in 0..3 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    let still_stopped = f.get(&path).await;
    assert_eq!(still_stopped["status"], "stopped");
    assert_eq!(still_stopped["assessment_count"], 1);
    assert_eq!(assessment_rows(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn assessment_budget_stops_after_one_accepted_feasible_cycle() {
    let f = Fixture::start().await;
    let source = f.source("budget-monitor-source", 0).await;
    let saved = subscribe(&f, subscription_request(&f, "budget-monitor", &source, 1)).await;
    let path = monitor_path(&saved);
    let completed = drain_until(&f, &path, |v| v["status"] == "completed").await;
    assert_eq!(completed["assessment_count"], 1);
    let child = f
        .get(&assessment_path(&completed["last_assessment_id"]))
        .await;
    assert_eq!(proof(&child)["output"]["verdict"], "feasible");
    f.source("post-budget-cut", 3).await;
    for _ in 0..3 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    assert_eq!(f.get(&path).await, completed);
    assert_eq!(assessment_rows(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn committed_child_recovers_after_link_save_failure_without_renewing_its_request() {
    for stop_before_recovery in [false, true] {
        let mut f = Fixture::start().await;
        let source = f.source("lost-link-source", 0).await;
        let saved = subscribe(
            &f,
            subscription_request(&f, "lost-link-monitor", &source, 1),
        )
        .await;
        let path = monitor_path(&saved);
        let monitor_id = saved["monitor_id"].as_str().unwrap();
        let db = sqlite(&f);
        let stored = |connection: &rusqlite::Connection| -> Value {
            let text: String = connection
                .query_row(
                    "SELECT record_json FROM venue_monitors WHERE monitor_id=?1",
                    [monitor_id],
                    |row| row.get(0),
                )
                .unwrap();
            serde_json::from_str(&text).unwrap()
        };
        let original = stored(&db);
        let exact_request = original["intents"][0]["request"].clone();
        assert_eq!(exact_request["source_job_id"], source);
        assert!(original["intents"][0]["assessment_id"].is_null());
        db.execute_batch(
            "CREATE TRIGGER fail_monitor_child_link BEFORE UPDATE OF record_json ON venue_monitors
             WHEN json_extract(OLD.record_json,'$.intents[0].assessment_id') IS NULL
              AND json_extract(NEW.record_json,'$.intents[0].assessment_id') IS NOT NULL
             BEGIN SELECT RAISE(ABORT,'fixture rejects child link commit'); END;",
        )
        .unwrap();
        assert!(
            genesis_server::venue_monitors::drain_once(&f.app)
                .await
                .is_err(),
            "the child must commit before the injected subscription write failure"
        );
        assert_eq!(assessment_rows(&f), 1);
        assert_eq!(
            stored(&db),
            original,
            "failed row update cannot change the frozen intent"
        );
        let child_id: String = db
            .query_row("SELECT assessment_id FROM venue_assessments", [], |row| {
                row.get(0)
            })
            .unwrap();
        let child = f.get(&format!("/venue-assessments/{child_id}")).await;
        f.collected(&child).await;
        let native_job = f
            .app
            .store
            .get_job(&JobId::new(child["job_id"].as_str().unwrap()))
            .unwrap();
        let receipt = f
            .specialist
            .execution_receipt(&native_job, &binding())
            .unwrap();
        assert_eq!(native_job.status, JobStatus::Verifying);
        assert_eq!(
            native_job.deadline,
            Some(serde_json::from_value(exact_request["deadline"].clone()).unwrap())
        );
        assert_eq!(f.providers.execution_posts().len(), 1);
        let effects = f.side_effect_counts();
        if stop_before_recovery {
            let (status, stopped) = f.post(&format!("{path}/stop"), json!({})).await;
            assert_eq!(status, StatusCode::OK, "{stopped}");
            assert_eq!(stopped["status"], "stopped");
            assert!(stopped["pending_assessment_id"].is_null());
        }
        db.execute_batch("DROP TRIGGER fail_monitor_child_link")
            .unwrap();
        drop(db);

        // The new AppState has no in-memory child ID. Its only recovery input
        // is the saved exact request, whose deadline/key must never be renewed.
        f.restart(false).await;
        let calls_before = f.providers.world.lock().unwrap().seen.len();
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        let recovered = f.get(&path).await;
        assert_eq!(recovered["assessment_count"], 1);
        assert_eq!(recovered["pending_assessment_id"], child_id);
        assert_eq!(recovered["pending_source_job_id"], source);
        if stop_before_recovery {
            assert_eq!(
                recovered["status"], "stopped",
                "historical lookup cannot reactivate a stopped subscription"
            );
        }
        let persisted = stored(&sqlite(&f));
        assert_eq!(persisted["intents"][0]["request"], exact_request);
        assert_eq!(persisted["intents"][0]["assessment_id"], child_id);
        assert_eq!(persisted["binding_sha256"], original["binding_sha256"]);
        assert_eq!(assessment_rows(&f), 1);
        assert_eq!(
            f.providers.world.lock().unwrap().seen.len(),
            calls_before,
            "link recovery is local and read-only toward providers"
        );
        assert_eq!(f.app.store.get_job(&native_job.job_id).unwrap(), native_job);
        assert_eq!(f.side_effect_counts(), effects);

        genesis_server::venue_assessments::drain_once(&f.app)
            .await
            .unwrap();
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        let accepted = f.get(&format!("/venue-assessments/{child_id}")).await;
        assert_eq!(proof(&accepted)["decision"], "accepted");
        assert_eq!(proof(&accepted)["run_id"], receipt.run_id);
        let final_monitor = f.get(&path).await;
        assert_eq!(
            final_monitor["status"],
            if stop_before_recovery {
                "stopped"
            } else {
                "completed"
            }
        );
        assert_eq!(final_monitor["last_assessment_id"], child_id);
        assert_eq!(final_monitor["assessment_count"], 1);
        let (status, replay) = f.post("/venue-assessments", exact_request).await;
        assert_eq!(status, StatusCode::OK, "{replay}");
        assert_eq!(replay, accepted);
        assert_eq!(assessment_rows(&f), 1);
        assert_eq!(f.providers.execution_posts().len(), 1);
        assert_eq!(f.side_effect_counts(), (effects.0 + 1, effects.1));
    }
}

#[tokio::test]
async fn durable_event_cursor_finds_late_verification_after_hundreds_of_unrelated_events() {
    let mut f = Fixture::start().await;
    let baseline_source = f.source("cursor-baseline-source", 0).await;
    let baseline_created = f
        .app
        .store
        .get_job(&JobId::new(&baseline_source))
        .unwrap()
        .created_at;
    let (status, deferred) = f
        .post(
            "/capability-runs",
            json!({"client_request_id":"cursor-deferred-update","installation_id":f.installation,
                "role_id":f.monitor,"responsibility_id":f.responsibility,"suite_id":suite().suite_id,
                "input":suite().cases[3].input}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{deferred}");
    let deferred_id = deferred["job_id"].as_str().unwrap().to_owned();
    let deferred_created = f
        .app
        .store
        .get_job(&JobId::new(&deferred_id))
        .unwrap()
        .created_at;
    assert!(deferred_created > baseline_created);
    assert_eq!(
        f.get(&format!("/capability-runs/{deferred_id}")).await["phase"],
        "prepared",
        "the future source is created, but has no collected or accepted proof yet"
    );
    let registered_at = Utc::now();
    let saved = subscribe(
        &f,
        subscription_request(&f, "cursor-monitor", &baseline_source, 4),
    )
    .await;
    assert!(deferred_created < registered_at);
    let path = monitor_path(&saved);
    let baseline = drain_until(&f, &path, |value| {
        value["last_assessment_id"].is_string() && value["pending_assessment_id"].is_null()
    })
    .await;
    assert_eq!(baseline["assessment_count"], 1);
    assert_eq!(baseline["status"], "active");
    let cursor_before = baseline["event_cursor"].as_u64().unwrap();
    for index in 0..205 {
        let (status, observation) = f
            .post(
                &format!("/missions/{}/observations", f.mission),
                json!({"source":"cursor-fixture","summary":format!("Unrelated valid event {index}")}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{observation}");
    }
    // The existing trusted cards case is a different event/date. Its proof is
    // genuinely accepted, but cannot spend this monitor's same-event budget.
    let wrong_event = f.source("cursor-wrong-event-source", 1).await;
    f.collect(&deferred_id).await;
    let (status, verified) = f
        .post(&format!("/capability-runs/{deferred_id}/verify"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{verified}");
    assert_eq!(verified["decision"], "verified");
    let events = f.app.store.events_after(cursor_before, 512).unwrap();
    assert!(events.len() > 200);
    let verified_sequence = events
        .iter()
        .find_map(|event| match &event.kind {
            genesis_core::events::EventKind::JobStatusChanged {
                job_id,
                to: JobStatus::Succeeded,
                ..
            } if job_id.as_str() == deferred_id => Some(event.seq),
            _ => None,
        })
        .expect("the real invocation verifier must commit a Succeeded event");
    assert!(verified_sequence > cursor_before + 200);
    genesis_server::venue_monitors::drain_once(&f.app)
        .await
        .unwrap();
    let first_page = f.get(&path).await;
    assert_eq!(first_page["assessment_count"], 1);
    let cursor_after_page = first_page["event_cursor"].as_u64().unwrap();
    assert!(cursor_after_page > cursor_before);
    assert!(
        cursor_after_page < verified_sequence,
        "a bounded page cannot skip directly to the last event"
    );

    // Restart must retain progress rather than repeatedly re-reading the first
    // page or discarding jobs whose creation predates subscription registration.
    f.restart(false).await;
    assert_eq!(f.get(&path).await["event_cursor"], cursor_after_page);
    let mut selected = None;
    for _ in 0..32 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
        let record = f.get(&path).await;
        assert_ne!(record["pending_source_job_id"], wrong_event);
        assert_eq!(record["status"], "active", "{record}");
        if record["pending_source_job_id"] == deferred_id {
            selected = Some(record);
            break;
        }
    }
    let selected = selected.expect("bounded cursor pages must discover the late verified source");
    assert_eq!(
        selected["assessment_count"], 2,
        "the unrelated event input spent no intent"
    );
    assert!(selected["event_cursor"].as_u64().unwrap() >= verified_sequence);
    let completed = drain_until(&f, &path, |v| v["status"] == "completed").await;
    assert_eq!(completed["assessment_count"], 2);
    let cut = f
        .get(&assessment_path(&completed["last_assessment_id"]))
        .await;
    assert_eq!(cut["source_job_id"], deferred_id);
    assert_eq!(proof(&cut)["decision"], "accepted");
    assert_eq!(proof(&cut)["output"]["verdict"], "reopen");
    assert!(proof(&cut)["requirement_id"].is_string());
    assert_eq!(assessment_rows(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);
    let effects = f.side_effect_counts();
    for _ in 0..3 {
        genesis_server::venue_monitors::drain_once(&f.app)
            .await
            .unwrap();
    }
    assert_eq!(f.providers.execution_posts().len(), 2);
    assert_eq!(f.side_effect_counts(), effects);
}
