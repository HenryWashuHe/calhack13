//! Inert records and local files only; the imported driver remains ignored.
#[path = "aba_attempt_2.rs"]
mod driver;
use genesis_core::{
    development_evaluation::{EvaluationOutcome, EvaluationReceipt},
    development_method::ExecutionOutcome,
    development_trial::TrialPlan,
    development_verdict::{decide, SlotEvidence, Verdict},
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-fixed-cart",
    )
}
fn retained(slot: &str) -> (TrialPlan, SlotEvidence) {
    let dir = root();
    let scoring: EvaluationReceipt = serde_json::from_slice(
        &fs::read(dir.join(format!("{slot}-score/evaluation.json"))).unwrap(),
    )
    .unwrap();
    let EvaluationOutcome::Scored { metrics } = &scoring.outcome else {
        panic!()
    };
    let bytes = fs::read(dir.join(format!("{slot}-score/artifacts/{}", metrics.sha256))).unwrap();
    (
        serde_json::from_slice(&fs::read(dir.join("plan.json")).unwrap()).unwrap(),
        SlotEvidence {
            execution: serde_json::from_slice(
                &fs::read(dir.join(format!("{slot}/receipt.json"))).unwrap(),
            )
            .unwrap(),
            scoring: Some(scoring),
            metrics: Some(bytes),
        },
    )
}

#[test]
fn policy_requires_both_execution_outcomes_even_when_verdict_accepts() {
    for role in ["candidate", "control"] {
        for outcome in [
            ExecutionOutcome::Failed,
            ExecutionOutcome::Interrupted,
            ExecutionOutcome::Unknown,
        ] {
            let (plan, mut candidate) = retained("candidate");
            let (_, mut control) = retained("control");
            let evidence = if role == "candidate" {
                &mut candidate
            } else {
                &mut control
            };
            evidence.execution.outcome = outcome;
            let binding = plan.expected_binding(role).unwrap();
            let request = plan
                .evaluation_request_for_accounting(role, &evidence.execution)
                .unwrap()
                .unwrap();
            let scoring = evidence.scoring.as_mut().unwrap();
            scoring.request_sha256 = request
                .identity_hash(&evidence.execution, &binding)
                .unwrap();
            scoring.request = request;
            if outcome != ExecutionOutcome::Unknown {
                assert!(matches!(
                    decide(
                        &plan,
                        &fs::read(root().join("criterion.json")).unwrap(),
                        &candidate,
                        &control
                    )
                    .unwrap(),
                    Verdict::Accepted { .. }
                ));
            }
            let result = driver::process_eligibility(&plan, &candidate, &control).unwrap();
            assert_eq!(result["eligible"], false, "{role}: {outcome:?}");
            assert!(!result[role]["reasons"].as_array().unwrap().is_empty());
        }
    }
    let (plan, candidate) = retained("candidate");
    let (_, control) = retained("control");
    assert_eq!(
        driver::process_eligibility(&plan, &candidate, &control).unwrap()["eligible"],
        true
    );
}

#[test]
fn policy_missing_scoring_and_foreign_bindings_do_not_pass() {
    let (plan, candidate) = retained("candidate");
    let (_, mut control) = retained("control");
    control.scoring = None;
    control.metrics = None;
    assert_eq!(
        driver::process_eligibility(&plan, &candidate, &control).unwrap()["eligible"],
        false
    );
    let (_, mut control) = retained("control");
    control.execution.actual_method.sha256 = "a".repeat(64);
    assert!(driver::process_eligibility(&plan, &candidate, &control).is_err());
}

#[test]
fn policy_preparation_reproduces_retained_receive_plan_and_rejects_bad_slots() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/invention-lab/evidence/rehearsal-aba2-receiving-20261025/a-trial/rehearse5-a-pinned/plan.json");
    let original: TrialPlan = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let control = original.slots[1].method.clone();
    let prepared = driver::receiving_plan(
        json!(original),
        "org_example_a",
        "rehearse5-a-pinned",
        &control,
    )
    .unwrap();
    assert_eq!(prepared, original);
    let mut changed = prepared.clone();
    changed.scoring_grant.wall_time_ms += 1;
    assert_ne!(changed, prepared);
    assert_ne!(changed.reference().unwrap(), prepared.reference().unwrap());
    let mut wrong = json!(original);
    wrong["slots"].as_array_mut().unwrap().swap(0, 1);
    assert!(driver::receiving_plan(wrong, "org_example_a", "a2-pinned", &control).is_err());
}

#[test]
fn policy_waits_for_exact_observation_assignment() {
    let mut state = json!({"requirements":{"r":{"requirement_id":"r","mission_id":"m","triggering_evidence":["o"],"status":"UNMET","owner":{"kind":"mission_controller"}}},"responsibilities":{}});
    assert_eq!(driver::assigned_work(&state, "m", "o"), None);
    state["requirements"]["r"]["status"] = json!("ASSIGNED");
    state["requirements"]["r"]["owner"] = json!({"kind":"role","id":"designer"});
    state["responsibilities"]["s"] = json!({"responsibility_id":"s","mission_id":"m","requirement_id":"r","status":"OPEN","owner":{"kind":"role","id":"designer"}});
    assert!(driver::assigned_work(&state, "m", "o").is_some());
    assert_eq!(driver::assigned_work(&state, "m", "foreign"), None);
}

#[test]
fn policy_source_orders_receiver_pin_before_a_trial_and_checks_exact_plan() {
    let source = include_str!("aba_attempt_2.rs");
    let write_pin = source
        .find("write(&receiver_path, &json!(receiver));")
        .unwrap();
    let restart = source.find("let _a_proc = a.spawn").unwrap();
    let execute = source
        .find("let (actual_plan, a_verdict) = receive(")
        .unwrap();
    let compare = source.find("if actual_plan != frozen_a_plan").unwrap();
    let adopt = source.find("let accepted = a_verdict").unwrap();
    assert!(write_pin < restart && restart < execute && execute < compare && compare < adopt);
}

#[test]
fn policy_snapshot_keeps_original_bytes_after_input_changes() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "aba250-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let package = fs::read(root().parent().unwrap().join("fixed-cart.package.json")).unwrap();
    fs::write(dir.join("source.package.json"), &package).unwrap();
    let spec = json!({"schema":"lab-receive-spec/1","packages":{"fixed-cart":{"path":"source.package.json","sha256":"dc5b74f34698885641ca5c673ae80f00f8248ce2e073ac3fe1367d924fd2f3bd"}}});
    fs::write(dir.join("source.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
    let frozen = driver::snapshot_spec(&dir.join("source.json"), &dir.join("frozen"));
    fs::write(dir.join("source.package.json"), b"changed").unwrap();
    let value: Value = serde_json::from_slice(&fs::read(frozen).unwrap()).unwrap();
    assert_eq!(
        fs::read(
            dir.join("frozen")
                .join(value["packages"]["fixed-cart"]["path"].as_str().unwrap())
        )
        .unwrap(),
        package
    );
    assert_eq!(
        fs::read(dir.join("frozen/source-spec.json")).unwrap(),
        serde_json::to_vec(&spec).unwrap()
    );
    fs::remove_dir_all(dir).unwrap();
}
