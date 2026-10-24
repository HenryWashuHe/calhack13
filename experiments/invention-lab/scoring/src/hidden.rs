//! Comparison scoring on a committed hidden set (`genesis-lab-hidden/2`).
//!
//! The salt holder publishes a [`Descriptor`] before development; plans pin it
//! as their `instance_set` and pin [`protocol`] as the hidden-commitment
//! protocol. Development only ever sees the plan's public instances. After
//! every slot of the comparison has a finished execution whose method process
//! the executor saw end (the comparison is closed), the salt holder scores each retained output on the
//! committed draw here, privately. The salt is not revealed by scoring; the
//! reveal is separate evidence anyone can check with [`Descriptor::opens`] and
//! then re-run.
//!
//! Before anything runs: the plan must be a hidden-commitment plan naming this
//! protocol, its whole scoring context must be [`context`] for the descriptor,
//! the salt must open the descriptor, every slot's execution must be present
//! and bound to the plan with executor-observed termination evidence recorded
//! by [`record_termination`] (a recovered receipt has none), the step grant must cover a full episode per hidden
//! instance and the wall grant must be nonzero. Admitted work stops at the
//! wall grant. The trace (which names hidden seeds) stays in the scorer's
//! store; the receipt names it only by digest.
use crate::{run_jobs, Job, ScoringError};
use genesis_core::development_evaluation::{EvaluationReceipt, ScoringContext};
use genesis_core::development_method::{ExactContentRef, ExecutionReceipt};
use genesis_core::development_trial::{ScoringInstances, TrialPlan, TrialPurpose};
use genesis_lab_eval::commit::{Descriptor, PROTOCOL};
use genesis_lab_eval::score::score_comparison;
use genesis_lab_exec::refs;
use genesis_lab_exec::run::{receipt_sha256, Termination};
use genesis_lab_exec::store::Store;
use serde_json::json;

fn canonical(value: &serde_json::Value) -> Vec<u8> {
    genesis_core::digest::canonical_json(value).into_bytes()
}

/// The hidden-commitment protocol a comparison plan names.
pub fn protocol() -> ExactContentRef {
    refs::content(
        "lab-hidden-protocol/1",
        "genesis-lab-hidden",
        "2",
        &canonical(&json!({"protocol": PROTOCOL, "evaluator": refs::evaluator()})),
    )
}

/// The exact descriptor bytes a plan pins as its instance set.
pub fn descriptor_ref(d: &Descriptor) -> ExactContentRef {
    refs::content(
        "lab-hidden-set/1",
        &d.comparison,
        "2",
        &canonical(&serde_json::to_value(d).expect("descriptor serializes")),
    )
}

/// The complete scoring context this adapter implements for a hidden set:
/// the family's public task, evaluator, generator, schemas and rule, with the
/// committed descriptor as the instance set.
pub fn context(d: &Descriptor) -> ScoringContext {
    let mut c = refs::scoring_context(d.family, &[]);
    c.instance_set = descriptor_ref(d);
    c
}

/// A paired comparison over the same slots, methods, grants and public
/// development instances as `base`, scored on the committed hidden set. Each
/// slot becomes an arm of one trial.
pub fn comparison_plan(base: &TrialPlan, trial_id: &str, d: &Descriptor) -> TrialPlan {
    let mut p = base.clone();
    p.id = format!("{}-comparison", base.id);
    p.scoring_instances = ScoringInstances::HiddenCommitment {
        protocol: protocol(),
    };
    p.context = context(d);
    p.scoring_grant.simulation_steps = d.count as u64 * d.family.max_steps() as u64;
    for s in &mut p.slots {
        s.purpose = TrialPurpose::Comparison {
            trial_id: trial_id.into(),
            arm_id: s.slot_id.clone(),
        };
    }
    p
}

fn termination_name(slot_id: &str) -> String {
    format!(
        "termination-{}.json",
        &refs::sha256_hex(slot_id.as_bytes())[..16]
    )
}

