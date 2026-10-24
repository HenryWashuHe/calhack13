//! Method adoption: pre-execution checks and the evidence-based outcome.
//! In-process; the designs are fixed fixtures, not generated methods.
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

#[test]
fn prechecks_decide_before_anything_executes() {
    let b = receiver("org_b");
    let p = package("Python 3", vec![]);
    assert_eq!(precheck(&offer(&p), &b), Decision::Selected);

    let mut own = offer(&p);
    own.publisher = "org_b".into();
    assert_eq!(precheck(&own, &b), Decision::OwnPublication);

    let mut kind = offer(&p);
    kind.reference.schema = "lab-method/1".into();
    assert!(matches!(
        precheck(&kind, &b),
        Decision::UnsupportedKind { .. }
    ));

    let other_runtime = package("Python 2.7", vec![]);
    match precheck(&offer(&other_runtime), &b) {
        Decision::IncompatibleEnvironment { reason } => {
            assert!(reason.contains("runtime"), "{reason}")
        }
        d => panic!("{d:?}"),
    }
    let mut other_protocol = receiver("org_b");
    other_protocol.protocol = refs::content("lab-protocol/1", "other", "1", b"other");
    match precheck(&offer(&p), &other_protocol) {
        Decision::IncompatibleEnvironment { reason } => {
            assert!(reason.contains("protocol"), "{reason}")
        }
        d => panic!("{d:?}"),
    }
    let dep = refs::content("lab-dependency/1", "numpy", "2", b"numpy");
    let with_dep = package("Python 3", vec![dep.clone()]);
    assert!(matches!(
        precheck(&offer(&with_dep), &b),
        Decision::IncompatibleEnvironment { .. }
    ));
    let mut allowing = receiver("org_b");
    allowing.allowed_dependencies = vec![dep];
    assert_eq!(precheck(&offer(&with_dep), &allowing), Decision::Selected);

    // Exact identity only: tampered or re-encoded bytes are not the offer.
    let mut tampered = offer(&p);
    tampered.bytes.push(b' ');
    assert!(matches!(
        precheck(&tampered, &b),
        Decision::InvalidManifest { .. }
    ));
    let mut pretty = offer(&p);
    let v: serde_json::Value = serde_json::from_slice(&pretty.bytes).unwrap();
    pretty.bytes = serde_json::to_vec_pretty(&v).unwrap();
    pretty.reference.sha256 = refs::sha256_hex(&pretty.bytes);
    assert!(matches!(
        precheck(&pretty, &b),
        Decision::InvalidManifest { .. }
    ));
    let mut renamed = offer(&p);
    renamed.reference.version = "2.0.0".into();
    assert!(matches!(
        precheck(&renamed, &b),
        Decision::InvalidManifest { .. }
    ));

    let mut installed = receiver("org_b");
    installed.installed = vec![p.reference().unwrap()];
    assert_eq!(
        precheck(&offer(&p), &installed),
        Decision::InstallationConflict
    );
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

#[test]
fn the_receivers_own_evidence_decides_the_outcome() {
    let b = receiver("org_example_b");
    let (p, criterion) = plan(true);
    let good = slot(
        &p,
        "adopt",
        "candidate",
        Some(REFERENCE),
        ExecutionOutcome::OutputGenerated,
    );
    let weak = slot(
        &p,
        "adopt",
        "control",
        Some(BOX),
        ExecutionOutcome::OutputGenerated,
    );
    match conclude(&b, &p, &criterion, &good, &weak).unwrap() {
        Outcome::Adopted { installation } => {
            assert_eq!(installation.org_id, "org_example_b");
            assert_eq!(installation.method, p.slots[0].method);
            assert_eq!(installation.acceptance_plan, p.reference().unwrap());
            assert_eq!(installation.criterion, p.criterion);
            assert_eq!(installation.evaluator, refs::evaluator());
            assert_eq!(installation.environment, p.development_environment);
        }
        o => panic!("{o:?}"),
    }

    // The same method underperforming the receiver's baseline is declined.
    let weak_c = slot(
        &p,
        "decline",
        "candidate",
        Some(BOX),
        ExecutionOutcome::OutputGenerated,
    );
    let good_k = slot(
        &p,
        "decline",
        "control",
        Some(REFERENCE),
        ExecutionOutcome::OutputGenerated,
    );
    assert!(matches!(
        conclude(&b, &p, &criterion, &weak_c, &good_k).unwrap(),
        Outcome::NotApplicable { .. }
    ));

    // An unknown execution is unknown, never a decline or a pass.
    let lost = slot(&p, "unknown", "candidate", None, ExecutionOutcome::Unknown);
    assert!(matches!(
        conclude(&b, &p, &criterion, &lost, &weak).unwrap(),
        Outcome::Unknown { .. }
    ));

    // Without the explicit allowance, unknown model usage is insufficient.
    let (strict, strict_criterion) = plan(false);
    let g = slot(
        &strict,
        "strict",
        "candidate",
        Some(REFERENCE),
        ExecutionOutcome::OutputGenerated,
    );
    let w = slot(
        &strict,
        "strict",
        "control",
        Some(BOX),
        ExecutionOutcome::OutputGenerated,
    );
    assert!(matches!(
        conclude(&b, &strict, &strict_criterion, &g, &w).unwrap(),
        Outcome::InsufficientEvidence { .. }
    ));

    // Another organization's plan is not this receiver's evidence.
    assert!(conclude(&receiver("org_a"), &p, &criterion, &good, &weak).is_err());
}
