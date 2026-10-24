//! Installed-version failure -> bounded replacement, using real server journals
//! and HTTP adapters against loopback providers. No live provider claim.
#[path = "deployed_capability_repair/fixture.rs"]
mod fixture;

use fixture::{assert_redacted, suite, Fixture, NEW_PARSER, OLD_PARSER, ORACLE};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

async fn create(f: &Fixture, request: &Value) -> Value {
    let (status, body) = f.org.post("/capability-repairs", request.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_redacted(&body);
    body
}

fn child_id(value: &Value) -> &str {
    value["acquisition_id"]
        .as_str()
        .expect("repair acquisition ID")
}

#[tokio::test]
async fn installed_failure_repairs_once_survives_restart_and_b_accepts_exact_replacement() {
    let mut f = Fixture::start().await;
    let (job, failure) = f.invocation("fresh-semantic-failure", true).await;
    assert!(failure["case_report"]["issues"]
        .as_array()
        .unwrap()
        .contains(&json!("value_mismatch")));
    let request = f.repair_request(&job);
    let requests = f.providers.requests();
    let repair = create(&f, &request).await;
    let id = child_id(&repair).to_owned();
    assert_eq!(repair["parent_acquisition_id"], f.parent_id);
    assert_eq!(repair["failure_job_id"], job);
    assert_eq!(repair["failure_proof_ref"], failure["proof_ref"]);
    assert_eq!(
        repair["parent_artifact"],
        failure["case_report"]["artifact"]
    );
    assert_eq!(
        f.providers.requests(),
        requests,
        "preparing repair must not call providers"
    );
    assert_eq!(
        f.org
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        2
    );
    assert_eq!(
        f.org.app.store.acquisition_counters().unwrap().model_calls,
        1
    );

    f.org.restart(&f.providers).await;
    let (status, replay) = f.org.post("/capability-repairs", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(
        replay, repair,
        "restart must not renew the deadline or rebind the child"
    );
    assert_eq!(
        f.org.get(&format!("/capability-repairs/{id}")).await,
        repair
    );
    assert_eq!(f.providers.requests(), requests);

    let completed = f.org.drive(&id).await;
    assert_eq!(completed["phase"], "verified", "{completed}");
    let record = f.org.app.acquisition_runtime.status(&id).unwrap();
    assert_eq!(
        record.session().generation().extractor().unwrap(),
        NEW_PARSER
    );
    let verified = record.session().verified(&suite()).unwrap();
    assert_eq!(verified.verification.cases.len(), suite().cases.len());
    assert_ne!(
        verified.artifact.source_sha256,
        f.parent_manifest["source_digest"]
    );
    assert_eq!(verified.artifact.capability_version, "1.0.1");
    assert_eq!(f.providers.count("POST", "/messages"), 2);
    assert_eq!(f.providers.count("POST", "/actors"), 2);
    let published = f.org.publish(&id).await;
    assert_eq!(published["version"], "1.0.1");
    assert_eq!(
        published["artifact"]["build_id"],
        verified.artifact.build_id
    );

    // Publication alone must not change A's accepted v1 installation.
    let a = f.org.get("/organization").await;
    assert_eq!(
        a["installations"][&f.installation_id]["capability"]["version"],
        "1.0.0"
    );
    let (b, b_intent) = f.new_org_b().await;
    let fetched = b
        .get("/capabilities/deployed-venue-reader/versions/1.0.1")
        .await;
    assert_eq!(fetched["source_digest"], published["source_digest"]);
    let model_calls = f.providers.count("POST", "/messages");
    let b_installation = b.install_accept(&b_intent, "1.0.1").await;
    let state = b.get("/organization").await;
    assert_eq!(
        state["installations"][&b_installation]["capability"]["version"],
        "1.0.1"
    );
    assert_eq!(
        state["installations"][&b_installation]["source_digest"],
        published["source_digest"]
    );
    assert_eq!(
        f.providers.count("POST", "/messages"),
        model_calls,
        "B runs its own checks without synthesizing"
    );
    assert!(b.get("/ledger").await.as_array().unwrap().is_empty());

    let world = f.providers.world.lock().unwrap();
    let prompts: Vec<_> = world
        .requests
        .iter()
        .filter(|(m, p, _)| m == "POST" && p == "/v1/messages")
        .map(|(_, _, body)| body.to_string())
        .collect();
    assert_eq!(prompts.len(), 2);
    assert!(
        prompts[1].contains(OLD_PARSER),
        "replacement generation must use the proven parent parser"
    );
    assert!(prompts[1].contains("value_mismatch"));
    assert!(prompts
        .iter()
        .all(|prompt| !prompt.contains(ORACLE) && !prompt.contains("\"expected\"")));
}

#[tokio::test]
async fn operator_boundary_and_verified_results_cannot_create_repair_work() {
    let f = Fixture::start().await;
    let (good, _) = f.invocation("successful-invocation", false).await;
    let request = f.repair_request(&good);
    let before = f.providers.requests();
    for principal in ["agent", "adapter"] {
        let (status, body) = f
            .org
            .request(
                Method::POST,
                "/capability-repairs",
                principal,
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    let (status, body) = f.org.post("/capability-repairs", request).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_redacted(&body);
    assert_eq!(f.org.app.store.acquisition_ids().unwrap().len(), 1);
    assert_eq!(f.providers.requests(), before);
}

#[tokio::test]
async fn replacement_identity_must_be_new_same_capability_and_have_no_extra_repair() {
    let f = Fixture::start().await;
    let (job, _) = f.invocation("invalid-replacement", true).await;
    let request = f.repair_request(&job);
    let mut wrong_capability = request.clone();
    wrong_capability["acquisition"]["candidate"]["capability_id"] = json!("unrelated-capability");
    let mut same_version = request.clone();
    same_version["acquisition"]["candidate"]["capability_version"] = json!("1.0.0");
    let mut same_actor = request.clone();
    same_actor["acquisition"]["candidate"]["actor_name"] =
        f.intent["candidate"]["actor_name"].clone();
    let mut extra_repair = request.clone();
    extra_repair["acquisition"]["repair"] = json!({"capability_id":"deployed-venue-reader","capability_version":"1.0.2","actor_name":"third-parser","source_version":"0.1"});
    let before = f.providers.requests();
    for invalid in [wrong_capability, same_version, same_actor, extra_repair] {
        let (status, body) = f.org.post("/capability-repairs", invalid).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_redacted(&body);
    }
    assert_eq!(
        f.org
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        1
    );
    assert_eq!(f.providers.requests(), before);
}

#[tokio::test]
async fn another_key_or_failure_of_same_parent_cannot_create_second_attempt() {
    let mut f = Fixture::start().await;
    let (first, _) = f.invocation("first-failed-call", true).await;
    let request = f.repair_request(&first);
    let repair = create(&f, &request).await;
    let (second, _) = f.invocation("second-failed-call", true).await;
    let mut duplicate = f.repair_request(&second);
    duplicate["acquisition"]["client_request_id"] = json!("another-repair-key");
    let before = f.providers.requests();
    let (status, body) = f.org.post("/capability-repairs", duplicate.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    f.org.restart(&f.providers).await;
    let (status, body) = f.org.post("/capability-repairs", duplicate).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, replay) = f.org.post("/capability-repairs", request).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, repair);
    assert_eq!(
        f.org
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        2
    );
    assert_eq!(
        f.org.app.store.acquisition_counters().unwrap().model_calls,
        1
    );
    assert_eq!(f.providers.requests(), before);
}

#[tokio::test]
async fn changed_saved_proof_source_or_artifact_never_starts_repair() {
    let f = Fixture::start().await;
    let (job, _) = f.invocation("tampered-failure", true).await;
    let request = f.repair_request(&job);
    let db = rusqlite::Connection::open(f.org.config.db.as_ref().unwrap()).unwrap();
    let original_proof: String = db
        .query_row(
            "SELECT proof_json FROM capability_invocation_verifications WHERE job_id=?1",
            [&job],
            |row| row.get(0),
        )
        .unwrap();
    let original_run: String = db
        .query_row(
            "SELECT record_json FROM capability_runs WHERE job_id=?1",
            [&job],
            |row| row.get(0),
        )
        .unwrap();
    let before = f.providers.requests();
    // Private-storage fault injection; none of these edits simulates an
    // authenticated provider result or a legitimately revised source snapshot.
    for variant in ["proof", "source", "artifact"] {
        if variant == "proof" {
            let mut stored: Value = serde_json::from_str(&original_proof).unwrap();
            stored["proof"]["request_sha256"] = json!("0".repeat(64));
            db.execute(
                "UPDATE capability_invocation_verifications SET proof_json=?1 WHERE job_id=?2",
                rusqlite::params![stored.to_string(), job],
            )
            .unwrap();
        } else {
            let mut stored: Value = serde_json::from_str(&original_run).unwrap();
            if variant == "source" {
                stored["evidence"]["source"]["sha256"] = json!("0".repeat(64));
            } else {
                stored["evidence"]["artifact"]["build_id"] = json!("different-native-build");
            }
            db.execute(
                "UPDATE capability_runs SET record_json=?1 WHERE job_id=?2",
                rusqlite::params![stored.to_string(), job],
            )
            .unwrap();
        }
        let (status, body) = f.org.post("/capability-repairs", request.clone()).await;
        assert!(!status.is_success(), "corrupt {variant} accepted: {body}");
        assert_redacted(&body);
        assert_eq!(
            f.org
                .app
                .store
                .acquisition_counters()
                .unwrap()
                .total_intents,
            1
        );
        assert_eq!(f.providers.requests(), before);
        db.execute(
            "UPDATE capability_invocation_verifications SET proof_json=?1 WHERE job_id=?2",
            rusqlite::params![original_proof, job],
        )
        .unwrap();
        db.execute(
            "UPDATE capability_runs SET record_json=?1 WHERE job_id=?2",
            rusqlite::params![original_run, job],
        )
        .unwrap();
    }
    create(&f, &request).await;
}

#[tokio::test]
async fn failed_repair_link_insert_rolls_back_child_history_and_quota() {
    let f = Fixture::start().await;
    let (job, _) = f.invocation("atomic-repair-link", true).await;
    let request = f.repair_request(&job);
    let db = rusqlite::Connection::open(f.org.config.db.as_ref().unwrap()).unwrap();
    let history_before: u64 = db
        .query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    let counts_before = f.org.app.store.acquisition_counters().unwrap();
    let ids_before = f.org.app.store.acquisition_ids().unwrap();
    let requests = f.providers.requests();
    // Abort the final link write after the child and its first history entry
    // have been prepared, proving these writes share the quota transaction.
    db.execute_batch("CREATE TRIGGER reject_repair_link BEFORE INSERT ON capability_repairs BEGIN SELECT RAISE(ABORT, 'fixture link insert failure'); END;").unwrap();
    let (status, body) = f.org.post("/capability-repairs", request.clone()).await;
    assert!(!status.is_success(), "{body}");
    assert_redacted(&body);
    assert_eq!(
        f.org.app.store.acquisition_counters().unwrap(),
        counts_before
    );
    assert_eq!(f.org.app.store.acquisition_ids().unwrap(), ids_before);
    let history_after: u64 = db
        .query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    let links: u64 = db
        .query_row("SELECT COUNT(*) FROM capability_repairs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(history_after, history_before);
    assert_eq!(links, 0);
    assert_eq!(f.providers.requests(), requests);
    db.execute_batch("DROP TRIGGER reject_repair_link").unwrap();
    create(&f, &request).await;
    assert_eq!(
        f.org
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        2
    );
}

#[tokio::test]
async fn server_record_rejects_valid_sessions_that_replace_or_strip_repair_origin() {
    use genesis_forge::{AcquisitionSession, RepairFeedback};
    let f = Fixture::start().await;
    let (job, _) = f.invocation("immutable-repair-origin", true).await;
    let repair = create(&f, &f.repair_request(&job)).await;
    let child = f
        .org
        .app
        .acquisition_runtime
        .status(child_id(&repair))
        .unwrap();
    let parent = f.org.app.acquisition_runtime.status(&f.parent_id).unwrap();
    let changed_origin = AcquisitionSession::new_deployed_repair(
        child.session().plan().clone(),
        &suite(),
        parent.session(),
        format!("invocation:{}", "0".repeat(64)),
        vec![RepairFeedback::ValueMismatch],
    )
    .unwrap();
    changed_origin.validate(&suite()).unwrap();
    assert_eq!(child.session().plan(), changed_origin.plan());
    assert!(
        child.with_session(changed_origin).is_err(),
        "a separately valid session cannot replace the journal's failure binding"
    );
    let stripped_origin =
        AcquisitionSession::new(child.session().plan().clone(), &suite()).unwrap();
    assert!(
        child.with_session(stripped_origin).is_err(),
        "an ordinary acquisition cannot erase a deployed repair's attempt limit"
    );
    assert_eq!(
        f.org.app.store.acquisition_counters().unwrap().model_calls,
        1
    );
    assert_eq!(f.providers.count("POST", "/messages"), 1);
}

#[tokio::test]
async fn another_verified_acquisition_cannot_supply_the_failed_versions_parser() {
    let f = Fixture::start().await;
    let (job, _) = f.invocation("wrong-parent-failure", true).await;
    let mut other = f.intent.clone();
    other["client_request_id"] = json!("different-proven-parent");
    other["candidate"]["capability_version"] = json!("9.0.0");
    other["candidate"]["actor_name"] = json!("different-verified-actor");
    let (status, acquired) = f.org.post("/capability-acquisitions", other).await;
    assert_eq!(status, StatusCode::CREATED, "{acquired}");
    let other_id = acquired["id"].as_str().unwrap();
    assert_eq!(f.org.drive(other_id).await["phase"], "verified");
    let mut request = f.repair_request(&job);
    request["parent_acquisition_id"] = json!(other_id);
    let requests = f.providers.requests();
    let intents = f
        .org
        .app
        .store
        .acquisition_counters()
        .unwrap()
        .total_intents;
    let (status, body) = f.org.post("/capability-repairs", request).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        f.org
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        intents
    );
    assert_eq!(f.providers.requests(), requests);
}

#[tokio::test]
async fn historical_failure_does_not_authorize_new_repair_without_current_scope() {
    let f = Fixture::start().await;
    let (job, _) = f.invocation("no-current-repair-authority", true).await;
    let request = f.repair_request(&job);
    f.remove_acquire_scope().await;
    let requests = f.providers.requests();
    let (status, body) = f.org.post("/capability-repairs", request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        f.org
            .app
            .store
            .acquisition_counters()
            .unwrap()
            .total_intents,
        1
    );
    assert_eq!(f.providers.requests(), requests);
}

#[tokio::test]
async fn lost_authority_after_generation_fences_repair_before_actor_creation() {
    let f = Fixture::start().await;
    let (job, _) = f.invocation("late-authority-loss", true).await;
    let request = f.repair_request(&job);
    let repair = create(&f, &request).await;
    let id = child_id(&repair);
    let (status, generated) = f
        .org
        .post(&format!("/capability-acquisitions/{id}/step"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{generated}");
    assert_eq!(generated["generation_phase"], "generated");
    let requests = f.providers.requests();
    f.remove_acquire_scope().await;
    let (status, fenced) = f
        .org
        .post(&format!("/capability-acquisitions/{id}/step"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{fenced}");
    assert_eq!(fenced["phase"], "fenced", "{fenced}");
    assert_eq!(f.providers.requests(), requests);
    assert_eq!(f.providers.count("POST", "/actors"), 1);
    let (status, replay) = f.org.post("/capability-repairs", request).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["acquisition_id"], id);
    let record = f.org.app.acquisition_runtime.status(id).unwrap();
    assert_eq!(
        record.session().generation().extractor().unwrap(),
        NEW_PARSER
    );
}

#[tokio::test]
async fn ambiguous_replacement_actor_post_remains_one_consumed_attempt_after_restart() {
    let mut f = Fixture::start().await;
    let (job, _) = f.invocation("unknown-replacement-submission", true).await;
    let request = f.repair_request(&job);
    let repair = create(&f, &request).await;
    let id = child_id(&repair).to_owned();
    f.providers.world.lock().unwrap().unknown_actor = true;
    assert_eq!(f.org.drive(&id).await["phase"], "needs_reconciliation");
    let requests = f.providers.requests();
    f.providers.world.lock().unwrap().unknown_actor = false;
    f.org.restart(&f.providers).await;
    assert_eq!(f.org.drive(&id).await["phase"], "needs_reconciliation");
    let (status, replay) = f.org.post("/capability-repairs", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, repair);
    let mut another = request;
    another["acquisition"]["client_request_id"] = json!("do-not-replace-unknown");
    let (status, body) = f.org.post("/capability-repairs", another).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(f.providers.requests(), requests);
    let counts = f.org.app.store.acquisition_counters().unwrap();
    assert_eq!(counts.model_calls, 2);
    assert_eq!(counts.actor_creates, 2);
    assert_eq!(counts.total_intents, 2);
}

#[tokio::test]
async fn unchanged_parser_is_rejected_without_actor_or_third_generation() {
    let mut f = Fixture::start().await;
    let (job, _) = f.invocation("unchanged-repair", true).await;
    let request = f.repair_request(&job);
    let repair = create(&f, &request).await;
    let id = child_id(&repair).to_owned();
    f.providers.world.lock().unwrap().unchanged_parser = true;
    let completed = f.org.drive(&id).await;
    assert_eq!(completed["phase"], "rejected", "{completed}");
    assert_eq!(completed["unchanged_repair"], true);
    assert_eq!(f.providers.count("POST", "/actors"), 1);
    assert_eq!(f.providers.count("POST", "/messages"), 2);
    let requests = f.providers.requests();
    f.org.restart(&f.providers).await;
    assert_eq!(f.org.drive(&id).await["phase"], "rejected");
    let mut another = request;
    another["acquisition"]["client_request_id"] = json!("do-not-reset-repair-budget");
    let (status, body) = f.org.post("/capability-repairs", another).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(f.providers.requests(), requests);
}
