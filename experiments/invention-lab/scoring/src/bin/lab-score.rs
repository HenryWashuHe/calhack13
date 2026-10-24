//! `lab-score`: trusted acceptance scoring and the acceptance verdict.
//!
//!   lab-score score <plan.json> <slot> <state-dir> <score-dir>
//!     writes <score-dir>/evaluation.json
//!   lab-score comparison-plan <acceptance-plan.json> <comparison-id> <count> <salt-file> <out-dir>
//!     writes <out-dir>/plan.json (each slot an arm of trial t1, scored on the hidden
//!     draw) and <out-dir>/descriptor.json (the commitment, publishable; keep the salt)
//!   lab-score score-hidden <plan.json> <slot> <descriptor.json> <salt-file> <score-dir>
//!                          <slot>=<state-dir>...  (every slot of the closed comparison)
//!     writes <score-dir>/evaluation.json; the salt stays in its file
//!   lab-score verdict <plan.json> <criterion.json> <candidate-state> <candidate-score>
//!                     <control-state> <control-score>
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::ExecutionReceipt;
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::store::Store;
use genesis_lab_score::verdict::{decide, SlotEvidence};
use std::path::Path;
use std::process::ExitCode;

fn json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = (|| -> Result<serde_json::Value, String> {
        Ok(match a.as_slice() {
            ["score", plan, slot, dir, out] => {
                let plan: TrialPlan = json(plan)?;
                let receipt: ExecutionReceipt = json(&format!("{dir}/receipt.json"))?;
                let output = receipt
                    .outputs
                    .first()
                    .ok_or("no retained output to score")?;
                let bytes = std::fs::read(format!("{dir}/artifacts/{}", output.sha256))
                    .map_err(|e| format!("read output: {e}"))?;
                let store = Store::open(Path::new(out)).map_err(|e| e.to_string())?;
                let scored = genesis_lab_score::score_slot(&plan, slot, &receipt, &bytes, &store)
                    .map_err(|e| format!("{e:?}"))?;
                // score_slot already retained <score-dir>/evaluation.json.
                let metrics = match &scored.outcome {
                    EvaluationOutcome::Scored { metrics } => {
                        let m: serde_json::Value =
                            json(&format!("{out}/artifacts/{}", metrics.sha256))?;
                        serde_json::json!({"success_count": m["success_count"], "of": m["of"]})
                    }
                    other => serde_json::json!({"outcome": other}),
                };
                serde_json::json!({"slot": slot, "result": metrics, "usage": scored.usage})
            }
            ["comparison-plan", base, comparison, count, salt, out] => {
                let base: TrialPlan = json(base)?;
                let salt = std::fs::read_to_string(salt).map_err(|e| format!("read salt: {e}"))?;
                let salt = salt.trim_end_matches('\n');
                if salt.len() < 16 {
                    return Err("use a salt of at least 16 characters".into());
                }
                let family = base
                    .development_instances
                    .first()
                    .and_then(genesis_lab_exec::refs::resolve_instance)
                    .ok_or("the base plan's first instance does not resolve")?
                    .family;
                let count: u32 = count.parse().map_err(|_| "count is not a number")?;
                let descriptor =
                    genesis_lab_eval::commit::Descriptor::new(comparison, salt, family, count);
                let plan = genesis_lab_score::hidden::comparison_plan(&base, "t1", &descriptor);
                let reference = plan.reference().map_err(|e| format!("plan: {e:?}"))?;
                std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
                for (name, value) in [
                    ("plan.json", serde_json::to_value(&plan).unwrap()),
                    (
                        "descriptor.json",
                        serde_json::to_value(&descriptor).unwrap(),
                    ),
                ] {
                    std::fs::write(
                        format!("{out}/{name}"),
                        serde_json::to_vec_pretty(&value).unwrap(),
                    )
                    .map_err(|e| format!("write {name}: {e}"))?;
                }
                serde_json::json!({"plan": reference, "descriptor": descriptor})
            }
            ["score-hidden", plan, slot, descriptor, salt, out, arms @ ..] if !arms.is_empty() => {
                let plan: TrialPlan = json(plan)?;
                let descriptor: genesis_lab_eval::commit::Descriptor = json(descriptor)?;
                let salt = std::fs::read_to_string(salt).map_err(|e| format!("read salt: {e}"))?;
                let mut closure = vec![];
                for arm in arms {
                    let (id, dir) = arm
                        .split_once('=')
                        .ok_or_else(|| format!("expected <slot>=<state-dir>, got {arm}"))?;
                    let receipt: ExecutionReceipt = json(&format!("{dir}/receipt.json"))?;
                    let ended = genesis_lab_exec::run::termination(id, Path::new(dir))
                        .map_err(|e| format!("{dir}: {e:?}"))?
                        .ok_or_else(|| {
                            format!("slot {id}: no executor-observed termination (recovered or unfinished)")
                        })?;
                    closure.push((id, dir, receipt, ended));
                }
                let (_, dir, receipt, _) = closure
                    .iter()
                    .find(|(id, _, _, _)| id == slot)
                    .ok_or("the scored slot is not among the closed slots")?;
                let output = receipt
                    .outputs
                    .first()
                    .ok_or("no retained output to score")?;
                let bytes = std::fs::read(format!("{dir}/artifacts/{}", output.sha256))
                    .map_err(|e| format!("read output: {e}"))?;
                let refs: Vec<(&str, &ExecutionReceipt)> =
                    closure.iter().map(|(id, _, r, _)| (*id, r)).collect();
                let store = Store::open(Path::new(out)).map_err(|e| e.to_string())?;
                for (_, _, _, ended) in &closure {
                    genesis_lab_score::hidden::record_termination(&store, ended)
                        .map_err(|e| e.to_string())?;
                }
                let scored = genesis_lab_score::hidden::score_hidden(
                    &plan,
                    slot,
                    &bytes,
                    &refs,
                    &descriptor,
                    salt.trim_end_matches('\n'),
                    &store,
                )
                .map_err(|e| format!("{e:?}"))?;
                serde_json::json!({"slot": slot, "outcome": scored.outcome, "usage": scored.usage})
            }
            ["verdict", plan, criterion, cs, cx, ks, kx] => {
                let plan: TrialPlan = json(plan)?;
                let criterion =
                    std::fs::read(criterion).map_err(|e| format!("read criterion: {e}"))?;
                let load = |state: &str, score: &str| -> Result<SlotEvidence, String> {
                    let execution: ExecutionReceipt = json(&format!("{state}/receipt.json"))?;
                    let path = format!("{score}/evaluation.json");
                    let scoring: Option<EvaluationReceipt> = if Path::new(&path).exists() {
                        Some(json(&path)?)
                    } else {
                        None
                    };
                    let metrics = match scoring.as_ref().map(|r| &r.outcome) {
                        Some(EvaluationOutcome::Scored { metrics }) => Some(
                            std::fs::read(format!("{score}/artifacts/{}", metrics.sha256))
                                .map_err(|e| format!("read metrics: {e}"))?,
                        ),
                        _ => None,
                    };
                    Ok(SlotEvidence {
                        execution,
                        scoring,
                        metrics,
                    })
                };
                let verdict = decide(&plan, &criterion, &load(cs, cx)?, &load(ks, kx)?)?;
                serde_json::to_value(verdict).unwrap()
            }
            _ => return Err("usage: see the header of src/bin/lab-score.rs".into()),
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
