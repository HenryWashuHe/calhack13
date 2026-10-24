//! Experiments (metered development runs) and scoring receipts.
//! Outcomes follow genesis-history-experiment #22: design success is a metric
//! under `Scored`; `Invalid` is a design-validation failure; `Failed` and
//! `Unknown` describe the scoring worker, never the design.
use crate::design::{hex, Design, LANGUAGE_VERSION};
use crate::family::{instance, Family, Instance, FAMILY_VERSION};
use crate::sim::{run, Episode, Frame};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const EVALUATOR_ID: &str = "genesis-lab-eval";
pub const EVALUATOR_VERSION: &str = "0.1.0";

/// Digest over everything that decides a score: engine version, this crate's
/// version, design language, family version and the material/motor tables
/// (which live in the source of this version).
pub fn evaluator_digest() -> String {
    let id = format!(
        "{EVALUATOR_ID}@{EVALUATOR_VERSION};rapier2d=0.22.0;enhanced-determinism;language={LANGUAGE_VERSION};families={FAMILY_VERSION}"
    );
    hex(&Sha256::digest(id.as_bytes()))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Scored,
    Invalid,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    pub success: bool,
    pub progress: f32,
    pub cost: f32,
    pub joints_broken: u32,
    pub steps_to_target: Option<u32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// Metered development run inside a method execution.
    Experiment,
    /// An organization's local acceptance check; development instances only.
    Acceptance,
    /// Declared comparison; hidden instances after reveal only.
    Comparison,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub evaluator: String,
    pub evaluator_sha256: String,
    pub purpose: Purpose,
    pub language: u32,
    pub design_sha256: String,
    pub family: Family,
    pub family_version: u32,
    pub seed: u64,
    /// Hidden instances: the commitment the seed was derived under.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_commitment: Option<String>,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Metrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_sha256: Option<String>,
    /// Steps actually simulated; 0 when nothing ran.
    pub simulation_steps: u32,
}

/// Result of a development experiment, with the display trace.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExperimentResult {
    pub receipt: Receipt,
    pub frames: Vec<Frame>,
    pub steps_remaining: u64,
}

fn base(design: &Design, inst: &Instance, purpose: Purpose) -> Receipt {
    Receipt {
        evaluator: format!("{EVALUATOR_ID}@{EVALUATOR_VERSION}"),
        evaluator_sha256: evaluator_digest(),
        purpose,
        language: design.language,
        design_sha256: design.digest(),
        family: inst.family,
        family_version: inst.version,
        seed: inst.seed,
        hidden_commitment: None,
        outcome: Outcome::Unknown,
        metrics: None,
        reason: None,
        trace_sha256: None,
        simulation_steps: 0,
    }
}

fn evaluate(design: &Design, inst: &Instance, purpose: Purpose) -> (Receipt, Vec<Frame>) {
    let mut r = base(design, inst, purpose);
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
            r.simulation_steps = steps_run;
            (r, vec![])
        }
        Ok(ep) => {
            r.outcome = Outcome::Scored;
            r.simulation_steps = ep.steps_run;
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
            (r, vec![])
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    HiddenInstance,
    BudgetSpent {
        needed: u32,
        remaining: u64,
    },
    /// The salt doesn't open the published commitment.
    WrongSalt,
    /// Outside the committed draw.
    IndexOutOfRange,
}

/// One declared comparison's hidden draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HiddenSet<'a> {
    pub comparison: &'a str,
    pub family: Family,
    pub count: u32,
    pub commitment: &'a str,
}

/// Development seeds are below 2^32; hidden seeds have the top bit set.
pub fn is_hidden_seed(seed: u64) -> bool {
    seed >= 1 << 63
}

/// A metered development run. Refuses hidden instances, and refuses up front
/// when the remaining simulation steps can't cover a full episode.
pub fn experiment(
    design: &Design,
    family: Family,
    seed: u64,
    steps_remaining: u64,
) -> Result<ExperimentResult, Refusal> {
    if is_hidden_seed(seed) {
        return Err(Refusal::HiddenInstance);
    }
    let needed = family.max_steps();
    if steps_remaining < needed as u64 {
        return Err(Refusal::BudgetSpent {
            needed,
            remaining: steps_remaining,
        });
    }
    let inst = instance(family, seed);
    let (receipt, frames) = evaluate(design, &inst, Purpose::Experiment);
    let used = receipt.simulation_steps as u64;
    Ok(ExperimentResult {
        steps_remaining: steps_remaining - used,
        receipt,
        frames,
    })
}

/// Acceptance scoring on a development instance.
pub fn score_acceptance(design: &Design, family: Family, seed: u64) -> Result<Receipt, Refusal> {
    if is_hidden_seed(seed) {
        return Err(Refusal::HiddenInstance);
    }
    Ok(evaluate(design, &instance(family, seed), Purpose::Acceptance).0)
}

/// Private comparison scoring by the salt holder (holdout timeline step 5):
/// the salt must open the published commitment and the index must be inside
/// the committed draw. Anyone can re-run this after the reveal.
pub fn score_comparison(
    design: &Design,
    set: &HiddenSet<'_>,
    salt: &str,
    index: u32,
) -> Result<Receipt, Refusal> {
    if crate::commit::commitment(set.comparison, salt, set.family, set.count) != set.commitment {
        return Err(Refusal::WrongSalt);
    }
    if index >= set.count {
        return Err(Refusal::IndexOutOfRange);
    }
    let seed = crate::commit::hidden_seed(set.comparison, salt, set.family, index);
    let mut r = evaluate(design, &instance(set.family, seed), Purpose::Comparison).0;
    r.hidden_commitment = Some(set.commitment.to_owned());
    Ok(r)
}
