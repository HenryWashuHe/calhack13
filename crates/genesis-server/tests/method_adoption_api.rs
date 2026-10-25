//! Method adoption and the invocation check through real organization and
//! registry HTTP services. B selects A's published child method (`4f455dc6…`)
//! and adopts it from B's own retained #116 acceptance trial. Nothing runs:
//! the authorization check is not execution, and no provider is called.
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialPlan;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::{ObservationId, OrgId, RequirementId, ResponsibilityId, RoleId};
use genesis_core::lifecycle::{RequirementStatus, RoleState};
use genesis_core::model::{Observation, OrgState, Owner, Requirement};
use genesis_server::app::AppState;
use genesis_server::method_adoption::{AcceptancePin, MethodReceiver};
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, serve_org, OrgConfig};
use serde_json::{json, Value};
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

/// Applies one direct state change as the next revision.
fn mutate(app: &AppState, change: impl FnOnce(&mut OrgState)) {
    let mut state = app.store.current_state().unwrap();
    let base = state.revision;
    state.revision += 1;
    change(&mut state);
    app.store
        .commit_state(base, &state, vec![], &EventMeta::default(), vec![], |_| {
            Ok(())
        })
        .unwrap();
}

struct Org {
    role: RoleId,
    responsibility: ResponsibilityId,
    requirement: RequirementId,
}

/// Two roles, each owning one responsibility with its own assigned requirement.
fn bootstrap(app: &AppState) -> (genesis_core::ids::MissionId, [Org; 2]) {
    bootstrap_with(
        app,
        &["capability.venue_lookup"],
        &["capability.venue_lookup"],
    )
}

