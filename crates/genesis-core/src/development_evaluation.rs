//! Experimental independent scoring records. No evaluator or comparison executes here.

use crate::development_method::{
    BudgetAssessment, ContractError, DevelopmentRunBinding, ExactContentRef, ExecutionOutcome,
    ExecutionPurpose, ExecutionReceipt, ResourceGrant, Usage,
};
use crate::digest::canonical_sha256;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvaluationError {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error("invalid evaluation field: {0}")]
    Invalid(&'static str),
    #[error("evaluation differs from the caller's frozen request or execution")]
    BindingMismatch,
    #[error("output was not retained by the bound execution")]
    OutputNotProduced,
    #[error("observed scoring context differs from the trusted request")]
    ContextMismatch,
    #[error("duplicate request identity or comparison slot")]
    Duplicate,
    #[error("receipt was not requested by the frozen roster")]
    Unexpected,
}

fn nonblank(value: &str) -> Result<(), EvaluationError> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(EvaluationError::Invalid("identifier or reason"));
    }
    Ok(())
}

fn hash<T: Serialize>(domain: &str, record: &T) -> String {
    canonical_sha256(&serde_json::json!({"domain":domain,"version":1,"record":record}))
}

/// Trusted scoring inputs. Instance content stays private to the evaluator;
/// serializing a reference does not implement commitment/reveal or confidentiality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoringContext {
    pub public_task: ExactContentRef,
    pub evaluator: ExactContentRef,
    pub environment: ExactContentRef,
    pub generator: ExactContentRef,
    pub instance_set: ExactContentRef,
    pub design_schema: ExactContentRef,
    pub metric_schema: ExactContentRef,
    pub scoring_rule: ExactContentRef,
}

impl ScoringContext {
    fn validate(&self) -> Result<(), EvaluationError> {
        for reference in [
            &self.public_task,
            &self.evaluator,
            &self.environment,
            &self.generator,
            &self.instance_set,
            &self.design_schema,
            &self.metric_schema,
            &self.scoring_rule,
        ] {
            reference.validate()?;
        }
        Ok(())
    }
}

/// Frozen after a particular output exists, before its scoring request is sent.
/// This is not the pre-execution development trial plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationRequest {
    pub request_id: String,
    pub execution_sha256: String,
    pub output: ExactContentRef,
    pub context: ScoringContext,
    pub purpose: ExecutionPurpose,
    pub budget: ResourceGrant,
}

/// Hash all execution evidence, including outputs, outcome, transcript and usage.
/// Validation/hashing does not authenticate who produced that evidence.
pub fn execution_receipt_hash(
    execution: &ExecutionReceipt,
    binding: &DevelopmentRunBinding,
) -> Result<String, EvaluationError> {
    execution.validate_against(binding)?;
    Ok(hash("genesis.development_execution_receipt", execution))
}

impl EvaluationRequest {
    pub fn validate_against(
        &self,
        execution: &ExecutionReceipt,
        binding: &DevelopmentRunBinding,
    ) -> Result<BudgetAssessment, EvaluationError> {
        nonblank(&self.request_id)?;
        self.context.validate()?;
        self.output.validate()?;
        let accounting = execution.validate_against(binding)?;
        if self.execution_sha256 != execution_receipt_hash(execution, binding)?
            || self.purpose != binding.purpose
            || self.context.public_task != binding.public_task
            || self.context.evaluator != binding.evaluator
        {
            return Err(EvaluationError::BindingMismatch);
        }
        if !execution.outputs.contains(&self.output) {
            return Err(EvaluationError::OutputNotProduced);
        }
        Ok(accounting)
    }

