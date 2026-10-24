//! `lab-exec`: run one frozen trial slot. JSON out.
//!
//!   lab-exec ref evaluator | environment <namespaces|none> | runtime | protocol
//!   lab-exec package <source-dir> <id> <version> <entry> <out.json>
//!   lab-exec synthesis-plan <family> <model> <operation-id> <out.json>
//!     (a genesis-forge MethodSynthesisPlan for this host's runtime and protocol)
//!   lab-exec example-plan <namespaces|none> <instances> <sim-steps> <wall-ms> <out-dir> [family [candidate.package.json]]
//!     (writes plan.json, criterion.json and both control packages)
//!   lab-exec run <plan.json> <slot> <package.json> <state-dir> <namespaces|none>
//!   lab-exec recover <plan.json> <slot> <state-dir> <namespaces|none>
//!   lab-exec check <plan.json> <slot> <state-dir> [score-dir]
use genesis_core::development_evaluation::EvaluationReceipt;
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExactContentRef, ExecutionReceipt, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::{assess_trials, TrialEvidence, TrialPlan};
use genesis_lab_exec::{example, refs, run, sandbox::Mode};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn mode(s: &str) -> Result<Mode, String> {
    match s {
        "namespaces" => Ok(Mode::Namespaces),
        "none" => Ok(Mode::None),
        _ => Err(format!("unknown sandbox mode {s}")),
    }
}
fn num<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("not a number: {s}"))
}
fn json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}
fn interpreter_version() -> String {
    run::interpreter_version("python3")
}
fn config(m: Mode) -> run::Config {
    run::Config {
        sandbox: m,
        interpreter_version: interpreter_version(),
        stage_root: match m {
            Mode::Namespaces => PathBuf::from("/run/genesis-lab"),
            Mode::None => std::env::temp_dir().join("genesis-lab-stage"),
        },
    }
}
/// Every file under `dir`, as a #36 package for this host's runtime.
fn package(dir: &Path, id: &str, version: &str, entry: &str) -> Result<MethodPackage, String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<MethodSourceFile>) -> Result<(), String> {
        for e in std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
            let path = e.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let content = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
                out.push(MethodSourceFile { path: rel, content });
            }
        }
        Ok(())
    }
    let mut files = vec![];
    walk(dir, dir, &mut files)?;
    MethodPackage::new(
        id.into(),
        version.into(),
        refs::runtime(&interpreter_version()),
        refs::protocol(),
        entry.into(),
        vec![],
        files,
    )
    .map_err(|e| format!("package: {e}"))
}
fn write_package(p: &MethodPackage, out: &Path) -> Result<ExactContentRef, String> {
    let bytes = p.artifact_json().map_err(|e| format!("package: {e}"))?;
    std::fs::write(out, bytes.as_bytes()).map_err(|e| format!("write {}: {e}", out.display()))?;
    p.reference().map_err(|e| format!("package: {e}"))
}
fn finished(f: genesis_lab_exec::session::Finished) -> serde_json::Value {
    serde_json::json!({
        "outcome": f.receipt.outcome,
        "outputs": f.receipt.outputs,
        "usage": f.receipt.usage,
        "transcript": f.receipt.transcript,
        "transcript_path": f.transcript_path,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = (|| -> Result<serde_json::Value, String> {
        Ok(match a.as_slice() {
            ["ref", "evaluator"] => serde_json::to_value(refs::evaluator()).unwrap(),
            ["ref", "environment", m] => {
                serde_json::to_value(refs::environment(mode(m)?, &interpreter_version())).unwrap()
            }
            ["ref", "runtime"] => {
                serde_json::to_value(refs::runtime(&interpreter_version())).unwrap()
            }
            ["ref", "protocol"] => serde_json::to_value(refs::protocol()).unwrap(),
            ["package", dir, id, version, entry, out] => serde_json::to_value(write_package(
                &package(Path::new(dir), id, version, entry)?,
                Path::new(out),
            )?)
            .unwrap(),
            ["synthesis-plan", family, model, operation, out] => {
                let family = genesis_lab_eval::family::Family::parse(family)
                    .ok_or_else(|| format!("unknown family {family}"))?;
                let doc = |schema: &str, id: &str, text: &str| serde_json::json!({"reference": refs::content(schema, id, "1", text.as_bytes()), "text": text});
                let task = format!(
                    "Task family {name} (version {version}) in the GENESIS invention lab. \
                     A run gets public development instances of this family through the protocol. \
                     Develop one design that meets the family's success rule on as many instances \
                     as possible, using experiments within the run's remaining simulation steps, \
                     and submit it. The submitted design is later scored by a separate trusted \
                     evaluator on the receiving organization's instances.",
                    name = family.name(),
                    version = genesis_lab_eval::family::FAMILY_VERSION,
                );
                let plan = serde_json::json!({
                    "operation_id": operation,
                    "org_id": "org_example_a",
                    "target_id": "generated-method",
                    "target_version": "0.1.0",
                    "runtime": refs::runtime(&interpreter_version()),
                    "protocol": {"reference": refs::protocol(), "text": refs::PROTOCOL},
                    "entrypoint": "method.py",
                    "dependencies": [],
                    "public_task": doc("lab-task-description/1", family.name(), &task),
                    "public_inputs": [doc("lab-design-language/1", "language-1", include_str!("../../DESIGN_LANGUAGE.md"))],
                    "instruction": "Write one Python 3 program using only the standard library that implements the protocol exactly: read the instances, spend experiments within the remaining simulation steps to search for a design that succeeds on many instances, then submit the best design found and exit 0. Never write anything but protocol requests to standard output.",
                    "settings": {"model": model, "max_tokens": 8192, "timeout_ms": 120000, "sampling": "provider_default"},
                });
                std::fs::write(out, serde_json::to_vec_pretty(&plan).unwrap())
                    .map_err(|e| format!("write {out}: {e}"))?;
                serde_json::json!({"plan": out, "runtime": refs::runtime(&interpreter_version()), "protocol": refs::protocol()})
            }
            ["example-plan", m, n, steps, wall, out, rest @ ..] if rest.len() <= 2 => {
                let family = match rest.first() {
                    Some(f) => genesis_lab_eval::family::Family::parse(f)
                        .ok_or_else(|| format!("unknown family {f}"))?,
                    None => genesis_lab_eval::family::Family::RavineCrossing,
                };
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("methods");
                let out = Path::new(out);
                std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
                // An optional candidate package (for example one exported by
                // genesis-forge method-generate-export) replaces the
                // random-search control; its bytes are frozen as-is.
                let candidate = match rest.get(1) {
                    Some(path) => {
                        let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
                        let v: serde_json::Value = serde_json::from_slice(&bytes)
                            .map_err(|e| format!("candidate package: {e}"))?;
                        let label = |k: &str| {
                            v["package"][k]
                                .as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| format!("candidate package has no {k}"))
                        };
                        std::fs::write(out.join("candidate.package.json"), &bytes)
                            .map_err(|e| format!("write candidate: {e}"))?;
                        ExactContentRef {
                            domain: "genesis".into(),
                            schema: "development-method-source/1".into(),
                            id: label("id")?,
                            version: label("version")?,
                            sha256: refs::sha256_hex(&bytes),
                        }
                    }
                    None => write_package(
                        &package(
                            &root.join("random-search"),
                            "seed-random-search",
                            "0.1.0",
                            "random_search.py",
                        )?,
                        &out.join("candidate.package.json"),
                    )?,
                };
                let methods = example::ExampleMethods {
                    candidate,
                    control: write_package(
                        &package(
                            &root.join("fixed-cart"),
                            "fixed-cart",
                            "0.1.0",
                            "fixed_cart.py",
                        )?,
                        &out.join("control.package.json"),
                    )?,
                };
                let grant = ResourceGrant {
                    model_calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    wall_time_ms: num(wall)?,
                    simulation_steps: num(steps)?,
                };
                let plan = example::acceptance_plan_for(
                    family,
                    &methods,
                    mode(m)?,
                    &interpreter_version(),
                    num(n)?,
                    grant,
                );
                let reference = plan.reference().map_err(|e| format!("plan: {e:?}"))?;
                let ids: Vec<String> = plan
                    .development_instances
                    .iter()
                    .map(|r| r.id.clone())
                    .collect();
                std::fs::write(
                    out.join("criterion.json"),
                    example::criterion_bytes(&ids, 1),
                )
                .map_err(|e| format!("write criterion: {e}"))?;
                let path = out.join("plan.json");
                std::fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap())
                    .map_err(|e| format!("write plan: {e}"))?;
                serde_json::json!({"plan": reference, "dir": out})
            }
            ["run", plan, slot, package, dir, m] => {
                let plan: TrialPlan = json(plan)?;
                let bytes = std::fs::read(package).map_err(|e| format!("read {package}: {e}"))?;
                finished(
                    run::execute(&plan, slot, &bytes, Path::new(dir), &config(mode(m)?))
                        .map_err(|e| format!("{e:?}"))?,
                )
            }
            ["recover", plan, slot, dir, m] => {
                let plan: TrialPlan = json(plan)?;
                finished(
                    run::recover(&plan, slot, Path::new(dir), &config(mode(m)?))
                        .map_err(|e| format!("{e:?}"))?,
                )
            }
            ["check", plan, slot, dir, rest @ ..] if rest.len() <= 1 => {
                let plan: TrialPlan = json(plan)?;
                let scoring: Option<EvaluationReceipt> = match rest.first() {
                    Some(out) => Some(json(&format!("{out}/evaluation.json"))?),
                    None => None,
                };
                let ledger: ExperimentLedger = json(&format!("{dir}/ledger.json"))?;
                let receipt: ExecutionReceipt = json(&format!("{dir}/receipt.json"))?;
                let scope = ExperimentScope {
                    plan: &plan,
                    slot_id: slot,
                };
                let budget = ledger
                    .validate_execution(scope, &receipt)
                    .map_err(|e| format!("execution: {e:?}"))?;
                let rows = assess_trials(
                    &plan,
                    &[TrialEvidence {
                        slot_id: slot.to_string(),
                        execution: receipt,
                        scoring,
                    }],
                )
                .map_err(|e| format!("trial: {e:?}"))?;
                let row = rows.iter().find(|r| r.slot_id == *slot).unwrap();
                serde_json::json!({
                    "budget": format!("{budget:?}"),
                    "experiments": ledger.records().len(),
                    "development_outcome": format!("{:?}", row.development_outcome),
                    "output": row.output,
                    "scoring_status": format!("{:?}", row.scoring_status),
                })
            }
            _ => return Err("usage: see the header of src/bin/lab-exec.rs".into()),
        })
    })();
    match out {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
