//! One slot's experiment session. Every experiment goes through the #32
//! ledger: reserve, persist, evaluate, store the result, settle, persist, reply.
use crate::refs;
use crate::store::Store;
use genesis_core::development_evaluation::EvaluationError;
use genesis_core::development_experiment::{
    ExperimentLedger, ExperimentOutcome, ExperimentScope, ExperimentSettlement,
};
use genesis_core::development_method::{
    ExactContentRef, ExecutionOutcome, ExecutionReceipt, ResourceUsage, Usage,
};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_eval::design::Design;
use genesis_lab_eval::score::{self, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

/// What a slot directory records besides the ledger.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunState {
    pub started: bool,
    /// The method identity verified before the process started.
    pub method: Option<ExactContentRef>,
    pub submission: Option<ExactContentRef>,
    /// Set when finalization seals the ledger; nothing is accepted afterwards.
    #[serde(default)]
    pub sealed: bool,
    /// Set only after the receipt is durable.
    pub finished: bool,
}

#[derive(Debug)]
pub enum RunError {
    /// Nothing was executed; the slot stays missing.
    Refused(String),
    /// The slot already has execution evidence; it is never started again.
    AlreadyStarted,
    Store(std::io::Error),
    Ledger(EvaluationError),
}

impl From<std::io::Error> for RunError {
    fn from(e: std::io::Error) -> Self {
        Self::Store(e)
    }
}
impl From<EvaluationError> for RunError {
    fn from(e: EvaluationError) -> Self {
        Self::Ledger(e)
    }
}

fn to_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec_pretty(value).expect("state serializes")
}

/// One slot's ledger, run state and store. The method's requests arrive here
/// one at a time; the process around it lives in `run`.
pub struct Session<'a> {
    pub plan: &'a TrialPlan,
    pub slot_id: &'a str,
    pub store: Store,
    pub ledger: ExperimentLedger,
    pub state: RunState,
}

impl<'a> Session<'a> {
    pub fn scope(&self) -> ExperimentScope<'a> {
        ExperimentScope {
            plan: self.plan,
            slot_id: self.slot_id,
        }
    }
    pub fn save(&self) -> std::io::Result<()> {
        // Run state first: a slot with any state file is never started again.
        self.store.write("run.json", &to_bytes(&self.state))?;
        self.store.write("ledger.json", &to_bytes(&self.ledger))
    }

    /// One method request. Refusals are replies, not ledger records.
    pub fn handle(&mut self, line: &str) -> Result<Value, RunError> {
        if self.state.sealed {
            return Ok(json!({"ok": false, "refused": "the execution is sealed"}));
        }
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            return Ok(json!({"ok": false, "refused": "request is not JSON"}));
        };
        match req["op"].as_str() {
            Some("instances") => {
                let remaining = self.ledger.assess(self.scope())?.remaining_steps;
                let list: Vec<Value> = self
                    .plan
                    .development_instances
                    .iter()
                    .filter_map(|r| {
                        let inst = refs::resolve_instance(r)?;
                        Some(json!({
                            "id": r.id, "max_steps": inst.family.max_steps(), "world": inst,
                        }))
                    })
                    .collect();
                Ok(json!({"ok": true, "instances": list, "remaining_steps": remaining}))
            }
            Some("experiment") => self.experiment(&req),
            Some("submit") => self.submit(&req),
            _ => Ok(json!({"ok": false, "refused": "unknown op"})),
        }
    }

    fn design_bytes(req: &Value) -> (Result<Design, String>, Vec<u8>) {
        let raw = serde_json::to_vec(&req["design"]).unwrap_or_default();
        match serde_json::from_value::<Design>(req["design"].clone()) {
            Ok(d) => {
                let normalized = d.canonical();
                (Ok(d), normalized)
            }
            Err(e) => (Err(format!("design does not parse: {e}")), raw),
        }
    }

    fn experiment(&mut self, req: &Value) -> Result<Value, RunError> {
        let Some(instance) = self
            .plan
            .development_instances
            .iter()
            .find(|r| Some(r.id.as_str()) == req["instance"].as_str())
            .cloned()
        else {
            return Ok(json!({"ok": false, "refused": "not a public instance of this plan"}));
        };
        let Some(world) = refs::resolve_instance(&instance) else {
            return Ok(json!({"ok": false, "refused": "instance does not resolve"}));
        };
        let (design, bytes) = Self::design_bytes(req);
        let schema = if design.is_ok() {
            "lab-design/1"
        } else {
            "lab-design-raw/1"
        };
        let design_ref = refs::content(schema, "submission", "1", &bytes);
        let limit = world.family.max_steps() as u64;
        let call = match self
            .ledger
            .reserve(self.scope(), instance, design_ref.clone(), limit)
        {
            Ok(call) => call,
            Err(e) => return Ok(json!({"ok": false, "refused": format!("{e:?}")})),
        };
        // The pending call is durable before the evaluator runs.
        self.store.put_artifact(&design_ref, &bytes)?;
        self.save()?;
        let (reply, settlement) = match design {
            Err(reason) => {
                let result = json!({"outcome": "invalid", "reason": reason});
                self.returned(result, 0)?
            }
            Ok(d) => match score::experiment(&d, world.family, world.seed, limit) {
                Err(refusal) => {
                    // Not expected after a valid reservation; recorded as known
                    // zero-step failure rather than dropped.
                    let reason = format!("evaluator refused: {refusal:?}");
                    (
                        json!({"outcome": "failed", "reason": reason}),
                        ExperimentSettlement {
                            outcome: ExperimentOutcome::Failed { reason },
                            simulation_steps: ResourceUsage::Measured { value: 0 },
                        },
                    )
                }
                Ok(run) => {
                    let r = run.receipt;
                    let result = json!({
                        "outcome": r.outcome, "metrics": r.metrics, "reason": r.reason,
                        "trace_sha256": r.trace_sha256, "steps": r.simulation_steps,
                    });
                    match (r.outcome, r.simulation_steps) {
                        (Outcome::Unknown, _) | (_, None) => {
                            let reason = r
                                .reason
                                .unwrap_or_else(|| "evaluator result unknown".into());
                            (
                                result,
                                ExperimentSettlement {
                                    outcome: ExperimentOutcome::Unknown {
                                        reason: reason.clone(),
                                    },
                                    simulation_steps: ResourceUsage::Unknown { reason },
                                },
                            )
                        }
                        (Outcome::Failed, Some(steps)) => {
                            let reason = r.reason.unwrap_or_else(|| "simulation failed".into());
                            (
                                result,
                                ExperimentSettlement {
                                    outcome: ExperimentOutcome::Failed { reason },
                                    simulation_steps: ResourceUsage::Measured {
                                        value: steps as u64,
                                    },
                                },
                            )
                        }
                        (_, Some(steps)) => self.returned(result, steps as u64)?,
                    }
                }
            },
        };
        self.ledger.settle(self.scope(), &call, settlement)?;
        self.save()?;
        let remaining = self.ledger.assess(self.scope())?.remaining_steps;
        let mut reply = reply;
        reply["ok"] = true.into();
        reply["index"] = call.index.into();
        reply["remaining_steps"] = json!(remaining);
        Ok(reply)
    }

    /// Stores the result artifact and settles as returned with measured steps.
    fn returned(
        &self,
        result: Value,
        steps: u64,
    ) -> Result<(Value, ExperimentSettlement), RunError> {
        let bytes = genesis_core::digest::canonical_json(&result).into_bytes();
        let reference = refs::content("lab-experiment-result/1", "result", "1", &bytes);
        self.store.put_artifact(&reference, &bytes)?;
        Ok((
            result,
            ExperimentSettlement {
                outcome: ExperimentOutcome::Returned {
                    observed_evaluator: refs::evaluator(),
                    result: Box::new(reference),
                },
                simulation_steps: ResourceUsage::Measured { value: steps },
            },
        ))
    }

    /// v1 retains at most one output per slot; it is the normalized design.
    fn submit(&mut self, req: &Value) -> Result<Value, RunError> {
        if self.state.submission.is_some() {
            return Ok(json!({"ok": false, "refused": "an output was already submitted"}));
        }
        let (design, bytes) = Self::design_bytes(req);
        if let Err(reason) = design {
            return Ok(json!({"ok": false, "refused": reason}));
        }
        let reference = refs::content("lab-design/1", "output", "1", &bytes);
        self.store.put_artifact(&reference, &bytes)?;
        self.state.submission = Some(reference.clone());
        self.save()?;
        Ok(json!({"ok": true, "output": reference.sha256}))
    }
}

