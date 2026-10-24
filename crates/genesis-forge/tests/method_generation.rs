use genesis_core::development_method::{
    DevelopmentRunBinding, ExecutionPurpose, ResourceGrant, ResourceUsage,
};
use genesis_core::development_package::MethodPackage;
use genesis_core::ids::{AttemptId, OrgId};
use genesis_forge::method_synthesis::{
    MethodSynthesisPhase, MethodSynthesisPlan, MethodSynthesisSession,
};
use genesis_forge::text_generation::{GenerationTransport, TextFailure, TextOutcome};
use genesis_forge::AnthropicGenerator;
use serde_json::{json, Value};
#[path = "common/method_plan.rs"]
mod plan_fixture;
use plan_fixture::{hash, plan, reference};
use std::time::Duration;
#[path = "common/text_generation_fixture.rs"]
mod fixture;
use fixture::{response, serve};

async fn finish(session: &mut MethodSynthesisSession, body: Value) -> Vec<u8> {
    let request = session.begin().unwrap();
    let (endpoint, task) = serve(serde_json::to_vec(&body).unwrap(), 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    session.complete(completion).unwrap();
    task.await.unwrap()
}

#[tokio::test]
async fn generation_exports_exact_package_and_pins_submitted_settings() {
    let source = "# café\r\nprint('candidate')\r\n";
    let mut session = MethodSynthesisSession::new(plan()).unwrap();
    let request = session.plan().request().unwrap();
    let wire = finish(
        &mut session,
        response(&json!({"source":source}).to_string()),
    )
    .await;
    assert_eq!(session.phase(), MethodSynthesisPhase::Generated);
    let boundary = wire.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    assert_eq!(&wire[boundary + 4..], request.wire_bytes().unwrap());
    assert!(String::from_utf8_lossy(&wire[..boundary]).contains("anthropic-version: 2023-06-01"));
    assert_eq!(
        session.completion().unwrap().wire_sha256,
        hash(&wire[boundary + 4..])
    );
    assert_eq!(
        session.completion().unwrap().usage.input_tokens,
        ResourceUsage::Measured { value: 100 }
    );
    let saved = serde_json::to_vec(&session).unwrap();
    let restored: MethodSynthesisSession = serde_json::from_slice(&saved).unwrap();
    let package = restored.package().unwrap();
    assert_eq!(package.files()[0].content, source);
    let binding = DevelopmentRunBinding {
        org_id: OrgId::from("org_b"),
        run_id: "run_1".into(),
        attempt_id: AttemptId::from("attempt_1"),
        method: package.reference().unwrap(),
        dependencies: plan().dependencies,
        public_task: plan().public_task.reference,
        evaluator: reference("evaluator", b"evaluator"),
        model_settings: reference("settings", b"settings"),
        environment: reference("environment", b"environment"),
        purpose: ExecutionPurpose::Ordinary {},
        budget: ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 1000,
            simulation_steps: 10,
        },
    };
    let bytes = package.artifact_json().unwrap();
    assert_eq!(
        MethodPackage::load_for(
            bytes.as_bytes(),
            &binding,
            &plan().runtime,
            &plan().protocol.reference
        )
        .unwrap(),
        package
    );
    let mut changed = bytes.into_bytes();
    changed.push(b' ');
    assert!(MethodPackage::load_for(
        &changed,
        &binding,
        &plan().runtime,
        &plan().protocol.reference
    )
    .is_err());
    let evidence: Value = serde_json::from_str(&restored.derivation_json().unwrap()).unwrap();
    assert_eq!(evidence["package"]["sha256"], binding.method.sha256);
}

#[tokio::test]
async fn malformed_source_retains_provider_usage_and_never_exports() {
    for text in [
        "not JSON",
        r#"{"source":"one","source":"two"}"#,
        r#"{"source":"code","runtime":"foreign"}"#,
        r#"{"source":""}"#,
        r#"{"source":"x\u0000y"}"#,
    ] {
        let mut session = MethodSynthesisSession::new(plan()).unwrap();
        finish(&mut session, response(text)).await;
        assert_eq!(session.phase(), MethodSynthesisPhase::RejectedSource);
        assert_eq!(
            session.completion().unwrap().usage.output_tokens,
            ResourceUsage::Measured { value: 32 }
        );
        assert!(session.package().is_err());
        assert!(session.begin().is_err());
        let restored: MethodSynthesisSession =
            serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
        assert_eq!(restored.phase(), MethodSynthesisPhase::RejectedSource);
    }
}

#[tokio::test]
async fn foreign_completion_and_tampered_checkpoints_cannot_change_pending_intent() {
    let mut a = MethodSynthesisSession::new(plan()).unwrap();
    finish(&mut a, response(&json!({"source":"print(1)"}).to_string())).await;
    let mut changed_plan = plan();
    changed_plan.operation_id = "another_operation".into();
    let mut b = MethodSynthesisSession::new(changed_plan).unwrap();
    b.begin().unwrap();
    let before = serde_json::to_vec(&b).unwrap();
    assert!(b.complete(a.completion().unwrap().clone()).is_err());
    assert_eq!(before, serde_json::to_vec(&b).unwrap());
    for field in ["target_id", "target_version", "entrypoint", "org_id"] {
        let mut value = serde_json::to_value(&a).unwrap();
        value["plan"][field] = json!("substituted");
        assert!(serde_json::from_value::<MethodSynthesisSession>(value).is_err());
    }
    let mut value = serde_json::to_value(&a).unwrap();
    value["plan"]["settings"]["max_tokens"] = json!(999);
    assert!(serde_json::from_value::<MethodSynthesisSession>(value).is_err());
    let mut value = serde_json::to_value(&a).unwrap();
    value["completion"]["usage"]["output_tokens"]["value"] = json!(0);
    assert!(serde_json::from_value::<MethodSynthesisSession>(value).is_err());
}

