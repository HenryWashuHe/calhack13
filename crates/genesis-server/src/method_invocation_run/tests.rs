//! Store tests over the real retained #116 records and the adoption-store
//! fixture. Receipts are #116's own, rebound to each invocation plan; no
//! method or scorer runs.
use super::*;
use crate::method_adoption_store::tests::Fixture;
use genesis_core::development_method::ExecutionOutcome;
use genesis_core::ids::RoleId;

struct Run {
    f: Fixture,
    authorization: MethodInvocationGrant,
    request: MethodInvocationRequest,
}

impl Run {
    fn new() -> Self {
        let f = Fixture::new();
        f.store.init_method_runs().unwrap();
        f.create(&f.intent).unwrap();
        let request = MethodInvocationRequest {
            role_id: f.intent.role_id.clone(),
            responsibility_id: f.intent.responsibility_id.clone(),
            publisher: f.intent.publisher.clone(),
            reference: f.intent.reference.clone(),
        };
        let authorization = authorize_method_invocation(
            &f.store,
            &f.store.current_state().unwrap(),
            &f.records.receiver,
            &request,
            f.now,
        )
        .unwrap();
        Self {
            f,
            authorization,
            request,
        }
    }
    fn accepted(&self) -> TrialPlan {
        let path = self
            .f
            .records
            .receiver
            .records_root
            .join("vs-fixed-cart/plan.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }
    /// The accepted plan reduced to its candidate slot, run under the grant.
    fn plan(&self) -> TrialPlan {
        let mut plan = self.accepted();
        plan.id = "invoke-rough-transport".into();
        plan.slots.truncate(1);
        let slot = &mut plan.slots[0];
        slot.slot_id = "use".into();
        slot.run_id = "run-invoke-use".into();
        slot.attempt_id = genesis_core::ids::AttemptId::new("att_invoke_use");
        slot.scoring_request_id = "score-invoke-use".into();
        slot.purpose = TrialPurpose::Invocation {
            grant: invocation_grant(&self.authorization),
        };
        plan
    }
    fn intent(&self, key: &str, plan: TrialPlan) -> CreateMethodRun {
        CreateMethodRun {
            client_request_id: key.into(),
            request: self.request.clone(),
            plan,
        }
    }
    fn state(&self) -> OrgState {
        self.f.store.current_state().unwrap()
    }
    fn prepare(&self, intent: &CreateMethodRun) -> Result<(MethodRunRecord, bool)> {
        self.f
            .store
            .prepare_method_run(intent, &self.state(), &self.f.records.receiver, self.f.now)
    }
    /// #116's candidate receipt, rebound to the run's frozen binding.
    fn receipt(&self, record: &MethodRunRecord, outcome: ExecutionOutcome) -> ExecutionReceipt {
        let path = self
            .f
            .records
            .receiver
            .records_root
            .join("vs-fixed-cart/candidate/receipt.json");
        let mut receipt: ExecutionReceipt =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let binding = record.plan.expected_binding("use").unwrap();
        receipt.binding_sha256 = binding.canonical_hash().unwrap();
        receipt.actual_method = binding.method.clone();
        receipt.actual_environment = binding.environment.clone();
        receipt.binding = binding;
        receipt.outcome = outcome;
        receipt
    }
}

fn later_epoch(state: &mut OrgState, run: &MethodRunRecord) {
    state
        .assignments
        .get_mut(&run.authorization.grant.assignment_id)
        .unwrap()
        .epoch += 1;
}

#[test]
fn a_run_is_prepared_started_and_completed_once_under_the_same_grant() {
    let r = Run::new();
    let intent = r.intent("run-1", r.plan());
    let (record, new) = r.prepare(&intent).unwrap();
    assert!(new);
    assert_eq!(record.status, MethodRunStatus::Prepared);
    assert_eq!(record.authorization, r.authorization);
    // Same key and intent: the stored record; same key, other intent: refused.
    assert_eq!(r.prepare(&intent).unwrap(), (record.clone(), false));
    let mut other = r.plan();
    other.id = "invoke-other".into();
    assert!(matches!(
        r.prepare(&r.intent("run-1", other)).unwrap_err(),
        E::IdempotencyConflict
    ));

    let receiver = &r.f.records.receiver;
    let started =
        r.f.store
            .start_method_run(&record.id, &r.state(), receiver, r.f.now)
            .unwrap();
    assert_eq!(started.status, MethodRunStatus::Started);
    // Started exactly once.
    assert!(matches!(
        r.f.store
            .start_method_run(&record.id, &r.state(), receiver, r.f.now)
            .unwrap_err(),
        E::Status(MethodRunStatus::Started)
    ));
    let receipt = r.receipt(&record, ExecutionOutcome::OutputGenerated);
    let done =
        r.f.store
            .complete_method_run(&record.id, &receipt, &r.state(), receiver, r.f.now)
            .unwrap();
    assert_eq!(done.status, MethodRunStatus::Completed);
    assert_eq!(done.receipt.as_ref(), Some(&receipt));
    // One receipt per run, never replaced.
    assert!(matches!(
        r.f.store
            .complete_method_run(&record.id, &receipt, &r.state(), receiver, r.f.now)
            .unwrap_err(),
        E::Status(MethodRunStatus::Completed)
    ));
    assert_eq!(r.f.store.method_run(&record.id).unwrap(), done);
}

#[test]
fn only_a_plan_bounded_by_the_adoption_is_frozen() {
    let r = Run::new();
    type Edit = fn(&mut TrialPlan, &Run);
    let cases: [(&str, Edit); 7] = [
        ("later epoch", |p, r| {
            let mut g = invocation_grant(&r.authorization);
            g.epoch += 1;
            p.slots[0].purpose = TrialPurpose::Invocation { grant: g };
        }),
        ("acceptance slot", |p, _| {
            p.slots[0].purpose = TrialPurpose::Acceptance {
                role: genesis_core::development_method::AcceptanceRole::Candidate,
            }
        }),
        ("other method", |p, r| {
            p.slots[0].method = r.accepted().slots[1].method.clone()
        }),
        ("other organization", |p, _| {
            p.slots[0].org_id = genesis_core::ids::OrgId::new("org_example_a")
        }),
        ("other task", |p, r| {
            p.context.public_task = r.accepted().context.evaluator.clone()
        }),
        ("larger grant", |p, _| {
            p.development_grant.simulation_steps += 1
        }),
        ("model settings", |p, r| {
            p.model_settings = r.accepted().context.evaluator.clone()
        }),
    ];
    for (n, (name, edit)) in cases.into_iter().enumerate() {
        let mut plan = r.plan();
        edit(&mut plan, &r);
        let error = r.prepare(&r.intent(&format!("bad-{n}"), plan)).unwrap_err();
        assert!(matches!(error, E::Plan(_)), "{name}: {error:?}");
    }
    // A smaller grant on other public instances is fine.
    let mut smaller = r.plan();
    smaller.development_grant.simulation_steps -= 1;
    smaller.development_instances.truncate(3);
    assert!(r.prepare(&r.intent("smaller", smaller)).is_ok());
}

#[test]
fn no_adoption_means_no_run() {
    let r = Run::new();
    let mut intent = r.intent("other-role", r.plan());
    intent.request.role_id = RoleId::new("role_without_adoption");
    assert!(matches!(
        r.prepare(&intent).unwrap_err(),
        E::Adoption(MethodAdoptionError::NotAdopted)
    ));
}

#[test]
fn a_changed_grant_fences_before_start_and_marks_a_late_receipt() {
    let r = Run::new();
    let receiver = &r.f.records.receiver;
    let (record, _) = r.prepare(&r.intent("fence-before", r.plan())).unwrap();
    let mut changed = r.state();
    later_epoch(&mut changed, &record);
    assert!(matches!(
        r.f.store
            .start_method_run(&record.id, &changed, receiver, r.f.now)
            .unwrap_err(),
        E::Status(MethodRunStatus::Fenced)
    ));
    assert_eq!(
        r.f.store.method_run(&record.id).unwrap().status,
        MethodRunStatus::Fenced
    );
    // Fenced is final, even if the grant were somehow current again.
    assert!(matches!(
        r.f.store
            .start_method_run(&record.id, &r.state(), receiver, r.f.now)
            .unwrap_err(),
        E::Status(MethodRunStatus::Fenced)
    ));

    let (late, _) = r.prepare(&r.intent("fence-during", r.plan())).unwrap();
    r.f.store
        .start_method_run(&late.id, &r.state(), receiver, r.f.now)
        .unwrap();
    let receipt = r.receipt(&late, ExecutionOutcome::OutputGenerated);
    let done =
        r.f.store
            .complete_method_run(&late.id, &receipt, &changed, receiver, r.f.now)
            .unwrap();
    assert_eq!(done.status, MethodRunStatus::CompletedAfterFence);
}

#[test]
fn receipts_must_bind_to_the_frozen_plan_and_unknown_is_final() {
    let r = Run::new();
    let receiver = &r.f.records.receiver;
    let (record, _) = r.prepare(&r.intent("receipts", r.plan())).unwrap();
    r.f.store
        .start_method_run(&record.id, &r.state(), receiver, r.f.now)
        .unwrap();
    // #116's original receipt is bound to its acceptance trial, not this run.
    let path = receiver
        .records_root
        .join("vs-fixed-cart/candidate/receipt.json");
    let original: ExecutionReceipt = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(matches!(
        r.f.store
            .complete_method_run(&record.id, &original, &r.state(), receiver, r.f.now)
            .unwrap_err(),
        E::Receipt(_)
    ));
    assert_eq!(
        r.f.store.method_run(&record.id).unwrap().status,
        MethodRunStatus::Started
    );
    let unknown = r.receipt(&record, ExecutionOutcome::Unknown);
    let done =
        r.f.store
            .complete_method_run(&record.id, &unknown, &r.state(), receiver, r.f.now)
            .unwrap();
    assert_eq!(done.receipt.unwrap().outcome, ExecutionOutcome::Unknown);
}
