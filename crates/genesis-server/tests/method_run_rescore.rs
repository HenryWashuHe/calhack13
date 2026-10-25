//! Invocation results under the optional pinned rescorer. B adopts A's child
//! from B's retained #116 trial vs the fixed cart, runs it once under the
//! grant (the receipt is #116's candidate receipt rebound; nothing executes)
//! and posts the run's evaluation. With a rescorer configured the output must
//! come with the evaluation, and the evaluation must reproduce under the
//! pinned scorer before it is recorded or attached. Default cases use a
//! stand-in scorer script; the ignored case runs the real `lab-score` named
//! by `GENESIS_TEST_LAB_SCORE`. No method runs.
use genesis_core::development_evaluation::EvaluationReceipt;
use genesis_core::development_method::{ExactContentRef, ExecutionReceipt};
use genesis_core::development_trial::TrialPlan;
use genesis_core::engine::{bootstrap_mission, MissionBootstrap};
use genesis_core::ids::{ObservationId, OrgId, RequirementId};
use genesis_core::lifecycle::RequirementStatus;
use genesis_core::model::{Observation, Owner, Requirement};
use genesis_server::method_adoption::{AcceptancePin, MethodReceiver};
use genesis_server::method_rescore::MethodRescorer;
use genesis_server::registry::{self, RegistryState};
use genesis_server::store::EventMeta;
use genesis_server::{build_org, serve_org, OrgConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

/// One completed, unscored run under B's grant for the adopted child.
struct Run {
    _dir: tempfile::TempDir,
    scratch: PathBuf,
    token: String,
    /// The result URL on a server without a rescorer.
    plain: String,
    app: genesis_server::app::AppState,
    id: String,
    plan: TrialPlan,
    receipt: ExecutionReceipt,
    output: Vec<u8>,
    /// #116's candidate evaluation rebound to this run's request: it passes
    /// the store's identity checks but its metrics were never computed for
    /// this run, as an operator could post it.
    rebound: EvaluationReceipt,
}

impl Run {
    /// Serves the same organization again with `rescorer` and returns the
    /// result URL there.
    async fn rescored(&self, rescorer: MethodRescorer) -> String {
        let mut app = self.app.clone();
        app.method_rescorer = Some(Arc::new(rescorer));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
        tokio::spawn(async move { serve_org(app, listener).await.unwrap() });
        format!("{base}/method-runs/{}/result", self.id)
    }

    /// A stand-in scorer that writes `evaluation` as its result.
    fn stand_in(&self, name: &str, evaluation: &EvaluationReceipt) -> MethodRescorer {
        let fixture = self.scratch.join(format!("{name}.json"));
        std::fs::write(&fixture, serde_json::to_vec(evaluation).unwrap()).unwrap();
        let script = format!(
            "#!/bin/sh\nset -e\n[ \"$1\" = score ] && [ -f \"$4/receipt.json\" ]\n\
             /bin/mkdir -p \"$5\"\n/bin/cp {} \"$5/evaluation.json\"\n",
            fixture.display()
        );
        let path = self.scratch.join(format!("{name}.sh"));
        std::fs::write(&path, &script).unwrap();
        MethodRescorer {
            path,
            sha256: hex(script.as_bytes()),
            timeout_ms: None,
        }
    }
}

async fn completed_run() -> Run {
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
    let trial = records.join("b-fixed-cart");
    copy_dir(
        &Path::new(&format!(
            "{EVIDENCE}/live-method-20261024-b-revision-receiver"
        ))
        .join("vs-fixed-cart"),
        &trial,
    );
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
    let token = config.tokens.operator.clone();

    let (status, adopted) = call(
        &format!("{base}/method-adoptions"),
        &token,
        json!({"client_request_id":"adopt","mission_id":mission,"requirement_id":requirement,
            "role_id":role,"responsibility_id":rsp_id,"publisher":"org_example_a","reference":child,
            "decision":{"decision":"adopt","trial":"b-fixed-cart"}}),
    )
    .await;
    assert_eq!(status, 201, "{adopted}");
    assert_eq!(adopted["outcome"]["outcome"], "adopted", "{adopted}");
    let ask = json!({"role_id":role,"responsibility_id":rsp_id,
        "publisher":"org_example_a","reference":child});
    let (status, current) = call(
        &format!("{base}/method-invocations/authorize"),
        &token,
        ask.clone(),
    )
    .await;
    assert_eq!(status, 200, "{current}");

    // One-slot invocation plan on the trial's own panel.
    let mut invocation = plan.clone();
    invocation["id"] = json!("invoke-rescore");
    let mut slot = invocation["slots"][0].clone();
    for (k, v) in [
        ("slot_id", "use"),
        ("run_id", "run-invoke-rescore"),
        ("attempt_id", "att_invoke_rescore"),
        ("scoring_request_id", "score-invoke-rescore"),
    ] {
        slot[k] = json!(v);
    }
    let g = &current["grant"];
    slot["purpose"] = json!({"kind":"invocation","grant":{"adoption_id":current["adoption_id"],
        "requirement_id":current["requirement_id"],"role_id":g["role_id"],
        "responsibility_id":g["responsibility_id"],"assignment_id":g["assignment_id"],"epoch":g["epoch"]}});
    invocation["slots"] = json!([slot]);
    let runs = format!("{base}/method-runs");
    let (status, prepared) = call(
        &runs,
        &token,
        json!({"client_request_id":"r1","request":ask,"plan":invocation}),
    )
    .await;
    assert_eq!(status, 201, "{prepared}");
    let id = prepared["id"].as_str().unwrap().to_string();
    let (status, _) = call(&format!("{runs}/{id}/start"), &token, json!(null)).await;
    assert_eq!(status, 200);
    let frozen: TrialPlan = serde_json::from_value(prepared["plan"].clone()).unwrap();
    let binding = frozen.expected_binding("use").unwrap();
    let mut receipt: ExecutionReceipt =
        serde_json::from_slice(&std::fs::read(trial.join("candidate/receipt.json")).unwrap())
            .unwrap();
    receipt.binding_sha256 = binding.canonical_hash().unwrap();
    receipt.actual_method = binding.method.clone();
    receipt.actual_environment = binding.environment.clone();
    receipt.binding = binding;
    let (status, done) = call(&format!("{runs}/{id}/receipt"), &token, json!(receipt)).await;
    assert_eq!((status, done["status"].clone()), (200, json!("completed")));
    let output = std::fs::read(
        trial
            .join("candidate/artifacts")
            .join(&receipt.outputs[0].sha256),
    )
    .unwrap();
    let mut rebound: EvaluationReceipt = serde_json::from_slice(
        &std::fs::read(trial.join("candidate-score/evaluation.json")).unwrap(),
    )
    .unwrap();
    let request = frozen
        .evaluation_request_for_accounting("use", &receipt)
        .unwrap()
        .unwrap();
    rebound.request_sha256 = request.identity_hash(&receipt, &receipt.binding).unwrap();
    rebound.request = request;
    let scratch = dir.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    Run {
        scratch,
        token,
        plain: format!("{runs}/{id}/result"),
        app,
        id,
        plan: frozen,
        receipt,
        output,
        rebound,
        _dir: dir,
    }
}

fn refused(status: u16, body: &Value, why: &str) {
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"], "method_rescore", "{body}");
    assert!(
        body["message"].as_str().unwrap_or_default().contains(why),
        "{body}"
    );
}

#[tokio::test]
async fn a_rescorer_records_only_an_evaluation_it_reproduces() {
    let run = completed_run().await;
    let url = run.rescored(run.stand_in("genuine", &run.rebound)).await;
    let posted = |evaluation: &EvaluationReceipt, output: &[u8]| json!({"evaluation":evaluation,"output_hex":to_hex(output)});
    // The evaluation alone isn't enough once results are rescored.
    let (status, body) = call(&url, &run.token, json!(run.rebound)).await;
    refused(status, &body, "send output_hex");
    // The output must be the bytes the receipt names.
    let (status, body) = call(&url, &run.token, posted(&run.rebound, b"other")).await;
    refused(status, &body, "not the run's output");
    // An evaluation the scorer doesn't reproduce is refused. It would pass
    // the store's identity checks: only the metrics reference differs.
    let mut forged = run.rebound.clone();
    let mut metrics = run.rebound.trace.clone();
    metrics.schema = "lab-scoring-metrics/1".into();
    metrics.id = "lab-scoring-metrics/1".into();
    metrics.sha256 = "a".repeat(64);
    forged.outcome = genesis_core::development_evaluation::EvaluationOutcome::Scored { metrics };
    let (status, body) = call(&url, &run.token, posted(&forged, &run.output)).await;
    refused(status, &body, "does not reproduce");
    // Nothing was recorded by the refusals; the reproduced one attaches.
    let (status, scored) = call(&url, &run.token, posted(&run.rebound, &run.output)).await;
    assert_eq!(status, 200, "{scored}");
    assert_eq!(
        scored["result"]["attachment"]["kind"], "attached",
        "{scored}"
    );
    assert!(scored["result"]["rescorer"].is_string(), "{scored}");
    // Without a rescorer the plain body still works and nothing changed; the
    // run already has its one result.
    assert_eq!(
        call(&run.plain, &run.token, json!(run.rebound)).await.0,
        409
    );
}

#[tokio::test]
async fn without_a_rescorer_either_body_is_recorded_unrescored() {
    let run = completed_run().await;
    let body = json!({"evaluation":run.rebound,"output_hex":to_hex(&run.output)});
    let (status, scored) = call(&run.plain, &run.token, body).await;
    assert_eq!(status, 200, "{scored}");
    assert!(scored["result"].get("rescorer").is_none(), "{scored}");
}

/// `GENESIS_TEST_LAB_SCORE=<lab-score> cargo test -p genesis-server --test
/// method_run_rescore -- --ignored`
#[tokio::test]
#[ignore = "needs a built lab-score; set GENESIS_TEST_LAB_SCORE"]
async fn the_real_lab_score_refuses_a_rebound_evaluation_and_records_its_own() {
    let lab_score = std::env::var("GENESIS_TEST_LAB_SCORE").expect("GENESIS_TEST_LAB_SCORE");
    let run = completed_run().await;
    let url = run
        .rescored(MethodRescorer {
            path: lab_score.clone().into(),
            sha256: hex(&std::fs::read(&lab_score).unwrap()),
            timeout_ms: None,
        })
        .await;
    // The rebound evaluation keeps #116's scoring trace. Its metrics happen to
    // match (same output, same panel), but this run's own trace differs, so
    // it doesn't reproduce.
    let body = json!({"evaluation":run.rebound,"output_hex":to_hex(&run.output)});
    let (status, refusal) = call(&url, &run.token, body).await;
    refused(status, &refusal, "does not reproduce");
    // Score this run honestly with the same scorer, then post that.
    let work = run.scratch.join("honest");
    std::fs::create_dir_all(work.join("state/artifacts")).unwrap();
    std::fs::write(
        work.join("plan.json"),
        serde_json::to_vec(&run.plan).unwrap(),
    )
    .unwrap();
    std::fs::write(
        work.join("state/receipt.json"),
        serde_json::to_vec(&run.receipt).unwrap(),
    )
    .unwrap();
    std::fs::write(
        work.join("state/artifacts")
            .join(&run.receipt.outputs[0].sha256),
        &run.output,
    )
    .unwrap();
    let status = std::process::Command::new(&lab_score)
        .arg("score")
        .arg(work.join("plan.json"))
        .arg("use")
        .arg(work.join("state"))
        .arg(work.join("score"))
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let honest: Value =
        serde_json::from_slice(&std::fs::read(work.join("score/evaluation.json")).unwrap())
            .unwrap();
    let body = json!({"evaluation":honest,"output_hex":to_hex(&run.output)});
    let (status, scored) = call(&url, &run.token, body).await;
    assert_eq!(status, 200, "{scored}");
    assert_eq!(
        scored["result"]["attachment"]["kind"], "attached",
        "{scored}"
    );
    assert!(scored["result"]["rescorer"].is_string(), "{scored}");
}
