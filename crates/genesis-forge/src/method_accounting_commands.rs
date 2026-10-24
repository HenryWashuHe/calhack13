//! Offline assessment of explicit host-selected snapshots; never saves or recovers them.
use crate::{emit, storage, Arguments};
use genesis_core::development_evaluation::EvaluationReceipt;
use genesis_core::development_method::ExecutionReceipt;
use genesis_core::development_trial::{TrialEvidence, TrialPlan};
use genesis_forge::{
    method_accounting::{assess_costs, CostScope},
    method_synthesis::MethodSynthesisSession,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    scope: PathBuf,
    generation_checkpoints: Vec<PathBuf>,
    trial: Option<TrialFiles>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrialFiles {
    plan: PathBuf,
    evidence: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceInput {
    slot_id: String,
    execution: ExecutionReceipt,
    scoring: Option<EvaluationReceipt>,
}
fn resolve(base: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("evidence paths must not be empty".into());
    }
    Ok(base.join(path))
}
pub fn run(args: &Arguments) -> Result<u8, String> {
    let path = args.path("--manifest")?;
    let manifest: Manifest = storage::read_json(path)?;
    if manifest.schema != "genesis-method-cost-input/1"
        || manifest.generation_checkpoints.len() > 64
    {
        return Err("invalid cost input schema or generation count".into());
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let scope: CostScope = storage::read_json(&resolve(base, &manifest.scope)?)?;
    scope.validate().map_err(|e| e.to_string())?;
    let generations = manifest
        .generation_checkpoints
        .iter()
        .map(|p| storage::read_json::<MethodSynthesisSession>(&resolve(base, p)?))
        .collect::<Result<Vec<_>, _>>()?;
    let trial: Option<(TrialPlan, Vec<TrialEvidence>)> = manifest
        .trial
        .map(|t| {
            let evidence: Vec<EvidenceInput> = storage::read_json(&resolve(base, &t.evidence)?)?;
            Ok::<_, String>((
                storage::read_json(&resolve(base, &t.plan)?)?,
                evidence
                    .into_iter()
                    .map(|e| TrialEvidence {
                        slot_id: e.slot_id,
                        execution: e.execution,
                        scoring: e.scoring,
                    })
                    .collect(),
            ))
        })
        .transpose()?;
    let report = assess_costs(
        &scope,
        &generations,
        trial.as_ref().map(|(p, e)| (p, e.as_slice())),
    )
    .map_err(|e| e.to_string())?;
    emit(&report)?;
    // Successful assessment can include failed outcomes, unknown usage and overruns.
    // Exit zero is not authorization, budget compliance or a positive verdict.
    Ok(0)
}
