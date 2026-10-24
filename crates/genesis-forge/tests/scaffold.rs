use genesis_forge::{venue_source, venue_source_for_actor, ForgeStateError};
use serde_json::Value;

const PARSER: &str = "export function extract() { throw new Error('candidate parser'); }\n";

#[test]
fn actor_identity_is_bound_before_hashing_without_changing_other_scaffold_files() {
    let urls = ["https://fixtures.example/table.html".into()];
    let original = venue_source(PARSER, &urls).unwrap();
    let bound = venue_source_for_actor(PARSER, &urls, "genesis-custom-actor", "0.2").unwrap();
    let renamed = venue_source_for_actor(PARSER, &urls, "genesis-another-actor", "0.2").unwrap();
    let versioned = venue_source_for_actor(PARSER, &urls, "genesis-custom-actor", "0.3").unwrap();
    assert_ne!(bound.sha256().unwrap(), original.sha256().unwrap());
    assert_ne!(bound.sha256().unwrap(), renamed.sha256().unwrap());
    assert_ne!(bound.sha256().unwrap(), versioned.sha256().unwrap());
    for (before, after) in original.files().iter().zip(bound.files()) {
        assert_eq!(before.name, after.name);
        if before.name == ".actor/actor.json" {
            assert_eq!(
                before.content,
                include_str!("../../../actors/venue-extractor-template/.actor/actor.json")
            );
            let mut expected: Value = serde_json::from_str(&before.content).unwrap();
            expected["name"] = "genesis-custom-actor".into();
            expected["version"] = "0.2".into();
            assert_eq!(
                serde_json::from_str::<Value>(&after.content).unwrap(),
                expected
            );
        } else {
            assert_eq!(before.content, after.content);
        }
    }
}

#[test]
fn bound_actor_metadata_uses_the_existing_exact_candidate_contract() {
    let urls = ["https://fixtures.example/table.html".into()];
    for (name, version) in [
        ("", "0.1"),
        ("Bad/Name", "0.1"),
        ("-actor", "0.1"),
        ("actor-", "0.1"),
        ("actor", "latest"),
        ("actor", "00.1"),
        ("actor", "1.0.0"),
    ] {
        assert_eq!(
            venue_source_for_actor(PARSER, &urls, name, version).unwrap_err(),
            ForgeStateError::InvalidCandidate
        );
    }
    assert!(venue_source_for_actor(PARSER, &urls, "actor", "99.99").is_ok());
}

#[test]
fn packaged_actor_contains_only_production_files_and_exact_candidate_bytes() {
    let source = venue_source(PARSER, &["https://fixtures.example/table.html".into()]).unwrap();
    let names: Vec<_> = source.files().iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            ".actor/actor.json",
            ".actor/input_schema.json",
            "Dockerfile",
            "allowed-sources.json",
            "package-lock.json",
            "package.json",
            "src/contract.ts",
            "src/extractor.ts",
            "src/main.ts",
            "src/runtime.ts",
            "tsconfig.json"
        ]
    );
    let parser = source
        .files()
        .iter()
        .find(|f| f.name == "src/extractor.ts")
        .unwrap();
    assert_eq!(parser.content, PARSER);
    let lock: Value = serde_json::from_str(
        &source
            .files()
            .iter()
            .find(|f| f.name == "package-lock.json")
            .unwrap()
            .content,
    )
    .unwrap();
    assert_eq!(lock["lockfileVersion"], 3);
    assert!(source
        .files()
        .iter()
        .all(|f| !f.name.contains("test") && !f.name.contains("oracle")));
}

#[test]
fn source_allowlist_is_deduplicated_and_order_does_not_change_package_identity() {
    let a = "https://fixtures.example/a.html".to_string();
    let b = "https://fixtures.example/b.html".to_string();
    let first = venue_source(PARSER, &[b.clone(), a.clone(), b.clone()]).unwrap();
    let second = venue_source(PARSER, &[a.clone(), b.clone()]).unwrap();
    assert_eq!(first.sha256().unwrap(), second.sha256().unwrap());
    let policy: Value = serde_json::from_str(
        &first
            .files()
            .iter()
            .find(|f| f.name == "allowed-sources.json")
            .unwrap()
            .content,
    )
    .unwrap();
    assert_eq!(policy, serde_json::json!({"urls": [a, b]}));
    assert_ne!(
        first.sha256().unwrap(),
        venue_source(PARSER, &["https://fixtures.example/c.html".into()])
            .unwrap()
            .sha256()
            .unwrap()
    );
}

#[test]
fn package_rejects_urls_that_the_production_runtime_cannot_accept() {
    for url in [
        "http://fixtures.example/a",
        "https://user:secret@fixtures.example/a",
        "https://fixtures.example/a#part",
        "https://fixtures.example",
        "https://FIXTURES.example/a",
        "https://fixtures.example/../a",
        "file:///tmp/a",
        "not a url",
    ] {
        assert_eq!(
            venue_source(PARSER, &[url.into()]).unwrap_err(),
            ForgeStateError::InvalidSource,
            "{url}"
        );
    }
    assert!(venue_source(PARSER, &[]).is_err());
    let urls: Vec<_> = (0..100)
        .map(|i| format!("https://fixtures.example/{i}"))
        .collect();
    assert!(venue_source(PARSER, &urls).is_ok());
    let mut too_many = urls;
    too_many.push("https://fixtures.example/overflow".into());
    assert!(venue_source(PARSER, &too_many).is_err());
}

#[test]
fn candidate_parser_must_be_present_and_bounded() {
    let urls = ["https://fixtures.example/a".into()];
    assert!(venue_source(" \n", &urls).is_err());
    assert!(venue_source(&"x".repeat(128 * 1024 + 1), &urls).is_err());
    assert!(venue_source(&"x".repeat(128 * 1024), &urls).is_ok());
}