/// As [`bootstrap`], with each role's scopes given.
fn bootstrap_with(
    app: &AppState,
    designer_scopes: &[&str],
    other_scopes: &[&str],
) -> (genesis_core::ids::MissionId, [Org; 2]) {
    let initial = app.store.current_state().unwrap();
    let plan: MissionBootstrap = serde_json::from_value(json!({
        "goal":"Transport over rough terrain",
        "roles":[
            {"key":"designer","name":"Designer","charter":"Design carts",
             "authority":{"scopes":designer_scopes,"budget_cents":100}},
            {"key":"other","name":"Other","charter":"Other work",
             "authority":{"scopes":other_scopes,"budget_cents":100}}],
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

fn publication(package_json: &str, reference: &ExactContentRef) -> Value {
    json!({"reference":reference,"package_json":package_json,
        "intent":"Rough-transport cart design method","applicability":["lab family v1"]})
}

#[tokio::test]
async fn adoption_authorizes_only_the_owner_for_the_exact_version_under_current_authority() {
    let dir = tempfile::tempdir().unwrap();
    let state = RegistryState::open(
        dir.path().join("registry.db"),
        [
            ("a-token".into(), OrgId::new("org_example_a")),
            ("b-token".into(), OrgId::new("org_example_b")),
        ]
        .into(),
        vec![],
    )
    .unwrap();
    let (registry_url, _registry) = serve(registry::router(state)).await;
    let v1_json = std::fs::read_to_string(format!(
        "{EVIDENCE}/live-method-20261024-b-revision/method-package.json"
    ))
    .unwrap();
    let envelope: Value = serde_json::from_str(&v1_json).unwrap();
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let v1 = ExactContentRef {
        domain: "genesis".into(),
        schema: "development-method-source/1".into(),
        id: envelope["package"]["id"].as_str().unwrap().into(),
        version: envelope["package"]["version"].as_str().unwrap().into(),
        sha256: "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d".into(),
    };
    let v2_package = MethodPackage::new(
        v1.id.clone(),
        "2".into(),
        field("runtime"),
        field("protocol"),
        envelope["package"]["entrypoint"].as_str().unwrap().into(),
        vec![],
        vec![MethodSourceFile {
            path: envelope["package"]["entrypoint"].as_str().unwrap().into(),
            content: "# inert newer version; never executed\n".into(),
        }],
    )
    .unwrap();
    let (v2_json, v2) = (
        v2_package.artifact_json().unwrap(),
        v2_package.reference().unwrap(),
    );
    let publish = format!("{registry_url}/registry/v1/methods");
    assert_eq!(
        call(&publish, "a-token", publication(&v1_json, &v1))
            .await
            .0,
        201
    );

    let mut config = OrgConfig::dev("org_example_b", "Receiving organization");
    config.db = Some(dir.path().join("b.sqlite"));
    config.registry_url = Some(registry_url.clone());
    config.registry_token = Some("b-token".into());
    let mut app = build_org(&config, None).unwrap();
    let records = dir.path().join("records");
    let receiving = format!("{EVIDENCE}/live-method-20261024-b-revision-receiver");
    copy_dir(
        &Path::new(&receiving).join("vs-fixed-cart"),
        &records.join("b-fixed-cart"),
    );
    copy_dir(
        &Path::new(&receiving).join("vs-random-search"),
        &records.join("b-random-search"),
    );
    let plan: Value =
        serde_json::from_slice(&std::fs::read(records.join("b-fixed-cart/plan.json")).unwrap())
            .unwrap();
    let (mission, [owner, other]) = bootstrap(&app);
    let unconfigured = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", unconfigured.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { serve_org(served, unconfigured).await.unwrap() });
    let token = config.tokens.operator.clone();
    let intent = |key: &str, org: &Org, reference: &ExactContentRef, decision: Value| {
        json!({"client_request_id":key,"mission_id":mission,"requirement_id":org.requirement,
            "role_id":org.role,"responsibility_id":org.responsibility,
            "publisher":"org_example_a","reference":reference,"decision":decision})
    };
    let adopt = json!({"decision":"adopt","trial":"b-fixed-cart"});
    // Without a configured receiver nothing is decided.
    let (status, _) = call(
        &format!("{base}/method-adoptions"),
        &token,
        intent("k0", &owner, &v1, adopt.clone()),
    )
    .await;
    assert_eq!(status, 503);

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
    let adoptions = format!("{base}/method-adoptions");
    let authorize = format!("{base}/method-invocations/authorize");
    let ask = |org: &Org, reference: &ExactContentRef| {
        json!({"role_id":org.role,"responsibility_id":org.responsibility,
            "publisher":"org_example_a","reference":reference})
    };

    // A retained trial other than the requirement's pinned plan can't decide.
    let other_trial = json!({"decision":"adopt","trial":"b-random-search"});
    let (status, body) = call(&adoptions, &token, intent("k9", &owner, &v1, other_trial)).await;
    assert_eq!(
        (status, body["error"].clone()),
        (422, json!("method_acceptance"))
    );
    // A request can't smuggle receipts or a verdict.
    let mut smuggled = intent("k1", &owner, &v1, adopt.clone());
    smuggled["verdict"] = json!({"verdict":"accepted","candidate":6,"control":0,"of":6});
    assert_eq!(call(&adoptions, &token, smuggled).await.0, 400);

    let (status, adopted) =
        call(&adoptions, &token, intent("k1", &owner, &v1, adopt.clone())).await;
    assert_eq!(status, 201, "{adopted}");
    assert_eq!(adopted["outcome"]["outcome"], "adopted");
    assert_eq!(
        adopted["outcome"]["evidence"]["acceptance"]["acceptance"],
        "accepted"
    );

    // The owner is authorized for exactly v1.
    let (status, grant) = call(&authorize, &token, ask(&owner, &v1)).await;
    assert_eq!(status, 200, "{grant}");
    assert_eq!(grant["adoption_id"], adopted["id"]);
    // Existing-owner control: the other role has a valid grant of its own and
    // explicitly declined, so it is not authorized.
    let decline = json!({"decision":"decline","reason":"keep our own method"});
    let (status, declined) = call(&adoptions, &token, intent("k2", &other, &v1, decline)).await;
    assert_eq!(
        (status, declined["outcome"]["outcome"].clone()),
        (201, json!("declined"))
    );
    assert_eq!(call(&authorize, &token, ask(&other, &v1)).await.0, 409);
    // Naming the adopted responsibility with another role, or the owner with
    // another responsibility, is not the adopted binding.
    for (role, responsibility) in [
        (&other.role, &owner.responsibility),
        (&owner.role, &other.responsibility),
    ] {
        let mixed = json!({"role_id":role,"responsibility_id":responsibility,
            "publisher":"org_example_a","reference":v1});
        assert_eq!(call(&authorize, &token, mixed).await.0, 409);
    }
    // A newer publication never upgrades B.
    assert_eq!(
        call(&publish, "a-token", publication(&v2_json, &v2))
            .await
            .0,
        201
    );
    assert_eq!(
        call(&authorize, &token, ask(&owner, &v2)).await.1["error"],
        "method_not_adopted"
    );
    assert_eq!(call(&authorize, &token, ask(&owner, &v1)).await.0, 200);

    // Authorization follows current state: a changed assignment epoch fences
    // it, and only the identical current grant authorizes again.
    let assignment = adopted["grant"]["assignment_id"]
        .as_str()
        .unwrap()
        .to_string();
    mutate(&app, |s| {
        let a = s
            .assignments
            .iter_mut()
            .find(|(id, _)| id.as_str() == assignment)
            .unwrap()
            .1;
        a.epoch += 1;
    });
    let (status, body) = call(&authorize, &token, ask(&owner, &v1)).await;
    assert_eq!(
        (status, body["error"].clone()),
        (409, json!("method_authority"))
    );
    mutate(&app, |s| {
        let a = s
            .assignments
            .iter_mut()
            .find(|(id, _)| id.as_str() == assignment)
            .unwrap()
            .1;
        a.epoch -= 1;
    });
    assert_eq!(call(&authorize, &token, ask(&owner, &v1)).await.0, 200);
    // Running the adopted method under the grant: the operator proposes a
    // one-slot invocation plan, the server freezes it, starts it once and
    // records the receipt. The receipt is #116's own, rebound; nothing runs.
    let (_, current) = call(&authorize, &token, ask(&owner, &v1)).await;
    let mut invocation = plan.clone();
    invocation["id"] = json!("invoke-rough-transport");
    let mut slot = invocation["slots"][0].clone();
    for (k, v) in [
        ("slot_id", "use"),
        ("run_id", "run-invoke-use"),
        ("attempt_id", "att_invoke_use"),
        ("scoring_request_id", "score-invoke-use"),
    ] {
        slot[k] = json!(v);
    }
    let g = &current["grant"];
    slot["purpose"] = json!({"kind":"invocation","grant":{"adoption_id":current["adoption_id"],
        "requirement_id":current["requirement_id"],"role_id":g["role_id"],
        "responsibility_id":g["responsibility_id"],"assignment_id":g["assignment_id"],"epoch":g["epoch"]}});
    invocation["slots"] = json!([slot]);
    let run = |key: &str| {
        // One run per exact plan, so each run names its own plan id.
        let mut plan = invocation.clone();
        plan["id"] = json!(format!("invoke-{key}"));
        // ...and its own run, attempt and scoring ids.
        plan["slots"][0]["run_id"] = json!(format!("run-invoke-{key}"));
        plan["slots"][0]["attempt_id"] = json!(format!("att_invoke_{key}"));
        plan["slots"][0]["scoring_request_id"] = json!(format!("score-invoke-{key}"));
        json!({"client_request_id":key,"request":ask(&owner, &v1),"plan":plan})
    };
    let runs = format!("{base}/method-runs");
    let (status, prepared) = call(&runs, &token, run("r1")).await;
    assert_eq!(status, 201, "{prepared}");
    assert_eq!(prepared["status"], "prepared");
    assert_eq!(call(&runs, &token, run("r1")).await.0, 200);
    let id = prepared["id"].as_str().unwrap().to_string();
    let (status, started) = call(&format!("{runs}/{id}/start"), &token, json!(null)).await;
    assert_eq!((status, started["status"].clone()), (200, json!("started")));
    assert_eq!(
        call(&format!("{runs}/{id}/start"), &token, json!(null))
            .await
            .1["error"],
        "method_run_status"
    );
    let original: genesis_core::development_method::ExecutionReceipt = serde_json::from_slice(
        &std::fs::read(
            app.method_receiver
                .as_ref()
                .unwrap()
                .records_root
                .join("b-fixed-cart/candidate/receipt.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let receipt_url = format!("{runs}/{id}/receipt");
    assert_eq!(call(&receipt_url, &token, json!(original)).await.0, 422);
    let frozen: TrialPlan = serde_json::from_value(prepared["plan"].clone()).unwrap();
    let binding = frozen.expected_binding("use").unwrap();
    let mut receipt = original.clone();
    receipt.binding_sha256 = binding.canonical_hash().unwrap();
    receipt.actual_method = binding.method.clone();
    receipt.actual_environment = binding.environment.clone();
    receipt.binding = binding;
    let (status, done) = call(&receipt_url, &token, json!(receipt)).await;
    assert_eq!(
        (status, done["status"].clone()),
        (200, json!("completed")),
        "{done}"
    );
    // Its scored output: #116's candidate evaluation rebound to this run's
    // request. #116's evaluation as retained names #116's run and is refused.
    let score_path = app
        .method_receiver
        .as_ref()
        .unwrap()
        .records_root
        .join("b-fixed-cart/candidate-score/evaluation.json");
    let mut evaluation: genesis_core::development_evaluation::EvaluationReceipt =
        serde_json::from_slice(&std::fs::read(score_path).unwrap()).unwrap();
    let result_url = format!("{runs}/{id}/result");
    assert_eq!(call(&result_url, &token, json!(evaluation)).await.0, 422);
    let request = frozen
        .evaluation_request_for_accounting("use", &receipt)
        .unwrap()
        .unwrap();
    evaluation.request_sha256 = request.identity_hash(&receipt, &receipt.binding).unwrap();
    evaluation.request = request;
    let (status, scored) = call(&result_url, &token, json!(evaluation)).await;
    assert_eq!(status, 200, "{scored}");
    assert_eq!(scored["result"]["attachment"]["kind"], "attached");
    assert_eq!(
        scored["result"]["attachment"]["responsibility_id"],
        json!(owner.responsibility)
    );
    assert_eq!(call(&result_url, &token, json!(evaluation)).await.0, 409);
    // A run prepared under the grant, started after the epoch changed, is
    // fenced and never runs.
    let (_, second) = call(&runs, &token, run("r2")).await;
    let second = second["id"].as_str().unwrap().to_string();
    mutate(&app, |s| {
        s.assignments
            .iter_mut()
            .find(|(id, _)| id.as_str() == assignment)
            .unwrap()
            .1
            .epoch += 1;
    });
    let (status, body) = call(&format!("{runs}/{second}/start"), &token, json!(null)).await;
    assert_eq!(
        (status, body["error"].clone()),
        (409, json!("method_run_status"))
    );
    let fenced: Value = reqwest::Client::new()
        .get(format!("{runs}/{second}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fenced["status"], "fenced");
    mutate(&app, |s| {
        s.assignments
            .iter_mut()
            .find(|(id, _)| id.as_str() == assignment)
            .unwrap()
            .1
            .epoch -= 1;
    });
    // A retired role is never authorized.
    mutate(&app, |s| {
        s.roles.get_mut(&owner.role).unwrap().state = RoleState::Retired
    });
    assert_eq!(call(&authorize, &token, ask(&owner, &v1)).await.0, 409);
}

#[path = "common/review_method_adoption_api.rs"]
mod review_method_adoption_api;

/// g2-attempt-1 found that a role without `method.develop` could adopt and
/// invoke a method in an organization whose constitution reserves that scope.
/// There, adoption and invocation now require it of the deciding role; an
/// organization whose constitution never grants it is unaffected (above).
#[tokio::test]
async fn adoption_requires_method_develop_where_the_constitution_reserves_it() {
    let dir = tempfile::tempdir().unwrap();
    let state = RegistryState::open(
        dir.path().join("registry.db"),
        [("a-token".into(), OrgId::new("org_example_a"))].into(),
        vec!["b-token".into()],
    )
    .unwrap();
    let (registry_url, _registry) = serve(registry::router(state)).await;
    let v1_json = std::fs::read_to_string(format!(
        "{EVIDENCE}/live-method-20261024-b-revision/method-package.json"
    ))
    .unwrap();
    let envelope: Value = serde_json::from_str(&v1_json).unwrap();
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let v1 = ExactContentRef {
        domain: "genesis".into(),
        schema: "development-method-source/1".into(),
        id: envelope["package"]["id"].as_str().unwrap().into(),
        version: envelope["package"]["version"].as_str().unwrap().into(),
        sha256: "4f455dc6e625bdaf554d567bdd7f0bc0c7b601a1ea6332d4e7a13e02005db14d".into(),
    };
    let publish = format!("{registry_url}/registry/v1/methods");
    assert_eq!(
        call(&publish, "a-token", publication(&v1_json, &v1))
            .await
            .0,
        201
    );
    let mut config = OrgConfig::dev("org_example_b", "Receiving organization");
    config.db = Some(dir.path().join("b.sqlite"));
    config.registry_url = Some(registry_url.clone());
    config.registry_token = Some("b-token".into());
    config.constitution.scopes = ["method.develop", "mission.coordinate"]
        .into_iter()
        .map(String::from)
        .collect();
    let mut app = build_org(&config, None).unwrap();
    let records = dir.path().join("records");
    let receiving = format!("{EVIDENCE}/live-method-20261024-b-revision-receiver");
    copy_dir(
        &Path::new(&receiving).join("vs-fixed-cart"),
        &records.join("b-fixed-cart"),
    );
    let plan: Value =
        serde_json::from_slice(&std::fs::read(records.join("b-fixed-cart/plan.json")).unwrap())
            .unwrap();
    // The designer holds method.develop; the other role doesn't. Each owns its
    // own requirement, and both requirements pin the same accepting plan.
    let (mission, [holder, lacking]) =
        bootstrap_with(&app, &["method.develop"], &["mission.coordinate"]);
    let pin = |org: &Org| AcceptancePin {
        requirement_id: org.requirement.clone(),
        publisher: OrgId::new("org_example_a"),
        plan: serde_json::from_value::<TrialPlan>(plan.clone())
            .unwrap()
            .reference()
            .unwrap(),
    };
    app.method_receiver = Some(Arc::new(MethodReceiver {
        runtime: field("runtime"),
        protocol: field("protocol"),
        environments: vec![
            serde_json::from_value(plan["development_environment"].clone()).unwrap(),
        ],
        evaluators: vec![serde_json::from_value(plan["context"]["evaluator"].clone()).unwrap()],
        allowed_dependencies: vec![],
        records_root: records,
        acceptance_plans: vec![pin(&holder), pin(&lacking)],
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { serve_org(served, listener).await.unwrap() });
    let token = config.tokens.operator.clone();
    let intent = |key: &str, org: &Org| {
        json!({"client_request_id":key,"mission_id":mission,"requirement_id":org.requirement,
            "role_id":org.role,"responsibility_id":org.responsibility,
            "publisher":"org_example_a","reference":v1,
            "decision":{"decision":"adopt","trial":"b-fixed-cart"}})
    };
    let adoptions = format!("{base}/method-adoptions");
    let authorize = format!("{base}/method-invocations/authorize");
    let ask = |org: &Org| {
        json!({"role_id":org.role,"responsibility_id":org.responsibility,
            "publisher":"org_example_a","reference":v1})
    };

    // The owner without the scope can't adopt, even with an accepting trial,
    // and so has nothing to invoke.
    let (status, body) = call(&adoptions, &token, intent("lacking", &lacking)).await;
    assert_eq!(
        (status, body["error"].clone()),
        (403, json!("method_scope")),
        "{body}"
    );
    assert_eq!(call(&authorize, &token, ask(&lacking)).await.0, 409);
    // The owner holding it adopts and is authorized.
    let (status, adopted) = call(&adoptions, &token, intent("holder", &holder)).await;
    assert_eq!(
        (status, adopted["outcome"]["outcome"].clone()),
        (201, json!("adopted")),
        "{adopted}"
    );
    assert_eq!(call(&authorize, &token, ask(&holder)).await.0, 200);
}
