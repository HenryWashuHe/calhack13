//! Independent PR102 regressions using synthetic retained records only.
//! No method, executor, scorer, simulator, namespace probe or provider runs.
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_experiment::{
    ExperimentLedger, ExperimentOutcome, ExperimentScope, ExperimentSettlement,
};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage, Usage,
};
use genesis_core::development_trial::{ScoringInstances, TrialPlan, TrialPurpose};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::feedback::receiver_feedback;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const PUBLIC_INPUT_LIMIT: usize = 128 * 1024;
const HIDDEN_CANARY: &str = "PRIVATE_COMPARISON_INSTANCE_CANARY";
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Retained {
    dir: PathBuf,
    plan: TrialPlan,
    state: Store,
    score: Store,
}
impl Drop for Retained {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn usage() -> Usage {
    Usage {
        model_calls: ResourceUsage::Measured { value: 0 },
        input_tokens: ResourceUsage::Measured { value: 0 },
        output_tokens: ResourceUsage::Measured { value: 0 },
        wall_time_ms: ResourceUsage::Measured { value: 0 },
        simulation_steps: ResourceUsage::Measured { value: 0 },
    }
}

fn artifact(
    store: &Store,
    schema: &str,
    value: &Value,
) -> genesis_core::development_method::ExactContentRef {
    let bytes = genesis_core::digest::canonical_json(value).into_bytes();
    let reference = refs::content(schema, schema, "1", &bytes);
    store.put_artifact(&reference, &bytes).unwrap();
    reference
}

fn retained(hidden: bool, large_public_experiment: bool) -> Retained {
    let dir = std::env::temp_dir().join(format!(
        "review-feedback-public-{}-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let state = Store::open(&dir.join("state")).unwrap();
    let score = Store::open(&dir.join("score")).unwrap();
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "candidate", "1", b"fixture candidate"),
        control: refs::content("lab-method/1", "control", "1", b"fixture control"),
    };
    let mut plan = example::acceptance_plan(
        &methods,
        Mode::None,
        "synthetic environment descriptor; never invoked",
        1,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 1000,
            simulation_steps: 2400,
        },
    );
    if hidden {
        plan.scoring_instances = ScoringInstances::HiddenCommitment {
            protocol: refs::content("fixture-commitment/1", "protocol", "1", b"opaque protocol"),
        };
        plan.context.instance_set = refs::content(
            "fixture-commitment/1",
            "hidden-set",
            "1",
            b"opaque commitment",
        );
        for slot in &mut plan.slots {
            slot.purpose = TrialPurpose::Comparison {
                trial_id: "comparison-one".into(),
                arm_id: slot.slot_id.clone(),
            };
        }
    }
    plan.validate().unwrap();
    let scope = ExperimentScope {
        plan: &plan,
        slot_id: "candidate",
    };
    let mut ledger = ExperimentLedger::new(scope).unwrap();
    let binding = plan.expected_binding("candidate").unwrap();
    if large_public_experiment {
        // This invalid design fits the executor's 1 MiB request-line limit.
        // Such rejected experiments legitimately retain their raw public input.
        let raw_design = json!({"public_note": "x".repeat(PUBLIC_INPUT_LIMIT)});
        assert!(
            serde_json::to_vec(&json!({
                "op": "experiment", "instance": plan.development_instances[0].id,
                "design": raw_design,
            }))
            .unwrap()
            .len()
                < genesis_lab_exec::run::MAX_LINE
        );
        let design = artifact(&state, "lab-design-raw/1", &raw_design);
        let result = artifact(
            &state,
            "lab-experiment-result/1",
            &json!({
                "outcome": "invalid", "reason": "design does not parse",
            }),
        );
        let call = ledger
            .reserve(scope, plan.development_instances[0].clone(), design, 1)
            .unwrap();
        ledger
            .settle(
                scope,
                &call,
                ExperimentSettlement {
                    outcome: ExperimentOutcome::Returned {
                        observed_evaluator: binding.evaluator.clone(),
                        result: Box::new(result),
                    },
                    simulation_steps: ResourceUsage::Measured { value: 0 },
                },
            )
            .unwrap();
    }
    ledger.seal(scope).unwrap();
    let submitted: Value = serde_json::from_str(include_str!(
        "../../evaluator/designs/reference-five-wheel.json"
    ))
    .unwrap();
    let execution = ExecutionReceipt {
        binding_sha256: binding.canonical_hash().unwrap(),
        actual_method: binding.method.clone(),
        actual_environment: binding.environment.clone(),
        binding: binding.clone(),
        outcome: ExecutionOutcome::OutputGenerated,
        outputs: vec![artifact(&state, "lab-design/1", &submitted)],
        transcript: ledger.reference(scope).unwrap(),
        usage: usage(),
    };
    ledger.validate_execution(scope, &execution).unwrap();
    let request = plan
        .evaluation_request("candidate", &execution)
        .unwrap()
        .unwrap();
    let instance = if hidden {
        HIDDEN_CANARY
    } else {
        plan.development_instances[0].id.as_str()
    };
    let evaluation = EvaluationReceipt {
        request_sha256: request.canonical_hash(&execution, &binding).unwrap(),
        observed_context: Some(request.context.clone()),
        request: request.clone(),
        outcome: EvaluationOutcome::Scored {
            metrics: artifact(
                &score,
                "lab-scoring-metrics/1",
                &json!({
                    "success_count": 0, "of": 1,
                    "instances": [{"instance": instance, "success": false, "progress": 0.5, "cost": 1}],
                }),
            ),
        },
        trace: artifact(&score, "lab-scoring-trace/1", &json!({"synthetic": true})),
        usage: usage(),
    };
    evaluation
        .validate_against(&request, &execution, &binding)
        .unwrap();
    state
        .write("ledger.json", &serde_json::to_vec(&ledger).unwrap())
        .unwrap();
    state
        .write("receipt.json", &serde_json::to_vec(&execution).unwrap())
        .unwrap();
    score
        .write("evaluation.json", &serde_json::to_vec(&evaluation).unwrap())
        .unwrap();
    Retained {
        dir,
        plan,
        state,
        score,
    }
}

