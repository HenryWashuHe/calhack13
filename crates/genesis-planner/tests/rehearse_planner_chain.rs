//! Rehearsal, not part of the suite: B selects A's child through the registry,
//! the intake and a scripted planner give it a specialist owning a new
//! requirement, B runs a fresh receiving trial, only the owner can adopt, and
//! the adopted method runs once under the owner's grant with the real
//! `lab-exec`. Ignored unless run explicitly with REHEARSAL_DIR (the
//! predeclared evidence directory holding `spec.json` and
//! `invocation-template.json`), LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY.
//! Root and the pinned runtime are the caller's business. The planner is a
//! `ReplayClient`: its proposal is scripted, not model judgment.
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_trial::TrialPlan;
use genesis_core::ids::OrgId;
use genesis_planner::{
    agent::{PlanOutcome, Planner},
    kernel::KernelClient,
    llm::ReplayClient,
    runner::Runner,
};
use genesis_server::method_adoption::{AcceptancePin, MethodReceiver};
use genesis_server::registry::{
    methods::{MethodPublication, MethodSearch},
    RegistryClient, RegistryState,
};
use genesis_server::{build_org, serve_org, spawn_registry, OrgConfig};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

const NEED: &str = "Develop rough-transport cart designs with a method another lab published";
const ACCEPTANCE: &str = "Beats the fixed cart by one success on six fresh public instances";
const TRIAL: &str = "rehearse2-accept-vs-fixed-cart";

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("set {name}")))
}

fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

struct Org {
    base: String,
    token: String,
    http: reqwest::Client,
}
impl Org {
    async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

async fn serve(app: genesis_server::app::AppState) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
    url
}

fn run(
    program: &str,
    args: &[&Path],
    envs: &[(&str, &Path)],
    token: Option<&str>,
) -> std::process::Output {
    let mut c = std::process::Command::new(program);
    c.args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    if let Some(t) = token {
        c.env("GENESIS_OPERATOR_TOKEN", t);
    }
    c.output().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "rehearsal: needs REHEARSAL_DIR, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY"]
