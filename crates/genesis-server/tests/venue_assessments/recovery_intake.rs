//! Opt-in recovery intake over genuine loopback invocation and assessment
//! receipts. Explicit worker drains expose the durable preparation boundary;
//! these tests do not claim native sponsor execution or a venue booking.
use super::*;
use genesis_core::events::EventKind;
use genesis_server::venue_recovery_intake;

fn recovery_count(f: &Fixture) -> u64 {
    sqlite(f)
        .query_row("SELECT COUNT(*) FROM venue_recoveries", [], |row| {
            row.get(0)
        })
        .unwrap()
}

async fn records(f: &Fixture) -> Vec<Value> {
    venue_recovery_intake::list(&f.app)
        .await
        .unwrap()
        .into_iter()
        .map(|record| serde_json::to_value(record).unwrap())
        .collect()
}

async fn drain(f: &Fixture, count: usize) {
    for _ in 0..count {
        venue_recovery_intake::drain_once(&f.app).await.unwrap();
    }
}

async fn until(f: &Fixture, assessment: &Value, done: impl Fn(&Value) -> bool) -> Value {
    let mut last = Vec::new();
    // The setup's historical events must also pass through the bounded trigger
    // and source pages. Never assume that the first drain reaches this source.
    for _ in 0..256 {
        venue_recovery_intake::drain_once(&f.app).await.unwrap();
        last = records(f).await;
        if let Some(record) = last
            .iter()
            .find(|record| record["assessment_id"] == assessment["assessment_id"])
        {
            if done(record) {
                return record.clone();
            }
        }
    }
    panic!("bounded intake wait failed: {last:?}");
}

fn exact_request(prepared: &Value) -> Value {
    json!({
        "client_request_id": prepared["client_request_id"],
        "reopened_assessment_id": prepared["assessment_id"],
        "candidate_job_ids": [prepared["candidate_job_id"]],
        "deadline": prepared["deadline"],
    })
}

async fn restart_enabled(f: &mut Fixture, bridge: &Bridge) {
    f.restart(false).await;
    f.app.automatic_venue_recovery = true;
    bridge.attach(f).await;
}

