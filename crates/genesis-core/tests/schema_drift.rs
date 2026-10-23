//! Contract drift check: the committed JSON Schemas in `schemas/` must match
//! the Rust types. Regenerate with `cargo run -p genesis-server -- export-schemas`.

use std::path::PathBuf;

#[test]
fn committed_schemas_match_rust_types() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    for (name, schema) in genesis_core::schema::all() {
        let path = dir.join(format!("{name}.schema.json"));
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing {}; run export-schemas", path.display()));
        let expected = serde_json::to_string_pretty(&schema).unwrap() + "\n";
        assert_eq!(
            committed, expected,
            "{name} schema drifted from the Rust types; run `cargo run -p genesis-server -- export-schemas`"
        );
    }
}
