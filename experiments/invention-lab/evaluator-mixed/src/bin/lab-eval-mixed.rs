//! `lab-eval-mixed` CLI for `mixed-terrain/1`. JSON in, JSON out. Same
//! commands and output shapes as `lab-eval` for the parts calibration uses:
//!
//!   lab-eval-mixed instance mixed-terrain <seed>
//!   lab-eval-mixed experiment <design.json> mixed-terrain <seed> <reserved-steps>
//!   lab-eval-mixed accept <design.json> mixed-terrain <seed>...
//!   lab-eval-mixed identity
//!
//! Local inspection primitives only: no grant, no ledger, never given to a
//! development method. Any other family name is refused.
use genesis_lab_eval::design::Design;
use genesis_lab_eval_mixed::{family, score};
use std::process::ExitCode;

fn design(path: &str) -> Result<Design, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("design {path}: {e}"))
}
fn fam(s: &str) -> Result<(), String> {
    if s == family::FAMILY {
        Ok(())
    } else {
        Err(format!("unknown family {s}"))
    }
}
fn num<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("not a number: {s}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = (|| -> Result<serde_json::Value, String> {
        Ok(match a.as_slice() {
            ["instance", f, seed] => {
                fam(f)?;
                serde_json::to_value(family::instance(num(seed)?)).unwrap()
            }
            ["experiment", d, f, seed, steps] => {
                fam(f)?;
                match score::experiment(&design(d)?, num(seed)?, num(steps)?) {
                    Ok(r) => serde_json::to_value(r).unwrap(),
                    Err(e) => return Err(format!("refused: {e:?}")),
                }
            }
            ["accept", d, f, seeds @ ..] if !seeds.is_empty() => {
                fam(f)?;
                let d = design(d)?;
                let mut receipts = vec![];
                for s in seeds {
                    receipts.push(
                        score::score_acceptance(&d, num(s)?)
                            .map_err(|e| format!("refused: {e:?}"))?,
                    );
                }
                let ok = receipts
                    .iter()
                    .filter(|r| r.metrics.as_ref().is_some_and(|m| m.success))
                    .count();
                serde_json::json!({"succeeded": ok, "of": receipts.len(), "receipts": receipts})
            }
            ["identity"] => serde_json::json!({
                "evaluator": format!("{}@{}", score::EVALUATOR_ID, score::EVALUATOR_VERSION),
                "evaluator_sha256": score::evaluator_digest(),
                "family": family::FAMILY,
                "family_version": family::FAMILY_VERSION,
                "files": score::digest_files(),
                "v1_evaluator_sha256": genesis_lab_eval::score::evaluator_digest(),
            }),
            _ => return Err("usage: see the header of src/bin/lab-eval-mixed.rs".into()),
        })
    })();
    match out {
        Ok(v) => {
            println!("{}", serde_json::to_string(&v).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
