//! Finite structural ceilings, not runtime authority or proof of chronology.
use crate::method_accounting::{AccountingError, CostScope};
use crate::method_stage::{validate_limits, StageAllocation, StageLimits, StageTrialBinding};
use crate::text_generation::{encode, hash, identifier, TextModelSettings};
use genesis_core::development_method::{ExactContentRef, ResourceGrant};
use genesis_core::development_trial::TrialPlan;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChainWork {
    Generation {
        settings: TextModelSettings,
        max_attempts: u32,
        grant: ResourceGrant,
    },
    TrialOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainStage {
    pub org_id: String,
    pub limits: StageLimits,
    pub trial_model_settings: ExactContentRef,
    pub work: ChainWork,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    id: String,
    total_grant: ResourceGrant,
    stages: Vec<ChainStage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ChainAllocation(Record);

impl<'de> Deserialize<'de> for ChainAllocation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Self(Record::deserialize(d)?);
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

fn invalid(reason: &'static str) -> AccountingError {
    AccountingError::Invalid(reason)
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

impl ChainAllocation {
    pub fn stages(&self) -> &[ChainStage] {
        &self.0.stages
    }

    pub fn total_grant(&self) -> &ResourceGrant {
        &self.0.total_grant
    }

    /// Retain before any participating work. Pure construction proves no timing.
    pub fn new(
        id: String,
        total_grant: ResourceGrant,
        stages: Vec<ChainStage>,
    ) -> Result<Self, AccountingError> {
        let value = Self(Record {
            schema_version: 1,
            id,
            total_grant,
            stages,
        });
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), AccountingError> {
        if self.0.schema_version != 1
            || !identifier(&self.0.id, 128)
            || !(1..=64).contains(&self.0.stages.len())
        {
            return Err(invalid("chain identity or stage count"));
        }
        let mut ids = BTreeSet::new();
        let mut allocated = [0u64; 5];
        for stage in &self.0.stages {
            if !identifier(&stage.org_id, 128) || !ids.insert(&stage.limits.id) {
                return Err(invalid("chain organization or duplicate stage"));
            }
            stage.trial_model_settings.validate()?;
            match &stage.work {
                ChainWork::Generation {
                    settings,
                    max_attempts,
                    grant,
                } => {
                    settings.validate()?;
                    if !(1..=64).contains(max_attempts)
                        || u64::from(*max_attempts) > grant.model_calls
                        || u64::from(*max_attempts) * u64::from(settings.max_tokens)
                            > grant.output_tokens
                    {
                        return Err(invalid("chain generation caps exceed grant"));
                    }
                    validate_limits(&stage.limits, grant)?;
                }
                ChainWork::TrialOnly => validate_limits(
                    &stage.limits,
                    &ResourceGrant {
                        model_calls: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        wall_time_ms: 0,
                        simulation_steps: 0,
                    },
                )?,
            }
            for (sum, amount) in allocated.iter_mut().zip(amounts(&stage.limits.total_grant)) {
                *sum = sum
                    .checked_add(amount)
                    .ok_or_else(|| invalid("chain ceiling overflow"))?;
            }
        }
        if allocated
            .into_iter()
            .zip(amounts(&self.0.total_grant))
            .any(|(allocated, ceiling)| allocated > ceiling)
        {
            return Err(invalid("stage ceilings exceed chain grant"));
        }
        Ok(())
    }

    pub fn sha256(&self) -> Result<String, AccountingError> {
        self.validate()?;
        Ok(hash(&encode(&("genesis-chain-allocation/1", self))?))
    }

    fn stage(&self, id: &str) -> Result<&ChainStage, AccountingError> {
        self.validate()?;
        self.0
            .stages
            .iter()
            .find(|s| s.limits.id == id)
            .ok_or_else(|| invalid("undeclared chain stage"))
    }

    pub fn bind_generation(
        &self,
        id: &str,
        allocation: &StageAllocation,
    ) -> Result<ChainStageBinding, AccountingError> {
        let stage = self.stage(id)?;
        let ChainWork::Generation {
            settings,
            max_attempts,
            grant,
        } = &stage.work
        else {
            return Err(invalid("trial-only stage cannot allocate generation"));
        };
        let allocation_sha256 = allocation.sha256()?;
        let roster = allocation.roster();
        if allocation.limits() != &stage.limits
            || roster.settings() != settings
            || roster.total_grant() != grant
            || roster.attempts().len() > *max_attempts as usize
            || roster.attempts().iter().any(|a| a.org_id != stage.org_id)
        {
            return Err(invalid("generation allocation differs from chain stage"));
        }
        Ok(ChainStageBinding {
            chain_sha256: self.sha256()?,
            stage_id: id.into(),
            target: BoundStage::Generation { allocation_sha256 },
        })
    }

    fn check_trial(&self, id: &str, trial: &TrialPlan) -> Result<(), AccountingError> {
        let stage = self.stage(id)?;
        trial.reference()?;
        if trial.model_settings != stage.trial_model_settings
            || trial
                .slots
                .iter()
                .any(|slot| slot.org_id.as_str() != stage.org_id)
        {
            return Err(invalid(
                "trial settings or organization differ from chain stage",
            ));
        }
        Ok(())
    }

    pub fn bind_trial_only(
        &self,
        id: &str,
        trial: &TrialPlan,
        scope: &CostScope,
    ) -> Result<ChainStageBinding, AccountingError> {
        let stage = self.stage(id)?;
        self.check_trial(id, trial)?;
        let cost_scope_sha256 = scope.sha256()?;
        let reference = trial.reference()?;
        let expected = scope
            .trial
            .as_ref()
            .ok_or_else(|| invalid("trial-only stage needs trial scope"))?;
        if !matches!(stage.work, ChainWork::TrialOnly)
            || !scope.generations.is_empty()
            || !scope.generated_links.is_empty()
            || trial.slots.len() > stage.limits.max_trial_slots as usize
            || trial.development_grant != stage.limits.development_grant
            || trial.scoring_grant != stage.limits.scoring_grant
            || scope.total_grant != stage.limits.total_grant
            || expected.plan != reference
            || expected
                .slots
                .iter()
                .ne(trial.slots.iter().map(|slot| &slot.slot_id))
        {
            return Err(invalid("trial-only binding differs from chain stage"));
        }
        Ok(ChainStageBinding {
            chain_sha256: self.sha256()?,
            stage_id: id.into(),
            target: BoundStage::TrialOnly {
                trial: reference,
                cost_scope_sha256,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum BoundStage {
    Generation {
        allocation_sha256: String,
    },
    TrialOnly {
        trial: ExactContentRef,
        cost_scope_sha256: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainStageBinding {
    chain_sha256: String,
    stage_id: String,
    target: BoundStage,
}

impl ChainStageBinding {
    pub fn stage_id(&self) -> &str {
        &self.stage_id
    }

    pub fn validate_generation(
        &self,
        chain: &ChainAllocation,
        allocation: &StageAllocation,
    ) -> Result<(), AccountingError> {
        if *self != chain.bind_generation(&self.stage_id, allocation)? {
            return Err(invalid("chain generation binding substitution"));
        }
        Ok(())
    }

    /// Generation stages must also supply their independently retained trial binding.
    /// Neither this check nor pure binding makes an alternate pathname authoritative.
    pub fn validate_trial(
        &self,
        chain: &ChainAllocation,
        generated: Option<(&StageAllocation, &StageTrialBinding)>,
        trial: &TrialPlan,
        scope: &CostScope,
    ) -> Result<(), AccountingError> {
        chain.check_trial(&self.stage_id, trial)?;
        match (&self.target, generated) {
            (BoundStage::Generation { .. }, Some((allocation, binding))) => {
                self.validate_generation(chain, allocation)?;
                binding.validate_against(allocation, trial, scope)?;
            }
            (BoundStage::TrialOnly { .. }, None) => {
                if *self != chain.bind_trial_only(&self.stage_id, trial, scope)? {
                    return Err(invalid("chain trial binding substitution"));
                }
            }
            _ => return Err(invalid("wrong chain binding kind")),
        }
        Ok(())
    }
}
