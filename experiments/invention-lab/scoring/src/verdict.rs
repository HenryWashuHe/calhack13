//! The receiving organization's acceptance verdict for a frozen candidate /
//! control plan, from validated evidence only.
//!
//! Every input is re-validated against the plan: `assess_trials` over both
//! slots' execution and scoring receipts, the criterion bytes against
//! `plan.criterion`, and each metrics artifact against its receipt. Anything
//! missing, failed or unknown is `Insufficient`, never a rejection or a pass.
//! A candidate whose retained output is an invalid design is `Rejected`.
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt, RequestStatus};
use genesis_core::development_method::{AcceptanceRole, ExecutionReceipt};
use genesis_core::development_trial::{assess_trials, TrialEvidence, TrialPlan, TrialPurpose};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// `lab-criterion/1`, rule `success-count-margin`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub rule: String,
    pub min_margin: u64,
    pub instances: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    let listed: Vec<&str> = rows.iter().filter_map(|r| r["instance"].as_str()).collect();
    if listed != instances.iter().map(String::as_str).collect::<Vec<_>>() {
        return Err("metrics cover different instances than the criterion".into());
    }
    Ok(rows.iter().filter(|r| r["success"] == true).count() as u64)
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
    let status = |slot: &str| {
        rows.iter()
            .find(|r| r.slot_id == slot)
            .map(|r| r.scoring_status)
    };
    match (status(cand_slot), status(ctrl_slot)) {
        (Some(RequestStatus::Invalid), _) => {
            return Ok(Verdict::Rejected {
                reason: "the candidate's retained output is not a valid design".into(),
            })
        }
        (Some(RequestStatus::Scored), Some(RequestStatus::Scored)) => {}
        (c, k) => {
            return Ok(Verdict::Insufficient {
                reason: format!("candidate scoring {c:?}, control scoring {k:?}"),
            })
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
