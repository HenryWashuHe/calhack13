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
        // Compare parsed values, not bytes: key order differs when another
        // crate enables serde_json/preserve_order.
        let committed: serde_json::Value = serde_json::from_str(&committed).unwrap();
        let expected = serde_json::to_value(&schema).unwrap();
        assert_eq!(
            committed, expected,
            "{name} schema drifted from the Rust types; run `cargo run -p genesis-server -- export-schemas`"
        );
    }
}
