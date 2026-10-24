//! Cooperative retained chain files; no execution, admission or timing proof.
use crate::{emit, method_stage_commands, storage, Arguments};
use genesis_core::development_trial::TrialPlan;
use genesis_forge::{
    method_accounting::CostScope,
    method_chain::{ChainAllocation, ChainStageBinding},
    method_stage::{StageAllocation, StageTrialBinding},
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StageInput {
    schema: String,
    stage_id: String,
    target: Target,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Generation {
        allocation: PathBuf,
        trial: Option<TrialInput>,
    },
    TrialOnly {
        plan: PathBuf,
        scope: PathBuf,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrialInput {
    plan: PathBuf,
    scope: PathBuf,
}
fn resolve(base: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("chain input paths must not be empty".into());
    }
    Ok(base.join(path))
}
fn trial(base: &Path, plan: &Path, scope: &Path) -> Result<(TrialPlan, CostScope), String> {
    Ok((
        storage::read_json(&resolve(base, plan)?)?,
        storage::read_json(&resolve(base, scope)?)?,
    ))
}
fn expected(
    chain: &ChainAllocation,
    input: &StageInput,
    base: &Path,
) -> Result<ChainStageBinding, String> {
    if input.schema != "genesis-chain-stage-input/1" {
        return Err("unsupported chain stage input schema".into());
    }
    match &input.target {
        Target::Generation {
            allocation,
            trial: t,
        } => {
            let path = resolve(base, allocation)?;
            let allocation: StageAllocation = storage::read_json(&path)?;
            let binding = chain
                .bind_generation(&input.stage_id, &allocation)
                .map_err(|e| e.to_string())?;
            if let Some(t) = t {
                let (trial, scope) = trial(base, &t.plan, &t.scope)?;
                let retained: StageTrialBinding =
                    storage::read_json(&method_stage_commands::binding_path(&path))?;
                binding
                    .validate_trial(chain, Some((&allocation, &retained)), &trial, &scope)
                    .map_err(|e| e.to_string())?;
            }
            Ok(binding)
        }
        Target::TrialOnly { plan, scope } => {
            let (trial, scope) = trial(base, plan, scope)?;
            chain
                .bind_trial_only(&input.stage_id, &trial, &scope)
                .map_err(|e| e.to_string())
        }
    }
}
pub(crate) fn companion(chain: &Path, stage: &str) -> PathBuf {
    // Call only with a validated stage identifier from expected() or StageAllocation.
    let mut path = chain.as_os_str().to_os_string();
    path.push(format!(".stage-{stage}.json"));
    path.into()
}
pub fn run(args: &Arguments) -> Result<u8, String> {
    let path = args.path("--chain")?;
    let manifest = args.path("--manifest")?;
    if args.command == "method-chain-prepare" {
        let chain: ChainAllocation = storage::read_json(manifest)?;
        let lock = storage::CheckpointLock::acquire(path)?;
        lock.create(&chain)?;
        emit(&serde_json::json!({
            "schema": "genesis-chain-prepared/1",
            "chain_sha256": chain.sha256().map_err(|e| e.to_string())?,
        }))?;
    } else {
        // Read-only checks neither acquire nor remove a lock, including after a
        // crash. Binding holds the canonical chain lock across validation/save.
        let lock = if args.command == "method-chain-bind" {
            Some(storage::CheckpointLock::acquire(path)?)
        } else {
            None
        };
        let chain: ChainAllocation = storage::read_json(path)?;
        let input: StageInput = storage::read_json(manifest)?;
        let base = manifest.parent().unwrap_or_else(|| Path::new("."));
        let binding = expected(&chain, &input, base)?;
        let companion = companion(path, &input.stage_id);
        if lock.is_some() {
            storage::CheckpointLock::acquire(&companion)?.create(&binding)?;
        } else {
            let retained: ChainStageBinding = storage::read_json(&companion)?;
            if retained != binding {
                return Err("retained chain stage binding differs from input".into());
            }
        }
        emit(&binding)?;
    }
    Ok(0)
}
