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
    AcceptanceRole, BudgetAssessment, ContractError, ExactContentRef, ExecutionOutcome,
    ExecutionReceipt, ResourceGrant, ResourceUsage, Usage,
};
use crate::development_trial::{
    assess_trials, ScoringInstances, TrialEvidence, TrialPlan, TrialPurpose,
};
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

/// A bound public-slot observation under process-comparison/1's eligibility
/// rule, not an acceptance verdict, execution grant or complete experiment.
#[derive(Debug, Serialize)]
pub struct ProcessSlotAssessment {
    pub schema: &'static str,
    pub plan: ExactContentRef,
    pub slot_id: String,
    pub method: ExactContentRef,
    pub execution: ExecutionOutcome,
    pub scoring: Option<EvaluationOutcome>,
    pub development_usage: Usage,
    pub scoring_usage: Option<Usage>,
    pub development_steps: Option<u64>,
    pub development_overrun: bool,
    pub scoring_within_grant: bool,
    pub successes: Option<u64>,
    pub eligible: bool,
    pub reasons: Vec<String>,
}

fn process_overrun(usage: &Usage, grant: &ResourceGrant) -> Result<bool, String> {
    usage.validate().map_err(|e| e.to_string())?;
    match usage.assess(grant) {
        Ok(_) => Ok(false),
        Err(ContractError::BudgetExceeded(_)) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

/// Rebuild identities from the independently retained expected public plan.
/// Missing scoring/metrics stay absent; malformed or mismatched evidence is an
/// error. Every observed outcome/usage survives even when it is ineligible.
///
/// Unlike `decide`, eligibility requires OutputGenerated and all five known
/// execution usage fields. Only scoring must be within its grant under this
/// frozen rule; a development overrun is reported, never authorized. The caller
/// still pins the protocol's roster, controls, model settings, grants and timing.
/// HiddenCommitment is refused: its scoring instance order cannot be inferred
/// from a public plan's development instances.
pub fn assess_process_slot(
    plan: &TrialPlan,
    slot_id: &str,
    evidence: &SlotEvidence,
) -> Result<ProcessSlotAssessment, String> {
    if !matches!(plan.scoring_instances, ScoringInstances::Development {}) {
        return Err("process eligibility requires public Development instances".into());
    }
    let binding = plan.expected_binding(slot_id).map_err(|e| e.to_string())?;
    let execution = &evidence.execution;
    execution
        .validate_identity_against(&binding)
        .map_err(|e| e.to_string())?;
    let mut result = ProcessSlotAssessment {
        schema: "genesis-process-slot-assessment/1",
        plan: plan.reference().map_err(|e| e.to_string())?,
        slot_id: slot_id.into(),
        method: binding.method.clone(),
        execution: execution.outcome,
        scoring: None,
        development_usage: execution.usage.clone(),
        scoring_usage: None,
        development_steps: match execution.usage.simulation_steps {
            ResourceUsage::Measured { value } => Some(value),
            ResourceUsage::Unknown { .. } => None,
        },
        development_overrun: process_overrun(&execution.usage, &plan.development_grant)?,
        scoring_within_grant: false,
        successes: None,
        eligible: false,
        reasons: Vec::new(),
    };
    if let Some(scoring) = &evidence.scoring {
        let request = plan
            .evaluation_request_for_accounting(slot_id, execution)
            .map_err(|e| e.to_string())?
            .ok_or("scoring without retained output")?;
        scoring
            .validate_identity_against(&request, execution, &binding)
            .map_err(|e| e.to_string())?;
        result.scoring = Some(scoring.outcome.clone());
        result.scoring_usage = Some(scoring.usage.clone());
        result.scoring_within_grant = matches!(
            scoring.usage.assess(&plan.scoring_grant),
            Ok(BudgetAssessment::WithinLimits)
        );
        match &scoring.outcome {
            EvaluationOutcome::Scored { metrics } => {
                if metrics.domain != "genesis-lab"
                    || metrics.schema != "lab-scoring-metrics/1"
                    || metrics.id != "lab-scoring-metrics/1"
                    || metrics.version != "1"
                {
                    return Err("unsupported metrics identity".into());
                }
                if let Some(bytes) = &evidence.metrics {
                    if bytes.len() > 1024 * 1024 {
                        return Err("metrics exceed 1 MiB".into());
                    }
                    let instances: Vec<_> = plan
                        .development_instances
                        .iter()
                        .map(|r| r.id.clone())
                        .collect();
                    result.successes = Some(successes(scoring, Some(bytes), &instances)?);
                }
            }
            _ if evidence.metrics.is_some() => return Err("metrics without Scored receipt".into()),
            _ => {}
        }
    } else if evidence.metrics.is_some() {
        return Err("metrics without scoring receipt".into());
    }
    if execution.outcome != ExecutionOutcome::OutputGenerated {
        result
            .reasons
            .push(format!("execution outcome {:?}", execution.outcome));
    }
    let unknown = unknown_usage(execution);
    if !unknown.is_empty() {
        result
            .reasons
            .push(format!("development usage unknown: {}", unknown.join(", ")));
    }
    if !result.scoring_within_grant {
        result
            .reasons
            .push("scoring usage is missing, unknown or over grant".into());
    }
    if result.successes.is_none() {
        result
            .reasons
            .push("no validated Scored success count".into());
    }
    result.eligible = result.reasons.is_empty();
    Ok(result)
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
