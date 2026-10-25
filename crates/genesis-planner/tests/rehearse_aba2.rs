//! Rehearsal, not part of the suite: the receiving half of `aba-attempt-2`
//! (docs/invention-lab/protocols/aba-attempt-2.md, stages 2–6) with real
//! processes, so the harness exists and has run before anything is frozen.
//! The child `4f455dc6` stands in for `child2`; no generation runs here.
//!
//! B receives the stand-in against its pinned fixed cart and publishes it
//! only if accepted. A finds it, a scripted planner assigns the return work
//! to A's **existing** design role (the existing-owner case), A receives it
//! against its own current method (the parent), A's receiver pins that exact
//! plan, A's owner adopts or declines, and only an adoption runs one
//! invocation with a scored result. The registry, A and B are separate
//! `genesis` processes with their own databases, tokens and receiver files.
//! Ignored unless run with REHEARSAL_DIR (holding b-receive.json,
//! a-receive.json and invocation-template.json), GENESIS_BIN, LAB_EXEC,
//! LAB_SCORE, RECEIVE_PY and INVOKE_PY. Root and the pinned runtime are the
//! caller's business. The planner is a `ReplayClient`: scripted, not a model.
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

const B_PINNED: &str = "rehearse5-b-pinned";
const A_PINNED: &str = "rehearse5-a-pinned";
const A_NEED: &str = "Develop ravine-crossing designs with a method another lab published";
const A_ACCEPTANCE: &str =
    "Beats our current method by one success on twelve fresh public ravine instances";

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

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// The exact reference of a spec package, from its retained bytes.
fn package_ref(dir: &Path, spec: &Value, name: &str) -> (ExactContentRef, String, Value) {
    let path = dir.join(spec["packages"][name]["path"].as_str().unwrap());
    let json = std::fs::read_to_string(&path).unwrap();
    let envelope: Value = serde_json::from_str(&json).unwrap();
    let reference = ExactContentRef {
        domain: "genesis".into(),
        schema: "development-method-source/1".into(),
        id: envelope["package"]["id"].as_str().unwrap().into(),
        version: envelope["package"]["version"].as_str().unwrap().into(),
        sha256: spec["packages"][name]["sha256"].as_str().unwrap().into(),
    };
    (reference, json, envelope)
}

/// One receiving run of `receive.py`; returns (plan, verdict) of `trial`.
fn receive(
    receive_py: &Path,
    spec: &Path,
    out: &Path,
    evidence: &Path,
    tag: &str,
    labs: &[(&str, OsString)],
    trial: &str,
) -> (Value, Value) {
    let r = python(receive_py, &[os(spec), os(out)], labs);
    std::fs::write(evidence.join(format!("{tag}-receive.stdout")), &r.stdout).unwrap();
    std::fs::write(evidence.join(format!("{tag}-receive.stderr")), &r.stderr).unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    (
        read(&out.join(trial).join("plan.json")),
        read(&out.join(trial).join("verdict.json")),
    )
}

