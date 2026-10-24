//! Independent review regressions for PR #64 at b82bd52e9f862b227d530d03deb4f498c9ac577b.
//! Fixture helpers copied unchanged from the author's adoption.rs at that head.
//! In-process fixed-design evidence only; no generated code or cloud execution.
use genesis_core::development_evaluation::EvaluationOutcome;
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{
    ExactContentRef, ExecutionOutcome, ResourceGrant, ResourceUsage,
};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_adopt::{conclude, precheck, Decision, Offer, Outcome, Receiver};
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::score_slot;
use genesis_lab_score::verdict::SlotEvidence;
use serde_json::json;

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");
/// One small box: valid, and never crosses a ravine.
const BOX: &str = r#"{"language":1,"payload_on":"b","bodies":[{"shape":"rect","id":"b","x":-1.0,"y":0.2,"w":0.4,"h":0.4,"material":"wood"}],"joints":[]}"#;

fn receiver(org: &str) -> Receiver {
    Receiver {
        org_id: org.into(),
        runtime: refs::runtime("Python 3"),
        protocol: refs::protocol(),
        allowed_dependencies: vec![],
        installed: vec![],
    }
}

fn package(runtime: &str, deps: Vec<ExactContentRef>) -> MethodPackage {
    MethodPackage::new(
        "a-method".into(),
        "1.0.0".into(),
        refs::runtime(runtime),
        refs::protocol(),
        "method.py".into(),
        deps,
        vec![MethodSourceFile {
            path: "method.py".into(),
            content: "print('x')\n".into(),
        }],
    )
    .unwrap()
}

fn offer(p: &MethodPackage) -> Offer {
    Offer {
        publisher: "org_a".into(),
        reference: p.reference().unwrap(),
        bytes: p.artifact_json().unwrap().into_bytes(),
    }
}

fn plan(allow_unknown: bool) -> (TrialPlan, Vec<u8>) {
    let methods = example::ExampleMethods {
        candidate: package("Python 3", vec![]).reference().unwrap(),
        control: refs::content("lab-method/1", "baseline", "1", b"baseline"),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: 2400,
    };
    let mut p = example::acceptance_plan(&methods, Mode::None, "Python 3", 3, grant);
    let ids: Vec<String> = p
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let mut c = json!({"rule": "success-count-margin", "min_margin": 1, "instances": ids});
    if allow_unknown {
        c["unknown_usage_allowed"] = json!(["model_calls", "input_tokens", "output_tokens"]);
    }
    let bytes = genesis_core::digest::canonical_json(&c).into_bytes();
    p.criterion = refs::content("lab-criterion/1", "success-count-margin", "1", &bytes);
    (p, bytes)
}

/// Runs one slot in-process (submitting `design` if any) and scores it.
fn slot(
    p: &TrialPlan,
    tag: &str,
    id: &str,
    design: Option<&str>,
    outcome: ExecutionOutcome,
) -> SlotEvidence {
    let dir = std::env::temp_dir().join(format!("lab-adopt-{tag}-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session {
        plan: p,
        slot_id: id,
        store: Store::open(&dir.join("state")).unwrap(),
        ledger: ExperimentLedger::new(ExperimentScope {
            plan: p,
            slot_id: id,
        })
        .unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding(id).unwrap().method),
            ..Default::default()
        },
    };
    if let Some(d) = design {
        let d: serde_json::Value = serde_json::from_str(d).unwrap();
        let reply = s
            .handle(&json!({"op": "submit", "design": d}).to_string())
            .unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
    }
    let execution = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        outcome,
        ResourceUsage::Measured { value: 5 },
    )
    .unwrap()
    .receipt;
    let Some(output) = execution.outputs.first() else {
        return SlotEvidence {
            execution,
            scoring: None,
            metrics: None,
        };
    };
    let bytes = std::fs::read(dir.join("state/artifacts").join(&output.sha256)).unwrap();
    let store = Store::open(&dir.join("score")).unwrap();
    let scoring = score_slot(p, id, &execution, &bytes, &store).unwrap();
    let metrics = match &scoring.outcome {
        EvaluationOutcome::Scored { metrics } => {
            Some(std::fs::read(dir.join("score/artifacts").join(&metrics.sha256)).unwrap())
        }
        _ => None,
    };
    SlotEvidence {
        execution,
        scoring: Some(scoring),
        metrics,
    }
}

