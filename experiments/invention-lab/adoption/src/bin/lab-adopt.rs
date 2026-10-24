//! `lab-adopt`: method adoption around the receiver's own acceptance trial.
//!
//!   lab-adopt receiver <org-id> <out.json>
//!     this host's runtime and protocol, no dependencies, nothing installed
//!   lab-adopt precheck <receiver.json> <publisher-org> <offer.package.json>
//!   lab-adopt conclude <receiver.json> <plan.json> <criterion.json>
//!                      <candidate-state> <candidate-score> <control-state> <control-score>
//!                      [<org-dir>]
//!     with <org-dir>, an adopted method is recorded once as
//!     <org-dir>/installations/<method sha256>.json (an existing record is refused)
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::{ExactContentRef, ExecutionReceipt};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_adopt::{conclude, precheck, Offer, Outcome, Receiver};
use genesis_lab_exec::{refs, run, store::Store};
use genesis_lab_score::verdict::SlotEvidence;
use std::path::Path;
use std::process::ExitCode;

fn json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn evidence(state: &str, score: &str) -> Result<SlotEvidence, String> {
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
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = (|| -> Result<serde_json::Value, String> {
        Ok(match a.as_slice() {
            ["receiver", org, out] => {
                let r = Receiver {
                    org_id: org.to_string(),
                    runtime: refs::runtime(&run::interpreter_version("python3")),
                    protocol: refs::protocol(),
                    allowed_dependencies: vec![],
                    installed: vec![],
                };
                let v = serde_json::to_value(&r).unwrap();
                std::fs::write(out, serde_json::to_vec_pretty(&v).unwrap())
                    .map_err(|e| format!("write {out}: {e}"))?;
                v
            }
            ["precheck", receiver, publisher, package] => {
                let receiver: Receiver = json(receiver)?;
                let bytes = std::fs::read(package).map_err(|e| format!("read {package}: {e}"))?;
                let v: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|e| format!("{package}: {e}"))?;
                let label = |k: &str| v["package"][k].as_str().unwrap_or_default().to_owned();
                let offer = Offer {
                    publisher: publisher.to_string(),
                    reference: ExactContentRef {
                        domain: "genesis".into(),
                        schema: "development-method-source/1".into(),
                        id: label("id"),
                        version: label("version"),
                        sha256: refs::sha256_hex(&bytes),
                    },
                    bytes,
                };
                serde_json::json!({"offer": offer.reference, "decision": precheck(&offer, &receiver)})
            }
            ["conclude", receiver, plan, criterion, cs, cx, ks, kx, rest @ ..]
                if rest.len() <= 1 =>
            {
                let receiver: Receiver = json(receiver)?;
                let plan: TrialPlan = json(plan)?;
                let criterion =
                    std::fs::read(criterion).map_err(|e| format!("read criterion: {e}"))?;
                let outcome = conclude(
                    &receiver,
                    &plan,
                    &criterion,
                    &evidence(cs, cx)?,
                    &evidence(ks, kx)?,
                )?;
                if let (Some(dir), Outcome::Adopted { installation }) = (rest.first(), &outcome) {
                    let store = Store::open(&Path::new(dir).join("installations"))
                        .map_err(|e| e.to_string())?;
                    let name = format!("{}.json", installation.method.sha256);
                    if store.path(&name).exists() {
                        return Err(format!("{name} is already installed"));
                    }
                    store
                        .write(&name, &serde_json::to_vec_pretty(installation).unwrap())
                        .map_err(|e| e.to_string())?;
                }
                serde_json::to_value(&outcome).unwrap()
            }
            _ => return Err("usage: see the header of src/bin/lab-adopt.rs".into()),
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
