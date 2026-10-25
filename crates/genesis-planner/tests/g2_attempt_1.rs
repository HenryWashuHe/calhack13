//! `g2-attempt-1` driver (docs/invention-lab/protocols/g2-attempt-1.md), not
//! part of the suite. A registry, A and two independent B instances (B1, B2)
//! run as separate `genesis` processes with separate databases and tokens.
//! A publishes its generated `parent` and a handwritten `same-cart` offer. Each
//! B searches, posts one intake per offer, and the real `genesis-planner`
//! plans each intake observation with a live model; whatever it decides is
//! retained. For each committed requirement B runs its own receiving trial,
//! restarts pinned to it, the owner adopts or declines, and an adopted method
//! runs once under B's grant and is scored. B1 and B2 differ only in the lead's
//! charter and scopes. Ignored unless run with REHEARSAL_DIR, GENESIS_BIN,
//! PLANNER_BIN, GENESIS_LLM_*, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY.
//! Root and the pinned runtime are the caller's business.
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialPlan;
use genesis_core::ids::OrgId;
use genesis_server::method_adoption::{AcceptancePin, MethodReceiver};
use genesis_server::registry::{
    methods::{MethodPublication, MethodSearch},
    RegistryClient,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::net::TcpListener as StdListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const NEED: &str = "Develop rough-transport cart designs with a method another lab published";
const ACCEPTANCE: &str = "Beats the fixed cart by one success on twelve fresh public instances";
const OFFERS: [&str; 2] = ["parent", "same-cart"];

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("set {name}")))
}

fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn free_port() -> u16 {
    StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A child process that is killed and reaped when dropped.
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
    async fn get_status(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> Value {
        self.get_status(path).await.1
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

/// The non-reuse offer: B's own fixed cart, offered by A as a method of its
/// own. It can't beat the fixed cart by one success.
fn same_cart_package(fixed_cart: &Value) -> MethodPackage {
    let p = &fixed_cart["package"];
    let field = |k: &str| serde_json::from_value::<ExactContentRef>(p[k].clone()).unwrap();
    let source = p["files"][0]["content"].as_str().unwrap().replace(
        "Baseline control: submits one fixed two-wheel cart without experimenting.\n\n\
         Hand-written by Henry's Claude as the acceptance control slot. CONTROL ONLY.",
        "Offered as a method: submits one fixed two-wheel cart without experimenting.\n\n\
         Hand-written by Henry's Claude as g2-attempt-1's non-reuse offer. It submits\n\
         exactly the fixed cart's design, so it cannot beat that cart.",
    );
    assert!(
        source.contains("non-reuse offer"),
        "fixed-cart source changed"
    );
    MethodPackage::new(
        "same-cart".into(),
        "1".into(),
        field("runtime"),
        field("protocol"),
        "same_cart.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "same_cart.py".into(),
            content: source,
        }],
    )
    .unwrap()
}

/// Writes the frozen `same-cart` package before the protocol freezes; the run
/// itself only reads it and checks its sha256 against the spec.
#[test]
#[ignore = "freeze helper: needs REHEARSAL_DIR"]
fn write_same_cart_package() {
    let dir = env("REHEARSAL_DIR");
    let fixed =
        read(&dir.join("../live-method-20261024-b-revision-receiver/fixed-cart.package.json"));
    let package = same_cart_package(&fixed);
    let path = dir.join("same-cart.package.json");
    assert!(!path.exists(), "already written");
    std::fs::write(&path, package.artifact_json().unwrap()).unwrap();
    println!("{}", package.reference().unwrap().sha256);
}

fn offer_ref(dir: &Path, spec: &Value, key: &str) -> (ExactContentRef, String) {
    let entry = &spec["packages"][key];
    let json = std::fs::read_to_string(dir.join(entry["path"].as_str().unwrap())).unwrap();
    let envelope: Value = serde_json::from_str(&json).unwrap();
    let reference = ExactContentRef {
        domain: "genesis".into(),
        schema: "development-method-source/1".into(),
        id: envelope["package"]["id"].as_str().unwrap().into(),
        version: envelope["package"]["version"].as_str().unwrap().into(),
        sha256: entry["sha256"].as_str().unwrap().into(),
    };
    (reference, json)
}

struct Shared {
    dir: PathBuf,
    out: PathBuf,
    bin: PathBuf,
    planner_bin: PathBuf,
    lab_exec: PathBuf,
    lab_score: PathBuf,
    receive_py: PathBuf,
    invoke_py: PathBuf,
    registry_url: String,
    constitution: PathBuf,
    offers: BTreeMap<&'static str, ExactContentRef>,
    llm: Vec<(&'static str, OsString)>,
}

/// One B instance through stages 2–6. Returns its outcome summary.
async fn run_b(s: &Shared, name: &'static str, charter: &str, scopes: &[&str]) -> Value {
    let out = s.out.join(name);
    std::fs::create_dir(&out).unwrap();
    let b = OrgProc {
        id: "org_example_b",
        port: free_port(),
        db: s.out.join("state").join(format!("{name}.db")),
        log: out.join("org.log"),
        registry_token: "b-registry",
        constitution: s.constitution.clone(),
    };
    let b_proc = b.spawn(&s.bin, &s.registry_url, None);
    wait_up(&b.url()).await;
    let api = b.api();
    let founding = json!({"goal":"Develop physical designs with the best available methods",
        "roles":[{"key":"lead","name":"Lab lead","charter":charter,
            "authority":{"scopes":scopes,"budget_cents":10000}}],
        "responsibilities":[{"purpose":"Coordinate the lab","owner":"lead"}]});
    let (status, mission) = api.post("/missions", &founding).await;
    write(
        &out.join("mission.json"),
        &json!({"request":founding,"status":status,"body":mission}),
    );
    assert_eq!(status, 201, "{mission}");
    let mission = mission["mission_id"].as_str().unwrap().to_string();
    let lead = api.get("/organization").await["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == "Lab lead")
        .unwrap()["role_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Stage 2: search with B's read token, one intake per offer.
    let found = RegistryClient::new(&s.registry_url, "b-registry")
        .search_methods(&MethodSearch {
            query: "cart design".into(),
            limit: 10,
            runtime: None,
            protocol: None,
            exclude_publisher: Some(OrgId::new("org_example_b")),
        })
        .await
        .unwrap();
    write(&out.join("search.json"), &json!(found));
    let mut observations = BTreeMap::new();
    for offer in OFFERS {
        let wanted = &s.offers[offer];
        let selection = found
            .candidates
            .iter()
            .find(|c| json!(c)["reference"] == json!(wanted))
            .unwrap_or_else(|| panic!("{offer} not found by search"));
        let request = json!({"client_request_id":format!("{name}-{offer}-selection"),
            "need":NEED,"acceptance":ACCEPTANCE,"selection":selection});
        let (status, intake) = api
            .post(&format!("/missions/{mission}/method-development"), &request)
            .await;
        write(
            &out.join(format!("intake-{offer}.json")),
            &json!({"status":status,"body":intake}),
        );
        assert_eq!(status, 201, "{intake}");
        observations.insert(
            offer,
            intake["observation_id"].as_str().unwrap().to_string(),
        );
    }

    // Stage 3: one live `once` call per intake observation, in offer order.
    let planner = |sub: &str, observation: &str, with_model: bool| {
        let mut c = Command::new(&s.planner_bin);
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(&out)
            .args(["--org-url", &b.url(), "--token", "org_example_b-agent"])
            .args([sub, "--observation", observation]);
        if with_model {
            c.envs(s.llm.iter().map(|(k, v)| (k, v)));
        }
        c.output().unwrap()
    };
    let mut committed: BTreeMap<&str, Value> = BTreeMap::new();
    let mut seen: Vec<String> = vec![];
    for offer in OFFERS {
        let observation = &observations[offer];
        let prompt = planner("prompt", observation, false);
        std::fs::write(
            out.join(format!("planner-prompt-{offer}.txt")),
            &prompt.stdout,
        )
        .unwrap();
        assert!(prompt.status.success());
        let once = planner("once", observation, true);
        std::fs::write(
            out.join(format!("planner-once-{offer}.stdout")),
            &once.stdout,
        )
        .unwrap();
        std::fs::write(
            out.join(format!("planner-once-{offer}.stderr")),
            &once.stderr,
        )
        .unwrap();
        // A non-zero exit is recorded like any other outcome for this offer.
        let mut org = Value::Null;
        let mut new = None;
        for _ in 0..240 {
            org = api.get("/organization").await;
            new = org["requirements"]
                .as_object()
                .unwrap()
                .values()
                .find(|r| !seen.contains(&r["requirement_id"].as_str().unwrap().to_string()))
                .cloned();
            if new.is_some() || !once.status.success() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        write(&out.join(format!("organization-after-{offer}.json")), &org);
        if let Some(r) = new {
            seen.push(r["requirement_id"].as_str().unwrap().to_string());
            let responsibility = org["responsibilities"]
                .as_object()
                .unwrap()
                .values()
                .find(|x| x["requirement_id"] == r["requirement_id"])
                .unwrap()["responsibility_id"]
                .clone();
            committed.insert(
                offer,
                json!({"requirement":r,"responsibility":responsibility}),
            );
        }
    }
    let mut summary = json!({"b":name,"lead":lead,"offers":{}});
    for offer in OFFERS {
        summary["offers"][offer] = match committed.get(offer) {
            Some(c) => json!({"planner":"committed","owner":c["requirement"]["owner"],
                "owner_is_lead": c["requirement"]["owner"]["id"] == json!(lead)}),
            None => json!({"planner":"no requirement"}),
        };
    }
    if committed.is_empty() {
        write(&out.join("summary.json"), &summary);
        return summary;
    }

    // Stage 4: B's receiving trials for the committed offers only.
    let full = read(&s.dir.join(format!("{name}-receive.json")));
    let mut spec = full.clone();
    let keep = |t: &Value| committed.contains_key(t["candidate"].as_str().unwrap());
    spec["trials"] = json!(full["trials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| keep(t))
        .collect::<Vec<_>>());
    // The filtered spec lives in the run directory, so its package paths are
    // made absolute; the bytes are still checked against the same sha256.
    for v in spec["packages"].as_object_mut().unwrap().values_mut() {
        let path = s.dir.join(v["path"].as_str().unwrap());
        v["path"] = json!(path.canonicalize().unwrap());
    }
    let spec_path = out.join("receive.json");
    write(&spec_path, &spec);
    let trial_out = out.join("trial");
    let r = python(
        &s.receive_py,
        &[os(&spec_path), os(&trial_out)],
        &[
            ("LAB_EXEC", os(&s.lab_exec)),
            ("LAB_SCORE", os(&s.lab_score)),
        ],
    );
    std::fs::write(out.join("receive.stdout"), &r.stdout).unwrap();
    std::fs::write(out.join("receive.stderr"), &r.stderr).unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let pinned = |offer: &str| {
        format!(
            "{name}-{}-pinned",
            if offer == "parent" { "parent" } else { "same" }
        )
    };
    let mut pins = vec![];
    let mut first_plan = Value::Null;
    for offer in committed.keys() {
        let trial = trial_out.join(pinned(offer));
        let plan = read(&trial.join("plan.json"));
        let verdict = read(&trial.join("verdict.json"));
        summary["offers"][*offer]["trial"] = json!({"id":pinned(offer),"candidate":verdict["candidate"],
            "control":verdict["control"],"verdict":verdict["verdict"],"reason":verdict["reason"]});
        let random = trial_out.join(pinned(offer).replace("-pinned", "-vs-random"));
        if let Ok(bytes) = std::fs::read(random.join("verdict.json")) {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            summary["offers"][*offer]["reported_vs_random"] = json!({"candidate":v["candidate"],"control":v["control"],"verdict":v["verdict"],"reason":v["reason"]});
        }
        pins.push(AcceptancePin {
            requirement_id: committed[offer]["requirement"]["requirement_id"]
                .as_str()
                .unwrap()
                .into(),
            publisher: OrgId::new("org_example_a"),
            plan: serde_json::from_value::<TrialPlan>(plan.clone())
                .unwrap()
                .reference()
                .unwrap(),
        });
        first_plan = plan;
    }
    let envelope = read(&trial_out.join("packages/parent.package.json"));
    let field = |k: &str| {
        serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone()).unwrap()
    };
    let receiver = MethodReceiver {
        runtime: field("runtime"),
        protocol: field("protocol"),
        environments: vec![
            serde_json::from_value(first_plan["development_environment"].clone()).unwrap(),
        ],
        evaluators: vec![
            serde_json::from_value(first_plan["context"]["evaluator"].clone()).unwrap(),
        ],
        allowed_dependencies: vec![],
        records_root: trial_out.clone(),
        acceptance_plans: pins,
    };
    let receiver_path = out.join("receiver.json");
    write(&receiver_path, &json!(receiver));
    // Stop and reap the old B before its replacement binds the same port.
    drop(b_proc);
    let _b_proc = b.spawn(&s.bin, &s.registry_url, Some(&receiver_path));
    wait_up(&b.url()).await;

    // Stages 5 and 6 per committed offer.
    let a_on_b = Http {
        base: api.base.clone(),
        token: "org_example_a-operator".into(),
        http: reqwest::Client::new(),
    };
    for (offer, c) in &committed {
        let requirement_id = c["requirement"]["requirement_id"].as_str().unwrap();
        let owner = c["requirement"]["owner"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let responsibility = c["responsibility"].as_str().unwrap();
        let reference = &s.offers[offer];
        let trial = pinned(offer);
        let intent = |key: &str, role: &str, decision: &Value| {
            json!({"client_request_id":key,"mission_id":mission,"requirement_id":requirement_id,
                "role_id":role,"responsibility_id":responsibility,"publisher":"org_example_a",
                "reference":reference,"decision":decision})
        };
        let adopt = json!({"decision":"adopt","trial":trial});
        let (status_a, body_a) = a_on_b
            .post(
                "/method-adoptions",
                &intent(&format!("{offer}-a-in-b"), &owner, &adopt),
            )
            .await;
        // The lead control is sent only when someone else owns the work.
        let (lead_status, lead_attempt) = if owner == lead {
            (
                None,
                json!({"who":"B's lead","skipped":"the lead owns this work"}),
            )
        } else {
            let (status, body) = api
                .post(
                    "/method-adoptions",
                    &intent(&format!("{offer}-lead"), &lead, &adopt),
                )
                .await;
            (
                Some(status),
                json!({"who":"B's lead","status":status,"body":body}),
            )
        };
        write(
            &out.join(format!("refused-{offer}.json")),
            &json!([{"who":"A's operator token on B","status":status_a,"body":body_a}, lead_attempt]),
        );
        assert!(
            status_a == 401 || status_a == 403,
            "A acted in B: {status_a}"
        );
        if let Some(status) = lead_status {
            assert!(
                status >= 400,
                "the lead adopted work it doesn't own: {status}"
            );
        }
        let accepted = summary["offers"][*offer]["trial"]["verdict"] == "accepted";
        let decision = if accepted {
            adopt
        } else {
            json!({"decision":"decline","reason":"B's receiving trial did not accept it"})
        };
        let (status, decided) = api
            .post(
                "/method-adoptions",
                &intent(&format!("{offer}-owner"), &owner, &decision),
            )
            .await;
        write(
            &out.join(format!("decision-{offer}.json")),
            &json!({"status":status,"body":decided}),
        );
        assert_eq!(status, 201, "{decided}");
        summary["offers"][*offer]["decision"] = decided["outcome"]["outcome"].clone();
        if !accepted {
            continue;
        }

        let ask = json!({"role_id":owner,"responsibility_id":responsibility,
            "publisher":"org_example_a","reference":reference});
        let (status, current) = api.post("/method-invocations/authorize", &ask).await;
        write(
            &out.join(format!("authorization-{offer}.json")),
            &json!({"status":status,"body":current}),
        );
        assert_eq!(status, 200, "{current}");
        let plan = read(&trial_out.join(&trial).join("plan.json"));
        let template = read(&s.dir.join(format!("invoke-{name}-{offer}.json")));
        let tag = format!("g2a1-{name}-{offer}");
        let mut invocation = plan.clone();
        invocation["id"] = json!(format!("invoke-{tag}"));
        invocation["development_instances"] = template["development_instances"].clone();
        invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
        let mut slot = invocation["slots"][0].clone();
        slot["slot_id"] = json!("use");
        slot["run_id"] = json!(format!("run-{tag}"));
        slot["attempt_id"] = json!(format!("att_{}", tag.replace('-', "_")));
        slot["scoring_request_id"] = json!(format!("score-{tag}"));
        let g = &current["grant"];
        slot["purpose"] = json!({"kind":"invocation","grant":{"adoption_id":current["adoption_id"],
            "requirement_id":current["requirement_id"],"role_id":g["role_id"],
            "responsibility_id":g["responsibility_id"],"assignment_id":g["assignment_id"],"epoch":g["epoch"]}});
        invocation["slots"] = json!([slot]);
        let request_path = out.join(format!("proposed-request-{offer}.json"));
        let plan_path = out.join(format!("proposed-plan-{offer}.json"));
        write(&request_path, &ask);
        write(&plan_path, &invocation);
        let runs = out.join(format!("run-{offer}"));
        let token = [("GENESIS_OPERATOR_TOKEN", os("org_example_b-operator"))];
        let package = trial_out.join(format!("packages/{offer}.package.json"));
        let steps: [(&str, Vec<OsString>); 3] = [
            (
                "prepare",
                vec![
                    os("prepare"),
                    os("--org"),
                    os(&api.base),
                    os("--key"),
                    os(format!("{tag}-run")),
                    os("--request"),
                    os(&request_path),
                    os("--plan"),
                    os(&plan_path),
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
                    os(&package),
                    os("--lab-exec"),
                    os(&s.lab_exec),
                    os("--sandbox"),
                    os("namespaces"),
                ],
            ),
            (
                "score",
                vec![os("score"), os(&runs), os("--lab-score"), os(&s.lab_score)],
            ),
        ];
        let mut id = String::new();
        for (step, args) in steps {
            let o = python(&s.invoke_py, &args, &token);
            std::fs::write(out.join(format!("{step}-{offer}.stdout")), &o.stdout).unwrap();
            std::fs::write(out.join(format!("{step}-{offer}.stderr")), &o.stderr).unwrap();
            assert!(
                o.status.success(),
                "{step}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            if step == "prepare" {
                id = serde_json::from_slice::<Value>(&o.stdout).unwrap()["id"]
                    .as_str()
                    .unwrap()
                    .to_string();
            }
        }
        let record = api.get(&format!("/method-runs/{id}")).await;
        write(&out.join(format!("run-record-{offer}.json")), &record);
        summary["offers"][*offer]["invocation"] = json!({"run":id,"status":record["status"],
            "attachment":record["result"]["attachment"]["kind"]});
        assert_eq!(record["status"], "completed", "{record}");
    }
    write(&out.join("summary.json"), &summary);
    summary
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "g2-attempt-1: needs REHEARSAL_DIR, GENESIS_BIN, PLANNER_BIN, GENESIS_LLM_*, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY"]
async fn g2_attempt_1() {
    let dir = env("REHEARSAL_DIR");
    let out = dir.join("run");
    assert!(
        !out.exists(),
        "{} exists: the attempt runs once",
        out.display()
    );
    std::fs::create_dir_all(out.join("state")).unwrap();
    let bin = env("GENESIS_BIN");

    let mut constitution = genesis_server::demo_constitution();
    constitution.scopes = ["mission.coordinate", "method.develop"]
        .into_iter()
        .map(String::from)
        .collect();
    let constitution_path = out.join("state/constitution.json");
    write(&constitution_path, &json!(constitution));

    let registry_port = free_port();
    let registry_url = format!("http://127.0.0.1:{registry_port}");
    let _registry = Proc(
        Command::new(&bin)
            .arg("registry")
            .arg("--db")
            .arg(out.join("state/registry.db"))
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
        db: out.join("state/a.db"),
        log: out.join("org-a.log"),
        registry_token: "a-registry",
        constitution: constitution_path.clone(),
    };
    let _a_proc = a.spawn(&bin, &registry_url, None);
    wait_up(&a.url()).await;
    let a_api = a.api();
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

    // Stage 1: A publishes both offers with A's token.
    let spec = read(&dir.join("b1-receive.json"));
    let a_org = OrgId::new("org_example_a");
    let mut offers = BTreeMap::new();
    let mut forged = json!({});
    for (offer, intent) in [
        (
            "parent",
            "cart design method, developed for ravine crossing",
        ),
        (
            "same-cart",
            "cart design method that submits one fixed cart",
        ),
    ] {
        let (reference, package_json) = offer_ref(&dir, &spec, offer);
        let publication = MethodPublication {
            reference: reference.clone(),
            package_json,
            intent: intent.into(),
            applicability: vec!["lab family v1".into()],
        };
        RegistryClient::new(&registry_url, "a-registry")
            .publish_method(&a_org, &publication)
            .await
            .unwrap();
        let attempt = RegistryClient::new(&registry_url, "b-registry")
            .publish_method(&a_org, &publication)
            .await;
        forged[offer] = json!({"refused":attempt.is_err(),
            "error":attempt.as_ref().err().map(|e| format!("{e:?}"))});
        assert!(attempt.is_err(), "B published as A");
        offers.insert(offer, reference);
    }
    write(&out.join("b-publish-as-a.json"), &forged);
    // B's operator token means nothing to A.
    let b_on_a = Http {
        base: a_api.base.clone(),
        token: "org_example_b-operator".into(),
        http: reqwest::Client::new(),
    };
    let (status, body) = b_on_a.get_status("/organization").await;
    write(
        &out.join("b-token-on-a.json"),
        &json!({"status":status,"body":body}),
    );
    assert!(
        status == 401 || status == 403,
        "B's token worked on A: {status}"
    );

    let llm = [
        "GENESIS_LLM_PROVIDER",
        "GENESIS_LLM_MODEL",
        "GENESIS_LLM_API_KEY",
        "GENESIS_LLM_BASE_URL",
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
        "SSL_CERT_FILE",
    ]
    .into_iter()
    .filter_map(|k| std::env::var_os(k).map(|v| (k, v)))
    .collect();
    let shared = Shared {
        dir: dir.clone(),
        out: out.clone(),
        bin: bin.clone(),
        planner_bin: env("PLANNER_BIN"),
        lab_exec: env("LAB_EXEC"),
        lab_score: env("LAB_SCORE"),
        receive_py: env("RECEIVE_PY"),
        invoke_py: env("INVOKE_PY"),
        registry_url: registry_url.clone(),
        constitution: constitution_path,
        offers,
        llm,
    };
    let b1 = run_b(
        &shared,
        "b1",
        "Own the lab and evaluate reusable methods",
        &["mission.coordinate", "method.develop"],
    )
    .await;
    let b2 = run_b(
        &shared,
        "b2",
        "Own the lab and run rough-transport deliveries",
        &["mission.coordinate"],
    )
    .await;

    // A's state is its own.
    let a_state = a_api.get("/organization").await;
    write(&out.join("a-organization-at-end.json"), &a_state);
    assert!(a_state["requirements"].as_object().unwrap().is_empty());
    write(&out.join("summary.json"), &json!({"b1":b1,"b2":b2}));
}
