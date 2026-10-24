//! Experimental, pure identities and receipts for executable development methods.
//! These records confer no authority and do not certify design quality or benefit.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::{AttemptId, OrgId};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContractError {
    #[error("invalid contract field: {0}")]
    Invalid(&'static str),
    #[error("receipt differs from frozen execution binding")]
    BindingMismatch,
    #[error("actual method or environment differs from frozen identity")]
    IdentityMismatch,
    #[error("measured usage exceeds grant: {0}")]
    BudgetExceeded(&'static str),
}

fn nonblank(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(ContractError::Invalid(field));
    }
    Ok(())
}

fn digest(value: &str) -> Result<(), ContractError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(ContractError::Invalid("sha256"));
    }
    Ok(())
}

/// A digest addresses exact bytes; labels never request version resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactContentRef {
    pub domain: String,
    pub schema: String,
    pub id: String,
    pub version: String,
    pub sha256: String,
}

impl ExactContentRef {
    pub fn validate(&self) -> Result<(), ContractError> {
        for (value, name) in [
            (&self.domain, "domain"),
            (&self.schema, "schema"),
            (&self.id, "id"),
            (&self.version, "version"),
        ] {
            nonblank(value, name)?;
        }
        if matches!(
            self.version.to_ascii_lowercase().as_str(),
            "latest" | "current" | "head"
        ) || self.version.contains(['*', '^', '~', '<', '>', '|'])
        {
            return Err(ContractError::Invalid("exact version"));
        }
        digest(&self.sha256)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceRole {
    Candidate,
    Control,
}

/// Purpose cannot be changed after freezing, even when method and task match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionPurpose {
    Ordinary {},
    Acceptance {
        plan: ExactContentRef,
        role: AcceptanceRole,
    },
    Comparison {
        plan: ExactContentRef,
        trial_id: String,
        arm_id: String,
    },
}

impl ExecutionPurpose {
    fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::Ordinary {} => Ok(()),
            Self::Acceptance { plan, .. } => plan.validate(),
            Self::Comparison {
                plan,
                trial_id,
                arm_id,
            } => {
                plan.validate()?;
                nonblank(trial_id, "trial_id")?;
                nonblank(arm_id, "arm_id")
            }
        }
    }
}

/// Independent ceilings, not exchangeable units. Zero grants no use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceGrant {
    pub model_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub wall_time_ms: u64,
    pub simulation_steps: u64,
}

/// Freeze before execution. References contain public inputs, never holdout seeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentRunBinding {
    pub org_id: OrgId,
    pub run_id: String,
    pub attempt_id: AttemptId,
    pub method: ExactContentRef,
    pub dependencies: Vec<ExactContentRef>,
    pub public_task: ExactContentRef,
    pub evaluator: ExactContentRef,
    pub model_settings: ExactContentRef,
    pub environment: ExactContentRef,
    pub purpose: ExecutionPurpose,
    pub budget: ResourceGrant,
}

impl DevelopmentRunBinding {
    pub fn validate(&self) -> Result<(), ContractError> {
        for (value, name) in [
            (self.org_id.as_str(), "org_id"),
            (self.run_id.as_str(), "run_id"),
            (self.attempt_id.as_str(), "attempt_id"),
        ] {
            nonblank(value, name)?;
        }
        for reference in [
            &self.method,
            &self.public_task,
            &self.evaluator,
            &self.model_settings,
            &self.environment,
        ]
        .into_iter()
        .chain(&self.dependencies)
        {
            reference.validate()?;
        }
        let mut identities = std::collections::BTreeSet::new();
        for dependency in &self.dependencies {
            if !identities.insert((
                &dependency.domain,
                &dependency.schema,
                &dependency.id,
                &dependency.version,
            )) {
                return Err(ContractError::Invalid("duplicate dependency identity"));
            }
        }
        self.purpose.validate()
    }