/// Runs the scripted planner once against `org` and waits for its change.
async fn plan_once(org: &OrgProc, api: &Http, reply: String, evidence: &Path) -> Value {
    let model = Arc::new(ReplayClient::new([reply]));
    let mut runner = Runner::new(
        Planner::new(
            KernelClient::new(&org.url(), &format!("{}-agent", org.id)),
            model,
        ),
        Duration::from_millis(1),
    );
    let outcomes = runner.tick().await.unwrap();
    let PlanOutcome::Proposed { change, .. } = &outcomes[0].1 else {
        panic!("expected a scripted proposal: {outcomes:?}");
    };
    let mut committed = Value::Null;
    for _ in 0..400 {
        committed = api
            .get(&format!("/org-changes/{}", change.change_id.as_str()))
            .await;
        if ["COMMITTED", "FAILED", "REJECTED"].contains(&committed["status"].as_str().unwrap_or(""))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    write(&evidence.join("a-org-change.json"), &committed);
    committed
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "rehearsal: needs REHEARSAL_DIR, GENESIS_BIN, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY"]
async fn rehearse_the_receiving_half_of_aba_attempt_2() {
    let dir = env("REHEARSAL_DIR");
    let bin = env("GENESIS_BIN");
    let (lab_exec, lab_score) = (env("LAB_EXEC"), env("LAB_SCORE"));
    let (receive_py, invoke_py) = (env("RECEIVE_PY"), env("INVOKE_PY"));
    let labs = [("LAB_EXEC", os(&lab_exec)), ("LAB_SCORE", os(&lab_score))];
    let out = dir.join("run");
    assert!(
        !out.exists(),
        "{} exists: the rehearsal runs once",
        out.display()
    );
    std::fs::create_dir(&out).unwrap();
    let state = out.join("state");
    std::fs::create_dir(&state).unwrap();

    let mut constitution = genesis_server::demo_constitution();
    constitution.scopes = ["mission.coordinate", "method.develop"]
        .into_iter()
        .map(String::from)
        .collect();
    let constitution_path = state.join("constitution.json");
    write(&constitution_path, &json!(constitution));

    // Both organizations may publish as themselves; each token reads.
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
                "org_example_a:a-registry,org_example_b:b-registry",
            ])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(out.join("registry.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    wait_up(&registry_url).await;
    let org = |id: &'static str, token: &'static str, log: &str, db: &str| OrgProc {
        id,
        port: free_port(),
        db: state.join(db),
        log: out.join(log),
        registry_token: token,
        constitution: constitution_path.clone(),
    };
    let a = org("org_example_a", "a-registry", "org-a.log", "a.db");
    let b = org("org_example_b", "b-registry", "org-b.log", "b.db");
    let a_proc = a.spawn(&bin, &registry_url, None);
    let _b_proc = b.spawn(&bin, &registry_url, None);
    wait_up(&a.url()).await;
    wait_up(&b.url()).await;
    let (a_api, b_api) = (a.api(), b.api());

    // A's mission and its existing design role, which owns ravine designs.
    let (status, a_mission) = a_api
        .post(
            "/missions",
            &json!({"goal":"Develop ravine-crossing designs with the best available methods",
                "roles":[{"key":"designer","name":"Designer","charter":"Own A's ravine-crossing design work and its methods",
                    "authority":{"scopes":["mission.coordinate","method.develop"],"budget_cents":10000}}],
                "responsibilities":[{"purpose":"Ravine-crossing designs","owner":"designer"}]}),
        )
        .await;
    write(
        &out.join("a-mission.json"),
        &json!({"status":status,"body":a_mission}),
    );
    assert_eq!(status, 201, "{a_mission}");
    let a_mission = a_mission["mission_id"].as_str().unwrap().to_string();
    let designer = a_api.get("/organization").await["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Designer")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Stage 2: B receives the stand-in against its pinned fixed cart.
    let b_spec_path = dir.join("b-receive.json");
    let b_spec = read(&b_spec_path);
    let (b_plan, b_verdict) = receive(
        &receive_py,
        &b_spec_path,
        &dir.join("b-trial"),
        &out,
        "b",
        &labs,
        B_PINNED,
    );
    write(&out.join("b-verdict.json"), &b_verdict);
    let _ = b_plan;
    if b_verdict["verdict"] != "accepted" {
        // The attempt ends at B with a decline; nothing is published.
        return;
    }
    let (stand_in, package_json, envelope) = package_ref(&dir, &b_spec, "child2");
    let b_org = OrgId::new("org_example_b");
    let publication = MethodPublication {
        reference: stand_in.clone(),
        package_json: package_json.clone(),
        intent: "ravine and rough terrain cart design".into(),
        applicability: vec!["lab family v1".into()],
    };
    RegistryClient::new(&registry_url, "b-registry")
        .publish_method(&b_org, &publication)
        .await
        .unwrap();
    // Control: A's token cannot publish as B.
    let forged = RegistryClient::new(&registry_url, "a-registry")
        .publish_method(&b_org, &publication)
        .await;
    write(
        &out.join("a-publish-as-b.json"),
        &json!({"refused": forged.is_err(), "error": forged.as_ref().err().map(|e| format!("{e:?}"))}),
    );
    assert!(forged.is_err(), "A must not publish as B");

    // Stage 3: A finds it with its own token, posts the intake, and its
    // scripted planner gives the work to the existing design role.
    let found = RegistryClient::new(&registry_url, "a-registry")
        .search_methods(&MethodSearch {
            query: "ravine".into(),
            limit: 5,
            runtime: None,
            protocol: None,
            exclude_publisher: Some(OrgId::new("org_example_a")),
        })
        .await
        .unwrap();
    write(&out.join("a-search.json"), &json!(found));
    assert_eq!(found.candidates.len(), 1);
    let (status, intake) = a_api
        .post(
            &format!("/missions/{a_mission}/method-development"),
            &json!({"client_request_id":"rehearse5-a-selection","need":A_NEED,
                "acceptance":A_ACCEPTANCE,"selection":found.candidates[0]}),
        )
        .await;
    write(
        &out.join("a-intake.json"),
        &json!({"status":status,"body":intake}),
    );
    assert_eq!(status, 201, "{intake}");
    let reply = json!({"assessment":"A's existing design role already owns ravine work and has the authority to evaluate this method.",
        "verdict":"change","requirement":{"summary":A_NEED,"acceptance":A_ACCEPTANCE},
        "rationale":"Reuse the existing owner; a selection grants no execution rights.",
        "budget_limit_cents":10000,"operations":[
            {"op":"AssignResponsibility",
             "target":{"kind":"from_requirement","key":"method-work","requirement_id":"$REQUIREMENT",
                "purpose":A_NEED,"trigger":"new method evidence","termination":A_ACCEPTANCE},
             "owner":{"kind":"existing","role_id":designer}}]})
    .to_string();
    let committed = plan_once(&a, &a_api, reply, &out).await;
    assert_eq!(committed["status"], "COMMITTED", "{committed}");
    let a_state = a_api.get("/organization").await;
    write(&out.join("a-organization-after-change.json"), &a_state);
    let requirement = a_state["requirements"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    let requirement_id = requirement["requirement_id"].as_str().unwrap().to_string();
    let owner = requirement["owner"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        owner, designer,
        "the existing design role owns the return work"
    );
    assert_eq!(
        a_state["roles"].as_object().unwrap().len(),
        1,
        "no specialist was created"
    );
    let responsibility = a_state["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["requirement_id"] == requirement["requirement_id"])
        .unwrap()["responsibility_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Stage 4: A receives it against its own current method (the parent).
    let a_spec_path = dir.join("a-receive.json");
    let a_out = dir.join("a-trial");
    let (a_plan, a_verdict) = receive(
        &receive_py,
        &a_spec_path,
        &a_out,
        &out,
        "a",
        &labs,
        A_PINNED,
    );
    write(&out.join("a-verdict.json"), &a_verdict);
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let receiver = MethodReceiver {
        runtime: field("runtime"),
        protocol: field("protocol"),
        environments: vec![
            serde_json::from_value(a_plan["development_environment"].clone()).unwrap(),
        ],
        evaluators: vec![serde_json::from_value(a_plan["context"]["evaluator"].clone()).unwrap()],
        allowed_dependencies: vec![],
        records_root: a_out.clone(),
        acceptance_plans: vec![AcceptancePin {
            requirement_id: requirement_id.as_str().into(),
            publisher: b_org.clone(),
            plan: serde_json::from_value::<TrialPlan>(a_plan.clone())
                .unwrap()
                .reference()
                .unwrap(),
        }],
    };
    let receiver_path = state.join("a-receiver.json");
    write(&receiver_path, &json!(receiver));
    // Stop and reap the old listener before starting its replacement on the
    // same port. Spawning inside mem::replace races the old process's exit.
    drop(a_proc);
    let _a_proc = a.spawn(&bin, &registry_url, Some(&receiver_path));
    wait_up(&a.url()).await;

    // Stage 5: A's owner decides; B's operator token is refused on A.
    let intent = |key: &str, decision: Value| {
        json!({"client_request_id":key,"mission_id":a_mission,"requirement_id":requirement_id,
            "role_id":owner,"responsibility_id":responsibility,"publisher":"org_example_b",
            "reference":stand_in,"decision":decision})
    };
    let adopt = json!({"decision":"adopt","trial":A_PINNED});
    let b_on_a = Http {
        base: a_api.base.clone(),
        token: "org_example_b-operator".into(),
        http: reqwest::Client::new(),
    };
    let (status_b, body_b) = b_on_a
        .post("/method-adoptions", &intent("b-in-a", adopt.clone()))
        .await;
    write(
        &out.join("b-token-on-a.json"),
        &json!({"status":status_b,"body":body_b}),
    );
    assert!(
        status_b == 401 || status_b == 403,
        "B's token must not act in A: {status_b}"
    );
    let accepted = a_verdict["verdict"] == "accepted";
    let decision = if accepted {
        adopt
    } else {
        json!({"decision":"decline","reason":"the receiving trial against our current method did not accept it"})
    };
    let (status, decided) = a_api
        .post("/method-adoptions", &intent("a-owner-decides", decision))
        .await;
    write(
        &out.join("a-decision.json"),
        &json!({"status":status,"body":decided}),
    );
    assert_eq!(status, 201, "{decided}");
    if accepted {
        // Stage 6: one invocation under the existing owner's grant.
        let ask = json!({"role_id":owner,"responsibility_id":responsibility,
            "publisher":"org_example_b","reference":stand_in});
        let (status, current) = a_api.post("/method-invocations/authorize", &ask).await;
        write(
            &out.join("a-authorization.json"),
            &json!({"status":status,"body":current}),
        );
        assert_eq!(status, 200, "{current}");
        let template = read(&dir.join("invocation-template.json"));
        let mut invocation = a_plan.clone();
        invocation["id"] = json!("invoke-rehearsal5-20261025");
        invocation["development_instances"] = template["development_instances"].clone();
        invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
        let mut slot = invocation["slots"][0].clone();
        for (k, v) in [
            ("slot_id", "use"),
            ("run_id", "run-invoke-rehearsal5-20261025"),
            ("attempt_id", "att_invoke_rehearsal5_20261025"),
            ("scoring_request_id", "score-invoke-rehearsal5-20261025"),
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
        let runs = out.join("invocation");
        let token = [("GENESIS_OPERATOR_TOKEN", os("org_example_a-operator"))];
        let package = a_out.join("packages/child2.package.json");
        let steps = [
            (
                "prepare",
                vec![
                    os("prepare"),
                    os("--org"),
                    os(&a_api.base),
                    os("--key"),
                    os("rehearse5-run"),
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
                    os(package),
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
        let record = a_api.get(&format!("/method-runs/{id}")).await;
        write(&out.join("a-run-record.json"), &record);
        assert_eq!(record["status"], "completed", "{record}");
    }

    // Each organization's records stay its own.
    let b_state = b_api.get("/organization").await;
    write(&out.join("b-organization-at-end.json"), &b_state);
    assert!(b_state["requirements"].as_object().unwrap().is_empty());
    assert!(!b_state.to_string().contains(&requirement_id));
}
