use genesis_forge::{
    method_revision::{MethodRevisionInput, MAX_METHOD_REVISION_INPUT_BYTES},
    method_synthesis::PublicDocument,
};
use serde_json::json;
use sha2::{Digest, Sha256};
#[path = "common/method_revision_fixture.rs"]
mod fixtures;
use fixtures::*;

fn input(publisher: &str) -> MethodRevisionInput {
    let (mut spec, bytes) = fixture();
    let text = json!({"publisher": publisher, "parent": spec.parent}).to_string();
    let mut reference = spec.generation.public_task.reference.clone();
    reference.id = "selected-public-origin".into();
    reference.sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));
    spec.generation
        .public_inputs
        .push(PublicDocument { reference, text });
    MethodRevisionInput::new(spec, String::from_utf8(bytes).unwrap()).unwrap()
}

#[test]
fn bundle_roundtrip_preserves_existing_request_and_observation_identities() {
    let original = input("org-a");
    let restored: MethodRevisionInput =
        serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    let mut session = restored.prepare().unwrap();
    let direct = original
        .spec
        .prepare(original.parent_package_json.as_bytes())
        .unwrap();
    assert_eq!(session.checkpoint_sha256(), direct.checkpoint_sha256());
    assert_eq!(session.request_sha256(), direct.request_sha256());
    assert_eq!(session.plan(), direct.plan());
    assert!(session.completion().is_none());
    complete(&mut session, SOURCE);
    let observed = restored.observe(&session).unwrap();
    let direct = original
        .spec
        .observe(original.parent_package_json.as_bytes(), &session)
        .unwrap();
    assert_eq!(
        serde_json::to_value(observed).unwrap(),
        serde_json::to_value(direct).unwrap()
    );
    assert_eq!(original, restored);
    // A different valid origin must not relabel this saved request.
    let other = input("org-other");
    assert_ne!(
        other.prepare().unwrap().request_sha256(),
        session.request_sha256()
    );
    assert!(other.observe(&session).is_err());
}

#[test]
fn malformed_or_substituted_bundles_fail_existing_source_and_plan_checks() {
    let original = input("org-a");
    for kind in [
        "schema",
        "noncanonical",
        "source",
        "identity",
        "runtime",
        "large-parent",
        "large-input",
        "input-count",
    ] {
        let mut changed = original.clone();
        match kind {
            "schema" => changed.schema = "genesis-method-revision-input/2".into(),
            "noncanonical" => changed.parent_package_json.push('\n'),
            "source" => {
                changed.parent_package_json = changed
                    .parent_package_json
                    .replace("MUST_NOT_EXECUTE", "OTHER_SOURCE")
            }
            "identity" => changed.spec.parent.version = "other".into(),
            "runtime" => changed.spec.generation.runtime.sha256 = "b".repeat(64),
            "large-parent" => changed.parent_package_json = "x".repeat(128 * 1024 + 1),
            "large-input" => {
                changed.spec.generation.instruction = "x".repeat(MAX_METHOD_REVISION_INPUT_BYTES)
            }
            _ => {
                changed.spec.generation.public_inputs =
                    vec![changed.spec.generation.public_task.clone(); 16]
            }
        }
        assert!(changed.validate().is_err(), "{kind}");
        assert!(changed.prepare().is_err(), "{kind}");
    }
    let mut value = serde_json::to_value(original).unwrap();
    value["authority"] = json!("adopted");
    assert!(serde_json::from_value::<MethodRevisionInput>(value).is_err());
}

#[test]
fn pending_and_foreign_sessions_cannot_be_observed_through_a_bundle() {
    let input = input("org-a");
    let mut pending = input.prepare().unwrap();
    pending.begin().unwrap();
    let unchanged = serde_json::to_vec(&pending).unwrap();
    assert!(input.observe(&pending).is_err());
    assert_eq!(serde_json::to_vec(&pending).unwrap(), unchanged);
    pending.recover_interrupted().unwrap();
    assert!(input.observe(&pending).is_err());
    let mut foreign = input.clone();
    foreign.spec.generation.org_id = "another-receiver".into();
    let mut session = foreign.prepare().unwrap();
    complete(&mut session, SOURCE);
    assert!(input.observe(&session).is_err());
}
