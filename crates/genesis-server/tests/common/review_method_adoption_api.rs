//! Independent HTTP probes over retained JSON; no method or provider executes.
use super::*;
use genesis_core::development_trial::TrialPlan;
use genesis_server::method_adoption_store::CreateMethodAdoption;

struct Fixture {
    _dir: tempfile::TempDir,
    app: AppState,
    receiver: Arc<MethodReceiver>,
    intent: Value,
    ask: Value,
    adoption_id: String,
    token: String,
}

fn retained_adoption(registry_url: Option<String>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut config = OrgConfig::dev("org_example_b", "Receiver");
    config.db = Some(dir.path().join("receiver.sqlite"));
    config.registry_url = registry_url;
    config.registry_token = Some("synthetic-registry-token".into());
    let app = build_org(&config, None).unwrap();
    let (mission, [owner, _]) = bootstrap(&app);
    let records = dir.path().join("records");
    copy_dir(
        &Path::new(EVIDENCE).join("live-method-20261024-b-revision-receiver/vs-fixed-cart"),
        &records.join("fixed"),
    );
    let plan: TrialPlan =
        serde_json::from_slice(&std::fs::read(records.join("fixed/plan.json")).unwrap()).unwrap();
    let package = std::fs::read_to_string(
        Path::new(EVIDENCE).join("live-method-20261024-b-revision/method-package.json"),
    )
    .unwrap();
    let envelope: Value = serde_json::from_str(&package).unwrap();
    let receiver = Arc::new(MethodReceiver {
        runtime: serde_json::from_value(envelope["package"]["runtime"].clone()).unwrap(),
        protocol: serde_json::from_value(envelope["package"]["protocol"].clone()).unwrap(),
        environments: vec![plan.development_environment.clone()],
        evaluators: vec![plan.context.evaluator.clone()],
        allowed_dependencies: vec![],
        records_root: records,
        acceptance_plans: vec![genesis_server::method_adoption::AcceptancePin {
            requirement_id: owner.requirement.clone(),
            publisher: OrgId::new("org_example_a"),
            plan: plan.reference().unwrap(),
        }],
    });
    let reference = &plan.slots[0].method;
    let intent = json!({"client_request_id":"review-existing", "mission_id":mission,
        "requirement_id":owner.requirement, "role_id":owner.role,
        "responsibility_id":owner.responsibility, "publisher":"org_example_a",
        "reference":reference, "decision":{"decision":"adopt", "trial":"fixed"}});
    let typed: CreateMethodAdoption = serde_json::from_value(intent.clone()).unwrap();
    let (record, _) = app
        .store
        .create_method_adoption(&typed, &package, &receiver, chrono::Utc::now())
        .unwrap();
    let ask = json!({"role_id":owner.role, "responsibility_id":owner.responsibility,
        "publisher":"org_example_a", "reference":reference});
    Fixture {
        _dir: dir,
        app,
        receiver,
        intent,
        ask,
        adoption_id: record.id,
        token: config.tokens.operator,
    }
}

async fn start(app: AppState) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
    (base, task)
}

#[tokio::test]
async fn review_disabled_receiver_must_not_issue_an_invocation_grant() {
    let f = retained_adoption(None);
    assert!(f.app.method_receiver.is_none());
    let (base, server) = start(f.app.clone()).await;
    let (status, _) = call(
        &format!("{base}/method-invocations/authorize"),
        &f.token,
        f.ask,
    )
    .await;
    server.abort();
    // This tests only the mutating/authorization feature switch. Reading the
    // historical record can remain available when the receiver is disabled.
    assert_eq!(status, 503, "disabled receiver still issued a method grant");
}

#[tokio::test]
async fn review_identical_replay_must_not_depend_on_registry_availability() {
    let (registry_url, registry) = serve(
        axum::Router::new().fallback(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
    )
    .await;
    let mut f = retained_adoption(Some(registry_url));
    f.app.method_receiver = Some(f.receiver.clone());
    let (base, server) = start(f.app.clone()).await;
    let (status, body) = call(&format!("{base}/method-adoptions"), &f.token, f.intent).await;
    server.abort();
    registry.abort();
    assert_eq!(status, 200, "stored replay was blocked by registry: {body}");
    assert_eq!(body["id"], f.adoption_id);
}

#[tokio::test]
async fn review_changed_receiver_acceptance_policy_fences_existing_authorization() {
    let mut f = retained_adoption(None);
    f.app.method_receiver = Some(f.receiver.clone());
    let (base, original_server) = start(f.app.clone()).await;
    assert_eq!(
        call(
            &format!("{base}/method-invocations/authorize"),
            &f.token,
            f.ask.clone()
        )
        .await
        .0,
        200,
        "unchanged receiver policy must authorize the retained adoption"
    );
    original_server.abort();

    // Simulate the same persisted organization restarted with its receiver's
    // stronger acceptance policy. Existing evidence is not rewritten.
    let plan: TrialPlan = serde_json::from_slice(
        &std::fs::read(
            Path::new(EVIDENCE)
                .join("live-method-20261024-b-revision-receiver/vs-random-search/plan.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut receiver = (*f.receiver).clone();
    receiver.acceptance_plans[0].plan = plan.reference().unwrap();
    f.app.method_receiver = Some(Arc::new(receiver));
    let (base, changed_server) = start(f.app.clone()).await;
    let (status, body) = call(
        &format!("{base}/method-invocations/authorize"),
        &f.token,
        f.ask,
    )
    .await;
    changed_server.abort();
    assert_eq!(
        status, 409,
        "changed receiver acceptance policy still authorized old evidence: {body}"
    );
}
