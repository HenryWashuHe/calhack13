//! Post-run catalogue membership over caller-retained references, with no I/O.
//! Neither reference equality nor a complete record proves actual source access,
//! artifact normalization, chronology, unrecorded effects, or execution policy.
use crate::development_evaluation::EvaluationError;
use crate::development_experiment::{
    ExperimentAccounting, ExperimentLedger, ExperimentOutcome, ExperimentRecord, ExperimentScope,
};
use crate::development_method::*;
use crate::digest::canonical_sha256;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_CATALOGUE_DESIGNS: usize = 256;

/// Purpose-specific references to the same declared bytes; IDs may differ.
/// This declaration does not independently verify either artifact's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogueEntry {
    pub experiment: ExactContentRef,
    pub output: ExactContentRef,
}

/// The caller must retain and pin this record before treating it as a declaration.
/// Deserialization alone does not validate its binding or authenticate its origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignCatalogue {
    schema_version: u8,
    plan: ExactContentRef,
    slot_id: String,
    binding_sha256: String,
    entries: Vec<CatalogueEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueLedgerState {
    Missing,
    Unsealed,
    Sealed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueBudget {
    WithinLimits,
    Unknown,
    Exceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueExperiment {
    /// Includes the original index, exact design and failed/unknown/pending state.
    pub record: ExperimentRecord,
    /// None means this exact reference is outside the declared catalogue.
    pub catalogue_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueOutput {
    pub design: ExactContentRef,
    pub catalogue_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueAudit {
    pub catalogue_sha256: String,
    pub ledger_state: CatalogueLedgerState,
    pub experiments: Vec<CatalogueExperiment>,
    pub ledger_accounting: Option<ExperimentAccounting>,
    pub execution_outcome: Option<ExecutionOutcome>,
    pub execution_usage: Option<Usage>,
    pub execution_budget: Option<CatalogueBudget>,
    pub output: Option<CatalogueOutput>,
    /// All supplied records are linked, sealed and known within their grants.
    /// This is NOT catalogue membership, successful execution, or completeness
    /// of the actual history. Confirmed failures can have complete records.
    pub recorded_evidence_complete: bool,
}

impl DesignCatalogue {
    pub fn new(
        scope: ExperimentScope<'_>,
        entries: Vec<CatalogueEntry>,
    ) -> Result<Self, EvaluationError> {
        let catalogue = Self {
            schema_version: 1,
            plan: scope.plan.reference()?,
            slot_id: scope.slot_id.into(),
            binding_sha256: scope
                .plan
                .expected_binding(scope.slot_id)?
                .canonical_hash()?,
            entries,
        };
        catalogue.validate_against(scope)?;
        Ok(catalogue)
    }

    pub fn entries(&self) -> &[CatalogueEntry] {
        &self.entries
    }

    pub fn validate_against(&self, scope: ExperimentScope<'_>) -> Result<(), EvaluationError> {
        if self.schema_version != 1
            || self.entries.is_empty()
            || self.entries.len() > MAX_CATALOGUE_DESIGNS
        {
            return Err(EvaluationError::Invalid("design catalogue schema or size"));
        }
        if self.plan != scope.plan.reference()?
            || self.slot_id != scope.slot_id
            || self.binding_sha256
                != scope
                    .plan
                    .expected_binding(scope.slot_id)?
                    .canonical_hash()?
        {
            return Err(EvaluationError::BindingMismatch);
        }
        let (mut experiments, mut outputs) = (BTreeSet::new(), BTreeSet::new());
        for entry in &self.entries {
            let (a, b) = (&entry.experiment, &entry.output);
            a.validate()?;
            b.validate()?;
            if (&a.domain, &a.schema, &a.version, &a.sha256)
                != (&b.domain, &b.schema, &b.version, &b.sha256)
            {
                return Err(EvaluationError::Invalid("catalogue purpose aliases"));
            }
            for (reference, identities) in [(a, &mut experiments), (b, &mut outputs)] {
                if !identities.insert((
                    &reference.domain,
                    &reference.schema,
                    &reference.id,
                    &reference.version,
                    &reference.sha256,
                )) {
                    return Err(EvaluationError::Duplicate);
                }
            }
        }
        Ok(())
    }

    /// Order is part of identity; no sorting, normalization or byte resolution.
    pub fn canonical_hash(&self, scope: ExperimentScope<'_>) -> Result<String, EvaluationError> {
        self.validate_against(scope)?;
        Ok(canonical_sha256(&serde_json::json!({
            "domain":"genesis.development_design_catalogue", "version":1, "catalogue":self,
        })))
    }

    pub fn audit(
        &self,
        scope: ExperimentScope<'_>,
        ledger: Option<&ExperimentLedger>,
        execution: Option<&ExecutionReceipt>,
    ) -> Result<CatalogueAudit, EvaluationError> {
        let mut audit = CatalogueAudit {
            catalogue_sha256: self.canonical_hash(scope)?,
            ledger_state: CatalogueLedgerState::Missing,
            experiments: vec![],
            ledger_accounting: None,
            execution_outcome: None,
            execution_usage: None,
            execution_budget: None,
            output: None,
            recorded_evidence_complete: false,
        };
        let transcript = if let Some(ledger) = ledger {
            audit.ledger_accounting = Some(ledger.assess(scope)?);
            audit.experiments = ledger
                .records()
                .iter()
                .map(|record| CatalogueExperiment {
                    catalogue_index: self
                        .entries
                        .iter()
                        .position(|e| e.experiment == record.call.design),
                    record: record.clone(),
                })
                .collect();
            match ledger.reference(scope) {
                Ok(reference) => {
                    audit.ledger_state = CatalogueLedgerState::Sealed;
                    Some(reference)
                }
                Err(EvaluationError::Invalid("unsealed experiment transcript")) => {
                    audit.ledger_state = CatalogueLedgerState::Unsealed;
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        if let Some(execution) = execution {
            let binding = scope.plan.expected_binding(scope.slot_id)?;
            execution.validate_identity_against(&binding)?;
            // Reuse the trial's single-output/identity rules without authorizing
            // scoring or dropping a correctly bound receipt because it overran.
            scope
                .plan
                .evaluation_request_for_accounting(scope.slot_id, execution)?;
            audit.execution_budget = Some(match execution.usage.assess(&binding.budget) {
                Ok(BudgetAssessment::WithinLimits) => CatalogueBudget::WithinLimits,
                Ok(BudgetAssessment::Unknown) => CatalogueBudget::Unknown,
                Err(ContractError::BudgetExceeded(_)) => CatalogueBudget::Exceeded,
                Err(error) => return Err(error.into()),
            });
            audit.execution_outcome = Some(execution.outcome);
            audit.execution_usage = Some(execution.usage.clone());
            audit.output = execution.outputs.first().map(|design| CatalogueOutput {
                catalogue_index: self.entries.iter().position(|e| &e.output == design),
                design: design.clone(),
            });
            if let (Some(ledger), Some(transcript)) = (ledger, transcript) {
                let accounting = audit.ledger_accounting.as_ref().unwrap();
                // validate_execution returns early on overrun, so require these
                // links BEFORE retaining that error as an accounting finding.
                if execution.transcript != transcript
                    || execution.usage.simulation_steps != accounting.simulation_steps
                {
                    return Err(EvaluationError::BindingMismatch);
                }
                match ledger.validate_execution(scope, execution) {
                    Ok(_) | Err(EvaluationError::Contract(ContractError::BudgetExceeded(_))) => {}
                    Err(error) => return Err(error),
                }
                audit.recorded_evidence_complete = !accounting.limit_exceeded
                    && audit.execution_budget == Some(CatalogueBudget::WithinLimits)
                    && execution.outcome != ExecutionOutcome::Unknown
                    && ledger.records().iter().all(|r| {
                        r.settlement.as_ref().is_some_and(|s| {
                            !matches!(s.outcome, ExperimentOutcome::Unknown { .. })
                                && matches!(s.simulation_steps, ResourceUsage::Measured { .. })
                        })
                    });
            }
        }
        Ok(audit)
    }
}
