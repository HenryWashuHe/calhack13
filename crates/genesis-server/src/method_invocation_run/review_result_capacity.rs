//! Independent retained-record fixtures only. No method, scorer or process executes.
use super::*;
use genesis_core::development_evaluation::EvaluationReceipt;

fn evaluate_retained_run(r: &Run, record: &MethodRunRecord) -> EvaluationReceipt {
    let path =
        r.f.records
            .receiver
            .records_root
            .join("vs-fixed-cart/candidate-score/evaluation.json");
    let mut evaluation: EvaluationReceipt =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let receipt = record.receipt.as_ref().unwrap();
    let binding = record.plan.expected_binding("use").unwrap();
    let request = record
        .plan
        .evaluation_request_for_accounting("use", receipt)
        .unwrap()
        .unwrap();
    evaluation.request_sha256 = request.identity_hash(receipt, &binding).unwrap();
    evaluation.observed_context = Some(request.context.clone());
    evaluation.request = request;
    evaluation
        .validate_identity_against(&evaluation.request, receipt, &binding)
        .unwrap();
    evaluation
}

fn complete(r: &Run, record: &MethodRunRecord) -> MethodRunRecord {
    let receiver = &r.f.records.receiver;
    r.f.store
        .start_method_run(&record.id, &r.state(), receiver, r.f.now)
        .unwrap();
    r.f.store
        .complete_method_run(
            &record.id,
            &r.receipt(record, ExecutionOutcome::OutputGenerated),
            &r.state(),
            receiver,
            r.f.now,
        )
        .unwrap()
}

#[test]
fn review_result_capacity_control_accepts_an_ordinary_bound_evaluation() {
    let r = Run::new();
    let (record, _) = r.prepare(&r.intent("capacity-control", r.plan())).unwrap();
    let done = complete(&r, &record);
    let evaluation = evaluate_retained_run(&r, &done);
    let result = r.f.store.record_method_run_result(
        &done.id,
        &evaluation,
        &r.state(),
        Some(&r.f.records.receiver),
        r.f.now,
    );
    assert!(result.is_ok(), "ordinary bound result: {result:?}");
}

fn accepted_plan_can_record_result(mutate: impl FnOnce(&mut TrialPlan)) {
    let r = Run::new();
    let mut plan = r.plan_named("result-capacity");
    mutate(&mut plan);
    plan.validate().unwrap();
    let prepared = r.prepare(&r.intent("result-capacity", plan));
    if prepared.is_err() {
        return; // Refusing this impossible result before execution is acceptable.
    }
    let (record, _) = prepared.unwrap();
    let done = complete(&r, &record);
    let evaluation = evaluate_retained_run(&r, &done);
    let size = serde_json::to_vec(&evaluation).unwrap().len();
    assert!(size > EVALUATION_ALLOWANCE);
    let result = r.f.store.record_method_run_result(
        &done.id,
        &evaluation,
        &r.state(),
        Some(&r.f.records.receiver),
        r.f.now,
    );
    assert!(
        result.is_ok(),
        "prepare and completion succeeded but their required {size}-byte evaluation cannot be retained: {result:?}"
    );
}

#[test]
fn review_prepare_must_not_freeze_a_context_that_cannot_fit_a_scored_result() {
    // Scored receipts repeat this exact context in request and observed_context.
    // It is not part of the execution binding, so the existing binding cap cannot help.
    accepted_plan_can_record_result(|plan| plan.context.instance_set.id = "i".repeat(18_000));
}

#[test]
fn review_prepare_must_bound_the_scoring_request_id_before_execution() {
    // This changes no task, panel, source or evaluator reference at all.
    // The scorer must echo the frozen identifier, but the execution binding omits it.
    accepted_plan_can_record_result(|plan| plan.slots[0].scoring_request_id = "s".repeat(40_000));
}