#[tokio::test]
async fn automatic_recovery_requires_explicit_opt_in() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    reopened(&f).await;
    assert!(!f.app.automatic_venue_recovery);
    let effects = f.side_effect_counts();
    let revision = f.app.current_state().unwrap().revision;
    for _ in 0..20 {
        assert_eq!(venue_recovery_intake::drain_once(&f.app).await.unwrap(), 0);
    }
    assert!(records(&f).await.is_empty());
    assert_eq!(recovery_count(&f), 0);
    assert_eq!(f.app.current_state().unwrap().revision, revision);
    assert_eq!(f.side_effect_counts(), effects);
    assert!(bridge.sends().is_empty());
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn historical_source_prepares_once_then_recovery_completes_across_restart() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let events = f.app.store.events_after(0, 4096).unwrap();
    let succeeded = |id: &str| {
        events
            .iter()
            .find(|event| {
                matches!(&event.kind,
                EventKind::JobStatusChanged { job_id, to: JobStatus::Succeeded, .. }
                    if job_id.as_str() == id)
            })
            .unwrap()
    };
    let source_event = succeeded(&source);
    assert!(source_event.mission_id.is_none());
    assert!(source_event.seq > 16, "source discovery must cross a page");
    assert!(source_event.seq < succeeded(reopened["job_id"].as_str().unwrap()).seq);

    f.app.automatic_venue_recovery = true;
    let prepared = until(&f, &reopened, |r| r["phase"] == "prepared").await;
    assert_eq!(prepared["candidate_job_id"], source);
    assert!(prepared["client_request_id"].is_string());
    assert!(prepared["deadline"].is_string());
    assert!(prepared["recovery_id"].is_null());
    assert_eq!(recovery_count(&f), 0, "preparation must be durable first");
    assert!(bridge.sends().is_empty());

    restart_enabled(&mut f, &bridge).await;
    assert_eq!(records(&f).await, vec![prepared.clone()]);
    // A newer verified source arriving after preparation must not replace the
    // saved candidate or renew the request's deadline during crash recovery.
    let newer = f.source("intake-after-preparation", 3).await;
    assert_ne!(newer, source);
    let linked = until(&f, &reopened, |r| r["phase"] == "linked").await;
    assert_eq!(exact_request(&linked), exact_request(&prepared));
    assert_eq!(linked["externally_managed"], false);
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 1);
    drain(&f, 20).await;
    assert_eq!(records(&f).await, vec![linked.clone()]);
    assert_eq!(recovery_count(&f), 1);

    let path = format!(
        "/venue-recoveries/{}",
        linked["recovery_id"].as_str().unwrap()
    );
    let mut recovery = f.get(&path).await;
    for _ in 0..40 {
        recovery = step(&f, &path).await;
        assert!(recovery["attention"].is_null(), "{recovery}");
        if recovery["assessment_id"].is_string() {
            break;
        }
    }
    let child = f
        .get(&format!(
            "/venue-assessments/{}",
            recovery["assessment_id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(child["source_job_id"], source);
    assert_eq!(child["policy"]["selected_venue_id"], "replacement-hall");
    f.collected(&child).await;
    let accepted = f.verify(&child).await;
    assert_eq!(proof(&accepted)["decision"], "accepted");
    assert_eq!(proof(&accepted)["output"]["verdict"], "feasible");
    let completed = step(&f, &path).await;
    assert_eq!(completed["phase"], "completed");
    assert_eq!(completed["retirement"]["local_contract_state"], "RELEASED");
    assert_eq!(completed["retirement"]["delivery"], "bridge_queued");
    assert_eq!(bridge.sends().len(), 3);
    assert_eq!(bridge.release_sends().len(), 1);
    assert_eq!(assessment_count(&f), 2);
    assert_eq!(f.providers.execution_posts().len(), 2);

    restart_enabled(&mut f, &bridge).await;
    drain(&f, 32).await;
    let all = records(&f).await;
    assert!(all.iter().any(|record| record == &linked));
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.get(&path).await, completed);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert_eq!(bridge.sends().len(), 3);
    assert_eq!(bridge.release_sends().len(), 1);
}

#[tokio::test]
async fn automatic_intake_links_an_existing_manual_recovery_without_recruiting_again() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, manual) = register(&f, recovery_request(&source, &reopened)).await;
    let revision = f.app.current_state().unwrap().revision;
    f.app.automatic_venue_recovery = true;
    let linked = until(&f, &reopened, |r| r["phase"] == "linked").await;
    assert_eq!(linked["recovery_id"], manual["recovery_id"]);
    assert_eq!(linked["externally_managed"], true);
    assert!(linked["client_request_id"].is_null());
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.app.current_state().unwrap().revision, revision);
    assert_eq!(f.get(&path).await, manual);

    restart_enabled(&mut f, &bridge).await;
    drain(&f, 20).await;
    assert_eq!(records(&f).await, vec![linked]);
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.get(&path).await, manual);
    assert!(bridge.sends().is_empty());
    assert_eq!(f.providers.execution_posts().len(), 1);
}

#[tokio::test]
async fn committed_exact_intent_is_reconciled_when_creation_is_disabled_after_restart() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (_, reopened) = reopened(&f).await;
    f.app.automatic_venue_recovery = true;
    let prepared = until(&f, &reopened, |r| r["phase"] == "prepared").await;
    // Public creation models the crash window after the coordinator commits
    // but before the intake records its link. No intake journal is edited.
    let (path, created) = register(&f, exact_request(&prepared)).await;
    f.restart(false).await;
    assert!(!f.app.automatic_venue_recovery);
    assert!(f.app.fetch_recovery.is_none());
    let linked = until(&f, &reopened, |r| r["phase"] == "linked").await;
    assert_eq!(linked["recovery_id"], created["recovery_id"]);
    assert_eq!(linked["externally_managed"], false);
    assert_eq!(exact_request(&linked), exact_request(&prepared));
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.get(&path).await, created);
    assert!(bridge.sends().is_empty());
}

