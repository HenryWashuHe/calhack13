//! `aba-attempt-2` driver, stages 2–6 (docs/invention-lab/protocols/aba-attempt-2.md),
//! not part of the suite. Built from rehearsal 5's driver (#230, with #236's
//! restart ordering) after rehearsals 5 and 6. Stage 1 (B's revision, Codex) is
//! not run here: `child2`'s path and sha256 come from the generation record
//! and are written into b-receive.json/a-receive.json at freeze.
//!
//! B receives `child2` against its pinned fixed cart and publishes it only if
//! accepted. A finds it, and the real `genesis-planner` plans A's intake with a
//! live model (one `once` call). The protocol's intended owner is A's
//! **existing** design role; if the planner gives the work to anyone else, or
//! makes no change, that is recorded as the outcome and A stops there. A then
//! receives `child2` against its own current method (the parent), pins that
//! exact plan, the owner adopts or declines, and only an adoption runs one
//! invocation with a scored result. Registry, A and B are separate `genesis`
//! processes with their own databases, tokens and receiver files. Ignored
//! unless run with REHEARSAL_DIR (holding b-receive.json, a-receive.json and
//! invocation-template.json), GENESIS_BIN, PLANNER_BIN, GENESIS_LLM_*,
//! LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY. Root and the pinned runtime
//! are the caller's business.
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::ExactContentRef;
use genesis_core::development_package::MethodPackage;
use genesis_core::development_trial::TrialPlan;
use genesis_core::development_verdict::{assess_process_slot, SlotEvidence};
use genesis_core::ids::OrgId;
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
use std::time::Duration;

const B_PINNED: &str = "b2-pinned";
const A_PINNED: &str = "a2-pinned";
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

/// A recorded requirement can precede its asynchronous assignment commit.
/// Read only the requested observation's unambiguous, assigned responsibility.
pub(crate) fn assigned_work(
    org: &Value,
    mission: &str,
    observation: &str,
) -> Option<(String, String, String)> {
    let mut matches = org["requirements"].as_object()?.values().filter(|r| {
        r["mission_id"] == mission
            && r["triggering_evidence"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == observation))
    });
    let requirement = matches.next()?;
    if matches.next().is_some()
        || requirement["status"] != "ASSIGNED"
        || requirement["owner"]["kind"] != "role"
    {
        return None;
    }
    let id = requirement["requirement_id"].as_str()?;
    let owner = requirement["owner"]["id"].as_str()?;
    let mut responsibilities = org["responsibilities"].as_object()?.values().filter(|r| {
        r["requirement_id"] == id
            && r["mission_id"] == mission
            && r["owner"] == requirement["owner"]
            && r["status"] == "OPEN"
    });
    let responsibility = responsibilities.next()?["responsibility_id"].as_str()?;
    if responsibilities.next().is_some() {
        return None;
    }
    Some((id.into(), owner.into(), responsibility.into()))
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
    let package: MethodPackage = serde_json::from_value(envelope["package"].clone()).unwrap();
    MethodPackage::load_source_for(
        json.as_bytes(),
        &reference,
        package.dependencies(),
        package.runtime(),
        package.protocol(),
    )
    .unwrap();
    (reference, json, envelope)
}

