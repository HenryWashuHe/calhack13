use genesis_core::development_method::*;
use genesis_core::development_package::*;
use genesis_core::digest::canonical_json;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn reference(id: &str) -> ExactContentRef {
    ExactContentRef {
        domain: "fixture".into(),
        schema: "fixture/1".into(),
        id: id.into(),
        version: "v1".into(),
        sha256: "a".repeat(64),
    }
}

fn file(path: &str, content: &str) -> MethodSourceFile {
    MethodSourceFile {
        path: path.into(),
        content: content.into(),
    }
}

fn package(files: Vec<MethodSourceFile>) -> Result<MethodPackage, ContractError> {
    MethodPackage::new(
        "search".into(),
        "v1".into(),
        reference("runtime"),
        reference("protocol"),
        "src/main.txt".into(),
        vec![reference("parts"), reference("helpers")],
        files,
    )
}

fn fixture() -> MethodPackage {
    package(vec![
        file("src/main.txt", "seed control\n"),
        file("README.txt", "not executed"),
    ])
    .unwrap()
}

fn binding(package: &MethodPackage, org: &str) -> DevelopmentRunBinding {
    DevelopmentRunBinding {
        org_id: org.into(),
        run_id: format!("{org}-run"),
        attempt_id: format!("{org}-attempt").into(),
        method: package.reference().unwrap(),
        dependencies: package.dependencies().to_vec(),
        public_task: reference("public-task"),
        evaluator: reference("evaluator"),
        model_settings: reference("fixed-model"),
        environment: reference("environment"),
        purpose: ExecutionPurpose::Ordinary {},
        budget: ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 1000,
            simulation_steps: 200,
        },
    }
}

fn load(bytes: &[u8], expected: &DevelopmentRunBinding) -> Result<MethodPackage, ContractError> {
    MethodPackage::load_for(
        bytes,
        expected,
        &reference("runtime"),
        &reference("protocol"),
    )
}

fn repin_wire(wire: &Value, expected: &DevelopmentRunBinding) -> (String, DevelopmentRunBinding) {
    let bytes = canonical_json(wire);
    let mut expected = expected.clone();
    expected.method.sha256 = format!("{:x}", Sha256::digest(bytes.as_bytes()));
    (bytes, expected)
}

#[test]
fn independent_organizations_load_exact_source_without_sharing_authority() {
    let published = fixture();
    let bytes = published.artifact_json().unwrap();
    let org_a = binding(&published, "org-a");
    let org_b = binding(&published, "org-b");
    assert_ne!(
        org_a.canonical_hash().unwrap(),
        org_b.canonical_hash().unwrap()
    );
    for org in [&org_a, &org_b] {
        let loaded = load(bytes.as_bytes(), org).unwrap();
        assert_eq!(loaded, published);
        assert_eq!(loaded.entrypoint(), "src/main.txt");
        assert_eq!(loaded.files()[1].content.as_bytes(), b"seed control\n");
        assert_eq!(loaded.runtime(), &reference("runtime"));
        assert_eq!(loaded.protocol(), &reference("protocol"));
    }
    // Only loading is exercised; no acceptance, execution or benefit is claimed.
}

#[test]
fn a_revision_cannot_replace_an_organizations_frozen_method() {
    let old = fixture();
    let org_a = binding(&old, "org-a");
    let revision = package(vec![file("src/main.txt", "different search procedure\n")]).unwrap();
    assert_ne!(old.reference().unwrap(), revision.reference().unwrap());
    let revised_bytes = revision.artifact_json().unwrap();
    assert_eq!(
        load(revised_bytes.as_bytes(), &org_a),
        Err(ContractError::IdentityMismatch)
    );
    // A host must explicitly change its selected exact method; this is not adoption policy.
    let mut new_run = org_a.clone();
    new_run.method = revision.reference().unwrap();
    assert!(load(revised_bytes.as_bytes(), &new_run).is_ok());
    assert!(load(old.artifact_json().unwrap().as_bytes(), &new_run).is_err());
}

#[test]
fn input_order_is_normalized_but_source_bytes_and_metadata_are_frozen() {
    let files = vec![file("src/main.txt", "é\n"), file("a.txt", "helper")];
    let original = package(files.clone()).unwrap();
    let mut reversed = files;
    reversed.reverse();
    assert_eq!(
        original.reference().unwrap(),
        package(reversed).unwrap().reference().unwrap()
    );
    let expected = binding(&original, "a");
    let original_wire: Value = serde_json::from_str(&original.artifact_json().unwrap()).unwrap();
    for (pointer, replacement) in [
        ("/package/files/1/content", json!("é\r\n")),
        ("/package/files/1/content", json!("e\u{301}\n")),
        ("/package/entrypoint", json!("a.txt")),
        ("/package/runtime/sha256", json!("b".repeat(64))),
        ("/package/protocol/version", json!("v2")),
        ("/package/dependencies/0/sha256", json!("b".repeat(64))),
    ] {
        let mut changed = original_wire.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            load(canonical_json(&changed).as_bytes(), &expected),
            Err(ContractError::IdentityMismatch),
            "{pointer}"
        );
    }
}

