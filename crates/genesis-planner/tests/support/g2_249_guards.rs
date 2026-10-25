//! Inert retained-record regressions; no process, provider, method or scorer runs.
use super::*;
use genesis_core::development_method::ExecutionOutcome;
use genesis_core::development_verdict::{decide, Verdict};

fn retained() -> (PathBuf, TrialPlan, SlotEvidence, SlotEvidence) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-fixed-cart",
    );
    let plan = serde_json::from_value(read_checked(&root.join("plan.json")).unwrap()).unwrap();
    let candidate = retained_slot(&root, "candidate").unwrap();
    let control = retained_slot(&root, "control").unwrap();
    (root, plan, candidate, control)
}

fn change_outcome(
    plan: &TrialPlan,
    slot: &str,
    evidence: &mut SlotEvidence,
    outcome: ExecutionOutcome,
) {
    evidence.execution.outcome = outcome;
    let binding = plan.expected_binding(slot).unwrap();
    let request = plan
        .evaluation_request_for_accounting(slot, &evidence.execution)
        .unwrap()
        .unwrap();
    let scoring = evidence.scoring.as_mut().unwrap();
    scoring.request_sha256 = request
        .identity_hash(&evidence.execution, &binding)
        .unwrap();
    scoring.request = request;
}

#[test]
fn complete_evidence_preserves_both_accept_and_non_reuse_outcomes() {
    let (_, plan, candidate, control) = retained();
    for status in ["accepted", "rejected"] {
        let report =
            trial_eligibility(&plan, &json!({"verdict": status}), &candidate, &control).unwrap();
        assert_eq!(report["eligible"], true);
        assert_eq!(report["candidate"]["successes"], 3);
        assert_eq!(report["control"]["successes"], 2);
    }
}

#[test]
fn an_accepted_trial_cannot_admit_failed_or_interrupted_execution() {
    for slot in ["candidate", "control"] {
        for outcome in [ExecutionOutcome::Failed, ExecutionOutcome::Interrupted] {
            let (root, plan, mut candidate, mut control) = retained();
            let changed = if slot == "candidate" {
                &mut candidate
            } else {
                &mut control
            };
            change_outcome(&plan, slot, changed, outcome);
            let criterion = std::fs::read(root.join("criterion.json")).unwrap();
            let verdict = decide(&plan, &criterion, &candidate, &control).unwrap();
            assert!(
                matches!(verdict, Verdict::Accepted { .. }),
                "the old settled-verdict check would admit this"
            );
            let report = trial_eligibility(&plan, &json!(verdict), &candidate, &control).unwrap();
            assert_eq!(report["eligible"], false);
            assert!(!report[slot]["reasons"].as_array().unwrap().is_empty());
        }
    }
}

#[test]
fn unknown_or_unsettled_results_remain_ineligible() {
    let (_, plan, mut candidate, control) = retained();
    for status in ["insufficient", "unknown"] {
        assert_eq!(
            trial_eligibility(&plan, &json!({"verdict":status}), &candidate, &control).unwrap()
                ["eligible"],
            false
        );
    }
    change_outcome(
        &plan,
        "candidate",
        &mut candidate,
        ExecutionOutcome::Unknown,
    );
    assert_eq!(
        trial_eligibility(&plan, &json!({"verdict":"accepted"}), &candidate, &control).unwrap()
            ["eligible"],
        false
    );
}

#[test]
fn foreign_slot_identity_and_tampered_metrics_do_not_pass() {
    let (_, plan, mut candidate, control) = retained();
    candidate.execution.binding.run_id = "foreign-run".into();
    assert!(
        trial_eligibility(&plan, &json!({"verdict":"accepted"}), &candidate, &control).is_err()
    );
    let (_, plan, mut candidate, control) = retained();
    candidate.metrics = Some(b"{}".to_vec());
    assert!(
        trial_eligibility(&plan, &json!({"verdict":"accepted"}), &candidate, &control).is_err()
    );
}

#[test]
fn operation_stop_preserves_prior_observations_and_other_instances() {
    let before =
        json!({"b":"b1", "offer":"parent", "planner_exit":0, "trial":{"verdict":"accepted"}});
    let stopped = finish_instance(
        before.clone(),
        Err("start helper: executable unavailable".into()),
    );
    assert_eq!(stopped["trial"], before["trial"]);
    assert_eq!(stopped["planner_exit"], 0);
    assert_eq!(stopped["stopped"], "start helper: executable unavailable");
    let next = json!({"b":"b2", "offer":"same-cart", "decision":"not_applicable"});
    assert_eq!(finish_instance(next.clone(), Ok(())), next);
}
