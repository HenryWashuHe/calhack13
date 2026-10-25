//! Read-only assembly from retained bindings and existing nested cost manifests.
use crate::{
    emit,
    method_accounting_commands::{load, LoadedCosts},
    method_chain_commands, method_stage_commands, storage, Arguments,
};
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStageBinding},
    method_chain_costs::{assess_chain_costs, ChainStageEvidence},
    method_stage::{StageAllocation, StageTrialBinding},
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    stages: Vec<StageInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StageInput {
    stage_id: String,
    target: Target,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Generation {
        cost_manifest: PathBuf,
        allocation: PathBuf,
        admission: PathBuf,
    },
    TrialOnly {
        cost_manifest: PathBuf,
    },
}
enum LoadedTarget {
    Generation(Box<GenerationFiles>),
    TrialOnly,
}
struct GenerationFiles {
    allocation: StageAllocation,
    admission: GenerationAdmission,
    trial_binding: Option<StageTrialBinding>,
}
struct LoadedStage {
    binding: ChainStageBinding,
    costs: LoadedCosts,
    target: LoadedTarget,
}
impl LoadedStage {
    fn evidence(&self) -> Result<ChainStageEvidence<'_>, String> {
        match &self.target {
            LoadedTarget::Generation(generation) => {
                let GenerationFiles {
                    allocation,
                    admission,
                    trial_binding,
                } = generation.as_ref();
                let trial = match (&self.costs.trial, trial_binding) {
                    (Some((plan, rows)), Some(binding)) => Some((binding, plan, rows.as_slice())),
                    (None, None) => None,
                    _ => return Err("generation trial requires its retained binding".into()),
                };
                Ok(ChainStageEvidence::Generation {
                    binding: &self.binding,
                    allocation,
                    admission,
                    scope: &self.costs.scope,
                    snapshots: &self.costs.generations,
                    trial,
                })
            }
            LoadedTarget::TrialOnly => {
                let (trial, rows) = self
                    .costs
                    .trial
                    .as_ref()
                    .ok_or("trial-only stage requires its exact trial")?;
                Ok(ChainStageEvidence::TrialOnly {
                    binding: &self.binding,
                    scope: &self.costs.scope,
                    trial,
                    evidence: rows,
                })
            }
        }
    }
}
fn resolve(base: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("chain cost paths must not be empty".into());
    }
    Ok(base.join(path))
}
pub fn run(args: &Arguments) -> Result<u8, String> {
    let chain_path = args.path("--chain")?;
    let chain: ChainAllocation = storage::read_json(chain_path)?;
    let manifest_path = args.path("--manifest")?;
    let manifest: Manifest = storage::read_json(manifest_path)?;
    if manifest.schema != "genesis-chain-cost-input/1" || manifest.stages.len() > 64 {
        return Err("invalid chain cost schema or stage count".into());
    }
    let mut selected = BTreeSet::new();
    for input in &manifest.stages {
        if !chain.stages().iter().any(|s| s.limits.id == input.stage_id)
            || !selected.insert(&input.stage_id)
        {
            return Err("duplicate or undeclared chain cost stage".into());
        }
    }
    let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut loaded = Vec::new();
    for input in &manifest.stages {
        // Membership above validates the identifier before using it in a pathname.
        let binding: ChainStageBinding = storage::read_json(&method_chain_commands::companion(
            chain_path,
            &input.stage_id,
        ))?;
        if binding.stage_id() != input.stage_id {
            return Err("retained companion names a different chain stage".into());
        }
        let (costs, target) = match &input.target {
            Target::Generation {
                cost_manifest,
                allocation,
                admission,
            } => {
                let costs = load(&resolve(base, cost_manifest)?)?;
                let path = resolve(base, allocation)?;
                let allocation = storage::read_json(&path)?;
                let admission = storage::read_json(&resolve(base, admission)?)?;
                let trial_binding = if costs.trial.is_some() {
                    Some(storage::read_json(&method_stage_commands::binding_path(
                        &path,
                    ))?)
                } else {
                    None
                };
                (
                    costs,
                    LoadedTarget::Generation(Box::new(GenerationFiles {
                        allocation,
                        admission,
                        trial_binding,
                    })),
                )
            }
            Target::TrialOnly { cost_manifest } => {
                let costs = load(&resolve(base, cost_manifest)?)?;
                if !costs.generations.is_empty()
                    || !costs.scope.generations.is_empty()
                    || !costs.scope.generated_links.is_empty()
                {
                    return Err("trial-only cost input cannot contain generation evidence".into());
                }
                (costs, LoadedTarget::TrialOnly)
            }
        };
        loaded.push(LoadedStage {
            binding,
            costs,
            target,
        });
    }
    let inputs = loaded
        .iter()
        .map(LoadedStage::evidence)
        .collect::<Result<Vec<_>, _>>()?;
    emit(&assess_chain_costs(&chain, &inputs).map_err(|e| e.to_string())?)?;
    Ok(0)
}
