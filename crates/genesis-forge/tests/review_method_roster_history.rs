//! Independent review of roster 390a867: retained historical prompt identity.
use genesis_core::development_method::ResourceGrant;
use genesis_forge::method_roster::GenerationRoster;
use genesis_forge::method_synthesis::{
    MethodSynthesisPhase, MethodSynthesisPlan, MethodSynthesisSession,
};
use genesis_forge::text_generation::{
    TextCompletion, TextFailure, TextGenerationRequest, TextOutcome,
};
use serde_json::{json, Value};
#[path = "common/method_plan.rs"]
mod fixture;

// Simulate a valid checkpoint produced by a prior prompt builder. As in the
// existing method_generation test, this checksum is not writer authentication.
fn historical_prepared() -> MethodSynthesisSession {
    let mut saved =
        serde_json::to_value(MethodSynthesisSession::new(fixture::plan()).unwrap()).unwrap();
    saved["request"]["system"] = json!("Historical prompt retained by contract version 1.");
    let request: TextGenerationRequest = serde_json::from_value(saved["request"].clone()).unwrap();
    saved["request_sha256"] = json!(request.sha256().unwrap());
    let plan: MethodSynthesisPlan = serde_json::from_value(saved["plan"].clone()).unwrap();
    saved["checkpoint_sha256"] = json!(fixture::hash(
        &serde_json::to_vec(&(
            "genesis-method-generation-checkpoint/1",
            1u32,
            plan,
            request,
            saved["request_sha256"].as_str().unwrap(),
            saved["output_contract"].as_str().unwrap(),
            MethodSynthesisPhase::Prepared,
            None::<TextCompletion>,
        ))
        .unwrap()
    ));
    serde_json::from_value(saved).unwrap()
}

#[test]
fn initial_identity_survives_historical_prompt_and_every_later_phase() {
    let initial = historical_prepared();
    assert_ne!(
        initial.request_sha256(),
        initial.plan().request().unwrap().sha256().unwrap()
    );
    let roster = GenerationRoster::freeze(
        "historical".into(),
        initial.plan().settings.clone(),
        ResourceGrant {
            model_calls: 1,
            input_tokens: 100,
            output_tokens: u64::from(initial.plan().settings.max_tokens),
            wall_time_ms: 1000,
            simulation_steps: 0,
        },
        std::slice::from_ref(&initial),
    )
    .unwrap();
    let mut pending = initial.clone();
    let request = pending.begin().unwrap();
    let mut snapshots = vec![initial.clone(), pending.clone()];
    for outcome in [
        TextOutcome::Unknown {
            reason: TextFailure::Interrupted,
        },
        TextOutcome::Failed {
            reason: TextFailure::Http,
        },
        TextOutcome::Returned {
            text: json!({"source":"print(1)"}).to_string(),
        },
        TextOutcome::Returned {
            text: json!({"source":""}).to_string(),
        },
    ] {
        let mut completion = TextCompletion::interrupted(&request).unwrap();
        if matches!(&outcome, TextOutcome::Returned { .. }) {
            completion.transport =
                genesis_forge::text_generation::GenerationTransport::LoopbackFixture;
            completion.actual_model = Some(request.settings.model.clone());
            completion.message_id = Some("msg_historical_fixture".into());
            completion.response_sha256 = Some("a".repeat(64));
            completion.usage.model_calls =
                genesis_core::development_method::ResourceUsage::Measured { value: 1 };
        }
        completion.outcome = outcome;
        let mut advanced = pending.clone();
        advanced.complete(completion).unwrap();
        snapshots.push(advanced);
    }
    for snapshot in snapshots {
        let before: Value = serde_json::to_value(&snapshot).unwrap();
        let restored: MethodSynthesisSession = serde_json::from_value(before.clone()).unwrap();
        assert_eq!(
            restored.prepared_checkpoint_sha256().unwrap(),
            initial.checkpoint_sha256()
        );
        let report = roster.assess(std::slice::from_ref(&restored)).unwrap();
        assert_eq!(
            report.costs.rows[0].evidence_sha256.as_deref(),
            Some(restored.checkpoint_sha256())
        );
        assert_eq!(serde_json::to_value(&restored).unwrap(), before);
    }
}
