//! Test-only generator of inert synthetic fresh-panel records for
//! `tools/test_pc1_report.py`. Nothing executes and nothing here is evidence.
//!
//!     pc1_fixture <config.json>
//!
//! The config names a retained template trial, an output trial directory that
//! already holds the expected `plan.json`, and per-slot synthetic results:
//! `{"template": dir, "out": dir, "slots": {"candidate": {"successes": n,
//! "outcome"?: "failed", "unknown_development"?: ["model_calls"],
//! "scoring"?: "scored"|"unknown"|"absent", "scoring_usage"?: {"field": n|null}}}}`.
//! The template's receipts are rebound to that plan the same way
//! `genesis-core/tests/process_slot_assessment.rs` rebinds them, so the
//! records carry real typed identities and the real assessor reads them.
use genesis_core::{
    development_evaluation::{EvaluationOutcome, EvaluationReceipt},
    development_method::ExecutionReceipt,
    development_trial::TrialPlan,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn usage_value(v: &Value) -> Value {
    match v {
        Value::Null => json!({"kind": "unknown", "reason": "synthetic"}),
        n => json!({"kind": "measured", "value": n}),
    }
}

fn main() {
    let config = read(Path::new(&std::env::args().nth(1).expect("config path")));
    let template = Path::new(config["template"].as_str().unwrap());
    let out = Path::new(config["out"].as_str().unwrap());
    let plan: TrialPlan = serde_json::from_value(read(&out.join("plan.json"))).unwrap();
    for (slot, spec) in config["slots"].as_object().unwrap() {
        let mut execution = read(&template.join(slot).join("receipt.json"));
        if let Some(outcome) = spec.get("outcome") {
            execution["outcome"] = outcome.clone();
        }
        for field in spec["unknown_development"].as_array().into_iter().flatten() {
            execution["usage"][field.as_str().unwrap()] = usage_value(&Value::Null);
        }
        let mut execution: ExecutionReceipt = serde_json::from_value(execution).unwrap();
        let binding = plan.expected_binding(slot).unwrap();
        execution.binding = binding.clone();
        execution.binding_sha256 = binding.canonical_hash().unwrap();
        execution.actual_method = binding.method.clone();
        execution.actual_environment = binding.environment.clone();
        let dir = out.join(slot);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("receipt.json"),
            serde_json::to_vec(&execution).unwrap(),
        )
        .unwrap();

        let kind = spec["scoring"].as_str().unwrap_or("scored");
        if kind == "absent" {
            continue;
        }
        let successes = spec["successes"].as_u64().unwrap();
        let rows: Vec<Value> = (plan.development_instances.iter().enumerate())
            .map(|(i, r)| json!({"instance": r.id, "success": (i as u64) < successes, "progress": 1.0}))
            .collect();
        let metrics = serde_json::to_vec(&json!({ "instances": rows })).unwrap();
        let digest = format!("{:x}", Sha256::digest(&metrics));
        let score = out.join(format!("{slot}-score"));
        let mut scoring = read(&template.join(format!("{slot}-score/evaluation.json")));
        for (field, value) in spec["scoring_usage"].as_object().into_iter().flatten() {
            scoring["usage"][field] = usage_value(value);
        }
        let mut scoring: EvaluationReceipt = serde_json::from_value(scoring).unwrap();
        let request = plan
            .evaluation_request_for_accounting(slot, &execution)
            .unwrap()
            .unwrap();
        scoring.request_sha256 = request.identity_hash(&execution, &binding).unwrap();
        scoring.observed_context = Some(request.context.clone());
        scoring.request = request;
        match (&mut scoring.outcome, kind) {
            (EvaluationOutcome::Scored { metrics }, "scored") => metrics.sha256 = digest.clone(),
            (_, "unknown") => {
                scoring.outcome = EvaluationOutcome::Unknown {
                    reason: "synthetic".into(),
                }
            }
            other => panic!("unsupported scoring fixture {other:?}"),
        }
        fs::create_dir_all(score.join("artifacts")).unwrap();
        fs::write(score.join("artifacts").join(&digest), &metrics).unwrap();
        fs::write(
            score.join("evaluation.json"),
            serde_json::to_vec(&scoring).unwrap(),
        )
        .unwrap();
    }
}
