//! Frozen single-task trial batches. Structural evidence, not enforced execution.
use crate::development_evaluation::{
    execution_receipt_hash, EvaluationError, EvaluationReceipt, EvaluationRequest, RequestStatus,
    ScoringContext,
};
use crate::development_method::*;
use crate::digest::canonical_sha256;
use crate::ids::{AttemptId, OrgId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrialPurpose {
    Acceptance { role: AcceptanceRole },
    Comparison { trial_id: String, arm_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScoringInstances {
    Development {},
    /// context.instance_set identifies an immutable commitment artifact. Its
    /// protocol must bind generator, set size and domain, not just an opaque salt.
    HiddenCommitment {
        protocol: ExactContentRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialSlot {
    pub slot_id: String,
    pub scoring_request_id: String,
    pub org_id: OrgId,
    pub run_id: String,
    pub attempt_id: AttemptId,
    pub method: ExactContentRef,
    pub dependencies: Vec<ExactContentRef>,
    pub purpose: TrialPurpose,
}

/// v1 permits one attempt and at most one retained output per slot. No retries
/// or post-score attempt selection. Metered internal search remains permitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialPlan {
    pub id: String,
    pub version: String,
    pub criterion: ExactContentRef,
    pub development_instances: Vec<ExactContentRef>,
    pub scoring_instances: ScoringInstances,
    pub context: ScoringContext,
    pub model_settings: ExactContentRef,
    pub development_environment: ExactContentRef,
    pub development_grant: ResourceGrant,
    pub scoring_grant: ResourceGrant,
    /// Declared order is part of identity, even if evidence arrives out of order.
    pub slots: Vec<TrialSlot>,
}

fn invalid(field: &'static str) -> EvaluationError {
    EvaluationError::Invalid(field)
}
fn nonblank(value: &str) -> Result<(), EvaluationError> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(invalid("trial identifier"));
    }
    Ok(())
}

impl TrialPlan {
    fn unchecked_reference(&self) -> ExactContentRef {
        ExactContentRef {
            domain: "genesis".into(),
            schema: "development-trial-plan/1".into(),
            id: self.id.clone(),
            version: self.version.clone(),
            sha256: canonical_sha256(
                &serde_json::json!({"domain":"genesis.development_trial_plan","version":1,"plan":self}),
            ),
        }
    }

    fn binding(&self, slot: &TrialSlot) -> DevelopmentRunBinding {
        let plan = self.unchecked_reference();
        DevelopmentRunBinding {
            org_id: slot.org_id.clone(),
            run_id: slot.run_id.clone(),
            attempt_id: slot.attempt_id.clone(),
            method: slot.method.clone(),
            dependencies: slot.dependencies.clone(),
            public_task: self.context.public_task.clone(),
            evaluator: self.context.evaluator.clone(),
            model_settings: self.model_settings.clone(),
            environment: self.development_environment.clone(),
            purpose: match &slot.purpose {
                TrialPurpose::Acceptance { role } => {
                    ExecutionPurpose::Acceptance { plan, role: *role }
                }
                TrialPurpose::Comparison { trial_id, arm_id } => ExecutionPurpose::Comparison {
                    plan,
                    trial_id: trial_id.clone(),
                    arm_id: arm_id.clone(),
                },
            },
            budget: self.development_grant.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), EvaluationError> {
        self.unchecked_reference().validate()?;
        self.criterion.validate()?;
        self.context.validate()?;
        if self.slots.is_empty() || self.development_instances.is_empty() {
            return Err(invalid("empty trial plan"));
        }
        let mut instances = BTreeSet::new();
        for instance in &self.development_instances {
            instance.validate()?;
            if !instances.insert((
                &instance.domain,
                &instance.schema,
                &instance.id,
                &instance.version,
            )) {
                return Err(EvaluationError::Duplicate);
            }
        }
        let mut ids = BTreeSet::new();
        let mut requests = BTreeSet::new();
        let mut runs = BTreeSet::new();
        let mut attempts = BTreeSet::new();
        let mut acceptance = BTreeSet::new();
        let mut acceptance_orgs = BTreeSet::new();
        let mut treatments = BTreeMap::new();
        let mut trials: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for slot in &self.slots {
            nonblank(&slot.slot_id)?;
            nonblank(&slot.scoring_request_id)?;
            self.binding(slot).validate()?;
            if !ids.insert(&slot.slot_id)
                || !requests.insert(&slot.scoring_request_id)
                || !runs.insert((&slot.org_id, &slot.run_id))
                || !attempts.insert((&slot.org_id, &slot.attempt_id))
            {
                return Err(EvaluationError::Duplicate);
            }
            match &slot.purpose {
                TrialPurpose::Acceptance { role } => {
                    acceptance_orgs.insert(&slot.org_id);
                    if !acceptance.insert(match role {
                        AcceptanceRole::Candidate => 0,
                        AcceptanceRole::Control => 1,
                    }) {
                        return Err(EvaluationError::Duplicate);
                    }
                }
                TrialPurpose::Comparison { trial_id, arm_id } => {
                    let treatment = (&slot.org_id, &slot.method, &slot.dependencies);
                    if treatments
                        .insert(arm_id, treatment)
                        .is_some_and(|old| old != treatment)
                    {
                        return Err(invalid("changing comparison treatment"));
                    }
                    if !trials.entry(trial_id).or_default().insert(arm_id) {
                        return Err(EvaluationError::Duplicate);
                    }
                }
            }
        }
        match &self.scoring_instances {
            ScoringInstances::Development {}
                if trials.is_empty() && acceptance.len() == 2 && acceptance_orgs.len() == 1 => {}
            ScoringInstances::HiddenCommitment { protocol }
                if acceptance.is_empty() && !trials.is_empty() =>
            {
                protocol.validate()?;
                let first = trials.values().next().unwrap();
                if first.len() < 2 || trials.values().any(|arms| arms != first) {
                    return Err(invalid("unpaired comparison arms"));
                }
            }
            _ => return Err(invalid("mixed purpose or instance policy")),
        }
        Ok(())
    }

    pub fn reference(&self) -> Result<ExactContentRef, EvaluationError> {
        self.validate()?;
        Ok(self.unchecked_reference())
    }

    pub fn expected_binding(
        &self,
        slot_id: &str,
    ) -> Result<DevelopmentRunBinding, EvaluationError> {
        self.validate()?;
        Ok(self.binding(
            self.slots
                .iter()
                .find(|s| s.slot_id == slot_id)
                .ok_or(EvaluationError::Unexpected)?,
        ))
    }

    /// Construct only from a retained, validated execution; never accept an
    /// incoming scorer's request as the expected request. Reveal ordering and
    /// persisted pre-execution commitment require a trusted runtime ledger.
    pub fn evaluation_request(
        &self,
        slot_id: &str,
        execution: &ExecutionReceipt,
    ) -> Result<Option<EvaluationRequest>, EvaluationError> {
        let binding = self.expected_binding(slot_id)?;
        execution.validate_against(&binding)?;
        if execution.outputs.len() > 1 {
            return Err(invalid("multiple outputs unsupported"));
        }
        let Some(output) = execution.outputs.first() else {
            return Ok(None);
        };
        let slot = self.slots.iter().find(|s| s.slot_id == slot_id).unwrap();
        Ok(Some(EvaluationRequest {
            request_id: slot.scoring_request_id.clone(),
            execution_sha256: execution_receipt_hash(execution, &binding)?,
            output: output.clone(),
            context: self.context.clone(),
            purpose: binding.purpose,
            budget: self.scoring_grant.clone(),
        }))
    }
}

pub struct TrialEvidence {
    pub slot_id: String,
    pub execution: ExecutionReceipt,
    pub scoring: Option<EvaluationReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialAssessment {
    pub slot_id: String,
    /// None means missing execution evidence, not a successful or zero-cost run.
    pub development_outcome: Option<ExecutionOutcome>,
    pub output: Option<ExactContentRef>,
    pub development_accounting: Option<BudgetAssessment>,
    pub scoring_status: RequestStatus,
    pub scoring_accounting: Option<BudgetAssessment>,
}

/// Project every predeclared slot, including failures with no output. This
/// validates records, not execution completeness, authorization or benefit.
pub fn assess_trials(
    plan: &TrialPlan,
    evidence: &[TrialEvidence],
) -> Result<Vec<TrialAssessment>, EvaluationError> {
    plan.validate()?;
    let mut received = BTreeMap::new();
    for item in evidence {
        if !plan.slots.iter().any(|s| s.slot_id == item.slot_id) {
            return Err(EvaluationError::Unexpected);
        }
        if received.insert(&item.slot_id, item).is_some() {
            return Err(EvaluationError::Duplicate);
        }
    }
    plan.slots
        .iter()
        .map(|slot| {
            let mut row = TrialAssessment {
                slot_id: slot.slot_id.clone(),
                development_outcome: None,
                output: None,
                development_accounting: None,
                scoring_status: RequestStatus::Missing,
                scoring_accounting: None,
            };
            if let Some(item) = received.get(&slot.slot_id) {
                let binding = plan.binding(slot);
                row.development_accounting = Some(item.execution.validate_against(&binding)?);
                row.development_outcome = Some(item.execution.outcome);
                let request = plan.evaluation_request(&slot.slot_id, &item.execution)?;
                row.output = item.execution.outputs.first().cloned();
                if let Some(scoring) = &item.scoring {
                    let request = request.ok_or(EvaluationError::OutputNotProduced)?;
                    row.scoring_accounting = Some(
                        scoring
                            .validate_against(&request, &item.execution, &binding)?
                            .scoring,
                    );
                    row.scoring_status = match scoring.outcome {
                        crate::development_evaluation::EvaluationOutcome::Scored { .. } => {
                            RequestStatus::Scored
                        }
                        crate::development_evaluation::EvaluationOutcome::Invalid { .. } => {
                            RequestStatus::Invalid
                        }
                        crate::development_evaluation::EvaluationOutcome::Failed { .. } => {
                            RequestStatus::Failed
                        }
                        crate::development_evaluation::EvaluationOutcome::Unknown { .. } => {
                            RequestStatus::Unknown
                        }
                    };
                }
            }
            Ok(row)
        })
        .collect()
}