/// Records, in the scorer's store, termination evidence the executor observed
/// for one slot (from `genesis_lab_exec::run::termination`). Hidden scoring
/// refuses until every slot of the comparison has a record bound to its exact
/// receipt.
pub fn record_termination(store: &Store, t: &Termination) -> std::io::Result<()> {
    store.write(
        &termination_name(&t.slot_id),
        &serde_json::to_vec_pretty(t).expect("termination serializes"),
    )
}

/// Scores one closed comparison slot's retained output on the hidden draw.
/// `closure` holds every slot's finished execution receipt, the scored slot's
/// included.
#[allow(clippy::too_many_arguments)]
pub fn score_hidden(
    plan: &TrialPlan,
    slot_id: &str,
    output_bytes: &[u8],
    closure: &[(&str, &ExecutionReceipt)],
    descriptor: &Descriptor,
    salt: &str,
    store: &Store,
) -> Result<EvaluationReceipt, ScoringError> {
    let refused = |why: &str| Err(ScoringError::Refused(why.into()));
    match &plan.scoring_instances {
        ScoringInstances::HiddenCommitment { protocol: p } if *p == protocol() => {}
        _ => return refused("not a comparison plan on this hidden-set protocol"),
    }
    if plan.context != context(descriptor) {
        return refused("the plan's scoring context is not this descriptor's");
    }
    if !descriptor.opens(salt) {
        return refused("the salt does not open the descriptor");
    }
    // Closed: every slot of the plan has exactly one execution bound to it,
    // and the scorer's store holds executor-observed termination evidence for
    // that exact receipt. A receipt alone isn't enough: a recovered receipt is
    // written without anyone seeing the method process end.
    for slot in &plan.slots {
        let mut found = closure.iter().filter(|(id, _)| *id == slot.slot_id);
        let (Some((_, receipt)), None) = (found.next(), found.next()) else {
            return refused("the comparison is not closed: a slot has no single execution");
        };
        plan.evaluation_request(&slot.slot_id, receipt)?;
        let ended = store
            .read(&termination_name(&slot.slot_id))
            .ok()
            .and_then(|b| serde_json::from_slice::<Termination>(&b).ok());
        let Some(ended) = ended
            .filter(|t| t.slot_id == slot.slot_id && t.execution_sha256 == receipt_sha256(receipt))
        else {
            return Err(ScoringError::Refused(format!(
                "the comparison is not closed: no termination evidence for slot {}",
                slot.slot_id
            )));
        };
        // A parent-only observation leaves descendants the method started
        // possibly still running; only a whole-tree observation closes it.
        if !ended.whole_tree {
            return Err(ScoringError::Refused(format!(
                "the comparison is not closed: slot {}'s termination covers only the method process, not its descendants",
                slot.slot_id
            )));
        }
    }
    if closure
        .iter()
        .any(|(id, _)| !plan.slots.iter().any(|s| s.slot_id == *id))
    {
        return refused("an execution names a slot outside the plan");
    }
    let execution = closure
        .iter()
        .find(|(id, _)| *id == slot_id)
        .map(|(_, r)| *r)
        .ok_or(ScoringError::Refused("no such slot".into()))?;
    let binding = plan.expected_binding(slot_id)?;
    let request = plan
        .evaluation_request(slot_id, execution)?
        .ok_or(ScoringError::NoOutput)?;
    if refs::sha256_hex(output_bytes) != request.output.sha256 {
        return refused("output bytes differ from the retained output");
    }
    let reserved = descriptor.count as u64 * descriptor.family.max_steps() as u64;
    if reserved > request.budget.simulation_steps {
        return Err(ScoringError::Refused(format!(
            "scoring needs {reserved} steps; the scoring grant allows {}",
            request.budget.simulation_steps
        )));
    }
    if request.budget.wall_time_ms == 0 {
        return refused("the scoring grant allows no wall time");
    }
    let jobs: Vec<Job> = (0..descriptor.count)
        .map(|i| Job {
            trace_label: json!({"hidden_index": i}),
            id: format!("hidden/{i}"),
        })
        .collect();
    run_jobs(
        slot_id,
        &request,
        execution,
        &binding,
        output_bytes,
        store,
        &jobs,
        |design, i| score_comparison(design, descriptor, salt, i as u32),
    )
}
