//! An example acceptance plan for exercising the executor end to end. Every
//! reference below except the methods, instances, evaluator and environment
//! names a small descriptor written here; none of it is a real organization's
//! policy. Both methods are hand-written **controls**.
use crate::refs::{self, content};
use crate::sandbox::Mode;
use genesis_core::development_method::{AcceptanceRole, ExactContentRef, ResourceGrant};
use genesis_core::development_trial::{ScoringInstances, TrialPlan, TrialPurpose, TrialSlot};
use genesis_core::ids::{AttemptId, OrgId};
use genesis_lab_eval::family::Family;

fn descriptor(schema: &str, id: &str, value: serde_json::Value) -> ExactContentRef {
    content(
        schema,
        id,
        "1",
        genesis_core::digest::canonical_json(&value).as_bytes(),
    )
}

/// Exact bytes of a success-count criterion: accept when the candidate's
/// successes on exactly these instances exceed the control's by `min_margin`.
pub fn criterion_bytes(instances: &[String], min_margin: u64) -> Vec<u8> {
    genesis_core::digest::canonical_json(&serde_json::json!({
        "rule": "success-count-margin",
        "min_margin": min_margin,
        "instances": instances,
    }))
    .into_bytes()
}

pub struct ExampleMethods {
    pub candidate: ExactContentRef,
    pub control: ExactContentRef,
}

/// Ravine-crossing development seeds `0..instances`, one org, candidate and
/// control slots with equal grants.
pub fn acceptance_plan(
    methods: &ExampleMethods,
    sandbox: Mode,
    interpreter_version: &str,
    instances: u64,
    grant: ResourceGrant,
) -> TrialPlan {
    acceptance_plan_for(
        Family::RavineCrossing,
        methods,
        sandbox,
        interpreter_version,
        instances,
        grant,
    )
}

/// The same plan on any task family's development seeds `0..instances`, for
/// example a receiving organization checking another organization's method on
/// its own, different task.
pub fn acceptance_plan_for(
    family: Family,
    methods: &ExampleMethods,
    sandbox: Mode,
    interpreter_version: &str,
    instances: u64,
    grant: ResourceGrant,
) -> TrialPlan {
    let development_instances: Vec<_> = (0..instances)
        .map(|s| refs::instance_ref(family, s))
        .collect();
    let ids: Vec<_> = development_instances.iter().map(|r| r.id.clone()).collect();
    let org = OrgId::new("org_example_b");
    let slot = |id: &str, method: &ExactContentRef, role| TrialSlot {
        slot_id: id.into(),
        scoring_request_id: format!("score-{id}"),
        org_id: org.clone(),
        run_id: format!("run-{id}"),
        attempt_id: AttemptId::new(format!("att_{id}")),
        method: method.clone(),
        dependencies: vec![],
        purpose: TrialPurpose::Acceptance { role },
    };
    TrialPlan {
        id: format!("example-{}-acceptance", family.name()),
        version: "1".into(),
        criterion: content(
            "lab-criterion/1",
            "success-count-margin",
            "1",
            &criterion_bytes(&ids, 1),
        ),
        development_instances: development_instances.clone(),
        scoring_instances: ScoringInstances::Development {},
        context: refs::scoring_context(family, &ids),
        model_settings: descriptor(
            "lab-model-settings/1",
            "none",
            serde_json::json!({"models": "none"}),
        ),
        development_environment: refs::environment(sandbox, interpreter_version),
        development_grant: grant.clone(),
        scoring_grant: ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 600_000,
            simulation_steps: instances * family.max_steps() as u64,
        },
        slots: vec![
            slot("candidate", &methods.candidate, AcceptanceRole::Candidate),
            slot("control", &methods.control, AcceptanceRole::Control),
        ],
    }
}

/// A genesis-forge `MethodSynthesisPlan` (as JSON) for one method that develops
/// designs for `family` through this host's runtime and protocol: entrypoint
/// `method.py`, no dependencies, a task description as the public task and the
/// public design-language document as the one public input. Organization
/// `org_example_a`, target `generated-method@0.1.0`, provider-default sampling.
pub fn synthesis_plan(
    family: Family,
    model: &str,
    operation_id: &str,
    interpreter_version: &str,
) -> serde_json::Value {
    let doc = |schema: &str, id: &str, text: &str| serde_json::json!({"reference": content(schema, id, "1", text.as_bytes()), "text": text});
    let task = format!(
        "Task family {name} (version {version}) in the GENESIS invention lab. \
         A run gets public development instances of this family through the protocol. \
         Develop one design that meets the family's success rule on as many instances \
         as possible, using experiments within the run's remaining simulation steps, \
         and submit it. The submitted design is later scored by a separate trusted \
         evaluator on the receiving organization's instances.",
        name = family.name(),
        version = genesis_lab_eval::family::FAMILY_VERSION,
    );
    serde_json::json!({
        "operation_id": operation_id,
        "org_id": "org_example_a",
        "target_id": "generated-method",
        "target_version": "0.1.0",
        "runtime": refs::runtime(interpreter_version),
        "protocol": {"reference": refs::protocol(), "text": refs::PROTOCOL},
        "entrypoint": "method.py",
        "dependencies": [],
        "public_task": doc("lab-task-description/1", family.name(), &task),
        "public_inputs": [doc("lab-design-language/1", "language-1", include_str!("../DESIGN_LANGUAGE.md"))],
        "instruction": "Write one Python 3 program using only the standard library that implements the protocol exactly: read the instances, spend experiments within the remaining simulation steps to search for a design that succeeds on many instances, then submit the best design found and exit 0. Never write anything but protocol requests to standard output.",
        "settings": {"model": model, "max_tokens": 8192, "timeout_ms": 120000, "sampling": "provider_default"},
    })
}
