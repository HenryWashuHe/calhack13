//! Bounded offline capture of all development selections. No source execution.
use crate::{emit, storage, Arguments};
use genesis_core::digest::canonical_sha256;
use genesis_forge::{
    process_cohort::{assess_cohort, CohortDeclaration},
    process_generation_origins::{assess_generation_origins, GenerationOrigins},
    process_selection_record::RetainedSelection,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    declaration: PathBuf,
    selections: [PathBuf; 8],
    #[serde(default)]
    generation_origins: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedCohort {
    schema: String,
    declaration: CohortDeclaration,
    selections: [RetainedSelection; 8],
    report: Value,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    generation_origins: Option<GenerationOrigins>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    generation_report: Option<Value>,
}
// Absent fields preserve legacy bytes; explicit null is not a valid receipt.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)?
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom("generation provenance must not be null"))
}
fn resolve(base: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("cohort input paths must not be empty".into());
    }
    Ok(base.join(path))
}
pub fn run(args: &Arguments) -> Result<u8, String> {
    let path = args.path("--cohort")?;
    let (retained, report) = if args.command == "process-cohort-prepare" {
        let manifest_path = args.path("--manifest")?;
        let manifest: Manifest = storage::read_json(manifest_path)?;
        if manifest.schema != "genesis-process-cohort-input/1" {
            return Err("unsupported cohort input schema".into());
        }
        let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let declaration = storage::read_json(&resolve(base, &manifest.declaration)?)?;
        let selections = manifest
            .selections
            .iter()
            .map(|p| storage::read_json(&resolve(base, p)?))
            .collect::<Result<Vec<_>, String>>()?
            .try_into()
            .map_err(|_| "expected eight selections")?;
        let report = assess_cohort(&declaration, &selections)?;
        let generation_origins = manifest
            .generation_origins
            .as_ref()
            .map(|p| storage::read_json(&resolve(base, p)?))
            .transpose()?;
        let generation_report = generation_origins
            .as_ref()
            .map(|origins| assess_generation_origins(origins, &declaration, &selections))
            .transpose()?;
        let retained = RetainedCohort {
            schema: if generation_origins.is_some() {
                "genesis-retained-process-cohort/2"
            } else {
                "genesis-retained-process-cohort/1"
            }
            .into(),
            declaration,
            selections,
            report: serde_json::to_value(&report).map_err(|e| e.to_string())?,
            generation_origins,
            generation_report,
        };
        // All original records are validated before publication. Existing writer
        // bounds the complete capture to 8 MiB, private/create-only/durable.
        storage::CheckpointLock::acquire(path)?.create_compact(&retained)?;
        (retained, report)
    } else {
        let retained: RetainedCohort = storage::read_json(path)?;
        match (
            retained.schema.as_str(),
            &retained.generation_origins,
            &retained.generation_report,
        ) {
            ("genesis-retained-process-cohort/1", None, None) => {}
            ("genesis-retained-process-cohort/2", Some(origins), Some(saved)) => {
                let report =
                    assess_generation_origins(origins, &retained.declaration, &retained.selections)?;
                if &report != saved {
                    return Err("retained generation report differs from recomputed origins".into());
                }
            }
            _ => return Err("unsupported retained cohort schema or generation provenance".into()),
        }
        let report = assess_cohort(&retained.declaration, &retained.selections)?;
        if retained.report != serde_json::to_value(&report).map_err(|e| e.to_string())? {
            return Err("retained report differs from recomputed cohort".into());
        }
        (retained, report)
    };
    let selections: Vec<_> = report
        .cells
        .iter()
        .map(|cell| {
            let s = &cell.selection;
            let selected = s.selected_round.map(|round| {
                let row = &s.rows[usize::from(round - 1)];
                json!({"round":round,"method":row.method,"plan":row.plan})
            });
            json!({"arm":s.arm,"family":s.family,"selection_sha256":cell.selection_sha256,
            "selected":selected})
        })
        .collect();
    let mut summary = json!({"schema":"genesis-process-cohort-summary/1","scope":report.scope,
        "cohort_sha256":canonical_sha256(&json!(retained)),
        "declaration_sha256":report.declaration_sha256,
        "report_sha256":canonical_sha256(&retained.report),"selections":selections});
    if let (Some(origins), Some(report)) =
        (&retained.generation_origins, &retained.generation_report)
    {
        summary["generation_origins_sha256"] = canonical_sha256(&json!(origins)).into();
        summary["generation_report_sha256"] = canonical_sha256(report).into();
    }
    emit(&summary)?;
    Ok(0)
}