    pub fn canonical_hash(
        &self,
        execution: &ExecutionReceipt,
        binding: &DevelopmentRunBinding,
    ) -> Result<String, EvaluationError> {
        self.validate_against(execution, binding)?;
        Ok(hash("genesis.development_evaluation_request", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvaluationOutcome {
    Scored { metrics: ExactContentRef },
    Invalid { reason: String },
    Failed { reason: String },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationReceipt {
    pub request: EvaluationRequest,
    pub request_sha256: String,
    pub observed_context: Option<ScoringContext>,
    pub outcome: EvaluationOutcome,
    /// Evaluator trace or local attempt/reconciliation log, depending on outcome.
    pub trace: ExactContentRef,
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluationAccounting {
    pub development: BudgetAssessment,
    pub scoring: BudgetAssessment,
}

impl EvaluationReceipt {
    /// The expected request/execution come from trusted, separately frozen state.
    /// A self-consistent untrusted receipt is never a substitute for that state.
    pub fn validate_against(
        &self,
        expected: &EvaluationRequest,
        execution: &ExecutionReceipt,
        binding: &DevelopmentRunBinding,
    ) -> Result<EvaluationAccounting, EvaluationError> {
        let development = expected.validate_against(execution, binding)?;
        if self.request != *expected
            || self.request_sha256 != expected.canonical_hash(execution, binding)?
        {
            return Err(EvaluationError::BindingMismatch);
        }
        if let Some(context) = &self.observed_context {
            context.validate()?;
            if context != &expected.context {
                return Err(EvaluationError::ContextMismatch);
            }
        }
        if matches!(
            self.outcome,
            EvaluationOutcome::Scored { .. } | EvaluationOutcome::Invalid { .. }
        ) && self.observed_context.is_none()
        {
            return Err(EvaluationError::Invalid("missing observed scoring context"));
        }
        match &self.outcome {
            EvaluationOutcome::Scored { metrics } => metrics.validate()?,
            EvaluationOutcome::Invalid { reason }
            | EvaluationOutcome::Failed { reason }
            | EvaluationOutcome::Unknown { reason } => nonblank(reason)?,
        }
        self.trace.validate()?;
        Ok(EvaluationAccounting {
            development,
            scoring: self.usage.assess(&expected.budget)?,
        })
    }
}

/// Borrow independently frozen state, never the fields claimed by an incoming receipt.
pub struct EvaluationExpectation<'a> {
    pub request: &'a EvaluationRequest,
    pub execution: &'a ExecutionReceipt,
    pub binding: &'a DevelopmentRunBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Missing,
    Scored,
    Invalid,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestAssessment {
    pub request_id: String,
    pub status: RequestStatus,
    /// Scoring retained output does not turn a failed development run into success.
    pub development_outcome: ExecutionOutcome,
    pub development_accounting: BudgetAssessment,
    /// None means no receipt, not zero consumption or permission to resubmit.
    pub scoring_accounting: Option<BudgetAssessment>,
}

/// Assess only the frozen scoring-request roster, in its declared order.
/// No aggregate "comparison complete/passed" result is produced: unrequested
/// development failures and no-output runs require a separate pre-execution plan.
pub fn assess_request_roster(
    expected: &[EvaluationExpectation<'_>],
    receipts: &[EvaluationReceipt],
) -> Result<Vec<RequestAssessment>, EvaluationError> {
    if expected.is_empty() {
        return Err(EvaluationError::Invalid("empty request roster"));
    }
    let mut ids = BTreeSet::new();
    let mut comparison_slots = BTreeSet::new();
    for item in expected {
        item.request
            .validate_against(item.execution, item.binding)?;
        if !ids.insert(&item.request.request_id) {
            return Err(EvaluationError::Duplicate);
        }
        if let ExecutionPurpose::Comparison {
            plan,
            trial_id,
            arm_id,
        } = &item.request.purpose
        {
            let slot = hash("genesis.evaluation_request_slot", &(plan, trial_id, arm_id));
            if !comparison_slots.insert(slot) {
                return Err(EvaluationError::Duplicate);
            }
        }
    }
    let mut received = BTreeMap::new();
    for receipt in receipts {
        if !ids.contains(&receipt.request.request_id) {
            return Err(EvaluationError::Unexpected);
        }
        if received
            .insert(&receipt.request.request_id, receipt)
            .is_some()
        {
            return Err(EvaluationError::Duplicate);
        }
    }
    expected
        .iter()
        .map(|item| {
            let development = item
                .request
                .validate_against(item.execution, item.binding)?;
            let (status, scoring) = match received.get(&item.request.request_id) {
                None => (RequestStatus::Missing, None),
                Some(receipt) => {
                    let accounting =
                        receipt.validate_against(item.request, item.execution, item.binding)?;
                    let status = match receipt.outcome {
                        EvaluationOutcome::Scored { .. } => RequestStatus::Scored,
                        EvaluationOutcome::Invalid { .. } => RequestStatus::Invalid,
                        EvaluationOutcome::Failed { .. } => RequestStatus::Failed,
                        EvaluationOutcome::Unknown { .. } => RequestStatus::Unknown,
                    };
                    (status, Some(accounting.scoring))
                }
            };
            Ok(RequestAssessment {
                request_id: item.request.request_id.clone(),
                status,
                development_outcome: item.execution.outcome,
                development_accounting: development,
                scoring_accounting: scoring,
            })
        })
        .collect()
}
