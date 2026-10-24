//! Independent source-transfer boundary probes; no code execution or host I/O.
use genesis_core::development_method::*;
use genesis_core::development_package::*;
use genesis_core::digest::canonical_json;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn reference(id: &str) -> ExactContentRef {
    ExactContentRef {
        domain: "review".into(),
        schema: "review/1".into(),
        id: id.into(),
        version: "v1".into(),
        sha256: "a".repeat(64),
    }
}
fn source(path: &str, content: String) -> MethodSourceFile {
    MethodSourceFile {
        path: path.into(),
        content,
    }
}
fn make(files: Vec<MethodSourceFile>, dependencies: Vec<ExactContentRef>) -> MethodPackage {
    MethodPackage::new(
        "method".into(),
        "v1".into(),
        reference("runtime"),
        reference("protocol"),
        "main.txt".into(),
        dependencies,
        files,
    )
    .unwrap()
}
fn binding(package: &MethodPackage) -> DevelopmentRunBinding {
    DevelopmentRunBinding {
        org_id: "receiver".into(),
        run_id: "independent-run".into(),
        attempt_id: "attempt".into(),
        method: package.reference().unwrap(),
        dependencies: package.dependencies().to_vec(),
        public_task: reference("task"),
        evaluator: reference("evaluator"),
        model_settings: reference("model"),
        environment: reference("environment"),
        purpose: ExecutionPurpose::Ordinary {},
        budget: ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 1,
            simulation_steps: 1,
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
fn repin(bytes: &[u8], expected: &DevelopmentRunBinding) -> DevelopmentRunBinding {
    let mut expected = expected.clone();
    expected.method.sha256 = format!("{:x}", Sha256::digest(bytes));
    expected
}

#[test]
fn review_received_digest_precedes_decode_and_duplicate_fields_never_become_packages() {
    let package = make(vec![source("main.txt", "control text".into())], vec![]);
    let expected = binding(&package);
    for malformed in [b"not json".as_slice(), b"\xff".as_slice()] {
        assert_eq!(
            load(malformed, &expected),
            Err(ContractError::IdentityMismatch)
        );
        assert_eq!(
            load(malformed, &repin(malformed, &expected)),
            Err(ContractError::Invalid("method package encoding"))
        );
    }
    let valid = package.artifact_json().unwrap();
    let duplicate_envelope = valid.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    let duplicate_source = valid.replacen(
        "\"content\":\"control text\"",
        "\"content\":\"control text\",\"content\":\"changed\"",
        1,
    );
    let duplicate_reference=valid.replacen("\"sha256\":", "\"sha256\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",\"sha256\":",1);
    for duplicate in [duplicate_envelope, duplicate_source, duplicate_reference] {
        assert_ne!(duplicate, valid);
        assert_eq!(
            load(
                duplicate.as_bytes(),
                &repin(duplicate.as_bytes(), &expected)
            ),
            Err(ContractError::Invalid("method package encoding"))
        );
    }
    assert!(load(valid.as_bytes(), &expected).is_ok());
}

#[test]
fn review_exact_source_limits_accept_and_a_rehashed_alias_or_dependency_overflow_rejects() {
    let mut files = vec![
        source("main.txt", "x".repeat(2 * 1024 * 1024)),
        source("helper.txt", "y".repeat(2 * 1024 * 1024)),
    ];
    files.extend((0..254).map(|n| source(&format!("f{n:03}.txt"), String::new())));
    let dependencies = (0..128)
        .map(|n| reference(&format!("dependency-{n}")))
        .collect();
    let package = make(files, dependencies);
    let expected = binding(&package);
    let bytes = package.artifact_json().unwrap();
    let restored = load(bytes.as_bytes(), &expected).unwrap();
    assert_eq!(restored.files().len(), 256);
    assert_eq!(restored.dependencies().len(), 128);
    assert_eq!(
        restored
            .files()
            .iter()
            .map(|f| f.content.len())
            .sum::<usize>(),
        4 * 1024 * 1024
    );
    let wire: Value = serde_json::from_str(&bytes).unwrap();

    let mut overflow = wire.clone();
    let extra = reference("one-too-many");
    overflow["package"]["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(&extra).unwrap());
    let bytes = canonical_json(&overflow);
    let mut expected_overflow = repin(bytes.as_bytes(), &expected);
    expected_overflow.dependencies.push(extra);
    assert_eq!(
        load(bytes.as_bytes(), &expected_overflow),
        Err(ContractError::Invalid("method dependency count"))
    );

    let mut alias = wire;
    alias["package"]["files"][0]["path"] = Value::String("MAIN.TXT".into());
    let files = alias["package"]["files"].as_array_mut().unwrap();
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let bytes = canonical_json(&alias);
    assert_eq!(
        load(bytes.as_bytes(), &repin(bytes.as_bytes(), &expected)),
        Err(ContractError::Invalid("method source file"))
    );
}
