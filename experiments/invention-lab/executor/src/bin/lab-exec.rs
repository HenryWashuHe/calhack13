//! `lab-exec`: run one frozen trial slot. JSON out.
//!
//!   lab-exec ref evaluator | environment <namespaces|none> | method <method.json>
//!   lab-exec example-plan <namespaces|none> <instances> <sim-steps> <wall-ms> <out.json>
//!   lab-exec run <plan.json> <slot> <method.json> <state-dir> <namespaces|none>
//!   lab-exec recover <plan.json> <slot> <state-dir> <namespaces|none>
//!   lab-exec check <plan.json> <slot> <state-dir>
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExecutionReceipt, ResourceGrant};
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
fn method_ref(path: &str) -> Result<genesis_core::development_method::ExactContentRef, String> {
    run::Method::load(Path::new(path))?.reference()
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
            ["ref", "method", path] => serde_json::to_value(method_ref(path)?).unwrap(),
            ["example-plan", m, n, steps, wall, out] => {
                let root = env!("CARGO_MANIFEST_DIR");
                let methods = example::ExampleMethods {
                    candidate: method_ref(&format!("{root}/methods/random-search/method.json"))?,
                    control: method_ref(&format!("{root}/methods/fixed-cart/method.json"))?,
                };
                let grant = ResourceGrant {
                    model_calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    wall_time_ms: num(wall)?,
                    simulation_steps: num(steps)?,
                };
                let plan = example::acceptance_plan(
                    &methods,
                    mode(m)?,
                    &interpreter_version(),
                    num(n)?,
                    grant,
                );
                let reference = plan.reference().map_err(|e| format!("plan: {e:?}"))?;
                std::fs::write(out, serde_json::to_vec_pretty(&plan).unwrap())
                    .map_err(|e| format!("write {out}: {e}"))?;
                serde_json::json!({"plan": reference, "path": out})
            }
            ["run", plan, slot, method, dir, m] => {
                let plan: TrialPlan = json(plan)?;
                let method = run::Method::load(Path::new(method))?;
                finished(
                    run::execute(&plan, slot, &method, Path::new(dir), &config(mode(m)?))
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
            ["check", plan, slot, dir] => {
                let plan: TrialPlan = json(plan)?;
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
                        scoring: None,
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
