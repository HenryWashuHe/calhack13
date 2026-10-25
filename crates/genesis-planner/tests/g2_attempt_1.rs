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
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialPlan;
use genesis_core::development_verdict::{assess_process_slot, SlotEvidence};
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

fn free_port() -> Result<u16, String> {
    StdListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("allocate port: {error}"))
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
    /// A transport failure comes back as status 0 with its description, never a panic.
    async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        match self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
        {
            Ok(r) => (r.status().as_u16(), r.json().await.unwrap_or(Value::Null)),
            Err(e) => (0, json!({"transport_error": e.to_string()})),
        }
    }
    async fn get_status(&self, path: &str) -> (u16, Value) {
        match self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
        {
            Ok(r) => (r.status().as_u16(), r.json().await.unwrap_or(Value::Null)),
            Err(e) => (0, json!({"transport_error": e.to_string()})),
        }
    }
    async fn get(&self, path: &str) -> Value {
        self.get_status(path).await.1
    }
}

async fn wait_up(addr: &str) -> Result<(), String> {
    let addr = addr.trim_start_matches("http://");
    for _ in 0..400 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Err(format!("{addr} did not come up"))
}

struct OrgProc {
    id: &'static str,
    /// Prefix of this process's operator/agent/adapter tokens.
    tokens: String,
    port: u16,
    db: PathBuf,
    log: PathBuf,
    registry_token: &'static str,
    constitution: PathBuf,
}
impl OrgProc {
    fn spawn(&self, bin: &Path, registry: &str, receiver: Option<&Path>) -> Result<Proc, String> {
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
            .args(["--operator-token", &format!("{}-operator", self.tokens)])
            .args(["--agent-token", &format!("{}-agent", self.tokens)])
            .args(["--adapter-token", &format!("{}-adapter", self.tokens)])
            .arg("--dev-stub")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdout(Stdio::null())
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log)
                    .map_err(|e| format!("open process log: {e}"))?,
            );
        if let Some(r) = receiver {
            c.arg("--method-receiver").arg(r);
        }
        c.spawn()
            .map(Proc)
            .map_err(|e| format!("start organization: {e}"))
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    fn api(&self) -> Http {
        Http {
            base: format!("{}/api/v1", self.url()),
            token: format!("{}-operator", self.tokens),
            http: reqwest::Client::new(),
        }
    }
}

