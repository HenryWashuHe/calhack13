//! Trusted acceptance scoring (#22) for one frozen slot. Separate from method
//! execution: it never runs a method, and nothing here is reachable by one.
//!
//! The request is rebuilt from the frozen plan and the retained execution
//! receipt (`TrialPlan::evaluation_request`), never taken from a caller. The
//! retained output bytes must match the request's output digest. Every public
//! development instance of the plan is simulated with the in-process evaluator
//! under the plan's separate scoring grant, which is checked before anything
//! runs. The receipt is validated against the rebuilt request before it is
//! returned. Hidden (comparison) scoring is not implemented here.
use crate::refs;
use crate::store::Store;
use genesis_core::development_evaluation::{
    EvaluationError, EvaluationOutcome, EvaluationReceipt, ScoringContext,
};
use genesis_core::development_method::{ExecutionReceipt, ResourceUsage, Usage};
use genesis_core::development_trial::{ScoringInstances, TrialPlan};
use genesis_lab_eval::design::Design;
use genesis_lab_eval::score::{score_acceptance, Outcome};
use serde_json::json;
use std::time::Instant;

#[derive(Debug)]
pub enum ScoringError {
    /// No output was retained, so there is nothing to score (status Missing).
    NoOutput,
    /// Refused before simulating anything.
    Refused(String),
    Contract(EvaluationError),
    Store(std::io::Error),
}

impl From<EvaluationError> for ScoringError {
    fn from(e: EvaluationError) -> Self {
        Self::Contract(e)
    }
}
impl From<std::io::Error> for ScoringError {
    fn from(e: std::io::Error) -> Self {
        Self::Store(e)
    }
}

fn store_json(
    store: &Store,
    schema: &str,
    value: &serde_json::Value,
) -> std::io::Result<genesis_core::development_method::ExactContentRef> {
    let bytes = genesis_core::digest::canonical_json(value).into_bytes();
    let reference = refs::content(schema, schema, "1", &bytes);
    store.put_artifact(&reference, &bytes)?;
    Ok(reference)
}

/// Scores the slot's retained output on the plan's development instances.
pub fn score_slot(
    plan: &TrialPlan,
    slot_id: &str,
    execution: &ExecutionReceipt,
    output_bytes: &[u8],
    store: &Store,
) -> Result<EvaluationReceipt, ScoringError> {
    if !matches!(plan.scoring_instances, ScoringInstances::Development {}) {
        return Err(ScoringError::Refused(
            "only acceptance scoring on public development instances is implemented".into(),
        ));
    }
    let binding = plan.expected_binding(slot_id)?;
    let request = plan
        .evaluation_request(slot_id, execution)?
        .ok_or(ScoringError::NoOutput)?;
    if refs::sha256_hex(output_bytes) != request.output.sha256 {
        return Err(ScoringError::Refused(
            "output bytes differ from the retained output".into(),
        ));
    }
    if refs::evaluator() != request.context.evaluator {
        return Err(ScoringError::Refused(
            "linked evaluator differs from the plan".into(),
        ));
    }
    let worlds = plan
        .development_instances
        .iter()
        .map(|r| refs::resolve_instance(r).map(|w| (r.clone(), w)))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| ScoringError::Refused("an instance does not resolve".into()))?;
    let reserved: u64 = worlds
        .iter()
        .map(|(_, w)| w.family.max_steps() as u64)
        .sum();
    if reserved > request.budget.simulation_steps {
        return Err(ScoringError::Refused(format!(
            "scoring needs {reserved} steps; the scoring grant allows {}",
            request.budget.simulation_steps
        )));
    }
    let request_sha256 = request.canonical_hash(execution, &binding)?;
    let started = Instant::now();

    let mut rows = vec![];
    let mut steps = Some(0u64);
    let mut outcome = None;
    match serde_json::from_slice::<Design>(output_bytes) {
        Err(e) => {
            outcome = Some(EvaluationOutcome::Invalid {
                reason: format!("output is not a design: {e}"),
            })
        }
        Ok(design) => {
            for (reference, world) in &worlds {
                let r = score_acceptance(&design, world.family, world.seed)
                    .map_err(|e| ScoringError::Refused(format!("{e:?}")))?;
                steps = match (steps, r.simulation_steps) {
                    (Some(total), Some(s)) => Some(total + s as u64),
                    _ => None,
                };
                if r.outcome == Outcome::Unknown && outcome.is_none() {
                    outcome = Some(EvaluationOutcome::Unknown {
                        reason: format!(
                            "{}: {}",
                            reference.id,
                            r.reason.clone().unwrap_or_default()
                        ),
                    });
                }
                if r.outcome == Outcome::Invalid && outcome.is_none() {
                    outcome = Some(EvaluationOutcome::Invalid {
                        reason: r.reason.clone().unwrap_or_else(|| "invalid design".into()),
                    });
                }
                rows.push(json!({"instance": reference, "receipt": r}));
            }
        }
    }
    let trace = store_json(
        store,
        "lab-scoring-trace/1",
        &json!({"slot": slot_id, "runs": rows}),
    )?;
    let outcome = match outcome {
        Some(o) => o,
        None => {
            let per_instance: Vec<_> = rows
                .iter()
                .map(|row| {
                    let m = &row["receipt"]["metrics"];
                    json!({"instance": row["instance"]["id"], "success": m["success"], "progress": m["progress"], "cost": m["cost"]})
                })
                .collect();
            let successes = per_instance.iter().filter(|r| r["success"] == true).count();
            let metrics = store_json(
                store,
                "lab-scoring-metrics/1",
                &json!({"success_count": successes, "of": per_instance.len(), "instances": per_instance}),
            )?;
            EvaluationOutcome::Scored { metrics }
        }
    };
    let observed_context: Option<ScoringContext> = match outcome {
        EvaluationOutcome::Scored { .. } | EvaluationOutcome::Invalid { .. } => {
            Some(request.context.clone())
        }
        _ => None,
    };
    let receipt = EvaluationReceipt {
        request: request.clone(),
        request_sha256,
        observed_context,
        outcome,
        trace,
        usage: Usage {
            model_calls: ResourceUsage::Measured { value: 0 },
            input_tokens: ResourceUsage::Measured { value: 0 },
            output_tokens: ResourceUsage::Measured { value: 0 },
            wall_time_ms: ResourceUsage::Measured {
                value: started.elapsed().as_millis() as u64,
            },
            simulation_steps: match steps {
                Some(value) => ResourceUsage::Measured { value },
                None => ResourceUsage::Unknown {
                    reason: "a scoring run stopped without reporting".into(),
                },
            },
        },
    };
    receipt.validate_against(&request, execution, &binding)?;
    Ok(receipt)
}
