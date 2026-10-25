//! `lab-score`: trusted acceptance scoring and the acceptance verdict.
//!
//!   lab-score score <plan.json> <slot> <state-dir> <score-dir>
//!     writes <score-dir>/evaluation.json
//!   lab-score verdict <plan.json> <criterion.json> <candidate-state> <candidate-score>
//!                     <control-state> <control-score>
//!   lab-score eligibility <expected-plan.json> <slot> <state-dir> <score-dir>
//!     prints the shared process-comparison slot assessment
//!     (`genesis_core::development_verdict::assess_process_slot`); the plan must
//!     be the caller's independently retained expected plan, not one read back
//!     from the slot's own records
//!   lab-score feedback <plan.json> <slot> <state-dir> <score-dir> <out.json>
//!     writes the receiver's public-instance observations for the slot
//!     (create-only) and prints their content reference
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::ExecutionReceipt;
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::store::Store;
use genesis_lab_score::verdict::{assess_process_slot, decide, SlotEvidence};
use std::path::Path;
use std::process::ExitCode;

/// Largest retained JSON record (plan, receipt, criterion) this tool reads.
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
/// Largest metrics artifact; matches the shared assessment's own bound.
const MAX_METRICS_BYTES: u64 = 1024 * 1024;

/// Reads at most `limit` bytes of one regular file, refusing anything larger
/// without loading it.
fn bounded(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("read {path}: {e}"))?;
    if !file
        .metadata()
        .map_err(|e| format!("read {path}: {e}"))?
        .is_file()
    {
        return Err(format!("{path}: not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read {path}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{path}: larger than {limit} bytes"));
    }
    Ok(bytes)
}

fn json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = bounded(path, MAX_JSON_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}

/// A slot's retained execution receipt, optional scoring receipt and the
/// metrics bytes a `Scored` receipt names.
fn slot_evidence(state: &str, score: &str) -> Result<SlotEvidence, String> {
    let execution: ExecutionReceipt = json(&format!("{state}/receipt.json"))?;
    let path = format!("{score}/evaluation.json");
    let scoring: Option<EvaluationReceipt> = if Path::new(&path).exists() {
        Some(json(&path)?)
    } else {
        None
    };
    let metrics = match scoring.as_ref().map(|r| &r.outcome) {
        Some(EvaluationOutcome::Scored { metrics }) => {
            // The digest names a file: check its shape before it becomes a path.
            let digest = &metrics.sha256;
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err("metrics digest is not a lowercase SHA-256".into());
            }
            Some(bounded(
                &format!("{score}/artifacts/{digest}"),
                MAX_METRICS_BYTES,
            )?)
        }
        _ => None,
    };
    Ok(SlotEvidence {
        execution,
        scoring,
        metrics,
    })
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
                let verdict = decide(
                    &plan,
                    &criterion,
                    &slot_evidence(cs, cx)?,
                    &slot_evidence(ks, kx)?,
                )?;
                serde_json::to_value(verdict).unwrap()
            }
            ["eligibility", plan, slot, state, score] => {
                let plan: TrialPlan = json(plan)?;
                let assessment = assess_process_slot(&plan, slot, &slot_evidence(state, score)?)?;
                serde_json::to_value(assessment).unwrap()
            }
            ["feedback", plan, slot, state, score, out] => {
                let plan: TrialPlan = json(plan)?;
                let open = |d: &str| Store::open(Path::new(d)).map_err(|e| format!("{d}: {e}"));
                let (reference, bytes) = genesis_lab_score::feedback::receiver_feedback(
                    &plan,
                    slot,
                    &open(state)?,
                    &open(score)?,
                )
                .map_err(|e| format!("feedback: {e:?}"))?;
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(out)
                    .and_then(|mut f| f.write_all(&bytes).and_then(|_| f.sync_all()))
                    .map_err(|e| format!("write {out}: {e}"))?;
                serde_json::to_value(reference).unwrap()
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
