//! Serial experiment accounting for a trusted trial executor, with no I/O.
//! The host must durably persist each transition before its associated effect.
use crate::development_evaluation::EvaluationError;
use crate::development_method::*;
use crate::development_trial::TrialPlan;
use crate::digest::canonical_json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Supplied by the trusted caller, never recovered from incoming evidence.
#[derive(Debug, Clone, Copy)]
pub struct ExperimentScope<'a> {
    pub plan: &'a TrialPlan,
    pub slot_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentCall {
    pub index: u64,
    pub binding_sha256: String,
    pub instance: ExactContentRef,
    pub design: ExactContentRef,
    pub step_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExperimentOutcome {
    Returned {
        observed_evaluator: ExactContentRef,
        result: Box<ExactContentRef>,
    },
    Failed {
        reason: String,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentSettlement {
    pub outcome: ExperimentOutcome,
    pub simulation_steps: ResourceUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentRecord {
    pub call: ExperimentCall,
    /// None is an outstanding effect, never a zero-cost failed experiment.
    pub settlement: Option<ExperimentSettlement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentLedger {
    slot_id: String,
    binding_sha256: String,
    records: Vec<ExperimentRecord>,
    sealed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperimentAccounting {
    pub simulation_steps: ResourceUsage,
    /// An observed overrun remains evidence; it is never discarded or refunded.
    pub limit_exceeded: bool,
    /// None blocks further calls, even when unresolved outcome cost is known.
    pub remaining_steps: Option<u64>,
}

fn invalid(field: &'static str) -> EvaluationError {
    EvaluationError::Invalid(field)
}
fn reason(value: &str) -> Result<(), EvaluationError> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(invalid("experiment reason"));
    }
    Ok(())
}

impl ExperimentLedger {
    pub fn new(scope: ExperimentScope<'_>) -> Result<Self, EvaluationError> {
        Ok(Self {
            slot_id: scope.slot_id.into(),
            binding_sha256: scope
                .plan
                .expected_binding(scope.slot_id)?
                .canonical_hash()?,
            records: vec![],
            sealed: false,
        })
    }

    pub fn records(&self) -> &[ExperimentRecord] {
        &self.records
    }

    /// Validate restored records against a separately trusted frozen plan.
    /// This does not authenticate that store or detect rollback to an older copy.
    pub fn assess(
        &self,
        scope: ExperimentScope<'_>,
    ) -> Result<ExperimentAccounting, EvaluationError> {
        let binding = scope.plan.expected_binding(scope.slot_id)?;
        if self.slot_id != scope.slot_id || self.binding_sha256 != binding.canonical_hash()? {
            return Err(EvaluationError::BindingMismatch);
        }
        let mut used = 0_u64;
        let mut blocked = false;
        let mut unknown_usage = None;
        let mut limit_exceeded = false;
        for (index, record) in self.records.iter().enumerate() {
            if blocked {
                return Err(invalid("experiment after unresolved call"));
            }
            let call = &record.call;
            if call.index != index as u64 || call.binding_sha256 != self.binding_sha256 {
                return Err(EvaluationError::BindingMismatch);
            }
            if !scope.plan.development_instances.contains(&call.instance) {
                return Err(invalid("experiment instance outside public plan"));
            }
            call.design.validate()?;
            if call.step_limit == 0 || call.step_limit > binding.budget.simulation_steps - used {
                return Err(ContractError::BudgetExceeded("simulation_steps").into());
            }
            let Some(settlement) = &record.settlement else {
                blocked = true;
                unknown_usage = Some("experiment settlement missing".into());
                continue;
            };
            match &settlement.outcome {
                ExperimentOutcome::Returned {
                    observed_evaluator,
                    result,
                } => {
                    if *observed_evaluator != binding.evaluator {
                        return Err(EvaluationError::ContextMismatch);
                    }
                    result.validate()?;
                }
                ExperimentOutcome::Failed { reason: message } => reason(message)?,
                ExperimentOutcome::Unknown { reason: message } => {
                    reason(message)?;
                    blocked = true;
                }
            }
            match &settlement.simulation_steps {
                ResourceUsage::Measured { value } => {
                    if *value > call.step_limit {
                        limit_exceeded = true;
                        blocked = true;
                    }
                    if let Some(total) = used.checked_add(*value) {
                        used = total;
                    } else {
                        limit_exceeded = true;
                        blocked = true;
                        unknown_usage = Some("experiment step total overflow".into());
                    }
                }
                ResourceUsage::Unknown { reason: message } => {
                    reason(message)?;
                    unknown_usage = Some(message.clone());
                    blocked = true;
                }
            }
        }
        Ok(ExperimentAccounting {
            limit_exceeded,
            simulation_steps: match unknown_usage {
                Some(reason) => ResourceUsage::Unknown { reason },
                None => ResourceUsage::Measured { value: used },
            },
            remaining_steps: if blocked || self.sealed {
                None
            } else {
                Some(binding.budget.simulation_steps - used)
            },
        })
    }

    /// Persist the resulting ledger before invoking the evaluator. No overlapping
    /// calls are admitted; a requested allowance cannot reset the cumulative grant.
    pub fn reserve(
        &mut self,
        scope: ExperimentScope<'_>,
        instance: ExactContentRef,
        design: ExactContentRef,
        step_limit: u64,
    ) -> Result<ExperimentCall, EvaluationError> {
        let remaining = self
            .assess(scope)?
            .remaining_steps
            .ok_or_else(|| invalid("experiment blocked"))?;
        if step_limit == 0 || step_limit > remaining {
            return Err(ContractError::BudgetExceeded("simulation_steps").into());
        }
        let call = ExperimentCall {
            index: self.records.len() as u64,
            binding_sha256: self.binding_sha256.clone(),
            instance,
            design,
            step_limit,
        };
        let mut next = self.clone();
        next.records.push(ExperimentRecord {
            call: call.clone(),
            settlement: None,
        });
        next.assess(scope)?;
        *self = next;
        Ok(call)
    }

    /// The trusted adapter supplies observed metering, never candidate testimony.
    /// Reconciliation may settle a persisted pending call; it must not resubmit it.
    pub fn settle(
        &mut self,
        scope: ExperimentScope<'_>,
        call: &ExperimentCall,
        settlement: ExperimentSettlement,
    ) -> Result<(), EvaluationError> {
        self.assess(scope)?;
        if self.sealed {
            return Err(invalid("sealed experiment ledger"));
        }
        let mut next = self.clone();
        let last = next.records.last_mut().ok_or(EvaluationError::Unexpected)?;
        if last.call != *call || last.settlement.is_some() {
            return Err(EvaluationError::BindingMismatch);
        }
        last.settlement = Some(settlement);
        next.assess(scope)?;
        *self = next;
        Ok(())
    }

    /// Freeze even incomplete/unknown records for an honest terminal transcript.
    /// This is a host state transition, not proof the candidate process stopped.
    pub fn seal(&mut self, scope: ExperimentScope<'_>) -> Result<(), EvaluationError> {
        self.assess(scope)?;
        self.sealed = true;
        Ok(())
    }

    /// Exact UTF-8 artifact bytes to retain under the transcript reference.
    pub fn transcript_json(&self, scope: ExperimentScope<'_>) -> Result<String, EvaluationError> {
        self.assess(scope)?;
        if !self.sealed {
            return Err(invalid("unsealed experiment transcript"));
        }
        Ok(canonical_json(&serde_json::json!({
            "domain":"genesis.development_experiment_transcript", "version":1, "ledger":self,
        })))
    }

    pub fn reference(
        &self,
        scope: ExperimentScope<'_>,
    ) -> Result<ExactContentRef, EvaluationError> {
        Ok(ExactContentRef {
            domain: "genesis".into(),
            schema: "development-experiment-transcript/1".into(),
            id: self.slot_id.clone(),
            version: "1".into(),
            sha256: format!(
                "{:x}",
                Sha256::digest(self.transcript_json(scope)?.as_bytes())
            ),
        })
    }

    /// For executions using this exact transcript and no unrecorded simulations.
    /// Model/token/wall accounting and actual effect isolation remain host duties.
    pub fn validate_execution(
        &self,
        scope: ExperimentScope<'_>,
        execution: &ExecutionReceipt,
    ) -> Result<BudgetAssessment, EvaluationError> {
        let accounting = self.assess(scope)?;
        if accounting.limit_exceeded {
            return Err(ContractError::BudgetExceeded("experiment steps").into());
        }
        let assessment =
            execution.validate_against(&scope.plan.expected_binding(scope.slot_id)?)?;
        if execution.transcript != self.reference(scope)?
            || execution.usage.simulation_steps != accounting.simulation_steps
        {
            return Err(EvaluationError::BindingMismatch);
        }
        Ok(assessment)
    }
}
