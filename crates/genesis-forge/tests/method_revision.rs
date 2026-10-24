use genesis_forge::method_synthesis::MethodSynthesisSession;
use genesis_forge::text_generation::GenerationTransport;
#[path = "common/method_revision_fixture.rs"]
mod fixtures;
use fixtures::*;

#[test]
fn exact_parent_enters_frozen_request_and_relabels_do_not_fake_source_changes() {
    let (spec, bytes) = fixture();
    let mut session = spec.prepare(&bytes).unwrap();
    let input = session.plan().public_inputs.last().unwrap();
    assert_eq!(input.reference, spec.parent);
    assert_eq!(input.text.as_bytes(), bytes);
    let restored: MethodSynthesisSession =
        serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
    assert_eq!(restored.plan(), session.plan());
    assert!(spec.observe(&bytes, &session).is_err());
    complete(&mut session, SOURCE);
    let report = spec.observe(&bytes, &session).unwrap();
    assert!(!report.source_changed);
    assert_ne!(report.child, report.parent);
    assert_eq!(report.parent_source_sha256, report.child_source_sha256);
    assert_eq!(report.org_id, "org_b");
    assert_eq!(report.transport, GenerationTransport::LoopbackFixture);
    assert_eq!(report.request_sha256, session.request_sha256());
    assert_eq!(report.checkpoint_sha256, session.checkpoint_sha256());
    assert_eq!(report.revision_spec_sha256, spec.sha256().unwrap());
}

#[test]
fn changed_source_is_observed_without_claiming_behavioral_improvement() {
    let (spec, bytes) = fixture();
    let mut session = spec.prepare(&bytes).unwrap();
    complete(
        &mut session,
        &format!("{SOURCE}\n# a byte-level change only"),
    );
    let report = spec.observe(&bytes, &session).unwrap();
    assert!(report.source_changed);
    assert_ne!(report.parent_source_sha256, report.child_source_sha256);
    assert_eq!(
        report.child,
        session.package().unwrap().reference().unwrap()
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("MUST_NOT_EXECUTE"));
}

#[test]
fn parent_substitution_and_host_contract_mismatch_fail_before_generation() {
    let (spec, bytes) = fixture();
    let mut noncanonical = bytes.clone();
    noncanonical.push(b'\n');
    assert!(spec.prepare(&noncanonical).is_err());
    for changed in ["version", "runtime", "protocol", "dependencies", "target"] {
        let mut wrong = spec.clone();
        match changed {
            "version" => wrong.parent.version = "renamed".into(),
            "runtime" => wrong.generation.runtime.sha256 = "b".repeat(64),
            "protocol" => {
                wrong.generation.protocol = wrong.generation.public_task.clone();
            }
            "dependencies" => wrong
                .generation
                .dependencies
                .push(wrong.generation.runtime.clone()),
            _ => {
                wrong.generation.target_id = wrong.parent.id.clone();
                wrong.generation.target_version = wrong.parent.version.clone();
            }
        }
        assert!(wrong.prepare(&bytes).is_err(), "{changed}");
    }
    let mut duplicate = spec.clone();
    duplicate
        .generation
        .public_inputs
        .push(genesis_forge::method_synthesis::PublicDocument {
            reference: spec.parent.clone(),
            text: String::from_utf8(bytes.clone()).unwrap(),
        });
    assert!(duplicate.prepare(&bytes).is_err());
}

#[test]
fn foreign_or_nonrevision_generation_and_uncertain_attempts_cannot_supply_derivation() {
    let (spec, bytes) = fixture();
    let mut unrelated = MethodSynthesisSession::new(spec.generation.clone()).unwrap();
    complete(&mut unrelated, "print(1)");
    assert!(spec.observe(&bytes, &unrelated).is_err());
    let mut foreign = spec.clone();
    foreign.generation.org_id = "org_other".into();
    let mut foreign_session = foreign.prepare(&bytes).unwrap();
    complete(&mut foreign_session, "print(1)");
    assert!(spec.observe(&bytes, &foreign_session).is_err());
    let mut unknown = spec.prepare(&bytes).unwrap();
    unknown.begin().unwrap();
    unknown.recover_interrupted().unwrap();
    assert!(spec.observe(&bytes, &unknown).is_err());
    assert!(unknown.completion().is_some());
}

#[test]
fn parent_inclusion_cannot_bypass_existing_input_count_or_size_limits() {
    let (mut spec, bytes) = fixture();
    spec.generation.public_inputs = vec![spec.generation.public_task.clone(); 16];
    assert!(spec.generation.validate().is_ok());
    assert!(spec.prepare(&bytes).is_err());
    let (mut spec, bytes) = fixture();
    spec.generation.instruction = "x".repeat(16 * 1024);
    assert!(spec.generation.validate().is_ok());
    assert!(spec.prepare(&bytes).is_err());
}

#[test]
fn a_different_valid_parent_cannot_relabel_an_existing_generation() {
    use genesis_core::development_package::{MethodPackage, MethodSourceFile};
    let (spec, bytes) = fixture();
    let mut session = spec.prepare(&bytes).unwrap();
    complete(&mut session, "print(2)");
    let replacement = MethodPackage::new(
        spec.parent.id.clone(),
        spec.parent.version.clone(),
        spec.generation.runtime.clone(),
        spec.generation.protocol.reference.clone(),
        "parent.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "parent.py".into(),
            content: "print('other parent')".into(),
        }],
    )
    .unwrap();
    let mut other = spec.clone();
    other.parent = replacement.reference().unwrap();
    let replacement_bytes = replacement.artifact_json().unwrap().into_bytes();
    assert!(other.prepare(&replacement_bytes).is_ok());
    assert!(other.observe(&replacement_bytes, &session).is_err());
}

#[test]
fn valid_multifile_and_large_packages_are_outside_revision_v1_bounds() {
    use genesis_core::development_package::{MethodPackage, MethodSourceFile};
    let (spec, _) = fixture();
    for files in [
        vec![
            MethodSourceFile {
                path: "parent.py".into(),
                content: SOURCE.into(),
            },
            MethodSourceFile {
                path: "helper.py".into(),
                content: "pass".into(),
            },
        ],
        vec![MethodSourceFile {
            path: "parent.py".into(),
            content: "x".repeat(128 * 1024),
        }],
    ] {
        let parent = MethodPackage::new(
            spec.parent.id.clone(),
            spec.parent.version.clone(),
            spec.generation.runtime.clone(),
            spec.generation.protocol.reference.clone(),
            "parent.py".into(),
            vec![],
            files,
        )
        .unwrap();
        let mut bounded = spec.clone();
        bounded.parent = parent.reference().unwrap();
        assert!(bounded
            .prepare(parent.artifact_json().unwrap().as_bytes())
            .is_err());
    }
}
