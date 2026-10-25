//! The receiving organization's acceptance verdict for a frozen candidate /
//! control plan, from validated evidence only.
//!
//! Every input is re-validated against the plan: `assess_trials` over both
//! slots' execution and scoring receipts, the criterion bytes against
//! `plan.criterion`, and each metrics artifact against its receipt and the
//! criterion's exact instances. Acceptance needs both slots' development to
//! have a known outcome (a confirmed failure or interruption counts when its
//! retained output was scored, per adoption.md) with fully known resource
//! accounting, and both scorings to be within their grant. A usage field may be unknown only if the
//! frozen criterion explicitly lists it in `unknown_usage_allowed`. Anything
//! missing, failed or unknown otherwise is `Insufficient`, never a rejection or
//! a pass. A candidate whose retained output is an invalid design is
//! `Rejected`.
use crate::development_evaluation::{EvaluationOutcome, EvaluationReceipt, RequestStatus};
use crate::development_method::{
    AcceptanceRole, BudgetAssessment, ExecutionOutcome, ExecutionReceipt, ResourceUsage,
};
use crate::development_trial::{assess_trials, TrialEvidence, TrialPlan, TrialPurpose};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// `lab-criterion/1`, rule `success-count-margin`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub rule: String,
    pub min_margin: u64,
    pub instances: Vec<String>,
    /// Usage fields the receiving organization explicitly accepts as unknown
    /// (for example model calls when the sandbox can't establish zero).
    #[serde(default)]
    pub unknown_usage_allowed: Vec<String>,
}

const USAGE_FIELDS: [&str; 5] = [
    "model_calls",
    "input_tokens",
    "output_tokens",
    "wall_time_ms",
    "simulation_steps",
];

/// The usage fields an execution reports as unknown.
fn unknown_usage(execution: &ExecutionReceipt) -> Vec<&'static str> {
    let u = &execution.usage;
    [
        &u.model_calls,
        &u.input_tokens,
        &u.output_tokens,
        &u.wall_time_ms,
        &u.simulation_steps,
    ]
    .into_iter()
    .zip(USAGE_FIELDS)
    .filter_map(|(usage, name)| matches!(usage, ResourceUsage::Unknown { .. }).then_some(name))
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Accepted {
        candidate: u64,
        control: u64,
        of: u64,
    },
    Rejected {
        reason: String,
    },
    Insufficient {
        reason: String,
    },
}

pub struct SlotEvidence {
    pub execution: ExecutionReceipt,
    pub scoring: Option<EvaluationReceipt>,
    /// Bytes of the metrics artifact a `Scored` receipt names.
    pub metrics: Option<Vec<u8>>,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn slot_for(plan: &TrialPlan, role: AcceptanceRole) -> Option<&str> {
    plan.slots.iter().find_map(|s| match s.purpose {
        TrialPurpose::Acceptance { role: r } if r == role => Some(s.slot_id.as_str()),
        _ => None,
    })
}

/// Successes from a validated `Scored` receipt's metrics artifact.
fn successes(
    receipt: &EvaluationReceipt,
    metrics: Option<&[u8]>,
    instances: &[String],
) -> Result<u64, String> {
    let EvaluationOutcome::Scored { metrics: reference } = &receipt.outcome else {
        return Err("not scored".into());
    };
    let bytes = metrics.ok_or("metrics artifact missing")?;
    if sha(bytes) != reference.sha256 {
        return Err("metrics bytes differ from the receipt".into());
    }
    let m: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("metrics: {e}"))?;
    let rows = m["instances"]
        .as_array()
        .ok_or("metrics without instances")?;
    // Exactly one row per criterion instance, in order, each with an exact
    // instance id and a boolean success; nothing else is counted.
    if rows.len() != instances.len() {
        return Err(format!(
            "metrics have {} rows for {} criterion instances",
            rows.len(),
            instances.len()
        ));
    }
    let mut successes = 0;
    for (row, expected) in rows.iter().zip(instances) {
        if row["instance"].as_str() != Some(expected.as_str()) {
            return Err(format!(
                "metrics row does not name criterion instance {expected}"
            ));
        }
        match row["success"].as_bool() {
            Some(true) => successes += 1,
            Some(false) => {}
            None => return Err(format!("metrics row for {expected} has no boolean success")),
        }
    }
    Ok(successes)
}

