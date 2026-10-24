//! `lab-eval` CLI. JSON in, JSON out.
//!
//!   lab-eval instance <family> <seed>
//!   lab-eval experiment <design.json> <family> <seed> <steps-remaining>
//!   lab-eval accept <design.json> <family> <seed>...
//!   lab-eval commit <comparison> <salt> <family> <count>
//!   lab-eval compare <design.json> <comparison> <family> <count> <commitment> <salt> <index>
use genesis_lab_eval::{commit, design::Design, family, score};
use std::process::ExitCode;

fn design(path: &str) -> Result<Design, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("design {path}: {e}"))
}
fn fam(s: &str) -> Result<family::Family, String> {
    family::Family::parse(s).ok_or_else(|| format!("unknown family {s}"))
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
                serde_json::to_value(family::instance(fam(f)?, num(seed)?)).unwrap()
            }
            ["experiment", d, f, seed, steps] => {
                match score::experiment(&design(d)?, fam(f)?, num(seed)?, num(steps)?) {
                    Ok(r) => serde_json::to_value(r).unwrap(),
                    Err(e) => return Err(format!("refused: {e:?}")),
                }
            }
            ["accept", d, f, seeds @ ..] if !seeds.is_empty() => {
                let d = design(d)?;
                let f = fam(f)?;
                let mut receipts = vec![];
                for s in seeds {
                    receipts.push(
                        score::score_acceptance(&d, f, num(s)?)
                            .map_err(|e| format!("refused: {e:?}"))?,
                    );
                }
                let ok = receipts
                    .iter()
                    .filter(|r| r.metrics.as_ref().is_some_and(|m| m.success))
                    .count();
                serde_json::json!({"succeeded": ok, "of": receipts.len(), "receipts": receipts})
            }
            ["commit", cmp, salt, f, n] => serde_json::json!({
                "comparison": cmp, "family": f, "count": num::<u32>(n)?,
                "commitment": commit::commitment(cmp, salt, fam(f)?, num(n)?)
            }),
            ["compare", d, cmp, f, n, c, salt, i] => {
                let set = score::HiddenSet {
                    comparison: cmp,
                    family: fam(f)?,
                    count: num(n)?,
                    commitment: c,
                };
                serde_json::to_value(
                    score::score_comparison(&design(d)?, &set, salt, num(i)?)
                        .map_err(|e| format!("refused: {e:?}"))?,
                )
                .unwrap()
            }
            _ => return Err("usage: see the header of src/bin/lab-eval.rs".into()),
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
