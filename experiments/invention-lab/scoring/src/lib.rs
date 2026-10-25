//! Henry's Claude: trusted acceptance scoring (#22) for one frozen slot, a
//! separate crate from the executor: it never runs a method, and nothing here
//! is reachable by one.
//!
//! The request is rebuilt from the frozen plan and the retained execution
//! receipt (`TrialPlan::evaluation_request`), never taken from a caller. The
//! retained output bytes must match the request's output digest. Every public
//! development instance of the plan is simulated with the in-process evaluator.
//!
//! Before anything runs, every field of the requested scoring context must
//! equal what this adapter implements (`refs::scoring_context` for the plan's
//! one family and exact instance list), the step grant must cover a full
//! episode per instance and the wall grant must be nonzero. Admitted work stops
//! at the wall grant (`Failed`). Unknown, failed and invalid primitive runs are
//! kept as such, never folded into `Scored`. The receipt is written to
//! `evaluation.json` before validation, so an overrun keeps its accounting, and
//! is validated against the rebuilt request before it is returned. Hidden
//! (comparison) scoring is not implemented here.
use genesis_core::development_evaluation::{
    EvaluationError, EvaluationOutcome, EvaluationReceipt, ScoringContext,
};
use genesis_core::development_method::{ExecutionReceipt, ResourceUsage, Usage};
use genesis_core::development_trial::{ScoringInstances, TrialPlan};
use genesis_lab_eval::design::Design;
use genesis_lab_eval::score::{score_acceptance, Outcome, Receipt};
pub mod feedback;
pub mod verdict;

use genesis_lab_exec::refs;
use genesis_lab_exec::store::Store;
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
    let worlds = plan
        .development_instances
        .iter()
        .map(|r| refs::resolve_instance(r).map(|w| (r.clone(), w)))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| ScoringError::Refused("an instance does not resolve".into()))?;
    let family = worlds[0].1.family;
    if worlds.iter().any(|(_, w)| w.family != family) {
        return Err(ScoringError::Refused(
            "this adapter scores one task family per plan".into(),
        ));
    }
    // Every field of the requested context must be exactly what this adapter
    // implements for this family and instance list; the plan says what was
    // requested, and this check is what lets the receipt call it observed.
    let ids: Vec<String> = plan
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let supported = refs::scoring_context(family, &ids);
    if request.context != supported {
        let field = [
            (
                "public_task",
                request.context.public_task == supported.public_task,
            ),
            (
                "evaluator",
                request.context.evaluator == supported.evaluator,
            ),
            (
                "environment",
                request.context.environment == supported.environment,
            ),
            (
                "generator",
                request.context.generator == supported.generator,
            ),
            (
                "instance_set",
                request.context.instance_set == supported.instance_set,
            ),
            (
                "design_schema",
                request.context.design_schema == supported.design_schema,
            ),
            (
                "metric_schema",
                request.context.metric_schema == supported.metric_schema,
            ),
            (
                "scoring_rule",
                request.context.scoring_rule == supported.scoring_rule,
            ),
        ]
        .into_iter()
        .find_map(|(name, same)| (!same).then_some(name))
        .unwrap_or("context");
        return Err(ScoringError::Refused(format!(
            "this adapter does not implement the requested {field}"
        )));
    }
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
    if request.budget.wall_time_ms == 0 {
        return Err(ScoringError::Refused(
            "the scoring grant allows no wall time".into(),
        ));
    }
    let request_sha256 = request.canonical_hash(execution, &binding)?;
    let started = Instant::now();

    let deadline = std::time::Duration::from_millis(request.budget.wall_time_ms);
    let mut rows = vec![];
    let mut runs = vec![];
    let mut steps = Some(0u64);
    let mut stopped = None;
    let design = serde_json::from_slice::<Design>(output_bytes);
    if let Ok(design) = &design {
        for (reference, world) in &worlds {
            // Admitted work stops at the scoring wall grant.
            if started.elapsed() >= deadline {
                stopped = Some(format!(
                    "the scoring wall grant ran out after {} of {} instances",
                    runs.len(),
                    worlds.len()
                ));
                break;
            }
            let r = score_acceptance(design, world.family, world.seed)
                .map_err(|e| ScoringError::Refused(format!("{e:?}")))?;
            steps = match (steps, r.simulation_steps) {
                (Some(total), Some(s)) => Some(total + s as u64),
                _ => None,
            };
            rows.push(json!({"instance": reference, "receipt": r}));
            runs.push((reference.id.clone(), r));
        }
    }
    let trace = store_json(
        store,
        "lab-scoring-trace/1",
        &json!({"slot": slot_id, "runs": rows}),
    )?;
    let outcome = match (&design, stopped) {
        (Err(e), _) => EvaluationOutcome::Invalid {
            reason: format!("output is not a design: {e}"),
        },
        (Ok(_), Some(reason)) => EvaluationOutcome::Failed { reason },
        (Ok(_), None) => match aggregate(&runs) {
            Some(outcome) => outcome,
            None => {
                let per_instance: Vec<_> = runs
                    .iter()
                    .map(|(id, r)| {
                        let m = r.metrics.as_ref().expect("scored runs carry metrics");
                        json!({"instance": id, "success": m.success, "progress": m.progress, "cost": m.cost})
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
        },
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
    // Retained before validation, so an overrun or other contract failure
    // still leaves its accounting next to the artifacts it produced.
    store.write(
        "evaluation.json",
        &serde_json::to_vec_pretty(&receipt).expect("receipt serializes"),
    )?;
    receipt.validate_against(&request, execution, &binding)?;
    Ok(receipt)
}

/// The scoring outcome for a set of per-instance primitive runs, when it isn't
/// `Scored`: any unknown run makes it `Unknown`; otherwise any failed worker
/// makes it `Failed`; otherwise any invalid design makes it `Invalid`.
pub fn aggregate(runs: &[(String, Receipt)]) -> Option<EvaluationOutcome> {
    let first = |o: Outcome| runs.iter().find(|(_, r)| r.outcome == o);
    let why = |(id, r): &(String, Receipt), default: &str| {
        format!(
            "{id}: {}",
            r.reason.clone().unwrap_or_else(|| default.into())
        )
    };
    if let Some(run) = first(Outcome::Unknown) {
        return Some(EvaluationOutcome::Unknown {
            reason: why(run, "unknown scoring run"),
        });
    }
    if let Some(run) = first(Outcome::Failed) {
        return Some(EvaluationOutcome::Failed {
            reason: why(run, "scoring worker failed"),
        });
    }
    if let Some(run) = first(Outcome::Invalid) {
        return Some(EvaluationOutcome::Invalid {
            reason: why(run, "invalid design"),
        });
    }
    if runs.iter().any(|(_, r)| r.metrics.is_none()) {
        return Some(EvaluationOutcome::Failed {
            reason: "a scored run carried no metrics".into(),
        });
    }
    None
}