#[test]
fn repinning_source_does_not_override_trusted_runtime_protocol_or_dependencies() {
    let original = fixture();
    let expected = binding(&original, "a");
    let wire: Value = serde_json::from_str(&original.artifact_json().unwrap()).unwrap();
    for pointer in [
        "/package/runtime/id",
        "/package/protocol/id",
        "/package/dependencies/0/id",
        "/package/id",
        "/package/version",
    ] {
        let mut changed = wire.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("substituted");
        let (bytes, repinned) = repin_wire(&changed, &expected);
        assert_eq!(
            load(bytes.as_bytes(), &repinned),
            Err(ContractError::IdentityMismatch),
            "{pointer}"
        );
    }
    for dependencies in [vec![], vec![reference("helpers"), reference("parts")]] {
        let mut changed = expected.clone();
        changed.dependencies = dependencies;
        assert!(load(original.artifact_json().unwrap().as_bytes(), &changed).is_err());
    }
}

#[test]
fn rejects_missing_entrypoints_unsafe_paths_and_case_or_directory_aliases() {
    for path in [
        "",
        "/absolute",
        "../outside",
        "a/../b",
        "a//b",
        "a/./b",
        "a/",
        "a\\b",
        "a:b",
        "x\0y",
    ] {
        assert!(
            package(vec![file("src/main.txt", "ok"), file(path, "bad")]).is_err(),
            "{path:?}"
        );
    }
    for path in [
        "src",
        "SRC",
        "src/main.txt",
        "Src/Main.txt",
        "src/main.txt/child",
    ] {
        assert!(
            package(vec![file("src/main.txt", "ok"), file(path, "bad")]).is_err(),
            "{path}"
        );
    }
    assert!(package(vec![file("other.txt", "missing entrypoint")]).is_err());
    assert!(package(vec![file("Src/main.txt", "wrong case")]).is_err());
    assert!(package(vec![]).is_err());
}

#[test]
fn wire_loader_rejects_malformed_noncanonical_unknown_and_invalid_rehashed_data() {
    let original = fixture();
    let expected = binding(&original, "a");
    let wire: Value = serde_json::from_str(&original.artifact_json().unwrap()).unwrap();
    for (pointer, replacement) in [
        ("/domain", json!("another-format")),
        ("/version", json!(2)),
        ("/package/entrypoint", json!("absent")),
        ("/package/files/0/path", json!("../outside")),
        (
            "/package/dependencies/1",
            wire["package"]["dependencies"][0].clone(),
        ),
    ] {
        let mut changed = wire.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        let (bytes, repinned) = repin_wire(&changed, &expected);
        assert!(load(bytes.as_bytes(), &repinned).is_err(), "{pointer}");
    }
    let mut unknown = wire.clone();
    unknown["extra"] = json!(true);
    let (bytes, repinned) = repin_wire(&unknown, &expected);
    assert!(load(bytes.as_bytes(), &repinned).is_err());
    for bytes in [
        b"not json".to_vec(),
        vec![255],
        serde_json::to_vec_pretty(&wire).unwrap(),
    ] {
        let mut repinned = expected.clone();
        repinned.method.sha256 = format!("{:x}", Sha256::digest(&bytes));
        assert!(load(&bytes, &repinned).is_err());
    }
    let mut unknown = wire.clone();
    unknown["package"]["files"][0]["mode"] = json!("executable");
    let (bytes, repinned) = repin_wire(&unknown, &expected);
    assert!(load(bytes.as_bytes(), &repinned).is_err());
    let mut reordered = wire;
    reordered["package"]["files"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let (bytes, repinned) = repin_wire(&reordered, &expected);
    assert!(load(bytes.as_bytes(), &repinned).is_err());
}

#[test]
fn bounds_apply_before_loading_and_to_source_and_encoded_package() {
    let expected = binding(&fixture(), "a");
    assert!(load(&vec![b' '; MAX_PACKAGE_BYTES + 1], &expected).is_err());
    assert!(package(vec![file("src/main.txt", &"x".repeat(2 * 1024 * 1024 + 1))]).is_err());
    assert!(package(vec![
        file("src/main.txt", &"x".repeat(2 * 1024 * 1024)),
        file("a", &"x".repeat(2 * 1024 * 1024)),
        file("b", "x")
    ])
    .is_err());
    // Escapes can exceed the wire bound even with permitted source byte counts.
    assert!(package(vec![file("src/main.txt", &"\0".repeat(2 * 1024 * 1024))]).is_err());
    let mut files = vec![file("src/main.txt", "ok")];
    files.extend((0..256).map(|n| file(&format!("file{n}"), "")));
    assert!(package(files).is_err());
}
