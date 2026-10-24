//! Offline, create-only stage files. Use one retained canonical allocation path.
use crate::{emit, storage, Arguments};
use genesis_core::development_trial::TrialPlan;
use genesis_forge::{
    method_accounting::CostScope,
    method_admission::GenerationAdmission,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::MethodSynthesisSession,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preparation {
    schema: String,
    limits: StageLimits,
    prepared_checkpoints: Vec<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingInput {
    schema: String,
    plan: PathBuf,
    scope: PathBuf,
}
fn resolve(base: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("stage input paths must not be empty".into());
    }
    Ok(base.join(path))
}
pub fn binding_path(allocation: &Path) -> PathBuf {
    let mut name = allocation.as_os_str().to_os_string();
    name.push(".trial-binding.json");
    PathBuf::from(name)
}
pub fn run(args: &Arguments) -> Result<u8, String> {
    let manifest = args.path("--manifest")?;
    let base = manifest.parent().unwrap_or_else(|| Path::new("."));
    let path = args.path("--allocation")?;
    if args.command == "method-stage-prepare" {
        // Same outer journal lock as participating generation steps. No work can
        // reserve this journal between the unused check and allocation save.
        let journal_lock = storage::CheckpointLock::acquire(args.path("--admission")?)?;
        let admission: GenerationAdmission = journal_lock.load()?;
        let input: Preparation = storage::read_json(manifest)?;
        if input.schema != "genesis-stage-input/1"
            || !(1..=64).contains(&input.prepared_checkpoints.len())
        {
            return Err("invalid stage input schema or count".into());
        }
        let prepared = input
            .prepared_checkpoints
            .iter()
            .map(|p| storage::read_json::<MethodSynthesisSession>(&resolve(base, p)?))
            .collect::<Result<Vec<_>, String>>()?;
        let allocation = StageAllocation::prepare(input.limits, &admission, &prepared)
            .map_err(|e| e.to_string())?;
        let lock = storage::CheckpointLock::acquire(path)?;
        lock.create(&allocation)?;
        emit(
            &serde_json::json!({"schema":"genesis-stage-prepared/1", "allocation_sha256":allocation.sha256().map_err(|e|e.to_string())?}),
        )?;
    } else {
        let lock = storage::CheckpointLock::acquire(path)?;
        let allocation: StageAllocation = lock.load()?;
        let input: BindingInput = storage::read_json(manifest)?;
        if input.schema != "genesis-stage-trial-input/1" {
            return Err("unsupported stage trial schema".into());
        }
        let trial: TrialPlan = storage::read_json(&resolve(base, &input.plan)?)?;
        let scope: CostScope = storage::read_json(&resolve(base, &input.scope)?)?;
        let binding = allocation.bind(&trial, &scope).map_err(|e| e.to_string())?;
        let binding_lock = storage::CheckpointLock::acquire(&binding_path(path))?;
        binding_lock.create(&binding)?;
        emit(&binding)?;
    }
    Ok(0)
}
