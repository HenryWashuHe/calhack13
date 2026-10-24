//! Offline declaration and assessment. No mutation of generation checkpoints.
use crate::{emit, storage, Arguments};
use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_roster::GenerationRoster, method_synthesis::MethodSynthesisSession,
    text_generation::TextModelSettings,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    schema: String,
    id: String,
    settings: TextModelSettings,
    total_grant: ResourceGrant,
    prepared_checkpoints: Vec<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshots {
    schema: String,
    generation_checkpoints: Vec<PathBuf>,
}
fn sessions(base: &Path, paths: &[PathBuf]) -> Result<Vec<MethodSynthesisSession>, String> {
    if paths.len() > 64 {
        return Err("at most 64 generation checkpoints are allowed".into());
    }
    paths
        .iter()
        .map(|path| {
            if path.as_os_str().is_empty() {
                return Err("checkpoint path must not be empty".into());
            }
            storage::read_json(&base.join(path))
        })
        .collect()
}
pub fn run(args: &Arguments) -> Result<u8, String> {
    let manifest = args.path("--manifest")?;
    let base = manifest.parent().unwrap_or_else(|| Path::new("."));
    if args.command == "method-roster-prepare" {
        let input: Declaration = storage::read_json(manifest)?;
        if input.schema != "genesis-generation-roster-input/1" {
            return Err("unsupported roster declaration schema".into());
        }
        let prepared = sessions(base, &input.prepared_checkpoints)?;
        let roster =
            GenerationRoster::freeze(input.id, input.settings, input.total_grant, &prepared)
                .map_err(|e| e.to_string())?;
        let lock = storage::CheckpointLock::acquire(args.path("--roster")?)?;
        lock.create(&roster)?;
        emit(
            &serde_json::json!({"roster_sha256":roster.sha256().map_err(|e|e.to_string())?,
            "declared_attempts":prepared.len(), "authority":"declaration only; does not gate provider calls or prove chronology"}),
        )?;
    } else {
        let input: Snapshots = storage::read_json(manifest)?;
        if input.schema != "genesis-generation-roster-snapshots/1" {
            return Err("unsupported roster snapshot schema".into());
        }
        let roster: GenerationRoster = storage::read_json(args.path("--roster")?)?;
        let snapshots = sessions(base, &input.generation_checkpoints)?;
        emit(&roster.assess(&snapshots).map_err(|e| e.to_string())?)?;
    }
    Ok(0)
}
