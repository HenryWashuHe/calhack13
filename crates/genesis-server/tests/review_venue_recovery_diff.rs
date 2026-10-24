//! Henry's Claude: Rust/Python differential for the Fetch venue replacement
//! contract (`venue_recovery_domain` vs `genesis_fetch_bridge.venue_selection`).
//! Driven by `scripts/review/venue_recovery_diff.py`; skipped unless its
//! environment variables are set.
//!
//! Phase `inputs`: read generated contexts, fill `policy_sha256` with the
//! Rust hash, and emit `provider_input` (or the Rust rejection) per case.
//! Phase `verify`: read the inputs and Python's outputs and run
//! `verify_output` on every case Python answered.
use genesis_server::venue_recovery_domain::{provider_input, verify_output, RecoveryContext};
use genesis_verification::json_sha256;
use serde_json::{json, Value};

fn read(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn venue_recovery_differential() {
    let Ok(phase) = std::env::var("VENUE_DIFF_PHASE") else {
        return;
    };
    let dir = std::env::var("VENUE_DIFF_DIR").unwrap();
    match phase.as_str() {
        "inputs" => {
            let contexts = read(&format!("{dir}/contexts.json"));
            let mut out = Vec::new();
            for mut raw in contexts.as_array().unwrap().iter().cloned() {
                raw["policy_sha256"] = json!(json_sha256(&raw["policy"]));
                let entry = match serde_json::from_value::<RecoveryContext>(raw.clone()) {
                    Err(e) => json!({"rust": "deserialize", "detail": e.to_string()}),
                    Ok(context) => match provider_input(&context) {
                        Ok(input) => json!({"rust": "ok", "context": raw, "input": input}),
                        Err(e) => json!({"rust": format!("{e:?}"), "context": raw}),
                    },
                };
                out.push(entry);
            }
            std::fs::write(
                format!("{dir}/inputs.json"),
                serde_json::to_vec(&out).unwrap(),
            )
            .unwrap();
        }
        "verify" => {
            let inputs = read(&format!("{dir}/inputs.json"));
            let outputs = read(&format!("{dir}/outputs.json"));
            let mut verdicts = Vec::new();
            for (entry, answer) in inputs
                .as_array()
                .unwrap()
                .iter()
                .zip(outputs.as_array().unwrap())
            {
                let verdict = match (entry["rust"].as_str(), answer.get("output")) {
                    (Some("ok"), Some(output)) => {
                        let context: RecoveryContext =
                            serde_json::from_value(entry["context"].clone()).unwrap();
                        match verify_output(&context, output) {
                            Ok(_) => "verified".to_owned(),
                            Err(e) => format!("{e:?}"),
                        }
                    }
                    _ => "n/a".to_owned(),
                };
                verdicts.push(json!(verdict));
            }
            std::fs::write(
                format!("{dir}/verdicts.json"),
                serde_json::to_vec(&verdicts).unwrap(),
            )
            .unwrap();
        }
        other => panic!("unknown phase {other}"),
    }
}