async fn rehearse_select_plan_receive_adopt_invoke() {
    let dir = env("REHEARSAL_DIR");
    let (lab_exec, lab_score) = (env("LAB_EXEC"), env("LAB_SCORE"));
    let (receive_py, invoke_py) = (env("RECEIVE_PY"), env("INVOKE_PY"));
    let out = dir.join("chain");
    assert!(
        !out.exists(),
        "{} exists: the rehearsal runs once",
        out.display()
    );
    std::fs::create_dir(&out).unwrap();

    // A publishes its child; B finds it by search.
    let registry = RegistryState::open_in_memory(
        [("a-token".into(), OrgId::new("org_example_a"))].into(),
        vec!["b-token".into()],
    )
    .unwrap();
    let registry_url = format!(
        "http://{}",
        spawn_registry(registry, "127.0.0.1:0").await.unwrap()
    );
    let spec: Value =
        serde_json::from_slice(&std::fs::read(dir.join("spec.json")).unwrap()).unwrap();
    let child_path = dir.join(spec["packages"]["child"]["path"].as_str().unwrap());
    let package_json = std::fs::read_to_string(&child_path).unwrap();
    let envelope: Value = serde_json::from_str(&package_json).unwrap();
    let child = ExactContentRef {
        domain: "genesis".into(),
        schema: "development-method-source/1".into(),
        id: envelope["package"]["id"].as_str().unwrap().into(),
        version: envelope["package"]["version"].as_str().unwrap().into(),
        sha256: spec["packages"]["child"]["sha256"].as_str().unwrap().into(),
    };
    RegistryClient::new(&registry_url, "a-token")
        .publish_method(
            &OrgId::new("org_example_a"),
            &MethodPublication {
                reference: child.clone(),
                package_json: package_json.clone(),
                intent: "rough terrain cart design".into(),
                applicability: vec!["lab family v1".into()],
            },
        )
        .await
        .unwrap();
    let found = RegistryClient::new(&registry_url, "b-token")
        .search_methods(&MethodSearch {
            query: "rough terrain".into(),
            limit: 5,
            runtime: None,
            protocol: None,
            exclude_publisher: Some(OrgId::new("org_example_b")),
        })
        .await
        .unwrap();
    write(&out.join("search.json"), &json!(found));
    assert_eq!(found.candidates.len(), 1);

    // B's organization, mission and lab lead.
    let mut cfg = OrgConfig::dev("org_example_b", "Receiving lab");
    cfg.registry_url = Some(registry_url);
    cfg.registry_token = Some("b-token".into());
    cfg.constitution.scopes = ["mission.coordinate", "method.develop"]
        .into_iter()
        .map(String::from)
        .collect();
    let mut app = build_org(&cfg, None).unwrap();
    let url = serve(app.clone()).await;
    let org = Org {
        base: format!("{url}/api/v1"),
        token: cfg.tokens.operator.clone(),
        http: reqwest::Client::new(),
    };
    let (status, mission) = org
        .post(
            "/missions",
            &json!({
                "goal":"Develop physical designs with the best available methods",
                "roles":[{"key":"lead","name":"Lab lead","charter":"Own the lab and evaluate reusable methods",
                    "authority":{"scopes":["mission.coordinate","method.develop"],"budget_cents":10000}}],
                "responsibilities":[{"purpose":"Coordinate the lab","owner":"lead"}]
            }),
        )
        .await;
    assert_eq!(status, 201, "{mission}");
    let mission = mission["mission_id"].as_str().unwrap().to_string();
    let roles = org.get("/organization").await["roles"].clone();
    let lead = roles
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Lab lead")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 1. Intake, then the scripted planner and the kernel.
    let request = json!({"client_request_id":"rehearse2-selection","need":NEED,
        "acceptance":ACCEPTANCE,"selection":found.candidates[0]});
    let (status, intake) = org
        .post(&format!("/missions/{mission}/method-development"), &request)
        .await;
    write(
        &out.join("intake.json"),
        &json!({"status":status,"body":intake}),
    );
    assert_eq!(status, 201, "{intake}");
    let reply = json!({"assessment":"The mission needs continuing, accountable ownership of this method.",
        "verdict":"change","requirement":{"summary":NEED,"acceptance":ACCEPTANCE},
        "rationale":"Assign accountable development work; selection grants no execution rights.",
        "budget_limit_cents":10000,"operations":[
            {"op":"CreateRole","key":"method-owner","name":"Method specialist",
             "charter":"Evaluate and use the selected rough-transport method",
             "authority":{"scopes":["method.develop"],"budget_cents":1000},
             "reports_to":{"kind":"existing","role_id":lead},"provider":"elastic",
             "justification":"independent_ownership"},
            {"op":"AssignResponsibility",
             "target":{"kind":"from_requirement","key":"method-work","requirement_id":"$REQUIREMENT",
                "purpose":NEED,"trigger":"new method evidence","termination":ACCEPTANCE},
             "owner":{"kind":"new","key":"method-owner"}}]})
    .to_string();
    let model = Arc::new(ReplayClient::new([reply]));
    let mut runner = Runner::new(
        Planner::new(KernelClient::new(&url, &cfg.tokens.agent), model.clone()),
        Duration::from_millis(1),
    );
    let outcomes = runner.tick().await.unwrap();
    let PlanOutcome::Proposed { change, .. } = &outcomes[0].1 else {
        panic!("expected a scripted proposal: {outcomes:?}");
    };
    let mut committed = Value::Null;
    for _ in 0..200 {
        committed = org
            .get(&format!("/org-changes/{}", change.change_id.as_str()))
            .await;
        if ["COMMITTED", "FAILED", "REJECTED"].contains(&committed["status"].as_str().unwrap_or(""))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    write(&out.join("org-change.json"), &committed);
    assert_eq!(committed["status"], "COMMITTED", "{committed}");
    let state = org.get("/organization").await;
    let requirement = state["requirements"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    let requirement_id = requirement["requirement_id"].as_str().unwrap().to_string();
    let owner = requirement["owner"]["id"].as_str().unwrap().to_string();
    assert_ne!(
        owner, lead,
        "the scripted change creates a specialist owner"
    );
    let responsibility = state["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["requirement_id"] == requirement["requirement_id"])
        .unwrap()["responsibility_id"]
        .as_str()
        .unwrap()
        .to_string();
    let lead_responsibility = state["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["owner"]["id"] == lead.as_str())
        .unwrap()["responsibility_id"]
        .as_str()
        .unwrap()
        .to_string();
    write(&out.join("organization-after-change.json"), &state);

    // 2. Only now, B's receiving trial.
    let trial_out = dir.join("trial");
    let received = run(
        "python3",
        &[
            Path::new("-I"),
            &receive_py,
            &dir.join("spec.json"),
            &trial_out,
        ],
        &[("LAB_EXEC", &lab_exec), ("LAB_SCORE", &lab_score)],
        None,
    );
    std::fs::write(out.join("receive.stdout"), &received.stdout).unwrap();
    std::fs::write(out.join("receive.stderr"), &received.stderr).unwrap();
    assert!(
        received.status.success(),
        "{}",
        String::from_utf8_lossy(&received.stderr)
    );
    let trial_dir = trial_out.join(TRIAL);
    let plan: Value =
        serde_json::from_slice(&std::fs::read(trial_dir.join("plan.json")).unwrap()).unwrap();
    let verdict: Value =
        serde_json::from_slice(&std::fs::read(trial_dir.join("verdict.json")).unwrap()).unwrap();

    // 3. The receiver pins that plan for the planner-created requirement.
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    app.method_receiver = Some(Arc::new(MethodReceiver {
        runtime: field("runtime"),
        protocol: field("protocol"),
        environments: vec![
            serde_json::from_value(plan["development_environment"].clone()).unwrap(),
        ],
        evaluators: vec![serde_json::from_value(plan["context"]["evaluator"].clone()).unwrap()],
        allowed_dependencies: vec![],
        records_root: trial_out.clone(),
        acceptance_plans: vec![AcceptancePin {
            requirement_id: requirement_id.as_str().into(),
            publisher: OrgId::new("org_example_a"),
            plan: serde_json::from_value::<TrialPlan>(plan.clone())
                .unwrap()
                .reference()
                .unwrap(),
        }],
    }));
    let url = serve(app.clone()).await;
    let org = Org {
        base: format!("{url}/api/v1"),
        ..org
    };
    let intent = |key: &str, role: &str, resp: &str, decision: Value| {
        json!({"client_request_id":key,"mission_id":mission,"requirement_id":requirement_id,
            "role_id":role,"responsibility_id":resp,"publisher":"org_example_a",
            "reference":child,"decision":decision})
    };
    let adopt = json!({"decision":"adopt","trial":TRIAL});
    // Existing-owner control: the lab lead isn't the requirement's owner.
    let (status, body) = org
        .post(
            "/method-adoptions",
            &intent("lead-adopts", &lead, &responsibility, adopt.clone()),
        )
        .await;
    let (status2, body2) = org
        .post(
            "/method-adoptions",
            &intent("lead-own-work", &lead, &lead_responsibility, adopt.clone()),
        )
        .await;
    write(
        &out.join("lead-attempts.json"),
        &json!([{"status":status,"body":body},{"status":status2,"body":body2}]),
    );
    assert!(
        status >= 400 && status2 >= 400,
        "the lead must not adopt: {status} {status2}"
    );

    let accepted = verdict["verdict"] == "accepted";
    let decision = if accepted {
        adopt
    } else {
        json!({"decision":"decline","reason":"the receiving trial did not accept it"})
    };
    let (status, adopted) = org
        .post(
            "/method-adoptions",
            &intent("owner-decides", &owner, &responsibility, decision),
        )
        .await;
    write(
        &out.join("owner-decision.json"),
        &json!({"status":status,"body":adopted}),
    );
    assert_eq!(status, 201, "{adopted}");
    if !accepted {
        return;
    }
    assert_eq!(adopted["outcome"]["outcome"], "adopted");

    // 4. One invocation under the owner's grant.
    let ask = json!({"role_id":owner,"responsibility_id":responsibility,
        "publisher":"org_example_a","reference":child});
    let (status, current) = org.post("/method-invocations/authorize", &ask).await;
    write(
        &out.join("authorization.json"),
        &json!({"status":status,"body":current}),
    );
    assert_eq!(status, 200, "{current}");
    let template: Value =
        serde_json::from_slice(&std::fs::read(dir.join("invocation-template.json")).unwrap())
            .unwrap();
    let mut invocation = plan.clone();
    invocation["id"] = json!("invoke-rehearsal2-20261025");
    invocation["development_instances"] = template["development_instances"].clone();
    invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
    let mut slot = invocation["slots"][0].clone();
    for (k, v) in [
        ("slot_id", "use"),
        ("run_id", "run-invoke-rehearsal2-20261025"),
        ("attempt_id", "att_invoke_rehearsal2_20261025"),
        ("scoring_request_id", "score-invoke-rehearsal2-20261025"),
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
    let token = cfg.tokens.operator.clone();
    let (base, rq, pl, rs, pkg, le, ip) = (
        org.base.clone(),
        out.join("proposed-request.json"),
        out.join("proposed-plan.json"),
        runs.clone(),
        trial_out.join("packages/child.package.json"),
        lab_exec.clone(),
        invoke_py.clone(),
    );
    let (prepared, ran) = tokio::task::spawn_blocking(move || {
        let p = std::process::Command::new("python3")
            .arg("-I")
            .arg(&ip)
            .args([
                "prepare",
                "--org",
                &base,
                "--key",
                "rehearse2-run",
                "--request",
            ])
            .arg(&rq)
            .arg("--plan")
            .arg(&pl)
            .arg("--out")
            .arg(&rs)
            .env("GENESIS_OPERATOR_TOKEN", &token)
            .output()
            .unwrap();
        let r = std::process::Command::new("python3")
            .arg("-I")
            .arg(&ip)
            .arg("run")
            .arg(&rs)
            .arg("--package")
            .arg(&pkg)
            .arg("--lab-exec")
            .arg(&le)
            .args(["--sandbox", "namespaces"])
            .env("GENESIS_OPERATOR_TOKEN", &token)
            .output()
            .unwrap();
        (p, r)
    })
    .await
    .unwrap();
    for (name, o) in [("prepare", &prepared), ("run", &ran)] {
        std::fs::write(out.join(format!("{name}.stdout")), &o.stdout).unwrap();
        std::fs::write(out.join(format!("{name}.stderr")), &o.stderr).unwrap();
    }
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let id = serde_json::from_slice::<Value>(&prepared.stdout).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let record = org.get(&format!("/method-runs/{id}")).await;
    write(&out.join("run-record.json"), &record);
    assert_eq!(record["status"], "completed", "{record}");
}
