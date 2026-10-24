use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_forge::method_revision::MethodRevisionSpec;
use genesis_forge::method_synthesis::MethodSynthesisSession;
use genesis_forge::text_generation::{GenerationTransport, TextCompletion, TextOutcome};
#[path = "method_plan.rs"]
mod plan_fixture;
pub const SOURCE: &str = "raise Exception('source-only: MUST_NOT_EXECUTE')";
pub fn fixture() -> (MethodRevisionSpec, Vec<u8>) {
    let mut generation = plan_fixture::plan();
    let parent = MethodPackage::new(
        "parent".into(),
        "1".into(),
        generation.runtime.clone(),
        generation.protocol.reference.clone(),
        "parent.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "parent.py".into(),
            content: SOURCE.into(),
        }],
    )
    .unwrap();
    generation.org_id = "org_b".into();
    generation.target_version = "2".into();
    (
        MethodRevisionSpec {
            generation,
            parent: parent.reference().unwrap(),
        },
        parent.artifact_json().unwrap().into_bytes(),
    )
}
pub fn complete(session: &mut MethodSynthesisSession, source: &str) {
    let request = session.begin().unwrap();
    let mut completion = TextCompletion::interrupted(&request).unwrap();
    completion.transport = GenerationTransport::LoopbackFixture;
    completion.actual_model = Some(request.settings.model.clone());
    completion.message_id = Some("msg_revision_fixture".into());
    completion.response_sha256 = Some("a".repeat(64));
    completion.usage.model_calls =
        genesis_core::development_method::ResourceUsage::Measured { value: 1 };
    completion.outcome = TextOutcome::Returned {
        text: serde_json::json!({"source":source}).to_string(),
    };
    session.complete(completion).unwrap();
}
