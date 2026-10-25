//! Three-round development selection from process-comparison/1. Pure reporting;
//! trusted expected plans are inputs, never recovered from incoming receipts.
//! This is not the full 24-cell roster, chronology, endpoint or adoption gate.
use genesis_core::{
    development_evaluation::EvaluationOutcome,
    development_method::*,
    development_trial::{TrialEvidence, TrialPlan, TrialPurpose},
    development_verdict::Criterion,
    digest::canonical_sha256,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{cmp::Reverse, collections::BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    RavineCrossing,
    RoughTransport,
    ReturnMix,
    ProjectileTarget,
}
impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::RavineCrossing => "ravine-crossing",
            Self::RoughTransport => "rough-transport",
            Self::ReturnMix => "return-mix",
            Self::ProjectileTarget => "projectile-target",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Workflow,
    Fresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoundInput {
    /// Caller-declared absence, not proof of a generation outcome or zero cost.
    NoMethod {
        reason: String,
    },
    Trial {
        input: Box<TrialInput>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialInput {
    pub plan: TrialPlan,
    pub criterion: Vec<u8>,
    pub evidence: Option<TrialEvidence>,
    pub metrics: Option<Vec<u8>>,
}
#[derive(Debug, Serialize)]
pub struct SelectionRow {
    pub round: u8,
    pub no_method: Option<String>,
    pub plan: Option<ExactContentRef>,
    pub method: Option<ExactContentRef>,
    pub evidence_sha256: Option<String>,
    pub execution: Option<ExecutionOutcome>,
    pub scoring: Option<EvaluationOutcome>,
    pub development_usage: Option<Usage>,
    pub scoring_usage: Option<Usage>,
    pub development_overrun: bool,
    pub scoring_within_grant: bool,
    pub successes: Option<u64>,
    pub eligible: bool,
}
#[derive(Debug, Serialize)]
pub struct SelectionReport {
    pub schema: &'static str,
    pub protocol: &'static str,
    pub scope: &'static str,
    pub arm: Arm,
    pub family: Family,
    pub input_sha256: String,
    /// Always all three original positions; absent evidence is never scored 0.
    pub rows: Vec<SelectionRow>,
    /// One-based position into rows, or no eligible candidate.
    pub selected_round: Option<u8>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn fail(message: &str) -> String {
    message.into()
}
fn known(usage: &Usage) -> bool {
    [
        &usage.model_calls,
        &usage.input_tokens,
        &usage.output_tokens,
        &usage.wall_time_ms,
        &usage.simulation_steps,
    ]
    .iter()
    .all(|v| matches!(v, ResourceUsage::Measured { .. }))
}
fn overrun(usage: &Usage, grant: &ResourceGrant) -> Result<bool, String> {
    usage.validate().map_err(|e| e.to_string())?;
    match usage.assess(grant) {
        Ok(_) => Ok(false),
        Err(ContractError::BudgetExceeded(_)) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}
fn count(input: &TrialInput, outcome: &EvaluationOutcome) -> Result<Option<u64>, String> {
    let EvaluationOutcome::Scored { metrics } = outcome else {
        if input.metrics.is_some() {
            return Err(fail("metrics without Scored receipt"));
        }
        return Ok(None);
    };
    if metrics.domain != "genesis-lab"
        || metrics.schema != "lab-scoring-metrics/1"
        || metrics.id != "lab-scoring-metrics/1"
        || metrics.version != "1"
    {
        return Err(fail("unsupported metrics identity"));
    }
    let Some(bytes) = &input.metrics else {
        return Ok(None);
    };
    if bytes.len() > 1024 * 1024 || hash(bytes) != metrics.sha256 {
        return Err(fail("metrics bounds or digest mismatch"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let rows = value["instances"]
        .as_array()
        .ok_or("missing metrics instances")?;
    if rows.len() != 6 {
        return Err(fail("metrics must contain all six instances"));
    }
    rows.iter()
        .zip(&input.plan.development_instances)
        .try_fold(0, |sum, (row, instance)| {
            if row["instance"].as_str() != Some(&instance.id) {
                return Err(fail("metrics instance identity or order differs"));
            }
            row["success"]
                .as_bool()
                .map(|v| sum + u64::from(v))
                .ok_or_else(|| fail("metrics success is not boolean"))
        })
        .map(Some)
}

/// Caller must retain these expected plans independently of evidence and freeze
/// the resulting choice before the fresh panel. Hashes do not establish timing.
pub fn select_development(
    arm: Arm,
    family: Family,
    rounds: &[RoundInput; 3],
) -> Result<SelectionReport, String> {
    let (mut plans, mut runs, mut attempts, mut scoring_ids) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    let mut cohort = None;
    let mut rows = Vec::new();
    for (index, round) in rounds.iter().enumerate() {
        let mut row = SelectionRow {
            round: index as u8 + 1,
            no_method: None,
            plan: None,
            method: None,
            evidence_sha256: None,
            execution: None,
            scoring: None,
            development_usage: None,
            scoring_usage: None,
            development_overrun: false,
            scoring_within_grant: false,
            successes: None,
            eligible: false,
        };
        match round {
            RoundInput::NoMethod { reason } => {
                if reason.trim().is_empty() || reason.len() > 2048 {
                    return Err(fail("absence reason must be nonempty and bounded"));
                }
                row.no_method = Some(reason.clone());
            }
            RoundInput::Trial { input } => {
                let plan = &input.plan;
                let reference = plan.reference().map_err(|e| e.to_string())?;
                if !plans.insert((reference.id.clone(), reference.version.clone())) {
                    return Err(fail("repeated logical trial identity"));
                }
                if plan.development_instances.len() != 6 || plan.slots.len() != 2 {
                    return Err(fail(
                        "selection needs six instances and a candidate/control pair",
                    ));
                }
                for (seed, instance) in plan.development_instances.iter().enumerate() {
                    if instance.domain != "genesis-lab"
                        || instance.schema != "lab-instance/1"
                        || instance.version != "1"
                        || instance.id != format!("{}/1/{seed}", family.name())
                    {
                        return Err(fail(
                            "selection requires the declared development family and seeds",
                        ));
                    }
                }
                if input.criterion.len() > 64 * 1024
                    || hash(&input.criterion) != plan.criterion.sha256
                {
                    return Err(fail("criterion bounds or digest mismatch"));
                }
                let criterion: Criterion =
                    serde_json::from_slice(&input.criterion).map_err(|e| e.to_string())?;
                if criterion.rule != "success-count-margin"
                    || criterion.min_margin != 1
                    || !criterion.unknown_usage_allowed.is_empty()
                    || criterion
                        .instances
                        .iter()
                        .ne(plan.development_instances.iter().map(|r| &r.id))
                {
                    return Err(fail("criterion differs from the strict development policy"));
                }
                let slot = |role| {
                    plan.slots
                        .iter()
                        .find(|s| s.purpose == TrialPurpose::Acceptance { role })
                        .ok_or("not an acceptance pair")
                };
                let candidate = slot(AcceptanceRole::Candidate)?;
                let control = slot(AcceptanceRole::Control)?;
                for slot in &plan.slots {
                    if !runs.insert((slot.org_id.clone(), slot.run_id.clone()))
                        || !attempts.insert((slot.org_id.clone(), slot.attempt_id.clone()))
                        || !scoring_ids
                            .insert((slot.org_id.clone(), slot.scoring_request_id.clone()))
                    {
                        return Err(fail("repeated run, attempt or scoring identity"));
                    }
                }
                // All policy fields must agree; only per-trial identities and
                // the candidate method/dependencies may differ between rounds.
                let mut policy = serde_json::to_value(plan).map_err(|e| e.to_string())?;
                policy.as_object_mut().unwrap().remove("id");
                policy["slots"] = json!({"org":candidate.org_id,
                    "control":control.method, "dependencies":control.dependencies});
                if cohort.as_ref().is_some_and(|old| old != &policy) {
                    return Err(fail(
                        "rounds differ in frozen task, control or resource policy",
                    ));
                }
                cohort = Some(policy);
                row.plan = Some(reference);
                row.method = Some(candidate.method.clone());
                if let Some(evidence) = &input.evidence {
                    if evidence.slot_id != candidate.slot_id {
                        return Err(fail("evidence is not the candidate slot"));
                    }
                    let binding = plan
                        .expected_binding(&candidate.slot_id)
                        .map_err(|e| e.to_string())?;
                    let execution = &evidence.execution;
                    execution
                        .validate_identity_against(&binding)
                        .map_err(|e| e.to_string())?;
                    row.evidence_sha256 = Some(canonical_sha256(&json!(evidence)));
                    row.execution = Some(execution.outcome);
                    row.development_usage = Some(execution.usage.clone());
                    row.development_overrun = overrun(&execution.usage, &plan.development_grant)?;
                    if let Some(scoring) = &evidence.scoring {
                        let request = plan
                            .evaluation_request_for_accounting(&candidate.slot_id, execution)
                            .map_err(|e| e.to_string())?
                            .ok_or("scoring without retained output")?;
                        scoring
                            .validate_identity_against(&request, execution, &binding)
                            .map_err(|e| e.to_string())?;
                        row.scoring = Some(scoring.outcome.clone());
                        row.scoring_usage = Some(scoring.usage.clone());
                        row.scoring_within_grant =
                            !overrun(&scoring.usage, &plan.scoring_grant)? && known(&scoring.usage);
                        row.successes = count(input, &scoring.outcome)?;
                    } else if input.metrics.is_some() {
                        return Err(fail("metrics without scoring receipt"));
                    }
                    // Match the frozen protocol: development usage must be
                    // known, scoring within grant. Development overrun is also
                    // reported; this selection never authorizes another effect.
                    row.eligible = execution.outcome == ExecutionOutcome::OutputGenerated
                        && known(&execution.usage)
                        && row.scoring_within_grant
                        && row.successes.is_some();
                } else if input.metrics.is_some() {
                    return Err(fail("metrics without execution"));
                }
            }
        }
        rows.push(row);
    }
    let selected_round = rows
        .iter()
        .filter(|r| r.eligible)
        .min_by_key(|r| {
            let ResourceUsage::Measured { value: steps } =
                r.development_usage.as_ref().unwrap().simulation_steps
            else {
                unreachable!("eligible usage is known")
            };
            (
                Reverse(r.successes.unwrap()),
                steps,
                r.round,
                &r.method.as_ref().unwrap().sha256,
            )
        })
        .map(|r| r.round);
    Ok(SelectionReport {
        schema: "genesis-process-development-selection/1",
        protocol: "process-comparison/1",
        scope: "single_arm_family_development",
        arm,
        family,
        input_sha256: canonical_sha256(&json!({"schema":"genesis-process-development-selection/1",
            "arm":arm,"family":family,"rounds":rounds})),
        rows,
        selected_round,
    })
}
