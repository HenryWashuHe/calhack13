//! Structural stage allocation and later exact trial binding. Trusted host
//! persistence/entry checks establish ordering; hashes alone do not prove it.
use crate::method_accounting::{AccountingError, CostScope};
use crate::method_admission::GenerationAdmission;
use crate::method_admitted_costs::{assess_admitted_costs, AdmittedCostReport};
use crate::method_roster::GenerationRoster;
use crate::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use crate::text_generation::{encode, hash, identifier};
use genesis_core::development_method::{ExactContentRef, ResourceGrant};
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageLimits {
    pub id: String,
    pub max_trial_slots: u32,
    pub development_grant: ResourceGrant,
    pub scoring_grant: ResourceGrant,
    pub total_grant: ResourceGrant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllocationRecord {
    schema_version: u32,
    limits: StageLimits,
    roster: GenerationRoster,
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct StageAllocation(AllocationRecord);

impl<'de> Deserialize<'de> for StageAllocation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let allocation = Self(AllocationRecord::deserialize(deserializer)?);
        allocation.validate().map_err(serde::de::Error::custom)?;
        Ok(allocation)
    }
}

fn invalid(message: &'static str) -> AccountingError {
    AccountingError::Invalid(message)
}
fn amounts(g: &ResourceGrant) -> [u64; 5] {
    [
        g.model_calls,
        g.input_tokens,
        g.output_tokens,
        g.wall_time_ms,
        g.simulation_steps,
    ]
}

impl StageAllocation {
    /// The host must lock the admission journal while checking these snapshots
    /// and persist this allocation before any participating generation intent.
    pub fn prepare(
        limits: StageLimits,
        admission: &GenerationAdmission,
        prepared: &[MethodSynthesisSession],
    ) -> Result<Self, AccountingError> {
        if admission.summary()?.reserved_attempts != 0
            || prepared.len() != admission.roster().attempts().len()
            || prepared
                .iter()
                .any(|s| s.phase() != MethodSynthesisPhase::Prepared)
        {
            return Err(invalid(
                "stage preparation requires the complete unused roster",
            ));
        }
        admission.roster().assess(prepared)?;
        let allocation = Self(AllocationRecord {
            schema_version: 1,
            limits,
            roster: admission.roster().clone(),
        });
        allocation.validate()?;
        Ok(allocation)
    }

    pub fn validate(&self) -> Result<(), AccountingError> {
        self.0.roster.validate()?;
        let limits = &self.0.limits;
        if self.0.schema_version != 1
            || !identifier(&limits.id, 128)
            || !(1..=64).contains(&limits.max_trial_slots)
        {
            return Err(invalid("stage allocation identity or slot count"));
        }
        for (((generation, development), scoring), ceiling) in amounts(self.0.roster.total_grant())
            .into_iter()
            .zip(amounts(&limits.development_grant))
            .zip(amounts(&limits.scoring_grant))
            .zip(amounts(&limits.total_grant))
        {
            let allocated = development
                .checked_add(scoring)
                .and_then(|v| v.checked_mul(u64::from(limits.max_trial_slots)))
                .and_then(|v| v.checked_add(generation))
                .ok_or_else(|| invalid("stage allocation arithmetic overflow"))?;
            if allocated > ceiling {
                return Err(invalid("declared grants exceed stage ceiling"));
            }
        }
        Ok(())
    }

    pub fn sha256(&self) -> Result<String, AccountingError> {
        self.validate()?;
        Ok(hash(&encode(&("genesis-stage-allocation/1", self))?))
    }

    pub fn validate_admission(
        &self,
        admission: &GenerationAdmission,
    ) -> Result<(), AccountingError> {
        self.validate()?;
        admission.validate()?;
        if self.0.roster.sha256()? != admission.roster().sha256()? {
            return Err(invalid("admission differs from stage allocation"));
        }
        Ok(())
    }

    /// Pure construction can be called repeatedly. The participating host must
    /// keep ONE create-only binding per allocation, before any runtime intent.
    pub fn bind(
        &self,
        trial: &TrialPlan,
        scope: &CostScope,
    ) -> Result<StageTrialBinding, AccountingError> {
        let allocation_sha256 = self.sha256()?;
        let cost_scope_sha256 = scope.sha256()?;
        let reference = trial.reference()?;
        let limits = &self.0.limits;
        let expected = scope
            .trial
            .as_ref()
            .ok_or_else(|| invalid("stage requires a trial scope"))?;
        if trial.slots.len() > limits.max_trial_slots as usize
            || trial.development_grant != limits.development_grant
            || trial.scoring_grant != limits.scoring_grant
            || scope.total_grant != limits.total_grant
            || scope.generations != self.0.roster.attempts()
            || expected.plan != reference
            || expected
                .slots
                .iter()
                .ne(trial.slots.iter().map(|s| &s.slot_id))
        {
            return Err(invalid("trial or cost scope differs from stage allocation"));
        }
        Ok(StageTrialBinding {
            schema_version: 1,
            allocation_sha256,
            trial: reference,
            cost_scope_sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageTrialBinding {
    schema_version: u32,
    allocation_sha256: String,
    trial: ExactContentRef,
    cost_scope_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct StageCostReport {
    pub schema: &'static str,
    pub allocation_sha256: String,
    pub binding_sha256: String,
    pub costs: AdmittedCostReport,
}

impl StageTrialBinding {
    pub fn validate_against(
        &self,
        allocation: &StageAllocation,
        trial: &TrialPlan,
        scope: &CostScope,
    ) -> Result<(), AccountingError> {
        if *self != allocation.bind(trial, scope)? {
            return Err(invalid("stage trial binding substitution"));
        }
        Ok(())
    }

    /// Read-only accounting, not runtime authorization or a budget verdict.
    pub fn assess(
        &self,
        allocation: &StageAllocation,
        admission: &GenerationAdmission,
        scope: &CostScope,
        snapshots: &[MethodSynthesisSession],
        trial: (&TrialPlan, &[TrialEvidence]),
    ) -> Result<StageCostReport, AccountingError> {
        self.validate_against(allocation, trial.0, scope)?;
        allocation.validate_admission(admission)?;
        Ok(StageCostReport {
            schema: "genesis-stage-cost-report/1",
            allocation_sha256: self.allocation_sha256.clone(),
            binding_sha256: hash(&encode(&("genesis-stage-trial-binding/1", self))?),
            costs: assess_admitted_costs(admission, scope, snapshots, Some(trial))?,
        })
    }
}
