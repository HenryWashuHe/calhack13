//! Assessment of a trusted declared roster, not admission or exhaustive metering.
use crate::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use crate::text_generation::{identifier, GenerationTransport};
use crate::SynthesisError;
use genesis_core::development_evaluation::{
    execution_receipt_identity_hash, EvaluationError, EvaluationOutcome,
};
use genesis_core::development_method::*;
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use genesis_core::digest::canonical_sha256;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, thiserror::Error)]
pub enum AccountingError {
    #[error("invalid accounting evidence: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Generation(#[from] SynthesisError),
    #[error(transparent)]
    Evaluation(#[from] EvaluationError),
    #[error(transparent)]
    Contract(#[from] ContractError),
}
type Result<T> = std::result::Result<T, AccountingError>;
fn invalid(reason: &'static str) -> AccountingError {
    AccountingError::Invalid(reason)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationExpectation {
    pub org_id: String,
    pub operation_id: String,
    pub plan_sha256: String,
    pub request_sha256: String,
    /// Exact retained checkpoint selected by the trusted host, not its latest alias.
    pub checkpoint_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialExpectation {
    pub plan: ExactContentRef,
    /// Every slot in the plan, in its original order.
    pub slots: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedMethodLink {
    pub org_id: String,
    pub operation_id: String,
    pub slot_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostScope {
    pub id: String,
    pub generations: Vec<GenerationExpectation>,
    pub trial: Option<TrialExpectation>,
    pub generated_links: Vec<GeneratedMethodLink>,
    pub total_grant: ResourceGrant,
}
impl CostScope {
    pub fn validate(&self) -> Result<()> {
        if !identifier(&self.id, 128)
            || self.generations.len() > 64
            || (self.generations.is_empty() && self.trial.is_none())
        {
            return Err(invalid("scope"));
        }
        let mut operations = BTreeSet::new();
        let mut requests = BTreeSet::new();
        let mut checkpoints = BTreeSet::new();
        for g in &self.generations {
            if !identifier(&g.org_id, 128)
                || !identifier(&g.operation_id, 128)
                || [&g.plan_sha256, &g.request_sha256, &g.checkpoint_sha256]
                    .into_iter()
                    .any(|s| !digest(s))
                || !operations.insert((&g.org_id, &g.operation_id))
                || !requests.insert(&g.request_sha256)
                || !checkpoints.insert(&g.checkpoint_sha256)
            {
                return Err(invalid("generation identity or duplicate"));
            }
        }
        if let Some(trial) = &self.trial {
            trial.plan.validate()?;
            let mut slots = BTreeSet::new();
            if trial.slots.is_empty()
                || trial.slots.len() > 64
                || trial
                    .slots
                    .iter()
                    .any(|s| s.trim().is_empty() || !slots.insert(s))
            {
                return Err(invalid("trial slots"));
            }
        }
        let mut linked = BTreeSet::new();
        for link in &self.generated_links {
            if !operations.contains(&(&link.org_id, &link.operation_id))
                || !self
                    .trial
                    .as_ref()
                    .is_some_and(|t| t.slots.contains(&link.slot_id))
                || !linked.insert(&link.slot_id)
            {
                return Err(invalid("generated link"));
            }
        }
        Ok(())
    }
    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        Ok(canonical_sha256(
            &serde_json::json!({"domain":"genesis.cost_scope/1","scope":self}),
        ))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CostStage {
    Generation,
    Execution,
    Scoring,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CostObservation {
    Missing,
    Generation {
        phase: MethodSynthesisPhase,
        transport: Option<GenerationTransport>,
    },
    Execution {
        outcome: ExecutionOutcome,
        has_output: bool,
    },
    Scoring {
        outcome: EvaluationOutcome,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct CostRow {
    pub stage: CostStage,
    pub identity: String,
    pub evidence_sha256: Option<String>,
    pub observation: CostObservation,
    pub usage: Usage,
    /// A known overrun only; false can coexist with unknown usage.
    /// None for missing evidence and generation, which has no ResourceGrant.
    pub individual_limit_exceeded: Option<bool>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResourceTotal {
    pub resource: &'static str,
    /// Saturates at u64::MAX on overflow, still a valid known lower bound.
    pub known_lower_bound: u64,
    pub unknown: bool,
    pub overflow: bool,
    pub limit_exceeded: bool,
}
#[derive(Debug, Serialize)]
pub struct CostReport {
    pub schema: &'static str,
    pub scope_sha256: String,
    pub rows: Vec<CostRow>,
    /// Exact package/dependency matches only, not execution or authenticated lineage.
    /// False for missing/failed generation or a missing trial.
    pub matched_package_links: Vec<bool>,
    pub generation: Vec<ResourceTotal>,
    pub execution: Vec<ResourceTotal>,
    pub scoring: Vec<ResourceTotal>,
    pub combined: Vec<ResourceTotal>,
}
fn fields(usage: &Usage) -> [&ResourceUsage; 5] {
    [
        &usage.model_calls,
        &usage.input_tokens,
        &usage.output_tokens,
        &usage.wall_time_ms,
        &usage.simulation_steps,
    ]
}
fn totals(rows: &[CostRow], stage: Option<CostStage>, grant: &ResourceGrant) -> Vec<ResourceTotal> {
    [
        ("model_calls", grant.model_calls),
        ("input_tokens", grant.input_tokens),
        ("output_tokens", grant.output_tokens),
        ("wall_time_ms", grant.wall_time_ms),
        ("simulation_steps", grant.simulation_steps),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (resource, limit))| {
        let mut total = ResourceTotal {
            resource,
            known_lower_bound: 0,
            unknown: false,
            overflow: false,
            limit_exceeded: false,
        };
        for row in rows.iter().filter(|r| stage.is_none_or(|s| r.stage == s)) {
            match fields(&row.usage)[index] {
                ResourceUsage::Unknown { .. } => total.unknown = true,
                ResourceUsage::Measured { value } => {
                    match total.known_lower_bound.checked_add(*value) {
                        Some(sum) => total.known_lower_bound = sum,
                        None => {
                            total.known_lower_bound = u64::MAX;
                            total.overflow = true;
                        }
                    }
                }
            }
        }
        total.limit_exceeded = total.overflow || total.known_lower_bound > limit;
        total
    })
    .collect()
}
fn missing(stage: CostStage, identity: String) -> CostRow {
    let unknown = ResourceUsage::Unknown {
        reason: "declared stage evidence missing".into(),
    };
    CostRow {
        stage,
        identity,
        evidence_sha256: None,
        observation: CostObservation::Missing,
        usage: Usage {
            model_calls: unknown.clone(),
            input_tokens: unknown.clone(),
            output_tokens: unknown.clone(),
            wall_time_ms: unknown.clone(),
            simulation_steps: unknown,
        },
        individual_limit_exceeded: None,
    }
}
fn exceeds(usage: &Usage, grant: &ResourceGrant) -> Result<bool> {
    match usage.assess(grant) {
        Ok(_) => Ok(false),
        Err(ContractError::BudgetExceeded(_)) => Ok(true),
        Err(error) => Err(error.into()),
    }
}
/// Assess exact host-selected snapshots. Missing evidence is unknown, including
/// scoring after a no-output execution. The caller owns roster completeness,
/// storage trust and proof of when budgets were declared. No I/O or verdict.
pub fn assess_costs(
    scope: &CostScope,
    generations: &[MethodSynthesisSession],
    trial: Option<(&TrialPlan, &[TrialEvidence])>,
) -> Result<CostReport> {
    let scope_sha256 = scope.sha256()?;
    let mut seen = BTreeSet::new();
    for session in generations {
        session.validate()?;
        let p = session.plan();
        let Some(expected) = scope
            .generations
            .iter()
            .find(|g| g.org_id == p.org_id && g.operation_id == p.operation_id)
        else {
            return Err(invalid("unexpected generation"));
        };
        if !seen.insert((&p.org_id, &p.operation_id))
            || expected.plan_sha256 != p.sha256()?
            || expected.request_sha256 != session.request_sha256()
            || expected.checkpoint_sha256 != session.checkpoint_sha256()
        {
            return Err(invalid("generation binding or duplicate"));
        }
    }
    let mut rows = vec![];
    for expected in &scope.generations {
        let mut row = missing(
            CostStage::Generation,
            format!("{}/{}", expected.org_id, expected.operation_id),
        );
        if let Some(s) = generations.iter().find(|s| {
            s.plan().org_id == expected.org_id && s.plan().operation_id == expected.operation_id
        }) {
            row.evidence_sha256 = Some(s.checkpoint_sha256().into());
            row.observation = CostObservation::Generation {
                phase: s.phase(),
                transport: s.completion().map(|c| c.transport),
            };
            if let Some(c) = s.completion() {
                c.usage.model_calls.clone_into(&mut row.usage.model_calls);
                c.usage.input_tokens.clone_into(&mut row.usage.input_tokens);
                c.usage
                    .output_tokens
                    .clone_into(&mut row.usage.output_tokens);
                c.usage.wall_time_ms.clone_into(&mut row.usage.wall_time_ms);
                row.usage.simulation_steps = ResourceUsage::Measured { value: 0 };
                row.usage.validate()?;
            }
        }
        rows.push(row);
    }
    if let Some((plan, evidence)) = trial {
        let expected = scope
            .trial
            .as_ref()
            .ok_or_else(|| invalid("unexpected trial"))?;
        if plan.reference()? != expected.plan
            || plan
                .slots
                .iter()
                .map(|s| &s.slot_id)
                .ne(expected.slots.iter())
        {
            return Err(invalid("trial binding"));
        }
        let mut seen = BTreeSet::new();
        for item in evidence {
            if !expected.slots.contains(&item.slot_id) || !seen.insert(&item.slot_id) {
                return Err(invalid("unexpected or duplicate trial slot"));
            }
        }
    }
    if let Some(expected) = &scope.trial {
        for slot in &expected.slots {
            let mut execution = missing(CostStage::Execution, slot.clone());
            let mut scoring = missing(CostStage::Scoring, slot.clone());
            if let Some((plan, item)) =
                trial.and_then(|(p, e)| e.iter().find(|e| e.slot_id == *slot).map(|e| (p, e)))
            {
                let binding = plan.expected_binding(slot)?;
                let e = &item.execution;
                execution.evidence_sha256 = Some(execution_receipt_identity_hash(e, &binding)?);
                execution.usage = e.usage.clone();
                execution.individual_limit_exceeded = Some(exceeds(&e.usage, &binding.budget)?);
                execution.observation = CostObservation::Execution {
                    outcome: e.outcome,
                    has_output: !e.outputs.is_empty(),
                };
                let request = plan.evaluation_request_for_accounting(slot, e)?;
                if let Some(receipt) = &item.scoring {
                    let request = request.ok_or(EvaluationError::OutputNotProduced)?;
                    receipt.validate_identity_against(&request, e, &binding)?;
                    scoring.evidence_sha256 = Some(canonical_sha256(
                        &serde_json::json!({"domain":"genesis.cost_scoring_receipt/1","receipt":receipt}),
                    ));
                    scoring.usage = receipt.usage.clone();
                    scoring.individual_limit_exceeded =
                        Some(exceeds(&receipt.usage, &request.budget)?);
                    scoring.observation = CostObservation::Scoring {
                        outcome: receipt.outcome.clone(),
                    };
                }
            }
            rows.extend([execution, scoring]);
        }
    }
    let mut matched_package_links = vec![];
    for link in &scope.generated_links {
        let session = generations
            .iter()
            .find(|s| s.plan().org_id == link.org_id && s.plan().operation_id == link.operation_id);
        let package = session.and_then(|s| s.package().ok());
        let verified = if let (Some(package), Some((plan, _))) = (package, trial) {
            let binding = plan.expected_binding(&link.slot_id)?;
            if package.reference()? != binding.method
                || package.dependencies() != binding.dependencies
            {
                return Err(invalid("generated method differs from slot"));
            }
            true
        } else {
            false
        };
        matched_package_links.push(verified);
    }
    Ok(CostReport {
        schema: "genesis-method-cost-report/1",
        scope_sha256,
        generation: totals(&rows, Some(CostStage::Generation), &scope.total_grant),
        execution: totals(&rows, Some(CostStage::Execution), &scope.total_grant),
        scoring: totals(&rows, Some(CostStage::Scoring), &scope.total_grant),
        combined: totals(&rows, None, &scope.total_grant),
        rows,
        matched_package_links,
    })
}
