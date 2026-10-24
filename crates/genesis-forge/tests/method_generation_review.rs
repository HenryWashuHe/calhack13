//! Henry's Claude: independent regressions for method-generation state guards
//! that a mutation run found untested (the current code is correct on each).
use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use genesis_forge::text_generation::{TextCompletion, TextFailure, TextOutcome};
use genesis_forge::AnthropicGenerator;
use serde_json::{json, Value};
#[path = "common/method_plan.rs"]
mod plan_fixture;
use plan_fixture::{hash, plan};
use std::time::Duration;
#[path = "common/text_generation_fixture.rs"]
mod fixture;
use fixture::{response, serve};

/// A genuine loopback completion for `plan()`'s request, from a separate session.
async fn returned(source: &str) -> TextCompletion {
    let mut s = MethodSynthesisSession::new(plan()).unwrap();
    let request = s.begin().unwrap();
    let body = response(&json!({ "source": source }).to_string());
    let (endpoint, task) = serve(serde_json::to_vec(&body).unwrap(), 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    task.await.unwrap();
    completion
}

fn bytes(s: &MethodSynthesisSession) -> Vec<u8> {
    serde_json::to_vec(s).unwrap()
}

#[tokio::test]
async fn a_completion_needs_a_persisted_pending_intent() {
    let completion = returned("print(1)").await;
    // Prepared: begin() was never called, so no intent was persisted before a
    // POST. A completion arriving now has no request this checkpoint sent.
    let mut prepared = MethodSynthesisSession::new(plan()).unwrap();
    let before = bytes(&prepared);
    assert!(prepared.complete(completion).is_err());
    assert_eq!(bytes(&prepared), before);
    assert_eq!(prepared.phase(), MethodSynthesisPhase::Prepared);
}

#[tokio::test]
async fn a_settled_outcome_is_never_replaced_by_a_later_completion() {
    let late = returned("print('late')").await;

    // Unknown (interrupted) stays unknown: a later Returned completion for the
    // same request can't turn it into generated source.
    let mut unknown = MethodSynthesisSession::new(plan()).unwrap();
    unknown.begin().unwrap();
    assert!(unknown.recover_interrupted().unwrap());
    let before = bytes(&unknown);
    assert!(unknown.complete(late.clone()).is_err());
    assert_eq!(bytes(&unknown), before);
    assert_eq!(unknown.phase(), MethodSynthesisPhase::NeedsReconciliation);
    assert!(unknown.package().is_err());

    // Generated stays exactly the first source.
    let mut generated = MethodSynthesisSession::new(plan()).unwrap();
    generated.begin().unwrap();
    generated
        .complete(returned("print('first')").await)
        .unwrap();
    let before = bytes(&generated);
    assert!(generated.complete(late.clone()).is_err());
    assert_eq!(bytes(&generated), before);
    assert_eq!(
        generated.package().unwrap().files()[0].content,
        "print('first')"
    );

    // A rejected source isn't silently repaired by a second completion.
    let mut rejected = MethodSynthesisSession::new(plan()).unwrap();
    rejected.begin().unwrap();
    rejected.complete(returned("").await).unwrap();
    assert_eq!(rejected.phase(), MethodSynthesisPhase::RejectedSource);
    assert!(rejected.complete(late).is_err());
    assert_eq!(rejected.phase(), MethodSynthesisPhase::RejectedSource);
}

// Same reseal as method_generation.rs: a consistent checksum written by some
// earlier (buggy) writer. The checksum is integrity, not authentication.
fn reseal(value: &mut Value) {
    use genesis_forge::text_generation::TextGenerationRequest;
    let plan: genesis_forge::method_synthesis::MethodSynthesisPlan =
        serde_json::from_value(value["plan"].clone()).unwrap();
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
async fn a_resealed_checkpoint_whose_phase_contradicts_its_completion_is_refused() {
    let mut unknown = MethodSynthesisSession::new(plan()).unwrap();
    unknown.begin().unwrap();
    unknown.recover_interrupted().unwrap();
    assert_eq!(
        unknown.completion().unwrap().outcome,
        TextOutcome::Unknown {
            reason: TextFailure::Interrupted
        }
    );
    let mut generated = MethodSynthesisSession::new(plan()).unwrap();
    generated.begin().unwrap();
    generated.complete(returned("print(1)").await).unwrap();

    for (session, phase) in [
        // An unknown outcome recorded as a definite failure would read as
        // settled rather than needing reconciliation.
        (&unknown, "failed"),
        (&unknown, "generated"),
        (&unknown, "requesting"),
        // Valid source recorded as rejected, or as still pending.
        (&generated, "rejected_source"),
        (&generated, "requesting"),
    ] {
        let mut value = serde_json::to_value(session).unwrap();
        value["phase"] = json!(phase);
        reseal(&mut value);
        assert!(
            serde_json::from_value::<MethodSynthesisSession>(value).is_err(),
            "{phase} accepted over {:?}",
            session.phase()
        );
    }
    // Control: resealing without a change still loads.
    let mut value = serde_json::to_value(&generated).unwrap();
    reseal(&mut value);
    assert!(serde_json::from_value::<MethodSynthesisSession>(value).is_ok());
}

#[tokio::test]
async fn a_response_with_more_than_one_content_block_is_not_source() {
    let mut s = MethodSynthesisSession::new(plan()).unwrap();
    let request = s.begin().unwrap();
    let mut body = response(&json!({ "source": "print(1)" }).to_string());
    // A second text block: neither block alone is the response's answer.
    let extra = body["content"][0].clone();
    body["content"].as_array_mut().unwrap().push(extra);
    let (endpoint, task) = serve(serde_json::to_vec(&body).unwrap(), 200, Duration::ZERO).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    task.await.unwrap();
    assert!(
        !matches!(completion.outcome, TextOutcome::Returned { .. }),
        "{:?}",
        completion.outcome
    );
    s.complete(completion).unwrap();
    assert_ne!(s.phase(), MethodSynthesisPhase::Generated);
    assert!(s.package().is_err());
}

#[test]
fn a_resealed_unknown_usage_reason_must_still_be_a_real_reason() {
    let mut s = MethodSynthesisSession::new(plan()).unwrap();
    s.begin().unwrap();
    s.recover_interrupted().unwrap();
    for reason in [json!(""), json!("   "), json!("x".repeat(257))] {
        let mut value = serde_json::to_value(&s).unwrap();
        assert_eq!(
            value["completion"]["usage"]["model_calls"]["kind"],
            "unknown"
        );
        value["completion"]["usage"]["model_calls"]["reason"] = reason.clone();
        reseal(&mut value);
        assert!(
            serde_json::from_value::<MethodSynthesisSession>(value).is_err(),
            "accepted unknown reason {reason}"
        );
    }
}
