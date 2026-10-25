//! Rehearsal, not part of the suite: the registry, organization A and
//! organization B run as three separate `genesis` processes, each with its
//! own database, tokens and (for B) receiver file. A publishes its child with
//! its own registry token. B selects it, a scripted planner gives it a
//! specialist owning a new requirement, B runs a fresh receiving trial,
//! restarts with a receiver pin for that requirement, the owner adopts (the
//! lead is refused), the method runs once under the grant, and its scored
//! output is posted as the run's result. Ignored unless run with
//! REHEARSAL_DIR, GENESIS_BIN, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY.
//! The planner is a `ReplayClient` (scripted), driven over HTTP against B's
//! process. Root and the pinned runtime are the caller's business.
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
    RegistryClient,
};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::net::TcpListener as StdListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const NEED: &str = "Develop rough-transport cart designs with a method another lab published";
const ACCEPTANCE: &str = "Beats the fixed cart by one success on six fresh public instances";
const TRIAL: &str = "rehearse3-accept-vs-fixed-cart";

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("set {name}")))
}

fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn free_port() -> u16 {
    StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A child process that is killed when dropped.
struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Http {
    base: String,
    token: String,
    http: reqwest::Client,
}
impl Http {
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

async fn wait_up(addr: &str) {
    let addr = addr.trim_start_matches("http://");
    for _ in 0..400 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{addr} did not come up");
}

struct OrgProc {
    id: &'static str,
    port: u16,
    db: PathBuf,
    log: PathBuf,
    registry_token: &'static str,
    constitution: PathBuf,
}
impl OrgProc {
    fn spawn(&self, bin: &Path, registry: &str, receiver: Option<&Path>) -> Proc {
        let mut c = Command::new(bin);
        c.arg("org")
            .args(["--org-id", self.id, "--name", self.id])
            .arg("--db")
            .arg(&self.db)
            .args(["--bind", &format!("127.0.0.1:{}", self.port)])
            .arg("--constitution")
            .arg(&self.constitution)
            .args([
                "--registry-url",
                registry,
                "--registry-token",
                self.registry_token,
            ])
            .args(["--operator-token", &format!("{}-operator", self.id)])
            .args(["--agent-token", &format!("{}-agent", self.id)])
            .args(["--adapter-token", &format!("{}-adapter", self.id)])
            .arg("--dev-stub")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdout(Stdio::null())
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log)
                    .unwrap(),
            );
        if let Some(r) = receiver {
            c.arg("--method-receiver").arg(r);
        }
        Proc(c.spawn().unwrap())
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    fn api(&self) -> Http {
        Http {
            base: format!("{}/api/v1", self.url()),
            token: format!("{}-operator", self.id),
            http: reqwest::Client::new(),
        }
    }
}

fn python(script: &Path, args: &[OsString], envs: &[(&str, OsString)]) -> std::process::Output {
    let mut c = Command::new("python3");
    c.arg("-I").arg(script).args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn os<T: Into<OsString>>(v: T) -> OsString {
    v.into()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "rehearsal: needs REHEARSAL_DIR, GENESIS_BIN, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY"]
async fn rehearse_two_organizations_in_separate_processes() {
    let dir = env("REHEARSAL_DIR");
    let bin = env("GENESIS_BIN");
    let (lab_exec, lab_score) = (env("LAB_EXEC"), env("LAB_SCORE"));
    let (receive_py, invoke_py) = (env("RECEIVE_PY"), env("INVOKE_PY"));
    let out = dir.join("orgs");
    assert!(
        !out.exists(),
        "{} exists: the rehearsal runs once",
        out.display()
    );
    std::fs::create_dir(&out).unwrap();
    let state = out.join("state");
    std::fs::create_dir(&state).unwrap();

    // One constitution file, read separately by each organization process.
    let mut constitution = genesis_server::demo_constitution();
    constitution.scopes = ["mission.coordinate", "method.develop"]
        .into_iter()
        .map(String::from)
        .collect();
    let constitution_path = state.join("constitution.json");
    write(&constitution_path, &json!(constitution));

    // The registry process: A's token may publish as A, B's token only reads.
    let registry_port = free_port();
    let registry_url = format!("http://127.0.0.1:{registry_port}");
    let _registry = Proc(
        Command::new(&bin)
            .arg("registry")
            .arg("--db")
            .arg(state.join("registry.db"))
            .args(["--bind", &format!("127.0.0.1:{registry_port}")])
            .args([
                "--publishers",
                "org_example_a:a-registry",
                "--readers",
                "b-registry",
            ])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(out.join("registry.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    wait_up(&registry_url).await;
    let a = OrgProc {
        id: "org_example_a",
        port: free_port(),
        db: state.join("a.db"),
        log: out.join("org-a.log"),
        registry_token: "a-registry",
        constitution: constitution_path.clone(),
    };
    let b = OrgProc {
        id: "org_example_b",
        port: free_port(),
        db: state.join("b.db"),
        log: out.join("org-b.log"),
        registry_token: "b-registry",
        constitution: constitution_path.clone(),
    };
    let _a_proc = a.spawn(&bin, &registry_url, None);
    let mut b_proc = b.spawn(&bin, &registry_url, None);
    wait_up(&a.url()).await;
    wait_up(&b.url()).await;
    let (a_api, b_api) = (a.api(), b.api());

    // A: its own mission and lead, in its own process and database.
    let (status, a_mission) = a_api
        .post(
            "/missions",
            &json!({"goal":"Develop physical designs",
                "roles":[{"key":"designer","name":"Designer","charter":"Design carts",
                    "authority":{"scopes":["method.develop"],"budget_cents":1000}}],
                "responsibilities":[{"purpose":"Ravine-crossing designs","owner":"designer"}]}),
        )
        .await;
    write(
        &out.join("a-mission.json"),
        &json!({"status":status,"body":a_mission}),
    );
    assert_eq!(status, 201, "{a_mission}");

    // A publishes its child with A's registry token.
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
    let publication = MethodPublication {
        reference: child.clone(),
        package_json: package_json.clone(),
        intent: "rough terrain cart design".into(),
        applicability: vec!["lab family v1".into()],
    };
    let a_org = OrgId::new("org_example_a");
    RegistryClient::new(&registry_url, "a-registry")
        .publish_method(&a_org, &publication)
        .await
        .unwrap();
    // Control: B's reader token cannot publish as A.
    let forged = RegistryClient::new(&registry_url, "b-registry")
        .publish_method(&a_org, &publication)
        .await;
    write(
        &out.join("b-publish-as-a.json"),
        &json!({"refused": forged.is_err(),
        "error": forged.as_ref().err().map(|e| format!("{e:?}"))}),
    );
    assert!(forged.is_err(), "B must not publish as A");

    // B: mission and lead; search with B's own read token.
    let (status, mission) = b_api
        .post(
            "/missions",
            &json!({"goal":"Develop physical designs with the best available methods",
                "roles":[{"key":"lead","name":"Lab lead","charter":"Own the lab and evaluate reusable methods",
                    "authority":{"scopes":["mission.coordinate","method.develop"],"budget_cents":10000}}],
                "responsibilities":[{"purpose":"Coordinate the lab","owner":"lead"}]}),
        )
        .await;
    assert_eq!(status, 201, "{mission}");
    let mission = mission["mission_id"].as_str().unwrap().to_string();
    let lead = b_api.get("/organization").await["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Lab lead")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_string();
    let found = RegistryClient::new(&registry_url, "b-registry")
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
    let request = json!({"client_request_id":"rehearse3-selection","need":NEED,
        "acceptance":ACCEPTANCE,"selection":found.candidates[0]});
    let (status, intake) = b_api
        .post(&format!("/missions/{mission}/method-development"), &request)
        .await;
    write(
        &out.join("intake.json"),
        &json!({"status":status,"body":intake}),
    );
    assert_eq!(status, 201, "{intake}");

    // B's scripted planner, over HTTP against B's process with B's agent token.
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
        Planner::new(KernelClient::new(&b.url(), "org_example_b-agent"), model),
        Duration::from_millis(1),
    );
    let outcomes = runner.tick().await.unwrap();
    let PlanOutcome::Proposed { change, .. } = &outcomes[0].1 else {
        panic!("expected a scripted proposal: {outcomes:?}");
    };
    let mut committed = Value::Null;
    for _ in 0..400 {
        committed = b_api
            .get(&format!("/org-changes/{}", change.change_id.as_str()))
            .await;
        if ["COMMITTED", "FAILED", "REJECTED"].contains(&committed["status"].as_str().unwrap_or(""))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    write(&out.join("org-change.json"), &committed);
    assert_eq!(committed["status"], "COMMITTED", "{committed}");
    let org = b_api.get("/organization").await;
    let requirement = org["requirements"]
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
    let responsibility = org["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["requirement_id"] == requirement["requirement_id"])
        .unwrap()["responsibility_id"]
        .as_str()
        .unwrap()
        .to_string();
    write(&out.join("b-organization-after-change.json"), &org);

    // B's receiving trial, only after that commit.
    let trial_out = dir.join("trial");
    let r = python(
        &receive_py,
        &[os(dir.join("spec.json")), os(&trial_out)],
        &[("LAB_EXEC", os(&lab_exec)), ("LAB_SCORE", os(&lab_score))],
    );
    std::fs::write(out.join("receive.stdout"), &r.stdout).unwrap();
    std::fs::write(out.join("receive.stderr"), &r.stderr).unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let trial_dir = trial_out.join(TRIAL);
    let plan: Value =
        serde_json::from_slice(&std::fs::read(trial_dir.join("plan.json")).unwrap()).unwrap();
    let verdict: Value =
        serde_json::from_slice(&std::fs::read(trial_dir.join("verdict.json")).unwrap()).unwrap();

    // B's operator writes a receiver file pinned to the new requirement, and B
    // restarts with it. A never sees this file.
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let receiver = MethodReceiver {
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
    };
    let receiver_path = state.join("b-receiver.json");
    write(&receiver_path, &json!(receiver));
    drop(std::mem::replace(
        &mut b_proc,
        b.spawn(&bin, &registry_url, Some(&receiver_path)),
    ));
    wait_up(&b.url()).await;

    let intent = |key: &str, role: &str, decision: Value| {
        json!({"client_request_id":key,"mission_id":mission,"requirement_id":requirement_id,
            "role_id":role,"responsibility_id":responsibility,"publisher":"org_example_a",
            "reference":child,"decision":decision})
    };
    let adopt = json!({"decision":"adopt","trial":TRIAL});
    // A's operator token means nothing to B.
    let a_on_b = Http {
        base: b_api.base.clone(),
        token: "org_example_a-operator".into(),
        http: reqwest::Client::new(),
    };
    let (status_a, body_a) = a_on_b
        .post(
            "/method-adoptions",
            &intent("a-adopts-in-b", &owner, adopt.clone()),
        )
        .await;
    let (status, body) = b_api
        .post(
            "/method-adoptions",
            &intent("lead-adopts", &lead, adopt.clone()),
        )
        .await;
    write(
        &out.join("refused-attempts.json"),
        &json!([
        {"who":"A's operator token on B","status":status_a,"body":body_a},
        {"who":"B's lead","status":status,"body":body}]),
    );
    assert!(
        status_a == 401 || status_a == 403,
        "A's token must not act in B: {status_a}"
    );
    assert!(status >= 400, "the lead must not adopt: {status}");
    let accepted = verdict["verdict"] == "accepted";
    let decision = if accepted {
        adopt
    } else {
        json!({"decision":"decline","reason":"the receiving trial did not accept it"})
    };
    let (status, adopted) = b_api
        .post(
            "/method-adoptions",
            &intent("owner-decides", &owner, decision),
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

    // One invocation under the owner's grant, then its scored result.
    let ask = json!({"role_id":owner,"responsibility_id":responsibility,
        "publisher":"org_example_a","reference":child});
    let (status, current) = b_api.post("/method-invocations/authorize", &ask).await;
    write(
        &out.join("authorization.json"),
        &json!({"status":status,"body":current}),
    );
    assert_eq!(status, 200, "{current}");
    let template: Value =
        serde_json::from_slice(&std::fs::read(dir.join("invocation-template.json")).unwrap())
            .unwrap();
    let mut invocation = plan.clone();
    invocation["id"] = json!("invoke-rehearsal3-20261025");
    invocation["development_instances"] = template["development_instances"].clone();
    invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
    let mut slot = invocation["slots"][0].clone();
    for (k, v) in [
        ("slot_id", "use"),
        ("run_id", "run-invoke-rehearsal3-20261025"),
        ("attempt_id", "att_invoke_rehearsal3_20261025"),
        ("scoring_request_id", "score-invoke-rehearsal3-20261025"),
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
    let token = [("GENESIS_OPERATOR_TOKEN", os("org_example_b-operator"))];
    let steps = [
        (
            "prepare",
            vec![
                os("prepare"),
                os("--org"),
                os(&b_api.base),
                os("--key"),
                os("rehearse3-run"),
                os("--request"),
                os(out.join("proposed-request.json")),
                os("--plan"),
                os(out.join("proposed-plan.json")),
                os("--out"),
                os(&runs),
            ],
        ),
        (
            "run",
            vec![
                os("run"),
                os(&runs),
                os("--package"),
                os(trial_out.join("packages/child.package.json")),
                os("--lab-exec"),
                os(&lab_exec),
                os("--sandbox"),
                os("namespaces"),
            ],
        ),
        (
            "score",
            vec![os("score"), os(&runs), os("--lab-score"), os(&lab_score)],
        ),
    ];
    let mut id = String::new();
    for (name, args) in steps {
        let o = python(&invoke_py, &args, &token);
        std::fs::write(out.join(format!("{name}.stdout")), &o.stdout).unwrap();
        std::fs::write(out.join(format!("{name}.stderr")), &o.stderr).unwrap();
        assert!(
            o.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        if name == "prepare" {
            id = serde_json::from_slice::<Value>(&o.stdout).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
        }
    }
    let record = b_api.get(&format!("/method-runs/{id}")).await;
    write(&out.join("run-record.json"), &record);
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(
        record["result"]["attachment"]["kind"], "attached",
        "{record}"
    );

    // A's state is its own: no requirement, role, adoption or run from B.
    let a_state = a_api.get("/organization").await;
    write(&out.join("a-organization-at-end.json"), &a_state);
    assert!(a_state["requirements"].as_object().unwrap().is_empty());
    assert!(!a_state.to_string().contains(&owner));
    let status = a_api
        .http
        .get(format!("{}/method-runs/{id}", a_api.base))
        .bearer_auth(&a_api.token)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    write(&out.join("a-run-lookup.json"), &json!({"status": status}));
    assert_eq!(status, 404, "B's run must not exist in A");
}