/// A fixed provider response for the $750 original venue under a $700 budget.
/// Dispatch is paused while the fixture response is installed. The actual
/// response still travels through the CREAO client, private runtime journal,
/// and assessment verifier; no proof or execution receipt is manufactured.
async fn price_reopen_without_replacement(f: &mut Fixture) -> Value {
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
    let source = f.source("intake-original-too-expensive", 0).await;
    f.server.stop().await;
    f.server = Server::router(f.app.clone().router()).await;
    let mut body = f.request_body("intake-price-reopen", &source);
    body["policy"]["budget"]["minor_units"] = json!(70000);
    let assessment = f.create(body).await;
    let job = f
        .app
        .store
        .get_job(&JobId::new(assessment["job_id"].as_str().unwrap()))
        .unwrap();
    let context = &job.input["context"];
    assert_eq!(context["venue"]["venue_id"], "grand-hall");
    assert_eq!(context["venue"]["price"]["minor_units"], 75000);
    assert_eq!(context["policy"]["budget"]["minor_units"], 70000);
    let response = json!({
        "schema_version": 1, "assessment_id": context["assessment_id"],
        "source_job_id": context["source_job_id"], "invocation_proof_ref": context["invocation_proof_ref"],
        "policy_sha256": context["policy_sha256"], "selected_venue_id": "grand-hall",
        "evidence": context["venue"]["evidence"],
        "checks": {"capacity": true, "price": false, "window": true, "availability": true, "step_free": true},
        "verdict": "reopen",
    });
    {
        let mut world = f.providers.world.lock().unwrap();
        world.mode = ResponseMode::ReplayFirst;
        world.first = Some(response);
    }
    f.server.stop().await;
    f.server = Server::org(f.app.clone(), false).await;
    f.collected(&assessment).await;
    let accepted = f.verify(&assessment).await;
    assert_eq!(proof(&accepted)["decision"], "accepted");
    assert_eq!(proof(&accepted)["output"]["verdict"], "reopen");
    f.providers.mode(ResponseMode::Correct);
    accepted
}

#[tokio::test]
async fn no_feasible_candidate_waits_then_accepts_later_verified_evidence_after_restart() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let reopened = price_reopen_without_replacement(&mut f).await;
    f.app.automatic_venue_recovery = true;
    let waiting = until(&f, &reopened, |r| {
        r["attention"] == "awaiting_feasible_verified_source"
    })
    .await;
    assert_eq!(waiting["phase"], "discovering");
    assert!(waiting["client_request_id"].is_null());
    assert!(waiting["deadline"].is_null());
    assert_eq!(recovery_count(&f), 0);
    drain(&f, 20).await;
    assert_eq!(recovery_count(&f), 0);

    restart_enabled(&mut f, &bridge).await;
    let later = f.source("intake-later-affordable-replacement", 3).await;
    let prepared = until(&f, &reopened, |r| r["phase"] == "prepared").await;
    assert_eq!(prepared["candidate_job_id"], later);
    assert!(prepared["attention"].is_null());
    assert_eq!(recovery_count(&f), 0);
    let linked = until(&f, &reopened, |r| r["phase"] == "linked").await;
    assert_eq!(exact_request(&linked), exact_request(&prepared));
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(assessment_count(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert!(bridge.sends().is_empty());
}

#[tokio::test]
async fn accepted_feasible_assessment_does_not_create_a_recovery() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let source = f.source("intake-feasible-source", 0).await;
    let assessment = f.create(f.request_body("intake-feasible", &source)).await;
    f.collected(&assessment).await;
    let accepted = f.verify(&assessment).await;
    assert_eq!(proof(&accepted)["output"]["verdict"], "feasible");
    f.app.automatic_venue_recovery = true;
    let ignored = until(&f, &accepted, |r| r["phase"] == "ignored").await;
    assert!(ignored["recovery_id"].is_null());
    assert!(ignored["client_request_id"].is_null());
    drain(&f, 20).await;
    assert_eq!(recovery_count(&f), 0);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert!(bridge.sends().is_empty());
}

