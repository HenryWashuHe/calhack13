//! Inert records only: prepare/start mutate SQLite, never launch a method.
use super::*;

#[test]
fn review_prepare_refuses_a_binding_that_cannot_fit_any_receipt() {
    let r = Run::new();
    let mut plan = r.plan();
    // Unlike the public instances, the plan id is repeated inside every
    // matching receipt's bound purpose. It is valid but larger than the cap.
    plan.id = "p".repeat(70_000);
    plan.validate().unwrap();
    let result = r.prepare(&r.intent("oversized-binding", plan));
    if let Ok((record, _)) = &result {
        let binding = record.plan.expected_binding("use").unwrap();
        assert!(serde_json::to_vec(&binding).unwrap().len() > RECEIPT_ALLOWANCE);
        r.f.store
            .start_method_run(&record.id, &r.state(), &r.f.records.receiver, r.f.now)
            .unwrap();
        let receipt = r.receipt(record, ExecutionOutcome::Unknown);
        let completion = r.f.store.complete_method_run(
            &record.id,
            &receipt,
            &r.state(),
            &r.f.records.receiver,
            r.f.now,
        );
        assert!(matches!(completion, Err(E::Receipt(_))));
        assert_eq!(
            r.f.store.method_run(&record.id).unwrap().status,
            MethodRunStatus::Started
        );
    }
    assert!(
        result.is_err(),
        "a plan whose binding alone exceeds the receipt cap must be rejected before start"
    );
}
