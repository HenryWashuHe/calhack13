//! Experiment and acceptance primitives for `mixed-terrain/1`, with v1's
//! receipt shape and outcome meanings. Like v1's, these hold no grant and no
//! ledger and are not a method-facing contract. No hidden comparison here.
use crate::family::{instance, Instance, FAMILY, FAMILY_VERSION, MAX_STEPS};
use crate::sim::run;
use genesis_lab_eval::design::{hex, Design, LANGUAGE_VERSION};
use genesis_lab_eval::score::{Metrics, Outcome, Purpose};
use genesis_lab_eval::sim::{Episode, Frame};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const EVALUATOR_ID: &str = "genesis-lab-eval-mixed";
pub const EVALUATOR_VERSION: &str = "0.1.0";

/// Every file of this crate that decides a result. The v1 crate's own digest
/// (which covers design language v1) is folded in as well.
const CONTENT: [(&str, &[u8]); 6] = [
    ("Cargo.toml", include_bytes!("../Cargo.toml")),
    ("Cargo.lock", include_bytes!("../Cargo.lock")),
    ("src/lib.rs", include_bytes!("lib.rs")),
    ("src/family.rs", include_bytes!("family.rs")),
    ("src/sim.rs", include_bytes!("sim.rs")),
    ("src/score.rs", include_bytes!("score.rs")),
];

pub fn digest_files() -> Vec<&'static str> {
    CONTENT.iter().map(|(name, _)| *name).collect()
}

/// Content digest of this evaluator: its own sources and lockfile plus the v1
/// evaluator digest it builds on. It identifies source, not a binary.
pub fn evaluator_digest() -> String {
    let mut h = Sha256::new();
    h.update(format!(
        "{EVALUATOR_ID}@{EVALUATOR_VERSION};language={LANGUAGE_VERSION};family={FAMILY}/{FAMILY_VERSION};v1={}\n",
        genesis_lab_eval::score::evaluator_digest()
    ));
    for (name, bytes) in CONTENT {
        h.update((name.len() as u64).to_be_bytes());
        h.update(name.as_bytes());
        h.update((bytes.len() as u64).to_be_bytes());
        h.update(bytes);
    }
    hex(&h.finalize())
}

/// v1's receipt fields, with the family as a string.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub evaluator: String,
    pub evaluator_sha256: String,
    pub purpose: Purpose,
    pub language: u32,
    pub design_sha256: String,
    pub family: String,
    pub family_version: u32,
    pub seed: u64,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Metrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_sha256: Option<String>,
    /// 0 when nothing ran, `None` when usage is unknown (never zero).
    pub simulation_steps: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExperimentResult {
    pub receipt: Receipt,
    pub frames: Vec<Frame>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    HiddenInstance,
    ReservationTooSmall { needed: u32, offered: u64 },
}

fn evaluate(design: &Design, inst: &Instance, purpose: Purpose) -> (Receipt, Vec<Frame>) {
    let mut r = Receipt {
        evaluator: format!("{EVALUATOR_ID}@{EVALUATOR_VERSION}"),
        evaluator_sha256: evaluator_digest(),
        purpose,
        language: design.language,
        design_sha256: design.digest(),
        family: inst.family.clone(),
        family_version: inst.version,
        seed: inst.seed,
        outcome: Outcome::Unknown,
        metrics: None,
        reason: None,
        trace_sha256: None,
        simulation_steps: Some(0),
    };
    let cost = match design.validate(&inst.rules) {
        Ok(cost) => cost,
        Err(reason) => {
            r.outcome = Outcome::Invalid;
            r.reason = Some(reason);
            return (r, vec![]);
        }
    };
    match catch_unwind(AssertUnwindSafe(|| run(design, inst))) {
        Ok(Episode {
            diverged: true,
            steps_run,
            ..
        }) => {
            r.outcome = Outcome::Failed;
            r.reason = Some("the simulation produced a non-finite state".into());
            r.simulation_steps = Some(steps_run);
            (r, vec![])
        }
        Ok(ep) => {
            r.outcome = Outcome::Scored;
            r.simulation_steps = Some(ep.steps_run);
            r.trace_sha256 = Some(ep.trace_sha256);
            r.metrics = Some(Metrics {
                success: ep.success,
                progress: ep.progress,
                cost: (cost * 1000.0).round() / 1000.0,
                joints_broken: ep.joints_broken,
                steps_to_target: ep.steps_to_target,
            });
            (r, ep.frames)
        }
        Err(_) => {
            r.outcome = Outcome::Unknown;
            r.reason = Some("the scoring worker stopped before reporting a result".into());
            r.simulation_steps = None;
            (r, vec![])
        }
    }
}

fn is_hidden_seed(seed: u64) -> bool {
    seed >= 1 << 63
}

/// One development run under a reservation the caller's ledger already made.
pub fn experiment(
    design: &Design,
    seed: u64,
    reserved_steps: u64,
) -> Result<ExperimentResult, Refusal> {
    if is_hidden_seed(seed) {
        return Err(Refusal::HiddenInstance);
    }
    if reserved_steps < MAX_STEPS as u64 {
        return Err(Refusal::ReservationTooSmall {
            needed: MAX_STEPS,
            offered: reserved_steps,
        });
    }
    let (receipt, frames) = evaluate(design, &instance(seed), Purpose::Experiment);
    Ok(ExperimentResult { receipt, frames })
}

/// Acceptance simulation on a public instance.
pub fn score_acceptance(design: &Design, seed: u64) -> Result<Receipt, Refusal> {
    if is_hidden_seed(seed) {
        return Err(Refusal::HiddenInstance);
    }
    Ok(evaluate(design, &instance(seed), Purpose::Acceptance).0)
}
