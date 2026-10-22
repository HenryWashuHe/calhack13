//! Independent HTTP regressions for acceptance idempotency and immutable pins.
//! Fixtures are trusted test setup; no Actor submission or cloud call occurs.
use axum::Router;
use chrono::Utc;
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::capability::CapabilityManifest;
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::ids::{InstallationId, JobId, ResponsibilityId, RoleId};
use genesis_core::lifecycle::{
    AssignmentState, InstallationStatus, JobStatus, MissionStatus, ResponsibilityStatus, RoleState,
};
use genesis_core::model::{OrgState, Provider};
use genesis_providers::{ApifyCapabilityExecutor, CapabilityRunReceipt, ProviderSet};
use genesis_server::app::AppState;
use genesis_server::capability_runtime::CapabilityRunPhase;
use genesis_server::capability_store;
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::{put_job, EventMeta};
use genesis_server::{build_org, parse_publishers, OrgConfig};
use genesis_verification::{decode_json, json_sha256, ExecutionEvidence, OracleSuite};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, task }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}

fn manifest() -> CapabilityManifest {
    serde_json::from_value(json!({
        "capability_id":"redteam-fixture-capability","version":"1.0.0","publisher":"org_a",
        "source_digest":"a".repeat(64),"intent":"Read configured venue fixtures",
        "input_schema":{"type":"object"},"output_schema":{"type":"object"},
        "required_permissions":["capability.venue_lookup"],
        "artifact":{"actor_id":"fixture-actor","build_id":"fixture-build","build_number":"0.0.1"},
        "role_template":null,"responsibility_template":null,"workflow_procedure":null,
        "acceptance_suite_ref":suite().suite_id,"verification":{
            "passed":true,"cases":[
                {"name":"a","kind":"success","passed":true},
                {"name":"b","kind":"success","passed":true},
                {"name":"c","kind":"unsupported","passed":true}],
            "tested_environment":"synthetic test setup, not live provider evidence",
            "oracle_ref":"trusted-test-fixture","verified_at":"2026-10-21T00:00:00Z"},
        "state":"APPROVED","published_at":null,
    }))
    .unwrap()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
        .unwrap()
}

async fn json_response(response: reqwest::Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = response.text().await.unwrap();
    let body = serde_json::from_str(&body).unwrap_or(Value::String(body));
    (status, body)
}

fn fixture(
    suites: Vec<OracleSuite>,
    empty_permissions: bool,
) -> (AppState, InstallationId, RoleId, ResponsibilityId) {
    let mut config = OrgConfig::dev("org_a", "Review organization");
    config.dev_stub = false;
    config.oracle_suites = suites;
    // These tests only queue jobs via app.router(), without serve_org's worker.
    // The literal loopback client prevents any accidental external connection.
    let executor = ApifyCapabilityExecutor::new(
        ApifyClient::for_local_testing(
            "synthetic-review-token-never-live",
            RunLimits::default(),
            "http://127.0.0.1:1",
        )
        .unwrap(),
    );
    let app = build_org(
        &config,
        Some(ProviderSet::new().with_executor(Arc::new(executor))),
    )
    .unwrap();
    let initial = app.current_state().unwrap();
    let bootstrap: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Review acceptance batching",
        "roles":[{"key":"owner","name":"Owner","charter":"Own fixture acceptance",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}}],
        "responsibilities":[{"purpose":"Accept fixture capability","owner":"owner"}],
    }))
    .unwrap();
    let (bootstrapped, _) = engine::bootstrap_mission(&initial, &bootstrap, Utc::now()).unwrap();
    app.store
        .commit_state(
            initial.revision,
            &bootstrapped.state,
            bootstrapped.effects,
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    let role = bootstrapped.state.roles.keys().next().unwrap().clone();
    let responsibility = bootstrapped
        .state
        .responsibilities
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut manifest = manifest();
    if empty_permissions {
        manifest.required_permissions.clear();
    }
    let (installed, installation) =
        engine::begin_installation(&bootstrapped.state, &manifest, json!({}), Utc::now()).unwrap();
    app.store
        .commit_state(
            bootstrapped.state.revision,
            &installed.state,
            installed.effects,
            &EventMeta::default(),
            vec![],
            |tx| capability_store::insert_pin(tx, &installation, &manifest),
        )
        .unwrap();
    (app, installation, role, responsibility)
}

