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
    /// A distinct invocation plan: its own plan, run, attempt and scoring ids.
    fn plan_named(&self, name: &str) -> TrialPlan {
        let mut plan = self.plan();
        plan.id = format!("invoke-{name}");
        let slot = &mut plan.slots[0];
        slot.run_id = format!("run-invoke-{name}");
        slot.attempt_id = genesis_core::ids::AttemptId::new(format!("att_invoke_{name}"));
        slot.scoring_request_id = format!("score-invoke-{name}");
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

    let (late, _) = r
        .prepare(&r.intent("fence-during", r.plan_named("second")))
        .unwrap();
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

#[test]
fn a_changed_retained_acceptance_plan_bounds_nothing() {
    // The accepted plan is re-read from the receiver's records; if its bytes
    // no longer match the adoption's evidence, no invocation is frozen.
    let r = Run::new();
    let plan = r.plan();
    let path =
        r.f.records
            .receiver
            .records_root
            .join("vs-fixed-cart/plan.json");
    let mut retained: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    retained["development_grant"]["simulation_steps"] = serde_json::json!(1_000_000);
    std::fs::write(&path, serde_json::to_vec(&retained).unwrap()).unwrap();
    assert!(matches!(
        r.prepare(&r.intent("tampered", plan)).unwrap_err(),
        E::InvalidRecord
    ));
}

#[test]
fn keys_sizes_and_stored_records_are_checked() {
    let r = Run::new();
    for key in ["", "k\n", &"k".repeat(257)] {
        assert!(matches!(
            r.prepare(&r.intent(key, r.plan())).unwrap_err(),
            E::InvalidInput
        ));
    }
    // The longest allowed key is accepted.
    assert!(r
        .prepare(&r.intent(&"k".repeat(256), r.plan_named("longest-key")))
        .is_ok());
    // A plan whose frozen record would exceed the record bound is refused.
    let mut huge = r.plan();
    let first = huge.development_instances[0].clone();
    huge.development_instances = (0..12_000)
        .map(|i| {
            let mut instance = first.clone();
            instance.id = format!("rough-transport/1/{}", 100_000 + i);
            instance
        })
        .collect();
    assert!(matches!(
        r.prepare(&r.intent("huge", huge)).unwrap_err(),
        E::InvalidInput
    ));
    // The indexed status must agree with the stored record.
    let (record, _) = r.prepare(&r.intent("indexed", r.plan())).unwrap();
    r.f.store
        .lock()
        .execute(
            "UPDATE method_runs SET status='started' WHERE id=?1",
            rusqlite::params![record.id],
        )
        .unwrap();
    assert!(matches!(
        r.f.store.method_run(&record.id).unwrap_err(),
        E::InvalidRecord
    ));
}

#[test]
fn the_run_count_is_bounded() {
    let r = Run::new();
    for n in 0..MAX_METHOD_RUNS {
        r.prepare(&r.intent(&format!("run-{n}"), r.plan_named(&n.to_string())))
            .unwrap();
    }
    assert!(matches!(
        r.prepare(&r.intent("one-too-many", r.plan())).unwrap_err(),
        E::Limit
    ));
}

#[test]
fn one_exact_plan_is_frozen_once() {
    // Identical plans share run and attempt ids; a receipt from one run would
    // fit the other, so a second run of the same plan is refused.
    let r = Run::new();
    r.prepare(&r.intent("first", r.plan())).unwrap();
    assert!(matches!(
        r.prepare(&r.intent("second", r.plan())).unwrap_err(),
        E::Plan(_)
    ));
}

#[test]
fn a_prepared_run_always_has_room_for_its_receipt() {
    // Independent review: a record just under the bound at prepare could never
    // take its receipt, leaving the run `started` for good.
    let r = Run::new();
    let receiver = &r.f.records.receiver;
    let first = r.plan().development_instances[0].clone();
    let sized = |n: usize, name: &str| {
        let mut plan = r.plan_named(name);
        plan.development_instances = (0..n)
            .map(|i| {
                let mut instance = first.clone();
                instance.id = format!("rough-transport/1/{}", 200_000 + i);
                instance
            })
            .collect();
        plan
    };
    // Find the largest plan prepare accepts (each probe is its own run).
    let (mut lo, mut hi) = (1usize, 12_000usize);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        let name = format!("probe-{mid}");
        if r.prepare(&r.intent(&name, sized(mid, &name))).is_ok() {
            lo = mid
        } else {
            hi = mid
        }
    }
    let (record, _) = r
        .prepare(&r.intent("largest", sized(lo, "largest")))
        .unwrap();
    r.f.store
        .start_method_run(&record.id, &r.state(), receiver, r.f.now)
        .unwrap();
    let receipt = r.receipt(&record, ExecutionOutcome::OutputGenerated);
    let done =
        r.f.store
            .complete_method_run(&record.id, &receipt, &r.state(), receiver, r.f.now)
            .unwrap();
    assert_eq!(done.status, MethodRunStatus::Completed);
}

#[test]
fn an_overrun_is_recorded_not_hidden() {
    let r = Run::new();
    let receiver = &r.f.records.receiver;
    let (record, _) = r.prepare(&r.intent("overrun", r.plan())).unwrap();
    r.f.store
        .start_method_run(&record.id, &r.state(), receiver, r.f.now)
        .unwrap();
    let mut receipt = r.receipt(&record, ExecutionOutcome::OutputGenerated);
    receipt.usage.simulation_steps = genesis_core::development_method::ResourceUsage::Measured {
        value: record.plan.development_grant.simulation_steps + 1,
    };
    let done =
        r.f.store
            .complete_method_run(&record.id, &receipt, &r.state(), receiver, r.f.now)
            .unwrap();
    assert_eq!(
        done.budget,
        Some(RunBudget::Exceeded {
            resource: "simulation_steps".into()
        })
    );
    // A compliant receipt is recorded as within limits.
    let (ok, _) = r
        .prepare(&r.intent("within", r.plan_named("within")))
        .unwrap();
    r.f.store
        .start_method_run(&ok.id, &r.state(), receiver, r.f.now)
        .unwrap();
    let fine =
        r.f.store
            .complete_method_run(
                &ok.id,
                &r.receipt(&ok, ExecutionOutcome::OutputGenerated),
                &r.state(),
                receiver,
                r.f.now,
            )
            .unwrap();
    assert_eq!(fine.budget, Some(RunBudget::WithinLimits));
}

#[test]
fn run_and_attempt_ids_are_never_reused() {
    let r = Run::new();
    // The accepted trial's own candidate ids are refused.
    let accepted = r.accepted();
    let mut reuse = r.plan_named("reuse");
    reuse.slots[0].attempt_id = accepted.slots[0].attempt_id.clone();
    assert!(matches!(
        r.prepare(&r.intent("reuse-accepted", reuse)).unwrap_err(),
        E::Plan(_)
    ));
    // Two runs can't share ids by varying only the plan id.
    r.prepare(&r.intent("first", r.plan_named("a"))).unwrap();
    let mut same_ids = r.plan_named("a");
    same_ids.id = "invoke-other".into();
    assert!(matches!(
        r.prepare(&r.intent("second", same_ids)).unwrap_err(),
        E::Plan(_)
    ));
}

#[path = "review_completion.rs"]
mod review_completion;

#[path = "result_tests.rs"]
mod result_tests;

#[path = "review_result_capacity.rs"]
mod review_result_capacity;
