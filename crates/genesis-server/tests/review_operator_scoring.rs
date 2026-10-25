//! Pressure test of the receiving decision's trust in operator scoring. B's
//! retained #116 trial vs seeded random search was Rejected (candidate 3/6,
//! control 4/6). An operator edits the control's scoring metrics so two of its
//! successes read as failures, keeps every hash in the retained records
//! consistent, and asks to adopt. Without a rescorer the server derives its
//! acceptance from those records, so the forged evidence decides. With the
//! optional pinned rescorer, the decision is refused unless the retained
//! scoring reproduces. Local HTTP fixtures; the default cases use a stand-in
//! scorer script, and the ignored case runs the real `lab-score` when
//! `GENESIS_TEST_LAB_SCORE` names it. No method runs.
use genesis_core::development_method::ExactContentRef;
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

/// How the rescorer is configured for one decision.
enum Scorer {
    None,
    /// A stand-in that writes the genuine retained scoring for each slot.
    Genuine,
    /// The stand-in, pinned to the wrong digest.
    WrongPin,
    /// A stand-in that never finishes, with a short timeout.
    Hangs,
    /// The genuine stand-in, which after scoring the control slot swaps a
    /// consistent forgery into the retained records (a race with assessment).
    ForgesAfter,
    /// The real scorer at this path.
    Real(String),
}

fn stand_in(
    dir: &Path,
    pristine: &Path,
    hang: bool,
    after: Option<(&Path, &Path)>,
) -> (std::path::PathBuf, String) {
    let p = pristine.display();
    let script = if hang {
        "#!/bin/sh\nwhile :; do :; done\n".to_string()
    } else {
        let race = after
            .map(|(forged, trial)| {
                format!(
                    "if [ \"$3\" = control ]; then /bin/cp -r {}/control-score/. {}/control-score/; fi\n",
                    forged.display(),
                    trial.display()
                )
            })
            .unwrap_or_default();
        format!(
            "#!/bin/sh\nset -e\n[ \"$1\" = score ] && [ -f \"$4/receipt.json\" ]\n\
             /bin/mkdir -p \"$5/artifacts\"\n\
             /bin/cp {p}/$3-score/evaluation.json \"$5/evaluation.json\"\n\
             /bin/cp {p}/$3-score/artifacts/* \"$5/artifacts/\"\n{race}"
        )
    };
    let path = dir.join(if hang { "hang.sh" } else { "stand-in.sh" });
    std::fs::write(&path, &script).unwrap();
    (path, hex(script.as_bytes()))
}

/// Serves B with one assigned requirement and the retained trial (forged or
/// not), asks to adopt A's child method with it, and returns the response.
async fn decide(forged: bool, scorer: Scorer) -> (u16, Value) {
    decide_with(forged, None, scorer).await
}

