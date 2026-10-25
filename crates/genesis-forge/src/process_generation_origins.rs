//! Join retained source-generation history to development selections, not authority.
use crate::{
    method_revision::MethodRevisionInput,
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisPlan, MethodSynthesisSession},
    process_cohort::{assess_cohort, CohortDeclaration},
    process_selection::{Arm, RoundInput},
    process_selection_record::RetainedSelection,
    text_generation::{ReasoningEffort, ReasoningPolicy},
};
use genesis_core::{
    development_method::AcceptanceRole, development_trial::TrialPurpose, digest::canonical_sha256,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationOrigin {
    pub arm: Arm,
    pub round: u8,
    /// Independently retained initial identity, not the later result hash.
    pub prepared_checkpoint_sha256: Option<String>,
    pub snapshot: Option<MethodSynthesisSession>,
    pub revision: Option<MethodRevisionInput>,
    /// Caller-recorded reason; its causal correctness is not established here.
    pub no_method_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationOrigins {
    pub schema: String,
    /// Common fresh-generation plan before substituting operation/target names.
    pub base: MethodSynthesisPlan,
    pub positions: [GenerationOrigin; 6],
}

fn matches_base(plan: &MethodSynthesisPlan, base: &MethodSynthesisPlan, revision: bool) -> bool {
    let mut normalized = plan.clone();
    normalized.operation_id.clone_from(&base.operation_id);
    normalized.target_id.clone_from(&base.target_id);
    normalized.target_version.clone_from(&base.target_version);
    if revision {
        // Additional feedback is retained but its origin/completeness is not checked.
        if !normalized.public_inputs.starts_with(&base.public_inputs) {
            return false;
        }
        normalized.public_inputs.clone_from(&base.public_inputs);
        normalized.instruction.clone_from(&base.instruction);
    }
    normalized == *base
}

/// Revalidate originals; reports, supplied package refs and phase labels are not
/// evidence. The host must bound inputs and independently pin this capture.
pub fn assess_generation_origins(
    origins: &GenerationOrigins,
    declaration: &CohortDeclaration,
    selections: &[RetainedSelection; 8],
) -> Result<Value, String> {
    let cohort = assess_cohort(declaration, selections)?;
    origins.base.validate().map_err(|e| e.to_string())?;
    if origins.schema != "genesis-process-generation-origins/1"
        || origins.base.settings.model != "claude-sonnet-5-5"
        || origins.base.settings.reasoning
            != Some(ReasoningPolicy::Adaptive {
                effort: ReasoningEffort::High,
            })
    {
        return Err("origins require process-comparison/1 model and reasoning policy".into());
    }
    let mut ordered = BTreeMap::new();
    let (mut prepared, mut operations, mut requests) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for origin in &origins.positions {
        if !(1..=3).contains(&origin.round)
            || ordered.insert((origin.arm, origin.round), origin).is_some()
        {
            return Err("expected each of the six generation positions once".into());
        }
        if let Some(pin) = &origin.prepared_checkpoint_sha256 {
            if pin.len() != 64
                || !pin
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || !prepared.insert(pin)
            {
                return Err("invalid or repeated Prepared checkpoint pin".into());
            }
        }
    }
    let mut rows = Vec::with_capacity(6);
    for ((arm, round), origin) in &ordered {
        let is_revision = *arm == Arm::Workflow && *round > 1;
        let mut parent = None;
        let package = if let Some(snapshot) = &origin.snapshot {
            snapshot.validate().map_err(|e| e.to_string())?;
            if origin.prepared_checkpoint_sha256.as_deref()
                != Some(
                    snapshot
                        .prepared_checkpoint_sha256()
                        .map_err(|e| e.to_string())?
                        .as_str(),
                )
            {
                return Err(
                    "generation differs from independently pinned Prepared checkpoint".into(),
                );
            }
            let plan = snapshot.plan();
            let canonical_prepared =
                MethodSynthesisSession::new(plan.clone()).map_err(|e| e.to_string())?;
            if canonical_prepared.request_sha256() != snapshot.request_sha256() {
                return Err(
                    "generation request differs from canonical source-generation contract".into(),
                );
            }
            if !operations.insert((&plan.org_id, &plan.operation_id))
                || !requests.insert(snapshot.request_sha256())
            {
                return Err("generation attempt reused across process positions".into());
            }
            if is_revision {
                let previous = ordered[&(*arm, round - 1)]
                    .snapshot
                    .as_ref()
                    .ok_or("workflow revision has no preceding generation")?;
                let previous_package = previous
                    .package()
                    .map_err(|_| "workflow revision has no generated parent")?;
                let expected_parent = previous_package.reference().map_err(|e| e.to_string())?;
                let revision = origin
                    .revision
                    .as_ref()
                    .ok_or("workflow revision input missing")?;
                if revision.spec.parent != expected_parent
                    || revision.prepare().map_err(|e| e.to_string())?.plan() != plan
                    || !matches_base(&revision.spec.generation, &origins.base, true)
                {
                    return Err(
                        "workflow revision differs from immediate parent or base policy".into(),
                    );
                }
                parent = Some(expected_parent);
            } else if origin.revision.is_some() || !matches_base(plan, &origins.base, false) {
                return Err(
                    "fresh generation differs from declared base inputs or settings".into(),
                );
            }
            if snapshot.phase() == MethodSynthesisPhase::Generated {
                Some(snapshot.package().map_err(|e| e.to_string())?)
            } else {
                None
            }
        } else {
            if origin.revision.is_some() {
                return Err("revision input without a generation snapshot".into());
            }
            None
        };
        let method = package
            .as_ref()
            .map(|p| p.reference())
            .transpose()
            .map_err(|e| e.to_string())?;
        if method.is_some() && origin.no_method_reason.is_some() {
            return Err("generated method cannot be marked absent".into());
        }
        if method.is_none()
            && !origin
                .no_method_reason
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty() && s.len() <= 1024)
        {
            return Err("missing method requires an explicit retained absence reason".into());
        }
        for selection in selections.iter().filter(|s| s.inputs.arm == *arm) {
            match (&selection.inputs.rounds[usize::from(round - 1)], &method) {
                (RoundInput::Trial { input }, Some(method)) => {
                    let candidate = input
                        .plan
                        .slots
                        .iter()
                        .find(|s| {
                            s.purpose
                                == TrialPurpose::Acceptance {
                                    role: AcceptanceRole::Candidate,
                                }
                        })
                        .ok_or("missing candidate slot")?;
                    if candidate.method != *method
                        || candidate.dependencies
                            != origin.snapshot.as_ref().unwrap().plan().dependencies
                    {
                        return Err("development candidate differs from original generated package or dependencies".into());
                    }
                }
                (RoundInput::NoMethod { reason }, None)
                    if Some(reason) == origin.no_method_reason.as_ref() => {}
                _ => return Err("generation outcome differs from cohort method presence".into()),
            }
        }
        let snapshot = origin.snapshot.as_ref();
        let completion = snapshot.and_then(|s| s.completion());
        rows.push(json!({"arm":arm,"round":round,
            "prepared_checkpoint_sha256":origin.prepared_checkpoint_sha256,
            "checkpoint_sha256":snapshot.map(|s|s.checkpoint_sha256()),
            "org_id":snapshot.map(|s|&s.plan().org_id),"operation_id":snapshot.map(|s|&s.plan().operation_id),
            "phase":snapshot.map(|s|s.phase()),"method":method,"parent":parent,
            "no_method_reason":origin.no_method_reason,"transport":completion.map(|c|c.transport),
            "actual_model":completion.and_then(|c|c.actual_model.as_ref()),"usage":completion.map(|c|&c.usage)}));
    }
    Ok(
        json!({"schema":"genesis-process-generation-origin-report/1",
        "scope":"generation_origin_and_base_input_binding",
        "origins_sha256":canonical_sha256(&json!(origins)),
        "cohort_declaration_sha256":cohort.declaration_sha256,"positions":rows,
        "unverified":["feedback_origin_and_completeness","absence_reason_causality","chain_admission_and_cost_binding","chronology_and_exhaustive_history","provider_default_sampling","runtime_and_adoption_authority"]}),
    )
}
