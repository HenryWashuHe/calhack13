//! Admission-only regressions. The executor and scorer paths do not exist,
//! so neither a method nor any child process can run, even on the broken code.
use super::*;
use genesis_server::method_rescore::{MethodReexecutor, MethodRescorer};

async fn request_after(change: impl FnOnce(&AppState, &mut Value)) -> (u16, Value) {
    let dir = tempfile::tempdir().unwrap();
    let registry = RegistryState::open(
        dir.path().join("registry.sqlite"),
        [("publisher-token".into(), OrgId::new("org_example_a"))].into(),
        vec!["receiver-token".into()],
    )
    .unwrap();
    let (registry_url, registry_task) = serve(registry::router(registry)).await;
    let package = std::fs::read_to_string(
        Path::new(EVIDENCE).join("live-method-20261024-b-revision/method-package.json"),
    )
    .unwrap();
    let records = dir.path().join("records");
    copy_dir(
        &Path::new(EVIDENCE).join("live-method-20261024-b-revision-receiver/vs-fixed-cart"),
        &records.join("fixed"),
    );
    let plan: TrialPlan =
        serde_json::from_slice(&std::fs::read(records.join("fixed/plan.json")).unwrap()).unwrap();
    let reference = plan.slots[0].method.clone();
    assert_eq!(
        call(
            &format!("{registry_url}/registry/v1/methods"),
            "publisher-token",
            publication(&package, &reference),
        )
        .await
        .0,
        201
    );
    let mut config = OrgConfig::dev("org_example_b", "Receiver");
    config.db = Some(dir.path().join("org.sqlite"));
    config.registry_url = Some(registry_url);
    config.registry_token = Some("receiver-token".into());
    let mut app = build_org(&config, None).unwrap();
    let (mission, [owner, _]) = bootstrap(&app);
    let envelope: Value = serde_json::from_str(&package).unwrap();
    app.method_receiver = Some(Arc::new(MethodReceiver {
        runtime: serde_json::from_value(envelope["package"]["runtime"].clone()).unwrap(),
        protocol: serde_json::from_value(envelope["package"]["protocol"].clone()).unwrap(),
        environments: vec![plan.development_environment.clone()],
        evaluators: vec![plan.context.evaluator.clone()],
        allowed_dependencies: vec![],
        records_root: records,
        acceptance_plans: vec![AcceptancePin {
            requirement_id: owner.requirement.clone(),
            publisher: OrgId::new("org_example_a"),
            plan: plan.reference().unwrap(),
        }],
    }));
    // Deliberately absent binaries make staging an observable, inert sentinel.
    app.method_reexecutor = Some(Arc::new(MethodReexecutor {
        path: dir.path().join("absent-executor"),
        sha256: "0".repeat(64),
        sandbox: "namespaces".into(),
        packages: dir.path().join("absent-packages"),
        path_env: None,
        timeout_ms: Some(1),
    }));
    app.method_rescorer = Some(Arc::new(MethodRescorer {
        path: dir.path().join("absent-scorer"),
        sha256: "0".repeat(64),
        timeout_ms: Some(1),
    }));
    let mut intent = json!({"client_request_id":"admission-only", "mission_id":mission,
        "requirement_id":owner.requirement, "role_id":owner.role,
        "responsibility_id":owner.responsibility, "publisher":"org_example_a",
        "reference":reference, "decision":{"decision":"adopt", "trial":"fixed"}});
    change(&app, &mut intent);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/api/v1/method-adoptions",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
    let response = call(&url, &config.tokens.operator, intent).await;
    server.abort();
    registry_task.abort();
    response
}

#[tokio::test]
async fn valid_current_owner_reaches_the_inert_executor_sentinel() {
    let (status, body) = request_after(|_, _| {}).await;
    assert_eq!(status, 422, "{body}");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("absent-executor is unreadable"));
}

#[tokio::test]
async fn wrong_role_is_refused_before_executor_staging() {
    let (status, body) = request_after(|_, intent| intent["role_id"] = json!("alien-role")).await;
    assert_eq!(
        (status, body["error"].clone()),
        (409, json!("method_authority")),
        "{body}"
    );
}

#[tokio::test]
async fn wrong_mission_is_refused_before_executor_staging() {
    let (status, body) =
        request_after(|_, intent| intent["mission_id"] = json!("alien-mission")).await;
    assert_eq!(
        (status, body["error"].clone()),
        (409, json!("method_authority")),
        "{body}"
    );
}

#[tokio::test]
async fn expired_mission_is_refused_before_executor_staging() {
    let (status, body) = request_after(|app, intent| {
        let id: genesis_core::ids::MissionId =
            serde_json::from_value(intent["mission_id"].clone()).unwrap();
        mutate(app, |state| {
            state.missions.get_mut(&id).unwrap().deadline =
                Some(chrono::Utc::now() - chrono::Duration::seconds(1))
        });
    })
    .await;
    assert_eq!(
        (status, body["error"].clone()),
        (409, json!("method_authority")),
        "{body}"
    );
}

#[tokio::test]
async fn missing_reserved_scope_is_refused_before_executor_staging() {
    let (status, body) = request_after(|app, _| {
        mutate(app, |state| {
            state.constitution.scopes.insert("method.develop".into());
        });
    })
    .await;
    assert_eq!(
        (status, body["error"].clone()),
        (403, json!("method_scope")),
        "{body}"
    );
}

#[tokio::test]
async fn changed_requirement_owner_is_refused_before_executor_staging() {
    let (status, body) = request_after(|app, intent| {
        let id: RequirementId = serde_json::from_value(intent["requirement_id"].clone()).unwrap();
        mutate(app, |state| {
            state.requirements.get_mut(&id).unwrap().owner = Owner::Role(RoleId::new("alien-owner"))
        });
    })
    .await;
    assert_eq!(
        (status, body["error"].clone()),
        (409, json!("method_authority")),
        "{body}"
    );
}