/// The receipt and the paths of the frozen transcript and receipt.
pub struct Finished {
    pub receipt: ExecutionReceipt,
    pub transcript_path: PathBuf,
}

fn usage(bounds_model_use: bool, wall: ResourceUsage, steps: ResourceUsage) -> Usage {
    let model = |what: &str| {
        if bounds_model_use {
            ResourceUsage::Measured { value: 0 }
        } else {
            ResourceUsage::Unknown {
                reason: format!("{what}: the sandbox doesn't establish that no model or local service is reachable"),
            }
        }
    };
    Usage {
        model_calls: model("model calls"),
        input_tokens: model("input tokens"),
        output_tokens: model("output tokens"),
        wall_time_ms: wall,
        simulation_steps: steps,
    }
}

/// Seals the ledger, freezes the transcript and writes the #18 receipt.
/// `environment` is the actual executor environment; `bounds_model_use` says
/// whether model calls and tokens can be recorded as measured zero.
pub fn finish(
    slot: &mut Session<'_>,
    environment: ExactContentRef,
    bounds_model_use: bool,
    outcome: ExecutionOutcome,
    wall: ResourceUsage,
) -> Result<Finished, RunError> {
    slot.ledger.seal(slot.scope())?;
    slot.state.sealed = true;
    let transcript = slot.ledger.transcript_json(slot.scope())?;
    let transcript_ref = slot.ledger.reference(slot.scope())?;
    slot.store.write("transcript.json", transcript.as_bytes())?;
    let binding = slot.plan.expected_binding(slot.slot_id)?;
    let accounting = slot.ledger.assess(slot.scope())?;
    let receipt = ExecutionReceipt {
        binding_sha256: binding.canonical_hash().map_err(EvaluationError::from)?,
        actual_method: slot
            .state
            .method
            .clone()
            .ok_or_else(|| RunError::Refused("no verified method identity".into()))?,
        actual_environment: environment,
        outcome,
        outputs: slot.state.submission.iter().cloned().collect(),
        transcript: transcript_ref,
        usage: usage(bounds_model_use, wall, accounting.simulation_steps),
        binding,
    };
    // The sealed ledger and run state are durable before the receipt exists:
    // a failure here leaves a slot that `recover` can finalize, never a
    // receipt whose ledger on disk can't validate it.
    slot.save()?;
    slot.store.write("receipt.json", &to_bytes(&receipt))?;
    slot.state.finished = true;
    slot.save()?;
    Ok(Finished {
        receipt,
        transcript_path: slot.store.path("transcript.json"),
    })
}
