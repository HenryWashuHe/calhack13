//! `lab-score`: trusted acceptance scoring and the acceptance verdict.
//!
//!   lab-score score <plan.json> <slot> <state-dir> <score-dir>
//!     writes <score-dir>/evaluation.json
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
