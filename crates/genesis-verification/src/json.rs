use serde::de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use std::fmt;

const MAX_BYTES: usize = 8 * 1024 * 1024;

/// A sanitized decoding error. Raw field names and values are never included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonDecodeError {
    pub reason: &'static str,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for JsonDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.reason, self.line, self.column
        )
    }
}
impl std::error::Error for JsonDecodeError {}

/// Decode external JSON before converting it to `Value`. Rejects duplicate keys
/// at any depth, trailing data, excessive recursion and files over 8 MiB. Use at
/// adapter/CLI boundaries; ambiguity cannot be recovered from an existing Value.
pub fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, JsonDecodeError> {
    if bytes.len() > MAX_BYTES {
        return Err(JsonDecodeError {
            reason: "JSON exceeds 8 MiB",
            line: 0,
            column: 0,
        });
    }
    let strict: StrictValue = serde_json::from_slice(bytes).map_err(|e| JsonDecodeError {
        reason: "invalid or ambiguous JSON",
        line: e.line(),
        column: e.column(),
    })?;
    serde_json::from_value(strict.0).map_err(|_| JsonDecodeError {
        reason: "JSON does not match schema",
        line: 0,
        column: 0,
    })
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<StrictValue>()? {
                    items.push(item.0);
                }
                Ok(StrictValue(Value::Array(items)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut object = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if object.contains_key(&key) {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                    object.insert(key, map.next_value::<StrictValue>()?.0);
                }
                Ok(StrictValue(Value::Object(object)))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ambiguity_at_every_depth_including_escaped_keys() {
        for bytes in [
            br#"{"a":1,"a":2}"#.as_slice(),
            br#"{"outer":[{"a":1,"\u0061":2}]}"#,
            br#"{"outer":{"x":{},"x":null}}"#,
        ] {
            assert!(decode_json::<Value>(bytes).is_err());
        }
    }

    #[test]
    fn preserves_json_values_and_integer_precision() {
        let bytes = br#"{"n":null,"b":true,"f":1.25,"i":-10,"u":18446744073709551615,"s":"hello","a":[{},[]]}"#;
        assert_eq!(
            decode_json::<Value>(bytes).unwrap(),
            serde_json::from_slice::<Value>(bytes).unwrap()
        );
    }

    #[test]
    fn error_does_not_disclose_payload() {
        let error =
            decode_json::<Value>(br#"{"private-secret":1,"private-secret":2}"#).unwrap_err();
        assert!(!format!("{error:?} {error}").contains("private-secret"));
    }
}
