//! Offline selection capture. A retained record proves consistency, not timing,
//! origin, whole-protocol compliance or authority to execute the selected method.
use crate::{emit, storage, Arguments};
use genesis_core::{development_trial::TrialEvidence, digest::canonical_sha256};
use genesis_forge::process_selection::{
    select_development, Arm, Family, RoundInput, SelectionReport, TrialInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    arm: Arm,
    family: Family,
    rounds: [RoundFiles; 3],
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RoundFiles {
    NoMethod {
        reason: String,
    },
    Trial {
        plan: PathBuf,
        criterion: PathBuf,
        evidence: Option<PathBuf>,
        metrics: Option<PathBuf>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    arm: Arm,
    family: Family,
    rounds: [RoundInput; 3],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedSelection {
    schema: String,
    inputs: Inputs,
    report: Value,
}

fn resolve(base: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("selection input paths must not be empty".into());
    }
    Ok(base.join(path))
}

fn capture(path: &Path) -> Result<Inputs, String> {
    let manifest: Manifest = storage::read_json(path)?;
    if manifest.schema != "genesis-process-selection-input/1" {
        return Err("unsupported selection input schema".into());
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut rounds = Vec::with_capacity(3);
    for round in manifest.rounds {
        rounds.push(match round {
            RoundFiles::NoMethod { reason } => RoundInput::NoMethod { reason },
            RoundFiles::Trial {
                plan,
                criterion,
                evidence,
                metrics,
            } => RoundInput::Trial {
                input: Box::new(TrialInput {
                    plan: storage::read_json(&resolve(base, &plan)?)?,
                    criterion: storage::read_bytes(&resolve(base, &criterion)?, 64 * 1024)?,
                    evidence: evidence
                        .map(|p| storage::read_json::<TrialEvidence>(&resolve(base, &p)?))
                        .transpose()?,
                    metrics: metrics
                        .map(|p| storage::read_bytes(&resolve(base, &p)?, 1024 * 1024))
                        .transpose()?,
                }),
            },
        });
    }
    Ok(Inputs {
        arm: manifest.arm,
        family: manifest.family,
        rounds: rounds.try_into().map_err(|_| "expected three rounds")?,
    })
}

fn report(inputs: &Inputs) -> Result<SelectionReport, String> {
    select_development(inputs.arm, inputs.family, &inputs.rounds)
}

pub fn run(args: &Arguments) -> Result<u8, String> {
    let path = args.path("--selection")?;
    let (retained, computed) = if args.command == "process-select-prepare" {
        // Resolve/read/validate every input before touching the destination.
        let inputs = capture(args.path("--manifest")?)?;
        let computed = report(&inputs)?;
        let retained = RetainedSelection {
            schema: "genesis-retained-process-selection/1".into(),
            inputs,
            report: serde_json::to_value(&computed).map_err(|e| e.to_string())?,
        };
        storage::CheckpointLock::acquire(path)?.create_compact(&retained)?;
        (retained, computed)
    } else {
        // Read-only even when a crashed preparation left a lock behind.
        let retained: RetainedSelection = storage::read_json(path)?;
        if retained.schema != "genesis-retained-process-selection/1" {
            return Err("unsupported retained selection schema".into());
        }
        let computed = report(&retained.inputs)?;
        if retained.report != serde_json::to_value(&computed).map_err(|e| e.to_string())? {
            return Err("retained report differs from recomputed selection".into());
        }
        (retained, computed)
    };
    let selected = computed.selected_round.map(|round| {
        let row = &computed.rows[usize::from(round - 1)];
        json!({"round":round, "method":row.method, "plan":row.plan})
    });
    emit(&json!({
        "schema":"genesis-process-selection-summary/1",
        "scope":computed.scope,
        "arm":computed.arm, "family":computed.family,
        "selection_sha256":canonical_sha256(&json!(retained)),
        "input_sha256":computed.input_sha256,
        "report_sha256":canonical_sha256(&retained.report),
        "selected":selected,
    }))?;
    Ok(0)
}
