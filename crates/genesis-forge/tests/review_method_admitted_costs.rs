//! Independent historical-identity and failed-cost regression for admitted costs.
use genesis_core::development_method::{ResourceGrant, ResourceUsage};
use genesis_forge::{
    method_accounting::{CostObservation, CostScope, GenerationExpectation},
    method_admission::GenerationAdmission,
    method_admitted_costs::assess_admitted_costs,
    method_roster::GenerationRoster,
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisPlan, MethodSynthesisSession},
    text_generation::{
        GenerationTransport, TextCompletion, TextFailure, TextGenerationRequest, TextOutcome,
    },
};
use serde_json::json;
#[path = "common/method_plan.rs"]
mod fixture;

// Model a valid retained checkpoint from an older prompt builder, not a new
// request reconstructed using today's builder. The checksum is not authentication.
fn historical_prepared() -> MethodSynthesisSession {
    let mut value =
        serde_json::to_value(MethodSynthesisSession::new(fixture::plan()).unwrap()).unwrap();
    value["request"]["system"] = json!("Retained historical method-generation prompt.");
    let request: TextGenerationRequest = serde_json::from_value(value["request"].clone()).unwrap();
    value["request_sha256"] = json!(request.sha256().unwrap());
    let plan: MethodSynthesisPlan = serde_json::from_value(value["plan"].clone()).unwrap();
    value["checkpoint_sha256"] = json!(fixture::hash(
        &serde_json::to_vec(&(
            "genesis-method-generation-checkpoint/1",
            1u32,
            plan,
            request,
            value["request_sha256"].as_str().unwrap(),
            value["output_contract"].as_str().unwrap(),
            MethodSynthesisPhase::Prepared,
            None::<TextCompletion>,
        ))
        .unwrap()
    ));
    serde_json::from_value(value).unwrap()
}

#[test]
fn historical_failed_attempt_keeps_unknowns_and_original_grant_overrun() {
    let prepared = historical_prepared();
    let reconstructed = MethodSynthesisSession::new(prepared.plan().clone()).unwrap();
    assert_ne!(prepared.request_sha256(), reconstructed.request_sha256());
    let generation_grant = ResourceGrant {
        model_calls: 1,
        input_tokens: 0,
        output_tokens: u64::from(prepared.plan().settings.max_tokens),
        wall_time_ms: 1,
        simulation_steps: 0,
    };
    let roster = GenerationRoster::freeze(
        "historical-stage".into(),
        prepared.plan().settings.clone(),
        generation_grant,
        std::slice::from_ref(&prepared),
    )
    .unwrap();
    let mut admission = GenerationAdmission::new(roster).unwrap();
    admission.reserve(&prepared).unwrap();
    let scope = CostScope {
        id: "historical-cost-scope".into(),
        generations: vec![GenerationExpectation {
            org_id: prepared.plan().org_id.clone(),
            operation_id: prepared.plan().operation_id.clone(),
            plan_sha256: prepared.plan().sha256().unwrap(),
            request_sha256: prepared.request_sha256().into(),
            checkpoint_sha256: prepared.checkpoint_sha256().into(),
        }],
        trial: None,
        generated_links: vec![],
        total_grant: ResourceGrant {
            model_calls: 10,
            input_tokens: 100,
            output_tokens: 1000,
            wall_time_ms: 1000,
            simulation_steps: 1000,
        },
    };
    let mut failed = prepared;
    let request = failed.begin().unwrap();
    let mut completion = TextCompletion::interrupted(&request).unwrap();
    // Synthetic retained host evidence; no provider call or live measurement.
    completion.transport = GenerationTransport::LoopbackFixture;
    completion.outcome = TextOutcome::Failed {
        reason: TextFailure::Http,
    };
    completion.usage.model_calls = ResourceUsage::Measured { value: 1 };
    completion.usage.input_tokens = ResourceUsage::Measured { value: 7 };
    failed.complete(completion).unwrap();
    let admission: GenerationAdmission =
        serde_json::from_slice(&serde_json::to_vec(&admission).unwrap()).unwrap();
    let failed: MethodSynthesisSession =
        serde_json::from_slice(&serde_json::to_vec(&failed).unwrap()).unwrap();
    let before = serde_json::to_vec(&(&admission, &scope, &failed)).unwrap();
    let report =
        assess_admitted_costs(&admission, &scope, std::slice::from_ref(&failed), None).unwrap();
    assert_eq!(report.costs.rows.len(), 1);
    assert!(matches!(
        report.costs.rows[0].observation,
        CostObservation::Generation {
            phase: MethodSynthesisPhase::Failed,
            transport: Some(GenerationTransport::LoopbackFixture),
        }
    ));
    assert_eq!(
        report.costs.rows[0].evidence_sha256.as_deref(),
        Some(failed.checkpoint_sha256())
    );
    assert_eq!(report.costs.combined[0].known_lower_bound, 1);
    assert_eq!(report.costs.combined[1].known_lower_bound, 7);
    assert!(report.costs.combined[2].unknown);
    assert!(report.costs.combined[3].unknown);
    assert!(report.generation.costs.combined[1].limit_exceeded);
    assert!(!report.costs.combined[1].limit_exceeded);
    assert_eq!(report.declared_scope_sha256, scope.sha256().unwrap());
    assert_ne!(report.declared_scope_sha256, report.costs.scope_sha256);
    assert_eq!(
        before,
        serde_json::to_vec(&(&admission, &scope, &failed)).unwrap()
    );
    assert!(assess_admitted_costs(&admission, &scope, &[reconstructed], None).is_err());
}
