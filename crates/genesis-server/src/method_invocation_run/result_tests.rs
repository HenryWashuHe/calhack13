//! The scored-output step: one evaluation per completed run, attached to the
//! responsibility only under a grant that still stands. Evaluations are
//! #116's own, rebound to each run; no scorer runs.
use super::*;
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};

/// A run prepared, started and completed under the grant.
fn completed(r: &Run, name: &str) -> MethodRunRecord {
    let (record, _) = r.prepare(&r.intent(name, r.plan_named(name))).unwrap();
    let receiver = &r.f.records.receiver;
    r.f.store
        .start_method_run(&record.id, &r.state(), receiver, r.f.now)
        .unwrap();
    let receipt = r.receipt(&record, ExecutionOutcome::OutputGenerated);
    r.f.store
        .complete_method_run(&record.id, &receipt, &r.state(), receiver, r.f.now)
        .unwrap()
}

/// #116's candidate evaluation, rebound to this run's request.
fn evaluation(r: &Run, record: &MethodRunRecord) -> EvaluationReceipt {
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
    evaluation.request = request;
    evaluation
}

fn record(
    r: &Run,
    id: &str,
    evaluation: &EvaluationReceipt,
    state: &OrgState,
    receiver: Option<&MethodReceiver>,
) -> Result<MethodRunRecord> {
    r.f.store
        .record_method_run_result(id, evaluation, state, receiver, r.f.now)
}

#[test]
fn a_scored_output_under_a_standing_grant_is_attached_once() {
    let r = Run::new();
    let run = completed(&r, "attach");
    let eval = evaluation(&r, &run);
    let done = record(&r, &run.id, &eval, &r.state(), Some(&r.f.records.receiver)).unwrap();
    let result = done.result.unwrap();
    assert_eq!(result.evaluation, eval);
    assert_eq!(
        result.attachment,
        Attachment::Attached {
            requirement_id: r.authorization.requirement_id.as_str().into(),
            responsibility_id: r.authorization.grant.responsibility_id.as_str().into(),
            role_id: r.authorization.grant.role_id.as_str().into(),
            adoption_id: r.authorization.adoption_id.clone(),
        }
    );
    assert_eq!(r.f.store.method_run(&run.id).unwrap().result, Some(result));
    // One evaluation per run.
    assert!(matches!(
        record(&r, &run.id, &eval, &r.state(), Some(&r.f.records.receiver)),
        Err(E::Status(MethodRunStatus::Completed))
    ));
}

#[test]
fn an_evaluation_of_anything_else_is_refused() {
    let r = Run::new();
    let run = completed(&r, "foreign");
    // #116's evaluation as retained names #116's run, not this one.
    let path =
        r.f.records
            .receiver
            .records_root
            .join("vs-fixed-cart/candidate-score/evaluation.json");
    let original: EvaluationReceipt =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let receiver = Some(&r.f.records.receiver);
    assert!(matches!(
        record(&r, &run.id, &original, &r.state(), receiver),
        Err(E::Receipt(_))
    ));
    // Another run's evaluation doesn't fit either.
    let other = completed(&r, "other");
    let theirs = evaluation(&r, &other);
    assert!(matches!(
        record(&r, &run.id, &theirs, &r.state(), receiver),
        Err(E::Receipt(_))
    ));
    assert_eq!(r.f.store.method_run(&run.id).unwrap().result, None);
}

#[test]
fn only_completed_runs_take_a_result() {
    let r = Run::new();
    let (prepared, _) = r
        .prepare(&r.intent("early", r.plan_named("early")))
        .unwrap();
    let eval = evaluation(&r, &completed(&r, "template"));
    assert!(matches!(
        record(
            &r,
            &prepared.id,
            &eval,
            &r.state(),
            Some(&r.f.records.receiver)
        ),
        Err(E::Status(MethodRunStatus::Prepared))
    ));
}

#[test]
fn a_stale_or_unscored_result_is_kept_but_attaches_to_nothing() {
    let reason = |record: MethodRunRecord| match record.result.unwrap().attachment {
        Attachment::NotAttached { reason } => reason,
        other => panic!("attached: {other:?}"),
    };
    // The grant changed after completion.
    let r = Run::new();
    let run = completed(&r, "stale");
    let eval = evaluation(&r, &run);
    let mut changed = r.state();
    later_epoch(&mut changed, &run);
    let done = record(&r, &run.id, &eval, &changed, Some(&r.f.records.receiver)).unwrap();
    assert_eq!(reason(done), "the run's grant no longer authorizes");
    // The receiver is disabled.
    let run = completed(&r, "disabled");
    let eval = evaluation(&r, &run);
    let done = record(&r, &run.id, &eval, &r.state(), None).unwrap();
    assert_eq!(reason(done), "the run's grant no longer authorizes");
    // The output wasn't scored.
    let run = completed(&r, "failed");
    let mut eval = evaluation(&r, &run);
    eval.outcome = EvaluationOutcome::Failed {
        reason: "scorer crashed".into(),
    };
    let done = record(&r, &run.id, &eval, &r.state(), Some(&r.f.records.receiver)).unwrap();
    assert_eq!(reason(done), "the output was not scored");
}

#[test]
fn a_run_completed_after_its_fence_never_attaches() {
    let r = Run::new();
    let (prepared, _) = r.prepare(&r.intent("late", r.plan_named("late"))).unwrap();
    let receiver = &r.f.records.receiver;
    r.f.store
        .start_method_run(&prepared.id, &r.state(), receiver, r.f.now)
        .unwrap();
    let mut changed = r.state();
    later_epoch(&mut changed, &prepared);
    let receipt = r.receipt(&prepared, ExecutionOutcome::OutputGenerated);
    let late =
        r.f.store
            .complete_method_run(&prepared.id, &receipt, &changed, receiver, r.f.now)
            .unwrap();
    assert_eq!(late.status, MethodRunStatus::CompletedAfterFence);
    let eval = evaluation(&r, &late);
    // Even if the grant later stands again, the late run doesn't attach.
    let done = record(&r, &late.id, &eval, &r.state(), Some(receiver)).unwrap();
    assert_eq!(
        done.result.unwrap().attachment,
        Attachment::NotAttached {
            reason: "the run completed after its grant changed".into()
        }
    );
}
