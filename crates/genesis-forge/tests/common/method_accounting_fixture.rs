use genesis_core::development_evaluation::*;
use genesis_core::development_method::*;
use genesis_core::development_trial::*;
use genesis_forge::method_accounting::*;
use genesis_forge::method_synthesis::*;
use genesis_forge::text_generation::*;
use serde_json::json;
#[path = "method_plan.rs"]
mod plan_fixture;

pub fn generation(id: &str, source: bool) -> MethodSynthesisSession {
    let mut plan = plan_fixture::plan();
    plan.operation_id = id.into();
    let mut session = MethodSynthesisSession::new(plan).unwrap();
    let request = session.begin().unwrap();
    let mut completion = TextCompletion::interrupted(&request).unwrap();
    completion.transport = GenerationTransport::LoopbackFixture;
    completion.actual_model = Some(request.settings.model.clone());
    completion.message_id = Some("msg_fixture".into());
    completion.response_sha256 = Some("a".repeat(64));
    completion.outcome = TextOutcome::Returned {
        text: if source {
            r#"{"source":"print(1)"}"#
        } else {
            "invalid source"
        }
        .into(),
    };
    let one = ResourceUsage::Measured { value: 1 };
    completion.usage = GenerationUsage {
        model_calls: one.clone(),
        input_tokens: one.clone(),
        output_tokens: one.clone(),
        wall_time_ms: one,
    };
    session.complete(completion).unwrap();
    session
}
pub fn scope(generations: &[MethodSynthesisSession], trial: Option<&TrialPlan>) -> CostScope {
    CostScope {
        id: "cost-test".into(),
        generations: generations
            .iter()
            .map(|s| GenerationExpectation {
                org_id: s.plan().org_id.clone(),
                operation_id: s.plan().operation_id.clone(),
                plan_sha256: s.plan().sha256().unwrap(),
                request_sha256: s.request_sha256().into(),
                checkpoint_sha256: s.checkpoint_sha256().into(),
            })
            .collect(),
        trial: trial.map(|p| TrialExpectation {
            plan: p.reference().unwrap(),
            slots: p.slots.iter().map(|s| s.slot_id.clone()).collect(),
        }),
        generated_links: vec![],
        total_grant: ResourceGrant {
            model_calls: 10,
            input_tokens: 100,
            output_tokens: 100,
            wall_time_ms: 1000,
            simulation_steps: 1000,
        },
    }
}
pub fn trial(generation: &MethodSynthesisSession) -> TrialPlan {
    let reference = |id| plan_fixture::reference(id, id.as_bytes());
    let grant = json!({"model_calls":1,"input_tokens":10,"output_tokens":10,"wall_time_ms":100,"simulation_steps":100});
    let slots: Vec<_> = ["candidate", "control"].into_iter().map(|id| json!({
        "slot_id":id,"scoring_request_id":id,"org_id":"org_b","run_id":id,"attempt_id":id,
        "method":generation.package().unwrap().reference().unwrap(),"dependencies":generation.plan().dependencies,
        "purpose":{"kind":"acceptance","role":id}})).collect();
    serde_json::from_value(json!({"id":"trial","version":"1","criterion":reference("criterion"),
        "development_instances":[reference("instance")],"scoring_instances":{"kind":"development"},
        "context":{"public_task":reference("different-public-task"),"evaluator":reference("evaluator"),"environment":reference("scoring-env"),
        "generator":reference("generator"),"instance_set":reference("instances"),"design_schema":reference("design"),"metric_schema":reference("metrics"),"scoring_rule":reference("rule")},
        "model_settings":reference("model"),"development_environment":reference("exec-env"),"development_grant":grant,"scoring_grant":grant,"slots":slots})).unwrap()
}
pub fn evidence(p: &TrialPlan, slot: &str) -> TrialEvidence {
    let binding = p.expected_binding(slot).unwrap();
    let one = json!({"kind":"measured","value":1});
    let execution: ExecutionReceipt = serde_json::from_value(json!({"binding":binding,"binding_sha256":binding.canonical_hash().unwrap(),
        "actual_method":binding.method,"actual_environment":binding.environment,"outcome":"output_generated",
        "outputs":[plan_fixture::reference("output",b"output")],"transcript":plan_fixture::reference("trace",b"trace"),
        "usage":{"model_calls":one,"input_tokens":one,"output_tokens":one,"wall_time_ms":one,"simulation_steps":one}})).unwrap();
    let request = p
        .evaluation_request_for_accounting(slot, &execution)
        .unwrap()
        .unwrap();
    let scoring = EvaluationReceipt {
        request_sha256: request.identity_hash(&execution, &binding).unwrap(),
        observed_context: Some(request.context.clone()),
        request,
        outcome: EvaluationOutcome::Scored {
            metrics: plan_fixture::reference("metrics", b"metrics"),
        },
        trace: plan_fixture::reference("score-trace", b"score-trace"),
        usage: execution.usage.clone(),
    };
    TrialEvidence {
        slot_id: slot.into(),
        execution,
        scoring: Some(scoring),
    }
}