#[tokio::test]
async fn irrelevant_event_backlog_does_not_spend_the_matching_source_page_budget() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let observation = f
        .app
        .current_state()
        .unwrap()
        .observations
        .keys()
        .next()
        .unwrap()
        .clone();
    // Lightweight scheduling fixture: these log-only no-change decisions do
    // not create provider work, invocation proofs, or assessment evidence.
    let noise = (0..10_000)
        .map(|_| EventKind::PlannerDecision {
            planner: "synthetic-backlog".into(),
            model: "fixture-no-model-call".into(),
            observation_id: observation.clone(),
            verdict: genesis_core::events::PlannerVerdict::NoChange,
            summary: "Irrelevant scheduling backlog".into(),
            rationale: "No organizational change requested by this fixture".into(),
            attempt: 1,
            latency_ms: 0,
            requirement_id: None,
            change_id: None,
            kernel_errors: vec![],
        })
        .collect();
    f.app
        .store
        .append_events(noise, &Default::default(), |_| Ok(()))
        .unwrap();
    let (source, reopened) = reopened(&f).await;
    f.app.automatic_venue_recovery = true;
    let mut prepared = None;
    for _ in 0..8 {
        venue_recovery_intake::drain_once(&f.app).await.unwrap();
        prepared = records(&f).await.into_iter().find(|record| {
            record["assessment_id"] == reopened["assessment_id"] && record["phase"] == "prepared"
        });
        if prepared.is_some() {
            break;
        }
    }
    let prepared = prepared.expect("irrelevant history must not consume trigger/source pages");
    assert_eq!(prepared["candidate_job_id"], source);
    assert_eq!(recovery_count(&f), 0);
    assert!(bridge.sends().is_empty());
}

#[tokio::test]
async fn old_unobserved_manual_recovery_links_without_creao_configuration() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (source, reopened) = reopened(&f).await;
    let (path, manual) = register(&f, recovery_request(&source, &reopened)).await;
    assert!(
        records(&f).await.is_empty(),
        "no authenticated intake anchor exists yet"
    );
    f.server.stop().await;
    // Explicit clock fixture changes only the event envelope used by the new
    // Observing hint. The real assessment proof, native receipts, source jobs,
    // and already-created recovery remain byte-for-byte intact.
    let old = (Utc::now() - ChronoDuration::hours(2)).to_rfc3339();
    assert_eq!(
        sqlite(&f)
            .execute(
                "UPDATE events SET created_at=?1 WHERE event_type='job_status_changed'
                 AND json_extract(payload_json,'$.job_id')=?2
                 AND json_extract(payload_json,'$.to')='SUCCEEDED'",
                rusqlite::params![old, reopened["job_id"].as_str().unwrap()],
            )
            .unwrap(),
        1
    );
    f.config.forge_checkpoints.clear();
    f.app = build_org(
        &f.config,
        Some(ProviderSet::new().with_dev_stub(DevStub::default())),
    )
    .unwrap();
    f.app.automatic_venue_recovery = true;
    assert!(f.app.providers.specialist(Provider::Creao).is_err());
    assert!(f.app.fetch_recovery.is_none());
    f.server = Server::router(f.app.clone().router()).await;
    let revision = f.app.current_state().unwrap().revision;
    let linked = until(&f, &reopened, |record| record["phase"] == "linked").await;
    assert_eq!(linked["recovery_id"], manual["recovery_id"]);
    assert_eq!(linked["externally_managed"], true);
    assert!(linked["attention"].is_null());
    assert!(linked["client_request_id"].is_null());
    assert_eq!(f.app.current_state().unwrap().revision, revision);
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.get(&path).await, manual);
    assert!(bridge.sends().is_empty());
    assert_eq!(f.providers.execution_posts().len(), 1);
}