pub fn decide(
    plan: &TrialPlan,
    criterion_bytes: &[u8],
    candidate: &SlotEvidence,
    control: &SlotEvidence,
) -> Result<Verdict, String> {
    if sha(criterion_bytes) != plan.criterion.sha256 {
        return Err("criterion bytes differ from the plan's criterion".into());
    }
    let criterion: Criterion =
        serde_json::from_slice(criterion_bytes).map_err(|e| format!("criterion: {e}"))?;
    if criterion.rule != "success-count-margin" {
        return Err(format!("unsupported criterion rule {}", criterion.rule));
    }
    if let Some(bad) = criterion
        .unknown_usage_allowed
        .iter()
        .find(|f| !USAGE_FIELDS.contains(&f.as_str()))
    {
        return Err(format!(
            "criterion allows unknown usage of unknown field {bad}"
        ));
    }
    let plan_ids: Vec<String> = plan
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    if criterion.instances != plan_ids {
        return Err("criterion instances differ from the plan's public instances".into());
    }
    let cand_slot = slot_for(plan, AcceptanceRole::Candidate).ok_or("no candidate slot")?;
    let ctrl_slot = slot_for(plan, AcceptanceRole::Control).ok_or("no control slot")?;
    let evidence = [(cand_slot, candidate), (ctrl_slot, control)].map(|(slot, e)| TrialEvidence {
        slot_id: slot.into(),
        execution: e.execution.clone(),
        scoring: e.scoring.clone(),
    });
    let rows = assess_trials(plan, &evidence).map_err(|e| format!("evidence: {e}"))?;
    let row = |slot: &str| rows.iter().find(|r| r.slot_id == slot).unwrap();
    let (cand_row, ctrl_row) = (row(cand_slot), row(ctrl_slot));
    // adoption.md: an unknown executor or evaluator outcome on either slot is
    // never a verdict, not even a rejection of an invalid candidate design.
    for (name, r) in [("candidate", cand_row), ("control", ctrl_row)] {
        if matches!(
            r.development_outcome,
            None | Some(ExecutionOutcome::Unknown)
        ) {
            return Ok(Verdict::Insufficient {
                reason: format!(
                    "{name} development outcome {:?} (unknown)",
                    r.development_outcome
                ),
            });
        }
        if r.scoring_status == RequestStatus::Unknown {
            return Ok(Verdict::Insufficient {
                reason: format!("{name} scoring outcome unknown"),
            });
        }
    }
    if cand_row.scoring_status == RequestStatus::Invalid {
        return Ok(Verdict::Rejected {
            reason: "the candidate's retained output is not a valid design".into(),
        });
    }
    for (name, r, e) in [
        ("candidate", cand_row, candidate),
        ("control", ctrl_row, control),
    ] {
        // A confirmed failure or interruption whose retained output was
        // scored still counts (one without scoreable output fails the scoring
        // check below).
        let unknown: Vec<_> = unknown_usage(&e.execution)
            .into_iter()
            .filter(|f| !criterion.unknown_usage_allowed.iter().any(|a| a == f))
            .collect();
        if !unknown.is_empty() {
            return Ok(Verdict::Insufficient {
                reason: format!("{name} development usage unknown: {}", unknown.join(", ")),
            });
        }
        if r.scoring_status != RequestStatus::Scored {
            return Ok(Verdict::Insufficient {
                reason: format!("{name} scoring {:?}", r.scoring_status),
            });
        }
        if r.scoring_accounting != Some(BudgetAssessment::WithinLimits) {
            return Ok(Verdict::Insufficient {
                reason: format!("{name} scoring accounting {:?}", r.scoring_accounting),
            });
        }
    }
    let count = |e: &SlotEvidence| {
        successes(
            e.scoring.as_ref().unwrap(),
            e.metrics.as_deref(),
            &criterion.instances,
        )
    };
    let (c, k) = (count(candidate)?, count(control)?);
    Ok(if c >= k.saturating_add(criterion.min_margin) {
        Verdict::Accepted {
            candidate: c,
            control: k,
            of: criterion.instances.len() as u64,
        }
    } else {
        Verdict::Rejected {
            reason: format!(
                "candidate {c}/{n} does not exceed control {k}/{n} by {}",
                criterion.min_margin,
                n = criterion.instances.len()
            ),
        }
    })
}