#[tokio::test]
async fn acceptance_client_key_binds_entire_batch_even_with_disjoint_case_ids() {
    let first = suite();
    let mut second = first.clone();
    second.suite_id = "second-suite-with-disjoint-case-ids".into();
    for case in &mut second.cases {
        case.case_id = format!("second-{}", case.case_id);
    }
    let (app, installation, role, responsibility) =
        fixture(vec![first.clone(), second.clone()], false);
    let observer = app.clone();
    let server = Server::start(app.router()).await;
    let http = client();
    let url = format!(
        "{}/api/v1/capability-installations/{installation}/checks",
        server.url
    );
    let mut body = json!({"client_request_id":"one-logical-batch","role_id":role,
        "responsibility_id":responsibility,"suite_id":first.suite_id});
    let (status, created) = json_response(
        http.post(&url)
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    assert_eq!(
        created["job_ids"].as_array().unwrap().len(),
        first.cases.len()
    );
    assert_eq!(observer.store.list_jobs().unwrap().len(), first.cases.len());

    let (status, replay) = json_response(
        http.post(&url)
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["job_ids"], created["job_ids"]);
    assert_eq!(observer.store.list_jobs().unwrap().len(), first.cases.len());

    body["suite_id"] = json!(second.suite_id);
    let (status, conflict) = json_response(
        http.post(&url)
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "same key must not create another paid batch: {conflict}"
    );
    assert_eq!(conflict["error"], "idempotency_conflict");
    assert_eq!(observer.store.list_jobs().unwrap().len(), first.cases.len());
}

async fn post(server: &Server, path: &str, body: Value) -> (StatusCode, Value) {
    json_response(
        client()
            .post(format!("{}/api/v1{path}", server.url))
            .bearer_auth("org_a-operator")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await
}

fn mutate(app: &AppState, change: impl FnOnce(&mut OrgState)) {
    let state = app.current_state().unwrap();
    let mut next = state.clone();
    change(&mut next);
    next.revision += 1;
    app.store
        .commit_state(
            state.revision,
            &next,
            vec![],
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
}

fn invalidate(app: &AppState, responsibility: &ResponsibilityId, reason: &str) {
    mutate(app, |state| {
        let mission = state.responsibilities[responsibility].mission_id.clone();
        match reason {
            "assignment_expired" => {
                let assignment = state
                    .active_assignment(responsibility)
                    .unwrap()
                    .assignment_id
                    .clone();
                state
                    .assignments
                    .get_mut(&assignment)
                    .unwrap()
                    .lease_expires_at = Some(Utc::now());
            }
            "mission_expired" => {
                state.missions.get_mut(&mission).unwrap().deadline = Some(Utc::now())
            }
            "mission_inactive" => {
                state.missions.get_mut(&mission).unwrap().status = MissionStatus::Succeeded
            }
            _ => unreachable!(),
        }
    });
}

/// Populate trusted fixture evidence, not a claim of live provider work. The
/// records pass the same storage and semantic validation as collected evidence.
fn seed_collected(app: &AppState, ids: &[JobId]) {
    let outputs: Vec<ExecutionEvidence> = decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/runs.json"
    ))
    .unwrap();
    for id in ids {
        let mut job = app.store.get_job(id).unwrap();
        let mut record = app.store.get_capability_run(id).unwrap();
        let request = &record.request;
        let mut evidence = outputs
            .iter()
            .find(|e| e.case_id == request.case_id)
            .unwrap()
            .clone();
        evidence.artifact = request.artifact.clone();
        let request_sha256 = json_sha256(&json!({
            "schema_version":1,"case_id":request.case_id,"artifact":request.artifact,
            "source":request.source,"input_sha256":json_sha256(&request.input),
        }));
        let receipt = CapabilityRunReceipt {
            provider: Provider::Apify,
            raw_status: "SYNTHETIC_TRUSTED_STORAGE_FIXTURE".into(),
            ticket: Some(
                serde_json::from_value(json!({"schema_version":1,
                "run_id":evidence.run_id,"dataset_id":evidence.dataset_id,
                "request_sha256":request_sha256}))
                .unwrap(),
            ),
            provider_refs: BTreeMap::from([
                ("actor_id".into(), request.artifact.actor_id.clone()),
                ("build_id".into(), request.artifact.build_id.clone()),
                ("build_number".into(), request.artifact.build_number.clone()),
                ("run_id".into(), evidence.run_id.clone()),
            ]),
        };
        job.provider_refs = receipt.provider_refs.clone();
        job.status = JobStatus::Verifying;
        job.result = Some(evidence.output.clone());
        record.receipt = Some(receipt);
        record.evidence = Some(evidence);
        record.phase = CapabilityRunPhase::Collected;
        record.validate(&job).unwrap();
        app.store
            .append_events(vec![], &EventMeta::default(), |tx| {
                put_job(tx, &job)?;
                tx.execute(
                    "UPDATE capability_runs SET phase='collected',record_json=?1 WHERE job_id=?2",
                    rusqlite::params![serde_json::to_string(&record)?, id.as_str()],
                )?;
                Ok(())
            })
            .unwrap();
    }
}

async fn collected_fixture() -> (
    AppState,
    Server,
    InstallationId,
    ResponsibilityId,
    Vec<JobId>,
) {
    let (app, installation, role, responsibility) = fixture(vec![suite()], false);
    let server = Server::start(app.clone().router()).await;
    let (status, response) = post(
        &server,
        &format!("/capability-installations/{installation}/checks"),
        json!({"client_request_id":"authority-checks","role_id":role,
            "responsibility_id":responsibility,"suite_id":suite().suite_id}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{response}");
    let ids: Vec<JobId> = serde_json::from_value(response["job_ids"].clone()).unwrap();
    seed_collected(&app, &ids);
    (app, server, installation, responsibility, ids)
}

#[tokio::test]
async fn empty_permissions_still_require_live_authority_for_checks_and_invocations() {
    for reason in ["assignment_expired", "mission_expired", "mission_inactive"] {
        let (app, installation, role, responsibility) = fixture(vec![suite()], true);
        let server = Server::start(app.clone().router()).await;
        invalidate(&app, &responsibility, reason);
        let (status, response) = post(
            &server,
            &format!("/capability-installations/{installation}/checks"),
            json!({"client_request_id":"blocked-checks","role_id":role,
                "responsibility_id":responsibility,"suite_id":suite().suite_id}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{response}");
        assert_eq!(response["error"], reason);
        mutate(&app, |state| {
            let installed = state.installations.get_mut(&installation).unwrap();
            installed.status = InstallationStatus::Accepted;
            state
                .roles
                .get_mut(&role)
                .unwrap()
                .capabilities
                .push(installed.capability.clone());
        });
        let (status, response) = post(
            &server,
            "/capability-runs",
            json!({
                "client_request_id":"blocked-invocation","installation_id":installation,
                "role_id":role,"responsibility_id":responsibility,"input":suite().cases[0].input,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{response}");
        assert_eq!(response["error"], reason);
        assert!(app.store.list_jobs().unwrap().is_empty());
    }
}

#[tokio::test]
async fn initial_acceptance_rejects_expired_verifier_authority_without_mutating_installation() {
    for reason in ["assignment_expired", "mission_expired", "mission_inactive"] {
        let (app, server, installation, responsibility, ids) = collected_fixture().await;
        invalidate(&app, &responsibility, reason);
        let revision = app.current_state().unwrap().revision;
        let (status, response) = post(
            &server,
            &format!("/capability-installations/{installation}/acceptance"),
            json!({"suite_id":suite().suite_id,"job_ids":ids}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{response}");
        assert_eq!(response["error"], reason);
        let state = app.current_state().unwrap();
        assert_eq!(state.revision, revision);
        assert_eq!(
            state.installations[&installation].status,
            InstallationStatus::Checking
        );
        assert!(app
            .store
            .capability_acceptance(&installation)
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn committed_acceptance_replay_is_read_only_after_verifier_lease_expires() {
    let (app, server, installation, responsibility, ids) = collected_fixture().await;
    let path = format!("/capability-installations/{installation}/acceptance");
    let body = json!({"suite_id":suite().suite_id,"job_ids":ids});
    let (status, accepted) = post(&server, &path, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    invalidate(&app, &responsibility, "assignment_expired");
    let revision = app.current_state().unwrap().revision;
    let (status, replay) = post(&server, &path, body).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["installation"], accepted["installation"]);
    assert_eq!(app.current_state().unwrap().revision, revision);
}

#[tokio::test]
async fn new_mission_inherits_committed_acceptance_after_verifier_mission_ends() {
    let (app, server, installation, responsibility, ids) = collected_fixture().await;
    let (status, accepted) = post(
        &server,
        &format!("/capability-installations/{installation}/acceptance"),
        json!({"suite_id":suite().suite_id,"job_ids":ids}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let state = app.current_state().unwrap();
    let bootstrap: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Independent follow-up using inherited capability",
        "roles":[{"key":"successor","name":"Successor","charter":"Use inherited tools",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":0}}],
        "responsibilities":[{"purpose":"Inspect another venue","owner":"successor"}],
    }))
    .unwrap();
    let (mut outcome, new_mission) =
        engine::bootstrap_mission(&state, &bootstrap, Utc::now()).unwrap();
    let old_assignment = outcome
        .state
        .active_assignment(&responsibility)
        .unwrap()
        .clone();
    let old_mission = outcome.state.responsibilities[&responsibility]
        .mission_id
        .clone();
    outcome.state.missions.get_mut(&old_mission).unwrap().status = MissionStatus::Succeeded;
    outcome
        .state
        .roles
        .get_mut(&old_assignment.role_id)
        .unwrap()
        .state = RoleState::Retired;
    outcome
        .state
        .responsibilities
        .get_mut(&responsibility)
        .unwrap()
        .status = ResponsibilityStatus::Completed;
    outcome
        .state
        .assignments
        .get_mut(&old_assignment.assignment_id)
        .unwrap()
        .state = AssignmentState::Released;
    let new_responsibility = outcome
        .state
        .responsibilities
        .values()
        .find(|r| r.mission_id == new_mission)
        .unwrap()
        .responsibility_id
        .clone();
    let new_role = outcome
        .state
        .active_assignment(&new_responsibility)
        .unwrap()
        .role_id
        .clone();
    let capability = outcome.state.installations[&installation]
        .capability
        .clone();
    outcome
        .state
        .roles
        .get_mut(&new_role)
        .unwrap()
        .capabilities
        .push(capability);
    app.store
        .commit_state(
            state.revision,
            &outcome.state,
            outcome.effects,
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    let (status, queued) = post(&server, "/capability-runs", json!({
        "client_request_id":"inherited-new-mission","installation_id":installation,
        "role_id":new_role,"responsibility_id":new_responsibility,"input":suite().cases[0].input,
    })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["mission_id"], new_mission.as_str());
    assert_eq!(
        app.current_state().unwrap().installations[&installation]
            .acceptance_report_ref
            .as_deref(),
        accepted["installation"]["acceptance_report_ref"].as_str()
    );
    assert_eq!(
        app.store
            .get_capability_run(&serde_json::from_value(queued["job_id"].clone()).unwrap())
            .unwrap()
            .phase,
        CapabilityRunPhase::Prepared
    );
}

#[tokio::test]
async fn registry_rejects_changed_build_under_same_version_and_source_digest() {
    let state =
        RegistryState::open_in_memory(parse_publishers("org_a:review-registry-token"), vec![])
            .unwrap();
    let server = Server::start(registry::router(state)).await;
    let http = client();
    let url = format!("{}/registry/v1/manifests", server.url);
    let original = manifest();
    let (status, created) = json_response(
        http.post(&url)
            .bearer_auth("review-registry-token")
            .json(&original)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (status, replay) = json_response(
        http.post(&url)
            .bearer_auth("review-registry-token")
            .json(&original)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(created, replay);

    let mut substituted = original.clone();
    substituted.artifact.build_id = "substituted-build".into();
    assert_eq!(substituted.source_digest, original.source_digest);
    let (status, conflict) = json_response(
        http.post(&url)
            .bearer_auth("review-registry-token")
            .json(&substituted)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"], "immutable_version");

    let (_, stored) = json_response(
        http.get(format!(
            "{}/registry/v1/capabilities/{}/versions/{}",
            server.url, original.capability_id, original.version
        ))
        .bearer_auth("review-registry-token")
        .send()
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(stored["artifact"]["build_id"], original.artifact.build_id);
}