async fn unavailable_newer_source_holds_preparation(deleted: bool) {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (older, reopened) = reopened(&f).await;
    let newer = f.source("intake-newer-unreadable-proof", 3).await;
    assert!(
        f.app.store.get_job(&JobId::new(&newer)).unwrap().created_at
            > f.app.store.get_job(&JobId::new(&older)).unwrap().created_at
    );
    let db = sqlite(&f);
    let proof: String = db
        .query_row(
            "SELECT proof_json FROM capability_invocation_verifications WHERE job_id=?1",
            [&newer],
            |row| row.get(0),
        )
        .unwrap();
    // Explicit unavailable-journal fixture: the job already succeeded with a
    // genuine proof, then that proof becomes unreadable or its row disappears.
    // Neither condition is an authenticated nonmatching source. An older valid
    // source must not erase the uncertainty and freeze a replacement request.
    let perturb = if deleted {
        "DELETE FROM capability_invocation_verifications WHERE job_id=?1"
    } else {
        "UPDATE capability_invocation_verifications SET proof_json='{}' WHERE job_id=?1"
    };
    assert_eq!(db.execute(perturb, [&newer]).unwrap(), 1);
    f.app.automatic_venue_recovery = true;
    drain(&f, 32).await;
    let waiting = records(&f).await;
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0]["phase"], "discovering", "{waiting:?}");
    assert!(waiting[0]["attention"].is_string());
    assert!(waiting[0]["client_request_id"].is_null());
    assert!(waiting[0]["candidate_job_id"].is_null());
    assert!(waiting[0]["deadline"].is_null());
    assert_eq!(recovery_count(&f), 0);
    assert!(bridge.sends().is_empty());

    // Restore the complete original row (job_id and exact proof_json), rather
    // than reconstructing a new decision or rerunning the provider.
    let restored = if deleted {
        db.execute(
            "INSERT INTO capability_invocation_verifications (job_id,proof_json) VALUES (?1,?2)",
            rusqlite::params![newer, proof],
        )
    } else {
        db.execute(
            "UPDATE capability_invocation_verifications SET proof_json=?1 WHERE job_id=?2",
            rusqlite::params![proof, newer],
        )
    };
    assert_eq!(restored.unwrap(), 1);
    let prepared = until(&f, &reopened, |record| record["phase"] == "prepared").await;
    assert_eq!(prepared["candidate_job_id"], newer);
    assert!(prepared["attention"].is_null());
    assert_eq!(recovery_count(&f), 0);
    let linked = until(&f, &reopened, |record| record["phase"] == "linked").await;
    assert_eq!(exact_request(&linked), exact_request(&prepared));
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert!(bridge.sends().is_empty());
}

#[tokio::test]
async fn unreadable_newer_source_holds_preparation_until_its_exact_proof_is_restored() {
    unavailable_newer_source_holds_preparation(false).await;
}

#[tokio::test]
async fn deleted_newer_source_proof_holds_preparation_until_its_exact_row_is_restored() {
    unavailable_newer_source_holds_preparation(true).await;
}

#[tokio::test]
async fn unavailable_recruiting_authority_does_not_freeze_an_older_candidate() {
    let mut f = Fixture::start().await;
    let bridge = Bridge::start().await;
    bridge.attach(&mut f).await;
    let (older, reopened) = reopened(&f).await;
    // Explicit authority fixture: historical native evidence stays unchanged;
    // the existing role temporarily loses permission to recruit contractors.
    f.mutate(|state| {
        state
            .roles
            .get_mut(&RoleId::new(&f.monitor))
            .unwrap()
            .authority
            .scopes
            .remove("contract.logistics");
    })
    .await;
    f.app.automatic_venue_recovery = true;
    drain(&f, 32).await;
    let blocked = records(&f).await;
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0]["phase"], "discovering", "{blocked:?}");
    assert!(blocked[0]["attention"].is_string());
    assert!(blocked[0]["client_request_id"].is_null());
    assert!(blocked[0]["candidate_job_id"].is_null());
    assert!(blocked[0]["deadline"].is_null());
    assert_eq!(recovery_count(&f), 0);

    let newer = f.source("intake-newer-while-recruiting-blocked", 3).await;
    assert!(
        f.app.store.get_job(&JobId::new(&newer)).unwrap().created_at
            > f.app.store.get_job(&JobId::new(&older)).unwrap().created_at
    );
    drain(&f, 16).await;
    let blocked = records(&f).await;
    assert_eq!(blocked[0]["phase"], "discovering");
    assert!(blocked[0]["client_request_id"].is_null());
    assert!(blocked[0]["deadline"].is_null());
    assert_eq!(recovery_count(&f), 0);
    assert_eq!(f.app.current_state().unwrap().active_contracts(), 0);
    assert!(bridge.sends().is_empty());

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
    let prepared = until(&f, &reopened, |record| record["phase"] == "prepared").await;
    assert_eq!(prepared["candidate_job_id"], newer);
    assert!(prepared["attention"].is_null());
    assert_eq!(recovery_count(&f), 0);
    let linked = until(&f, &reopened, |record| record["phase"] == "linked").await;
    assert_eq!(exact_request(&linked), exact_request(&prepared));
    assert_eq!(recovery_count(&f), 1);
    assert_eq!(f.providers.execution_posts().len(), 1);
    assert!(bridge.sends().is_empty());
}
