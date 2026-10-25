//! Independent receiver-input preservation and post-augmentation bound checks.
use super::*;
use genesis_core::digest::{canonical_json, canonical_sha256};
use genesis_forge::method_synthesis::PublicDocument;

fn public_padding(spec: &MethodRevisionSpec, id: &str, count: usize) -> PublicDocument {
    let value = json!({"padding": "x".repeat(count)});
    let mut reference = spec.generation.public_task.reference.clone();
    reference.id = id.into();
    reference.sha256 = canonical_sha256(&value);
    PublicDocument {
        reference,
        text: canonical_json(&value),
    }
}

#[tokio::test]
async fn fourteen_receiver_inputs_survive_exactly_before_selection_and_parent() {
    let (registry, selected, mut spec) = fixture().await;
    spec.generation.public_inputs = (0..14)
        .map(|index| public_padding(&spec, &format!("receiver-input-{index}"), index + 1))
        .collect();
    spec.generation.instruction = "Preserve the receiver's explicit revision objective.".into();
    spec.generation.validate().unwrap();
    let result = cli(&registry, &selected, &spec).await;
    assert!(result.status.success(), "{result:?}");
    assert_eq!(registry.fetches.load(Ordering::SeqCst), 1);
    let input: MethodRevisionInput = serde_json::from_slice(&result.stdout).unwrap();
    let mut retained = input.spec.clone();
    let selection = retained.generation.public_inputs.pop().unwrap();
    assert_eq!(
        retained, spec,
        "only the appended selection may alter the spec"
    );
    assert_eq!(
        selection.reference.domain,
        "genesis.method_registry_selection"
    );
    let metadata: Value = serde_json::from_str(&selection.text).unwrap();
    assert_eq!(metadata["candidate"], json!(selected));
    assert_eq!(selection.reference.sha256, canonical_sha256(&metadata));
    let session = input.prepare().unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Prepared);
    assert!(session.completion().is_none());
    assert_eq!(session.plan().public_inputs.len(), 16);
    assert_eq!(
        &session.plan().public_inputs[..14],
        &spec.generation.public_inputs
    );
    assert_eq!(session.plan().public_inputs[14], selection);
    assert_eq!(session.plan().public_inputs[15].reference, spec.parent);
    assert_eq!(
        session.plan().public_inputs[15].text,
        input.parent_package_json
    );
}

#[tokio::test]
async fn valid_maximum_receiver_plan_is_refused_after_adding_network_selection() {
    let (registry, selected, mut spec) = fixture().await;
    spec.generation.public_inputs = vec![
        public_padding(&spec, "receiver-large-first", 128 * 1024 - 32),
        public_padding(&spec, "receiver-large-second", 0),
    ];
    // Find the exact valid encoded-plan boundary without weakening document hashes.
    let (mut low, mut high) = (0usize, 128 * 1024usize);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        spec.generation.public_inputs[1] = public_padding(&spec, "receiver-large-second", middle);
        if spec.generation.validate().is_ok() {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    spec.generation.public_inputs[1] = public_padding(&spec, "receiver-large-second", low);
    spec.generation.validate().unwrap();
    spec.sha256().unwrap();
    assert_eq!(
        serde_json::to_vec(&spec.generation).unwrap().len(),
        256 * 1024
    );
    let result = cli(&registry, &selected, &spec).await;
    assert_eq!(registry.fetches.load(Ordering::SeqCst), 1);
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("selected parent cannot form a bounded revision input"));
    refused(result); // No successful JSON; sanitized diagnostics, no credential/source echo.
}
