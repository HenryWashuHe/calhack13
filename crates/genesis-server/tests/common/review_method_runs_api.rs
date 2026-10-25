//! HTTP over retained JSON only; no process, provider or evaluator executes.
use super::*;
use genesis_core::development_method::{ExecutionOutcome, ExecutionReceipt};
use genesis_core::development_trial::TrialPurpose;
use genesis_server::method_invocation::authorize_method_invocation;
use genesis_server::method_invocation_run::{invocation_grant, CreateMethodRun, MethodRunStatus};

#[tokio::test]
async fn review_disabled_receiver_retains_a_started_runs_receipt_after_fence() {
    let f = retained_adoption(None);
    let state = f.app.store.current_state().unwrap();
    let request = serde_json::from_value(f.ask.clone()).unwrap();
    let now = chrono::Utc::now();
    let authorization =
        authorize_method_invocation(&f.app.store, &state, &f.receiver, &request, now).unwrap();
    let mut plan: TrialPlan = serde_json::from_slice(
        &std::fs::read(f.receiver.records_root.join("fixed/plan.json")).unwrap(),
    )
    .unwrap();
    plan.id = "review-invocation-disabled".into();
    plan.slots.truncate(1);
    let slot = &mut plan.slots[0];
    slot.slot_id = "use".into();
    slot.run_id = "review-run".into();
    slot.attempt_id = "review-attempt".into();
    slot.scoring_request_id = "review-score".into();
    slot.purpose = TrialPurpose::Invocation {
        grant: invocation_grant(&authorization),
    };
    let (record, _) = f
        .app
        .store
        .prepare_method_run(
            &CreateMethodRun {
                client_request_id: "review-run-disabled".into(),
                request,
                plan,
            },
            &state,
            &f.receiver,
            now,
        )
        .unwrap();
    f.app
        .store
        .start_method_run(&record.id, &state, &f.receiver, now)
        .unwrap();
    let mut receipt: ExecutionReceipt = serde_json::from_slice(
        &std::fs::read(f.receiver.records_root.join("fixed/candidate/receipt.json")).unwrap(),
    )
    .unwrap();
    receipt.binding = record.plan.expected_binding("use").unwrap();
    receipt.binding_sha256 = receipt.binding.canonical_hash().unwrap();
    // This is explicitly a fixture with an unknown outcome, not run evidence.
    receipt.outcome = ExecutionOutcome::Unknown;
    receipt.outputs.clear();
    assert!(f.app.method_receiver.is_none());
    let (base, server) = start(f.app.clone()).await;
    let (status, body) = call(
        &format!("{base}/method-runs/{}/receipt", record.id),
        &f.token,
        json!(receipt),
    )
    .await;
    server.abort();
    assert_eq!(
        status, 200,
        "disabling new execution must not discard a matching final receipt: {body}"
    );
    assert_eq!(body["status"], "completed_after_fence");
    let retained = f.app.store.method_run(&record.id).unwrap();
    assert_eq!(retained.status, MethodRunStatus::CompletedAfterFence);
    assert_eq!(retained.receipt.unwrap(), receipt);
}