/// Keep the declared bytes separate from a derived spec with local snapshot paths.
pub(crate) fn snapshot_spec(source: &Path, out: &Path) -> PathBuf {
    let bytes = std::fs::read(source).unwrap();
    let original: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(original["schema"], "lab-receive-spec/1");
    let mut frozen = original.clone();
    let mut packages = Vec::new();
    for name in original["packages"].as_object().unwrap().keys() {
        assert!(!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        let (_, bytes, _) = package_ref(source.parent().unwrap(), &original, name);
        packages.push((name.clone(), bytes));
        frozen["packages"][name]["path"] = json!(format!("packages/{name}.package.json"));
    }
    std::fs::create_dir(out).unwrap();
    std::fs::create_dir(out.join("packages")).unwrap();
    std::fs::write(out.join("source-spec.json"), bytes).unwrap();
    for (name, bytes) in packages {
        std::fs::write(out.join(format!("packages/{name}.package.json")), bytes).unwrap();
    }
    let path = out.join("spec.json");
    write(&path, &frozen);
    path
}

/// Mirror receive.py's deterministic rewrite, without running either slot.
pub(crate) fn receiving_plan(
    mut example: Value,
    org: &str,
    trial: &str,
    control: &ExactContentRef,
) -> Result<TrialPlan, String> {
    let slots = example["slots"].as_array_mut().ok_or("missing slots")?;
    if slots.len() != 2 || slots[0]["slot_id"] != "candidate" || slots[1]["slot_id"] != "control" {
        return Err("unexpected example slots".into());
    }
    slots[1]["method"] = json!(control);
    for (slot, name) in slots.iter_mut().zip(["candidate", "control"]) {
        slot["org_id"] = json!(org);
        slot["run_id"] = json!(format!("run-receive-{trial}-{name}"));
        slot["attempt_id"] = json!(format!("att_receive-{trial}_{name}"));
        slot["scoring_request_id"] = json!(format!("score-receive-{trial}-{name}"));
    }
    example["id"] = json!(format!("receive-{trial}"));
    let plan: TrialPlan = serde_json::from_value(example).map_err(|e| e.to_string())?;
    plan.validate().map_err(|e| e.to_string())?;
    Ok(plan)
}

/// Future driver preparation only. This command generates a plan, not a trial.
fn prepare_plan(lab_exec: &Path, spec_path: &Path, out: &Path, trial: &str) -> TrialPlan {
    let spec = read(spec_path);
    let mut matches = spec["trials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["id"] == trial);
    let selected = matches.next().unwrap();
    assert!(matches.next().is_none(), "duplicate pinned trial");
    let root = spec_path.parent().unwrap();
    let candidate = selected["candidate"].as_str().unwrap();
    let control = selected["control"].as_str().unwrap();
    let (candidate_ref, _, _) = package_ref(root, &spec, candidate);
    let (control_ref, _, _) = package_ref(root, &spec, control);
    assert!(!out.exists(), "preparation must be create-only");
    std::fs::create_dir(out).unwrap();
    let result = Command::new(lab_exec)
        .args(["example-plan", spec["sandbox"].as_str().unwrap()])
        .arg(spec["instances"].as_u64().unwrap().to_string())
        .arg(spec["simulation_steps"].as_u64().unwrap().to_string())
        .arg(spec["wall_time_ms"].as_u64().unwrap().to_string())
        .arg(out)
        .arg(selected["family"].as_str().unwrap())
        .arg(root.join(spec["packages"][candidate]["path"].as_str().unwrap()))
        .arg(selected["first_seed"].as_u64().unwrap_or(0).to_string())
        .output()
        .unwrap();
    std::fs::write(out.join("prepare.stdout"), &result.stdout).unwrap();
    std::fs::write(out.join("prepare.stderr"), &result.stderr).unwrap();
    assert!(result.status.success(), "offline plan preparation failed");
    let plan = receiving_plan(
        read(&out.join("plan.json")),
        spec["org_id"].as_str().unwrap(),
        trial,
        &control_ref,
    )
    .unwrap();
    assert_eq!(plan.slots[0].method, candidate_ref);
    write(&out.join("plan.json"), &json!(plan));
    plan
}

pub(crate) fn process_eligibility(
    plan: &TrialPlan,
    candidate: &SlotEvidence,
    control: &SlotEvidence,
) -> Result<Value, String> {
    let c = assess_process_slot(plan, "candidate", candidate)?;
    let k = assess_process_slot(plan, "control", control)?;
    Ok(json!({"eligible":c.eligible && k.eligible,"candidate":c,"control":k}))
}

fn retained_eligibility(dir: &Path, plan: &TrialPlan) -> Value {
    let load = |slot: &str| -> Result<SlotEvidence, String> {
        let bytes =
            std::fs::read(dir.join(format!("{slot}/receipt.json"))).map_err(|e| e.to_string())?;
        let execution = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let score_path = dir.join(format!("{slot}-score/evaluation.json"));
        let scoring: Option<EvaluationReceipt> = if score_path.exists() {
            Some(
                serde_json::from_slice(&std::fs::read(score_path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let metrics = match scoring.as_ref().map(|s| &s.outcome) {
            Some(EvaluationOutcome::Scored { metrics }) => {
                metrics.validate().map_err(|e| e.to_string())?;
                Some(
                    std::fs::read(dir.join(format!("{slot}-score/artifacts/{}", metrics.sha256)))
                        .map_err(|e| e.to_string())?,
                )
            }
            _ => None,
        };
        Ok(SlotEvidence {
            execution,
            scoring,
            metrics,
        })
    };
    let result = load("candidate")
        .and_then(|c| load("control").and_then(|k| process_eligibility(plan, &c, &k)));
    result.unwrap_or_else(|error| json!({"eligible":false,"error":error}))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "rehearsal: needs REHEARSAL_DIR, GENESIS_BIN, LAB_EXEC, LAB_SCORE, RECEIVE_PY and INVOKE_PY"]
async fn aba_attempt_2() {
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

    // Stage 2: B receives `child2` against its pinned fixed cart.
    let b_spec_path = snapshot_spec(&dir.join("b-receive.json"), &out.join("b-inputs"));
    let a_spec_path = snapshot_spec(&dir.join("a-receive.json"), &out.join("a-inputs"));
    let b_spec = read(&b_spec_path);
    assert_eq!(b_spec["org_id"], "org_example_b");
    assert_eq!(read(&a_spec_path)["org_id"], "org_example_a");
    let frozen_a_plan = prepare_plan(&lab_exec, &a_spec_path, &out.join("a-prepared"), A_PINNED);
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
    let typed_b_plan: TrialPlan = serde_json::from_value(b_plan).unwrap();
    let b_eligibility = retained_eligibility(&dir.join("b-trial").join(B_PINNED), &typed_b_plan);
    write(&out.join("b-eligibility.json"), &b_eligibility);
    if b_verdict["verdict"] != "accepted" || b_eligibility["eligible"] != true {
        // The attempt ends at B with a decline; nothing is published.
        return;
    }
    let (child2, package_json, envelope) =
        package_ref(b_spec_path.parent().unwrap(), &b_spec, "child2");
    assert_eq!(typed_b_plan.slots[0].method, child2);
    assert_eq!(frozen_a_plan.slots[0].method, child2);
    let b_org = OrgId::new("org_example_b");
    let publication = MethodPublication {
        reference: child2.clone(),
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

    // Stage 3: A finds it with its own token and posts the intake.
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
            &json!({"client_request_id":"aba2-a-selection","need":A_NEED,
                "acceptance":A_ACCEPTANCE,"selection":found.candidates[0]}),
        )
        .await;
    write(
        &out.join("a-intake.json"),
        &json!({"status":status,"body":intake}),
    );
    assert_eq!(status, 201, "{intake}");
    // Stage 3: A's live planner, one `once` call for the intake observation,
    // with A's agent token. Model settings: the caller's GENESIS_LLM_* plus the
    // HTTPS proxy and CA file (B and A are plain HTTP on loopback).
    let llm: Vec<(&str, OsString)> = [
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
    let observation = intake["observation_id"].as_str().unwrap().to_string();
    let planner = |sub: &str, with_model: bool| {
        let mut c = Command::new(env("PLANNER_BIN"));
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(&out)
            .args(["--org-url", &a.url(), "--token", "org_example_a-agent"])
            .args([sub, "--observation", &observation]);
        if with_model {
            c.envs(llm.iter().map(|(k, v)| (k, v)));
        }
        c.output().unwrap()
    };
    let prompt = planner("prompt", false);
    std::fs::write(out.join("a-planner-prompt.txt"), &prompt.stdout).unwrap();
    assert!(prompt.status.success());
    let once = planner("once", true);
    std::fs::write(out.join("a-planner-once.stdout"), &once.stdout).unwrap();
    std::fs::write(out.join("a-planner-once.stderr"), &once.stderr).unwrap();
    let mut a_state = Value::Null;
    let mut assigned = None;
    for _ in 0..240 {
        a_state = a_api.get("/organization").await;
        assigned = assigned_work(&a_state, &a_mission, &observation);
        if assigned.is_some() || !once.status.success() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    write(&out.join("a-organization-after-change.json"), &a_state);
    let Some((requirement_id, owner, responsibility)) = assigned else {
        write(
            &out.join("a-planner-stop.json"),
            &json!({"status":"stopped",
            "observation_id":observation,"reason":"no matching committed assignment observed",
            "planner_exit_success":once.status.success()}),
        );
        return;
    };
    if owner != designer {
        write(
            &out.join("a-planner-stop.json"),
            &json!({"status":"stopped",
            "observation_id":observation,"reason":"planner chose a different owner","role_id":owner}),
        );
        return;
    }

    // Install the exact predeclared A plan before any A trial executes.
    let a_out = dir.join("a-trial");
    let a_plan = json!(frozen_a_plan);
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

    // Stage 4 runs only after the receiver has its exact frozen plan.
    let (actual_plan, a_verdict) = receive(
        &receive_py,
        &a_spec_path,
        &a_out,
        &out,
        "a",
        &labs,
        A_PINNED,
    );
    write(&out.join("a-verdict.json"), &a_verdict);
    let actual_plan: TrialPlan = serde_json::from_value(actual_plan).unwrap();
    if actual_plan != frozen_a_plan {
        write(
            &out.join("a-trial-stop.json"),
            &json!({"status":"stopped",
            "reason":"result plan differs from preinstalled receiver pin",
            "expected":frozen_a_plan.reference().unwrap(),"actual":actual_plan.reference().unwrap()}),
        );
        return;
    }
    let a_eligibility = retained_eligibility(&a_out.join(A_PINNED), &actual_plan);
    write(&out.join("a-eligibility.json"), &a_eligibility);

    // Stage 5: A's owner decides; B's operator token is refused on A.
    let intent = |key: &str, decision: Value| {
        json!({"client_request_id":key,"mission_id":a_mission,"requirement_id":requirement_id,
            "role_id":owner,"responsibility_id":responsibility,"publisher":"org_example_b",
            "reference":child2,"decision":decision})
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
    let accepted = a_verdict["verdict"] == "accepted" && a_eligibility["eligible"] == true;
    let decision = if accepted {
        adopt
    } else {
        json!({"decision":"decline","reason":"the receiving trial was not accepted with eligible process evidence"})
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
            "publisher":"org_example_b","reference":child2});
        let (status, current) = a_api.post("/method-invocations/authorize", &ask).await;
        write(
            &out.join("a-authorization.json"),
            &json!({"status":status,"body":current}),
        );
        assert_eq!(status, 200, "{current}");
        let template = read(&dir.join("invocation-template.json"));
        let mut invocation = a_plan.clone();
        invocation["id"] = json!("invoke-aba-attempt-2");
        invocation["development_instances"] = template["development_instances"].clone();
        invocation["context"]["instance_set"] = template["context"]["instance_set"].clone();
        let mut slot = invocation["slots"][0].clone();
        for (k, v) in [
            ("slot_id", "use"),
            ("run_id", "run-invoke-aba-attempt-2"),
            ("attempt_id", "att_invoke_aba_attempt_2"),
            ("scoring_request_id", "score-invoke-aba-attempt-2"),
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
                    os("aba2-run"),
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