/// Replace the candidate's retained output with the parent method's output on
/// the same panel (from the retained trial vs the parent), then rescore that
/// slot honestly with the real scorer so every record stays consistent.
fn swap_candidate_output(trial: &Path, lab_score: &str) {
    let parent_trial = Path::new(&format!(
        "{EVIDENCE}/live-method-20261024-b-revision-receiver"
    ))
    .join("vs-parent");
    let read = |p: &Path| -> Value { serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap() };
    let ours = read(&trial.join("plan.json"));
    let theirs = read(&parent_trial.join("plan.json"));
    assert_eq!(
        ours["development_instances"],
        theirs["development_instances"]
    );
    let donor = read(&parent_trial.join("control/receipt.json"));
    let output = donor["outputs"][0]["sha256"].as_str().unwrap();
    std::fs::copy(
        parent_trial.join("control/artifacts").join(output),
        trial.join("candidate/artifacts").join(output),
    )
    .unwrap();
    let mut receipt = read(&trial.join("candidate/receipt.json"));
    receipt["outputs"][0]["sha256"] = json!(output);
    std::fs::write(
        trial.join("candidate/receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    std::fs::remove_dir_all(trial.join("candidate-score")).unwrap();
    let status = std::process::Command::new(lab_score)
        .arg("score")
        .arg(trial.join("plan.json"))
        .arg("candidate")
        .arg(trial.join("candidate"))
        .arg(trial.join("candidate-score"))
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

async fn decide_with(forged: bool, swap: Option<&str>, scorer: Scorer) -> (u16, Value) {
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
    let pristine = dir.path().join("pristine");
    copy_dir(&trial, &pristine);
    if forged {
        forge_control(&trial);
    }
    if let Some(lab_score) = swap {
        swap_candidate_output(&trial, lab_score);
    }
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
    app.method_rescorer = match scorer {
        Scorer::None => None,
        Scorer::Genuine | Scorer::WrongPin | Scorer::Hangs | Scorer::ForgesAfter => {
            let hang = matches!(scorer, Scorer::Hangs);
            let forged = dir.path().join("forged");
            if matches!(scorer, Scorer::ForgesAfter) {
                copy_dir(&pristine, &forged);
                forge_control(&forged);
            }
            let after = matches!(scorer, Scorer::ForgesAfter)
                .then_some((forged.as_path(), trial.as_path()));
            let (path, sha256) = stand_in(dir.path(), &pristine, hang, after);
            let sha256 = if matches!(scorer, Scorer::WrongPin) {
                "0".repeat(64)
            } else {
                sha256
            };
            Some(MethodRescorer {
                path,
                sha256,
                timeout_ms: hang.then_some(300),
            })
        }
        Scorer::Real(path) => {
            let sha256 = hex(&std::fs::read(&path).unwrap());
            Some(MethodRescorer {
                path: path.into(),
                sha256,
                timeout_ms: None,
            })
        }
    }
    .map(Arc::new);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { serve_org(served, listener).await.unwrap() });

    call(
        &format!("{base}/method-adoptions"),
        &config.tokens.operator,
        json!({"client_request_id":"forged","mission_id":mission,"requirement_id":requirement,
            "role_id":role,"responsibility_id":rsp_id,"publisher":"org_example_a","reference":child,
            "decision":{"decision":"adopt","trial":"b-random-search"}}),
    )
    .await
}

fn refused(status: u16, body: &Value, why: &str) {
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"], "method_acceptance", "{body}");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(message.contains(why), "{body}");
}

#[tokio::test]
async fn forged_operator_scoring_decides_an_adoption_without_rescoring() {
    let (status, decided) = decide(true, Scorer::None).await;
    // The retained trial was Rejected 3/6 vs 4/6. With the forged control it reads
    // 3/6 vs 2/6 and the server adopts.
    assert_eq!(status, 201, "{decided}");
    assert_eq!(decided["outcome"]["outcome"], "adopted", "{decided}");
    assert_eq!(decided["outcome"]["evidence"]["acceptance"]["control"], 2);
    assert!(decided["outcome"]["evidence"].get("rescorer").is_none());
}

#[tokio::test]
async fn a_pinned_rescorer_refuses_forged_scoring() {
    let (status, body) = decide(true, Scorer::Genuine).await;
    refused(status, &body, "retained control scoring does not reproduce");
}

#[tokio::test]
async fn records_swapped_after_the_rescore_are_refused() {
    // The rescore sees the genuine records; assessment, inside the
    // transaction, would read a consistent forgery. The byte binding refuses.
    let (status, body) = decide(false, Scorer::ForgesAfter).await;
    refused(status, &body, "changed after rescoring");
}

#[tokio::test]
async fn untampered_scoring_reproduces_and_still_decides_on_its_own_verdict() {
    let (status, decided) = decide(false, Scorer::Genuine).await;
    assert_eq!(status, 201, "{decided}");
    assert_eq!(decided["outcome"]["outcome"], "not_applicable", "{decided}");
    let rescorer = decided["outcome"]["evidence"]["rescorer"].as_str().unwrap();
    assert_eq!(rescorer.len(), 64);
}

#[tokio::test]
async fn a_rescorer_that_misses_its_pin_or_hangs_refuses() {
    let (status, body) = decide(false, Scorer::WrongPin).await;
    refused(status, &body, "does not match its pin");
    let (status, body) = decide(false, Scorer::Hangs).await;
    refused(status, &body, "timed out on the candidate slot");
}

/// `GENESIS_TEST_LAB_SCORE=<lab-score> cargo test -p genesis-server --test
/// review_operator_scoring -- --ignored`
#[tokio::test]
#[ignore = "needs a built lab-score; set GENESIS_TEST_LAB_SCORE"]
async fn the_real_lab_score_reproduces_the_retained_trial_and_refuses_the_forgery() {
    let path = std::env::var("GENESIS_TEST_LAB_SCORE").expect("GENESIS_TEST_LAB_SCORE");
    let (status, decided) = decide(false, Scorer::Real(path.clone())).await;
    assert_eq!(status, 201, "{decided}");
    assert_eq!(decided["outcome"]["outcome"], "not_applicable", "{decided}");
    let (status, body) = decide(true, Scorer::Real(path)).await;
    refused(status, &body, "retained control scoring does not reproduce");
}

/// What rescoring does not cover: it scores the retained outputs as given. An
/// operator who swaps in another method's output and rescores it honestly
/// passes the rescore. Closing this needs re-execution, not rescoring.
#[tokio::test]
#[ignore = "needs a built lab-score; set GENESIS_TEST_LAB_SCORE"]
async fn rescoring_does_not_catch_a_substituted_output() {
    let path = std::env::var("GENESIS_TEST_LAB_SCORE").expect("GENESIS_TEST_LAB_SCORE");
    let (status, decided) = decide_with(false, Some(&path), Scorer::Real(path.clone())).await;
    assert_eq!(status, 201, "{decided}");
    assert_eq!(decided["outcome"]["outcome"], "adopted", "{decided}");
    assert!(decided["outcome"]["evidence"]["rescorer"].is_string());
}