#[test]
fn interrupted_checkpoint_never_replays() {
    let mut session = MethodSynthesisSession::new(plan()).unwrap();
    session.begin().unwrap();
    let mut restored: MethodSynthesisSession =
        serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
    assert!(restored.recover_interrupted().unwrap());
    assert!(!restored.recover_interrupted().unwrap());
    assert_eq!(
        restored.completion().unwrap().transport,
        GenerationTransport::Unobserved
    );
    assert!(restored.begin().is_err());
    assert_eq!(
        restored.completion().unwrap().outcome,
        TextOutcome::Unknown {
            reason: TextFailure::Interrupted
        }
    );
}

#[test]
fn host_metadata_public_bytes_and_required_settings_validate_before_requests() {
    for field in ["text", "sha256"] {
        let mut p = plan();
        if field == "text" {
            p.public_task.text.push('x')
        } else {
            p.public_task.reference.sha256 = "0".repeat(64)
        };
        assert!(MethodSynthesisSession::new(p).is_err());
    }
    let mut p = plan();
    p.entrypoint = "../escape.py".into();
    assert!(MethodSynthesisSession::new(p).is_err());
    let mut p = serde_json::to_value(plan()).unwrap();
    p["settings"].as_object_mut().unwrap().remove("sampling");
    assert!(serde_json::from_value::<MethodSynthesisPlan>(p).is_err());
    let mut p = plan();
    p.settings.max_tokens = 0;
    assert!(MethodSynthesisSession::new(p).is_err());
}

// Simulate a checkpoint written by a previous prompt builder under the same
// output contract. The checksum is integrity, not authentication of this writer.
fn reseal_record(value: &mut Value) {
    use genesis_forge::text_generation::{TextCompletion, TextGenerationRequest};
    let plan: MethodSynthesisPlan = serde_json::from_value(value["plan"].clone()).unwrap();
    let request: TextGenerationRequest = serde_json::from_value(value["request"].clone()).unwrap();
    let phase: MethodSynthesisPhase = serde_json::from_value(value["phase"].clone()).unwrap();
    let completion: Option<TextCompletion> =
        serde_json::from_value(value["completion"].clone()).unwrap();
    value["checkpoint_sha256"] = json!(hash(
        &serde_json::to_vec(&(
            "genesis-method-generation-checkpoint/1",
            1u32,
            plan,
            request,
            value["request_sha256"].as_str().unwrap(),
            value["output_contract"].as_str().unwrap(),
            phase,
            completion,
        ))
        .unwrap()
    ));
}

#[tokio::test]
async fn restored_intent_keeps_original_prompt_wire_and_identity() {
    use genesis_forge::text_generation::TextGenerationRequest;
    let mut saved = serde_json::to_value(MethodSynthesisSession::new(plan()).unwrap()).unwrap();
    saved["request"]["system"] = json!("Historical source-only prompt under contract v1.");
    assert!(serde_json::from_value::<MethodSynthesisSession>(saved.clone()).is_err());
    let original: TextGenerationRequest = serde_json::from_value(saved["request"].clone()).unwrap();
    saved["request_sha256"] = json!(original.sha256().unwrap());
    reseal_record(&mut saved);
    let mut prepared: MethodSynthesisSession = serde_json::from_value(saved.clone()).unwrap();
    assert_ne!(original, prepared.plan().request().unwrap());
    assert_eq!(prepared.request_sha256(), original.sha256().unwrap());
    assert_eq!(prepared.begin().unwrap(), original);
    let pending = serde_json::to_value(&prepared).unwrap();
    assert_eq!(pending["request"], saved["request"]);
    assert_eq!(pending["request_sha256"], saved["request_sha256"]);
    let mut restored: MethodSynthesisSession = serde_json::from_value(pending).unwrap();
    restored.recover_interrupted().unwrap();
    assert_eq!(
        restored.completion().unwrap().request_sha256,
        original.sha256().unwrap()
    );
    assert_eq!(
        restored.completion().unwrap().wire_sha256,
        hash(&original.wire_bytes().unwrap())
    );
    assert!(restored.begin().is_err());
    let mut completed: MethodSynthesisSession = serde_json::from_value(saved.clone()).unwrap();
    finish(&mut completed, response(r#"{"source":"print(1)"}"#)).await;
    let completed: MethodSynthesisSession =
        serde_json::from_value(serde_json::to_value(completed).unwrap()).unwrap();
    assert_eq!(completed.package().unwrap().files()[0].content, "print(1)");
    for mutation in ["user", "settings", "identity", "contract"] {
        let mut changed = saved.clone();
        match mutation {
            "user" => changed["request"]["user"] = json!("{}"),
            "settings" => changed["request"]["settings"]["max_tokens"] = json!(256),
            "identity" => changed["request_sha256"] = json!("0".repeat(64)),
            _ => changed["output_contract"] = json!("unknown-contract/2"),
        }
        if matches!(mutation, "user" | "settings") {
            let request: TextGenerationRequest =
                serde_json::from_value(changed["request"].clone()).unwrap();
            changed["request_sha256"] = json!(request.sha256().unwrap());
        }
        reseal_record(&mut changed);
        assert!(serde_json::from_value::<MethodSynthesisSession>(changed).is_err());
    }
}
