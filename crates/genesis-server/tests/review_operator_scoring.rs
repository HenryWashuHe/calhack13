//! Pressure test of the receiving decision's trust in operator scoring. B's
//! retained #116 trial vs seeded random search was Rejected (candidate 3/6,
//! control 4/6). An operator edits the control's scoring metrics so two of its
//! successes read as failures, keeps every hash in the retained records
//! consistent, and asks to adopt. Today the server derives its acceptance from
//! those records without rescoring, so the forged evidence decides.
//! Local HTTP fixtures only; nothing runs.
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_trial::TrialPlan;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::{ObservationId, OrgId, RequirementId};
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{Observation, Owner, Requirement};
use genesis_server::method_adoption::{AcceptancePin, MethodReceiver};
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, serve_org, OrgConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;

const EVIDENCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/invention-lab/evidence"
);

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Flip the first two of the control's successes to failures in its metrics
/// artifact, store it under its new digest and point the evaluation at it.
fn forge_control(trial: &Path) {
    let score = trial.join("control-score");
    let eval_path = score.join("evaluation.json");
    let mut eval: Value = serde_json::from_slice(&std::fs::read(&eval_path).unwrap()).unwrap();
    let old = eval["outcome"]["metrics"]["sha256"]
        .as_str()
        .unwrap()
        .to_string();
    let mut metrics: Value =
        serde_json::from_slice(&std::fs::read(score.join("artifacts").join(&old)).unwrap())
            .unwrap();
    let mut flipped = 0;
    for row in metrics["instances"].as_array_mut().unwrap() {
        if flipped < 2 && row["success"] == true {
            row["success"] = json!(false);
            flipped += 1;
        }
    }
    assert_eq!(flipped, 2);
    let bytes = serde_json::to_vec(&metrics).unwrap();
    let new = hex(&bytes);
    std::fs::write(score.join("artifacts").join(&new), &bytes).unwrap();
    std::fs::remove_file(score.join("artifacts").join(&old)).unwrap();
    eval["outcome"]["metrics"]["sha256"] = json!(new);
    std::fs::write(&eval_path, serde_json::to_vec(&eval).unwrap()).unwrap();
}

async fn serve(router: axum::Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

async fn call(url: &str, token: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn forged_operator_scoring_decides_an_adoption_without_rescoring() {
    let dir = tempfile::tempdir().unwrap();
    let state = RegistryState::open(
        dir.path().join("registry.db"),
        [("a-token".into(), OrgId::new("org_example_a"))].into(),
        vec!["b-token".into()],
    )
    .unwrap();
    let registry_url = serve(registry::router(state)).await;
    let package_json = std::fs::read_to_string(format!(
        "{EVIDENCE}/live-method-20261024-b-revision/method-package.json"
    ))
    .unwrap();
    let envelope: Value = serde_json::from_str(&package_json).unwrap();
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let child = ExactContentRef {
        domain: "genesis".into(),
        schema: "development-method-source/1".into(),
        id: envelope["package"]["id"].as_str().unwrap().into(),
        version: envelope["package"]["version"].as_str().unwrap().into(),
        sha256: "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d".into(),
    };
    let (status, _) = call(
        &format!("{registry_url}/registry/v1/methods"),
        "a-token",
        json!({"reference":child,"package_json":package_json,
            "intent":"Rough-transport cart design method","applicability":["lab family v1"]}),
    )
    .await;
    assert_eq!(status, 201);

    let mut config = OrgConfig::dev("org_example_b", "Receiving organization");
    config.db = Some(dir.path().join("b.sqlite"));
    config.registry_url = Some(registry_url);
    config.registry_token = Some("b-token".into());
    let mut app = build_org(&config, None).unwrap();
    let records = dir.path().join("records");
    let trial = records.join("b-random-search");
    copy_dir(
        &Path::new(&format!(
            "{EVIDENCE}/live-method-20261024-b-revision-receiver"
        ))
        .join("vs-random-search"),
        &trial,
    );
    forge_control(&trial);
    let plan: Value =
        serde_json::from_slice(&std::fs::read(trial.join("plan.json")).unwrap()).unwrap();

    // One role owning one assigned requirement.
    let initial = app.store.current_state().unwrap();
    let boot: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Transport over rough terrain",
        "roles":[{"key":"designer","name":"Designer","charter":"Design carts",
            "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}}],
        "responsibilities":[{"purpose":"Rough transport designs","owner":"designer"}]}))
    .unwrap();
    let now = chrono::Utc::now();
    let (outcome, mission) = bootstrap_mission(&initial, &boot, now).unwrap();
    let mut s = outcome.state;
    let (rsp_id, rsp) = s
        .responsibilities
        .iter()
        .next()
        .map(|(k, v)| (k.clone(), v.clone()))
        .unwrap();
    let Owner::Role(role) = rsp.owner.clone() else {
        panic!()
    };
    let observation = ObservationId::new("gap-0");
    let requirement = RequirementId::new("method-0");
    s.observations.insert(
        observation.clone(),
        Observation {
            observation_id: observation.clone(),
            mission_id: mission.clone(),
            source: "receiver".into(),
            summary: "needs a design method".into(),
            evidence_ref: None,
            observed_at: now,
        },
    );
    s.requirements.insert(
        requirement.clone(),
        Requirement {
            requirement_id: requirement.clone(),
            mission_id: mission.clone(),
            summary: "design method".into(),
            triggering_evidence: vec![observation],
            acceptance: "receiver acceptance trial".into(),
            status: RequirementStatus::Assigned,
            owner: Owner::Role(role.clone()),
        },
    );
    s.responsibilities.get_mut(&rsp_id).unwrap().requirement_id = Some(requirement.clone());
    app.store
        .commit_state(
            initial.revision,
            &s,
            outcome.effects,
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    app.method_receiver = Some(Arc::new(MethodReceiver {
        runtime: field("runtime"),
        protocol: field("protocol"),
        environments: vec![
            serde_json::from_value(plan["development_environment"].clone()).unwrap(),
        ],
        evaluators: vec![serde_json::from_value(plan["context"]["evaluator"].clone()).unwrap()],
        allowed_dependencies: vec![],
        records_root: records,
        acceptance_plans: vec![AcceptancePin {
            requirement_id: requirement.clone(),
            publisher: OrgId::new("org_example_a"),
            plan: serde_json::from_value::<TrialPlan>(plan.clone())
                .unwrap()
                .reference()
                .unwrap(),
        }],
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { serve_org(served, listener).await.unwrap() });

    let (status, decided) = call(
        &format!("{base}/method-adoptions"),
        &config.tokens.operator,
        json!({"client_request_id":"forged","mission_id":mission,"requirement_id":requirement,
            "role_id":role,"responsibility_id":rsp_id,"publisher":"org_example_a","reference":child,
            "decision":{"decision":"adopt","trial":"b-random-search"}}),
    )
    .await;
    // The retained trial was Rejected 3/6 vs 4/6. With the forged control it reads
    // 3/6 vs 2/6 and the server adopts.
    assert_eq!(status, 201, "{decided}");
    assert_eq!(decided["outcome"]["outcome"], "adopted", "{decided}");
    assert_eq!(decided["outcome"]["evidence"]["acceptance"]["control"], 2);
}
