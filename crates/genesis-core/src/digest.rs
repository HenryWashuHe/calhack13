//! Stable digests of JSON values.
//!
//! `serde_json::Value::to_string()` follows map iteration order, which changes
//! when another crate in the build enables `serde_json/preserve_order`. Digests
//! used as persisted identities must not depend on that, so they hash a
//! canonical form with object keys sorted recursively.

use serde_json::Value;

/// Serialize with object keys sorted at every level.
pub fn canonical_json(value: &Value) -> String {
    fn write(v: &Value, out: &mut String) {
        match v {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).expect("string serializes"));
                    out.push(':');
                    write(&map[*k], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            scalar => out.push_str(&scalar.to_string()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

/// SHA-256 (hex) of the canonical JSON form.
pub fn canonical_sha256(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(canonical_json(value).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_the_digest() {
        let a: Value =
            serde_json::from_str(r#"{"b":1,"a":{"y":[1,{"d":2,"c":3}],"x":null}}"#).unwrap();
        let b: Value =
            serde_json::from_str(r#"{"a":{"x":null,"y":[1,{"c":3,"d":2}]},"b":1}"#).unwrap();
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(canonical_sha256(&a), canonical_sha256(&b));
        assert_eq!(
            canonical_json(&json!({"b": 1, "a": "x"})),
            r#"{"a":"x","b":1}"#
        );
    }

    #[test]
    fn array_order_still_matters() {
        assert_ne!(
            canonical_sha256(&json!([1, 2])),
            canonical_sha256(&json!([2, 1]))
        );
    }
}