#[test]
fn public_feedback_refuses_valid_hidden_comparison_records() {
    let f = retained(true, false);
    let result = receiver_feedback(&f.plan, "candidate", &f.state, &f.score);
    let exposed = result
        .as_ref()
        .is_ok_and(|(_, bytes)| String::from_utf8_lossy(bytes).contains(HIDDEN_CANARY));
    assert!(
        result.is_err(),
        "hidden comparison accepted as public feedback; hidden canary exported: {exposed}"
    );
}

#[test]
fn public_feedback_fits_the_revision_public_document_byte_limit_or_refuses() {
    let f = retained(false, true);
    if let Ok((_, bytes)) = receiver_feedback(&f.plan, "candidate", &f.state, &f.score) {
        assert!(
            bytes.len() <= PUBLIC_INPUT_LIMIT,
            "{}-byte feedback cannot be a revision public document (limit {PUBLIC_INPUT_LIMIT})",
            bytes.len()
        );
    }
}

#[test]
fn exact_public_records_still_produce_bound_deterministic_feedback() {
    let f = retained(false, false);
    let (reference, bytes) = receiver_feedback(&f.plan, "candidate", &f.state, &f.score).unwrap();
    assert_eq!(reference.sha256, refs::sha256_hex(&bytes));
    assert!(bytes.len() < PUBLIC_INPUT_LIMIT);
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        doc["plan"],
        serde_json::to_value(f.plan.reference().unwrap()).unwrap()
    );
    assert_eq!(
        doc["public_instances"],
        json!([f.plan.development_instances[0].id])
    );
    assert_eq!(
        (reference, bytes),
        receiver_feedback(&f.plan, "candidate", &f.state, &f.score).unwrap()
    );
}
