//! Rehearsal, not a test of the suite: B adopts A's child from B's own fresh
//! receiving trial, then runs it once under the invocation grant with the real
//! `lab-exec` and `invoke.py`. Ignored unless run explicitly with
//! REHEARSAL_DIR (the predeclared evidence directory, with `trial/` from
//! `receive.py` and `invocation-template.json`), LAB_EXEC and INVOKE_PY set.
//! Root and the pinned runtime are the caller's business. The organization
//! state is bootstrapped through the store seam, not the planner.
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_trial::TrialPlan;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::{ObservationId, OrgId, RequirementId, ResponsibilityId, RoleId};
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{Observation, Owner, Requirement};
use genesis_server::app::AppState;
use genesis_server::method_adoption::{AcceptancePin, MethodReceiver};
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, serve_org, OrgConfig};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
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

async fn serve(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (
        url,
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}

async fn call(url: &str, token: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

struct Org {
    role: RoleId,
    responsibility: ResponsibilityId,
    requirement: RequirementId,
}

/// Two roles, each owning one responsibility with its own assigned requirement.
fn bootstrap(app: &AppState) -> (genesis_core::ids::MissionId, [Org; 2]) {
    let initial = app.store.current_state().unwrap();
    let plan: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Transport over rough terrain",
        "roles":[
            {"key":"designer","name":"Designer","charter":"Design carts",
             "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}},
            {"key":"other","name":"Other","charter":"Other work",
             "authority":{"scopes":["capability.venue_lookup"],"budget_cents":100}}],
        "responsibilities":[
            {"purpose":"Rough transport designs","owner":"designer"},
            {"purpose":"Other designs","owner":"other"}]
    }))
    .unwrap();
    let now = chrono::Utc::now();
    let (outcome, mission) = bootstrap_mission(&initial, &plan, now).unwrap();
    let mut state = outcome.state;
    let mut orgs = vec![];
    for (n, (id, responsibility)) in state.responsibilities.clone().into_iter().enumerate() {
        let Owner::Role(role) = responsibility.owner.clone() else {
            panic!()
        };
        let observation = ObservationId::new(format!("gap-{n}"));
        let requirement = RequirementId::new(format!("method-{n}"));
        state.observations.insert(
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
        state.requirements.insert(
            requirement.clone(),
            Requirement {
                requirement_id: requirement.clone(),
                mission_id: mission.clone(),
                summary: format!("design method {n}"),
                triggering_evidence: vec![observation],
                acceptance: "receiver acceptance trial".into(),
                status: RequirementStatus::Assigned,
                owner: Owner::Role(role.clone()),
            },
        );
        state.responsibilities.get_mut(&id).unwrap().requirement_id = Some(requirement.clone());
        orgs.push(Org {
            role,
            responsibility: id,
            requirement,
        });
    }
    let base = initial.revision;
    app.store
        .commit_state(
            base,
            &state,
            outcome.effects,
            &EventMeta::default(),
            vec![],
            |_| Ok(()),
        )
        .unwrap();
    let designer = state
        .roles
        .iter()
        .find(|(_, r)| r.name == "Designer")
        .unwrap()
        .0
        .clone();
    orgs.sort_by_key(|o| o.role != designer);
    (mission, orgs.try_into().ok().unwrap())
}

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("set {name}")))
}

fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "rehearsal: needs REHEARSAL_DIR, LAB_EXEC and INVOKE_PY"]
async fn rehearse_receive_adopt_invoke() {
    let _ = EVIDENCE;
    let dir_path = env("REHEARSAL_DIR");
    let (lab_exec, invoke_py) = (env("LAB_EXEC"), env("INVOKE_PY"));
    let trial_name = "rehearse-accept-vs-fixed-cart";
    let trial_dir = dir_path.join("trial").join(trial_name);
    let out = dir_path.join("invocation");
    assert!(
        !out.exists(),
        "{} exists: the rehearsal runs once",
        out.display()
    );
    std::fs::create_dir(&out).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let state = RegistryState::open(
        tmp.path().join("registry.db"),
        [
            ("a-token".into(), OrgId::new("org_example_a")),
            ("b-token".into(), OrgId::new("org_example_b")),
        ]
        .into(),
        vec![],
    )
    .unwrap();
    let (registry_url, _registry) = serve(registry::router(state)).await;
    let package_path = dir_path.join("trial/packages/child.package.json");
    let package_json = std::fs::read_to_string(&package_path).unwrap();
    let envelope: Value = serde_json::from_str(&package_json).unwrap();
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let plan: Value =
        serde_json::from_slice(&std::fs::read(trial_dir.join("plan.json")).unwrap()).unwrap();
    let child: ExactContentRef =
        serde_json::from_value(plan["slots"][0]["method"].clone()).unwrap();
    let publish = format!("{registry_url}/registry/v1/methods");
    let publication = json!({"reference":child,"package_json":package_json,
        "intent":"Rough-transport cart design method","applicability":["lab family v1"]});
    let (status, published) = call(&publish, "a-token", publication).await;
    assert_eq!(status, 201, "{published}");

    let mut config = OrgConfig::dev("org_example_b", "Receiving organization");
    config.db = Some(tmp.path().join("b.sqlite"));
    config.registry_url = Some(registry_url.clone());
    config.registry_token = Some("b-token".into());
    let mut app = build_org(&config, None).unwrap();
    let records = tmp.path().join("records");
    copy_dir(&trial_dir, &records.join(trial_name));
    let (mission, [owner, _other]) = bootstrap(&app);
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
            requirement_id: owner.requirement.clone(),
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
    let token = config.tokens.operator.clone();

    let adopt = json!({"client_request_id":"rehearse-adopt","mission_id":mission,
        "requirement_id":owner.requirement,"role_id":owner.role,
        "responsibility_id":owner.responsibility,"publisher":"org_example_a",
        "reference":child,"decision":{"decision":"adopt","trial":trial_name}});
    let (status, adopted) = call(&format!("{base}/method-adoptions"), &token, adopt).await;
    write(
        &out.join("adoption.json"),
        &json!({"status":status,"body":adopted}),
    );
    assert_eq!(status, 201, "{adopted}");
    assert_eq!(adopted["outcome"]["outcome"], "adopted");

    let ask = json!({"role_id":owner.role,"responsibility_id":owner.responsibility,
        "publisher":"org_example_a","reference":child});
    let (status, current) = call(
        &format!("{base}/method-invocations/authorize"),
        &token,
        ask.clone(),
    )
    .await;
    write(
        &out.join("authorization.json"),
        &json!({"status":status,"body":current}),
    );
    assert_eq!(status, 200, "{current}");

    // The invocation plan: the accepted plan's candidate slot alone, under the
    // grant, on the operator's fresh public instances from the template.
    let template: Value =
        serde_json::from_slice(&std::fs::read(dir_path.join("invocation-template.json")).unwrap())
            .unwrap();
    let mut invocation = plan.clone();
    invocation["id"] = json!("invoke-rehearsal-20261025");
    invocation["development_instances"] = template["development_instances"].clone();
    invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
    let mut slot = invocation["slots"][0].clone();
    for (k, v) in [
        ("slot_id", "use"),
        ("run_id", "run-invoke-rehearsal-20261025"),
        ("attempt_id", "att_invoke_rehearsal_20261025"),
        ("scoring_request_id", "score-invoke-rehearsal-20261025"),
    ] {
        slot[k] = json!(v);
    }
    let g = &current["grant"];
    slot["purpose"] = json!({"kind":"invocation","grant":{"adoption_id":current["adoption_id"],
        "requirement_id":current["requirement_id"],"role_id":g["role_id"],
        "responsibility_id":g["responsibility_id"],"assignment_id":g["assignment_id"],"epoch":g["epoch"]}});
    invocation["slots"] = json!([slot]);
    write(&out.join("proposed-request.json"), &ask);
    write(&out.join("proposed-plan.json"), &invocation);

    let runs = out.join("run");
    let script = |args: Vec<String>| {
        let invoke_py = invoke_py.clone();
        let token = token.clone();
        tokio::task::spawn_blocking(move || {
            std::process::Command::new("python3")
                .arg("-I")
                .arg(&invoke_py)
                .args(args)
                .env("GENESIS_OPERATOR_TOKEN", token)
                .output()
                .unwrap()
        })
    };
    let prepared = script(vec![
        "prepare".into(),
        "--org".into(),
        base.clone(),
        "--key".into(),
        "rehearse-run".into(),
        "--request".into(),
        out.join("proposed-request.json").display().to_string(),
        "--plan".into(),
        out.join("proposed-plan.json").display().to_string(),
        "--out".into(),
        runs.display().to_string(),
    ])
    .await
    .unwrap();
    std::fs::write(out.join("prepare.stdout"), &prepared.stdout).unwrap();
    std::fs::write(out.join("prepare.stderr"), &prepared.stderr).unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let ran = script(vec![
        "run".into(),
        runs.display().to_string(),
        "--package".into(),
        package_path.display().to_string(),
        "--lab-exec".into(),
        lab_exec.display().to_string(),
        "--sandbox".into(),
        "namespaces".into(),
    ])
    .await
    .unwrap();
    std::fs::write(out.join("run.stdout"), &ran.stdout).unwrap();
    std::fs::write(out.join("run.stderr"), &ran.stderr).unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let record: Value = reqwest::Client::new()
        .get(format!(
            "{base}/method-runs/{}",
            serde_json::from_slice::<Value>(&prepared.stdout).unwrap()["id"]
                .as_str()
                .unwrap()
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    write(&out.join("run-record.json"), &record);
    assert_eq!(record["status"], "completed", "{record}");
}