fn python(
    script: &Path,
    args: &[OsString],
    envs: &[(&str, OsString)],
) -> Result<std::process::Output, String> {
    let mut c = Command::new("python3");
    c.arg("-I").arg(script).args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().map_err(|e| format!("start helper: {e}"))
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

/// Stops this instance's stages with a recorded reason instead of a panic.
macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

fn read_checked(path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Retained bytes only: this does not execute a method or scorer.
fn retained_slot(root: &Path, slot: &str) -> Result<SlotEvidence, String> {
    let execution =
        serde_json::from_value(read_checked(&root.join(format!("{slot}/receipt.json")))?)
            .map_err(|e| format!("{slot} execution: {e}"))?;
    let scoring: EvaluationReceipt = serde_json::from_value(read_checked(
        &root.join(format!("{slot}-score/evaluation.json")),
    )?)
    .map_err(|e| format!("{slot} scoring: {e}"))?;
    let metrics = if let EvaluationOutcome::Scored { metrics } = &scoring.outcome {
        metrics.validate().map_err(|e| e.to_string())?;
        Some(
            std::fs::read(root.join(format!("{slot}-score/artifacts/{}", metrics.sha256)))
                .map_err(|e| format!("{slot} metrics: {e}"))?,
        )
    } else {
        None
    };
    Ok(SlotEvidence {
        execution,
        scoring: Some(scoring),
        metrics,
    })
}

fn trial_eligibility(
    plan: &TrialPlan,
    verdict: &Value,
    candidate: &SlotEvidence,
    control: &SlotEvidence,
) -> Result<Value, String> {
    let candidate = assess_process_slot(plan, "candidate", candidate)?;
    let control = assess_process_slot(plan, "control", control)?;
    let settled = verdict["verdict"] == "accepted" || verdict["verdict"] == "rejected";
    Ok(
        json!({"eligible": settled && candidate.eligible && control.eligible,
        "settled_verdict": settled, "candidate": candidate, "control": control}),
    )
}

fn finish_instance(mut summary: Value, result: Result<(), String>) -> Value {
    if let Err(stop) = result {
        summary["stopped"] = json!(stop);
    }
    summary
}

/// One B instance (`name` is B1 or B2, seeing exactly one `offer`) through
/// stages 2–6. Every outcome, including a stop, ends up in its summary; a stop
/// in one instance never prevents the others.
async fn run_b(
    s: &Shared,
    name: &'static str,
    offer: &'static str,
    charter: &str,
    scopes: &[&str],
) -> Value {
    let tag = format!("{name}-{offer}");
    let out = s.out.join(&tag);
    std::fs::create_dir(&out).unwrap();
    let mut summary = json!({"b":name,"offer":offer});
    let result = run_b_stages(s, name, offer, charter, scopes, &out, &mut summary).await;
    let summary = finish_instance(summary, result);
    write(&out.join("summary.json"), &summary);
    summary
}

async fn run_b_stages(
    s: &Shared,
    name: &'static str,
    offer: &'static str,
    charter: &str,
    scopes: &[&str],
    out: &Path,
    summary: &mut Value,
) -> Result<(), String> {
    let tag = format!("{name}-{offer}");
    let b = OrgProc {
        id: "org_example_b",
        tokens: format!("{tag}-b"),
        port: free_port()?,
        db: s.out.join("state").join(format!("{tag}.db")),
        log: out.join("org.log"),
        registry_token: "b-registry",
        constitution: s.constitution.clone(),
    };
    let b_proc = b.spawn(&s.bin, &s.registry_url, None)?;
    wait_up(&b.url()).await?;
    let api = b.api();
    let founding = read_checked(&s.dir.join(format!("founding-{name}.json")))?;
    ensure!(
        founding["roles"][0]["charter"] == json!(charter)
            && founding["roles"][0]["authority"]["scopes"] == json!(scopes),
        "founding-{name}.json differs from the declared charter/scopes"
    );
    let (status, mission) = api.post("/missions", &founding).await;
    write(
        &out.join("mission.json"),
        &json!({"status":status,"body":mission}),
    );
    ensure!(status == 201, "mission: {status} {mission}");
    let mission = mission["mission_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let org = api.get("/organization").await;
    let lead_role = org["roles"]
        .as_object()
        .and_then(|r| r.values().find(|r| r["name"] == "Lab lead"))
        .cloned()
        .ok_or("no Lab lead")?;
    let lead = lead_role["role_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let lead_responsibility = org["responsibilities"]
        .as_object()
        .and_then(|r| r.values().find(|r| r["owner"]["id"] == json!(lead)))
        .map(|r| {
            r["responsibility_id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .ok_or("no lead responsibility")?;
    summary["lead"] = json!(lead);

    // Stage 2: search with B's read token, one intake for this instance's offer.
    let found = RegistryClient::new(&s.registry_url, "b-registry")
        .search_methods(&MethodSearch {
            query: "cart design".into(),
            limit: 10,
            runtime: None,
            protocol: None,
            exclude_publisher: Some(OrgId::new("org_example_b")),
        })
        .await
        .map_err(|e| format!("search: {e:?}"))?;
    write(&out.join("search.json"), &json!(found));
    let reference = &s.offers[offer];
    let selection = found
        .candidates
        .iter()
        .find(|c| json!(c)["reference"] == json!(reference))
        .ok_or(format!("{offer} not found by search"))?;
    let request = json!({"client_request_id":format!("{tag}-selection"),
        "need":NEED,"acceptance":ACCEPTANCE,"selection":selection});
    let (status, intake) = api
        .post(&format!("/missions/{mission}/method-development"), &request)
        .await;
    write(
        &out.join("intake.json"),
        &json!({"status":status,"body":intake}),
    );
    ensure!(status == 201, "intake: {status} {intake}");
    let observation = intake["observation_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    // Stage 3: one live `once` call for that observation. The requirement is
    // matched by its triggering evidence and counts only once ASSIGNED.
    let planner = |sub: &str, with_model: bool| {
        let mut c = Command::new(&s.planner_bin);
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(out)
            .args(["--org-url", &b.url(), "--token", &format!("{tag}-b-agent")])
            .args([sub, "--observation", &observation]);
        if with_model {
            c.envs(s.llm.iter().map(|(k, v)| (k, v)));
        }
        c.output().map_err(|e| format!("start planner {sub}: {e}"))
    };
    let prompt = planner("prompt", false)?;
    std::fs::write(out.join("planner-prompt.txt"), &prompt.stdout).unwrap();
    ensure!(prompt.status.success(), "planner prompt failed");
    let once = planner("once", true)?;
    std::fs::write(out.join("planner-once.stdout"), &once.stdout).unwrap();
    std::fs::write(out.join("planner-once.stderr"), &once.stderr).unwrap();
    summary["planner_exit"] = json!(once.status.code());
    let mut org = Value::Null;
    let mut assigned = None;
    for _ in 0..240 {
        org = api.get("/organization").await;
        let mine = org["requirements"].as_object().and_then(|r| {
            r.values()
                .find(|r| {
                    r["triggering_evidence"]
                        .as_array()
                        .is_some_and(|e| e.contains(&json!(observation)))
                })
                .cloned()
        });
        summary["requirement"] = mine.clone().unwrap_or(Value::Null);
        let responsibility = mine.as_ref().and_then(|r| {
            org["responsibilities"]
                .as_object()
                .and_then(|x| {
                    x.values()
                        .find(|x| x["requirement_id"] == r["requirement_id"])
                })
                .map(|x| x["responsibility_id"].clone())
        });
        if let (Some(r), Some(rsp)) = (&mine, responsibility) {
            if r["status"] == "ASSIGNED" {
                assigned = Some((r.clone(), rsp));
                break;
            }
        }
        if !once.status.success() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    write(&out.join("organization-after-planner.json"), &org);
    let Some((requirement, responsibility)) = assigned else {
        summary["planner"] = json!("no assigned requirement (no change, escalation or unresolved)");
        return Ok(());
    };
    let requirement_id = requirement["requirement_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let owner = requirement["owner"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let responsibility = responsibility.as_str().unwrap_or_default().to_string();
    let owner_role = &org["roles"][&owner];
    summary["planner"] = json!("assigned");
    summary["owner"] = json!({"role_id":owner,"is_lead":owner == lead,
        "name":owner_role["name"],"charter":owner_role["charter"],"scopes":owner_role["authority"]["scopes"]});

    // Stage 4: B's receiving trials for this offer.
    let short = if offer == "parent" { "parent" } else { "same" };
    let pinned = format!("{name}-{short}-pinned");
    let full = read_checked(&s.dir.join(format!("{name}-receive.json")))?;
    let mut spec = full.clone();
    spec["trials"] = json!(full["trials"]
        .as_array()
        .ok_or("spec has no trials")?
        .iter()
        .filter(|t| t["candidate"] == json!(offer))
        .collect::<Vec<_>>());
    // The filtered spec lives in the run directory, so its package paths are
    // made absolute; the bytes are still checked against the same sha256.
    for v in spec["packages"]
        .as_object_mut()
        .ok_or("spec has no packages")?
        .values_mut()
    {
        let path = s.dir.join(v["path"].as_str().unwrap_or_default());
        v["path"] = json!(path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?);
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
    )?;
    std::fs::write(out.join("receive.stdout"), &r.stdout).unwrap();
    std::fs::write(out.join("receive.stderr"), &r.stderr).unwrap();
    ensure!(
        r.status.success(),
        "receive.py exited {:?}",
        r.status.code()
    );
    let plan = read_checked(&trial_out.join(&pinned).join("plan.json"))?;
    let verdict = read_checked(&trial_out.join(&pinned).join("verdict.json"))?;
    summary["trial"] = json!({"id":pinned,"verdict":verdict});
    if let Ok(v) = read_checked(
        &trial_out
            .join(pinned.replace("-pinned", "-vs-random"))
            .join("verdict.json"),
    ) {
        summary["reported_vs_random"] = v;
    }
    let typed_plan: TrialPlan = serde_json::from_value(plan.clone()).map_err(|e| e.to_string())?;
    let candidate = retained_slot(&trial_out.join(&pinned), "candidate")?;
    let control = retained_slot(&trial_out.join(&pinned), "control")?;
    let eligibility = trial_eligibility(&typed_plan, &verdict, &candidate, &control)?;
    write(&out.join("trial-eligibility.json"), &eligibility);
    summary["eligibility"] = eligibility.clone();
    if eligibility["eligible"] != true {
        // Failed, interrupted or unknown executions are ineligible: no decision.
        summary["decision"] = json!("ineligible: pinned trial lacks settled, eligible evidence");
        return Ok(());
    }

    // B's operator pins exactly that plan to the requirement and restarts.
    let envelope = read_checked(&trial_out.join(format!("packages/{offer}.package.json")))?;
    let field = |k: &str| serde_json::from_value::<ExactContentRef>(envelope["package"][k].clone());
    let receiver = MethodReceiver {
        runtime: field("runtime").map_err(|e| e.to_string())?,
        protocol: field("protocol").map_err(|e| e.to_string())?,
        environments: vec![
            serde_json::from_value(plan["development_environment"].clone())
                .map_err(|e| e.to_string())?,
        ],
        evaluators: vec![serde_json::from_value(plan["context"]["evaluator"].clone())
            .map_err(|e| e.to_string())?],
        allowed_dependencies: vec![],
        records_root: trial_out.clone(),
        acceptance_plans: vec![AcceptancePin {
            requirement_id: requirement_id.as_str().into(),
            publisher: OrgId::new("org_example_a"),
            plan: serde_json::from_value::<TrialPlan>(plan.clone())
                .map_err(|e| e.to_string())?
                .reference()
                .map_err(|e| format!("{e:?}"))?,
        }],
    };
    let receiver_path = out.join("receiver.json");
    write(&receiver_path, &json!(receiver));
    // Stop and reap the old B before its replacement binds the same port.
    drop(b_proc);
    let _b_proc = b.spawn(&s.bin, &s.registry_url, Some(&receiver_path))?;
    wait_up(&b.url()).await?;

    // Stage 5. Controls first: a token B never issued, and a role or grant
    // that doesn't hold this work. Then the owner asks to adopt with the
    // pinned trial, and B's receiver decides from that evidence.
    let intent = |key: &str, role: &str, rsp: &str| {
        json!({"client_request_id":key,"mission_id":mission,"requirement_id":requirement_id,
            "role_id":role,"responsibility_id":rsp,"publisher":"org_example_a",
            "reference":reference,"decision":{"decision":"adopt","trial":pinned}})
    };
    let unknown_token = Http {
        base: api.base.clone(),
        token: "org_example_a-operator".into(),
        http: reqwest::Client::new(),
    };
    let (status_t, body_t) = unknown_token
        .post(
            "/method-adoptions",
            &intent(&format!("{tag}-foreign-token"), &owner, &responsibility),
        )
        .await;
    // When the lead owns the work, the mismatched-grant control uses the
    // lead's other responsibility; otherwise the lead asks for the owner's.
    let (control_role, control_rsp, control_label) = if owner == lead {
        (
            lead.clone(),
            lead_responsibility.clone(),
            "the lead with its unrelated 'Coordinate the lab' responsibility",
        )
    } else {
        (
            lead.clone(),
            responsibility.clone(),
            "the lead, which does not own this responsibility",
        )
    };
    let (status_c, body_c) = api
        .post(
            "/method-adoptions",
            &intent(&format!("{tag}-mismatched"), &control_role, &control_rsp),
        )
        .await;
    write(
        &out.join("refused.json"),
        &json!([{"who":"a bearer token B never issued","status":status_t,"body":body_t},
            {"who":control_label,"status":status_c,"body":body_c}]),
    );
    summary["controls"] = json!({"unknown_token":status_t,"mismatched_grant":status_c});
    ensure!(
        status_t == 401 || status_t == 403,
        "a token B never issued acted: {status_t}"
    );
    ensure!(
        status_c >= 400,
        "the mismatched-grant control adopted: {status_c} {body_c}"
    );
    let (status, decided) = api
        .post(
            "/method-adoptions",
            &intent(&format!("{tag}-owner"), &owner, &responsibility),
        )
        .await;
    write(
        &out.join("decision.json"),
        &json!({"status":status,"body":decided}),
    );
    ensure!(status == 201, "owner decision: {status} {decided}");
    summary["decision"] = decided["outcome"]["outcome"].clone();
    if decided["outcome"]["outcome"] != "adopted" {
        return Ok(());
    }

    // Stage 6: one invocation under B's grant, scored and attached.
    let ask = json!({"role_id":owner,"responsibility_id":responsibility,
        "publisher":"org_example_a","reference":reference});
    let (status, current) = api.post("/method-invocations/authorize", &ask).await;
    write(
        &out.join("authorization.json"),
        &json!({"status":status,"body":current}),
    );
    ensure!(status == 200, "authorize: {status} {current}");
    let template = read_checked(&s.dir.join(format!("invoke-{name}-{offer}.json")))?;
    let label = format!("g2a1-{tag}");
    let mut invocation = plan.clone();
    invocation["id"] = json!(format!("invoke-{label}"));
    invocation["development_instances"] = template["development_instances"].clone();
    invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
    invocation["criterion"] = template["criterion"].clone();
    let mut slot = invocation["slots"][0].clone();
    slot["slot_id"] = json!("use");
    slot["run_id"] = json!(format!("run-{label}"));
    slot["attempt_id"] = json!(format!("att_{}", label.replace('-', "_")));
    slot["scoring_request_id"] = json!(format!("score-{label}"));
    let g = &current["grant"];
    slot["purpose"] = json!({"kind":"invocation","grant":{"adoption_id":current["adoption_id"],
        "requirement_id":current["requirement_id"],"role_id":g["role_id"],
        "responsibility_id":g["responsibility_id"],"assignment_id":g["assignment_id"],"epoch":g["epoch"]}});
    invocation["slots"] = json!([slot]);
    let request_path = out.join("proposed-request.json");
    let plan_path = out.join("proposed-plan.json");
    write(&request_path, &ask);
    write(&plan_path, &invocation);
    let runs = out.join("run");
    let token = [("GENESIS_OPERATOR_TOKEN", os(format!("{tag}-b-operator")))];
    let package = trial_out.join(format!("packages/{offer}.package.json"));
    let steps: [(&str, Vec<OsString>); 3] = [
        (
            "prepare",
            vec![
                os("prepare"),
                os("--org"),
                os(&api.base),
                os("--key"),
                os(format!("{label}-run")),
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
        let o = python(&s.invoke_py, &args, &token)?;
        std::fs::write(out.join(format!("{step}.stdout")), &o.stdout).unwrap();
        std::fs::write(out.join(format!("{step}.stderr")), &o.stderr).unwrap();
        ensure!(
            o.status.success(),
            "invoke {step} exited {:?}",
            o.status.code()
        );
        if step == "prepare" {
            id = serde_json::from_slice::<Value>(&o.stdout).map_err(|e| e.to_string())?["id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
        }
    }
    let record = api.get(&format!("/method-runs/{id}")).await;
    write(&out.join("run-record.json"), &record);
    summary["invocation"] = json!({"run":id,"status":record["status"],
        "attachment":record["result"]["attachment"]["kind"]});
    Ok(())
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

    let registry_port = free_port().unwrap();
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
    wait_up(&registry_url).await.unwrap();
    let a = OrgProc {
        id: "org_example_a",
        tokens: "org_example_a".into(),
        port: free_port().unwrap(),
        db: out.join("state/a.db"),
        log: out.join("org-a.log"),
        registry_token: "a-registry",
        constitution: constitution_path.clone(),
    };
    let _a_proc = a.spawn(&bin, &registry_url, None).unwrap();
    wait_up(&a.url()).await.unwrap();
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
    // A token A never issued is refused.
    let b_on_a = Http {
        base: a_api.base.clone(),
        token: "org_example_b-operator".into(),
        http: reqwest::Client::new(),
    };
    let (status, body) = b_on_a.get_status("/organization").await;
    write(
        &out.join("unknown-token-on-a.json"),
        &json!({"who":"a bearer token A never issued","status":status,"body":body}),
    );
    assert!(
        status == 401 || status == 403,
        "a token A never issued worked on A: {status}"
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
    // Four independent B instances, each seeing exactly one offer. B1 and B2
    // differ only in the lead's founding charter and scopes.
    let b1 = (
        "Own the lab and evaluate reusable methods",
        &["mission.coordinate", "method.develop"][..],
    );
    let b2 = (
        "Own the lab and run rough-transport deliveries",
        &["mission.coordinate"][..],
    );
    let mut results = json!({});
    for (name, (charter, scopes)) in [("b1", b1), ("b2", b2)] {
        for offer in OFFERS {
            results[format!("{name}-{offer}")] = run_b(&shared, name, offer, charter, scopes).await;
        }
    }

    // A's state is its own.
    let a_state = a_api.get("/organization").await;
    write(&out.join("a-organization-at-end.json"), &a_state);
    write(&out.join("summary.json"), &results);
    assert!(a_state["requirements"].as_object().unwrap().is_empty());
}

#[cfg(test)]
#[path = "support/g2_249_guards.rs"]
mod review_249;
