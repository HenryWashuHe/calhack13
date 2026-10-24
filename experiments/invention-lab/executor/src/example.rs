//! An example acceptance plan for exercising the executor end to end. Every
//! reference below except the methods, instances, evaluator and environment
//! names a small descriptor written here; none of it is a real organization's
//! policy. Both methods are hand-written **controls**.
use crate::refs::{self, content};
use crate::sandbox::Mode;
use genesis_core::development_evaluation::ScoringContext;
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
    let task = descriptor(
        "lab-task/1",
        family.name(),
        serde_json::json!({"family": family.name(), "version": genesis_lab_eval::family::FAMILY_VERSION}),
    );
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
        context: ScoringContext {
            public_task: task,
            evaluator: refs::evaluator(),
            environment: descriptor(
                "lab-scoring-env/1",
                "in-process",
                serde_json::json!({"scoring": "in-process primitives"}),
            ),
            generator: descriptor(
                "lab-generator/1",
                family.name(),
                serde_json::json!({"family": family.name(), "version": genesis_lab_eval::family::FAMILY_VERSION}),
            ),
            instance_set: descriptor(
                "lab-instance-set/1",
                "development",
                serde_json::json!({"instances": ids}),
            ),
            design_schema: descriptor(
                "lab-design-schema/1",
                "language-1",
                serde_json::json!({"language": 1}),
            ),
            metric_schema: descriptor(
                "lab-metric-schema/1",
                "v1",
                serde_json::json!({"metrics": ["success", "progress", "cost", "joints_broken", "steps_to_target"]}),
            ),
            scoring_rule: descriptor(
                "lab-scoring-rule/1",
                family.name(),
                serde_json::json!({"success": "payload held on the far bank for 1 s"}),
            ),
        },
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
