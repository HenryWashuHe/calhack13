//! Recompute declared-chain costs from original records, never nested totals twice.
use crate::method_accounting::{
    assess_costs, AccountingError, CostReport, CostScope, ResourceTotal,
};
use crate::method_admission::GenerationAdmission;
use crate::method_admitted_costs::{assess_admitted_costs, AdmittedCostReport};
use crate::method_chain::{ChainAllocation, ChainStageBinding};
use crate::method_stage::{StageAllocation, StageTrialBinding};
use crate::method_synthesis::MethodSynthesisSession;
use genesis_core::development_method::ResourceGrant;
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub enum ChainStageEvidence<'a> {
    Generation {
        binding: &'a ChainStageBinding,
        allocation: &'a StageAllocation,
        admission: &'a GenerationAdmission,
        scope: &'a CostScope,
        snapshots: &'a [MethodSynthesisSession],
        trial: Option<(&'a StageTrialBinding, &'a TrialPlan, &'a [TrialEvidence])>,
    },
    TrialOnly {
        binding: &'a ChainStageBinding,
        scope: &'a CostScope,
        trial: &'a TrialPlan,
        evidence: &'a [TrialEvidence],
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "report", rename_all = "snake_case")]
pub enum ChainObservedCosts {
    Generation(Box<AdmittedCostReport>),
    TrialOnly(Box<CostReport>),
}
impl ChainObservedCosts {
    fn costs(&self) -> &CostReport {
        match self {
            Self::Generation(report) => &report.costs,
            Self::TrialOnly(report) => report,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChainStageCosts {
    pub stage_id: String,
    pub reserved_grant: ResourceGrant,
    pub binding: Option<ChainStageBinding>,
    pub observed: Option<ChainObservedCosts>,
    /// No exact bound trial was supplied; unused capacity is not measured zero.
    pub unresolved_trial: bool,
}

#[derive(Debug, Serialize)]
pub struct ChainCostReport {
    pub schema: &'static str,
    pub chain_sha256: String,
    pub total_grant: ResourceGrant,
    /// Every declared stage in declaration order, including absent evidence.
    pub stages: Vec<ChainStageCosts>,
    /// Known lower bounds, not billing or elapsed study duration.
    pub combined: Vec<ResourceTotal>,
}

fn invalid(reason: &'static str) -> AccountingError {
    AccountingError::Invalid(reason)
}

/// Pure assessment of trusted records, not timing, completeness or authorization.
/// Reuse of a generated method belongs in a later TrialOnly stage. Identical
/// packages from distinct attempts are still charged; package hashes are not costs.
pub fn assess_chain_costs(
    chain: &ChainAllocation,
    evidence: &[ChainStageEvidence<'_>],
) -> Result<ChainCostReport, AccountingError> {
    let chain_sha256 = chain.sha256()?;
    let mut observed = BTreeMap::new();
    let (mut operations, mut requests, mut prepared) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let (mut trials, mut runs, mut attempts, mut receipts) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    for input in evidence {
        let (binding, scope, trial, report) = match input {
            ChainStageEvidence::Generation {
                binding,
                allocation,
                admission,
                scope,
                snapshots,
                trial,
            } => {
                binding.validate_generation(chain, allocation)?;
                allocation.validate_admission(admission)?;
                if scope.total_grant != allocation.limits().total_grant {
                    return Err(invalid("generation cost grant differs from chain stage"));
                }
                let (plan, report) = if let Some((trial_binding, plan, rows)) = trial {
                    binding.validate_trial(
                        chain,
                        Some((allocation, trial_binding)),
                        plan,
                        scope,
                    )?;
                    let assessed = trial_binding.assess(
                        allocation,
                        admission,
                        scope,
                        snapshots,
                        (plan, rows),
                    )?;
                    (Some(*plan), assessed.costs)
                } else {
                    if scope.trial.is_some() || !scope.generated_links.is_empty() {
                        return Err(invalid("generation-only costs contain trial expectations"));
                    }
                    (
                        None,
                        assess_admitted_costs(admission, scope, snapshots, None)?,
                    )
                };
                (
                    *binding,
                    *scope,
                    plan,
                    ChainObservedCosts::Generation(Box::new(report)),
                )
            }
            ChainStageEvidence::TrialOnly {
                binding,
                scope,
                trial,
                evidence,
            } => {
                binding.validate_trial(chain, None, trial, scope)?;
                let report = assess_costs(scope, &[], Some((trial, evidence)))?;
                (
                    *binding,
                    *scope,
                    Some(*trial),
                    ChainObservedCosts::TrialOnly(Box::new(report)),
                )
            }
        };
        for attempt in &scope.generations {
            if !operations.insert((attempt.org_id.clone(), attempt.operation_id.clone()))
                || !requests.insert(attempt.request_sha256.clone())
                || !prepared.insert(attempt.checkpoint_sha256.clone())
            {
                return Err(invalid(
                    "generation attempt counted in multiple chain stages",
                ));
            }
        }
        if let Some(trial) = trial {
            let reference = trial.reference()?;
            if !trials.insert((
                reference.domain,
                reference.schema,
                reference.id,
                reference.version,
            )) {
                return Err(invalid("duplicate or conflicting logical trial identity"));
            }
            for slot in &trial.slots {
                if !runs.insert((slot.org_id.clone(), slot.run_id.clone()))
                    || !attempts.insert((slot.org_id.clone(), slot.attempt_id.clone()))
                {
                    return Err(invalid(
                        "execution identity counted in multiple chain stages",
                    ));
                }
            }
        }
        for row in &report.costs().rows {
            if let Some(hash) = &row.evidence_sha256 {
                if !receipts.insert(hash.clone()) {
                    return Err(invalid("cost evidence counted in multiple chain stages"));
                }
            }
        }
        if observed
            .insert(binding.stage_id(), (binding, trial.is_none(), report))
            .is_some()
        {
            return Err(invalid("duplicate chain stage evidence"));
        }
    }
    let grant = chain.total_grant();
    let limits = [
        grant.model_calls,
        grant.input_tokens,
        grant.output_tokens,
        grant.wall_time_ms,
        grant.simulation_steps,
    ];
    let mut combined: Vec<_> = [
        "model_calls",
        "input_tokens",
        "output_tokens",
        "wall_time_ms",
        "simulation_steps",
    ]
    .into_iter()
    .map(|resource| ResourceTotal {
        resource,
        known_lower_bound: 0,
        unknown: false,
        overflow: false,
        limit_exceeded: false,
    })
    .collect();
    let mut stages = Vec::new();
    for stage in chain.stages() {
        let entry = observed.remove(stage.limits.id.as_str());
        let (binding, unresolved_trial, observed) = match entry {
            Some((binding, missing, report)) => (Some(binding.clone()), missing, Some(report)),
            None => (None, true, None),
        };
        for (index, sum) in combined.iter_mut().enumerate() {
            // Any unbound future work remains unknown, even for a declared zero grant.
            sum.unknown |= unresolved_trial;
            if let Some(report) = &observed {
                let part = &report.costs().combined[index];
                sum.unknown |= part.unknown;
                sum.overflow |= part.overflow;
                match sum.known_lower_bound.checked_add(part.known_lower_bound) {
                    Some(value) => sum.known_lower_bound = value,
                    None => {
                        sum.known_lower_bound = u64::MAX;
                        sum.overflow = true;
                    }
                }
            }
            sum.limit_exceeded = sum.overflow || sum.known_lower_bound > limits[index];
        }
        stages.push(ChainStageCosts {
            stage_id: stage.limits.id.clone(),
            reserved_grant: stage.limits.total_grant.clone(),
            binding,
            observed,
            unresolved_trial,
        });
    }
    // Each observed binding was validated against this chain; keep this explicit.
    if !observed.is_empty() {
        return Err(invalid("undeclared chain stage evidence"));
    }
    Ok(ChainCostReport {
        schema: "genesis-chain-cost-report/1",
        chain_sha256,
        total_grant: grant.clone(),
        stages,
        combined,
    })
}