    /// v1 domain separation; existing canonical JSON implementation is unchanged.
    /// Identity is not authentication, authorization, or proof of actual execution.
    pub fn canonical_hash(&self) -> Result<String, ContractError> {
        self.validate()?;
        Ok(crate::digest::canonical_sha256(&serde_json::json!({
            "domain": "genesis.development_run_binding", "version": 1, "binding": self,
        })))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceUsage {
    Measured { value: u64 },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub model_calls: ResourceUsage,
    pub input_tokens: ResourceUsage,
    pub output_tokens: ResourceUsage,
    pub wall_time_ms: ResourceUsage,
    pub simulation_steps: ResourceUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAssessment {
    WithinLimits,
    Unknown,
}

impl Usage {
    /// Validate accounting shape without interpreting any resource ceiling.
    pub fn validate(&self) -> Result<(), ContractError> {
        for usage in [
            &self.model_calls,
            &self.input_tokens,
            &self.output_tokens,
            &self.wall_time_ms,
            &self.simulation_steps,
        ] {
            if let ResourceUsage::Unknown { reason } = usage {
                nonblank(reason, "unknown usage reason")?;
            }
        }
        Ok(())
    }
    pub fn assess(&self, grant: &ResourceGrant) -> Result<BudgetAssessment, ContractError> {
        let mut assessment = BudgetAssessment::WithinLimits;
        for (name, usage, limit) in [
            ("model_calls", &self.model_calls, grant.model_calls),
            ("input_tokens", &self.input_tokens, grant.input_tokens),
            ("output_tokens", &self.output_tokens, grant.output_tokens),
            ("wall_time_ms", &self.wall_time_ms, grant.wall_time_ms),
            (
                "simulation_steps",
                &self.simulation_steps,
                grant.simulation_steps,
            ),
        ] {
            match usage {
                ResourceUsage::Measured { value } if *value > limit => {
                    return Err(ContractError::BudgetExceeded(name))
                }
                ResourceUsage::Measured { .. } => {}
                ResourceUsage::Unknown { reason } => {
                    nonblank(reason, "unknown usage reason")?;
                    assessment = BudgetAssessment::Unknown;
                }
            }
        }
        Ok(assessment)
    }
}

/// Output generation is separate from design success, evaluation and adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    OutputGenerated,
    Failed,
    Interrupted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub binding: DevelopmentRunBinding,
    pub binding_sha256: String,
    pub actual_method: ExactContentRef,
    pub actual_environment: ExactContentRef,
    pub outcome: ExecutionOutcome,
    pub outputs: Vec<ExactContentRef>,
    pub transcript: ExactContentRef,
    pub usage: Usage,
}

impl ExecutionReceipt {
    /// The caller supplies its trusted frozen binding, not one taken from this receipt.
    /// Even WithinLimits describes accounting only, never execution or design success.
    pub fn validate_against(
        &self,
        expected: &DevelopmentRunBinding,
    ) -> Result<BudgetAssessment, ContractError> {
        self.validate_identity_fields(expected)?;
        self.usage.assess(&expected.budget)
    }

    /// Accounting-only identity/shape check. An overrun remains valid evidence,
    /// not permission to execute, score, accept or adopt the result.
    pub fn validate_identity_against(
        &self,
        expected: &DevelopmentRunBinding,
    ) -> Result<(), ContractError> {
        self.validate_identity_fields(expected)?;
        self.usage.validate()
    }

    fn validate_identity_fields(
        &self,
        expected: &DevelopmentRunBinding,
    ) -> Result<(), ContractError> {
        let expected_hash = expected.canonical_hash()?;
        self.binding.validate()?;
        digest(&self.binding_sha256)?;
        if self.binding != *expected || self.binding_sha256 != expected_hash {
            return Err(ContractError::BindingMismatch);
        }
        self.actual_method.validate()?;
        self.actual_environment.validate()?;
        if self.actual_method != expected.method || self.actual_environment != expected.environment
        {
            return Err(ContractError::IdentityMismatch);
        }
        self.transcript.validate()?;
        for output in &self.outputs {
            output.validate()?;
        }
        if self.outcome == ExecutionOutcome::OutputGenerated && self.outputs.is_empty() {
            return Err(ContractError::Invalid("generated outputs"));
        }
        Ok(())
    }
}