fn accepted_fixture(tag: &str) -> (TrialPlan, Vec<u8>, SlotEvidence, SlotEvidence) {
    let (p, criterion) = plan(true);
    let candidate = slot(
        &p,
        tag,
        "candidate",
        Some(REFERENCE),
        ExecutionOutcome::OutputGenerated,
    );
    let control = slot(
        &p,
        tag,
        "control",
        Some(BOX),
        ExecutionOutcome::OutputGenerated,
    );
    (p, criterion, candidate, control)
}

#[test]
fn independent_compatible_selected_receiver_control() {
    let b = receiver("org_example_b");
    let (p, criterion, candidate, control) = accepted_fixture("independent-compatible");
    assert_eq!(
        precheck(&offer(&package("Python 3", vec![])), &b),
        Decision::Selected
    );
    assert!(matches!(
        conclude(&b, &p, &criterion, &candidate, &control),
        Ok(Outcome::Adopted { .. })
    ));
}

#[test]
fn independent_incompatible_receiver_cannot_install_after_precheck_refusal() {
    let mut b = receiver("org_example_b");
    b.runtime = refs::runtime("Python 2.7");
    let offered = offer(&package("Python 3", vec![]));
    assert!(matches!(
        precheck(&offered, &b),
        Decision::IncompatibleEnvironment { .. }
    ));
    let (p, criterion, candidate, control) = accepted_fixture("independent-runtime");
    let outcome = conclude(&b, &p, &criterion, &candidate, &control);
    assert!(!matches!(outcome, Ok(Outcome::Adopted { .. })),
        "receiver rejected this exact package's runtime before execution, but conclusion returned {outcome:?}");
}

#[test]
fn independent_existing_installation_cannot_be_adopted_again() {
    let mut b = receiver("org_example_b");
    let offered = offer(&package("Python 3", vec![]));
    b.installed.push(offered.reference.clone());
    assert_eq!(precheck(&offered, &b), Decision::InstallationConflict);
    let (p, criterion, candidate, control) = accepted_fixture("independent-installed");
    let outcome = conclude(&b, &p, &criterion, &candidate, &control);
    assert!(
        !matches!(outcome, Ok(Outcome::Adopted { .. })),
        "receiver already has the exact installation, but conclusion returned {outcome:?}"
    );
}

#[test]
fn independent_concurrent_cli_installation_is_recorded_once() {
    use std::sync::{Arc, Barrier};
    let (p, criterion, candidate, control) = accepted_fixture("independent-cli-race");
    let b = receiver("org_example_b");
    assert!(matches!(
        conclude(&b, &p, &criterion, &candidate, &control),
        Ok(Outcome::Adopted { .. })
    ));
    let dir =
        std::env::temp_dir().join(format!("lab-adopt-independent-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("receiver.json"), serde_json::to_vec(&b).unwrap()).unwrap();
    std::fs::write(dir.join("plan.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    std::fs::write(dir.join("criterion.json"), criterion).unwrap();
    let slot_dir = |id: &str| {
        std::env::temp_dir().join(format!(
            "lab-adopt-independent-cli-race-{id}-{}",
            std::process::id()
        ))
    };
    for round in 0..8 {
        let args: Vec<_> = [
            dir.join("receiver.json"),
            dir.join("plan.json"),
            dir.join("criterion.json"),
            slot_dir("candidate").join("state"),
            slot_dir("candidate").join("score"),
            slot_dir("control").join("state"),
            slot_dir("control").join("score"),
            dir.join(format!("org-{round}")),
        ]
        .into_iter()
        .map(|p| p.into_os_string())
        .collect();
        let barrier = Arc::new(Barrier::new(16));
        let workers: Vec<_> = (0..16)
            .map(|_| {
                let barrier = barrier.clone();
                let args = args.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    std::process::Command::new(env!("CARGO_BIN_EXE_lab-adopt"))
                        .arg("conclude")
                        .args(args)
                        .output()
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        let succeeded = results.iter().filter(|r| r.status.success()).count();
        assert_eq!(succeeded, 1, "round {round}: {succeeded} concurrent calls claimed to install the same exact method; expected one");
    }
}
