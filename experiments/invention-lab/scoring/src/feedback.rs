//! Receiver feedback: what a receiving organization observed when it ran one
//! frozen slot on its **public development instances**, as one canonical
//! public document another organization may use (for example as a revision
//! input).
//!
//! Everything in it comes from retained records that are checked first: the
//! ledger must validate against the execution receipt, the scoring receipt
//! must bind to that execution under the plan, and every artifact read must
//! match its digest. It holds no hidden instances, no scorer internals beyond
//! the per-instance metrics already in the scoring artifact, and no claim:
//! it is an observation, not a verdict, an acceptance or an improvement.
use crate::ScoringError;
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_experiment::{ExperimentLedger, ExperimentOutcome, ExperimentScope};
use genesis_core::development_method::{ExactContentRef, ExecutionReceipt};
use genesis_core::development_trial::{assess_trials, ScoringInstances, TrialEvidence, TrialPlan};
use genesis_lab_exec::refs;
use genesis_lab_exec::store::Store;
use serde_json::{json, Value};

pub const SCHEMA: &str = "lab-receiver-feedback/1";
/// Experiments listed at most; later ones are counted, not listed.
pub const MAX_LISTED_EXPERIMENTS: usize = 64;
/// The whole document fits one revision public input (the forge's 128 KiB
/// per-document limit). An experiment whose design doesn't fit is listed with
/// the design's digest and size instead; listing stops when even that doesn't
/// fit. A document that can't fit with no experiments listed is refused.
pub const MAX_FEEDBACK_BYTES: usize = 128 * 1024;

fn refused(why: impl Into<String>) -> ScoringError {
    ScoringError::Refused(why.into())
}

/// Reads `artifacts/<sha256>` from `store` and checks the bytes match.
fn artifact(store: &Store, reference: &ExactContentRef) -> Result<Value, ScoringError> {
    let bytes = store.read(&format!("artifacts/{}", reference.sha256))?;
    if refs::sha256_hex(&bytes) != reference.sha256 {
        return Err(refused(format!(
            "artifact {} differs from its digest",
            reference.sha256
        )));
    }
    serde_json::from_slice(&bytes).map_err(|e| refused(format!("artifact: {e}")))
}

fn canonical_len(value: &Value) -> usize {
    genesis_core::digest::canonical_json(value).len()
}

fn read<T: serde::de::DeserializeOwned>(store: &Store, name: &str) -> Result<T, ScoringError> {
    serde_json::from_slice(&store.read(name)?).map_err(|e| refused(format!("{name}: {e}")))
}

/// Builds the feedback document for `slot_id` from its state and score
/// stores, returning its content reference and canonical bytes.
pub fn receiver_feedback(
    plan: &TrialPlan,
    slot_id: &str,
    state: &Store,
    score: &Store,
) -> Result<(ExactContentRef, Vec<u8>), ScoringError> {
    // Only acceptance records on public development instances may be
    // exported; hidden-comparison scoring is never public, whatever its hashes.
    if !matches!(plan.scoring_instances, ScoringInstances::Development {}) {
        return Err(refused(
            "feedback is only for scoring on public development instances",
        ));
    }
    let binding = plan.expected_binding(slot_id)?;
    let ledger: ExperimentLedger = read(state, "ledger.json")?;
    let execution: ExecutionReceipt = read(state, "receipt.json")?;
    let evaluation: EvaluationReceipt = read(score, "evaluation.json")?;
    ledger
        .validate_execution(ExperimentScope { plan, slot_id }, &execution)
        .map_err(|e| refused(format!("execution: {e:?}")))?;
    let rows = assess_trials(
        plan,
        &[TrialEvidence {
            slot_id: slot_id.into(),
            execution: execution.clone(),
            scoring: Some(evaluation.clone()),
        }],
    )
    .map_err(|e| refused(format!("trial: {e:?}")))?;
    let row = rows
        .iter()
        .find(|r| r.slot_id == slot_id)
        .ok_or_else(|| refused("slot not in plan"))?;
    let EvaluationOutcome::Scored { metrics } = &evaluation.outcome else {
        return Err(refused("scoring outcome is not scored"));
    };
    let scoring = artifact(score, metrics)?;
    let output = execution
        .outputs
        .first()
        .ok_or_else(|| refused("no retained output"))?;

    let mut entries = vec![];
    for record in ledger.records().iter().take(MAX_LISTED_EXPERIMENTS) {
        let call = &record.call;
        let result = match record.settlement.as_ref().map(|s| &s.outcome) {
            Some(ExperimentOutcome::Returned { result, .. }) => {
                let r = artifact(state, result)?;
                json!({"outcome": r["outcome"], "steps": r["steps"], "metrics": r["metrics"]})
            }
            Some(ExperimentOutcome::Failed { .. }) => json!({"outcome": "failed"}),
            Some(ExperimentOutcome::Unknown { .. }) => json!({"outcome": "unknown"}),
            None => json!({"outcome": "pending"}),
        };
        let full = json!({
            "instance": call.instance.id,
            "design": artifact(state, &call.design)?,
            "result": result,
        });
        let omitted = json!({
            "instance": call.instance.id,
            "design_omitted": {"sha256": call.design.sha256, "bytes": canonical_len(&full["design"])},
            "result": result,
        });
        entries.push((full, omitted));
    }
    let submitted = artifact(state, output)?;
    let plan_ref = plan.reference()?;
    let build = |experiments: &[Value]| {
        json!({
            "schema": SCHEMA,
            "authority": "observations on the receiver's public development instances; not a verdict, acceptance or improvement claim",
            "receiver_org": binding.org_id,
            "plan": plan_ref,
            "slot": slot_id,
            "method": binding.method,
            "public_instances": plan.development_instances.iter().map(|r| &r.id).collect::<Vec<_>>(),
            "development": {
                "outcome": execution.outcome,
                "usage": execution.usage,
                "experiments_total": ledger.records().len(),
                "experiments_listed": experiments.len(),
                "experiments": experiments,
            },
            "submitted_design": submitted,
            "scoring_status": format!("{:?}", row.scoring_status),
            "scoring_accounting": format!("{:?}", row.scoring_accounting),
            "scoring": scoring,
        })
    };
    // Canonical arrays are `[a,b,...]`, so each listed entry adds its own
    // length plus one comma; the counter's digits are covered by the margin.
    let mut size = canonical_len(&build(&[])) + 16;
    let mut experiments = vec![];
    for (full, omitted) in entries {
        let comma = usize::from(!experiments.is_empty());
        let pick = [full, omitted]
            .into_iter()
            .find(|e| size + comma + canonical_len(e) <= MAX_FEEDBACK_BYTES);
        let Some(entry) = pick else { break };
        size += comma + canonical_len(&entry);
        experiments.push(entry);
    }
    let doc = build(&experiments);
    let bytes = genesis_core::digest::canonical_json(&doc).into_bytes();
    if bytes.len() > MAX_FEEDBACK_BYTES {
        return Err(refused(format!(
            "feedback is {} bytes, over the {MAX_FEEDBACK_BYTES}-byte public input limit",
            bytes.len()
        )));
    }
    let reference = refs::content(SCHEMA, slot_id, "1", &bytes);
    Ok((reference, bytes))
}
