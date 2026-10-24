//! Read-only joining of a trusted admission journal and host-declared trial
//! costs. This binds evidence; it does not prove when the scope was declared.
use crate::method_accounting::{assess_costs, AccountingError, CostReport, CostScope};
use crate::method_admission::{AdmissionSummary, GenerationAdmission};
use crate::method_roster::RosterCostReport;
use crate::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AdmittedCostReport {
    pub schema: &'static str,
    /// Scope with the original Prepared identities, not selected result hashes.
    pub declared_scope_sha256: String,
    pub admission: AdmissionSummary,
    /// Retains the roster's own generation grant, independently of combined costs.
    pub generation: RosterCostReport,
    /// The scope hash here binds the derived, exact observed checkpoint selection.
    pub costs: CostReport,
}

/// Retain every original attempt, including unreserved or missing observations.
/// Advanced snapshots require reservations; a reserved Prepared snapshot remains
/// uncertain and does not establish that a provider request actually occurred.
/// Trial/package identity checks and all usage semantics come from assess_costs.
pub fn assess_admitted_costs(
    admission: &GenerationAdmission,
    declared_scope: &CostScope,
    snapshots: &[MethodSynthesisSession],
    trial: Option<(&TrialPlan, &[TrialEvidence])>,
) -> Result<AdmittedCostReport, AccountingError> {
    let declared_scope_sha256 = declared_scope.sha256()?;
    let summary = admission.summary()?;
    let attempts = admission.roster().attempts();
    if declared_scope.generations.len() != attempts.len()
        || declared_scope
            .generations
            .iter()
            .zip(attempts)
            .any(|(selected, original)| {
                selected.org_id != original.org_id
                    || selected.operation_id != original.operation_id
                    || selected.plan_sha256 != original.plan_sha256
                    || selected.request_sha256 != original.request_sha256
                    || selected.checkpoint_sha256 != original.checkpoint_sha256
            })
    {
        return Err(AccountingError::Invalid(
            "cost scope must retain the complete original admission roster in order",
        ));
    }
    // Validates exact historical identities and duplicates before projection.
    let generation = admission.roster().assess(snapshots)?;
    let mut projected = declared_scope.clone();
    for snapshot in snapshots {
        let initial = snapshot.prepared_checkpoint_sha256()?;
        if snapshot.phase() != MethodSynthesisPhase::Prepared
            && !summary.reserved_prepared_checkpoints.contains(&initial)
        {
            return Err(AccountingError::Invalid(
                "advanced generation snapshot has no admission reservation",
            ));
        }
        let expected = projected
            .generations
            .iter_mut()
            .find(|entry| entry.checkpoint_sha256 == initial)
            .ok_or(AccountingError::Invalid("generation projection identity"))?;
        expected.checkpoint_sha256 = snapshot.checkpoint_sha256().into();
    }
    let costs = assess_costs(&projected, snapshots, trial)?;
    Ok(AdmittedCostReport {
        schema: "genesis-admitted-method-cost-report/1",
        declared_scope_sha256,
        admission: summary,
        generation,
        costs,
    })
}
