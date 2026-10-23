//! Henry's Claude: the `$REQUIREMENT` placeholder rule in model output parsing.
use genesis_planner::output::{extract_json, parse, OutputError, Verdict};
use serde_json::json;

fn change(requirement: serde_json::Value, operations: serde_json::Value) -> String {
    json!({"assessment":"a","verdict":"change","requirement":requirement,"rationale":"r",
        "operations":operations})
    .to_string()
}

fn assign_from_requirement() -> serde_json::Value {
    json!({"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"w",
        "requirement_id":"$REQUIREMENT","purpose":"p","trigger":"t","termination":"e"},
        "owner":{"kind":"existing","role_id":"role_lead"}})
}

#[test]
fn placeholder_without_a_requirement_is_incomplete() {
    let text = change(json!(null), json!([assign_from_requirement()]));
    assert_eq!(
        parse(&text),
        Err(OutputError::Incomplete(
            "operations use $REQUIREMENT without a requirement"
        ))
    );
    // The same operations with a stated requirement are fine.
    let text = change(
        json!({"summary":"s","acceptance":"x"}),
        json!([assign_from_requirement()]),
    );
    assert_eq!(parse(&text).unwrap().verdict, Verdict::Change);
}

#[test]
fn change_without_the_placeholder_needs_no_requirement() {
    let text = change(
        json!(null),
        json!([{"op":"AttachCapability","role":{"kind":"existing","role_id":"role_lead"},
            "capability":{"capability_id":"reader","version":"1.0.0"}}]),
    );
    let out = parse(&text).unwrap();
    assert_eq!(out.verdict, Verdict::Change);
    assert!(out.requirement.is_none());
}

#[test]
fn placeholder_is_found_only_as_an_exact_string_value() {
    // A string merely containing the placeholder text is not a use of it.
    let text = change(
        json!(null),
        json!([{"op":"AttachCapability","role":{"kind":"existing","role_id":"role_$REQUIREMENT_x"},
            "capability":{"capability_id":"reader","version":"1.0.0"}}]),
    );
    assert!(parse(&text).is_ok());
}

#[test]
fn json_extraction_takes_the_outermost_object() {
    assert_eq!(extract_json("x {\"a\":{}} y"), Some("{\"a\":{}}"));
    assert_eq!(extract_json("{}"), Some("{}"));
    assert_eq!(extract_json("} {"), None);
    assert_eq!(extract_json("no braces"), None);
}
